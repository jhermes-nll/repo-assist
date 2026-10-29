//! Two-sided guard: specs are excluded from the plan findings engine and from
//! `active_plans`, while real plans with equivalent content remain active/firing.

use repo_assist::checks::{self, CheckCtx};
use repo_assist::config::Config;
use repo_assist::store::{CheckWorkspace, PlanInfo, TaskInfo};
use std::collections::HashMap;

fn make_plan(id: &str, source: &str, state: &str, worktree_id: Option<i64>) -> PlanInfo {
    PlanInfo {
        id: id.to_string(),
        source: source.to_string(),
        title: id.to_string(),
        worktree_id,
        state: state.to_string(),
    }
}

fn task(plan_id: &str) -> TaskInfo {
    TaskInfo {
        id: format!("{}:1", plan_id),
        plan_id: plan_id.to_string(),
        ordinal: 1,
        title: "t".to_string(),
        declared: "open".to_string(),
        expected_paths: vec![],
        depends_on: vec![],
        observed: "unknown".to_string(),
        observed_reason: None,
    }
}

#[test]
fn open_spec_produces_no_plan_stalled_finding() {
    let cfg = Config::default_config();
    let mut tasks = vec![];
    // spec case
    let spec_plan = make_plan("spec:1", "spec", "active", None);
    let spec_task = task("spec:1");
    let ws = CheckWorkspace {
        worktrees: vec![],
        plans: vec![spec_plan.clone()],
        tasks: vec![spec_task.clone()],
        sessions: vec![],
        activity: HashMap::new(),
    };
    let ctx = CheckCtx {
        workspace: &ws,
        config: &cfg,
        now: 0,
    };
    let findings = checks::plan_stalled::check_plan_stalled(&ctx);
    assert!(
        findings.is_empty(),
        "spec must not fire plan.stalled: {findings:?}"
    );

    // plan (equivalent content) still fires
    let mut tasks2 = vec![task("plan:1")];
    tasks.append(&mut tasks2);
    let plan_plan = make_plan("plan:1", "superpowers", "active", None);
    let ws2 = CheckWorkspace {
        worktrees: vec![],
        plans: vec![spec_plan, plan_plan],
        tasks,
        sessions: vec![],
        activity: HashMap::new(),
    };
    let ctx2 = CheckCtx {
        workspace: &ws2,
        config: &cfg,
        now: 0,
    };
    let findings2 = checks::plan_stalled::check_plan_stalled(&ctx2);
    assert!(
        findings2
            .iter()
            .any(|f| f.dedupe_key == "plan.stalled:plan:1"),
        "plan must still fire plan.stalled: {findings2:?}"
    );
    assert!(
        !findings2
            .iter()
            .any(|f| f.dedupe_key == "plan.stalled:spec:1"),
        "spec must stay silent even alongside a plan: {findings2:?}"
    );
}

#[test]
fn active_plans_count_excludes_specs() {
    // Direct SQL predicate used by src/ipc/server.rs active_plans count.
    use repo_assist::store::Store;
    use tempfile::TempDir;
    let d = TempDir::new().unwrap();
    let db = d.path().join("db.sqlite");
    let store = Store::open(&db).expect("open");
    let now = 1_000i64;
    store
        .upsert_plan(
            &repo_assist::store::PlanRow {
                id: "spec-row",
                source: "spec",
                title: "spec",
                file: Some("docs/x.md"),
                worktree_id: None,
                content_hash: None,
                state: "active",
            },
            now,
        )
        .unwrap();
    store
        .upsert_plan(
            &repo_assist::store::PlanRow {
                id: "plan-row",
                source: "superpowers",
                title: "plan",
                file: Some("docs/superpowers/plans/x.md"),
                worktree_id: None,
                content_hash: None,
                state: "active",
            },
            now,
        )
        .unwrap();
    let count: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM plans WHERE state='active' AND source != ?1",
                [repo_assist::plans::discover::SPEC_SOURCE],
                |r| r.get(0),
            )
        })
        .unwrap();
    // Source string equality: use repo_assist's const via path.
    assert_eq!(count, 1, "only the real plan should count, got {count}");
}
