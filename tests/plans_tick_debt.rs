mod common;

use repo_assist::daemon::startup_scan;
use repo_assist::ipc::server::plans_tick_json;
use repo_assist::plans::parse_plan;
use repo_assist::store::Store;

fn git(wt: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(wt)
        .args(args)
        .status()
        .unwrap()
        .success());
}

/// Committed task whose plan text has no checkbox: derived box count is zero.
#[test]
fn tick_fails_loudly_when_committed_tasks_derive_zero_boxes() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    common::make_git_repo(&wt);
    std::fs::write(wt.join("src/foo.rs"), "base\n").unwrap();
    git(&wt, &["add", "src/foo.rs"]);
    git(&wt, &["commit", "-qm", "base"]);
    git(&wt, &["checkout", "-qb", "feature"]);
    std::fs::create_dir_all(wt.join("docs/superpowers/plans")).unwrap();
    std::fs::write(
        wt.join("docs/superpowers/plans/plan.md"),
        "## Task 1\nModify: `src/foo.rs`\nno checkbox here\n",
    )
    .unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "plan"]);
    std::fs::write(wt.join("src/foo.rs"), "changed\n").unwrap();
    git(&wt, &["commit", "-qam", "code"]);

    let store = Store::open(dir.path().join("db.sqlite")).unwrap();
    store.mark_startup_scan_complete();
    let res = plans_tick_json(&store, None, false, Some(&wt));
    assert!(res.is_err(), "mismatch must fail loudly, got {res:?}");
}

#[test]
fn bullet_layout_files_block_yields_expected_paths() {
    let text = concat!(
        "### Task 1: Thing\n",
        "**Files:**\n",
        "- Create: `src/foo.rs`\n",
        "  - Modify: `src/bar.rs:10-20`\n",
        "* Test: `tests/foo.rs`\n",
        "+ Delete: `src/old.rs`\n",
        "- [ ] step\n",
    );
    let plan = parse_plan(text);
    assert_eq!(plan.task_count(), 1);
    let mut got = plan.tasks[0].expected_paths.clone();
    got.sort();
    assert_eq!(
        got,
        vec!["src/bar.rs", "src/foo.rs", "src/old.rs", "tests/foo.rs"]
    );
}

#[test]
fn failed_startup_scan_does_not_signal_complete() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = Store::open(dir.path().join("db.sqlite")).unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    common::make_git_repo(&wt);
    // Break the store so the worktree sync fails.
    store
        .write(|conn| {
            conn.execute_batch("ALTER TABLE worktrees RENAME TO worktrees_gone")?;
            Ok(())
        })
        .unwrap();
    assert!(startup_scan(&store, &wt).is_err());
    assert!(!store.startup_scan_complete());
}

#[test]
fn plans_tick_json_err_when_startup_scan_failed() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    common::make_git_repo(&wt);
    let store = Store::open(dir.path().join("db.sqlite")).unwrap();
    store
        .write(|conn| {
            conn.execute_batch("ALTER TABLE worktrees RENAME TO worktrees_gone")?;
            Ok(())
        })
        .unwrap();
    assert!(startup_scan(&store, &wt).is_err());
    // barrier unset => tick path surfaces failure
    let res = plans_tick_json(&store, None, false, Some(&wt));
    assert!(res.is_err(), "expected Err, got {res:?}");
}

#[test]
fn files_colon_outside_bold_survives() {
    let text = concat!("### Task 1: Thing\n", "**Files**: `src/x.rs`\n",);
    let plan = parse_plan(text);
    assert_eq!(plan.task_count(), 1);
    assert_eq!(plan.tasks[0].expected_paths, vec!["src/x.rs"]);
}

/// Store holds committed ordinals the plan text no longer has (shrunken plan).
#[test]
fn tick_fails_loudly_when_committed_ordinal_exceeds_derived_tasks() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    common::make_git_repo(&wt);
    std::fs::write(wt.join("src/a.rs"), "base\n").unwrap();
    std::fs::write(wt.join("src/b.rs"), "base\n").unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "base"]);
    git(&wt, &["checkout", "-qb", "feature"]);
    std::fs::create_dir_all(wt.join("docs/superpowers/plans")).unwrap();
    let plan = wt.join("docs/superpowers/plans/plan.md");
    std::fs::write(
        &plan,
        "## Task 1\nModify: `src/a.rs`\n- [ ] a\n## Task 2\nModify: `src/b.rs`\n- [ ] b\n",
    )
    .unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "plan"]);
    std::fs::write(wt.join("src/a.rs"), "changed\n").unwrap();
    std::fs::write(wt.join("src/b.rs"), "changed\n").unwrap();
    git(&wt, &["commit", "-qam", "code"]);

    let store = Store::open(dir.path().join("db.sqlite")).unwrap();
    store.mark_startup_scan_complete();
    // First tick syncs both tasks into the store as committed.
    let _ = plans_tick_json(&store, None, false, Some(&wt));
    // Shrink the plan to one task; stale ordinal 2 stays in the store.
    std::fs::write(&plan, "## Task 1\nModify: `src/a.rs`\n- [x] a\n").unwrap();
    git(&wt, &["commit", "-qam", "shrink"]);
    let res = plans_tick_json(&store, None, false, Some(&wt));
    assert!(
        res.is_err(),
        "stale committed ordinal must fail, got {res:?}"
    );
}

/// A committed task whose checkbox vanished must fail even if another task keeps a box.
#[test]
fn tick_fails_loudly_when_committed_task_lost_its_checkbox() {
    let dir = tempfile::TempDir::new().unwrap();
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    common::make_git_repo(&wt);
    std::fs::write(wt.join("src/a.rs"), "base\n").unwrap();
    std::fs::write(wt.join("src/b.rs"), "base\n").unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "base"]);
    git(&wt, &["checkout", "-qb", "feature"]);
    std::fs::create_dir_all(wt.join("docs/superpowers/plans")).unwrap();
    let plan = wt.join("docs/superpowers/plans/plan.md");
    std::fs::write(
        &plan,
        "## Task 1\nModify: `src/a.rs`\n- [ ] a\n## Task 2\nModify: `src/b.rs`\n- [ ] b\n",
    )
    .unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "plan"]);
    std::fs::write(wt.join("src/a.rs"), "changed\n").unwrap();
    std::fs::write(wt.join("src/b.rs"), "changed\n").unwrap();
    git(&wt, &["commit", "-qam", "code"]);

    let store = Store::open(dir.path().join("db.sqlite")).unwrap();
    store.mark_startup_scan_complete();
    let _ = plans_tick_json(&store, None, false, Some(&wt));
    std::fs::write(
        &plan,
        "## Task 1\nModify: `src/a.rs`\n## Task 2\nModify: `src/b.rs`\n- [x] b\n",
    )
    .unwrap();
    git(&wt, &["commit", "-qam", "drop box"]);
    let res = plans_tick_json(&store, None, false, Some(&wt));
    assert!(res.is_err(), "task lost its checkbox, got {res:?}");
}
