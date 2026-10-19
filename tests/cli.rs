//! Acceptance for Task 1.2: IPC server/client, `ra daemon`, `ra status`, pidfile flock,
//! stale-socket cleanup, socket 0600, graceful shutdown, auto-spawn.

mod common;

use assert_cmd::Command;
use common::GitRepo;

/// `RA_DOCTOR_ENV`-style env isolation: point HOME/XDG dirs at the repo temp dir so tests
/// never touch the real `~/.local/state`.
fn isolate(repo_path: &std::path::Path) -> Vec<(String, String)> {
    let state = repo_path.join("state");
    let runtime = repo_path.join("run");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    vec![
        ("HOME".into(), state.display().to_string()),
        ("XDG_STATE_HOME".into(), state.display().to_string()),
        ("XDG_RUNTIME_DIR".into(), runtime.display().to_string()),
    ]
}

fn ra(envs: &[(String, String)]) -> Command {
    let mut cmd = Command::cargo_bin("ra").unwrap();
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd
}

#[test]
fn daemon_roundtrip_status_and_second_daemon_fails() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let envs = isolate(repo.path());

    // 1. Client command auto-spawns a daemon and answers `status` end to end.
    let out = ra(&envs)
        .arg("status")
        .arg("--json")
        .current_dir(repo.path())
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ra status failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let status: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("ra status --json should print a JSON object");
    // Worktrees are populated by the Task 2.1 git source; Task 1.2 only asserts the shell:
    // a stable envelope with a 12-char repo key and an (empty) worktree list.
    assert!(status["repo_key"]
        .as_str()
        .map(|s| s.len() == 12)
        .unwrap_or(false));
    assert!(status["worktrees"].is_array());

    // 2. The socket file exists with mode 0600.
    let key = status["repo_key"].as_str().unwrap();
    let sock = repo
        .path()
        .join("run")
        .join("repo-assist")
        .join(format!("{key}.sock"));
    assert!(sock.exists(), "socket missing at {}", sock.display());
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "socket mode {:o}", mode);

    // 3. A second daemon for the same repo fails fast with a clear message.
    let mut second = ra(&envs);
    second
        .arg("daemon")
        .arg("--foreground")
        .current_dir(repo.path())
        .timeout(std::time::Duration::from_secs(10));
    let out2 = second.output().unwrap();
    assert!(!out2.status.success(), "second daemon should exit non-zero");
    let msg = format!(
        "{}{}",
        String::from_utf8_lossy(&out2.stdout),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        msg.contains("already running"),
        "second-daemon error should mention the live daemon, got: {msg}"
    );

    // 4. Client-driven graceful shutdown of the auto-spawned daemon.
    let out3 = ra(&envs)
        .arg("status")
        .arg("--no-spawn")
        .arg("--json")
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(
        out3.status.success(),
        "no-spawn status failed: {}",
        String::from_utf8_lossy(&out3.stderr)
    );

    // There is no `ra shutdown` CLI yet; the shutdown method is exercised via the raw client.
    // (Method::Shutdown exists; CLI surface lands in a later task. Kill via the pidfile.)
    // Ask the daemon to shut down through the IPC `shutdown` method instead of a signal.
    let reply = repo_assist::ipc::client::Client::connect(sock.clone())
        .and_then(|mut c| c.call(repo_assist::ipc::Method::Shutdown, serde_json::Value::Null))
        .expect("shutdown request over IPC");
    match reply {
        repo_assist::ipc::Reply::Result { .. } => {}
        other => panic!("expected result for shutdown, got {other:?}"),
    }

    // The daemon exits on its own shortly after the shutdown request; the accept loop
    // removes the socket file at exit, so its disappearance is the cross-platform signal
    // (a /proc/<pid> existence check does not work on macOS).
    let mut gone = false;
    for _ in 0..50 {
        if !sock.exists() {
            gone = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        gone,
        "daemon did not shut down (socket still present) after the shutdown request"
    );
}
