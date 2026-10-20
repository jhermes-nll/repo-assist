//! Path resolution and repo-key derivation.
//!
//! The *repo key* is the first 12 hex chars of `sha256(canonicalized git common dir)`, so
//! every worktree of one repo resolves to the same key (and thus the same state directory,
//! DB, socket, and pidfile). See Section 4 of the implementation plan.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Canonicalize a path for keying: make it absolute and resolve symlinks. Falls back to the
/// original path if it does not (yet) exist on disk.
pub fn canonicalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The 12-char repo key: first 12 hex chars of `sha256(canonical common dir)`.
pub fn repo_key_from_git_common_dir(common_dir: &Path) -> String {
    let binding = canonicalize(common_dir);
    let bytes = binding.as_os_str().as_encoded_bytes();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex::encode(digest)[..12].to_string()
}

/// Resolve the git common dir of `repo_root` (Section 4: the repo key is derived from it).
///
/// Shells out to `git rev-parse --absolute-git-common-dir` (always resolved from inside the
/// repo, so every worktree of one repo returns the same common dir). Returns `None` when
/// `repo_root` is not inside a git repository.
pub fn git_common_dir(repo_root: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--absolute-git-common-dir")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let path = text.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Environment inputs used to resolve paths (overridable for tests).
#[derive(Debug, Clone)]
pub struct Env {
    /// `$HOME` (or `~`).
    pub home: String,
    /// `$XDG_STATE_HOME`; empty falls back to `$HOME/.local/state`.
    pub state_home: String,
    /// `$XDG_RUNTIME_DIR`; empty falls back to the state directory for the socket.
    pub runtime_dir: String,
}

impl Env {
    /// Read from `HOME` + `XDG_STATE_HOME` + `XDG_RUNTIME_DIR` (empty where unset).
    pub fn from_real() -> Self {
        Env {
            home: std::env::var("HOME").unwrap_or_default(),
            state_home: std::env::var("XDG_STATE_HOME").unwrap_or_default(),
            runtime_dir: std::env::var("XDG_RUNTIME_DIR").unwrap_or_default(),
        }
    }

    /// Construct directly (for tests).
    pub fn with(
        home: impl Into<String>,
        state_home: Option<String>,
        runtime_dir: Option<String>,
    ) -> Self {
        Env {
            home: home.into(),
            state_home: state_home.unwrap_or_default(),
            runtime_dir: runtime_dir.unwrap_or_default(),
        }
    }

    /// Base state directory: `$XDG_STATE_HOME/repo-assist/<repo-key>`, or
    /// `~/.local/state/repo-assist/<repo-key>` when `XDG_STATE_HOME` is unset.
    pub fn state_dir(&self, repo_key: &str) -> PathBuf {
        let base = if self.state_home.is_empty() {
            Path::new(&self.home).join(".local/state")
        } else {
            Path::new(&self.state_home).to_path_buf()
        };
        base.join("repo-assist").join(repo_key)
    }

    /// Full set of files that live in the state directory.
    pub fn state_files(&self, repo_key: &str) -> StateFiles {
        let dir = self.state_dir(repo_key).clone();
        StateFiles {
            dir: dir.clone(),
            db: dir.join("db.sqlite"),
            pid: dir.join("daemon.pid"),
            log: dir.join("daemon.log"),
        }
    }

    /// Unix socket path for a repo: `$XDG_RUNTIME_DIR/repo-assist/<repo-key>.sock`, or
    /// `<state dir>/daemon.sock` when `XDG_RUNTIME_DIR` is unset.
    pub fn socket_path(&self, repo_key: &str) -> PathBuf {
        if self.runtime_dir.is_empty() {
            self.state_dir(repo_key).join("daemon.sock")
        } else {
            Path::new(&self.runtime_dir)
                .join("repo-assist")
                .join(format!("{repo_key}.sock"))
        }
    }
    /// Directory holding rendered markdown views for a repo (§11).
    pub fn views_dir(&self, repo_key: &str) -> PathBuf {
        self.state_dir(repo_key).join("views")
    }
}

/// Full set of state files that live in the state directory.
#[derive(Debug, Clone)]
pub struct StateFiles {
    /// State directory: `$XDG_STATE_HOME/repo-assist/<repo-key>`.
    pub dir: PathBuf,
    /// SQLite database (`WAL` mode).
    pub db: PathBuf,
    /// Pidfile, held open via an `fs2` lock.
    pub pid: PathBuf,
    /// Rolling daemon log.
    pub log: PathBuf,
}
