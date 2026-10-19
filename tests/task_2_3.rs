//! Task 2.3 acceptance: the worktree event bus persists per-worktree dirty state that
//! `ra status` surfaces. Exercises `bus::start_bus` / `BusHandle::stop` directly (no CLI),
//! then reads `store.worktree_dirty_view()` and the persisted `events` row.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use repo_assist::daemon::bus::{self, BusHandle};
use repo_assist::store::Store;

/// Open the store at `<repo>/state/db.sqlite` (isolated from the real state dir).
fn open_store(repo: &common::GitRepo) -> Store {
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    Store::open(&db).unwrap()
}

/// Poll `store.worktree_dirty_view()` until `want_path` shows `count > 0`, up to `timeout`.
fn wait_dirty(store: &Store, want_path: &Path, timeout: Duration) -> Option<(i64, Option<i64>)> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(view) = store.worktree_dirty_view() {
            if let Some(e) = view.get(&want_path.display().to_string()) {
                if e.0 > 0 {
                    return Some(*e);
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A change inside a worktree causes the bus to persist a `worktree_dirty` event for it,
/// surfaced by `store.worktree_dirty_view()`.
#[test]
fn a_worktree_change_is_persisted_by_the_event_bus() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let store = Arc::new(open_store(&repo));

    // Start the bus; it begins watching immediately (fswatch::spawn is non-blocking).
    let rt: std::path::PathBuf = repo.path().to_path_buf();
    let bus: BusHandle = bus::start_bus(store.clone(), &rt);

    // Trigger a change to a tracked file.
    std::fs::write(repo.path().join("wt-a").join("a.txt"), "changed\n").unwrap();

    // The bus re-runs the git source and persists a worktree_dirty event within 8s.
    let got = wait_dirty(&store, &rt.join("wt-a"), Duration::from_secs(8));
    bus.stop();

    let (count, _last) = got.expect("no worktree_dirty event persisted within 8s; view was empty");
    assert!(count > 0, "expected count > 0, got {count}");

    // A `worktree_dirty` row must exist in the events table.
    let n: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM events WHERE source = 'worktree' AND kind = 'worktree_dirty'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert!(n >= 1, "expected >=1 worktree_dirty row, got {n}");
}

/// An ignored path produces no worktree_dirty event (the fswatch watcher filters it).
#[test]
fn ignored_paths_persist_no_worktree_dirty_event() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-i", "i.txt");
    std::fs::write(repo.path().join("wt-i").join(".gitignore"), "ignored.txt\n").unwrap();
    let store = Arc::new(open_store(&repo));

    // Start the bus, then write to a gitignored path.
    let rt: std::path::PathBuf = repo.path().to_path_buf();
    let bus: BusHandle = bus::start_bus(store.clone(), &rt);
    std::fs::write(repo.path().join("wt-i").join("ignored.txt"), "x\n").unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    bus.stop();

    let dirty = store.worktree_dirty_view().unwrap_or_default();
    let any_dirty: bool = dirty.values().any(|(c, _)| *c > 0);
    assert!(
        !any_dirty,
        "ignored path produced a worktree_dirty event: {dirty:?}"
    );
}
