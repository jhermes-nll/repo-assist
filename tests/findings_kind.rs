mod common;

use repo_assist::paths::Env;
use repo_assist::store::{FindingCandidate, Store};

struct DaemonGuard(std::process::Child);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn findings_kind_filters_over_ipc() {
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
        .unwrap_or_else(|| "manual".to_string());
    let sf = env.state_files(&key);
    std::fs::create_dir_all(&sf.dir).unwrap();
    let store = Store::open(&sf.db).unwrap();
    let c1 = FindingCandidate::new(
        "plan.drift",
        "k1",
        "k1",
        "k1",
        "warn",
        serde_json::json!({}),
        "explicit",
    );
    let c2 = FindingCandidate::new(
        "worktree.overlap",
        "k2",
        "k2",
        "k2",
        "warn",
        serde_json::json!({}),
        "explicit",
    );
    store.sync_findings(vec![c1, c2], 1000).unwrap();

    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    use std::process::Stdio;
    let cmd = std::process::Command::new(&ra_bin)
        .arg("daemon")
        .arg("--foreground")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _g = DaemonGuard(cmd);

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

    let out = assert_cmd::Command::cargo_bin("ra")
        .unwrap()
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .arg("findings")
        .arg("--kind")
        .arg("plan.drift")
        .arg("--json")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(out.status.success(), "exit: {:?}", out.status);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v["findings"].as_array().unwrap();
    assert!(arr.iter().any(|f| f["kind"] == "plan.drift"));
    assert!(!arr.iter().any(|f| f["kind"] == "worktree.overlap"));
}
