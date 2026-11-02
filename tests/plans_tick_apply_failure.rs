mod common;

use repo_assist::ipc::server::plans_tick_json;
use repo_assist::store::{PlanRow, Store, TaskRow};
use std::os::unix::fs::PermissionsExt;

#[test]
fn plans_tick_apply_failure_surfaces_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    common::make_git_repo(&wt);
    std::fs::create_dir_all(wt.join("plans")).unwrap();
    std::fs::write(wt.join("plans/plan.md"), "## Task 1\n- [ ] do thing\n").unwrap();
    // commit the plan so the worktree is clean and apply is attempted
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["add", "plans/plan.md"])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["commit", "-qm", "add plan"])
        .status()
        .unwrap();
    assert!(status.success());

    let db = dir.path().join("db.sqlite");
    let store = Store::open(&db).unwrap();
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path, branch, is_main, present, updated_ms) VALUES (?1, 'main', 1, 1, 0)",
                [wt.to_string_lossy().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
    let wt_id: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT id FROM worktrees WHERE path = ?1",
                [wt.to_string_lossy().to_string()],
                |r| r.get(0),
            )
        })
        .unwrap();
    store
        .upsert_plan(
            &PlanRow {
                id: "p",
                source: "superpowers",
                title: "plan",
                file: Some("plans/plan.md"),
                worktree_id: Some(wt_id),
                content_hash: None,
                state: "active",
            },
            1,
        )
        .unwrap();
    store
        .upsert_task(&TaskRow {
            id: "p/1",
            plan_id: "p",
            ordinal: 1,
            title: "do thing",
            declared: "open",
            expected_paths: &[],
            depends_on: &[],
        })
        .unwrap();
    store
        .set_task_observed("p/1", "committed", None)
        .unwrap();

    // Make the plan's parent directory read-only so apply_atomic cannot create its temp file.
    let plan_dir = wt.join("plans");
    let original_mode = std::fs::metadata(&plan_dir).unwrap().permissions().mode();
    std::fs::set_permissions(&plan_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = plans_tick_json(&store, None, true);

    // Restore before asserting so a failure does not leave a read-only tempdir behind.
    std::fs::set_permissions(&plan_dir, std::fs::Permissions::from_mode(original_mode)).unwrap();

    let result = result.unwrap();
    let plans = result.get("plans").and_then(|v| v.as_array()).unwrap();
    assert_eq!(plans.len(), 1);
    let p = &plans[0];
    assert_eq!(p.get("applied").and_then(|v| v.as_bool()), Some(false));
    let apply_error = p
        .get("apply_error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        !apply_error.is_empty(),
        "expected apply_error to be populated, got {:?}",
        p.get("apply_error")
    );
}
