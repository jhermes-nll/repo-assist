//! Pure rendering of markdown views (Task 1 / §11).
//!
//! Rendering takes data and returns `String`; filesystem writes live in [`crate::views::write`].

use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::VERSION;

/// Maximum number of lines in a rendered view (§11).
pub const VIEW_CAP_LINES: usize = 150;

/// Data fed to the renderers (mirrors the existing JSON builders' outputs).
#[derive(Debug, Clone)]
pub struct ViewData {
    pub status: Value,
    pub plans: Value,
    pub findings: Value,
    pub sessions: Value,
}

/// Metadata written into every view header.
#[derive(Debug, Clone)]
pub struct ViewMeta {
    pub generated_at_ms: i64,
    pub version: &'static str,
}

impl ViewMeta {
    pub fn new(now_ms: i64) -> Self {
        ViewMeta {
            generated_at_ms: now_ms,
            version: VERSION,
        }
    }
}

pub mod write;

/// Render STATUS.md.
pub fn render_status(d: &ViewData, now_ms: i64) -> String {
    let mut lines: Vec<String> = Vec::new();
    header(&mut lines, &ViewMeta::new(now_ms));
    lines.push("# Status".to_string());
    lines.push(String::new());
    let wt_count = d
        .status
        .get("worktree_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let plans = d
        .status
        .get("active_plans")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let findings = d
        .status
        .get("open_findings")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let sessions = d
        .status
        .get("sessions")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    lines.push(format!(
        "worktrees: {wt_count}  active_plans: {plans}  open_findings: {findings}  sessions: {sessions}"
    ));
    if let Some(wts) = d.status.get("worktrees").and_then(Value::as_array) {
        for wt in wts {
            let p = wt.get("path").and_then(Value::as_str).unwrap_or("?");
            let b = wt.get("branch").and_then(Value::as_str).unwrap_or("-");
            let c = wt.get("changed").and_then(Value::as_i64).unwrap_or(0);
            lines.push(format!("  {p} ({b}): {c} changed"));
        }
    }
    cap(&mut lines, false);
    lines.join("\n") + "\n"
}

/// Render PLANS.md.
pub fn render_plans(d: &ViewData, now_ms: i64) -> String {
    let mut lines: Vec<String> = Vec::new();
    header(&mut lines, &ViewMeta::new(now_ms));
    lines.push("# Plans".to_string());
    lines.push(String::new());
    if let Some(plans) = d.plans.get("plans").and_then(Value::as_array) {
        for p in plans {
            let id = p.get("id").and_then(Value::as_str).unwrap_or("?");
            let title = p.get("title").and_then(Value::as_str).unwrap_or("");
            let state = p.get("state").and_then(Value::as_str).unwrap_or("-");
            let done = p.get("declared_done").and_then(Value::as_i64).unwrap_or(0);
            let total = p.get("task_count").and_then(Value::as_i64).unwrap_or(0);
            lines.push(format!("{id}  {state}  {title}  ({done}/{total})"));
        }
    } else {
        lines.push("no plans".to_string());
    }
    cap(&mut lines, false);
    lines.join("\n") + "\n"
}

/// Render FINDINGS.md: open findings first, then resolved.
pub fn render_findings(d: &ViewData, now_ms: i64) -> String {
    let mut lines: Vec<String> = Vec::new();
    header(&mut lines, &ViewMeta::new(now_ms));
    lines.push("# Findings".to_string());
    lines.push(String::new());

    // Partition open vs resolved/acked.
    let mut open_rows: Vec<&Value> = Vec::new();
    let mut other_rows: Vec<&Value> = Vec::new();
    if let Some(arr) = d.findings.get("findings").and_then(Value::as_array) {
        for f in arr {
            let state = f.get("state").and_then(Value::as_str).unwrap_or("open");
            if state == "open" {
                open_rows.push(f);
            } else {
                other_rows.push(f);
            }
        }
    }
    lines.push("## Open".to_string());
    if open_rows.is_empty() {
        lines.push("(none)".to_string());
    }
    for f in &open_rows {
        let kind = f.get("kind").and_then(Value::as_str).unwrap_or("?");
        let subject = f.get("subject").and_then(Value::as_str).unwrap_or("");
        let summary = f.get("summary").and_then(Value::as_str).unwrap_or("");
        lines.push(format!("- {kind}: {subject} — {summary}"));
    }
    lines.push(String::new());
    lines.push("## Resolved".to_string());
    if other_rows.is_empty() {
        lines.push("(none)".to_string());
    }
    for f in &other_rows {
        let kind = f.get("kind").and_then(Value::as_str).unwrap_or("?");
        let subject = f.get("subject").and_then(Value::as_str).unwrap_or("");
        lines.push(format!("- {kind}: {subject}"));
    }
    cap(&mut lines, true);
    lines.join("\n") + "\n"
}

fn header(out: &mut Vec<String>, meta: &ViewMeta) {
    let iso = iso8601(meta.generated_at_ms);
    out.push(format!("generated_at: {iso}"));
    out.push(format!("daemon: {}", meta.version));
    out.push("This file is auto-generated and read-only; it may be stale.".to_string());
}

fn iso8601(ms: i64) -> String {
    let secs = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as i32;
    if let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) {
        let dt = dt.replace_nanosecond(nanos as u32).unwrap_or(dt);
        dt.format(&Rfc3339).unwrap_or_else(|_| dt.to_string())
    } else {
        ms.to_string()
    }
}

fn cap(lines: &mut Vec<String>, _open_first: bool) {
    if lines.len() > VIEW_CAP_LINES {
        lines.truncate(VIEW_CAP_LINES - 1);
        lines.push(format!("[... truncated to {VIEW_CAP_LINES} lines ...]"));
    }
}

/// Truncate helper exposed for tests wanting explicit control.
pub fn cap_lines(lines: &mut Vec<String>) {
    cap(lines, false);
}
