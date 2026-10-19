//! Task 2.2 acceptance: touching a file inside a worktree produces a dirty event; writes
//! to paths named in `.gitignore` produce none.

mod common;

use repo_assist::sources::fswatch::{self, WorktreeEvent};
use repo_assist::store::Store;
use std::sync::mpsc;
use std::time::Duration;

/// Spawn a watcher, then call `act` (which should trigger a change), then wait for `want`
/// dirty events. Returns after `timeout` elapses.
fn watch_run_and_wait(
    repo: &common::GitRepo,
    store: &Store,
    act: impl FnOnce(),
    want: usize,
    timeout: Duration,
) -> Vec<WorktreeEvent> {
    let (tx, rx) = mpsc::channel();
    let stop = fswatch::stop_handle();
    fswatch::spawn(store, repo.path(), tx, stop.clone()).unwrap();
    act();
    let mut got = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    while got.len() < want && std::time::Instant::now() < deadline {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
            if matches!(ev, WorktreeEvent::Dirty { .. }) {
                got.push(ev);
            }
        }
    }
    fswatch::shutdown(stop);
    got
}

/// Start a watcher without a triggering action (used to pre-warm the debouncer), waiting
/// for no events.
fn watch_run_and_wait_empty(repo: &common::GitRepo, store: &Store) {
    let (tx, rx) = mpsc::channel();
    let stop = fswatch::stop_handle();
    fswatch::spawn(store, repo.path(), tx, stop.clone()).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    fswatch::shutdown(stop);
    let _ = rx;
}

/// A plain edit inside a worktree produces a batched dirty event for that worktree.
#[test]
fn touching_a_file_marks_the_worktree_dirty() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = Store::open(&db).unwrap();
    repo_assist::sources::git::sync_worktrees(&store, repo.path()).unwrap();

    // Pre-warm the debouncer so the first *real* change fires promptly.
    watch_run_and_wait_empty(&repo, &store);

    // Write to a tracked file *while* the watcher is live.
    let events = watch_run_and_wait(
        &repo,
        &store,
        || std::fs::write(repo.path().join("wt-a").join("a.txt"), "changed\n").unwrap(),
        1,
        Duration::from_secs(6),
    );
    assert!(
        !events.is_empty(),
        "no dirty event within 6s; received: {:?}",
        events
    );

    let dirty_wts: Vec<String> = events
        .iter()
        .map(|e| match e {
            WorktreeEvent::Dirty { path } => path.display().to_string(),
        })
        .collect();
    assert!(
        dirty_wts.iter().any(|p| p.ends_with("wt-a")),
        "expected wt-a dirty, got {dirty_wts:?}"
    );
}

/// Writes to ignored paths (per `.gitignore`) trigger no dirty event.
#[test]
fn ignored_paths_trigger_no_event() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-i", "i.txt");
    // gitignore the target so the watcher should treat it as excluded.
    std::fs::write(repo.path().join("wt-i").join(".gitignore"), "ignored.txt\n").unwrap();
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = Store::open(&db).unwrap();

    // Spawn a watcher, then write to an ignored path *while* it is live.
    let (tx, rx) = mpsc::channel();
    let stop = fswatch::stop_handle();
    fswatch::spawn(&store, repo.path(), tx, stop.clone()).unwrap();
    std::fs::write(repo.path().join("wt-i").join("ignored.txt"), "x\n").unwrap();
    std::fs::write(repo.path().join("wt-i").join("ignored.txt"), "y\n").unwrap();

    // No dirty event should arrive within the debounce window (~0.5s) plus slack.
    let mut dirty = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
            if matches!(ev, WorktreeEvent::Dirty { .. }) {
                dirty = true;
                break;
            }
        }
    }
    fswatch::shutdown(stop);
    assert!(!dirty, "ignored path produced a dirty event");
}
