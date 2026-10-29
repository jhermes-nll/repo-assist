use serde_json::json;

use crate::checks::CheckCtx;
use crate::store::{FindingCandidate, PlanInfo};

/// `plan.overlap` (Section 9): two **active** plans in different worktrees intersect on
/// `expected_paths` after `ignore_paths`. Predictive (fires before any edit). Severity `warn`.
pub fn check_plan_overlap(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let patterns = ctx.config.ignore_paths.clone();
    let ws = &ctx.workspace.worktrees;
    let tasks = &ctx.workspace.tasks;

    // (plan, worktree path, expected paths) for each active plan attached to a worktree.
    let mut plan_paths: Vec<(&PlanInfo, String, Vec<String>)> = Vec::new();
    for p in &ctx.workspace.plans {
        if p.state != "active" || p.source == crate::plans::discover::SPEC_SOURCE {
            continue;
        }
        let wt = super::plan_worktree(ws, p);
        if let Some(wt) = wt {
            let expected = super::expected_filtered(p, tasks, &patterns);
            if !expected.is_empty() {
                plan_paths.push((p, wt.path.clone(), expected));
            }
        }
    }

    let mut out = Vec::new();
    for i in 0..plan_paths.len() {
        for j in (i + 1)..plan_paths.len() {
            let (a, wa, ea) = &plan_paths[i];
            let (b, wb, eb) = &plan_paths[j];
            if wa == wb {
                continue; // same worktree -> path intersection is a drift concern, not an overlap.
            }
            let set_b: std::collections::BTreeSet<&String> = eb.iter().collect();
            let intersection: Vec<String> =
                ea.iter().filter(|p| set_b.contains(*p)).cloned().collect();
            if intersection.is_empty() {
                continue;
            }
            let (k0, k1) = if a.id <= b.id {
                (a.id.clone(), b.id.clone())
            } else {
                (b.id.clone(), a.id.clone())
            };
            out.push(FindingCandidate::new(
                "plan.overlap",
                &format!("plan.overlap:{}:{}", k0, k1),
                &super::join_two(&a.id, &b.id),
                &format!(
                    "active plans {} and {} expect overlapping paths (e.g. {})",
                    a.id,
                    b.id,
                    intersection
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "?".to_string())
                ),
                "warn",
                json!({ "shared": intersection }),
                "inferred",
            ));
        }
    }
    out
}
