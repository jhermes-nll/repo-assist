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

pub fn select_tick_lines(text: &str, committed: &BTreeSet<usize>) -> Vec<usize> {
    let mut result = Vec::new();
    let mut current_task: Option<usize> = None;
    let mut task_count = 0usize;
    let mut generic_idx = 0usize;
    let has_task_headings = text
        .lines()
        .any(crate::plans::superpowers::is_task_heading);

    for (i, line) in text.split_inclusive('\n').enumerate() {
        if crate::plans::superpowers::is_task_heading(line) {
            task_count += 1;
            current_task = Some(task_count);
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
        if !trimmed.starts_with("- [") {
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

// shortcut: fixed `.planname.tick.tmp` name races between concurrent ticks of the same plan and reuses a crashed prior run's tmp; use a unique temp name (e.g. PID + nonce) if concurrent/atomic-hardening is needed.
pub fn apply_atomic(path: &Path, new_text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tick.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("plan")
    ));
    // Preserve the original file mode so we never widen (e.g. 0600 -> 0644) a plan.
    let perms = fs::metadata(path).map(|m| m.permissions()).ok();
    fs::write(&tmp, new_text)?;
    if let Some(p) = perms {
        let _ = fs::set_permissions(&tmp, p);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}
