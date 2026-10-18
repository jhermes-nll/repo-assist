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
}

/// One-screen summary: worktrees, active plans, open findings, sessions.
#[derive(Args, Debug, Default)]
pub struct StatusArgs {
    /// Emit JSON instead of human-readable text.
    #[arg(long)]
    pub json: bool,
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
            implemented: false,
            task: 2,
        },
        Status(_) => Spec {
            implemented: false,
            task: 2,
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
    if spec.implemented {
        run_implemented(cli.command)
    } else {
        Err(anyhow::anyhow!(
            "`{}` is not yet implemented (scheduled for Task {})",
            command_name(&cli.command),
            spec.task,
        ))
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

/// When phases grow subcommand implementations, route them here. Nothing is implemented yet.
fn run_implemented(_cmd: Command) -> Result<i32, anyhow::Error> {
    // Kept intentionally empty; each phase wires its subcommands directly in `run`.
    Err(anyhow::anyhow!("unhandled subcommand"))
}
