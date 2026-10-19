//! Worktree event bus (Task 2.3): bridges the fs watcher (`fswatch`, Task 2.2) to the store.
//!
//! A single spawned set of threads: one `fswatch` watcher (turning filesystem changes into
//! per-worktree `WorktreeEvent::Dirty { path }`) and one bus-drain thread that, on each dirty
//! event, re-runs the git source (`sync_worktrees`) to refresh the changed-file set and then
//! persists a `worktree_dirty` event row to the store (`append_event`). The daemon `status`
//! method reads those rows (`worktree_dirty_view`) to surface per-worktree dirty state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::sources::fswatch;
use crate::sources::git;
use crate::store::Store;

/// Handle for a running event bus.
pub struct BusHandle {
    stop: fswatch::StopHandle,
    stopping: Arc<AtomicBool>,
}

impl BusHandle {
    /// Stop the bus: stop the fswatch watcher and end the drain thread (idempotent).
    pub fn stop(&self) {
        stop_bus(self);
    }
}

/// Start the event bus for the repo containing `repo_root`, returning a handle.
///
/// Returns promptly (the watcher parks its debouncer on its own thread and the drain thread
/// runs separately). Call [`BusHandle::stop`] (or [`stop_bus`]) to tear the bus down.
pub fn start_bus(store: Arc<Store>, repo_root: &Path) -> BusHandle {
    // fswatch emits per-worktree Dirty events onto this channel.
    let (tx, rx) = mpsc::channel::<WorktreeEvent>();
    let stop = fswatch::stop_handle();

    // Start the watcher. Non-blocking: it parks the debouncer on its own thread. Log and
    // keep going (the drain thread still runs, it simply receives nothing) if the watcher
    // fails to start on an unusual filesystem backend.
    if let Err(e) = fswatch::spawn(&store, repo_root, tx, stop.clone()) {
        eprintln!("worktree watcher failed to start: {e}");
    }

    // Bus-drain thread: re-run the git source and persist each worktree_dirty event.
    let rt: Arc<PathBuf> = Arc::new(repo_root.to_path_buf());
    std::thread::spawn(move || bus_loop(rx, store, rt));

    BusHandle {
        stop,
        stopping: Arc::new(AtomicBool::new(false)),
    }
}

/// Stop the event bus identified by `handle`. Idempotent.
pub fn stop_bus(handle: &BusHandle) {
    handle.stopping.store(true, Ordering::SeqCst);
    fswatch::shutdown(handle.stop.clone());
}

/// The bus-drain loop: drain events until the channel closes or we are asked to stop.
fn bus_loop(rx: mpsc::Receiver<WorktreeEvent>, store: Arc<Store>, repo_root: Arc<PathBuf>) {
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            // Persist one worktree_dirty event after re-running the git source.
            Ok(WorktreeEvent::Dirty { path }) => {
                // Re-run the git source to refresh that worktree's changed-file set (Section 5).
                let _ = git::sync_worktrees(&store, repo_root.as_ref());
                // Record the worktree_dirty event in the events table.
                let _ = store.append_event(
                    git::now_ms(),
                    "worktree",
                    "worktree_dirty",
                    Some(&path.display().to_string()),
                    "{}",
                );
            }
            // Debounce timeout this tick: keep draining. Exit when the channel closes
            // (all senders gone) or when asked to stop.
            Err(e) => match e {
                mpsc::RecvTimeoutError::Disconnected => break,
                mpsc::RecvTimeoutError::Timeout => {}
            },
        }
    }
}

/// The per-worktree dirty event emitted by the watcher.
type WorktreeEvent = crate::sources::fswatch::WorktreeEvent;
