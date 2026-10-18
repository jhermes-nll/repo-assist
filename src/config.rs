//! Layered TOML configuration with `deny_unknown_fields`.
//!
//! Precedence: built-in defaults < `~/.config/repo-assist/config.toml` <
//! `<repo_root>/.repo-assist.toml` (Section 10). Unknown keys are an error with a clear
//! message. See Section 10 of the implementation plan.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level configuration (all fields present, defaulting to built-in values).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `auto` = `origin/HEAD`, else `main`, else `master`.
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    /// Paths excluded from overlap / drift checks.
    #[serde(default)]
    pub ignore_paths: Vec<String>,
    /// Thresholds (Section 9).
    #[serde(default = "default_thresholds")]
    pub thresholds: Thresholds,
    /// Source configuration (Section 10).
    #[serde(default = "default_sources")]
    pub sources: Sources,
    /// Markdown view configuration.
    #[serde(default = "default_views")]
    pub views: Views,
}

impl Config {
    /// Built-in default configuration.
    pub fn default_config() -> Self {
        Config {
            base_branch: default_base_branch(),
            ignore_paths: default_ignore_paths(),
            thresholds: default_thresholds(),
            sources: default_sources(),
            views: default_views(),
        }
    }

    /// Load layered configuration: defaults < user config (in `home`) < `<repo_root>` config.
    ///
    /// Returns a human-readable error string on invalid TOML or unknown keys.
    pub fn load(repo_root: &Path, home: &str) -> Result<Self, String> {
        let mut raw = RawConfig::default();
        if let Some(cfg) = user_config(home) {
            if cfg.is_file() {
                let text =
                    std::fs::read_to_string(&cfg).map_err(|e| format!("reading {cfg:?}: {e}"))?;
                let layer: RawConfig = toml::from_str(&text).map_err(to_config_error)?;
                raw = merge_raw(raw, layer);
            }
        }
        let repo_cfg = repo_root.join(".repo-assist.toml");
        if repo_cfg.is_file() {
            let text = std::fs::read_to_string(&repo_cfg)
                .map_err(|e| format!("reading {repo_cfg:?}: {e}"))?;
            let layer: RawConfig = toml::from_str(&text).map_err(to_config_error)?;
            raw = merge_raw(raw, layer);
        }
        Ok(from_raw(&raw))
    }
}

/// Fully-optional view of the config used to merge layers; any absent field contributes
/// nothing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
    #[serde(default)]
    thresholds: Option<Thresholds>,
    #[serde(default)]
    sources: Option<Sources>,
    #[serde(default)]
    views: Option<Views>,
}

fn merge_raw(a: RawConfig, b: RawConfig) -> RawConfig {
    RawConfig {
        base_branch: b.base_branch.or(a.base_branch),
        ignore_paths: b.ignore_paths.or(a.ignore_paths),
        thresholds: b.thresholds.or(a.thresholds),
        sources: b.sources.or(a.sources),
        views: b.views.or(a.views),
    }
}

fn from_raw(raw: &RawConfig) -> Config {
    Config {
        base_branch: raw.base_branch.clone().unwrap_or_else(default_base_branch),
        ignore_paths: raw
            .ignore_paths
            .clone()
            .unwrap_or_else(default_ignore_paths),
        thresholds: raw.thresholds.clone().unwrap_or_else(default_thresholds),
        sources: raw.sources.clone().unwrap_or_else(default_sources),
        views: raw.views.clone().unwrap_or_else(default_views),
    }
}

fn user_config(home: &str) -> Option<PathBuf> {
    if home.is_empty() {
        None
    } else {
        Some(
            Path::new(home)
                .join(".config")
                .join("repo-assist")
                .join("config.toml"),
        )
    }
}

fn to_config_error<E: std::fmt::Display>(e: E) -> String {
    format!("invalid config: {e}")
}

/// Thresholds (Section 9).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// Worktrees with no activity for this many hours trigger `worktree.abandoned`.
    #[serde(default = "default_abandoned_hours")]
    pub abandoned_hours: u64,
    /// Active plan with open tasks and no activity for this many hours triggers `plan.stalled`.
    #[serde(default = "default_stalled_hours")]
    pub stalled_hours: u64,
    /// Window (count) of last tool calls inspected by `agent.loop`.
    #[serde(default = "default_loop_window")]
    pub loop_window: u64,
    /// Same `(tool, args_hash)` occurrences that trip `agent.loop`.
    #[serde(default = "default_loop_repeats")]
    pub loop_repeats: u64,
    /// Session token total (0 = disabled).
    #[serde(default = "default_session_token_budget")]
    pub session_token_budget: u64,
    /// Session considered `recent` while its last event is within this many seconds.
    #[serde(default = "default_session_quiet_secs")]
    pub session_quiet_secs: u64,
}

/// Source configuration (Section 10).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    /// Git source (backstop poll interval).
    #[serde(default = "default_git_source")]
    pub git: GitSource,
    /// Plan-source configuration.
    #[serde(default = "default_plans_source")]
    pub plans: PlansSource,
    /// pi adapter configuration.
    #[serde(default = "default_pi_source")]
    pub pi: PiSource,
}

/// Git source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSource {
    /// Polling backstop interval in seconds.
    #[serde(default = "default_git_poll_secs")]
    pub poll_secs: u64,
}

/// Plan-source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlansSource {
    /// superpowers plan globs.
    #[serde(default = "default_superpowers_globs")]
    pub superpowers_globs: Vec<String>,
    /// checklist (generic) globs.
    #[serde(default = "default_checklist_globs")]
    pub checklist_globs: Vec<String>,
}

/// pi adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiSource {
    /// Whether the pi adapter is enabled.
    #[serde(default = "default_pi_enabled")]
    pub enabled: bool,
    /// Explicit sessions dir (empty = auto).
    #[serde(default)]
    pub sessions_dir: String,
}

/// Markdown view configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Views {
    /// Whether markdown rendering is enabled.
    #[serde(default = "default_views_enabled")]
    pub enabled: bool,
    /// View render debounce in milliseconds.
    #[serde(default = "default_views_debounce_ms")]
    pub debounce_ms: u64,
}

/// Built-in defaults (Section 10).
fn default_base_branch() -> String {
    "auto".to_string()
}
fn default_ignore_paths() -> Vec<String> {
    [
        "Cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "**/*.snap",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}
fn default_abandoned_hours() -> u64 {
    48
}
fn default_stalled_hours() -> u64 {
    4
}
fn default_loop_window() -> u64 {
    20
}
fn default_loop_repeats() -> u64 {
    5
}
fn default_session_token_budget() -> u64 {
    0
}
fn default_session_quiet_secs() -> u64 {
    120
}
fn default_git_poll_secs() -> u64 {
    5
}
fn default_superpowers_globs() -> Vec<String> {
    vec!["docs/superpowers/plans/*.md".to_string()]
}
fn default_checklist_globs() -> Vec<String> {
    vec![
        "TODO.md".to_string(),
        "PLAN.md".to_string(),
        "docs/plans/**/*.md".to_string(),
    ]
}
fn default_pi_enabled() -> bool {
    true
}
fn default_views_enabled() -> bool {
    true
}
fn default_views_debounce_ms() -> u64 {
    1000
}

fn default_thresholds() -> Thresholds {
    Thresholds {
        abandoned_hours: default_abandoned_hours(),
        stalled_hours: default_stalled_hours(),
        loop_window: default_loop_window(),
        loop_repeats: default_loop_repeats(),
        session_token_budget: default_session_token_budget(),
        session_quiet_secs: default_session_quiet_secs(),
    }
}
fn default_git_source() -> GitSource {
    GitSource {
        poll_secs: default_git_poll_secs(),
    }
}
fn default_sources() -> Sources {
    Sources {
        git: default_git_source(),
        plans: default_plans_source(),
        pi: default_pi_source(),
    }
}
fn default_plans_source() -> PlansSource {
    PlansSource {
        superpowers_globs: default_superpowers_globs(),
        checklist_globs: default_checklist_globs(),
    }
}
fn default_pi_source() -> PiSource {
    PiSource {
        enabled: default_pi_enabled(),
        sessions_dir: String::new(),
    }
}
fn default_views() -> Views {
    Views {
        enabled: default_views_enabled(),
        debounce_ms: default_views_debounce_ms(),
    }
}
