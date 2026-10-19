//! Pidfile lock acquisition (Section 6).
//!
//! The daemon holds an exclusive `flock` (via [`fs2`]) on a pidfile so that only one daemon
//! runs per repository. If another *live* daemon holds the lock, the second daemon refuses to
//! start (Section 6: "Refuse to start if another live daemon holds the pidfile flock"). If the
//! recorded PID is dead, the lock is considered stale and taken over.

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
    #[error("could not take the pidfile lock: {0}")]
    Flock(String),
    /// Another live daemon already holds the lock (Section 6).
    #[error("a daemon is already running for this repository (pid {0})")]
    Busy(u32),
}

/// Acquire the exclusive pidfile lock (fs2 `flock`) for the daemon running this process.
///
/// Returns an opened [`File`] held exclusively. If another *live* daemon already holds the
/// lock, returns [`LockError::Busy`]. If the recorded PID is dead, the lock is considered
/// stale and taken over (Section 6).
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

    // Inspect the previously recorded PID to detect a stale lock.
    if let Ok(previous) = std::fs::read_to_string(pid_path) {
        if let Ok(prev_pid) = previous.trim().parse::<u32>() {
            if prev_pid != 0 && prev_pid != std::process::id() && is_alive(prev_pid) {
                return Err(LockError::Busy(prev_pid));
            }
        }
    }

    // Stale or first run: record our own PID, then try (non-blocking) to take the exclusive
    // lock so a second daemon fails fast instead of hanging (Section 6: "Refuse to start").
    std::fs::write(pid_path, format!("{}", std::process::id()))
        .map_err(|e| LockError::Io(e.to_string()))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(file),
        Err(e) => {
            let held = std::fs::read_to_string(pid_path)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if held != 0 && held != std::process::id() {
                Err(LockError::Busy(held))
            } else {
                Err(LockError::Flock(e.to_string()))
            }
        }
    }
}

/// Whether `pid` refers to a live process (Linux `/proc/<pid>` directory exists).
fn is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}
