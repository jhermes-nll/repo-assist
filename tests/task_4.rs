mod common;

use std::path::{Path, PathBuf};

/// Committed redacted fixtures only.
fn redacted_fixtures() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().ends_with(".redacted.jsonl"))
                    .unwrap_or(false)
            })
            .collect();
    v.sort();
    v
}

const F1_ID: &str = "01a1249c-7c2c-75b4-9cf9-0a646cce0ffa";
const F2_ID: &str = "01a1249c-ec21-7703-8773-813f926a6357";
const F3_ID: &str = "01a1249d-14fb-744b-9318-378a5372ccfd";

fn hdr(id: &str, cwd: &str) -> String {
    serde_json::json!({
        "type": "session", "version": 3, "id": id,
        "timestamp": "2026-10-19T00:00:00.000Z", "cwd": cwd
    })
    .to_string()
}
fn assistant_ok(ts: &str) -> String {
    serde_json::json!({
        "type": "message", "id": "m1", "parentId": null, "timestamp": ts,
        "message": {
            "role": "assistant",
            "content": [{ "type": "text", "text": "hi" }],
            "usage": { "input": 10, "output": 5, "cacheRead": 0, "cacheWrite": 0,
                        "cost": { "total": 0.02 } }
        }
    })
    .to_string()
}
fn call(ts: &str, cid: &str) -> String {
    serde_json::json!({
        "type": "message", "id": "m2", "parentId": null, "timestamp": ts,
        "message": {
            "role": "assistant",
            "content": [{ "type": "toolCall", "id": cid, "name": "read",
                          "arguments": { "path": "/x" } }],
            "usage": { "input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0,
                        "cost": { "total": 0 } }
        }
    })
    .to_string()
}
fn result(ts: &str, cid: &str, is_error: bool) -> String {
    serde_json::json!({
        "type": "message", "id": "m3", "parentId": null, "timestamp": ts,
        "message": {
            "role": "toolResult", "toolCallId": cid, "toolName": "read",
            "isError": is_error,
            "content": [{ "type": "text", "text": "done" }]
        }
    })
    .to_string()
}

fn sessions_root_with_fixtures() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("sessions").join("--Users-jesse--");
    std::fs::create_dir_all(&dir).unwrap();
    for f in redacted_fixtures() {
        std::fs::copy(&f, dir.join(f.file_name().unwrap())).unwrap();
    }
    std::fs::write(dir.join("noise.meta.json"), "{}").unwrap();
    std::fs::create_dir_all(dir.join("subagents")).unwrap();
    std::fs::write(dir.join("subagents/inner.jsonl"), "{\"type\":\"session\"}").unwrap();
    let root = tmp.path().join("sessions");
    (tmp, root)
}

fn open_store(repo: &common::GitRepo) -> repo_assist::store::Store {
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    repo_assist::store::Store::open(&db).unwrap()
}

#[test]
fn parser_reads_the_redacted_fixtures_exactly() {
    let expected = [
        (
            F1_ID,
            11436,
            2253,
            32640,
            4,
            1791615597613_i64,
            1791615612968_i64,
        ),
        (
            F2_ID,
            4999,
            886,
            9344,
            2,
            1791615626273_i64,
            1791615634451_i64,
        ),
        (
            F3_ID,
            8609,
            2286,
            17536,
            3,
            1791615636731_i64,
            1791615651076_i64,
        ),
    ];
    for (i, f) in redacted_fixtures().iter().enumerate() {
        let ing = repo_assist::sources::sessions::ingest_file(f)
            .unwrap_or_else(|| panic!("fixture {:?} should parse", f));
        let (id, tin, tout, tcache, calls, started, last) = expected[i];
        assert_eq!(ing.id, id, "session id in {f:?}");
        assert_eq!(ing.tokens_in, tin, "tokens_in in {f:?}");
        assert_eq!(ing.tokens_out, tout, "tokens_out in {f:?}");
        assert_eq!(ing.tokens_cache, tcache, "tokens_cache in {f:?}");
        assert_eq!(ing.calls.len(), calls, "toolCall block count in {f:?}");
        assert_eq!(ing.started_ms, Some(started), "started_ms in {f:?}");
        assert_eq!(ing.last_event_ms, Some(last), "last_event_ms in {f:?}");
        assert_eq!(ing.parse_errors, 0, "fixtures parse cleanly");
        assert_eq!(
            ing.cost_usd,
            Some(0.0),
            "ollama fixtures carry cost total 0"
        );
        assert_eq!(ing.cwd.as_deref(), Some("<home>"), "redacted cwd");
        assert!(
            ing.calls
                .iter()
                .all(|c| c.is_error == Some(false) && c.result_hash.is_some()),
            "every call must carry its result linkage: {:?}",
            ing.calls
        );
    }
}

#[test]
fn parse_errors_are_counted_and_torn_tails_are_not() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("edge.jsonl");
    let body = [
        hdr("s-edge", "/x"),
        assistant_ok("2026-10-19T00:00:01.000Z"),
        "NOT JSON AT ALL".to_string(),
        serde_json::json!({"type":"compaction","id":"c"}).to_string(),
        serde_json::json!({"type":"totally_new_kind"}).to_string(),
        serde_json::json!({"id":"m4"}).to_string(),
        result("2026-10-19T00:00:02.000Z", "call_ghost", false),
        call("2026-10-19T00:00:03.000Z", "call_a"),
        result("2026-10-19T00:00:04.000Z", "call_a", true),
    ]
    .join("\n");
    std::fs::write(
        &f,
        format!("{body}\n{}", assistant_ok("2026-10-19T00:00:05.000Z")),
    )
    .unwrap();
    let ing = repo_assist::sources::sessions::ingest_file(&f).expect("edge file parses");
    assert_eq!(ing.id, "s-edge");
    assert_eq!(
        ing.parse_errors, 3,
        "exactly rules (a),(b),(c) fire — the torn tail is NOT an error"
    );
    assert_eq!(
        ing.tokens_in, 11,
        "assistant_ok (10) + the call's usage (1); the torn tail contributes nothing"
    );
    assert_eq!(ing.tokens_out, 6);
    assert_eq!(ing.cost_usd, Some(0.02), "cost accumulates, not replaces");
    assert_eq!(
        ing.calls.len(),
        1,
        "orphan result is dropped, one call parsed"
    );
    assert_eq!(
        ing.calls[0].is_error,
        Some(true),
        "result linked to its call in-pass"
    );
    assert!(ing.calls[0].result_hash.is_some());
}

#[test]
fn files_without_a_v3_header_are_skipped_whole() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("no_hdr.jsonl");
    std::fs::write(
        &f,
        format!(
            "{}\n{}\n",
            assistant_ok("2026-10-19T00:00:00.000Z"),
            assistant_ok("2026-10-19T00:00:01.000Z")
        ),
    )
    .unwrap();
    assert!(
        repo_assist::sources::sessions::ingest_file(&f).is_none(),
        "no header -> no Ingested"
    );
    let f2 = tmp.path().join("bad_ver.jsonl");
    std::fs::write(
        &f2,
        hdr("s9", "/x").replace("\"version\":3", "\"version\":9"),
    )
    .unwrap();
    assert!(
        repo_assist::sources::sessions::ingest_file(&f2).is_none(),
        "unsupported header version -> skip"
    );
    let f3 = tmp.path().join("empty.jsonl");
    std::fs::write(&f3, "").unwrap();
    assert!(repo_assist::sources::sessions::ingest_file(&f3).is_none());
}

#[test]
fn duplicate_call_ids_each_row_and_last_one_gets_the_result() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("dupes.jsonl");
    std::fs::write(
        &f,
        format!(
            "{}\n{}\n{}\n{}\n",
            hdr("s-dup", "/x"),
            call("2026-10-19T00:00:01.000Z", "call_x"),
            call("2026-10-19T00:00:02.000Z", "call_x"),
            result("2026-10-19T00:00:03.000Z", "call_x", true),
        ),
    )
    .unwrap();
    let ing = repo_assist::sources::sessions::ingest_file(&f).unwrap();
    assert_eq!(ing.calls.len(), 2, "duplicate toolCallId -> both rows");
    assert_eq!(ing.calls[0].is_error, None, "first dup keeps no linkage");
    assert_eq!(
        ing.calls[1].is_error,
        Some(true),
        "last dup receives the result"
    );
}

#[test]
fn upsert_session_with_calls_replaces_rows_and_is_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("db.sqlite");
    let store = repo_assist::store::Store::open(&db).unwrap();
    let row = repo_assist::store::SessionRow {
        id: "s1",
        agent: "pi",
        file: "/tmp/s1.jsonl",
        cwd: Some("/w"),
        worktree_id: None,
        started_ms: Some(1),
        last_event_ms: Some(2),
        tokens_in: 1,
        tokens_out: 2,
        tokens_cache: 3,
        cost_usd: Some(0.5),
        tool_calls: 1,
        byte_offset: 100,
        parse_errors: 0,
    };
    let calls = [repo_assist::store::ToolCallRow {
        ts_ms: 2,
        tool: "read",
        args_hash: "a1b2c3d4e5f60718",
        is_error: Some(false),
        result_hash: None,
    }];
    store.upsert_session_with_calls(&row, &calls).unwrap();

    let row2 = repo_assist::store::SessionRow {
        id: "s1",
        agent: "pi",
        file: "/tmp/s1.jsonl",
        cwd: Some("/w"),
        worktree_id: None,
        started_ms: Some(1),
        last_event_ms: Some(2),
        tokens_in: 9,
        tokens_out: 2,
        tokens_cache: 3,
        cost_usd: None,
        tool_calls: 0,
        byte_offset: 200,
        parse_errors: 1,
    };
    store.upsert_session_with_calls(&row2, &[]).unwrap();

    let (tin, off, pe, callsn): (i64, i64, i64, i64) = store
        .read(|conn| {
            let (tin, off, pe) = conn.query_row(
                "SELECT tokens_in, byte_offset, parse_errors FROM sessions WHERE id = 's1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let calls = conn.query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE session_id = 's1'",
                [],
                |r| r.get::<_, i64>(0),
            )?;
            Ok((tin, off, pe, calls))
        })
        .unwrap();
    assert_eq!(
        (tin, off, pe, callsn),
        (9, 200, 1, 0),
        "second upsert wins; calls replaced"
    );

    let row3 = repo_assist::store::SessionRow {
        id: "s2",
        agent: "pi",
        file: "/tmp/s1.jsonl",
        cwd: None,
        worktree_id: None,
        started_ms: None,
        last_event_ms: None,
        tokens_in: 0,
        tokens_out: 0,
        tokens_cache: 0,
        cost_usd: None,
        tool_calls: 0,
        byte_offset: 5,
        parse_errors: 0,
    };
    store.upsert_session_with_calls(&row3, &[]).unwrap();
    let n: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE file = '/tmp/s1.jsonl'",
                [],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(n, 1, "same file with a new session id replaces the old row");

    let offsets = store.session_file_offsets().unwrap();
    assert_eq!(offsets.get("s1"), None, "s1 was replaced away");
    assert_eq!(offsets.get("s2"), Some(&5));
}

#[test]
fn discovery_returns_top_level_jsonl_only_and_resolves_the_root() {
    let (_tmp, root) = sessions_root_with_fixtures();
    let files = repo_assist::sources::sessions::discover_session_files(&root);
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names.len(),
        3,
        ".meta.json and subagents/ are skipped: {names:?}"
    );
    assert!(names.iter().all(|n| n.ends_with(".redacted.jsonl")));

    std::env::set_var("PI_CODING_AGENT_SESSION_DIR", "/opt/somewhere");
    let env = repo_assist::paths::Env::with("/home/u", None, None);
    assert_eq!(
        repo_assist::sources::sessions::resolve_sessions_root(&env),
        std::path::PathBuf::from("/opt/somewhere")
    );
    std::env::remove_var("PI_CODING_AGENT_SESSION_DIR");
    std::env::set_var("PI_CODING_AGENT_DIR", "/opt/agent");
    assert_eq!(
        repo_assist::sources::sessions::resolve_sessions_root(&env),
        std::path::PathBuf::from("/opt/agent/sessions")
    );
    std::env::remove_var("PI_CODING_AGENT_DIR");
    assert_eq!(
        repo_assist::sources::sessions::resolve_sessions_root(&env),
        std::path::PathBuf::from("/home/u/.pi/agent/sessions")
    );
}

#[test]
fn sync_ingests_fixtures_with_expected_totals_and_null_worktree() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let store = open_store(&repo);
    let (_tmp, root) = sessions_root_with_fixtures();

    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);

    let rows: Vec<(String, i64, i64, i64, i64, Option<i64>)> = store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT s.id, s.tokens_in, s.tokens_out, s.tokens_cache, s.tool_calls, s.worktree_id FROM sessions s ORDER BY s.id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        })
        .unwrap();
    assert_eq!(rows.len(), 3, "exactly the three fixture sessions");
    let by_id = |id: &str| rows.iter().find(|r| r.0 == id).unwrap().clone();
    let f1 = by_id(F1_ID);
    assert_eq!((f1.1, f1.2, f1.3, f1.4), (11436, 2253, 32640, 4));
    for r in &rows {
        assert_eq!(r.5, None, "fixture sessions map to no worktree");
    }
    let calls: i64 = store
        .read(|conn| conn.query_row("SELECT COUNT(*) FROM tool_calls", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(calls, 9);
}

#[test]
fn sync_maps_sessions_inside_a_worktree_by_component_prefix() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-m", "m.txt");
    repo.add_worktree("wt-m2", "m2.txt");
    let store = open_store(&repo);
    let (_tmp, root) = sessions_root_with_fixtures();
    let dir = root.join("--tmp--");
    std::fs::create_dir_all(&dir).unwrap();
    let wt = std::fs::canonicalize(repo.path().join("wt-m")).unwrap();
    let wt2 = std::fs::canonicalize(repo.path().join("wt-m2")).unwrap();

    std::fs::write(
        dir.join("at-root.jsonl"),
        format!(
            "{}\n{}\n",
            hdr("s-at-root", wt.to_str().unwrap()),
            assistant_ok("2026-10-19T00:00:01.000Z")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("nested.jsonl"),
        format!(
            "{}\n{}\n",
            hdr("s-nested", wt.join("sub/dir").to_str().unwrap()),
            assistant_ok("2026-10-19T00:00:02.000Z")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("in-wt2.jsonl"),
        format!(
            "{}\n{}\n",
            hdr("s-in-wt2", wt2.join("sub").to_str().unwrap()),
            assistant_ok("2026-10-19T00:00:03.000Z")
        ),
    )
    .unwrap();
    let fool_cwd = format!("{}-suffix", wt.to_string_lossy());
    std::fs::write(
        dir.join("fool.jsonl"),
        format!(
            "{}\n{}\n",
            hdr("s-fool", &fool_cwd),
            assistant_ok("2026-10-19T00:00:04.000Z")
        ),
    )
    .unwrap();

    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);

    let id_of: std::collections::HashMap<String, i64> = store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT id, path FROM worktrees")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(0)?)))?;
            rows.collect::<Result<std::collections::HashMap<_, _>, _>>()
        })
        .unwrap();
    let wt_m_id = id_of
        .get(wt.to_str().unwrap())
        .copied()
        .expect("wt-m row exists");
    let wt_m2_id = id_of
        .get(wt2.to_str().unwrap())
        .copied()
        .expect("wt-m2 row exists");
    let main_id = id_of
        .get(
            std::fs::canonicalize(repo.path())
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .copied()
        .expect("the repo root is itself a present worktree");
    assert_ne!(wt_m_id, wt_m2_id);
    assert_ne!(main_id, wt_m_id);

    let wt_of = |sid: &str| {
        store
            .read(|conn| {
                conn.query_row(
                    "SELECT worktree_id FROM sessions WHERE id = ?1",
                    [sid],
                    |r| r.get::<_, Option<i64>>(0),
                )
            })
            .unwrap()
    };
    assert_eq!(
        wt_of("s-at-root"),
        Some(wt_m_id),
        "session at the worktree root maps to it"
    );
    assert_eq!(
        wt_of("s-nested"),
        Some(wt_m_id),
        "nested cwd maps to the same, most-specific worktree"
    );
    assert_eq!(
        wt_of("s-in-wt2"),
        Some(wt_m2_id),
        "cwd inside wt-m2 maps to wt-m2, NOT to wt-m"
    );
    assert_eq!(
        wt_of("s-fool"),
        Some(main_id),
        "string-prefix cwd falls to main-worktree containment, not wt-m"
    );
}

#[test]
fn sync_skips_unchanged_files_grows_on_append_and_resets_on_shrink() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-s", "s.txt");
    let store = open_store(&repo);
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("sessions");
    let dir = root.join("--x--");
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = redacted_fixtures()[1].clone();
    let target = dir.join(fixture.file_name().unwrap());
    std::fs::copy(&fixture, &target).unwrap();

    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    let size = std::fs::metadata(&target).unwrap().len() as i64;
    let (tin, calls) = read_agg(&store, F2_ID, &size);
    assert_eq!(
        (tin, calls),
        (4999, 2),
        "first pass ingests the whole fixture"
    );

    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO tool_calls(session_id, ts_ms, tool, args_hash, is_error, result_hash) VALUES (?1, 0, 'marker', 'marker', NULL, NULL)",
                [F2_ID],
            )
        })
        .unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    assert_eq!(read_agg(&store, F2_ID, &size), (4999, 2));
    let marker: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1 AND tool = 'marker'",
                [F2_ID],
                |r| r.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(
        marker, 1,
        "the byte-offset skip cursor prevented a re-parse"
    );

    let appended = assistant_ok("2026-10-19T00:00:09.000Z");
    let mut body = std::fs::read_to_string(&target).unwrap();
    body.push_str(&appended);
    body.push('\n');
    std::fs::write(&target, &body).unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    let new_size = std::fs::metadata(&target).unwrap().len() as i64;
    assert_eq!(read_agg(&store, F2_ID, &new_size), (4999 + 10, 2));

    let body2 = std::fs::read_to_string(&target).unwrap();
    let kept: String = body2.lines().take(10).map(|l| format!("{l}\n")).collect();
    std::fs::write(&target, kept).unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    let shrunk = std::fs::metadata(&target).unwrap().len() as i64;
    assert_eq!(read_agg(&store, F2_ID, &shrunk), (4999, 2));
}

fn read_agg(store: &repo_assist::store::Store, id: &str, expect_off: &i64) -> (i64, i64) {
    let (tin, calls, off): (i64, i64, i64) = store
        .read(|conn| {
            conn.query_row(
                "SELECT s.tokens_in, s.tool_calls, s.byte_offset FROM sessions s WHERE s.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .unwrap();
    assert_eq!(
        off, *expect_off,
        "byte_offset tracks the file size (skip cursor)"
    );
    (tin, calls)
}

#[test]
fn ra_sessions_end_to_end_over_ipc() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-e", "e.txt");
    repo.add_worktree("wt-map", "m.txt");
    let (state, runtime) = (repo.path().join("state"), repo.path().join("run"));
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    let (_fix_tmp, pi_root) = sessions_root_with_fixtures();
    let wt_map = std::fs::canonicalize(repo.path().join("wt-map")).unwrap();
    std::fs::write(
        pi_root.join("mapped.jsonl"),
        format!(
            "{}\n{}\n",
            hdr("s-mapped-ipc", wt_map.to_str().unwrap()),
            assistant_ok("2026-10-19T00:00:01.000Z")
        ),
    )
    .unwrap();
    let envs: Vec<(String, String)> = vec![
        ("HOME".into(), state.display().to_string()),
        ("XDG_STATE_HOME".into(), state.display().to_string()),
        ("XDG_RUNTIME_DIR".into(), runtime.display().to_string()),
        (
            "PI_CODING_AGENT_SESSION_DIR".into(),
            pi_root.display().to_string(),
        ),
    ];
    let ra_bin = std::env::var("CARGO_BIN_EXE_ra").unwrap();

    let _daemon = DaemonGuard(
        std::process::Command::new(&ra_bin)
            .arg("daemon")
            .arg("--foreground")
            .envs(envs.iter().cloned())
            .current_dir(repo.path())
            .spawn()
            .expect("spawn ra daemon --foreground"),
    );

    let key = repo_assist::paths::git_common_dir(repo.path())
        .map(|c| repo_assist::paths::repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sock = runtime.join("repo-assist").join(format!("{key}.sock"));
    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(sock.exists(), "daemon did not bind its socket at {sock:?}");

    let mut served = Vec::<serde_json::Value>::new();
    for _ in 0..30 {
        let out = assert_cmd::Command::cargo_bin("ra")
            .unwrap()
            .envs(envs.iter().cloned())
            .current_dir(repo.path())
            .arg("sessions")
            .arg("--json")
            .timeout(std::time::Duration::from_secs(20))
            .output()
            .expect("run ra sessions --json");
        if out.status.success() {
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            if v["sessions"].as_array().map(|a| a.len()) == Some(4) {
                served = v["sessions"].as_array().unwrap().clone();
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert_eq!(
        served.len(),
        4,
        "the daemon must ingest and serve the fixture sessions plus the mapped one"
    );

    let f3 = served
        .iter()
        .find(|s| s["id"].as_str() == Some(F3_ID))
        .unwrap();
    assert_eq!(f3["tokens_in"].as_i64(), Some(8609));
    assert_eq!(f3["tool_calls"].as_i64(), Some(3));
    let f2 = served
        .iter()
        .find(|s| s["id"].as_str() == Some(F2_ID))
        .unwrap();
    assert_eq!(f2["id"].as_str(), Some(F2_ID));
    // any fixture session is unmapped
    let unmapped = served
        .iter()
        .find(|s| s["id"].as_str() == Some(F1_ID))
        .unwrap();
    assert_eq!(unmapped["agent"].as_str(), Some("pi"));
    assert_eq!(
        unmapped["worktree"].as_str(),
        None,
        "unmapped session shows null worktree"
    );
    assert_eq!(unmapped["parse_errors"].as_i64(), Some(0));
    assert!(unmapped["byte_offset"].as_i64().unwrap_or(0) > 0);

    let mapped = served
        .iter()
        .find(|s| s["id"].as_str() == Some("s-mapped-ipc"))
        .expect("mapped session present");
    assert_eq!(
        mapped["worktree"].as_str(),
        Some(wt_map.to_str().unwrap()),
        "a session inside a worktree renders its worktree path over IPC"
    );

    let out = assert_cmd::Command::cargo_bin("ra")
        .unwrap()
        .envs(envs.iter().cloned())
        .current_dir(repo.path())
        .arg("status")
        .arg("--no-spawn")
        .arg("--json")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "status --no-spawn: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(status["sessions"].as_i64(), Some(4));

    let out = assert_cmd::Command::cargo_bin("ra")
        .unwrap()
        .envs(envs.iter().cloned())
        .current_dir(repo.path())
        .arg("sessions")
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(F3_ID), "full ids are shown: {text}");
    assert!(text.contains("pi"));
    assert!(text.contains("8609"), "token counts are shown: {text}");
}

struct DaemonGuard(std::process::Child);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn missing_usage_cost_is_null_in_ingest_and_db() {
    let tmp = tempfile::TempDir::new().unwrap();
    let f = tmp.path().join("nocost.jsonl");
    let line = serde_json::json!({
        "type": "message", "id": "m1", "parentId": null, "timestamp": "2026-10-19T00:00:01.000Z",
        "message": {
            "role": "assistant",
            "content": [{ "type": "text", "text": "hi" }],
            "usage": { "input": 10, "output": 5, "cacheRead": 0, "cacheWrite": 0 }
        }
    });
    std::fs::write(&f, format!("{}\n{}\n", hdr("s-nocost", "/x"), line)).unwrap();
    let ing = repo_assist::sources::sessions::ingest_file(&f).unwrap();
    assert_eq!(ing.cost_usd, None);

    let db = tmp.path().join("db.sqlite");
    let store = repo_assist::store::Store::open(&db).unwrap();
    let row = repo_assist::store::SessionRow {
        id: "s-nocost",
        agent: "pi",
        file: "/tmp/nocost.jsonl",
        cwd: Some("/x"),
        worktree_id: None,
        started_ms: Some(0),
        last_event_ms: Some(0),
        tokens_in: ing.tokens_in,
        tokens_out: ing.tokens_out,
        tokens_cache: ing.tokens_cache,
        cost_usd: ing.cost_usd,
        tool_calls: 0,
        byte_offset: 0,
        parse_errors: 0,
    };
    store.upsert_session_with_calls(&row, &[]).unwrap();
    let cost: Option<f64> = store
        .read(|conn| {
            conn.query_row(
                "SELECT cost_usd FROM sessions WHERE id = 's-nocost'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(cost, None);
}

#[test]
fn upsert_session_with_calls_is_atomic_on_midstream_failure() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = tmp.path().join("db.sqlite");
    let store = repo_assist::store::Store::open(&db).unwrap();

    let row0 = repo_assist::store::SessionRow {
        id: "s0",
        agent: "pi",
        file: "/tmp/s0.jsonl",
        cwd: Some("/w"),
        worktree_id: None,
        started_ms: Some(1),
        last_event_ms: Some(2),
        tokens_in: 1,
        tokens_out: 2,
        tokens_cache: 3,
        cost_usd: Some(0.5),
        tool_calls: 0,
        byte_offset: 7,
        parse_errors: 0,
    };
    store.upsert_session_with_calls(&row0, &[]).unwrap();

    // Force every subsequent tool_calls INSERT to fail.
    store
        .write(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER boom BEFORE INSERT ON tool_calls BEGIN SELECT RAISE(ABORT, 'boom'); END;",
            )
        })
        .unwrap();

    let row1 = repo_assist::store::SessionRow {
        id: "s0",
        agent: "pi",
        file: "/tmp/s0.jsonl",
        cwd: Some("/w"),
        worktree_id: None,
        started_ms: Some(1),
        last_event_ms: Some(2),
        tokens_in: 9,
        tokens_out: 2,
        tokens_cache: 3,
        cost_usd: None,
        tool_calls: 1,
        byte_offset: 99,
        parse_errors: 1,
    };
    let call = repo_assist::store::ToolCallRow {
        ts_ms: 2,
        tool: "read",
        args_hash: "a1b2c3d4e5f60718",
        is_error: Some(false),
        result_hash: None,
    };
    let res = store.upsert_session_with_calls(&row1, &[call]);
    assert!(res.is_err(), "the forced failure must surface as an error");

    let off: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT byte_offset FROM sessions WHERE id = 's0'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(
        off, 7,
        "byte_offset must not advance when the write fails mid-way"
    );
}
