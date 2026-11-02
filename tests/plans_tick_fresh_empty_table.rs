mod common;

use repo_assist::ipc::server::plans_tick_json;
use repo_assist::store::Store;

#[test]
fn plans_tick_fresh_daemon_empty_worktrees_table_ticks_box() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    common::make_git_repo(&wt);

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
    assert!(std::process::Command::new("git")
        .current_dir(&wt)
        .args(["checkout", "-b", "feature"])
        .status()
        .unwrap()
        .success());
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

    let db = dir.path().join("db.sqlite");
    let store = Store::open(&db).unwrap();
    // Genuinely empty worktrees table, no plans synced: supply only repo path.
    let res = plans_tick_json(&store, None, false, Some(&wt)).unwrap();
    let boxes = res.get("total_boxes").and_then(|v| v.as_u64()).unwrap_or(0);
    assert_eq!(boxes, 1, "expected one box, got {boxes}: {res}");
}
