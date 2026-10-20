//! Environment checks (Task 2 / §9).

use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::ipc::client::Client;
use crate::paths::Env;
use crate::sources::sessions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

pub fn run_all(env: &Env, repo_path: &Path, cfg_result: Result<&Config, &str>) -> Vec<Check> {
    let Some(common) = crate::paths::git_common_dir(repo_path) else {
        return vec![Check {
            name: "git repository",
            status: CheckStatus::Fail,
            detail: format!("not a git repository: {}", repo_path.display()),
        }];
    };
    let repo_key = crate::paths::repo_key_from_git_common_dir(&common);
    let cfg = match cfg_result {
        Ok(c) => c,
        Err(_) => &Config::default_config(),
    };
    let config_check = match cfg_result {
        Ok(_) => Check {
            name: "config validity",
            status: CheckStatus::Ok,
            detail: "loaded (or defaults)".to_string(),
        },
        Err(msg) => Check {
            name: "config validity",
            status: CheckStatus::Fail,
            detail: msg.to_string(),
        },
    };
    vec![
        git_version_check(),
        state_dir_writable(env, &repo_key),
        socket_reachable(env, &repo_key),
        adapter_roots(env, cfg),
        config_check,
    ]
}

pub fn git_version_from_output(version_output: &str) -> Check {
    let parsed = parse_git_version(version_output);
    git_version_check_result(parsed, version_output)
}

pub fn parse_git_version(s: &str) -> Option<(u32, u32)> {
    // look for "git version x.y..."
    let s = s.trim();
    let after = s.split("git version").nth(1)?.trim();
    let mut parts = after.split_whitespace().next()?.split('.');
    let maj: u32 = parts.next()?.parse().ok()?;
    let min: u32 = parts.next()?.parse().ok()?;
    Some((maj, min))
}

fn git_version_check() -> Check {
    let output = std::process::Command::new("git").arg("--version").output();
    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            git_version_from_output(&text)
        }
        Err(e) => Check {
            name: "git version",
            status: CheckStatus::Fail,
            detail: format!("could not run git: {e}"),
        },
    }
}

fn git_version_check_result(parsed: Option<(u32, u32)>, raw: &str) -> Check {
    match parsed {
        Some((maj, min)) if (maj, min) >= (2, 38) => Check {
            name: "git version",
            status: CheckStatus::Ok,
            detail: format!("git {maj}.{min}"),
        },
        Some((maj, min)) => Check {
            name: "git version",
            status: CheckStatus::Warn,
            detail: format!(
                "git {maj}.{min}; `worktree.merge_conflict` is disabled: needs git ≥ 2.38 for `git merge-tree --write-tree`"
            ),
        },
        None => Check {
            name: "git version",
            status: CheckStatus::Warn,
            detail: format!(
                "unparseable git version: {}; `worktree.merge_conflict` is disabled: needs git ≥ 2.38 for `git merge-tree --write-tree`",
                raw.trim()
            ),
        },
    }
}

fn state_dir_writable(env: &Env, repo_key: &str) -> Check {
    let dir = env.state_dir(repo_key);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Check {
            name: "state directory writable",
            status: CheckStatus::Fail,
            detail: format!("cannot create {}: {e}", dir.display()),
        };
    }
    let probe = dir.join(".doctor-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Check {
                name: "state directory writable",
                status: CheckStatus::Ok,
                detail: "writable".to_string(),
            }
        }
        Err(e) => Check {
            name: "state directory writable",
            status: CheckStatus::Fail,
            detail: format!("not writable: {e}"),
        },
    }
}

fn socket_reachable(env: &Env, repo_key: &str) -> Check {
    let sock = env.socket_path(repo_key);
    match Client::connect(sock.clone()) {
        Ok(mut c) => match c.call(crate::ipc::Method::Ping, serde_json::Value::Null) {
            Ok(_) => Check {
                name: "socket reachable",
                status: CheckStatus::Ok,
                detail: "reachable".to_string(),
            },
            Err(e) => Check {
                name: "socket reachable",
                status: CheckStatus::Fail,
                detail: format!("ping failed: {e}"),
            },
        },
        Err(crate::ipc::client::IpcError::NotRunning(_)) => Check {
            name: "socket reachable",
            status: CheckStatus::Warn,
            detail: "no daemon running".to_string(),
        },
        Err(e) => Check {
            name: "socket reachable",
            status: CheckStatus::Warn,
            detail: format!("no daemon running: {e}"),
        },
    }
}

fn adapter_roots(env: &Env, cfg: &Config) -> Check {
    let root = if !cfg.sources.pi.sessions_dir.is_empty() {
        let d = &cfg.sources.pi.sessions_dir;
        if let Some(rest) = d.strip_prefix("~/") {
            std::path::PathBuf::from(format!("{}/{}", env.home.trim_end_matches('/'), rest))
        } else {
            std::path::PathBuf::from(d)
        }
    } else {
        sessions::resolve_sessions_root(env)
    };
    if !root.exists() {
        return Check {
            name: "adapter roots",
            status: CheckStatus::Warn,
            detail: format!("absent: {}", root.display()),
        };
    }
    match root.read_dir() {
        Ok(_) => Check {
            name: "adapter roots",
            status: CheckStatus::Ok,
            detail: format!("{}", root.display()),
        },
        Err(e) => Check {
            name: "adapter roots",
            status: CheckStatus::Fail,
            detail: format!("unreadable: {e}"),
        },
    }
}
