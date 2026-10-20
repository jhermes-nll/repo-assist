mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

use repo_assist::tui::app::{Action, App, Row, Tab};
use repo_assist::tui::client::{DataSource, Snapshot};
use repo_assist::tui::ui::draw;

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn render(app: &App) -> String {
    let backend = TestBackend::new(70, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    format!("{}", terminal.backend())
}

#[test]
fn tui_worktrees_tab_renders() {
    let mut app = App::new();
    app.repo_key = "repo-key".into();
    app.daemon_version = "0.1.0".into();
    app.rows[Tab::Worktrees.index()] = vec!["wt-a".into(), "wt-b".into()];
    app.tab = Tab::Worktrees;
    let view = render(&app);
    assert!(view.contains("repo: repo-key"));
    assert!(view.contains("Worktrees"));
    assert!(view.contains("wt-a"));
    assert!(view.contains("wt-b"));
}

#[test]
fn tui_plans_tab_renders() {
    let mut app = App::new();
    app.repo_key = "repo-key".into();
    app.rows[Tab::Plans.index()] = vec!["Plan A".into()];
    app.tab = Tab::Plans;
    let view = render(&app);
    assert!(view.contains("Plans"));
    assert!(view.contains("Plan A"));
}

#[test]
fn tui_findings_tab_renders() {
    let mut app = App::new();
    app.rows[Tab::Findings.index()] = vec!["finding-1".into()];
    app.tab = Tab::Findings;
    let view = render(&app);
    assert!(view.contains("Findings"));
    assert!(view.contains("finding-1"));
}

#[test]
fn tui_sessions_tab_renders() {
    let mut app = App::new();
    app.rows[Tab::Sessions.index()] = vec!["sess-1".into()];
    app.tab = Tab::Sessions;
    let view = render(&app);
    assert!(view.contains("Sessions"));
    assert!(view.contains("sess-1"));
}

#[test]
fn tui_tab_switching() {
    let mut app = App::new();
    assert_eq!(app.tab, Tab::Worktrees);

    assert_eq!(
        app.on_key(key(KeyCode::Char('2'))),
        Some(Action::SelectTab(Tab::Plans))
    );
    assert_eq!(app.tab, Tab::Plans);

    assert_eq!(app.on_key(key(KeyCode::Tab)), Some(Action::NextTab));
    assert_eq!(app.tab, Tab::Findings);

    assert_eq!(
        app.on_key(key(KeyCode::Char('4'))),
        Some(Action::SelectTab(Tab::Sessions))
    );
    assert_eq!(app.tab, Tab::Sessions);

    // wrap around
    app.on_key(key(KeyCode::Tab));
    assert_eq!(app.tab, Tab::Worktrees);
}

#[test]
fn tui_cursor_moves_and_clamps() {
    let mut app = App::new();
    app.rows[Tab::Worktrees.index()] = vec!["a".into(), "b".into(), "c".into()];
    assert_eq!(app.active_cursor(), 0);

    app.on_key(key(KeyCode::Char('j')));
    assert_eq!(app.active_cursor(), 1);
    app.on_key(key(KeyCode::Char('j')));
    assert_eq!(app.active_cursor(), 2);
    // at last end stays
    app.on_key(key(KeyCode::Char('j')));
    assert_eq!(app.active_cursor(), 2);

    app.on_key(key(KeyCode::Char('k')));
    assert_eq!(app.active_cursor(), 1);
    app.on_key(key(KeyCode::Char('k')));
    assert_eq!(app.active_cursor(), 0);
    // at first end stays
    app.on_key(key(KeyCode::Char('k')));
    assert_eq!(app.active_cursor(), 0);
}

#[test]
fn tui_detail_open_and_close() {
    let mut app = App::new();
    app.rows[Tab::Worktrees.index()] = vec!["row".into()];
    assert!(app.detail.is_none());

    assert_eq!(app.on_key(key(KeyCode::Enter)), Some(Action::Enter));
    assert!(app.detail.is_some());

    assert_eq!(app.on_key(key(KeyCode::Esc)), Some(Action::ClearFilter));
    assert!(app.detail.is_none());
}

#[test]
fn tui_ack_returns_action() {
    let mut app = App::new();
    assert_eq!(app.on_key(key(KeyCode::Char('a'))), Some(Action::Ack));
}

#[test]
fn tui_filter_flow_and_persistence() {
    let mut app = App::new();
    app.rows[Tab::Worktrees.index()] = vec!["apple".into(), "banana".into()];

    // enter filter mode
    assert_eq!(
        app.on_key(key(KeyCode::Char('/'))),
        Some(Action::Filter('/'))
    );
    assert!(app.filtering);

    // typing updates filter
    app.on_key(key(KeyCode::Char('a')));
    assert_eq!(app.active_filter(), "a");

    // filter "app" -> exactly 1
    app.filter[Tab::Worktrees.index()] = "app".to_string();
    let fr = app.filtered_rows();
    assert_eq!(fr.len(), 1);

    // case-insensitive "AP" -> 1
    app.filter[Tab::Worktrees.index()] = "AP".to_string();
    let fr2 = app.filtered_rows();
    assert_eq!(fr2.len(), 1);

    // cross-tab isolation: other tabs rows absent
    app.tab = Tab::Plans;
    assert_eq!(app.filtered_rows().len(), 0);
    app.tab = Tab::Worktrees;

    // Esc clears
    app.on_key(key(KeyCode::Esc));
    assert!(!app.filtering);
    assert_eq!(app.active_filter(), "");

    // per-tab: set filter on worktrees
    app.filter[Tab::Worktrees.index()] = "app".to_string();
    app.tab = Tab::Plans;
    assert_eq!(app.active_filter(), "");

    // set filter on plans then switch back: worktrees filter unchanged
    app.filter[Tab::Plans.index()] = "zzz".to_string();
    app.tab = Tab::Worktrees;
    assert_eq!(app.active_filter(), "app");
    app.tab = Tab::Plans;
    assert_eq!(app.active_filter(), "zzz");

    // filter survives a refresh (apply_snapshot does not clear filter)
    let ss = Snapshot::default();
    app.apply_snapshot(&ss);
    assert_eq!(app.filter[Tab::Plans.index()], "zzz");
}

#[test]
fn tui_m3_selection_stays_visible_after_refresh() {
    let mut app = App::new();
    app.tab = Tab::Findings;
    app.filter[Tab::Findings.index()] = "ap".to_string();
    app.rows[Tab::Findings.index()] = vec!["apple".into(), "application".into(), "banana".into()];
    app.clamp_cursor();
    app.on_key(key(KeyCode::Char('j'))); // select "application"
    assert_eq!(
        app.selected_row().map(|r| r.text.as_str()),
        Some("application")
    );
    let mut snap = Snapshot::default();
    snap.rows[Tab::Findings.index()] = vec!["apple".into(), "banana".into()];
    app.apply_snapshot(&snap);
    assert_eq!(app.active_cursor(), 0);
    assert_eq!(app.selected_row().map(|r| r.text.as_str()), Some("apple"));
}

#[test]
fn tui_q_sets_should_quit() {
    let mut app = App::new();
    assert_eq!(app.on_key(key(KeyCode::Char('q'))), Some(Action::Quit));
    assert!(app.should_quit);
}

#[test]
fn tui_non_tty_refused() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert_cmd::Command::cargo_bin("ra")
        .unwrap()
        .current_dir(tmp.path())
        .arg("tui")
        .assert()
        .failure()
        .stderr(predicates::str::contains("interactive terminal"));
}

// Integration: reconnect
struct DaemonGuard(std::process::Child);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_daemon(
    ra_bin: &str,
    repo_root: &std::path::Path,
    state: &std::path::Path,
    runtime: &std::path::Path,
) -> DaemonGuard {
    let cmd = std::process::Command::new(ra_bin)
        .env("HOME", state)
        .env("XDG_STATE_HOME", state)
        .env("XDG_RUNTIME_DIR", runtime)
        .current_dir(repo_root)
        .arg("daemon")
        .arg("--foreground")
        .spawn()
        .expect("spawn daemon");
    DaemonGuard(cmd)
}

fn wait_socket(sock: &std::path::Path) {
    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(sock.exists(), "daemon socket not created");
}

#[test]
fn tui_reconnect_survives_daemon_restart() {
    use repo_assist::paths::repo_key_from_git_common_dir;

    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let runtime = tempfile::TempDir::new().unwrap();
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();

    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());

    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));

    let _g = spawn_daemon(&ra_bin, repo.path(), state.path(), runtime.path());
    wait_socket(&sock);

    let mut ds = DataSource::new(sock.clone());
    assert!(ds.fetch_all().is_ok(), "initial fetch should succeed");

    // kill daemon
    drop(_g);
    std::thread::sleep(std::time::Duration::from_millis(500));
    // fetch should now fail (not panic)
    assert!(ds.fetch_all().is_err(), "fetch after kill should fail");

    // restart
    let _g2 = spawn_daemon(&ra_bin, repo.path(), state.path(), runtime.path());
    wait_socket(&sock);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(ds.fetch_all().is_ok(), "fetch after restart should succeed");
}

#[test]
fn tui_ack_finding_end_to_end() {
    use repo_assist::paths::repo_key_from_git_common_dir;
    use repo_assist::paths::Env;
    use repo_assist::store::{FindingCandidate, FindingsFilter, Store};

    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let runtime = tempfile::TempDir::new().unwrap();
    let env = Env::with(
        "/tmp",
        Some(state.path().display().to_string()),
        Some(runtime.path().display().to_string()),
    );
    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sf = env.state_files(&key);
    std::fs::create_dir_all(&sf.dir).unwrap();
    let store = Store::open(&sf.db).unwrap();
    let c = FindingCandidate::new(
        "plan.drift",
        "k1",
        "k1",
        "k1",
        "warn",
        serde_json::json!({}),
        "explicit",
    );
    store.sync_findings(vec![c], 1000).unwrap();
    let rows0 = store
        .list_findings(&FindingsFilter {
            open: true,
            all: false,
            kind: None,
        })
        .unwrap();
    assert!(!rows0.is_empty());
    let expected_id = rows0[0].id.to_string();

    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));

    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let _g = spawn_daemon(&ra_bin, repo.path(), state.path(), runtime.path());
    wait_socket(&sock);

    let mut ds = DataSource::new(sock.clone());
    let snap = ds.fetch_all().expect("fetch");
    let frows = &snap.rows[repo_assist::tui::app::Tab::Findings.index()];
    assert!(!frows.is_empty(), "expected at least one finding row");
    assert_eq!(frows[0].id, expected_id, "row id should be real finding id");

    // ack through TUI client path
    ds.ack_finding(&frows[0].id).expect("ack ok");

    // fetch again, state acked
    let snap2 = ds.fetch_all().expect("fetch2");
    // need daemon finding state; query store direct
    let store2 = Store::open(&sf.db).unwrap();
    let rows1 = store2
        .list_findings(&FindingsFilter {
            open: false,
            all: true,
            kind: None,
        })
        .unwrap();
    assert!(rows1
        .iter()
        .any(|r| r.id.to_string() == expected_id && r.state == "acked"));
    let _ = snap2;
}

#[test]
fn tui_ack_finding_bogus_fails() {
    use repo_assist::paths::repo_key_from_git_common_dir;
    use repo_assist::paths::Env;
    use repo_assist::store::{FindingCandidate, Store};

    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let runtime = tempfile::TempDir::new().unwrap();
    let env = Env::with(
        "/tmp",
        Some(state.path().display().to_string()),
        Some(runtime.path().display().to_string()),
    );
    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sf = env.state_files(&key);
    std::fs::create_dir_all(&sf.dir).unwrap();
    let store = Store::open(&sf.db).unwrap();
    let c = FindingCandidate::new(
        "plan.drift",
        "k1",
        "k1",
        "k1",
        "warn",
        serde_json::json!({}),
        "explicit",
    );
    store.sync_findings(vec![c], 1000).unwrap();

    let sock = runtime
        .path()
        .join("repo-assist")
        .join(format!("{key}.sock"));
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();
    let _g = spawn_daemon(&ra_bin, repo.path(), state.path(), runtime.path());
    wait_socket(&sock);
    let mut ds = DataSource::new(sock.clone());
    let _ = ds.fetch_all();
    assert!(ds.ack_finding("999999").is_err(), "bogus ack should fail");
}

#[test]
fn tui_detail_survives_refresh_same_row() {
    use repo_assist::tui::client::Snapshot;
    let mut app = App::new();
    app.repo_key = "repo-key".into();
    let row = Row::with_id("id-1".to_string(), "row one".to_string());
    app.rows[Tab::Worktrees.index()] = vec![row.clone()];
    app.tab = Tab::Worktrees;
    app.cursor[0] = 0;
    app.on_key(key(KeyCode::Enter));
    assert!(app.detail.is_some());

    let mut snap = Snapshot::default();
    snap.rows[Tab::Worktrees.index()] = vec![row];
    app.apply_snapshot(&snap);
    assert!(
        app.detail.is_some(),
        "detail should survive refresh with same row"
    );

    // tab switch clears
    app.on_key(key(KeyCode::Tab));
    assert!(app.detail.is_none());

    // absent row clears
    let mut app2 = App::new();
    app2.rows[Tab::Worktrees.index()] =
        vec![Row::with_id("id-1".to_string(), "row one".to_string())];
    app2.tab = Tab::Worktrees;
    app2.cursor[0] = 0;
    app2.on_key(key(KeyCode::Enter));
    app2.apply_snapshot(&Snapshot::default());

    assert!(app2.detail.is_none());
}

#[test]
fn tui_detail_renders_in_buffer() {
    let mut app = App::new();
    app.repo_key = "repo-key".into();
    app.rows[Tab::Worktrees.index()] =
        vec![Row::with_id("id-1".to_string(), "row one".to_string())];
    app.tab = Tab::Worktrees;
    app.cursor[0] = 0;
    app.on_key(key(KeyCode::Enter));
    let view = render(&app);
    assert!(view.contains("DETAIL:"));
    assert!(view.contains("row one"));
}

#[test]
fn tui_worktrees_sessions_headers() {
    let mut app = App::new();
    app.repo_key = "repo-key".into();
    let mut w = Row::with_id("p".to_string(), "p".to_string());
    w.cells = vec!["/tmp/wt".into(), "main".into(), "3".into()];
    app.rows[Tab::Worktrees.index()] = vec![w];
    app.tab = Tab::Worktrees;
    let view = render(&app);
    assert!(view.contains("path | branch | changed"));
    assert!(view.contains("main"));

    let mut app2 = App::new();
    let mut s = Row::with_id("sess-1".to_string(), "sess-1".to_string());
    s.cells = vec!["sess-1".into(), "claude".into(), "123".into()];
    app2.rows[Tab::Sessions.index()] = vec![s];
    app2.tab = Tab::Sessions;
    let view2 = render(&app2);
    assert!(view2.contains("id | agent | last_event_ms"));
    assert!(view2.contains("claude"));
}
