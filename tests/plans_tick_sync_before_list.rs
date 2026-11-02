mod common;

use repo_assist::ipc::server::plans_tick_json;
use repo_assist::store::Store;

#[test]
fn plans_tick_syncs_before_list() {
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
        assert!(std::process::Command::new("git")
            .current_dir(&wt)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    // feature branch
    assert!(std::process::Command::new("git")
        .current_dir(&wt)
        .args(["checkout", "-b", "feature"])
        .status()
        .unwrap()
        .success());
    // plan committed first
    std::fs::create_dir_all(wt.join("docs/superpowers/plans")).unwrap();
    std::fs::write(
        wt.join("docs/superpowers/plans/plan.md"),
        "## Task 1\nModify: `src/foo.rs`\n- [ ] do thing\n",
    )
    .unwrap();
    for args in [
        ["add", "docs/superpowers/plans/plan.md"].as_slice(),
        ["commit", "-qm", "plan base"].as_slice(),
    ] {
        assert!(std::process::Command::new("git")
            .current_dir(&wt)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    // later code change
    std::fs::write(wt.join("src/foo.rs"), "changed\n").unwrap();
    for args in [
        ["add", "src/foo.rs"].as_slice(),
        ["commit", "-qm", "code change"].as_slice(),
    ] {
        assert!(std::process::Command::new("git")
            .current_dir(&wt)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    let base_sha = String::from_utf8_lossy(
        &std::process::Command::new("git")
            .current_dir(&wt)
            .args(["rev-parse", "main"])
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    let db = dir.path().join("db.sqlite");
    let store = Store::open(&db).unwrap();
    // Store is populated (worktree present) but plans NOT synced.
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO worktrees(path, branch, is_main, present, updated_ms, base_ref, merge_base) VALUES (?1, 'feature', 0, 1, 0, 'main', ?2)",
                rusqlite::params![wt.to_string_lossy().to_string(), base_sha],
            )?;
            Ok(())
        })
        .unwrap();

    // One plans_tick call must sync first, then read list, then derive/tick.
    let res = plans_tick_json(&store, None, false, Some(&wt)).unwrap();
    let boxes = res.get("total_boxes").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(boxes, 1, "expected one box ticked, got {boxes}: {res}");
}
