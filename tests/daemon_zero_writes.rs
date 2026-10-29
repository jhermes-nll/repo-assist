//! DoD (card 20261022-20, item 2): the daemon makes zero writes inside any work tree.
//!
//! The plan (§2) makes the daemon advisory and read-only *with respect to work trees*:
//! all of its output goes to its own state directory (SQLite db, pidfile, log, and the
//! rendered markdown views) resolved under `$XDG_STATE_HOME`, never inside a work tree.
//!
//! This test is the proof of that guarantee. It spawns `ra daemon --foreground` against a
//! repo with N worktrees, lets it run (findings pass + event bus + sessions ingest +
//! debounced view render), then snapshots each worktree's file tree, `git status --porcelain`,
//! and commit count before and after the daemon run and asserts they are identical — the
//! daemon must not add, delete, modify, commit, or otherwise touch any work tree.

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Collect every file under `base`, returned relative to it and sorted. Skips the
/// `.git` entry (worktrees hold a `.git` pointer file, not a directory).
fn snapshot_tree(base: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.file_name().and_then(|f| f.to_str()) == Some(".git") {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.as_os_str().to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    walk(base, base, &mut out);
    out.sort();
    out
}

/// `git status --porcelain` (empty => clean) for a worktree, as raw bytes.
fn git_status_bytes(wt: &Path) -> Vec<u8> {
    Command::new("git")
        .arg("-C")
        .arg(wt)
        .arg("status")
        .arg("--porcelain")
        .output()
        .expect("git status ran")
        .stdout
}

/// Commit count reachable from HEAD — a daemon that wrote inside a work tree would
/// (somehow) show up here, so it is checked too.
fn commit_count(wt: &Path) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(wt)
        .arg("rev-list")
        .arg("--count")
        .arg("HEAD")
        .output()
        .expect("git rev-list ran");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct DaemonGuard(std::process::Child);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl std::ops::Deref for DaemonGuard {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for DaemonGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

type WorktreeSnapshot = (String, Vec<u8>, String);

fn snapshot(repo: &common::GitRepo) -> Vec<WorktreeSnapshot> {
    repo.paths
        .iter()
        .map(|wt| {
            (
                snapshot_tree(wt).join(","),
                git_status_bytes(wt),
                commit_count(wt),
            )
        })
        .collect()
}

#[test]
fn daemon_writes_nothing_inside_any_worktree() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("alpha", "a.md");
    repo.add_worktree("beta", "b.md");
    repo.add_worktree("docs", "notes.md");

    // Snapshot every worktree before the daemon runs.
    let before = snapshot(&repo);

    // Route the daemon's own state OUTSIDE any work tree. Any write it legitimately
    // makes (db, pid, log, views) therefore lands in a state dir we control, not in a
    // work tree — exactly the invariant under test.
    let state = tempfile::TempDir::new().expect("state tempdir");
    let runtime = tempfile::TempDir::new().expect("runtime tempdir");
    let key = {
        let r = repo.path();
        repo_assist::paths::git_common_dir(r)
            .map(|c| repo_assist::paths::repo_key_from_git_common_dir(&c))
            .expect("repo key derivable")
    };

    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").expect("ra binary path");
    let daemon = Command::new(&ra_bin)
        .arg("daemon")
        .arg("--foreground")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");
    let mut daemon = DaemonGuard(daemon);

    // Wait for the socket, then let the daemon run: the findings pass fires once
    // immediately, the event bus drains, and (with the default config) the debounced
    // view-render thread fires.
    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));
    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(sock.exists(), "daemon did not create its socket");
    thread::sleep(Duration::from_secs(3));

    // Tear the daemon down.
    daemon.kill().expect("kill daemon");
    let _ = daemon.wait();

    // Snapshot every worktree again.
    let after = snapshot(&repo);

    for ((bt, bstat, bcount), (at, astat, acount)) in before.iter().zip(after.iter()) {
        assert_eq!(
            bt, at,
            "file tree changed inside a work tree after the daemon ran"
        );
        assert_eq!(
            bstat, astat,
            "git status changed inside a work tree after the daemon ran"
        );
        assert_eq!(
            bcount, acount,
            "commit count changed inside a work tree after the daemon ran"
        );
    }

    // Sanity check: the daemon DID write *somewhere* — to its own state dir, not to any
    // work tree. This proves the test observed activity, so "no writes" is a real result.
    let per_repo = state.path().join("repo-assist").join(&key);
    assert!(
        per_repo.exists(),
        "daemon state dir was created outside work trees"
    );
    assert!(
        std::fs::read_dir(&per_repo)
            .expect("state dir readable")
            .next()
            .is_some(),
        "expected daemon to write db/pid/log under the per-repo state dir"
    );
}
