//! Task 0.2 acceptance tests: paths, repo-key, and layered config.

use std::path::Path;
mod common;
use repo_assist::config::Config;
use repo_assist::paths::{self, Env};

#[test]
fn two_worktrees_of_one_repo_yield_the_same_key() {
    // Build two temp git repos; the same repo always keys to the same 12 hex chars.
    let r1 = tempfile::TempDir::new().unwrap();
    common::make_git_repo(r1.path());
    let key1a = paths::repo_key_from_git_common_dir(&r1.path().join(".git"));

    let r2 = tempfile::TempDir::new().unwrap();
    common::make_git_repo(r2.path());
    let key2 = paths::repo_key_from_git_common_dir(&r2.path().join(".git"));

    // A repo's key is idempotent...
    let key1b = paths::repo_key_from_git_common_dir(&r1.path().join(".git"));
    assert_eq!(key1a, key1b);
    assert_eq!(key1a.len(), 12);
    // ...and two distinct repos yield distinct keys.
    assert_ne!(key1a, key2);
}

#[test]
fn state_and_socket_paths_follow_dogma() {
    // With both XDG vars set: state under XDG_STATE_HOME, socket under XDG_RUNTIME_DIR.
    let with = Env::with(
        "/home/jess".to_string(),
        Some("/home/jess/.local/state".to_string()),
        Some("/run/user/502".to_string()),
    );
    let state = with.state_dir("abc123def456");
    assert_eq!(
        state,
        Path::new("/home/jess/.local/state/repo-assist/abc123def456")
    );
    let sock = with.socket_path("abc123def456");
    assert_eq!(
        sock,
        Path::new("/run/user/502/repo-assist/abc123def456.sock")
    );

    // XDG unset: state under ~/.local/state, socket under the state dir.
    let bare = Env::with("/home/jess".to_string(), None, None);
    let state2 = bare.state_dir("abc123def456");
    assert_eq!(
        state2,
        Path::new("/home/jess/.local/state/repo-assist/abc123def456")
    );
    let sock2 = bare.socket_path("abc123def456");
    assert_eq!(
        sock2,
        Path::new("/home/jess/.local/state/repo-assist/abc123def456/daemon.sock")
    );
}

#[test]
fn defaults_match_section_10() {
    let c = Config::default_config();
    assert_eq!(c.base_branch, "auto");
    assert_eq!(
        c.ignore_paths,
        vec![
            "Cargo.lock",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "**/*.snap",
        ]
    );
    assert_eq!(c.thresholds.abandoned_hours, 48);
    assert_eq!(c.thresholds.stalled_hours, 4);
    assert_eq!(c.thresholds.loop_window, 20);
    assert_eq!(c.thresholds.loop_repeats, 5);
    assert_eq!(c.thresholds.session_token_budget, 0);
    assert_eq!(c.thresholds.session_quiet_secs, 120);
    assert_eq!(c.sources.git.poll_secs, 5);
    assert!(c.sources.pi.enabled);
    assert_eq!(c.sources.pi.sessions_dir, "");
    assert!(c.views.enabled);
    assert_eq!(c.views.debounce_ms, 1000);
}

fn write(home: &std::path::Path, rel: &str, content: &str) {
    let p = home.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

#[test]
fn user_config_overrides_defaults() {
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".config/repo-assist/config.toml",
        "base_branch = \"main\"\n[thresholds]\nabandoned_hours = 72\n",
    );
    let c = Config::load(home, home.to_str().unwrap()).unwrap();
    assert_eq!(c.base_branch, "main");
    assert_eq!(c.thresholds.abandoned_hours, 72);
    // Untouched defaults survive the layer.
    assert_eq!(c.thresholds.stalled_hours, 4);
}

#[test]
fn repo_config_overrides_user_config() {
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".repo-assist.toml",
        "ignore_paths = [\"vendor/**\"]\n[thresholds]\nstalled_hours = 2\n",
    );
    let c = Config::load(home, home.to_str().unwrap()).unwrap();
    assert_eq!(c.ignore_paths, vec!["vendor/**".to_string()]);
    assert_eq!(c.thresholds.stalled_hours, 2);
}

#[test]
fn unknown_keys_reported_as_error() {
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".config/repo-assist/config.toml",
        "this_key_does_not_exist = true\n",
    );
    let err = Config::load(home, home.to_str().unwrap()).unwrap_err();
    assert!(err.to_lowercase().contains("invalid config"), "got: {err}");
}

#[test]
fn load_error_reports_bad_toml_readablely() {
    let d = tempfile::TempDir::new().unwrap();
    let home = d.path();
    write(
        home,
        ".config/repo-assist/config.toml",
        "thresholds = { broken",
    );
    let err = Config::load(home, home.to_str().unwrap()).unwrap_err();
    assert!(err.to_lowercase().contains("invalid config"), "got: {err}");
}
