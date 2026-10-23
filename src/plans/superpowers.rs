//! Superpowers plan parser (Phase 3.1).
//!
//! Parse a superpowers plan Markdown file: a plan title (first `# ` heading) plus one task
//! per `# {2,4}` heading matching `Task \d+`. Within each task, `Create:`/`Modify:`/`Test:`/
//! `Delete:` path declarations populate `expected_paths` (paths are stripped of `:line`,
//! `:line-line` suffixes and any trailing `(notes)`), and `- [ ]`/`- [x]` checkboxes set the
//! declared state (a task is `done` only if it has at least one checkbox and all of them
//! are `[x]`).
//!
//! When the document contains no `Task N` headings this parser returns zero tasks (no
//! warning here — the auto [`crate::plans::parse_plan`] decides it is "unknown" and warns).

use crate::plans::strip_heading;
use crate::plans::{task_id, ParsedPlan, Task};

/// The `Create:`/`Modify:`/`Test:`/`Delete:`/`Update:` directive keywords that introduce a
/// path declaration within a task.
const PATH_DIRECTIVES: &[&str] = &["create", "modify", "test", "delete", "update"];

/// Parse superpowers-plan **content** (see module docs).
///
/// Returns zero tasks when no `Task N` heading is present.
pub fn parse(text: &str) -> ParsedPlan {
    let mut plan = ParsedPlan::default();
    let mut plan_title = String::new();

    // Per-task accumulator fields.
    let mut have_task = false;
    let mut cur_title = String::new();
    let mut cur_checked: u32 = 0;
    let mut cur_total: u32 = 0;
    let mut cur_paths: Vec<String> = Vec::new();

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

        // A task heading (`# {2,4} Task \d+`)? Start a new task (flush the previous one).
        if let Some((_num, title)) = parse_task_heading(trimmed) {
            if have_task {
                tasks.push(task_from(
                    &cur_title,
                    cur_checked,
                    cur_total,
                    &cur_paths,
                    tasks.len() as i64 + 1,
                ));
            }
            cur_title = title;
            cur_checked = 0;
            cur_total = 0;
            cur_paths.clear();
            have_task = true;
            continue;
        }

        if have_task {
            if let Some(checked) = crate::plans::parse_checkbox(trimmed) {
                cur_total += 1;
                if checked {
                    cur_checked += 1;
                }
                continue;
            }
            if let Some(paths) = parse_path_decl(trimmed) {
                cur_paths.extend(paths.into_iter().filter(|p| !p.is_empty()));
            }
        }
    }
    if have_task {
        tasks.push(task_from(
            &cur_title,
            cur_checked,
            cur_total,
            &cur_paths,
            tasks.len() as i64 + 1,
        ));
    }

    plan.title = plan_title;
    plan.tasks = tasks;
    plan
}

/// Build one `Task` from its accumulated state.
#[inline]
fn task_from(title: &str, checked: u32, total: u32, paths: &[String], ordinal: i64) -> Task {
    let declared = if total > 0 && checked == total {
        "done"
    } else {
        "open"
    };
    let plan_id = String::new();
    Task {
        id: task_id(&plan_id, ordinal),
        plan_id,
        // 1-based ordering (see store::TaskRow.ordinal).
        ordinal,
        title: title.to_string(),
        declared: declared.to_string(),
        expected_paths: paths.to_vec(),
        depends_on: Vec::new(),
    }
}

/// Match a task heading `# {2,4} Task \d+...`, returning `(number, title)`.
fn parse_task_heading(line: &str) -> Option<(String, String)> {
    let mut hashes = 0usize;
    let mut rest = line;
    while rest.starts_with('#') {
        hashes += 1;
        rest = &rest[1..];
    }
    if !(2..=4).contains(&hashes) {
        return None;
    }
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("Task")?;
    let rest = rest.trim_start();

    // Collect the leading digits after "Task".
    let mut digits = String::new();
    let mut idx = 0usize;
    let bytes = rest.as_bytes();
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        digits.push(bytes[idx] as char);
        idx += 1;
    }
    if digits.is_empty() {
        return None;
    }

    // Title = remainder, dropping a leading `:`.
    let title = rest[idx..].trim_start_matches(':').trim().to_string();
    let title = if title.is_empty() {
        format!("Task {digits}")
    } else {
        title
    };
    Some((digits, title))
}

/// Parse a path-declaration line (e.g. `Create: `src/foo.rs` (add caching)) into cleaned
/// path(s). Returns `None` when the line is not a recognized directive, or has no path.
fn parse_path_decl(line: &str) -> Option<Vec<String>> {
    let s = line.trim();

    // Leading word (the directive keyword).
    let mut kw_end = 0usize;
    let bytes = s.as_bytes();
    while kw_end < bytes.len() && bytes[kw_end].is_ascii_alphabetic() {
        kw_end += 1;
    }
    let word = s[..kw_end].to_ascii_lowercase();
    if !PATH_DIRECTIVES.contains(&word.as_str()) {
        return None;
    }

    let after = s[kw_end..].trim_start();
    let after = after.strip_prefix(':')?;
    let paths = extract_backticked(after)?;
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// Extract every `` `...` `` span from `s`, cleaning each. Returns `None` when there is no
/// backticked content at all (so the caller can distinguish "no directive" from "directive
/// with no path" — both collapse to no paths here).
fn extract_backticked(s: &str) -> Option<Vec<String>> {
    let mut paths = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        // A missing closing backtick is tolerated: take the rest of the line as the path so
        // paths already collected on this directive are not discarded.
        let (inner, next) = match after.find('`') {
            Some(close) => (&after[..close], &after[close + 1..]),
            None => (after, ""),
        };
        if let Some(cleaned) = clean_path(inner) {
            if !cleaned.is_empty() {
                paths.push(cleaned);
            }
        }
        rest = next;
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

/// Clean a single backticked path: strip a trailing `(notes)` note and a trailing `:line` /
/// `:line-line` range. Returns `None` when nothing meaningful remains.
fn clean_path(inner: &str) -> Option<String> {
    let mut p = inner.trim();

    // Drop a trailing parenthetical note, e.g. "src/foo.rs (add caching)".
    if let Some(b) = p.find('(') {
        p = &p[..b];
    }

    // Drop a trailing ":12" or ":12-34" line-range suffix.
    if let Some(colon) = p.rfind(':') {
        let tail = &p[colon + 1..];
        if is_line_range(tail) {
            p = &p[..colon];
        }
    }

    let p = p.trim();
    if p.is_empty() {
        None
    } else {
        Some(p.to_string())
    }
}

/// Whether `s` is a `NN` or `NN-MM` line range.
fn is_line_range(s: &str) -> bool {
    let mut count = 0;
    let mut minus_seen = false;
    let mut saw_digit = false;
    for c in s.chars() {
        if c == '-' {
            if minus_seen || count == 0 {
                return false;
            }
            minus_seen = true;
        } else if c.is_ascii_digit() {
            saw_digit = true;
            count += 1;
        } else {
            return false;
        }
    }
    saw_digit && count <= 8
}
