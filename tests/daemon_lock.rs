//! Tests for `acquire_lock`'s Busy path and its bounded no-pid fallback.

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::{Duration, Instant};

use repo_assist::daemon::lock::{acquire_lock, LockError};
use tempfile::TempDir;

// fs2 is not a dev-dependency, so hold the flock through libc's flock(2) directly —
// the same no-new-dependency FFI pattern tests/stale_socket.rs uses for `close`.
extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

const LOCK_EX: i32 = 2;

/// Open `pid_path` and hold an exclusive flock on it for the lifetime of the returned file.
///
/// flock(2) is scoped to the open file description, so `acquire_lock`'s independent
/// `try_lock_exclusive` on its own descriptor is denied while this one lives.
fn hold_flock(pid_path: &Path) -> std::fs::File {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(pid_path)
        .unwrap();
    let rc = unsafe { flock(f.as_raw_fd(), LOCK_EX) };
    assert_eq!(rc, 0, "flock(LOCK_EX) must succeed inside the test");
    f
}

#[test]
fn busy_reports_foreign_pid_recovered_from_pidfile() {
    let dir = TempDir::new().unwrap();
    let pid_path = dir.path().join("ra.pid");
    // Pre-fill the pidfile with a foreign pid, as a winner that already wrote it would.
    std::fs::write(&pid_path, "424242424").unwrap();
    let _held = hold_flock(&pid_path);

    match acquire_lock(&pid_path) {
        Err(LockError::Busy(pid)) => assert_eq!(pid, 424242424),
        other => panic!("expected Err(LockError::Busy(424242424)), got {other:?}"),
    }
}

#[test]
fn flock_fallback_is_bounded_when_no_pid_recoverable() {
    let dir = TempDir::new().unwrap();
    let pid_path = dir.path().join("ra.pid");
    // Empty pidfile: no holder pid can be parsed out of it.
    std::fs::write(&pid_path, "").unwrap();
    let _held = hold_flock(&pid_path);

    let started = Instant::now();
    let result = acquire_lock(&pid_path);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(1000),
        "the 5-read / 4x2ms re-read loop must be bounded, took {elapsed:?}"
    );

    match result {
        Err(LockError::Flock(msg)) => assert!(
            msg.contains("no holder pid was recoverable"),
            "unexpected message: {msg}"
        ),
        other => panic!("expected Err(LockError::Flock(..)), got {other:?}"),
    }
}
