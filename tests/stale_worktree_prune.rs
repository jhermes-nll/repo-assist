//! Task 1 (prune stale worktrees): a previously-seen worktree whose directory no
//! longer exists is marked `present = 0` by `sync_worktrees` (the same mark-gone
//! convention plans use with `state = 'gone'`), and every aggregate — `plans`,
//! `status`, and the `stalled` computations — skips it.

mod common;

use common::GitRepo;
use repo_assist::checks::{self, CheckCtx};
use repo_assist::config::Config;
use repo_assist::ipc::server::{
    conflicts_json, plan_drift_value, plans_json, sessions_json, status_json, who_touches_json,
};
use repo_assist::sources::git::sync_worktrees;
use repo_assist::store::{
    CheckWorkspace, PlanInfo, PlanRow, SessionRow, Store, TaskInfo, TaskRow, WtInfo,
};
use std::collections::HashMap;
use std::path::Path;

/// `(id, present)` for the worktree row at `path` (git may canonicalize
/// `/var/...` to `/private/var/...`, so match either form).
fn wt_row(store: &Store, path: &Path) -> (i64, i64) {
    let p = path.to_str().expect("utf8 path").to_string();
    let p_private = format!("/private{p}");
    store
        .read(|conn| {
            conn.query_row(
                "SELECT id, present FROM worktrees WHERE path = ?1 OR path = ?2",
                [p.as_str(), p_private.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .expect("worktree row")
}

/// An active superpowers plan with one open task (stalled-eligible).
fn upsert_plan_with_open_task(store: &Store, id: &str, worktree_id: i64) {
    store
        .upsert_plan(
            &PlanRow {
                id,
                source: "superpowers",
                title: id,
                file: Some("docs/superpowers/plans/x.md"),
                worktree_id: Some(worktree_id),
                content_hash: None,
                state: "active",
            },
            1_000,
        )
        .unwrap();
    store
        .upsert_task(&TaskRow {
            id: &format!("{id}:1"),
            plan_id: id,
            ordinal: 1,
            title: "t",
            declared: "open",
            expected_paths: &[],
            depends_on: &[],
        })
        .unwrap();
}

/// (a) a plan row whose worktree dir was deleted is excluded from plans/stalled,
///     and (b) the live worktree still reports correctly.
#[test]
fn plans_json_excludes_deleted_worktree_and_keeps_live_one() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let wt = repo.paths[0].clone();

    let tmp = tempfile::TempDir::new().unwrap();
    let store = Store::open(tmp.path().join("db.sqlite")).expect("open");
    sync_worktrees(&store, repo.path()).expect("sync");

    let (main_id, main_present) = wt_row(&store, repo.path());
    let (gone_id, gone_present) = wt_row(&store, &wt);
    assert_eq!((main_present, gone_present), (1, 1));

    upsert_plan_with_open_task(&store, "live-plan", main_id);
    upsert_plan_with_open_task(&store, "gone-plan", gone_id);

    // Baseline: both rows are reported while the worktree exists.
    let before = plans_json(&store, &Config::default_config()).expect("plans_json");
    assert_eq!(before["plans"].as_array().unwrap().len(), 2);

    // Delete the worktree directory. The git admin entry survives, so discovery
    // still sees the path — sync must go by directory existence.
    std::fs::remove_dir_all(&wt).expect("remove worktree dir");
    sync_worktrees(&store, repo.path()).expect("sync after delete");
    assert_eq!(wt_row(&store, &wt).1, 0, "deleted dir must be present=0");

    let v = plans_json(&store, &Config::default_config()).expect("plans_json");
    let rows = v["plans"].as_array().expect("plans array");
    assert_eq!(rows.len(), 1, "gone worktree's plan must be excluded: {v}");
    let live = &rows[0];
    assert_eq!(live["id"], "live-plan");
    // Live worktree still reports: present, mapped to its stored path, stalled
    // computed (no recorded activity for an active plan with open tasks).
    let main_path: String = store
        .read(|conn| {
            conn.query_row("SELECT path FROM worktrees WHERE id = ?1", [main_id], |r| {
                r.get(0)
            })
        })
        .expect("main path");
    assert_eq!(live["worktree"].as_str(), Some(main_path.as_str()));
    assert_eq!(live["stalled"].as_bool(), Some(true));
}

// (d) pruning is idempotent: repeated syncs keep the gone flag, delete no rows,
// and produce stable plans_json output.
#[test]
fn pruning_is_idempotent() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let wt = repo.paths[0].clone();

    let tmp = tempfile::TempDir::new().unwrap();
    let store = Store::open(tmp.path().join("db.sqlite")).expect("open");
    sync_worktrees(&store, repo.path()).expect("sync");
    let (gone_id, _) = wt_row(&store, &wt);
    upsert_plan_with_open_task(&store, "gone-plan", gone_id);
    upsert_plan_with_open_task(&store, "live-plan", wt_row(&store, repo.path()).0);

    std::fs::remove_dir_all(&wt).expect("remove worktree dir");

    let mut last: Option<serde_json::Value> = None;
    for _ in 0..3 {
        sync_worktrees(&store, repo.path()).expect("sync");
        assert_eq!(
            wt_row(&store, &wt).1,
            0,
            "gone worktree must stay present=0"
        );
        let plans: i64 = store
            .read(|conn| conn.query_row("SELECT COUNT(*) FROM plans", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(plans, 2, "pruning must not delete plan rows");
        let v = plans_json(&store, &Config::default_config()).expect("plans_json");
        assert_eq!(v["plans"].as_array().unwrap().len(), 1);
        if let Some(prev) = &last {
            assert_eq!(prev, &v, "plans_json must be stable across runs");
        }
        last = Some(v);
    }
}

fn wt_info(id: i64, path: &str, present: bool) -> WtInfo {
    WtInfo {
        id,
        path: path.to_string(),
        branch: None,
        head: None,
        base_ref: None,
        merge_base: None,
        is_main: id == 1,
        present,
        changed: vec![],
    }
}

fn plan(id: &str, worktree_id: i64) -> PlanInfo {
    PlanInfo {
        id: id.to_string(),
        source: "superpowers".to_string(),
        title: id.to_string(),
        worktree_id: Some(worktree_id),
        state: "active".to_string(),
    }
}

fn open_task(plan_id: &str) -> TaskInfo {
    TaskInfo {
        id: format!("{plan_id}:1"),
        plan_id: plan_id.to_string(),
        ordinal: 1,
        title: "t".to_string(),
        declared: "open".to_string(),
        expected_paths: vec![],
        depends_on: vec![],
        observed: "unknown".to_string(),
        observed_reason: None,
    }
}

/// (a) the stalled finding engine skips plans on a gone worktree;
///     (b) a live worktree's plan still fires.
#[test]
fn plan_stalled_skips_gone_worktree_but_fires_on_live() {
    let cfg = Config::default_config();
    let ws = CheckWorkspace {
        worktrees: vec![wt_info(1, "/gone-wt", false), wt_info(2, "/live-wt", true)],
        plans: vec![plan("p-gone", 1), plan("p-live", 2)],
        tasks: vec![open_task("p-gone"), open_task("p-live")],
        sessions: vec![],
        activity: HashMap::new(),
    };
    let ctx = CheckCtx {
        workspace: &ws,
        config: &cfg,
        now: 0,
    };
    let findings = checks::plan_stalled::check_plan_stalled(&ctx);
    assert!(
        findings
            .iter()
            .any(|f| f.dedupe_key == "plan.stalled:p-live"),
        "live worktree plan must still fire: {findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.dedupe_key == "plan.stalled:p-gone"),
        "gone worktree plan must not fire: {findings:?}"
    );
}

/// (c) characterization: the existing gone-plan convention (a `state = 'gone'`
/// plan row in a still-present worktree) keeps being reported as before.
#[test]
fn gone_plan_rows_in_present_worktrees_are_unchanged() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = Store::open(tmp.path().join("db.sqlite")).expect("open");
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path, branch, is_main, present, updated_ms) \
                 VALUES ('/wt-live', 'main', 1, 1, 0)",
                [],
            )?;
            Ok(())
        })
        .expect("insert worktree");
    let live_id = wt_row(&store, Path::new("/wt-live")).0;
    upsert_plan_with_open_task(&store, "active-plan", live_id);
    store
        .upsert_plan(
            &PlanRow {
                id: "gone-plan",
                source: "superpowers",
                title: "gone",
                file: Some("docs/superpowers/plans/gone.md"),
                worktree_id: Some(live_id),
                content_hash: None,
                state: "gone",
            },
            1_000,
        )
        .unwrap();

    let v = plans_json(&store, &Config::default_config()).expect("plans_json");
    let rows = v["plans"].as_array().expect("plans array");
    let gone = rows
        .iter()
        .find(|p| p["id"] == "gone-plan")
        .expect("gone-state plan in a present worktree must still be listed");
    assert_eq!(gone["state"], "gone");
    assert_eq!(gone["stalled"].as_bool(), Some(false));
    assert_eq!(rows.len(), 2);
}

/// `status` counts must not include gone worktrees or their plans.
#[test]
fn status_json_excludes_gone_worktrees_from_counts() {
    use repo_assist::ipc::server::status_json;

    let tmp = tempfile::TempDir::new().unwrap();
    let store = Store::open(tmp.path().join("db.sqlite")).expect("open");
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path, branch, is_main, present, updated_ms) \
                 VALUES ('/wt-live', 'main', 1, 1, 0), ('/wt-gone', 'x', 0, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .expect("insert worktrees");
    let live_id = wt_row(&store, Path::new("/wt-live")).0;
    let gone_id = wt_row(&store, Path::new("/wt-gone")).0;
    upsert_plan_with_open_task(&store, "live-plan", live_id);
    upsert_plan_with_open_task(&store, "gone-plan", gone_id);

    let v = status_json(&store, "repo-key").expect("status_json");
    assert_eq!(
        v["worktree_count"].as_i64(),
        Some(1),
        "worktree_count must count present worktrees only: {v}"
    );
    assert_eq!(
        v["worktrees"].as_array().map(|a| a.len()),
        Some(1),
        "worktrees list must not include gone worktrees: {v}"
    );
    assert_eq!(
        v["active_plans"].as_i64(),
        Some(1),
        "active_plans must exclude plans of gone worktrees: {v}"
    );
}

/// A task whose single expected path is `expected` (overlap/drift eligible).
fn expecting_task(plan_id: &str, expected: &str) -> TaskInfo {
    TaskInfo {
        id: format!("{plan_id}:1"),
        plan_id: plan_id.to_string(),
        ordinal: 1,
        title: "t".to_string(),
        declared: "open".to_string(),
        expected_paths: vec![expected.to_string()],
        depends_on: vec![],
        observed: "unknown".to_string(),
        observed_reason: None,
    }
}

/// Store with one present (`/wt-live`) and one gone (`/wt-gone`) worktree.
fn two_worktree_store() -> (tempfile::TempDir, Store, i64, i64) {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = Store::open(tmp.path().join("db.sqlite")).expect("open");
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path, branch, is_main, present, updated_ms) \
                 VALUES ('/wt-live', 'main', 1, 1, 0), ('/wt-gone', 'x', 0, 0, 0)",
                [],
            )?;
            Ok(())
        })
        .expect("insert worktrees");
    let live = wt_row(&store, Path::new("/wt-live")).0;
    let gone = wt_row(&store, Path::new("/wt-gone")).0;
    (tmp, store, live, gone)
}

/// An active plan in `worktree_id` whose single task expects `expected`.
fn upsert_plan_expecting(store: &Store, id: &str, worktree_id: i64, expected: &str) {
    store
        .upsert_plan(
            &PlanRow {
                id,
                source: "superpowers",
                title: id,
                file: Some("docs/superpowers/plans/x.md"),
                worktree_id: Some(worktree_id),
                content_hash: None,
                state: "active",
            },
            1_000,
        )
        .unwrap();
    store
        .upsert_task(&TaskRow {
            id: &format!("{id}:1"),
            plan_id: id,
            ordinal: 1,
            title: "t",
            declared: "open",
            expected_paths: &[expected.to_string()],
            depends_on: &[],
        })
        .unwrap();
}

fn insert_session(store: &Store, id: &str, worktree_id: Option<i64>) {
    // Distinct file per session: upsert_session_with_calls keeps only the newest
    // id per file (same-file cleanup at store/mod.rs).
    let file = format!("{id}.jsonl");
    let row = SessionRow {
        id,
        agent: "agent",
        file: &file,
        cwd: None,
        worktree_id,
        started_ms: Some(0),
        last_event_ms: Some(0),
        tokens_in: 0,
        tokens_out: 0,
        tokens_cache: 0,
        cost_usd: None,
        tool_calls: 0,
        byte_offset: 0,
        parse_errors: 0,
    };
    store.upsert_session_with_calls(&row, &[]).unwrap();
}

/// MUST-FIX: `check_plan_overlap` skips plans on gone worktrees but still fires
/// for two live worktrees.
#[test]
fn plan_overlap_skips_gone_worktree_but_fires_on_live() {
    let cfg = Config::default_config();
    let ws = CheckWorkspace {
        worktrees: vec![
            wt_info(1, "/gone-wt", false),
            wt_info(2, "/live-wt-a", true),
            wt_info(3, "/live-wt-b", true),
        ],
        plans: vec![plan("p-gone", 1), plan("p-live-a", 2), plan("p-live-b", 3)],
        tasks: vec![
            expecting_task("p-gone", "src/shared.rs"),
            expecting_task("p-live-a", "src/shared.rs"),
            expecting_task("p-live-b", "src/shared.rs"),
        ],
        sessions: vec![],
        activity: HashMap::new(),
    };
    let ctx = CheckCtx {
        workspace: &ws,
        config: &cfg,
        now: 0,
    };
    let findings = checks::plan_overlap::check_plan_overlap(&ctx);
    let keys: Vec<&str> = findings.iter().map(|f| f.dedupe_key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["plan.overlap:p-live-a:p-live-b"],
        "only the live/live pair must fire (pre-fix this also fires gone/live pairs)"
    );
}

/// Same-class: `ra who-touches` must not report plans or worktrees of a gone worktree.
#[test]
fn who_touches_excludes_gone_worktree() {
    let (_tmp, store, live_id, gone_id) = two_worktree_store();
    upsert_plan_expecting(&store, "live-touch", live_id, "src/shared.rs");
    upsert_plan_expecting(&store, "gone-touch", gone_id, "src/shared.rs");
    insert_session(&store, "s-gone", Some(gone_id));

    let v = who_touches_json(&store, "src/shared.rs").expect("who_touches_json");
    let plans = v["plans"].as_array().expect("plans array");
    assert_eq!(plans.len(), 1, "only the live plan may touch: {v}");
    assert_eq!(plans[0]["id"], "live-touch");
    let wts = v["worktrees"].as_array().expect("worktrees array");
    assert!(
        wts.iter().all(|w| w["path"] != "/wt-gone"),
        "gone worktree must not be listed: {v}"
    );
    assert!(
        !v.to_string().contains("s-gone"),
        "session of a gone worktree must not touch: {v}"
    );
}

/// Same-class: the sessions read path and the status count skip gone-worktree sessions.
#[test]
fn sessions_json_excludes_gone_worktree_sessions() {
    let (_tmp, store, live_id, gone_id) = two_worktree_store();
    insert_session(&store, "s-live", Some(live_id));
    insert_session(&store, "s-gone", Some(gone_id));
    insert_session(&store, "s-null", None);

    let v = sessions_json(&store).expect("sessions_json");
    let ids: Vec<&str> = v["sessions"]
        .as_array()
        .expect("sessions array")
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert_eq!(ids.len(), 2, "gone-worktree session must be excluded: {v}");
    assert!(ids.contains(&"s-live") && ids.contains(&"s-null"));

    // The status count must agree with the sessions list (status is the reference).
    let st = status_json(&store, "repo-key").expect("status_json");
    assert_eq!(
        st["sessions"].as_i64(),
        Some(2),
        "status sessions count must match the sessions list: {st}"
    );
}

/// Cheap future-guard: one fixture, every read surface — a gone worktree's path,
/// plan ids, and session ids must appear nowhere.
///
/// `worktrees_json` (`Method::Worktrees`) is deliberately exempt: it is the
/// inventory endpoint that carries the `present` flag (`ra worktrees` renders `!`).
/// A fully generic "loop every IPC method" test would need the async, stream-bound
/// request dispatcher refactored into a plain function — a refactor this task
/// forbids — so this enumerates today's read surfaces instead; a *new* read path
/// must be added here when it is written.
#[test]
fn no_read_surface_reports_a_gone_worktree() {
    let (_tmp, store, live_id, gone_id) = two_worktree_store();
    upsert_plan_expecting(&store, "live-plan", live_id, "src/shared.rs");
    upsert_plan_expecting(&store, "gone-plan", gone_id, "src/shared.rs");
    insert_session(&store, "s-live", Some(live_id));
    insert_session(&store, "s-gone", Some(gone_id));
    insert_session(&store, "s-null", None);

    let cfg = Config::default_config();
    let surfaces = vec![
        (
            "plans",
            serde_json::to_string(&plans_json(&store, &cfg).expect("plans")).unwrap(),
        ),
        (
            "status",
            serde_json::to_string(&status_json(&store, "k").expect("status")).unwrap(),
        ),
        (
            "sessions",
            serde_json::to_string(&sessions_json(&store).expect("sessions")).unwrap(),
        ),
        (
            "who_touches",
            serde_json::to_string(&who_touches_json(&store, "src/shared.rs").expect("who_touches"))
                .unwrap(),
        ),
        (
            "conflicts",
            serde_json::to_string(&conflicts_json(&store, &cfg).expect("conflicts")).unwrap(),
        ),
        (
            "drift",
            serde_json::to_string(&plan_drift_value(&store).expect("drift")).unwrap(),
        ),
    ];
    for (name, body) in &surfaces {
        assert!(
            !body.contains("/wt-gone"),
            "{name} must not report the gone worktree: {body}"
        );
        assert!(
            !body.contains("gone-plan"),
            "{name} must not report the gone plan: {body}"
        );
        assert!(
            !body.contains("s-gone"),
            "{name} must not report the gone session: {body}"
        );
    }

    // Findings engine directly: no candidate may mention the gone worktree or plan.
    let ws = store.check_workspace().expect("check_workspace");
    let ctx = CheckCtx {
        workspace: &ws,
        config: &cfg,
        now: repo_assist::sources::git::now_ms(),
    };
    let dump: String = checks::run_all_checks(&ctx)
        .iter()
        .map(|c| format!("{} {} {}", c.dedupe_key, c.subject, c.summary))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !dump.contains("gone-plan"),
        "findings must not mention the gone plan: {dump}"
    );
    assert!(
        !dump.contains("/wt-gone"),
        "findings must not mention the gone worktree: {dump}"
    );
}
