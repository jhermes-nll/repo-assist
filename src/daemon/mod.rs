//! The daemon process (Section 4 + Section 6 of the plan).
//!
//! A daemon runs for a single repo (its 12-char *repo key* being `sha256(canonical git common
//! dir)`). On start it:
//!
//! 1. resolves its repo key and state/socket/pidfile paths,
//! 2. (re)creates the socket, acquiring the pidfile `flock` (rejecting a second *live* daemon,
//!    Section 6),
//! 3. opens (and, if absent, creates+migrates) the SQLite store,
//! 4. accepts IPC connections on the Unix socket until asked to shut down.
//!
//! The daemon is advisory only: it never writes inside any work tree (Section 2).

use std::fs::OpenOptions;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

pub mod bus;
pub mod findings;
pub mod lock;
pub mod sessions;

use crate::config::Config;
use crate::ipc::server::run_accept_loop;
use crate::paths::{git_common_dir, Env, StateFiles};
use crate::store::Store;

/// State handed to the daemon after its repo key and paths are resolved.
pub struct DaemonState {
    /// The repo key (12 hex chars) this daemon serves.
    pub repo_key: String,
    /// Absolute path to the state directory.
    pub state: StateFiles,
    /// The daemon configuration.
    pub config: Config,
    /// Environment inputs (home, XDG vars).
    pub env: Env,
}

/// Run the daemon for the repository containing `repo_root` (Section 6).
///
/// Resolves the repo key, (re)creates the socket while holding the pidfile lock, opens (and
/// migrates if needed) the store, then runs the IPC accept loop until a SIGINT/SIGTERM is
/// received or a `shutdown` IPC request arrives (Section 6: graceful shutdown).
pub async fn run_foreground(repo_root: &Path, state: DaemonState) -> Result<(), std::io::Error> {
    let repo_key = state.repo_key.clone();
    let StateFiles { db, pid, .. } = state.state;

    // Resolve the git common dir from the repo root to compute the repo key (Section 4).
    let common_dir = git_common_dir(repo_root)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not a git repository"))?;
    if repo_key.is_empty() {
        return Err(std::io::Error::other(format!(
            "could not determine a repo key from {common_dir:?}"
        )));
    }

    // Acquire the pidfile lock (Section 6): returns early with a clear error if another live
    // daemon already holds the lock for this repo.
    let _pid_lock = lock::acquire_lock(&pid)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::AlreadyExists, e.to_string()))?;

    // (Re)create the socket directory, then clear any stale socket left by a previous crash
    // (Section 6: stale-socket cleanup).
    let sock = state.env.socket_path(&repo_key);
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| std::io::Error::other(format!("creating socket dir: {e}")))?;
    }
    remove_socket(&sock);

    // Open the store (creating and migrating it if the file is absent).
    let store = Arc::new(Store::open(&db).map_err(|e| std::io::Error::other(e.to_string()))?);

    // Task 2.3: start the worktree event bus (watcher + drain thread).
    let bus = crate::daemon::bus::start_bus(store.clone(), repo_root);

    // Task 5: the findings runner. Runs every 5s (once immediately, then periodically),
    // rebuilding the workspace, running all checks, and reconciling the results against
    // the persisted `findings` table. Drop-guarded: holding the handle keeps the thread
    // alive; dropping it (any return path) ends it.
    let _findings_thread = crate::daemon::findings::start_findings(
        store.clone(),
        state.config.clone(),
        repo_root.to_path_buf(),
        Duration::from_secs(5),
    );

    // Task 4: the sessions ingest ticker (pi files -> sessions/tool_calls rows). First
    // pass runs immediately; then every 60s. Drop-guarded: any return path below drops the
    // handle, which stops the thread.
    let sessions_root = crate::sources::sessions::resolve_sessions_root(&state.env);
    let _sessions_ticker = crate::daemon::sessions::SessionsTicker::start(
        store.clone(),
        repo_root.to_path_buf(),
        sessions_root,
    );

    // Signal to the accept loop to shut down.
    let (stop_tx, stop_rx) = watch::channel::<Option<u32>>(None);

    // Plans discovery ticker (Section 3.2): slow 60s backstop that scans every known
    // plan worktree and re-imports plans/tasks (plus observed derivation). Drop-guarded.
    let _plans_ticker = {
        let store = store.clone();
        let stop_rx_plans = stop_rx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(60));
                if stop_rx_plans.borrow().is_some() {
                    break;
                }
                if let Ok(wts) = store.plan_worktrees() {
                    for pw in wts {
                        if let (Some(id), Some(path)) = (pw.worktree_id, pw.path.as_deref()) {
                            let _ = crate::plans::discover::sync_plans(
                                &store,
                                id,
                                Path::new(path),
                                crate::plans::discover::default_adapters(),
                            );
                        }
                    }
                }
            }
        })
    };

    // Debounced auto-render (M1 / §11): background thread, never blocks IPC/tick loop.
    let _render_thread = if state.config.views.enabled {
        let cfg = state.config.clone();
        let store = store.clone();
        let repo_key = repo_key.clone();
        let stop_rx_render = stop_rx.clone();
        Some(std::thread::spawn(move || {
            let interval = Duration::from_millis(cfg.views.debounce_ms.max(100));
            loop {
                std::thread::sleep(interval);
                if stop_rx_render.borrow().is_some() {
                    break;
                }
                let env = crate::paths::Env::from_real();
                let dir = env.views_dir(&repo_key);
                let _ = dir;
                match crate::ipc::server::render_views_json(&store, &repo_key, &cfg) {
                    Ok(_) => {}
                    Err(e) => tracing::warn!("view render failed: {e}"),
                }
            }
        }))
    } else {
        None
    };

    // Race the external stop signal (SIGINT/SIGTERM) against the accept loop: the first to fire
    // ends the daemon (Section 6: graceful shutdown).
    let accept = run_accept_loop(
        sock.clone(),
        stop_rx,
        stop_tx.clone(),
        store,
        repo_key,
        Duration::from_secs(3600), // hard timeout only; normal shutdown is via signal/`shutdown`.
        state.config.clone(),
    );

    // SIGINT (ctrl_c) and SIGTERM both end the daemon gracefully (Section 6).
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // The accept arm's `Result` is the daemon body; the signal arms early-return. The
    // select's non-`break` value is discarded here (all arms end the function one way).
    tokio::select! {
        result = accept => { let _ = result; }
        _ = tokio::signal::ctrl_c() => {
            let _ = stop_tx.send(Some(0));
            drop(_findings_thread);
            crate::daemon::bus::stop_bus(&bus);
            return Ok(());
        }
        _ = sigterm.recv() => {
            let _ = stop_tx.send(Some(0));
            drop(_findings_thread);
            crate::daemon::bus::stop_bus(&bus);
            return Ok(());
        }
    }

    // Normal exit (accept loop returned): stop the findings runner and the event bus too.
    drop(_findings_thread);
    crate::daemon::bus::stop_bus(&bus);
    Ok(())
}

/// Spawn this binary as a detached background daemon for the repo at `repo_root`
/// (Section 7: client commands auto-start the daemon, detached via a setsid-style new
/// process group). The child runs `ra daemon --foreground` with stdio redirected to the
/// daemon log. Returns the child (its PID is the daemon pid).
pub fn spawn_detached(repo_root: &Path) -> Result<Child, std::io::Error> {
    let exe = std::env::current_exe()?;
    // Compute the state dir here so the log file can be created before the child starts.
    let common_dir = git_common_dir(repo_root)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not a git repository"))?;
    let repo_key = crate::paths::repo_key_from_git_common_dir(&common_dir);
    let env = Env::from_real();
    let state_files = env.state_files(&repo_key);
    std::fs::create_dir_all(&state_files.dir)?;

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state_files.log)?;

    let mut cmd = Command::new(exe);
    cmd.args(["daemon", "--foreground"])
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file));
    // Detach into a new session, so the daemon survives the CLI process exiting
    // (`setsid` semantics; stable Rust equivalent).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
}

/// Poll until the socket file exists (or `timeout` elapses). Returns Ok(true) when present.
pub fn wait_for_socket(sock: &Path, timeout: Duration) -> Result<bool, std::io::Error> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if sock.exists() {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

/// Remove a stale socket file at `path` if it exists (Section 6: stale-socket cleanup).
fn remove_socket(sock: &Path) {
    if sock.exists() {
        let _ = std::fs::remove_file(sock);
    }
}
