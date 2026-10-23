//! Task 2.3 acceptance: the findings-engine lifecycle.
//!
//! Exercises `Store::sync_findings` / `list_findings` / `ack_finding` directly (no daemon),
//! mirroring `tests/task_2_3.rs`. Covers every §9 lifecycle transition — new/refresh/resolve —
//! including the spec's required "ack then resolve then reappear" and severity dedupe on write.

mod common;

use repo_assist::store::{FindingCandidate, FindingsFilter};
use repo_assist::store::{FindingRow, Store};
use serde_json::json;

/// Open an isolated findings-engine database for one test's (unique temp) repo.
fn open_store(repo: &common::GitRepo) -> Store {
    let db = repo.path().join("state").join("findings-engine.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    Store::open(&db).unwrap()
}

/// Build a candidate with a distinct subject/summary so rows stay keyed by `dedupe_key`.
fn cand(kind: &str, key: &str, severity: &str) -> FindingCandidate {
    FindingCandidate::new(
        kind,
        key,
        key,
        key,
        severity,
        json!({ "note": key }),
        "explicit",
    )
}

/// Persist one fresh run of all-checks candidates (a new observation).
fn run_checks(
    store: &Store,
    candidates: Vec<FindingCandidate>,
    now: i64,
) -> Vec<repo_assist::store::FindingDelta> {
    store.sync_findings(candidates, now).unwrap()
}

/// Persist an empty candidate set — no check currently fires -> resolves.
fn resolve(store: &Store, now: i64) -> Vec<repo_assist::store::FindingDelta> {
    store.sync_findings(vec![], now).unwrap()
}

/// The persisted row for one dedupe_key, or None (queries with `all: true` so this
/// helper is neutral to state — tests assert state explicitly).
fn find(store: &Store, key: &str) -> Option<FindingRow> {
    store
        .list_findings(&FindingsFilter {
            all: true,
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .find(|f| f.dedupe_key == key)
}

/// The row id for one dedupe_key, or None (a finding that does not exist yet).
fn id_of(store: &Store, key: &str) -> Option<i64> {
    store
        .read(|conn| repo_assist::store::finding_id(conn, key))
        .unwrap()
}

/// New -> resolved -> reopened as the same row, with first_seen_ms preserved.
#[test]
fn finding_lifecycle_open_resolved_reopen_same_row() {
    let repo = common::GitRepo::new();
    let store = open_store(&repo);
    let key = "worktree.overlap:wt-a:wt-b";

    // open
    let deltas = run_checks(&store, vec![cand("worktree.overlap", key, "warn")], 1_000);
    assert_eq!(deltas.len(), 1);
    assert!(deltas[0].opened, "first sync opens a finding");
    assert_eq!(deltas[0].dedupe_key, key);
    let open_id = id_of(&store, key).unwrap();
    assert_eq!(find(&store, key).unwrap().first_seen_ms, 1_000);

    // resolve (the check stops firing)
    let deltas = resolve(&store, 2_000);
    assert_eq!(deltas.len(), 1);
    assert!(
        !deltas[0].opened,
        "a check that stops firing resolves the finding"
    );

    // reappear -> opened again as the same row
    let deltas = run_checks(&store, vec![cand("worktree.overlap", key, "warn")], 3_000);
    assert_eq!(deltas.len(), 1);
    assert!(
        deltas[0].opened,
        "reappealing a resolved finding reopens it"
    );

    let row = find(&store, key).unwrap();
    assert_eq!(row.state, "open");
    assert_eq!(
        row.first_seen_ms, 1_000,
        "first_seen_ms preserved across reopen"
    );
    assert_eq!(
        id_of(&store, key),
        Some(open_id),
        "reappealed as the same row"
    );
}

/// The spec's exact scenario: ack a finding, then resolve it, then have it reappear -> it must
/// reopen (not stay resolved).
#[test]
fn ack_then_resolve_then_reappear() {
    let repo = common::GitRepo::new();
    let store = open_store(&repo);
    let key = "worktree.overlap:ack:reappear";

    // open
    let deltas = run_checks(&store, vec![cand("worktree.overlap", key, "error")], 1_000);
    assert!(deltas[0].opened);
    let id = id_of(&store, key).unwrap();

    // ack it
    assert!(store.ack_finding(&id.to_string(), 1_500).unwrap());
    assert_eq!(find(&store, key).unwrap().state, "acked");

    // resolve while acked: the resolution is not notified (ack does not survive resolution),
    // but the row still flips to resolved.
    let deltas = resolve(&store, 2_000);
    assert!(
        deltas.is_empty(),
        "an acked finding's resolution emits no delta"
    );
    assert_eq!(find(&store, key).unwrap().state, "resolved");

    // reappear -> opened again, as the same row.
    let deltas = run_checks(&store, vec![cand("worktree.overlap", key, "error")], 3_000);
    assert!(
        deltas[0].opened,
        "a reappealing acked finding reopens (does not stay resolved)"
    );
    let row = find(&store, key).unwrap();
    assert_eq!(row.state, "open");
    assert_eq!(
        id_of(&store, key),
        Some(id),
        "reappealed as the same acked row"
    );
}

/// A finding that resolves and never reappears stays resolved and is not listed by `--open`.
#[test]
fn resolved_finding_stays_resolved_and_hides_from_open_filter() {
    let repo = common::GitRepo::new();
    let store = open_store(&repo);
    let key = "worktree.overlap:never:reappear";

    let deltas = run_checks(&store, vec![cand("worktree.overlap", key, "error")], 1_000);
    assert!(deltas[0].opened);
    resolve(&store, 2_000);

    assert_eq!(find(&store, key).unwrap().state, "resolved");
    // --open excludes resolved findings; --all still surfaces it.
    assert!(store
        .list_findings(&FindingsFilter {
            open: true,
            ..Default::default()
        })
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_findings(&FindingsFilter {
                all: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
}

/// Severity dedupe within a run: several checks may fire the same key; only the most-severe
/// view is written (the engine keeps the most-severe candidate and drops lower-severity ones
/// that collide on `dedupe_key`), and the deltas returned carry only the surviving view.
#[test]
fn sync_findings_keeps_the_most_severe_view_within_a_run() {
    let repo = common::GitRepo::new();
    let store = open_store(&repo);
    let key = "worktree.overlap:severity:dedupe";

    // A single run emits one info + one error candidate for the same key.
    let deltas = run_checks(
        &store,
        vec![
            cand("worktree.overlap", key, "info"),
            cand("worktree.overlap", key, "error"),
        ],
        1_000,
    );
    // Only the most-severe candidate is written; the lower-severity one is deduped away.
    assert_eq!(
        deltas.len(),
        1,
        "only the most-severe view is written per key"
    );
    assert!(deltas[0].opened && deltas[0].severity == "error");
    assert_eq!(find(&store, key).unwrap().severity, "error");
}

/// The bare findings view (neither --open nor --all) must exclude resolved rows.
#[test]
fn bare_findings_excludes_resolved() {
    let repo = common::GitRepo::new();
    let store = open_store(&repo);
    let key = "worktree.overlap:bare:resolved";

    // open then resolve it
    let deltas = run_checks(&store, vec![cand("worktree.overlap", key, "error")], 1_000);
    assert!(deltas[0].opened);
    resolve(&store, 2_000);
    assert_eq!(find(&store, key).unwrap().state, "resolved");

    // default filter (neither --open nor --all) excludes it ...
    assert!(
        store
            .list_findings(&FindingsFilter::default())
            .unwrap()
            .is_empty(),
        "a bare findings view excludes resolved rows"
    );
    // ... while --all still surfaces it.
    assert_eq!(
        store
            .list_findings(&FindingsFilter {
                all: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1,
        "--all still surfaces resolved rows"
    );
}
