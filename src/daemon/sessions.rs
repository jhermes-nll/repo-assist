//! Sessions ingest ticker (Task 4): a daemon-owned thread that re-ingests pi session
//! files into the store. Mirrors the worktree event bus: sources feed the store in the
//! background; RPC handlers stay pure store reads. Freshness: first pass immediately at
//! daemon start, then every [`TICK_INTERVAL`] — `ra sessions` output may trail reality by
//! up to ~60 seconds, which is the documented behavior.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::store::Store;

pub const TICK_INTERVAL: Duration = Duration::from_secs(60);

pub struct SessionsTicker {
    stop: Arc<AtomicBool>,
}

impl SessionsTicker {
    pub fn start(store: Arc<Store>, repo_root: PathBuf, sessions_root: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || loop {
            crate::sources::sessions::sync_sessions(&store, &repo_root, &sessions_root);
            let deadline = std::time::Instant::now() + TICK_INTERVAL;
            while std::time::Instant::now() < deadline {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            if flag.load(Ordering::SeqCst) {
                return;
            }
        });
        SessionsTicker { stop }
    }
}

impl Drop for SessionsTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
