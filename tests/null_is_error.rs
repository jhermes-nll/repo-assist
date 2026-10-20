mod common;

use repo_assist::store::{SessionRow, Store, ToolCallRow};
use tempfile::TempDir;

#[test]
fn check_workspace_survives_null_is_error() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("store.db");
    let store = Store::open(&db).unwrap();

    let row = SessionRow {
        id: "s1",
        agent: "test",
        file: "/tmp/s.jsonl",
        cwd: None,
        worktree_id: None,
        started_ms: Some(1),
        last_event_ms: Some(2),
        tokens_in: 0,
        tokens_out: 0,
        tokens_cache: 0,
        cost_usd: None,
        tool_calls: 1,
        byte_offset: 0,
        parse_errors: 0,
    };
    let calls = [ToolCallRow {
        ts_ms: 1,
        tool: "bash",
        args_hash: "h",
        is_error: None, // NULL
        result_hash: None,
    }];
    store.upsert_session_with_calls(&row, &calls).unwrap();

    // Precondition: the row truly is NULL on disk (not coerced to 0).
    // (checked implicitly: old code would error on load below)

    let ws = store
        .check_workspace()
        .expect("check_workspace must not fail on NULL is_error");
    assert_eq!(ws.sessions.len(), 1);
    assert_eq!(ws.sessions[0].tool_calls.len(), 1);
    assert!(ws.sessions[0].tool_calls[0].is_error.is_none());
}
