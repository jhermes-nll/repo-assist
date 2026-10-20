//! The findings runner (Task 5): periodically rebuilds the check workspace, runs every check,
//! and reconciles the results against the persisted `findings` table (Section 9).
//!
//! It runs as a background thread, started by [`crate::daemon::run_foreground`] and stopped by
//! drop-guard when the daemon exits. It never touches any work tree (Section 2): it only reads
//! store state and writes `findings` rows.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::checks::{run_all_checks, CheckCtx};
use crate::config::Config;
use crate::sources::git;
use crate::store::Store;

/// Start the findings runner. Runs once immediately, then every `run_every`. Returns the thread
/// handle; drop-guarded (the thread exits on drop), so the daemon must hold the handle.
pub fn start_findings(
    store: Arc<Store>,
    config: Config,
    repo_root: PathBuf,
    run_every: Duration,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || findings_loop(store, config, repo_root, run_every))
}

/// The findings loop: build the workspace, run all checks, persist the reconciled deltas, then
/// sleep and repeat until the daemon exits.
fn findings_loop(store: Arc<Store>, config: Config, _repo_root: PathBuf, run_every: Duration) {
    loop {
        run_once(&store, &config);
        std::thread::sleep(run_every);
    }
}

/// Run one cycle of the findings engine: build the workspace, run all checks, and persist the
/// reconciled findings (opening new findings, refreshing open/ack findings, resolving vanished
/// ones, reopening reappeared ones).
fn run_once(store: &Store, config: &Config) {
    // Building the workspace may fail if the DB is momentarily locked; log and skip this cycle.
    let workspace = match store.check_workspace() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("findings: check_workspace failed: {e}");
            return;
        }
    };
    let now = git::now_ms();
    let ctx = CheckCtx {
        workspace: &workspace,
        config,
        now,
    };
    // run_all_checks returns candidates deduplicated by `dedupe_key` on write (via the store).
    let candidates = run_all_checks(&ctx);
    if candidates.is_empty() {
        return;
    }
    match store.sync_findings(candidates, now) {
        Ok(deltas) => {
            for d in &deltas {
                if d.opened {
                    eprintln!("findings: opened {} ({})", d.kind, d.dedupe_key);
                } else {
                    eprintln!("findings: resolved {} ({})", d.kind, d.dedupe_key);
                }
            }
        }
        Err(e) => eprintln!("findings: sync_findings failed: {e}"),
    }
}
