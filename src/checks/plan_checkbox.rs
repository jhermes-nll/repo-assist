use std::collections::HashSet;

use serde_json::json;

use crate::checks::CheckCtx;
use crate::store::FindingCandidate;

/// `plan.checkbox_mismatch` (Section 9): `declared=open` but `observed=committed`; or
/// `declared=done` and `observed=not_started` with `expected_paths` known. Severity `warn`.
pub fn check_plan_checkbox(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let spec_plan_ids: HashSet<&str> = ctx
        .workspace
        .plans
        .iter()
        .filter(|p| p.source == crate::plans::discover::SPEC_SOURCE)
        .map(|p| p.id.as_str())
        .collect();
    let mut out = Vec::new();
    for t in &ctx.workspace.tasks {
        if spec_plan_ids.contains(t.plan_id.as_str()) {
            continue;
        }
        let mut flag: Option<(String, serde_json::Value)> = None;
        match (t.declared.as_str(), t.observed.as_str()) {
            ("open", "committed") => {
                flag = Some((
                    "declared open but observed committed".to_string(),
                    json!({}),
                ));
            }
            ("done", "not_started") if !t.expected_paths.is_empty() => {
                flag = Some((
                    "declared done but observed not started".to_string(),
                    json!({ "paths": t.expected_paths }),
                ));
            }
            _ => {}
        }
        if let Some((summary, evidence)) = flag {
            out.push(FindingCandidate::new(
                "plan.checkbox_mismatch",
                &format!("plan.checkbox_mismatch:{}", t.id),
                &format!("{}: {}", t.id, summary),
                &summary,
                "warn",
                evidence,
                "explicit",
            ));
        }
    }
    out
}

/// Compatibility alias for the old inline name.
pub fn check_checkbox_mismatch(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    check_plan_checkbox(ctx)
}
