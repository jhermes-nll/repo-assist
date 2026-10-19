//! Task 3 acceptance: drift detection — files changed in a plan's worktree that no task
//! *expects*. Exercises the store-level drift logic end to end: `sync_worktrees` (populate the
//! worktree table), `plan_worktrees` (plan -> worktree link), `changed_file_set` (live git
//! output), and `plan_drift_from_changed` (Drift = changed - union(tasks.expected_paths)).
//!
//! The test controls exactly one new file (`extra.txt`) on a worktree branch, so the assertions
//! only ever depend on whether that file is (or isn't) in the expected set.

mod common;

use std::path::Path;

use repo_assist::sources::git::{changed_file_set, now_ms, sync_worktrees};
use repo_assist::store::{PlanRow, Store, TaskRow};

fn open(db: &Path) -> Store {
    Store::open(db).expect("open store")
}

/// Query the `worktrees` rows whose path ends with `tail`; return (id, path, branch, base_ref)
/// for the matching one (there is exactly one per worktree).
fn find_worktree(store: &Store, tail: &str) -> (i64, String, Option<String>, Option<String>) {
    let tail_pat = format!("%/{}", tail);
    let found: Option<(i64, String, Option<String>, Option<String>)> = store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, path, branch, base_ref FROM worktrees WHERE path LIKE ?")?;
            let mut found = None;
            for r in stmt.query_map([tail_pat], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })? {
                found = Some(r?);
            }
            Ok(found)
        })
        .expect("map worktree rows");
    found.expect("a worktree row whose path ends with {tail:?}")
}

/// The drifted paths for `plan_id` given the worktree's actual changed set: changed files
/// minus the union of the plan's tasks `expected_paths` (stable, ordered by path).
fn drift_paths(store: &Store, plan_id: &str, changed: &[(String, String)]) -> Vec<String> {
    store
        .plan_drift_from_changed(plan_id, changed)
        .expect("plan_drift_from_changed")
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.path)
        .collect()
}

/// A plan whose worktree has one clearly-controlled changed file (`extra.txt`) drifts against
/// every plan task that does *not* expect it, and reports nothing once the task expects it.
#[test]
fn a_worktree_change_drifts_when_unexpected() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");

    // A new, tracked file committed onto the worktree branch — the one thing this test owns.
    let wt = repo.paths[0].clone();
    std::fs::write(wt.join("extra.txt"), "drifted\n").expect("write extra.txt");
    git(&wt, &["add", "extra.txt"]);
    git(&wt, &["commit", "-qm", "add extra"]);

    let db = repo
        .path()
        .join("state")
        .join("repo-assist")
        .join("manual")
        .join("db.sqlite");
    std::fs::create_dir_all(db.parent().expect("parent")).expect("create dirs");
    let store = open(&db);

    // Populate the worktrees table the way the daemon's event bus does.
    sync_worktrees(&store, repo.path()).expect("sync_worktrees");

    // Link a plan to the wt-a worktree via its real (id, path, branch, base_ref).
    let (wt_id, path, branch, base_ref) = find_worktree(&store, "wt-a");

    plan(
        &store,
        &PlanRow {
            id: "plan-drift",
            source: "manual",
            title: "Drift plan",
            file: None,
            worktree_id: Some(wt_id),
            content_hash: None,
            state: "active",
        },
    );
    task(
        &store,
        &TaskRow {
            id: "t-extra",
            plan_id: "plan-drift",
            ordinal: 1,
            title: "expect extra",
            declared: "done",
            expected_paths: &[],
            depends_on: &[],
        },
    );

    // Baseline: what does the worktree actually report as changed — and does it include our file?
    let changed = changed_file_set(Path::new(&path), branch.as_deref(), base_ref.as_deref());
    assert!(
        changed.iter().any(|(p, _)| p == "extra.txt"),
        "baseline changed set must contain 'extra.txt': {changed:?}"
    );

    // First, with the task expecting nothing relevant, `extra.txt` must show as drift.
    let drift = drift_paths(&store, "plan-drift", &changed);
    assert!(
        drift.iter().any(|p| p == "extra.txt"),
        "extra.txt must drift when no task expects it: {drift:?}"
    );

    // Now the task expects exactly `extra.txt`: it must drop out of the drift set.
    store
        .write(|conn| {
            conn.execute(
                "UPDATE tasks SET expected_paths = ?1 WHERE id = 't-extra'",
                ["[\"extra.txt\"]"],
            )?;
            // Cross-process WAL visibility (same quirk as the Task 2.4 IPC test): the read
            // connection opened by `plan_drift_from_changed` won't see an uncheckpointed WAL
            // page, so checkpoint explicitly before the follow-up read.
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").ok();
            Result::<(), _>::Ok(())
        })
        .expect("set expected_paths");

    let drift = drift_paths(&store, "plan-drift", &changed);
    assert!(
        !drift.iter().any(|p| p == "extra.txt"),
        "extra.txt must NOT drift once a task expects it: {drift:?}"
    );
}

/// A plan that owns no worktree produces nothing (it is skipped, never an error).
#[test]
fn plan_without_worktree_reports_nothing() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db = dir.path().join("db.sqlite");
    std::fs::create_dir_all(db.parent().expect("parent")).expect("create dirs");
    let store = open(&db);

    plan(
        &store,
        &PlanRow {
            id: "plan-no-wt",
            source: "manual",
            title: "No worktree plan",
            file: None,
            worktree_id: None,
            content_hash: None,
            state: "active",
        },
    );
    let drift = store
        .plan_drift_from_changed("plan-no-wt", &[])
        .expect("plan_drift_from_changed")
        .unwrap_or_default();
    assert!(
        drift.is_empty(),
        "a plan with no worktree drifts against nothing, got {drift:?}"
    );
}

/// A missing plan id resolves to `None` (not an error), matching the daemon's plan lookup.
#[test]
fn unknown_plan_returns_none() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let db = dir.path().join("db.sqlite");
    std::fs::create_dir_all(db.parent().expect("parent")).expect("create dirs");
    let store = open(&db);
    match store.plan_drift_from_changed("does-not-exist", &[]) {
        Ok(None) => {}
        other => panic!("unknown plan must resolve to Ok(None), got {other:?}"),
    }
}

/// Seed one plan + one task (expected_paths).
fn plan(store: &Store, row: &PlanRow<'_>) {
    store.upsert_plan(row, now_ms()).expect("upsert_plan");
}
/// Seed one task.
fn task(store: &Store, row: &TaskRow<'_>) {
    store.upsert_task(row).expect("upsert_task");
}

/// Git command helper (mirrors tests/common.rs `run`).
fn git(wt: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(wt)
        .args(args)
        .status()
        .expect("git executed");
    assert!(status.success(), "git {args:?} failed in {wt:?}");
}
