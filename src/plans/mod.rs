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
pub mod derive;
pub mod discover;
pub mod superpowers;

pub use derive::{
    derive, derive_observed, plan_id, run_truth_table, DeriveCase, DeriveInput, EvidenceSeed,
    Observed, ObservedOutcome,
};

/// A single parsed task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Plan-scoped task id of the form `{plan_id}:{ordinal}`.
    ///
    /// Parsers leave `plan_id` empty, so the provisional id ends up `:{ordinal}`; the plan
    /// importer assigns the real plan id via [`Task::set_plan_id`], which rewrites this
    /// field. Scoping by the plan keeps `tasks.id` (a global primary key) unique across
    /// plans; using the ordinal (not the source heading number) keeps ids collision-free
    /// when a plan repeats or renumbers its task headings.
    ///
    /// Stability: re-importing the same document re-derives the same ids. Inserting or
    /// deleting a task *before* an existing one shifts its ordinal — and therefore its id —
    /// so the importer must expect an id re-derivation on structure changes (the old id's
    /// `observed`/`observed_reason` state is intentionally preserved on upsert, i.e. it
    /// follows the row, not the id).
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
    /// Assign the owning plan id and re-derive the plan-scoped [`Task::id`] (`{plan_id}:{ordinal}`).
    pub fn set_plan_id(&mut self, plan_id: &str) {
        self.plan_id = plan_id.to_string();
        self.id = task_id(plan_id, self.ordinal);
    }

    /// Borrow this task as a `store::TaskRow` (for the plan importer to upsert).
    ///
    /// The row's `id` is the plan-scoped [`Task::id`]; call [`Task::set_plan_id`] first so it
    /// includes the owning plan and cannot collide with another plan's task.
    pub fn to_task_row(&self) -> TaskRow<'_> {
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

/// Build the plan-scoped, stable task id `{plan_id}:{ordinal}`.
pub(crate) fn task_id(plan_id: &str, ordinal: i64) -> String {
    format!("{plan_id}:{ordinal}")
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
    // Preserve a plan title discovered by either parser so an unrecognized
    // document that still has a `# ` heading does not lose it.
    let title = if !sp.title.is_empty() {
        sp.title
    } else {
        cl.title
    };
    let msg = format!(
        "plan parser recognized 0 tasks in {} chars; treating as an unknown plan format",
        text.len()
    );
    tracing::warn!("{msg}");
    ParsedPlan {
        title,
        warnings: vec![msg],
        ..Default::default()
    }
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
/// A genuine checkbox marker is a single char (`x`/`X`/space) or empty; multi-char content is a
/// markdown link (`- [spec](http://x)`) and yields `None` so link bullets are not misread as
/// unchecked boxes. A single *unknown* marker (e.g. `- [~]`) is a checkbox the author never
/// ticked: it counts as unchecked. The char after `]` must be whitespace or end-of-line; the
/// label after `]` is ignored here (callers extract it separately).
pub(crate) fn parse_checkbox(line: &str) -> Option<bool> {
    let s = line
        .trim_start()
        .strip_prefix('-')?
        .trim_start()
        .strip_prefix('[')?;
    let close = s.find(']')?;
    let marker = &s[..close];
    if marker.chars().count() > 1 {
        return None; // multi-char: markdown link, not a checkbox
    }
    if let Some(next) = s.get(close + 1..) {
        if let Some(ch) = next.chars().next() {
            if !ch.is_whitespace() {
                return None;
            }
        }
    }
    // Empty / unknown single-char markers are unchecked; only x/X is checked.
    Some(matches!(marker, "x" | "X"))
}

/// Every default plan glob across registered adapters (used by the daemon to decide
/// whether a `worktree_dirty` event needs plan discovery).
pub fn plan_globs() -> Vec<String> {
    crate::plans::discover::plan_globs()
}
