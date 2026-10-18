//! Task 0.2 acceptance tests: git repo helper builds worktrees with commits and dirty files,
//! and every worktree of one repo resolves to a shared repo key.

mod common;

use common::GitRepo;

/// Resolve a worktree's git common dir and return its repo key.
fn key_from_worktree(repo: &GitRepo, worktree_name: &str) -> String {
    let wt = repo.path().join(worktree_name);
    // In production, repo-assist runs `git rev-parse --git-common-dir` per worktree.
    let common_dir = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--absolute-git-common-dir")
        .current_dir(&wt)
        .output()
        .expect("git executed")
        .stdout;
    let common_dir = String::from_utf8(common_dir).expect("utf-8");
    let common_dir = common_dir.trim();
    repo_assist::paths::repo_key_from_git_common_dir(std::path::Path::new(common_dir))
}

#[test]
fn builds_a_repo_with_two_worktrees_sharing_a_key() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    repo.add_worktree("wt-b", "b.txt");

    // The two worktree roots are different directories...
    let a = repo.path().join("wt-a");
    let b = repo.path().join("wt-b");
    assert_ne!(a, b);

    // ...but both resolve to the same repo key (shared git common dir).
    let key_a = key_from_worktree(&repo, "wt-a");
    let key_b = key_from_worktree(&repo, "wt-b");
    assert_eq!(key_a, key_b);
    assert_eq!(key_a.len(), 12);
}

#[test]
fn dirty_files_are_tracked_in_worktrees() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    repo.add_worktree("wt-b", "b.txt");

    // Each worktree has its own dirty file committed on the worktree side...
    assert!(repo.path().join("wt-a").join("a.txt").exists());
    assert!(repo.path().join("wt-b").join("b.txt").exists());

    // ...and the base file exists once in the common dir.
    assert!(repo.path().join("base.txt").exists());
}

#[test]
fn worktree_list_lists_added_worktrees() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    repo.add_worktree("wt-b", "b.txt");

    let out = std::process::Command::new("git")
        .arg("worktree")
        .arg("list")
        .arg("--porcelain")
        .current_dir(repo.path())
        .output()
        .expect("git worktree list");
    let text = String::from_utf8(out.stdout).expect("valid utf-8");
    assert!(text.contains("wt-a"));
    assert!(text.contains("wt-b"));
}
