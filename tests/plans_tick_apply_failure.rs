mod common;

use repo_assist::ipc::server::plans_tick_json;
use repo_assist::store::Store;
use std::os::unix::fs::PermissionsExt;

#[test]
fn plans_tick_apply_failure_surfaces_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    common::make_git_repo(&wt);
    // base commit on main
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join("src/foo.rs"), "base\n").unwrap();
    for args in [
        ["add", "src/foo.rs"].as_slice(),
        ["commit", "-qm", "base"].as_slice(),
    ] {
        let status = std::process::Command::new("git")
            .current_dir(&wt)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    // feature branch
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["checkout", "-b", "feature"])
        .status()
        .unwrap();
    assert!(status.success());
    // plan committed first
    std::fs::create_dir_all(wt.join("docs/superpowers/plans")).unwrap();
    std::fs::write(
        wt.join("docs/superpowers/plans/plan.md"),
        "## Task 1\nModify: `src/foo.rs`\n- [ ] do thing\n",
    )
    .unwrap();
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["add", "docs/superpowers/plans/plan.md"])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["commit", "-qm", "plan base"])
        .status()
        .unwrap();
    assert!(status.success());
    // later code change
    std::fs::write(wt.join("src/foo.rs"), "changed\n").unwrap();
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["add", "src/foo.rs"])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["commit", "-qm", "code change"])
        .status()
        .unwrap();
    assert!(status.success());

    let base_sha = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["rev-parse", "main"])
        .output()
        .unwrap()
        .stdout;
    let base_sha = String::from_utf8_lossy(&base_sha).trim().to_string();

    let db = dir.path().join("db.sqlite");
    let store = Store::open(&db).unwrap();
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path, branch, is_main, present, updated_ms, base_ref, merge_base) VALUES (?1, 'feature', 0, 1, 0, 'main', ?2)",
                rusqlite::params![wt.to_string_lossy().to_string(), base_sha],
            )?;
            Ok(())
        })
        .unwrap();

    let plan_dir = wt.join("docs/superpowers/plans");
    let original_mode = std::fs::metadata(&plan_dir).unwrap().permissions().mode();
    std::fs::set_permissions(&plan_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = plans_tick_json(&store, None, true);

    std::fs::set_permissions(&plan_dir, std::fs::Permissions::from_mode(original_mode)).unwrap();

    let result = result.unwrap();
    let plans = result.get("plans").and_then(|v| v.as_array()).unwrap();
    assert_eq!(plans.len(), 1);
    let p = &plans[0];
    assert_eq!(p.get("applied").and_then(|v| v.as_bool()), Some(false));
    let apply_error = p.get("apply_error").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        !apply_error.is_empty(),
        "expected apply_error to be populated, got {:?}",
        p.get("apply_error")
    );
}
