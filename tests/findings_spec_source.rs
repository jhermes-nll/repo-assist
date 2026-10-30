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
fn spec_does_not_produce_plan_checkbox_mismatch() {
    let cfg = Config::default_config();
    // spec half
    let spec_plan = make_plan("spec:1", "spec", "active", None);
    let mut spec_task = task("spec:1");
    spec_task.declared = "open".to_string();
    spec_task.observed = "committed".to_string();
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
    assert!(
        checks::plan_checkbox::check_plan_checkbox(&ctx).is_empty(),
        "spec must not fire plan.checkbox_mismatch"
    );

    // plan (equivalent) still fires
    let plan_plan = make_plan("plan:1", "superpowers", "active", None);
    let mut plan_task = task("plan:1");
    plan_task.declared = "open".to_string();
    plan_task.observed = "committed".to_string();
    let ws2 = CheckWorkspace {
        worktrees: vec![],
        plans: vec![spec_plan, plan_plan],
        tasks: vec![spec_task, plan_task],
        sessions: vec![],
        activity: HashMap::new(),
    };
    let ctx2 = CheckCtx {
        workspace: &ws2,
        config: &cfg,
        now: 0,
    };
    let findings = checks::plan_checkbox::check_plan_checkbox(&ctx2);
    assert!(
        findings
            .iter()
            .any(|f| f.dedupe_key == "plan.checkbox_mismatch:plan:1:1"
                || f.dedupe_key.contains("plan:1")),
        "plan must still fire plan.checkbox_mismatch: {findings:?}"
    );
    assert!(
        !findings.iter().any(|f| f.dedupe_key.contains("spec:1")),
        "spec must stay silent: {findings:?}"
    );
}

#[test]
fn spec_does_not_produce_plan_drift() {
    use repo_assist::store::WtInfo;
    let cfg = Config::default_config();
    // worktree with a changed file
    let wt = WtInfo {
        id: 1,
        path: "/wt".to_string(),
        branch: None,
        head: None,
        base_ref: None,
        merge_base: None,
        is_main: false,
        present: true,
        changed: vec![("a.rs".to_string(), "M".to_string())],
    };
    // spec plan + task expecting nothing (drift requires expected paths; give task expected_paths different?)
    // For drift to fire for plan, we need expected set nonempty and changed file not in expected.
    let spec_plan = make_plan("spec:1", "spec", "active", Some(1));
    let mut spec_task = task("spec:1");
    spec_task.expected_paths = vec!["b.rs".to_string()];
    let ws = CheckWorkspace {
        worktrees: vec![wt.clone()],
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
    assert!(
        checks::plan_drift::check_plan_drift(&ctx).is_empty(),
        "spec must not produce plan.drift"
    );

    let plan_plan = make_plan("plan:1", "superpowers", "active", Some(1));
    let mut plan_task = task("plan:1");
    plan_task.expected_paths = vec!["b.rs".to_string()];
    let ws2 = CheckWorkspace {
        worktrees: vec![wt],
        plans: vec![spec_plan, plan_plan],
        tasks: vec![spec_task, plan_task],
        sessions: vec![],
        activity: HashMap::new(),
    };
    let ctx2 = CheckCtx {
        workspace: &ws2,
        config: &cfg,
        now: 0,
    };
    let findings = checks::plan_drift::check_plan_drift(&ctx2);
    assert!(
        findings.iter().any(|f| f.dedupe_key == "plan.drift:plan:1"),
        "plan must still produce plan.drift: {findings:?}"
    );
    assert!(
        !findings.iter().any(|f| f.dedupe_key == "plan.drift:spec:1"),
        "spec must stay silent: {findings:?}"
    );
}

#[test]
fn plans_json_stalled_flag_suppresses_specs() {
    use repo_assist::ipc::server::plans_json;
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
        .upsert_task(&repo_assist::store::TaskRow {
            id: "spec-row:1",
            plan_id: "spec-row",
            ordinal: 1,
            title: "t",
            declared: "open",
            expected_paths: &[],
            depends_on: &[],
        })
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
    store
        .upsert_task(&repo_assist::store::TaskRow {
            id: "plan-row:1",
            plan_id: "plan-row",
            ordinal: 1,
            title: "t",
            declared: "open",
            expected_paths: &[],
            depends_on: &[],
        })
        .unwrap();
    let cfg = Config::default_config();
    let v = plans_json(&store, &cfg).expect("plans_json");
    let plans = v.get("plans").unwrap().as_array().unwrap();
    let spec = plans.iter().find(|p| p["id"] == "spec-row").unwrap();
    let plan = plans.iter().find(|p| p["id"] == "plan-row").unwrap();
    assert_eq!(
        spec["stalled"].as_bool(),
        Some(false),
        "spec stalled must be false"
    );
    assert_eq!(
        plan["stalled"].as_bool(),
        Some(true),
        "plan stalled must be true"
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
