//! Command-line interface definition for the `ra` binary.
//!
//! This module is intentionally phase-agnostic: every subcommand documented in the
//! implementation plan (Section 7) is *recognized* here so that help text and error
//! messages are complete and stable, but dispatch only *runs* the subcommands that the
//! current phase implements. Unimplemented subcommands return a clear, phase-numbered
//! error instead of panicking.

use clap::{Args, Parser, Subcommand};

/// repo-assist — advisory, read-only git coordination daemon.
#[derive(Parser, Debug)]
#[command(
    name = "ra",
    version,
    about = "repo-assist — advisory git coordination daemon",
    long_about = None,
    disable_help_flag = false,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Run the daemon for the repo containing the cwd.
#[derive(Args, Debug)]
pub struct DaemonArgs {
    /// Stay in the foreground instead of detaching.
    #[arg(long)]
    pub foreground: bool,

    /// Accepted for symmetry with client commands; has no effect on `daemon`.
    #[arg(long)]
    pub no_spawn: bool,
}

/// One-screen summary: worktrees, active plans, open findings, sessions.
#[derive(Args, Debug, Default)]
pub struct StatusArgs {
    /// Emit JSON instead of human-readable text.
    #[arg(long)]
    pub json: bool,

    /// Do not auto-start the daemon if none is running (Section 7).
    #[arg(long)]
    pub no_spawn: bool,
}

/// Worktrees with branch, ahead/behind base, changed-file counts, last activity.
#[derive(Args, Debug, Default)]
pub struct WorktreesArgs {
    #[arg(long)]
    pub json: bool,
}

/// Plans with declared vs observed progress; detail lists tasks and evidence.
#[derive(Args, Debug, Default)]
pub struct PlansArgs {
    #[arg(long)]
    pub json: bool,
}

/// A single plan by id, listing its tasks and evidence.
#[derive(Args, Debug)]
pub struct PlanArgs {
    /// Plan id (12-char short id as stored in the DB).
    pub id: String,

    #[arg(long)]
    pub json: bool,
}

/// Files changed in a plan's worktree that no task expects.
#[derive(Args, Debug, Default)]
pub struct DriftArgs {
    #[arg(long)]
    pub json: bool,
}

/// Overlaps and merge conflicts between worktrees and between in-flight plans.
#[derive(Args, Debug, Default)]
pub struct ConflictsArgs {
    #[arg(long)]
    pub json: bool,
}

/// Worktrees, plans, and sessions currently touching a path.
#[derive(Args, Debug)]
pub struct WhoTouchesArgs {
    /// File or directory path.
    pub path: String,

    #[arg(long)]
    pub json: bool,
}

/// List findings.
#[derive(Args, Debug, Default)]
pub struct FindingsArgs {
    /// Only show open findings.
    #[arg(long)]
    pub open: bool,

    /// Show all findings (including acked and resolved).
    #[arg(long)]
    pub all: bool,

    /// Filter by finding kind.
    #[arg(long)]
    pub kind: Option<String>,

    #[arg(long)]
    pub json: bool,
}

/// Acknowledge a finding (suppresses notification but keeps it shown dimmed).
#[derive(Args, Debug)]
pub struct AckArgs {
    /// Finding id.
    pub id: String,
}

/// Known agent sessions.
#[derive(Args, Debug, Default)]
pub struct SessionsArgs {
    #[arg(long)]
    pub json: bool,
}

/// Open the TUI.
#[derive(Args, Debug, Default)]
pub struct TuiArgs {
    #[arg(long)]
    pub json: bool,
}

/// Environment checks (git version, state dir, socket, adapter roots, config).
#[derive(Args, Debug, Default)]
pub struct DoctorArgs {
    #[arg(long)]
    pub json: bool,
}

/// Force-render the markdown views now.
#[derive(Args, Debug, Default)]
pub struct RenderArgs {
    #[arg(long)]
    pub json: bool,
}

/// All recognized subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the daemon for the repo containing the cwd.
    Daemon(DaemonArgs),
    /// One-screen summary.
    Status(StatusArgs),
    /// Worktrees.
    Worktrees(WorktreesArgs),
    /// Plans.
    Plans(PlansArgs),
    /// A single plan by id.
    Plan(PlanArgs),
    /// Files changed in a plan's worktree that no task expects.
    Drift(DriftArgs),
    /// Overlaps and merge conflicts.
    Conflicts(ConflictsArgs),
    /// Worktrees/plans/sessions touching a path.
    WhoTouches(WhoTouchesArgs),
    /// Findings.
    Findings(FindingsArgs),
    /// Acknowledge a finding.
    Ack(AckArgs),
    /// Known agent sessions.
    Sessions(SessionsArgs),
    /// Open the TUI.
    Tui(TuiArgs),
    /// Environment checks.
    Doctor(DoctorArgs),
    /// Force-render markdown views.
    Render(RenderArgs),
}

/// Whether a given command is implemented in the current phase.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// True when the command actually runs.
    pub implemented: bool,
    /// Task number where it becomes functional.
    pub task: u8,
}

/// Map each subcommand to whether it runs and, if not, the task that implements it.
fn spec(cmd: &Command) -> Spec {
    use Command::*;
    match cmd {
        Daemon(_) => Spec {
            implemented: true,
            task: 1,
        },
        Status(_) => Spec {
            implemented: true,
            task: 1,
        },
        Worktrees(_) => Spec {
            implemented: false,
            task: 5,
        },
        Plans(_) => Spec {
            implemented: false,
            task: 3,
        },
        Plan(_) => Spec {
            implemented: false,
            task: 3,
        },
        Drift(_) => Spec {
            implemented: false,
            task: 3,
        },
        Conflicts(_) => Spec {
            implemented: false,
            task: 5,
        },
        WhoTouches(_) => Spec {
            implemented: false,
            task: 5,
        },
        Findings(_) => Spec {
            implemented: false,
            task: 5,
        },
        Ack(_) => Spec {
            implemented: false,
            task: 5,
        },
        Sessions(_) => Spec {
            implemented: false,
            task: 4,
        },
        Tui(_) => Spec {
            implemented: false,
            task: 5,
        },
        Doctor(_) => Spec {
            implemented: false,
            task: 5,
        },
        Render(_) => Spec {
            implemented: false,
            task: 2,
        },
    }
}

/// Run a subcommand, returning a process exit code.
///
/// Returns `Err` (mapped to exit code 2 in `main`) when the subcommand is recognized but
/// not yet implemented in the current phase. This keeps help text complete and errors clear
/// throughout the multi-phase build.
pub fn run(cli: Cli) -> Result<i32, anyhow::Error> {
    let spec = spec(&cli.command);
    if !spec.implemented {
        return Err(anyhow::anyhow!(
            "`{}` is not yet implemented (scheduled for Task {})",
            command_name(&cli.command),
            spec.task,
        ));
    }
    match cli.command {
        Command::Daemon(args) => run_daemon(&args),
        Command::Status(args) => run_status(&args),
        _ => run_implemented(cli.command),
    }
}

/// Resolve the repo (common dir, key, socket path) for the cwd, spawning the daemon when
/// allowed and none is running (Section 7: auto-spawn; `spawn=false` disables).
fn resolve_and_connect(spawn: bool) -> Result<crate::ipc::client::Client, anyhow::Error> {
    use crate::paths::{git_common_dir, repo_key_from_git_common_dir, Env};

    let cwd = std::env::current_dir().context("resolving cwd")?;
    let common_dir = git_common_dir(&cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "not a git repository (no git common dir above {})",
            cwd.display()
        )
    })?;
    let repo_key = repo_key_from_git_common_dir(&common_dir);
    let env = Env::from_real();
    let sock = env.socket_path(&repo_key);

    let connect_err = |e: crate::ipc::client::IpcError| e.to_string();
    match crate::ipc::client::Client::connect(sock.clone()) {
        Ok(client) => Ok(client),
        Err(e) if !matches!(e, crate::ipc::client::IpcError::NotRunning(_)) => Err(
            anyhow::anyhow!("connecting to the daemon: {}", connect_err(e)),
        ),
        Err(_) if !spawn => Err(anyhow::anyhow!(
            "no daemon running at {} (pass a spawnable command or start `ra daemon` manually)",
            sock.display()
        )),
        Err(_) => {
            // Auto-spawn (Section 7): start the daemon detached, then wait for the socket.
            let mut child = crate::daemon::spawn_detached(&cwd)
                .map_err(|e| anyhow::anyhow!("spawning the daemon: {e}"))?;
            let ok = crate::daemon::wait_for_socket(&sock, std::time::Duration::from_secs(5))
                .map_err(|e| anyhow::anyhow!("waiting for the daemon socket: {e}"))?;
            if !ok {
                // Let the child report why it failed, if it already exited.
                let _ = child.try_wait()?;
                anyhow::bail!("daemon did not create its socket within 5s");
            }
            Ok(crate::ipc::client::Client::connect(sock)
                .map_err(|e| anyhow::anyhow!("connecting to the daemon: {e}"))?)
        }
    }
}

/// `ra daemon`: run the daemon for the cwd repo, in the foreground of this process.
fn run_daemon(args: &DaemonArgs) -> Result<i32, anyhow::Error> {
    // `--foreground` on the CLI means "do not detach": when we are the spawned child we run
    // with `--foreground` too, so this entry is the daemon body either way.
    let _ = args;
    let cwd = std::env::current_dir().context("resolving cwd")?;
    use crate::paths::{git_common_dir, repo_key_from_git_common_dir, Env};
    let common_dir = git_common_dir(&cwd).context("not a git repository")?;
    let repo_key = repo_key_from_git_common_dir(&common_dir);
    let env = Env::from_real();

    // Set up tracing to the daemon log (Section 3: rolling file in the state dir).
    let state_files = env.state_files(&repo_key);
    std::fs::create_dir_all(&state_files.dir).context("creating the state dir")?;
    let file_appender = tracing_appender::rolling::never(&state_files.dir, "daemon.log");
    let (writer, _guard) = tracing_appender::non_blocking(file_appender);
    let _ = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let state = crate::daemon::DaemonState {
        repo_key: repo_key.clone(),
        state: state_files,
        config: crate::config::Config::default_config(),
        env,
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(crate::daemon::run_foreground(&cwd, state))?;
    Ok(0)
}

/// `ra status`: one-screen summary (Section 7).
fn run_status(args: &StatusArgs) -> Result<i32, anyhow::Error> {
    let mut client = resolve_and_connect(!args.no_spawn)?;
    let reply = client.call(crate::ipc::Method::Status, serde_json::Value::Null)?;
    match reply {
        crate::ipc::Reply::Result { result, .. } => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_status_human(&serde_json::from_value(result)?);
            }
        }
        crate::ipc::Reply::Error { error, .. } => {
            anyhow::bail!("daemon error [{}]: {}", error.code, error.message);
        }
    }
    Ok(0)
}

/// Human-readable one-screen status output.
fn print_status_human(s: &serde_json::Value) {
    let get = |k: &str| s.get(k).cloned().unwrap_or_default();
    println!(
        "worktrees: {}  plans(active): {}  findings(open): {}  sessions: {}",
        get("worktree_count"),
        get("active_plans"),
        get("open_findings"),
        get("sessions")
    );
    if let Some(list) = s.get("worktrees").and_then(|v| v.as_array()) {
        for wt in list {
            let path = wt.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let branch = wt.get("branch").and_then(|v| v.as_str()).unwrap_or("-");
            let changed = wt.get("changed").and_then(|v| v.as_i64()).unwrap_or(0);
            let present = wt.get("present").and_then(|v| v.as_bool()).unwrap_or(true);
            println!(
                "  {} {} ({}): {} changed file(s){}",
                if present { "*" } else { "!" },
                path,
                branch,
                changed,
                if wt.get("is_main").and_then(|v| v.as_bool()).unwrap_or(false) {
                    " [main]"
                } else {
                    ""
                }
            );
        }
    }
}

/// Human name of the subcommand, used in error messages.
fn command_name(cmd: &Command) -> &'static str {
    use Command::*;
    match cmd {
        Daemon(_) => "daemon",
        Status(_) => "status",
        Worktrees(_) => "worktrees",
        Plans(_) => "plans",
        Plan(_) => "plan",
        Drift(_) => "drift",
        Conflicts(_) => "conflicts",
        WhoTouches(_) => "who-touches",
        Findings(_) => "findings",
        Ack(_) => "ack",
        Sessions(_) => "sessions",
        Tui(_) => "tui",
        Doctor(_) => "doctor",
        Render(_) => "render",
    }
}

use anyhow::Context;

/// Route for subcommands recognized but not yet implemented in the current phase.
fn run_implemented(cmd: Command) -> Result<i32, anyhow::Error> {
    Err(anyhow::anyhow!(
        "`{}` is not yet implemented (scheduled for Task {})",
        command_name(&cmd),
        spec(&cmd).task,
    ))
}
