//! Plan-document parsers (Phase 3.1).
//!
//! Parse a plan document — a superpowers checklist file, or a generic checklist — into a
//! canonical [`ParsedPlan`] of [`Task`]s, each carrying a declared state (`open`/`done`) and
//! a list of expected paths.
//!
//! # Robustness (docs/verified-facts.md Item 2)
//!
//! Every parser is total: it never panics on malformed input, and an *unknown* format
//! produces a plan with zero tasks **plus a logged warning** (`tracing::warn!`) rather than
//! an error. The daemon's future plan importer converts [`Task`]s into `store::TaskRow`s
//! via [`Task::to_task_row`].

use std::path::Path;

use crate::store::TaskRow;

pub mod checklist;
pub mod superpowers;

/// A single parsed task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Stable task id (e.g. `1`, `2`, …); the heading number for superpowers, the per-plan
    /// index for the generic checklist format.
    pub id: String,
    /// The plan this task belongs to (populated by the plan importer, not the parser).
    pub plan_id: String,
    /// 1-based ordering (also `ordinal` in `store::TaskRow`).
    pub ordinal: i64,
    /// The task's human title (text after the `Task N` heading, or the checkbox label).
    pub title: String,
    /// Declared state: `"done"` or `"open"`.
    pub declared: String,
    /// Paths this task is expected to touch (`Create:`/`Modify:`/`Test:`/`Delete:`).
    pub expected_paths: Vec<String>,
    /// Tasks this task depends on (task ids), if any.
    pub depends_on: Vec<String>,
}

impl Task {
    /// Borrow this task as a `store::TaskRow` (for the plan importer to upsert).
    pub fn to_task_row<'a>(&'a self) -> TaskRow<'a> {
        TaskRow {
            id: &self.id,
            plan_id: &self.plan_id,
            ordinal: self.ordinal,
            title: &self.title,
            declared: &self.declared,
            expected_paths: &self.expected_paths,
            depends_on: &self.depends_on,
        }
    }
}

/// A parsed plan document: a title, an ordered list of [`Task`]s, and any warnings logged
/// while recognizing nothing (never panics — an unknown format yields zero tasks).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedPlan {
    /// Plan title (first `# ` heading). Empty when no heading was found.
    pub title: String,
    /// Ordered tasks.
    pub tasks: Vec<Task>,
    /// Human-readable warning(s) describing why the document was treated as "no tasks" —
    /// empty for a successfully parsed plan. Also echoed to the log via `tracing::warn!`.
    pub warnings: Vec<String>,
}

impl ParsedPlan {
    /// Number of tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// The plan's declared state: `done` only if every task is `done` and there is at least
    /// one task; otherwise `open` (including the zero-task / unknown case).
    pub fn declared_state(&self) -> &str {
        if self.tasks.is_empty() {
            return "open";
        }
        if self.tasks.iter().all(|t| t.declared == "done") {
            "done"
        } else {
            "open"
        }
    }
}

/// Parse a plan document's **content** — the canonical entry point.
///
/// Tries the superpowers format first (structured `Task N` headings + path declarations),
/// then the generic checklist format (one task per checkbox); if neither recognizes
/// anything the result is a zero-task plan **with a logged warning** (never a panic).
pub fn parse_plan(text: &str) -> ParsedPlan {
    // 1. Superpowers: tasks only when `Task N` headings are present.
    let sp = superpowers::parse(text);
    if !sp.tasks.is_empty() {
        return sp;
    }
    // 2. Generic checklist: one task per checkbox (no Task headings needed).
    let cl = checklist::parse(text);
    if !cl.tasks.is_empty() {
        return cl;
    }
    // 3. Unknown format: zero tasks + a logged/recorded warning.
    let msg = format!(
        "plan parser recognized 0 tasks in {} chars; treating as an unknown plan format",
        text.len()
    );
    tracing::warn!("{msg}");
    let mut out = ParsedPlan::default();
    out.warnings.push(msg);
    out
}

/// Parse a plan document **file** from disk (returns zero tasks if unreadable, logging a
/// note rather than panicking).
pub fn parse_plan_file(path: &Path) -> ParsedPlan {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_plan(&text),
        Err(e) => {
            tracing::warn!("plan file could not be read ({}): {}", path.display(), e);
            ParsedPlan::default()
        }
    }
}

/// Strip a level-`level` heading marker, returning the inner text (no warning on no match).
///
/// Only matches when there are exactly `level` leading hashes followed by a space, so `##`
/// does not satisfy a request for level 1.
pub(crate) fn strip_heading(line: &str, level: usize) -> Option<String> {
    let mut hashes = 0usize;
    let mut rest = line;
    while rest.starts_with('#') {
        hashes += 1;
        rest = &rest[1..];
    }
    if hashes == level && rest.starts_with(' ') {
        Some(rest[1..].trim().to_string())
    } else {
        None
    }
}

/// Parse a `- [ ]` / `- [x]` checkbox line, returning whether it is checked (false for `- [ ]`).
///
/// The label after the closing `]` is ignored here; callers extract it separately. Unknown
/// markers (e.g. `- [~]`) are treated as unchecked.
pub(crate) fn parse_checkbox(line: &str) -> Option<bool> {
    let s = line
        .trim_start()
        .strip_prefix('-')?
        .trim_start()
        .strip_prefix('[')?;
    let close = s.find(']')?;
    let marker = s[..close].trim();
    if marker.is_empty() {
        return Some(false); // `- [ ]`
    }
    match marker.chars().next().unwrap() {
        'x' | 'X' => Some(true),
        _ => Some(false),
    }
}
