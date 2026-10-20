mod common;

use repo_assist::doctor::{git_version_from_output, parse_git_version, CheckStatus};
use repo_assist::paths::Env;

#[test]
fn git_version_comparator_table() {
    // below
    let c = git_version_from_output("git version 2.9.0");
    assert_eq!(c.status, CheckStatus::Warn);
    // above/boundary
    for v in [
        "git version 2.38.0",
        "git version 2.100.0",
        "git version 3.0.0",
    ] {
        let c = git_version_from_output(v);
        assert_eq!(c.status, CheckStatus::Ok, "{}", v);
    }
    // unparseable
    let c = git_version_from_output("not a version");
    assert_eq!(c.status, CheckStatus::Warn);
    // parse_git_version
    assert_eq!(parse_git_version("git version 2.38.0"), Some((2, 38)));
    assert_eq!(parse_git_version("garbage"), None);
}

#[test]
fn merge_conflict_disabled_explanation() {
    let c = git_version_from_output("git version 2.37.0");
    assert_eq!(c.status, CheckStatus::Warn);
    assert!(c.detail.contains("worktree.merge_conflict"));
    assert!(c.detail.contains("needs git"));
}

#[test]
fn failing_check_exits_one() {
    let repo = common::GitRepo::new();
    // Point XDG_STATE_HOME at a file so state dir unwritable
    let blocker = tempfile::NamedTempFile::new().unwrap();
    // Use path as XDG_STATE_HOME (a file) -> create_dir_all fails
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let out = std::process::Command::new(&ra_bin)
        .arg("doctor")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", blocker.path())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected non-zero exit");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn warn_exits_zero() {
    // No daemon running => socket check Warn; should still exit 0
    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let runtime = tempfile::TempDir::new().unwrap();
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let out = std::process::Command::new(&ra_bin)
        .arg("doctor")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .output()
        .unwrap();
    // may have fail only if state dir? should be ok
    // allow exit 0
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn all_ok_exits_zero() {
    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let runtime = tempfile::TempDir::new().unwrap();
    let env = Env::with(
        "/tmp",
        Some(state.path().display().to_string()),
        Some(runtime.path().display().to_string()),
    );
    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_assist::paths::repo_key_from_git_common_dir(&c))
        .unwrap();
    let sf = env.state_files(&key);
    std::fs::create_dir_all(&sf.dir).unwrap();

    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    // start daemon
    let cmd = std::process::Command::new(&ra_bin)
        .arg("daemon")
        .arg("--foreground")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .spawn()
        .unwrap();
    struct G(std::process::Child);
    impl Drop for G {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _g = G(cmd);
    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));
    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(sock.exists());

    let out = std::process::Command::new(&ra_bin)
        .arg("doctor")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn json_shape() {
    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let out = std::process::Command::new(&ra_bin)
        .arg("doctor")
        .arg("--json")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .current_dir(repo.path())
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("bare array");
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert!(first.get("name").is_some());
    assert!(first.get("status").is_some());
    assert!(first.get("detail").is_some());
}

#[test]
fn invalid_config_exits_one_and_names_file() {
    let repo = common::GitRepo::new();
    std::fs::write(repo.path().join(".repo-assist.toml"), "unknown_key = 1\n").unwrap();
    let state = tempfile::TempDir::new().unwrap();
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let out = std::process::Command::new(&ra_bin)
        .arg("doctor")
        .arg("--json")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("bare array");
    let cfg = arr.iter().find(|c| c["name"] == "config validity").unwrap();
    assert_eq!(cfg["status"], "fail");
    assert!(cfg["detail"]
        .as_str()
        .unwrap()
        .contains(".repo-assist.toml"));
}
