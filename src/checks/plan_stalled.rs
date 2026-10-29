use serde_json::json;

use crate::checks::CheckCtx;
use crate::store::FindingCandidate;

/// `plan.stalled` (Section 9): active plan with open tasks and no worktree activity for
/// `stalled_hours`. Severity `info`. `dedupe_key = plan.stalled:<plan>`.
pub fn check_plan_stalled(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let hours = ctx.config.thresholds.stalled_hours;
    if hours == 0 {
        return Vec::new();
    }
    let cutoff = ctx.now - (hours as i64) * 3_600_000;
    let ws = &ctx.workspace.worktrees;
    let mut out = Vec::new();
    for p in &ctx.workspace.plans {
        if p.state != "active" {
            continue;
        }
        if !super::plan_has_open_task(p, &ctx.workspace.tasks) {
            continue;
        }
        let wt = super::plan_worktree(ws, p);
        let last = wt.and_then(|w| ctx.workspace.activity.get(&w.path).cloned().flatten());
        if let Some(last) = last {
            if last >= cutoff {
                continue;
            }
        }
        out.push(FindingCandidate::new(
            "plan.stalled",
            &format!("plan.stalled:{}", p.id),
            &format!("{}: plan stalled", p.id),
            &format!(
                "active plan {} has open tasks but no worktree activity for {}h",
                p.id, hours
            ),
            "info",
            json!({ "last_seen_ms": last }),
            "inferred",
        ));
    }
    out
}
