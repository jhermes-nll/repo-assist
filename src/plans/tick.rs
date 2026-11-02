//! Plan checkbox tick-back (write progress back to plan files).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub fn tick_one_line(line: &str) -> Option<String> {
    // Locate the checkbox marker the shared parser would accept, and only rewrite an
    // unchecked *space* marker in place (so an inner "[ ]" in the label is never touched).
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('-')?;
    let ws_len = rest.len() - rest.trim_start().len();
    let rest = &rest[ws_len..];
    if !rest.starts_with('[') {
        return None;
    }
    let close = rest.find(']')?;
    let marker = &rest[1..close];
    if marker != " " {
        return None;
    }
    if let Some(next) = rest.get(close + 1..) {
        if let Some(ch) = next.chars().next() {
            if !ch.is_whitespace() {
                return None;
            }
        }
    }
    // Position of '[' in the original line.
    let open_idx = line.len() - rest.len();
    let end = open_idx + 3; // "[ ]" is 3 bytes
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..open_idx]);
    out.push_str("[x]");
    out.push_str(&line[end..]);
    Some(out)
}

pub fn rewrite_lines(text: &str, to_tick: &[usize]) -> String {
    let set: BTreeSet<usize> = to_tick.iter().copied().collect();
    let parts: Vec<&str> = text.split_inclusive('\n').collect();
    let mut buf = Vec::with_capacity(parts.len());
    for (i, part) in parts.into_iter().enumerate() {
        if set.contains(&i) {
            if let Some(new) = tick_one_line(part) {
                buf.push(new);
                continue;
            }
        }
        buf.push(part.to_string());
    }
    buf.concat()
}

pub fn select_tick_lines(text: &str, committed: &BTreeSet<usize>) -> Vec<usize> {
    let mut result = Vec::new();
    let mut current_task: Option<usize> = None;
    let mut task_count = 0usize;
    let has_task_headings = text.lines().any(crate::plans::superpowers::is_task_heading);

    // Without task headings there is no reliable mapping from committed store rows to
    // checkboxes; never tick in that case.
    if !has_task_headings {
        return result;
    }

    for (i, line) in text.split_inclusive('\n').enumerate() {
        if crate::plans::superpowers::is_task_heading(line) {
            task_count += 1;
            current_task = Some(task_count);
            continue;
        }
        // Only genuine checkboxes; the checked/unchecked decision stays in parse_checkbox.
        let checked = match crate::plans::parse_checkbox(line) {
            Some(b) => b,
            None => continue,
        };

        // Only unchecked *space* boxes are tickable.
        if checked || tick_one_line(line).is_none() {
            continue;
        }

        if current_task
            .map(|cur| committed.contains(&cur))
            .unwrap_or(false)
        {
            result.push(i);
        }
    }
    result
}

pub fn ticked_task_count(text: &str, lines: &[usize]) -> usize {
    let set: std::collections::BTreeSet<usize> = lines.iter().copied().collect();
    let mut current_task: Option<usize> = None;
    let mut task_count = 0usize;
    let mut tasks = std::collections::BTreeSet::new();
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if crate::plans::superpowers::is_task_heading(line) {
            task_count += 1;
            current_task = Some(task_count);
            continue;
        }
        if crate::plans::parse_checkbox(line).is_some() && set.contains(&i) {
            if let Some(cur) = current_task {
                tasks.insert(cur);
            }
        }
    }
    tasks.len()
}

pub fn is_git_dirty(wt: &Path, rel: &str) -> bool {
    let out = std::process::Command::new("git")
        .current_dir(wt)
        .args(["status", "--porcelain", "--", rel])
        .output();
    let dirty = match out {
        Ok(o) => !o.status.success() || !o.stdout.is_empty(),
        Err(_) => true,
    };
    if dirty {
        return true;
    }
    // Gitignored/excluded files never show in status; refuse them too.
    let ignored = std::process::Command::new("git")
        .current_dir(wt)
        .args(["check-ignore", "-q", "--", rel])
        .status();
    matches!(ignored, Ok(s) if s.success())
}

// shortcut: concurrent ticks of the same plan can race on the rename; use a unique temp name (e.g. PID + nonce) if atomic-hardening is needed.
pub fn apply_atomic(path: &Path, new_text: &str, root: Option<&Path>) -> std::io::Result<()> {
    let canonical = path
        .canonicalize()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
    if let Some(root) = root {
        let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if !canonical.starts_with(&root_canonical) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "canonical target resolves outside repo/worktree root",
            ));
        }
    }
    // Deleted-between-read-and-write: metadata must exist.
    if !canonical.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "plan file no longer exists",
        ));
    }
    let dir = canonical.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tick.tmp",
        canonical
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("plan")
    ));
    let perms = fs::metadata(&canonical).map(|m| m.permissions()).ok();
    fs::write(&tmp, new_text)?;
    if let Some(p) = perms {
        let _ = fs::set_permissions(&tmp, p);
    }
    fs::rename(&tmp, &canonical)?;
    Ok(())
}
