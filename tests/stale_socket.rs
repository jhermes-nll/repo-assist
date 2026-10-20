mod common;

use repo_assist::paths::git_common_dir;
use repo_assist::paths::{repo_key_from_git_common_dir, Env};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixListener;
use tempfile::TempDir;

#[test]
fn stale_socket_auto_recovers() {
    let repo = common::GitRepo::new();
    let state = TempDir::new().unwrap();
    let runtime = TempDir::new().unwrap();
    let env = Env::with(
        "/tmp",
        Some(state.path().display().to_string()),
        Some(runtime.path().display().to_string()),
    );
    let key = git_common_dir(repo.path())
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sf = env.state_files(&key);
    std::fs::create_dir_all(&sf.dir).unwrap();

    let sock = env.socket_path(&key);
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    // create stale socket: bind, then forget listener so file survives and connection is refused
    let listener = UnixListener::bind(&sock).unwrap();
    let fd = listener.as_raw_fd();
    std::mem::forget(listener);
    extern "C" {
        fn close(fd: i32) -> i32;
    }
    unsafe {
        close(fd);
    }
    assert!(sock.exists(), "stale socket file must exist (precondition)");
    // verify connecting is refused
    let refused = std::os::unix::net::UnixStream::connect(&sock).is_err();
    assert!(
        refused,
        "connecting to stale socket must be refused (precondition)"
    );

    let out = assert_cmd::Command::cargo_bin("ra")
        .unwrap()
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .arg("status")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ra status should succeed past stale socket: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // cleanup daemon
    let _ = std::process::Command::new("pkill")
        .arg("-f")
        .arg("ra daemon")
        .status();
}
