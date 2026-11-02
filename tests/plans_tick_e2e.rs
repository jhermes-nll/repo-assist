mod common;

use std::process::Command;

fn make_repo(base: &std::path::Path) -> std::path::PathBuf {
    let wt = base.join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    common::make_git_repo(&wt);
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join("src/foo.rs"), "base\n").unwrap();
    for args in [
        ["add", "src/foo.rs"].as_slice(),
        ["commit", "-qm", "base"].as_slice(),
    ] {
        assert!(Command::new("git")
            .current_dir(&wt)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    assert!(Command::new("git")
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
    assert!(Command::new("git")
        .current_dir(&wt)
        .args(["add", "docs/superpowers/plans/plan.md"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .current_dir(&wt)
        .args(["commit", "-qm", "plan base"])
        .status()
        .unwrap()
        .success());
    std::fs::write(wt.join("src/foo.rs"), "changed\n").unwrap();
    assert!(Command::new("git")
        .current_dir(&wt)
        .args(["add", "src/foo.rs"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .current_dir(&wt)
        .args(["commit", "-qm", "code change"])
        .status()
        .unwrap()
        .success());
    wt
}

fn spawn_daemon(repo: &std::path::Path, state: &std::path::Path, runtime: &std::path::Path) {
    let bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let child = Command::new(bin)
        .env("HOME", state)
        .env("XDG_STATE_HOME", state)
        .env("XDG_RUNTIME_DIR", runtime)
        .current_dir(repo)
        .args(["daemon", "--foreground"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // wait
    for _ in 0..100 {
        let st = Command::new(std::env::var("CARGO_BIN_EXE_ra").unwrap())
            .env("HOME", state)
            .env("XDG_STATE_HOME", state)
            .env("XDG_RUNTIME_DIR", runtime)
            .current_dir(repo)
            .args(["status"])
            .output();
        if let Ok(o) = st {
            if o.status.success() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // leak child lifetime to end of test: intentionally leave daemon; killed on process exit
    std::mem::forget(child);
}

fn cli(
    repo: &std::path::Path,
    state: &std::path::Path,
    runtime: &std::path::Path,
    args: &[&str],
) -> (i32, String, String) {
    let out = Command::new(std::env::var("CARGO_BIN_EXE_ra").unwrap())
        .env("HOME", state)
        .env("XDG_STATE_HOME", state)
        .env("XDG_RUNTIME_DIR", runtime)
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn plans_tick_e2e_apply_dryrun_dirty() {
    let base = tempfile::TempDir::new_in("/tmp").unwrap();
    let repo = make_repo(base.path());
    let state = tempfile::TempDir::new_in("/tmp").unwrap();
    let runtime = tempfile::TempDir::new_in("/tmp").unwrap();
    spawn_daemon(&repo, state.path(), runtime.path());

    let plan_file = repo.join("docs/superpowers/plans/plan.md");
    let before = std::fs::read_to_string(&plan_file).unwrap();

    // dry-run (default): file bytes unchanged, but diff reported
    let (code, stdout, _stderr) = cli(&repo, state.path(), runtime.path(), &["plans", "tick"]);
    assert_eq!(code, 0, "stdout={stdout}");
    assert_eq!(std::fs::read_to_string(&plan_file).unwrap(), before);

    // apply: box becomes [x]
    let (code, _stdout, stderr) = cli(
        &repo,
        state.path(),
        runtime.path(),
        &["plans", "tick", "--apply"],
    );
    assert_eq!(code, 0, "stderr={stderr}");
    let after = std::fs::read_to_string(&plan_file).unwrap();
    assert!(after.contains("- [x] do thing"), "got {after}");

    // dirty worktree refused: modify src/foo.rs, apply must not change file
    std::fs::write(repo.join("src/foo.rs"), "dirty\n").unwrap();
    let before_dirty = std::fs::read_to_string(&plan_file).unwrap();
    let (_code, stdout, _stderr) = cli(
        &repo,
        state.path(),
        runtime.path(),
        &["plans", "tick", "--apply"],
    );
    // dirty plans skipped, no apply attempt. Refuse shown.
    assert!(
        stdout.contains("skipped") || stdout.contains("uncommitted"),
        "stdout={stdout}"
    );
    assert_eq!(std::fs::read_to_string(&plan_file).unwrap(), before_dirty);
}
