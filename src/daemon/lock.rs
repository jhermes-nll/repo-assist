//! Pidfile lock acquisition (Section 6).
//!
//! The daemon holds an exclusive `flock` (via [`fs2`]) on a pidfile so that only one daemon
//! runs per repository. If another *live* daemon holds the lock, the second daemon refuses to
//! start (Section 6: "Refuse to start if another live daemon holds the pidfile flock"). A stale
//! lock is released by the OS when its holder dies, so acquiring the flock is the authority on
//! liveness; the pidfile only names the holder for friendly messages.
//!
//! **Implementation contract:** correctness of writing the pid through `fs::write` on a second,
//! short-lived file descriptor depends on `fs2` being backed by `flock(2)` — `flock` is scoped
//! to the open file description, so it is unaffected by truncation of, or by closing, another
//! descriptor on the same path. Migrating this locker to `fcntl`/`F_SETLK` would silently drop
//! the lock when that descriptor closes. This lock path is also **Unix-only**: on Windows, fs2
//! uses `LockFileEx` range locking, where a contender's read of the locked region can fail
//! outright instead of returning the holder pid.

use std::fs::{File, OpenOptions};
use std::path::Path;

use fs2::FileExt;

/// Errors surfaced by [`acquire_lock`].
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// The state directory could not be created.
    #[error("could not create the state directory: {0}")]
    StateDir(String),
    /// An I/O error touching the pidfile.
    #[error("io error on the pidfile: {0}")]
    Io(String),
    /// The exclusive lock could not be taken (another process holds it).
    #[error("could not take the pidfile lock and no holder pid was recoverable: {0}")]
    Flock(String),
    /// Another live daemon already holds the lock (Section 6).
    #[error("a daemon is already running for this repository (pid {0})")]
    Busy(u32),
}

/// Acquire the exclusive pidfile lock (fs2 `flock`) for the daemon running this process.
///
/// Takes the flock first and writes the pidfile only on success, so the pidfile always names
/// the process that actually holds the lock. Returns an opened [`File`] held exclusively;
/// returns [`LockError::Busy`] if another daemon already holds the lock (Section 6).
pub fn acquire_lock(pid_path: &Path) -> Result<File, LockError> {
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LockError::StateDir(e.to_string()))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(pid_path)
        .map_err(|e| LockError::Io(e.to_string()))?;

    // Take the exclusive lock BEFORE recording our pid: the pidfile then always names
    // the process that actually holds the lock, and a failed contender reads the real
    // holder's pid instead of clobbering the file.
    match file.try_lock_exclusive() {
        Ok(()) => {
            std::fs::write(pid_path, format!("{}", std::process::id()))
                .map_err(|e| LockError::Io(e.to_string()))?;
            Ok(file)
        }
        Err(e) => {
            // The winner may not have finished writing its pid yet; do a short bounded
            // re-read of the pidfile before falling back to the raw flock error.
            let mut held = 0u32;
            for _ in 0..5 {
                held = std::fs::read_to_string(pid_path)
                    .ok()
                    .and_then(|t| t.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                if held != 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            if held != 0 && held != std::process::id() {
                Err(LockError::Busy(held))
            } else {
                Err(LockError::Flock(e.to_string()))
            }
        }
    }
}
