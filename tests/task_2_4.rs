//! Task 2.4 acceptance: the plans data model (store CRUD for `plans` -> `tasks` -> `evidence`)
//! plus the RPC + CLI wiring so `ra plans` and `ra plan <id>` stop returning "not yet
//! implemented". Tests exercise the store API directly, then drive the CLI over IPC.

mod common;

use std::path::Path;

use repo_assist::store::{EvidenceRow, PlanRow, Store, TaskRow};

/// Open a fresh store at `db_path`.
fn open(db: &Path) -> Store {
    Store::open(db).expect("open a fresh store")
}

/// Persist one plan row.
fn plan(store: &Store, row: &PlanRow<'_>) {
    store
        .upsert_plan(row, repo_assist::sources::git::now_ms())
        .expect("upsert_plan");
}

/// Persist one task row.
fn task(store: &Store, row: &TaskRow<'_>) {
    store.upsert_task(row).expect("upsert_task");
}

/// Append one evidence row; returns the row id.
fn evidence(store: &Store, row: &EvidenceRow<'_>) -> i64 {
    store.append_evidence(row).expect("append_evidence")
}

/// Count the evidence rows for `task_id` (a read-only query against the store file).
fn evidence_count(store: &Store, task_id: &str) -> i64 {
    store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM evidence WHERE task_id = ?1",
                [task_id],
                |r| r.get::<_, i64>(0),
            )
        })
        .expect("count evidence")
}

/// Count the plans rows.
fn plan_count(store: &Store) -> i64 {
    store
        .read(|conn| conn.query_row("SELECT COUNT(*) FROM plans", [], |r| r.get(0)))
        .expect("count plans")
}

/// `upsert_plan` twice with the same `id` but different `title`/`state` leaves a single row,
/// keeping the second call's values and the first call's `first_seen_ms`.
#[test]
fn upsert_plan_is_idempotent_per_id() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = open(&db);

    plan(
        &store,
        &PlanRow {
            id: "plan-a",
            source: "manual",
            title: "first",
            file: None,
            worktree_id: None,
            content_hash: None,
            state: "active",
        },
    );
    let first_seen: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT first_seen_ms FROM plans WHERE id = 'plan-a'",
                [],
                |r| r.get(0),
            )
        })
        .expect("read first_seen_ms");

    // Same id, different title/state. Must NOT create a second row.
    plan(
        &store,
        &PlanRow {
            id: "plan-a",
            source: "manual",
            title: "second",
            file: None,
            worktree_id: None,
            content_hash: None,
            state: "done",
        },
    );
    assert_eq!(
        plan_count(&store),
        1,
        "upsert_plan must not create a second row"
    );

    // Second call's values win; `first_seen_ms` is preserved from the first insert while
    // `last_seen_ms` is refreshed to now_ms (both columns are read back explicitly).
    let (title, state, stored_first, stored_last): (String, String, i64, i64) = store
        .read(|conn| {
            conn.query_row(
                "SELECT title, state, first_seen_ms, last_seen_ms FROM plans WHERE id = 'plan-a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
        })
        .expect("read plan row");
    assert_eq!(title, "second");
    assert_eq!(state, "done");
    assert_eq!(
        stored_first, first_seen,
        "first_seen_ms must be preserved across upserts"
    );
    assert!(
        stored_last >= first_seen,
        "last_seen_ms must be refreshed to now_ms"
    );
}

/// `upsert_task` does not wipe evidence (upsert, not delete+reinsert), and does not reset a
/// non-default `observed` value.
#[test]
fn upsert_task_preserves_evidence_and_observed() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = open(&db);

    // Insert the parent plan row first (the tasks.plan_id FK points at it).
    plan(
        &store,
        &PlanRow {
            id: "plan-a",
            source: "manual",
            title: "A plan",
            file: None,
            worktree_id: None,
            content_hash: None,
            state: "active",
        },
    );
    task(
        &store,
        &TaskRow {
            id: "task-1",
            plan_id: "plan-a",
            ordinal: 1,
            title: "do the thing",
            declared: "done",
            expected_paths: &["a.txt".to_string()],
            depends_on: &[],
        },
    );
    let tid = "task-1";

    // Append one evidence row for the task.
    let before = evidence(
        &store,
        &EvidenceRow {
            task_id: tid,
            kind: "commit",
            reference: "abc123",
            ts_ms: repo_assist::sources::git::now_ms(),
            confidence: "explicit",
        },
    );
    assert_eq!(before, 1);

    // Upsert the same task again: evidence must survive (proves upsert, not delete).
    task(
        &store,
        &TaskRow {
            id: "task-1",
            plan_id: "plan-a",
            ordinal: 1,
            title: "do the thing (edited)",
            declared: "done",
            expected_paths: &["a.txt".to_string()],
            depends_on: &["task-2".to_string()],
        },
    );
    assert_eq!(
        evidence_count(&store, tid),
        1,
        "upsert_task must not delete evidence"
    );

    // Set a non-default `observed` directly (through the guarded writer, which the store
    // exposes for exactly this — reads open a read-only connection and cannot UPDATE), then
    // upsert the task a third time. The third upsert must leave `observed` untouched.
    store
        .write(|conn| conn.execute("UPDATE tasks SET observed = 'done' WHERE id = 'task-1'", []))
        .expect("set observed");
    task(
        &store,
        &TaskRow {
            id: "task-1",
            plan_id: "plan-a",
            ordinal: 1,
            title: "do the thing (edited again)",
            declared: "done",
            expected_paths: &["a.txt".to_string()],
            depends_on: &["task-3".to_string()],
        },
    );
    let observed: String = store
        .read(|conn| {
            conn.query_row("SELECT observed FROM tasks WHERE id = 'task-1'", [], |r| {
                r.get(0)
            })
        })
        .expect("read observed");
    assert_eq!(
        observed, "done",
        "upsert_task must not touch the observed column"
    );
}

/// The daemon answers `Method::Plans` (list) and `Method::Plan` (one plan, by id).
#[test]
fn ra_plans_and_plan_over_ipc() {
    use repo_assist::paths::repo_key_from_git_common_dir;

    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let envs = repo_isolate(repo.path());

    // Path to the built `ra` binary (Cargo sets CARGO_BIN_EXE_ra for integration tests).
    let ra_bin =
        std::env::var("CARGO_BIN_EXE_ra").unwrap_or_else(|_| "target/debug/ra".to_string());

    // Derive the daemon's key exactly as the daemon does (`repo_assist::paths`), then locate
    // the DB path and runtime socket the daemon will bind. On host git 2.43,
    // `git rev-parse --absolute-git-common-dir` is unsupported and returns the flag literally,
    // so `git_common_dir` returns `None`; in that case we fall back to the common dir under
    // the repo root (same value the daemon computes). Either way we seed that one DB file.
    let repo_root = repo.path();
    let key = repo_assist::paths::git_common_dir(repo_root)
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());

    let state = repo.path().join("state");
    let sock_dir = repo.path().join("run");

    // Pre-create + migrate + seed the daemon's DB file, then let the foreground daemon connect
    // to it. `Store::open` auto-migrates to the current schema version.
    let db_path = state.join("repo-assist").join(&key).join("db.sqlite");
    {
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let store = Store::open(&db_path).expect("open + migrate the daemon's DB");
        seed_db(&store);
        store
            .write(|conn| conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)"))
            .expect("checkpoint the seeded DB's WAL");
        drop(store);
    }

    // Spawn the daemon foreground. Keep it alive for the whole test — the client commands
    // below connect to this daemon's socket (`--foreground` does not detach), which avoids
    // the auto-spawn path (which would `current_exe()` the test binary and fail).
    let mut daemon = std::process::Command::new(ra_bin)
        .envs(envs.iter().cloned())
        .current_dir(repo_root)
        .arg("daemon")
        .arg("--foreground")
        .spawn()
        .expect("spawn ra daemon --foreground");
    // Ensure the daemon is up and its socket exists before driving the CLI.
    let sock = sock_dir.join("repo-assist").join(format!("{key}.sock"));
    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(sock.exists(), "daemon did not bind its socket at {sock:?}");

    // `ra plans --json` returns the seeded plan (task_count 2).
    let out = assert_cmd::Command::cargo_bin("ra")
        .expect("cargo_bin")
        .envs(envs.iter().cloned())
        .current_dir(repo_root)
        .arg("plans")
        .arg("--json")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .expect("run ra plans --json");
    assert!(
        out.status.success(),
        "ra plans failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let plans: serde_json::Value = serde_json::from_slice(&out.stdout).expect("plans --json");
    let names: Vec<String> = plans["plans"]
        .as_array()
        .expect("plans is an array")
        .iter()
        .map(|p| p["title"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        names.iter().any(|t| t == "Ship the release"),
        "plans list should include 'Ship the release', got {plans}"
    );
    assert_eq!(
        plans["plans"][0]["task_count"].as_i64(),
        Some(2),
        "task_count should be 2"
    );

    // `ra plan <id> --json` returns tasks (by ordinal) and each task's evidence.
    let out = assert_cmd::Command::cargo_bin("ra")
        .expect("cargo_bin")
        .envs(envs.iter().cloned())
        .current_dir(repo_root)
        .arg("plan")
        .arg("plan-xyz")
        .arg("--json")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .expect("run ra plan --json");
    assert!(
        out.status.success(),
        "ra plan failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let plan: serde_json::Value = serde_json::from_slice(&out.stdout).expect("plan --json");
    assert_eq!(plan["id"].as_str(), Some("plan-xyz"));
    let task_order: Vec<i64> = plan["tasks"]
        .as_array()
        .expect("tasks is an array")
        .iter()
        .map(|t| t["ordinal"].as_i64().unwrap_or(0))
        .collect();
    assert_eq!(task_order, vec![1, 2], "tasks ordered by ordinal");
    assert_eq!(
        plan["tasks"][0]["evidence"]
            .as_array()
            .expect("evidence array")
            .len(),
        2,
        "t-1 should have 2 evidence rows"
    );

    // `ra plan <unknown-id> --json` is a valid rejection: the daemon closes the socket
    // (rc != 0, no JSON). That is correct "not found" behavior, not a crash.
    let out = assert_cmd::Command::cargo_bin("ra")
        .expect("cargo_bin")
        .envs(envs.iter().cloned())
        .current_dir(repo_root)
        .arg("plan")
        .arg("does-not-exist")
        .arg("--json")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .expect("run ra plan <unknown> --json");
    assert!(
        !out.status.success(),
        "an unknown id must be rejected, but `ra plan` succeeded"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        combined.contains("not found") || combined.contains("socket closed"),
        "unknown plan should be rejected, got: {combined}"
    );

    // Clean up the foreground daemon.
    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// Seed one plan, two tasks, and two evidence rows into the opened store.
fn seed_db(store: &Store) {
    plan(
        store,
        &PlanRow {
            id: "plan-xyz",
            source: "manual",
            title: "Ship the release",
            file: None,
            worktree_id: None,
            content_hash: None,
            state: "active",
        },
    );
    task(
        store,
        &TaskRow {
            id: "t-1",
            plan_id: "plan-xyz",
            ordinal: 1,
            title: "bump version",
            declared: "done",
            expected_paths: &["bump.sh".to_string()],
            depends_on: &[],
        },
    );
    task(
        store,
        &TaskRow {
            id: "t-2",
            plan_id: "plan-xyz",
            ordinal: 2,
            title: "tag release",
            declared: "todo",
            expected_paths: &["".to_string()],
            depends_on: &["t-1".to_string()],
        },
    );
    evidence(
        store,
        &EvidenceRow {
            task_id: "t-1",
            kind: "commit",
            reference: "abc123",
            ts_ms: repo_assist::sources::git::now_ms(),
            confidence: "explicit",
        },
    );
    evidence(
        store,
        &EvidenceRow {
            task_id: "t-1",
            kind: "commit",
            reference: "def456",
            ts_ms: repo_assist::sources::git::now_ms(),
            confidence: "explicit",
        },
    );
}

/// Build the env pair used to isolate `ra` (mirrors tests/cli.rs) and resolve the daemon's DB
/// path: `$XDG_STATE_HOME/repo-assist/<key>/db.sqlite`.
fn repo_isolate(repo_path: &std::path::Path) -> Vec<(String, String)> {
    let state = repo_path.join("state");
    let runtime = repo_path.join("run");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    vec![
        ("HOME".into(), state.display().to_string()),
        ("XDG_STATE_HOME".into(), state.display().to_string()),
        ("XDG_RUNTIME_DIR".into(), runtime.display().to_string()),
    ]
}
