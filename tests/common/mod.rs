//! Test helpers: build a temp git repo with N worktrees, commits, and dirty files.
//!
//! Used by the Task 0.2 acceptance tests.

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

/// Build a git repo in `dir` and commit one base file on `main`.
pub fn make_git_repo(dir: &Path) {
    run(dir, &["init", "-q", "-b", "main"]);
    run(dir, &["config", "user.email", "t@example.com"]);
    run(dir, &["config", "user.name", "test"]);
    run(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("base.txt"), "hello\n").expect("write base");
    run(dir, &["add", "base.txt"]);
    run(dir, &["commit", "-qm", "initial"]);
}

/// A freshly-built temp git repo whose worktrees and commits are tracked for cleanup.
/// Shared test helper; not used by every test target.
#[allow(dead_code)]
pub struct GitRepo {
    root: tempfile::TempDir,
    pub paths: Vec<std::path::PathBuf>,
}

impl GitRepo {
    /// Create a repo root, commit a base file on `main`, and return it.
    pub fn new() -> Self {
        let root = tempfile::TempDir::new().expect("temp dir");
        make_git_repo(root.path());
        GitRepo {
            root,
            paths: vec![],
        }
    }

    /// Absolute path to the repo root.
    pub fn path(&self) -> &Path {
        self.root.path()
    }

    /// Add a worktree at `<root>/<name>` from a unique branch, create base file `f`.
    pub fn add_worktree(&mut self, name: &str, file: &str) {
        let wt = self.root.path().join(name);
        // Each worktree needs its own branch; `main` is already checked out in the main tree.
        let branch = format!("wt/{name}");
        run(self.root.path(), &["branch", "-q", &branch]);
        run(
            self.root.path(),
            &["worktree", "add", "-q", wt.to_str().unwrap(), &branch],
        );
        run(
            &wt,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("init {name}"),
            ],
        );
        std::fs::write(wt.join(file), "dirty\n").expect("write dirty file");
        run(&wt, &["add", file]);
        run(&wt, &["commit", "-qm", "dirty work"]);
        self.paths.push(wt);
    }

    /// Create a branch (without switching) from the current HEAD in the first worktree.
    pub fn make_branch(&self, branch: &str) {
        run(self.root.path(), &["branch", "-q", branch]);
    }
}

/// Run a git command in `dir`, asserting success.
pub fn run(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git executed");
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}
