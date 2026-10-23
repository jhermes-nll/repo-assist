//! Generic checklist parser (Phase 3.1).
//!
//! Treats a file as a single plan whose tasks are **every** top-level checkbox (`- [ ]`/
//! `- [x]`), one task per item, with empty `expected_paths`. Used for generic checklist
//! files (`TODO.md`, `PLAN.md`, `docs/plans/**/*.md`); checks that require `expected_paths`
//! simply skip these tasks.

use crate::plans::{parse_checkbox, strip_heading, ParsedPlan, Task};

/// Parse generic-checklist **content** (see module docs).
pub fn parse(text: &str) -> ParsedPlan {
    let mut plan = ParsedPlan::default();
    let mut plan_title = String::new();
    let mut tasks: Vec<Task> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if plan_title.is_empty() {
            if let Some(inner) = strip_heading(trimmed, 1) {
                plan_title = inner;
            }
        }

        if let Some(checked) = parse_checkbox(trimmed) {
            let declared = if checked { "done" } else { "open" };
            let label = checkbox_label(trimmed);
            let ordinal = tasks.len() as i64 + 1;
            tasks.push(Task {
                id: ordinal.to_string(),
                plan_id: String::new(),
                ordinal,
                title: if label.is_empty() {
                    format!("Task {ordinal}")
                } else {
                    label
                },
                declared: declared.to_string(),
                expected_paths: Vec::new(),
                depends_on: Vec::new(),
            });
        }
    }

    plan.title = plan_title;
    plan.tasks = tasks;
    plan
}

/// The text following a checkbox marker (`- [x] fix the bug` -> "fix the bug"), or an empty
/// string when the line is not a checkbox.
fn checkbox_label(line: &str) -> String {
    let s = line.trim_start();
    let s = match s.strip_prefix('-') {
        Some(s) => s.trim_start(),
        None => return String::new(),
    };
    let s = match s.strip_prefix('[') {
        Some(s) => s,
        None => return String::new(),
    };
    let close = match s.find(']') {
        Some(c) => c,
        None => return String::new(),
    };
    // The label is the text *after* the closing bracket, not the marker inside it.
    s[close + 1..].trim().to_string()
}
