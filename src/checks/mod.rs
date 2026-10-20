//! Findings checks (Section 9).
//!
//! Every check is a pure function over a [`CheckWorkspace`] (assembled by the daemon from the
//! store) and returns [`FindingCandidate`]s. The daemon runs all checks, reconciles the
//! candidates against the persisted `findings` table (`Store::sync_findings`), and persists the
//! deltas. Checks never write to the store themselves.
//!
//! Conflicts are detected by scanning `git merge-tree` **output** for conflict markers, because
//! `git merge-tree --trivial-merge` exits `0` in both directions on this git (>= 2.38); relying
//! on the exit code would silently miss every conflict.

use std::process::Command;

use serde_json::{json, Value};

use crate::config::Config;
use crate::store::{CheckWorkspace, FindingCandidate, PlanInfo, TaskInfo, WtInfo, TC};

/// The context every check runs over (Section 9).
#[derive(Debug, Clone)]
pub struct CheckCtx<'a> {
    /// Everything the checks read (worktrees + changed sets, plans, tasks, sessions, activity).
    pub workspace: &'a CheckWorkspace,
    /// Daemon configuration, including thresholds and `ignore_paths`.
    pub config: &'a Config,
    /// Now, in unix milliseconds (Section 3).
    pub now: i64,
}

/// Run every check and return all candidate findings (deduped by `dedupe_key` on write).
pub fn run_all_checks(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let mut out: Vec<FindingCandidate> = Vec::new();
    out.extend(check_worktree_overlap(ctx));
    out.extend(check_worktree_merge_conflict(ctx));
    out.extend(check_worktree_abandoned(ctx));
    out.extend(check_plan_overlap(ctx));
    out.extend(check_plan_drift(ctx));
    out.extend(check_plan_stalled(ctx));
    out.extend(check_checkbox_mismatch(ctx));
    out.extend(check_agent_loop(ctx));
    out.extend(check_agent_tokens(ctx));
    out
}

/// Whether one path matches any `ignore_paths` pattern (Section 10 defaults: `*.lock` and
/// `**/*.snap`). Matching is intentionally lenient (exact, `*.suffix`, or `**/*.suffix`).
fn matches_ignore(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        if path == p {
            return true;
        }
        if let Some(suffix) = p.strip_prefix("**/*") {
            return path.ends_with(suffix);
        }
        if let Some(suffix) = p.strip_prefix("*.") {
            return path.ends_with(&format!(".{}", suffix));
        }
        if let Some(suffix) = p.strip_prefix("**/") {
            return path.contains(suffix) || path.ends_with(suffix);
        }
        false
    })
}

/// The changed-file set of one worktree, filtered by `ignore_paths` (Section 10).
fn changed_filter(wt: &WtInfo, patterns: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = wt
        .changed
        .iter()
        .filter(|(p, _)| !matches_ignore(p, patterns))
        .map(|(p, _)| p.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// The changed paths a plan expects (union of its tasks' `expected_paths`), filtered by
/// `ignore_paths`.
fn expected_filtered(plan: &PlanInfo, tasks: &[TaskInfo], patterns: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = tasks
        .iter()
        .filter(|t| t.plan_id == plan.id)
        .flat_map(|t| t.expected_paths.iter().cloned())
        .filter(|p| !matches_ignore(p, patterns))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// The worktree one plan maps to (the worktree whose id equals the plan's worktree_id), if any.
fn plan_worktree<'a>(ws: &'a [WtInfo], plan: &PlanInfo) -> Option<&'a WtInfo> {
    ws.iter().find(|w| plan.worktree_id == Some(w.id))
}

/// Whether any task of a plan is `declared=open`.
fn plan_has_open_task(plan: &PlanInfo, tasks: &[TaskInfo]) -> bool {
    tasks
        .iter()
        .any(|t| t.plan_id == plan.id && t.declared == "open")
}

fn join_two(a: &str, b: &str) -> String {
    format!("{}, {}", a, b)
}

fn join_paths(paths: &[String]) -> String {
    let one = |n: usize| {
        if n < paths.len() {
            paths[n].clone()
        } else {
            "?".to_string()
        }
    };
    format!("{}, {}, {}", one(0), one(1), one(2))
}

/// `worktree.overlap` (Section 9): changed-file sets of two worktrees intersect after
/// `ignore_paths`. Severity `warn`. `dedupe_key = worktree.overlap:<wtA>:<wtB>` (paths sorted).
fn check_worktree_overlap(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let patterns = ctx.config.ignore_paths.clone();
    let ws = &ctx.workspace.worktrees;
    let mut out = Vec::new();
    for i in 0..ws.len() {
        for j in (i + 1)..ws.len() {
            let a = &ws[i];
            let b = &ws[j];
            let changed_a = changed_filter(a, &patterns);
            let changed_b = changed_filter(b, &patterns);
            if changed_a.is_empty() || changed_b.is_empty() {
                continue;
            }
            let set_b: std::collections::BTreeSet<&String> = changed_b.iter().collect();
            let intersection: Vec<String> = changed_a
                .iter()
                .filter(|p| set_b.contains(*p))
                .cloned()
                .collect();
            if intersection.is_empty() {
                continue;
            }
            let (mut p0, mut p1) = (a.path.clone(), b.path.clone());
            if p0 > p1 {
                std::mem::swap(&mut p0, &mut p1);
            }
            let sample = intersection
                .first()
                .cloned()
                .unwrap_or_else(|| "?".to_string());
            out.push(FindingCandidate::new(
                "worktree.overlap",
                &format!("worktree.overlap:{}:{}", p0, p1),
                &join_two(&p0, &p1),
                &format!(
                    "worktrees {} and {} touch {} shared path(s) (e.g. {})",
                    a.path,
                    b.path,
                    intersection.len(),
                    sample
                ),
                "warn",
                json!({ "shared": intersection }),
                "explicit",
            ));
        }
    }
    out
}

/// `worktree.merge_conflict` (Section 9): `git merge-tree` between two branch heads fails to
/// trivially merge (detected from output). Severity `error`. `dedupe_key =
/// worktree.merge_conflict:<wtA>:<wtB>` (heads sorted). Cached by the ordered head pair.
fn check_worktree_merge_conflict(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let ws = ctx.workspace.worktrees.clone();
    // Cache by ordered head pair: (headA, headB) -> has_conflict.
    let mut cache: std::collections::BTreeMap<(String, String), bool> =
        std::collections::BTreeMap::new();
    let mut out = Vec::new();
    for i in 0..ws.len() {
        for j in (i + 1)..ws.len() {
            let a = &ws[i];
            let b = &ws[j];
            // Both branches must have a resolvable head to detect a conflict.
            let (ha, hb) = match (a.head.as_deref(), b.head.as_deref()) {
                (Some(x), Some(y)) if x != y => (x.to_string(), y.to_string()),
                _ => continue,
            };
            let (k0, k1) = if ha <= hb {
                (ha.clone(), hb.clone())
            } else {
                (hb.clone(), ha.clone())
            };
            if let Some(conflict) = cache.get(&(k0.clone(), k1.clone())) {
                if *conflict {
                    out.push(FindingCandidate::new(
                        "worktree.merge_conflict",
                        &format!("worktree.merge_conflict:{}:{}", ha, hb),
                        &join_two(&a.path, &b.path),
                        &format!("worktrees {} and {} cannot merge cleanly", a.path, b.path),
                        "error",
                        json!({ "heads": [ha, hb] }),
                        "explicit",
                    ));
                }
                continue;
            }
            let conflict = merge_conflict(a, b);
            cache.insert((k0, k1), conflict);
            if conflict {
                out.push(FindingCandidate::new(
                    "worktree.merge_conflict",
                    &format!("worktree.merge_conflict:{}:{}", ha, hb),
                    &join_two(&a.path, &b.path),
                    &format!("worktrees {} and {} cannot merge cleanly", a.path, b.path),
                    "error",
                    json!({ "heads": [ha, hb] }),
                    "explicit",
                ));
            }
        }
    }
    out
}

/// Whether `git merge-tree --trivial-merge <base> <headA> <headB>` signals a conflict.
fn merge_conflict(a: &WtInfo, b: &WtInfo) -> bool {
    let (ha, hb) = match (a.head.as_deref(), b.head.as_deref()) {
        (Some(x), Some(y)) => (x.to_string(), y.to_string()),
        _ => return false,
    };
    let base = head(&ha, &hb);
    // No common ancestor (unrelated histories) => cannot detect a conflict; treat as none.
    if base.is_empty() {
        return false;
    }
    // Run from a real worktree path so git has a cwd; the result only depends on the commit refs.
    let cwd = a.path.clone();
    match Command::new("git")
        .current_dir(&cwd)
        .arg("merge-tree")
        .arg("--trivial-merge")
        .arg("--name-only")
        .arg("--messages")
        .arg(&base)
        .arg(&ha)
        .arg(&hb)
        .output()
    {
        Ok(o) if o.status.success() => has_conflict_markers(&String::from_utf8_lossy(&o.stdout)),
        _ => false,
    }
}

/// Whether any conflict marker (`<<<<<<<`, `=======`, `>>>>>>>`, or `@@` hunk header) is present
/// in `git merge-tree --trivial-merge` output (the conflict signal on this git).
fn has_conflict_markers(output: &str) -> bool {
    output.lines().any(|l| {
        let s = l.trim_start();
        s.starts_with("<<<<<<<")
            || s.starts_with(">>>>>>>")
            || s.starts_with("=======")
            || s.starts_with("@@")
    })
}

/// Compute the merge-base (or empty string) of two commit refs.
fn head(a: &str, b: &str) -> String {
    Command::new("git")
        .arg("merge-base")
        .arg(a)
        .arg(b)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// `worktree.abandoned` (Section 9): no activity for `abandoned_hours` AND (uncommitted changes
/// OR an active plan with open tasks). Severity `info`.
fn check_worktree_abandoned(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let hours = ctx.config.thresholds.abandoned_hours;
    if hours == 0 {
        return Vec::new();
    }
    let cutoff = ctx.now - (hours as i64) * 3_600_000;
    let ws = &ctx.workspace.worktrees;
    let mut out = Vec::new();
    for wt in ws {
        if !wt.present {
            continue;
        }
        let last = ctx.workspace.activity.get(&wt.path).cloned().flatten();
        // Needs activity AND (uncommitted changes OR active plan with open tasks).
        let uncommitted = !changed_filter(wt, &ctx.config.ignore_paths).is_empty();
        let active_plan_with_open = ctx.workspace.plans.iter().any(|p| {
            p.worktree_id == Some(wt.id)
                && p.state == "active"
                && plan_has_open_task(p, &ctx.workspace.tasks)
        });
        if (uncommitted || active_plan_with_open) && last.is_none_or(|t| t < cutoff) {
            out.push(FindingCandidate::new(
                "worktree.abandoned",
                &format!("worktree.abandoned:{}", wt.path),
                &wt.path,
                &format!("worktree {} has no activity but is still in use", wt.path),
                "info",
                json!({ "last_seen_ms": last }),
                "inferred",
            ));
        }
    }
    out
}

/// `plan.overlap` (Section 9): two **active** plans in different worktrees intersect on
/// `expected_paths` after `ignore_paths`. Predictive (fires before any edit). Severity `warn`.
fn check_plan_overlap(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let patterns = ctx.config.ignore_paths.clone();
    let ws = ctx.workspace.worktrees.clone();
    // (plan, worktree path) for each active plan with a changed set.
    let mut plan_paths: Vec<(&PlanInfo, String)> = Vec::new();
    for p in &ctx.workspace.plans {
        if p.state != "active" {
            continue;
        }
        let wt = plan_worktree(&ws, p);
        if let Some(wt) = wt {
            let changed = changed_filter(wt, &patterns);
            if !changed.is_empty() {
                plan_paths.push((p, wt.path.clone()));
            }
        }
    }
    let mut out = Vec::new();
    for i in 0..plan_paths.len() {
        for j in (i + 1)..plan_paths.len() {
            let (a, wa) = &plan_paths[i];
            let (b, wb) = &plan_paths[j];
            if wa == wb {
                continue; // same worktree -> path intersection is a drift concern, not an overlap.
            }
            let wa_changed = changed_filter(ws.iter().find(|w| w.path == *wa).unwrap(), &patterns);
            let wb_changed = changed_filter(ws.iter().find(|w| w.path == *wb).unwrap(), &patterns);
            let set_b: std::collections::BTreeSet<&String> = wb_changed.iter().collect();
            let intersection: Vec<String> = wa_changed
                .iter()
                .filter(|p| set_b.contains(*p))
                .cloned()
                .collect();
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
                &join_two(&a.id, &b.id),
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

/// `plan.drift` (Section 9): a file in the plan's worktree changed set that no task expects.
/// Skipped for plans with no `expected_paths`. One finding per plan, listing up to 20 paths.
/// Severity `info` (up to 10 paths) or `warn` (more). `dedupe_key = plan.drift:<plan>`.
fn check_plan_drift(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let patterns = ctx.config.ignore_paths.clone();
    let ws = &ctx.workspace.worktrees;
    let mut out = Vec::new();
    for p in &ctx.workspace.plans {
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
        let wt = match plan_worktree(ws, p) {
            Some(w) => w,
            None => continue,
        };
        let changed = changed_filter(wt, &patterns);
        let expected = expected_filtered(p, &ctx.workspace.tasks, &patterns);
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
                join_paths(&list)
            ),
            severity,
            json!({ "paths": drift }),
            "explicit",
        ));
    }
    out
}

/// `plan.stalled` (Section 9): active plan with open tasks and no worktree activity for
/// `stalled_hours`. Severity `info`. `dedupe_key = plan.stalled:<plan>`.
fn check_plan_stalled(ctx: &CheckCtx) -> Vec<FindingCandidate> {
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
        if !plan_has_open_task(p, &ctx.workspace.tasks) {
            continue;
        }
        let wt = plan_worktree(ws, p);
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

/// `plan.checkbox_mismatch` (Section 9): `declared=open` but `observed=committed`; or
/// `declared=done` and `observed=not_started` with `expected_paths` known. Severity `warn`.
fn check_checkbox_mismatch(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let mut out = Vec::new();
    for t in &ctx.workspace.tasks {
        let mut flag: Option<(String, Value)> = None;
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

/// `agent.loop` (Section 9): in a session's last `loop_window` tool calls, the same
/// `(tool, args_hash)` occurs >= `loop_repeats` times AND (>= 80% are errors OR identical
/// `result_hash`). Severity `warn`.
fn check_agent_loop(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let window = ctx.config.thresholds.loop_window as usize;
    let repeats = ctx.config.thresholds.loop_repeats as usize;
    if window == 0 || repeats == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for s in &ctx.workspace.sessions {
        let calls = &s.tool_calls;
        if calls.len() < window {
            continue;
        }
        let recent: Vec<&TC> = calls.iter().rev().take(window).collect();
        // Count occurrences of each (tool, args_hash) in the window.
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut error_counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for c in recent {
            let key = format!("{}:{}", c.tool, c.args_hash);
            *counts.entry(key.clone()).or_insert(0) += 1;
            if c.is_error {
                *error_counts.entry(key.clone()).or_insert(0) += 1;
            }
        }
        for (key, count) in &counts {
            if *count < repeats {
                continue;
            }
            let (parts0, parts1) = key.split_once(':').unwrap_or((key.as_str(), ""));
            let err_count = error_counts.get(key).copied().unwrap_or(0);
            _ = *count as f64;
            // >= 80% are errors (in the window). There is no result_hash column, so we use the
            // error-ratio gate only.
            let ok = (err_count as f64 / *count as f64) >= 0.8;
            if ok {
                out.push(FindingCandidate::new(
                    "agent.loop",
                    &format!("agent.loop:{}:{}:{}", s.id, parts0, parts1),
                    &format!("agent loop in session {} ({})", s.id, parts0),
                    &format!(
                        "session {} repeated {} {} times in the last {} tool calls",
                        s.id, parts0, count, window
                    ),
                    "warn",
                    json!({ "session": s.id, "tool": parts0, "args_hash": parts1, "count": count, "window": window }),
                    "inferred",
                ));
            }
        }
    }
    out
}

/// `agent.tokens` (Section 9): session token total exceeds `session_token_budget` (0 = disabled).
/// Severity `warn`. `dedupe_key = agent.tokens:<session>`.
fn check_agent_tokens(ctx: &CheckCtx) -> Vec<FindingCandidate> {
    let budget = ctx.config.thresholds.session_token_budget;
    if budget == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for s in &ctx.workspace.sessions {
        let total = s.tokens_in + s.tokens_out + s.tokens_cache;
        if (total as u64) > budget {
            out.push(FindingCandidate::new(
                "agent.tokens",
                &format!("agent.tokens:{}", s.id),
                &format!("agent tokens: {} exceeded budget", s.id),
                &format!("session {} used {} tokens (budget {})", s.id, total, budget),
                "warn",
                json!({ "session": s.id, "total": total, "budget": budget }),
                "explicit",
            ));
        }
    }
    out
}
