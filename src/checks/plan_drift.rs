use serde_json::json;

use crate::checks::CheckCtx;
use crate::store::FindingCandidate;

/// `plan.drift` (Section 9): a file in the plan's worktree changed set that no task expects.
/// Skipped for plans with no `expected_paths`. One finding per plan, listing up to 20 paths.
/// Severity `info` (up to 10 paths) or `warn` (more). `dedupe_key = plan.drift:<plan>`.
pub fn check_plan_drift(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let patterns = ctx.config.ignore_paths.clone();
    let ws = &ctx.workspace.worktrees;
    let mut out = Vec::new();
    for p in &ctx.workspace.plans {
        if p.source == crate::plans::discover::SPEC_SOURCE {
            continue;
        }
        // Plans with no expected_paths cannot drift; also skip plans without a worktree.
        let total_expected: usize = ctx
            .workspace
            .tasks
            .iter()
            .filter(|t| t.plan_id == p.id)
            .map(|t| t.expected_paths.len())
            .sum();
        if total_expected == 0 {
            continue; // no task on this plan declares any path -> nothing to drift against.
        }
        let wt = match super::plan_worktree(ws, p) {
            Some(w) => w,
            None => continue,
        };
        let changed = super::changed_filter(wt, &patterns);
        let expected = super::expected_filtered(p, &ctx.workspace.tasks, &patterns);
        let expected_set: std::collections::BTreeSet<String> = expected.iter().cloned().collect();
        let drift: Vec<String> = changed
            .iter()
            .filter(|p| !expected_set.contains(*p))
            .cloned()
            .collect();
        if drift.is_empty() {
            continue;
        }
        let list: Vec<String> = drift.iter().take(20).cloned().collect();
        let severity = if drift.len() > 10 { "warn" } else { "info" };
        out.push(FindingCandidate::new(
            "plan.drift",
            &format!("plan.drift:{}", p.id),
            &format!("{}: {} drifted file(s)", p.id, drift.len()),
            &format!(
                "plan {} has {} path(s) no task expects (showing first {}): {}",
                p.id,
                drift.len(),
                list.len(),
                super::join_paths(&list)
            ),
            severity,
            json!({ "paths": drift }),
            "explicit",
        ));
    }
    out
}
