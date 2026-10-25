mod common;

use repo_assist::sources::git::now_ms;
use repo_assist::store::{EvidenceRow, PlanRow, Store, TaskRow};

#[test]
fn set_task_observed_and_clear_evidence() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = dir.path().join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = Store::open(&db).unwrap();

    store
        .upsert_plan(
            &PlanRow {
                id: "p",
                source: "t",
                title: "t",
                file: None,
                worktree_id: None,
                content_hash: None,
                state: "active",
            },
            now_ms(),
        )
        .unwrap();
    store
        .upsert_task(&TaskRow {
            id: "t1",
            plan_id: "p",
            ordinal: 1,
            title: "task",
            declared: "open",
            expected_paths: &[],
            depends_on: &[],
        })
        .unwrap();

    // Evidence: two kinds.
    store
        .append_evidence(&EvidenceRow {
            task_id: "t1",
            kind: "commit",
            reference: "abc",
            ts_ms: 1,
            confidence: "inferred",
        })
        .unwrap();
    store
        .append_evidence(&EvidenceRow {
            task_id: "t1",
            kind: "manual",
            reference: "xyz",
            ts_ms: 2,
            confidence: "explicit",
        })
        .unwrap();

    // set_task_observed visible.
    store.set_task_observed("t1", "committed", Some("reason")).unwrap();
    let (obs, reason): (String, Option<String>) = store
        .read(|conn| {
            conn.query_row(
                "SELECT observed, observed_reason FROM tasks WHERE id='t1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(obs, "committed");
    assert_eq!(reason.as_deref(), Some("reason"));

    // clear removes only matching kind.
    store.clear_evidence("t1", "commit").unwrap();
    let kinds: Vec<String> = store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT kind FROM evidence WHERE task_id='t1'")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(kinds, vec!["manual".to_string()]);
}
