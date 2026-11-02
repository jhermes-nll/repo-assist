mod common;

use repo_assist::ipc::server::plans_tick_json;
use repo_assist::store::Store;

#[test]
fn plans_tick_barrier_expires_on_incomplete_startup() {
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
    // Simulate startup having inserted the worktree row but NOT completed the scan: no mark.
    let db = dir.path().join("db.sqlite");
    let store = Store::open(&db).unwrap();
    let e = plans_tick_json(&store, None, false, Some(&wt))
        .expect_err("must fail when barrier not complete");
    let msg = e.to_string();
    assert!(
        msg.contains("syncing") || msg.contains("retry"),
        "msg={msg}"
    );
}
