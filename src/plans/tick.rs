//! Plan checkbox tick-back (write progress back to plan files).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub fn tick_one_line(line: &str) -> Option<String> {
    if line.contains("[ ]") {
        Some(line.replacen("[ ]", "[x]", 1))
    } else {
        None
    }
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

fn task_heading_ordinal(line: &str) -> Option<usize> {
    let t = line.trim();
    let mut hashes = 0usize;
    let mut rest = t;
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
    let mut digits = String::new();
    for b in rest.as_bytes() {
        if b.is_ascii_digit() {
            digits.push(*b as char);
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

pub fn select_tick_lines(text: &str, committed: &BTreeSet<usize>) -> Vec<usize> {
    let mut result = Vec::new();
    let mut current_task: Option<usize> = None;
    let mut generic_idx = 0usize;
    let has_task_headings = text.lines().any(|l| task_heading_ordinal(l).is_some());

    for (i, line) in text.split_inclusive('\n').enumerate() {
        if let Some(ord) = task_heading_ordinal(line) {
            current_task = Some(ord);
            continue;
        }
        if !line.starts_with('-') {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- [") {
            continue;
        }
        let marker_ok = {
            let s = trimmed.strip_prefix('-').unwrap_or("").trim_start();
            let s = s.strip_prefix('[').unwrap_or("");
            matches!(s.find(']'), Some(c) if c <= 1)
        };
        if !marker_ok {
            continue;
        }
        if !line.contains("[ ]") {
            continue;
        }
        let is_committed = if has_task_headings {
            if let Some(cur) = current_task {
                committed.contains(&cur)
            } else {
                false
            }
        } else {
            generic_idx += 1;
            committed.contains(&generic_idx)
        };
        if is_committed {
            result.push(i);
        }
    }
    result
}

pub fn is_git_dirty(wt: &Path, rel: &str) -> bool {
    let out = std::process::Command::new("git")
        .current_dir(wt)
        .args(["status", "--porcelain", "--", rel])
        .output();
    match out {
        Ok(o) => !o.stdout.is_empty(),
        Err(_) => true,
    }
}

pub fn apply_atomic(path: &Path, new_text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tick.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("plan")
    ));
    fs::write(&tmp, new_text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
