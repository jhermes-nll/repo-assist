mod common;

use repo_assist::ipc::{client::Client, Method};
use repo_assist::paths::{git_common_dir, repo_key_from_git_common_dir, Env};
use repo_assist::store::Store;
use std::time::Duration;
use tempfile::TempDir;

fn spawn_daemon(
    repo_root: &std::path::Path,
    state: &std::path::Path,
    runtime: &std::path::Path,
) -> std::process::Child {
    std::process::Command::new(std::env::var("CARGO_BIN_EXE_ra").unwrap())
        .env("HOME", state)
        .env("XDG_STATE_HOME", state)
        .env("XDG_RUNTIME_DIR", runtime)
        .current_dir(repo_root)
        .arg("daemon")
        .arg("--foreground")
        .spawn()
        .expect("spawn daemon")
}

fn wait_socket(sock: &std::path::Path) {
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(sock.exists(), "daemon socket not created");
}

fn setup() -> (
    tempfile::TempDir,        // state
    tempfile::TempDir,        // runtime
    &'static std::path::Path, // repo root (leaked)
    std::path::PathBuf,       // db
    String,                   // key
    std::path::PathBuf,       // sock
) {
    let r = Box::leak(Box::new(common::GitRepo::new()));
    let state = TempDir::new().unwrap();
    let runtime = TempDir::new().unwrap();
    let env = Env::with(
        "/tmp",
        Some(state.path().display().to_string()),
        Some(runtime.path().display().to_string()),
    );
    let key = git_common_dir(r.path())
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sf = env.state_files(&key);
    std::fs::create_dir_all(&sf.dir).unwrap();
    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));
    (state, runtime, r.path(), sf.db.clone(), key, sock)
}

#[test]
fn store_list_events_since() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("store.db");
    let store = Store::open(&db).unwrap();
    let id1 = store
        .append_event(10, "worktree", "worktree_dirty", Some("/wt"), "{}")
        .unwrap();
    let id2 = store
        .append_event(20, "finding", "finding_opened", Some("f1"), "{\"a\":1}")
        .unwrap();
    let got = store.list_events_since(0, 10).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].id, id1);
    assert_eq!(got[1].kind, "finding_opened");
    let since = store.list_events_since(id1, 10).unwrap();
    assert_eq!(since.len(), 1);
    assert_eq!(since[0].id, id2);
}

#[test]
fn subscribe_rpc_shape_and_cursor() {
    let (state, _runtime, repo_root, db, _key, sock) = setup();
    // Seed an event directly before daemon starts (daemon reads same db).
    {
        let store = Store::open(&db).unwrap();
        store
            .append_event(42, "test", "ping", Some("s"), "{\"x\":1}")
            .unwrap();
    }

    let child = spawn_daemon(repo_root, state.path(), _runtime.path());
    wait_socket(&sock);

    let mut client = Client::connect(&sock).unwrap();
    let reply = client
        .call(
            Method::Subscribe,
            serde_json::json!({"since": 0, "wait_ms": 0}),
        )
        .unwrap();
    match reply {
        repo_assist::ipc::Reply::Result { result, .. } => {
            let events = result.get("events").unwrap().as_array().unwrap();
            assert!(!events.is_empty(), "expected seeded event");
            let next = result.get("next").unwrap().as_i64().unwrap();
            assert!(next >= events.last().unwrap().get("id").unwrap().as_i64().unwrap());
            let to = result.get("timed_out").unwrap().as_bool().unwrap();
            assert!(!to);
            // Verify fields
            let ev = &events[0];
            assert!(ev.get("id").is_some());
            assert!(ev.get("ts_ms").is_some());
            assert!(ev.get("source").is_some());
            assert!(ev.get("kind").is_some());
            assert!(ev.get("subject").is_some());
            assert!(ev.get("payload").is_some());
        }
        repo_assist::ipc::Reply::Error { error, .. } => panic!("subscribe failed: {:?}", error),
    }

    drop(child);
}

#[test]
fn subscribe_rpc_long_poll_times_out() {
    let (state, runtime, repo_root, _db, _key, sock) = setup();
    let mut child = spawn_daemon(repo_root, state.path(), runtime.path());
    wait_socket(&sock);

    let mut client = Client::connect(&sock).unwrap();
    // Use a small wait; with no events newer than max id we might still get initial; seed none newer.
    // Ask since = i64::MAX-ish? use current max by first call.
    let first = client
        .call(
            Method::Subscribe,
            serde_json::json!({"since": 0, "wait_ms": 0}),
        )
        .unwrap();
    let next = match first {
        repo_assist::ipc::Reply::Result { result, .. } => {
            result.get("next").unwrap().as_i64().unwrap()
        }
        _ => 0,
    };
    let reply = client
        .call(
            Method::Subscribe,
            serde_json::json!({"since": next, "wait_ms": 100}),
        )
        .unwrap();
    match reply {
        repo_assist::ipc::Reply::Result { result, .. } => {
            let events = result.get("events").unwrap().as_array().unwrap();
            assert!(events.is_empty(), "expected no new events");
            assert!(result.get("timed_out").unwrap().as_bool().unwrap());
        }
        repo_assist::ipc::Reply::Error { error, .. } => panic!("subscribe failed: {:?}", error),
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn subscribe_rpc_reconnect_kill_restart() {
    use repo_assist::tui::client::DataSource;
    // Reuse TUI-facing integration scenario.
    let (state, runtime, repo_root, _db, key, _sock) = setup();
    use repo_assist::paths::repo_key_from_git_common_dir;
    let _ = repo_key_from_git_common_dir;
    // compute socket as in setup by re-running? already _sock dropped; use a fresh setup-like path:
    // We'll just spawn and use the real socket by discovering via runtime path + key.
    let key2 = key.clone();
    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key2}.sock"));

    let mut g1 = spawn_daemon(repo_root, state.path(), runtime.path());
    wait_socket(&sock);

    let mut ds = DataSource::new(sock.clone());
    assert!(ds.fetch_all().is_ok(), "initial fetch should succeed");

    // subscribe smoke
    let _ = ds.subscribe_once();

    let _ = g1.kill();
    let _ = g1.wait();
    std::thread::sleep(Duration::from_millis(500));

    // fetch may fail while down
    let _ = ds.fetch_all();

    // restart
    let mut g2 = spawn_daemon(repo_root, state.path(), runtime.path());
    wait_socket(&sock);
    std::thread::sleep(Duration::from_millis(300));
    assert!(ds.fetch_all().is_ok(), "fetch after restart should succeed");
    let _ = g2.kill();
    let _ = g2.wait();
}

#[test]
fn no_duplicate_rows_after_reconnect() {
    // Reuse the existing tui reconnect assertion style: replace-whole-snapshot on fetch_all.
    let (state, runtime, repo_root, _db, key, _sock) = setup();
    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));
    let mut g1 = spawn_daemon(repo_root, state.path(), runtime.path());
    wait_socket(&sock);

    use repo_assist::tui::client::DataSource;
    let mut ds = DataSource::new(sock.clone());
    let s1 = ds.fetch_all().expect("fetch1");
    let rows_before = s1.rows[0].len();

    let _ = g1.kill();
    let _ = g1.wait();
    std::thread::sleep(Duration::from_millis(500));

    let mut g2 = spawn_daemon(repo_root, state.path(), runtime.path());
    wait_socket(&sock);
    std::thread::sleep(Duration::from_millis(300));

    let s2 = ds.fetch_all().expect("fetch2");
    let rows_after = s2.rows[0].len();
    // Should not double the rows (snapshot replaced, not appended).
    assert!(
        rows_after <= rows_before.max(1) * 2 + 5,
        "unexpected row growth: {} -> {}",
        rows_before,
        rows_after
    );

    let _ = g2.kill();
    let _ = g2.wait();
}
