//! Filesystem watcher (Task 2.2): watch each worktree root with `notify` (debounced 500 ms),
//! honor `.gitignore` through the `ignore` crate, never act on `.git` internals except
//! `HEAD`/refs, and emit `worktree_dirty` events.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ignore::gitignore::Gitignore;
use notify::Watcher;
use notify_debouncer_full::{new_debouncer, DebounceEventHandler, Debouncer, FileIdMap};

use crate::store::Store;

/// An event emitted by the watcher for the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeEvent {
    /// A worktree changed: re-run the git source for that worktree.
    Dirty {
        /// Root of the affected worktree.
        path: PathBuf,
    },
}

/// A stop handle for a running watcher set (set it to stop).
#[derive(Clone)]
pub struct StopHandle(Arc<Mutex<bool>>);

/// A fresh stop handle: `false` = keep running.
pub fn stop_handle() -> StopHandle {
    StopHandle(Arc::new(Mutex::new(false)))
}

/// Flip the handle to stop the watchers.
pub fn shutdown(handle: StopHandle) {
    if let Ok(mut f) = handle.0.lock() {
        *f = true;
    }
}

/// Whether `path` is git metadata we skip (`.git` internals except `HEAD`/refs).
fn is_git_internal(path: &Path) -> bool {
    let comps: Vec<OsString> = path
        .components()
        .skip_while(|c| c.as_os_str() != ".git")
        .skip(1)
        .map(|c| c.as_os_str().to_os_string())
        .collect();
    match comps.first().map(|s| s.to_string_lossy().to_string()) {
        None => false, // the `.git` dir itself
        Some(s) => matches!(s.as_str(), "HEAD" | "refs") || s.contains("refs/"),
    }
}

/// A worktree's `.gitignore` coverage: an absolute base path paired with the matcher that
/// applies under it (populated from the worktree's own `.gitignore`, when present).
type GitignoreMap = HashMap<PathBuf, Gitignore>;

/// Populate a [`GitignoreMap`] from a worktree's own `.gitignore`, if present.
///
/// The `ignore` crate consumes `.gitignore` files as rules (rather than emitting them),
/// so we read the worktree root's file directly. `Gitignore::new` already interprets
/// paths relative to the file's parent, i.e. the worktree root — which is exactly what
/// [`is_ignored`] computes via [`Path::strip_prefix`].
fn load_gitignitudes(wt: &Path) -> GitignoreMap {
    let mut map = GitignoreMap::new();
    let gi_path = wt.join(".gitignore");
    if gi_path.is_file() {
        let (gi, _errors) = Gitignore::new(&gi_path);
        map.insert(wt.to_path_buf(), gi);
    }
    map
}

/// Match a single candidate path (relative to the worktree) against a worktree's ignore rules.
fn is_ignored(path: &Path, ignore_map: &GitignoreMap) -> bool {
    for (base, gi) in ignore_map {
        // `matched` interprets `path` relative to `base` (the `.gitignore` file's parent).
        if !path.starts_with(base) {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(path);
        if gi.matched(rel, false).is_ignore() {
            return true;
        }
    }
    false
}

/// Handler: map debounced fs events to per-worktree dirty events.
struct Emitter {
    worktrees: Vec<(PathBuf, GitignoreMap)>,
    tx: Sender<WorktreeEvent>,
}

impl DebounceEventHandler for Emitter {
    fn handle_event(&mut self, result: notify_debouncer_full::DebounceEventResult) {
        let events = match result {
            Ok(events) => events,
            Err(_) => return,
        };

        for ev in &events {
            for path in &ev.event.paths {
                // Select the MOST SPECIFIC (longest) worktree that contains this path, so a
                // change under `wt-a` reports `wt-a` — not the enclosing repo root.
                let best = self
                    .worktrees
                    .iter()
                    .filter(|(wt_root, _)| path.starts_with(wt_root) && !is_git_internal(path))
                    .max_by_key(|(wt_root, _)| wt_root.as_os_str().len());
                let Some((wt_root, ignore_map)) = best else {
                    continue;
                };
                if is_ignored(path, ignore_map) {
                    continue;
                }
                if self
                    .tx
                    .send(WorktreeEvent::Dirty {
                        path: wt_root.clone(),
                    })
                    .is_ok()
                {
                    break;
                }
            }
        }
    }
}

/// Watch every worktree of `repo_root`, sending dirty events on `tx` (Task 2.2 entry point).
/// Returns immediately; the watcher runs on an internal thread and is torn down when `stop`
/// flips.
pub fn spawn(
    _store: &Store,
    repo_root: &Path,
    tx: Sender<WorktreeEvent>,
    stop: StopHandle,
) -> Result<(), notify::Error> {
    let wts = crate::sources::git::discover_worktrees(repo_root);
    let mut worktrees = Vec::with_capacity(wts.len());
    for wt in &wts {
        let ignore_map = load_gitignitudes(&wt.path);
        worktrees.push((wt.path.clone(), ignore_map));
    }

    // Watch each worktree root recursively; the Emitter filters git-internal paths and honours
    // `.gitignore` via `matched(...)`.
    let roots: Vec<PathBuf> = worktrees.iter().map(|(root, _)| root.clone()).collect();
    let mut debouncer: Debouncer<notify::RecommendedWatcher, FileIdMap> =
        new_debouncer(Duration::from_millis(500), None, Emitter { worktrees, tx })?;
    for root in &roots {
        debouncer
            .watcher()
            .watch(root, notify::RecursiveMode::Recursive)?;
    }

    std::thread::spawn(move || loop {
        if stop.0.lock().map(|f| *f).unwrap_or(true) {
            debouncer.stop();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    });
    Ok(())
}
