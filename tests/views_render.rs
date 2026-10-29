mod common;

use repo_assist::paths::Env;
use repo_assist::views::write::write_views;
use repo_assist::views::{render_findings, render_plans, render_status, ViewData, VIEW_CAP_LINES};
use serde_json::json;

fn sample_data() -> ViewData {
    ViewData {
        status: json!({"worktree_count":1,"active_plans":1,"open_findings":1,"sessions":0,"worktrees":[]}),
        plans: json!({"plans":[{"id":"p1","title":"Plan","state":"active","declared_done":0,"task_count":1}]}),
        findings: json!({"findings":[
            {"kind":"k.open","severity":"high","subject":"s1","summary":"sum1","state":"open"},
            {"kind":"k.resolved","severity":"low","subject":"s2","summary":"sum2","state":"resolved"}
        ]}),
        sessions: json!({"sessions":[]}),
    }
}

#[test]
fn all_three_views_render_header_first() {
    let d = sample_data();
    let now = 1_700_000_000_000i64;
    for rendered in [
        render_status(&d, now),
        render_plans(&d, now),
        render_findings(&d, now),
    ] {
        let lines: Vec<&str> = rendered.lines().collect();
        assert!(lines[0].starts_with("generated_at: "));
        assert!(lines[1].starts_with("daemon: "));
        assert!(lines[2].starts_with("This file is auto-generated"));
    }
}

#[test]
fn open_findings_precede_resolved() {
    let d = sample_data();
    let out = render_findings(&d, 1_700_000_000_000i64);
    let open_idx = out.find("## Open").unwrap();
    let resolved_idx = out.find("## Resolved").unwrap();
    assert!(open_idx < resolved_idx);
    // severity shown
    assert!(out.contains("[high]"));
}

#[test]
fn cap_holds_and_header_survives() {
    let mut findings = Vec::new();
    for i in 0..160 {
        findings.push(json!({"kind":"k","severity":"low","subject":format!("s{i}"),"summary":"x","state":"open"}));
    }
    let d = ViewData {
        status: json!({}),
        plans: json!({}),
        findings: json!({"findings":findings}),
        sessions: json!({}),
    };
    let out = render_findings(&d, 1_700_000_000_000i64);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.len() <= VIEW_CAP_LINES);
    assert!(out.contains("[... truncated to 150 lines ...]"));
    assert!(lines[0].starts_with("generated_at: "));
}

#[test]
fn changed_only_writes_nothing_second_time() {
    let dir = tempfile::TempDir::new().unwrap();
    let d = sample_data();
    let now = 1_700_000_000_000i64;
    let rendered = vec![
        ("STATUS.md", render_status(&d, now)),
        ("PLANS.md", render_plans(&d, now)),
        ("FINDINGS.md", render_findings(&d, now)),
    ];
    let first = write_views(dir.path(), &rendered).unwrap();
    assert!(!first.is_empty());
    let second = write_views(dir.path(), &rendered).unwrap();
    assert!(second.is_empty(), "second render wrote: {:?}", second);
}

#[test]
fn changed_data_rewrites_and_no_tmp() {
    let dir = tempfile::TempDir::new().unwrap();
    let d1 = sample_data();
    let now = 1_700_000_000_000i64;
    let r1 = vec![("STATUS.md", render_status(&d1, now))];
    write_views(dir.path(), &r1).unwrap();
    let mut d2 = sample_data();
    d2.status =
        json!({"worktree_count":2,"active_plans":1,"open_findings":1,"sessions":0,"worktrees":[]});
    let r2 = vec![("STATUS.md", render_status(&d2, now + 1))];
    let w = write_views(dir.path(), &r2).unwrap();
    assert_eq!(w.len(), 1);
    // no tmp remains
    for e in std::fs::read_dir(dir.path()).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.ends_with(".tmp"), "tmp remains: {name}");
    }
}

#[test]
fn views_dir_created_when_absent() {
    let base = tempfile::TempDir::new().unwrap();
    let dir = base.path().join("nested/views");
    assert!(!dir.exists());
    let d = sample_data();
    let r = vec![("STATUS.md", render_status(&d, 1i64))];
    write_views(&dir, &r).unwrap();
    assert!(dir.exists());
}

#[test]
fn error_path_leaves_no_tmp() {
    // induce io error by making dir a file
    let base = tempfile::TempDir::new().unwrap();
    let blocker = base.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let d = sample_data();
    let r = vec![("STATUS.md", render_status(&d, 1i64))];
    let res = write_views(&blocker, &r);
    assert!(res.is_err());
    // no tmp sibling
    for e in std::fs::read_dir(base.path()).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.ends_with(".tmp"), "tmp remains: {name}");
    }
}

#[test]
fn no_write_inside_worktree() {
    let repo = common::GitRepo::new();
    let state = tempfile::TempDir::new().unwrap();
    let env = Env::with("/tmp", Some(state.path().display().to_string()), None);
    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_assist::paths::repo_key_from_git_common_dir(&c))
        .unwrap();
    let views = env.views_dir(&key);
    // snapshot worktree tree + status
    let status_before = git_status_porcelain(repo.path());
    let d = sample_data();
    let r = vec![("STATUS.md", render_status(&d, 1i64))];
    write_views(&views, &r).unwrap();
    let status_after = git_status_porcelain(repo.path());
    assert_eq!(status_before, status_after);
}

#[test]
fn render_views_rpc_over_daemon() {
    // spawn daemon pattern from findings_kind
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
        .arg("render")
        .arg("--json")
        .env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "exit: {:?}", out.status);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let written = v["written"].as_array().unwrap();
    assert_eq!(written.len(), 3);
    // kill daemon
}

fn git_status_porcelain(dir: &std::path::Path) -> String {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Item 4 (20261022-20): the three markdown views are snapshot-tested with `insta`
/// instead of hand-rolled comparisons. Render with a fixed timestamp so the output is
/// deterministic; `assert_snapshot!(value)` names each snapshot after the test function
/// (a clean identifier), and insta commits the rendered output as a companion `.snap`
/// file that every subsequent run compares against, failing on any drift.
#[test]
fn views_are_snapshot_tested_with_insta() {
    use insta::assert_snapshot;

    let d = sample_data();
    let now = 1_700_000_000_000i64;

    // Full data: one snapshot per view.
    assert_snapshot!(render_status(&d, now));
    assert_snapshot!(render_plans(&d, now));
    assert_snapshot!(render_findings(&d, now));
}

/// Empty-data snapshot: every view renders its header + "(none)" bodies without panicking.
#[test]
fn views_snapshot_empty_data() {
    use insta::assert_snapshot;

    let empty = ViewData {
        status: json!({}),
        plans: json!({}),
        findings: json!({"findings": []}),
        sessions: json!({}),
    };
    let now = 1_700_000_000_000i64;

    assert_snapshot!(render_status(&empty, now));
    assert_snapshot!(render_plans(&empty, now));
    assert_snapshot!(render_findings(&empty, now));
}
