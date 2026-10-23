//! Git source (Task 2.1): shell out to `git` (never libgit2), always `-C <path>`,
//! `--porcelain`/`-z` forms, `LC_ALL=C`, `GIT_OPTIONAL_LOCKS=0` for status (Section 3).
//!
//! `sync_worktrees` discovers all worktrees of the repo containing `repo_root`, upserts
//! `worktrees` rows (branch, head, base ref, merge base), replaces each worktree's
//! changed-file set (`worktree_files`: committed | staged | unstaged | untracked), and marks
//! vanished worktrees not present.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::store::Store;

/// Run `git` in `cwd`; `None` on any failure or non-zero exit.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .env("LC_ALL", "C")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Run a read-only `git` command without taking the index lock (Section 3).
fn git_ro(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// A discovered worktree from `git worktree list --porcelain -z`.
#[derive(Debug, Clone)]
pub struct Wt {
    /// Absolute worktree path.
    pub path: PathBuf,
    /// Branch name, without `refs/heads/` (None when detached).
    pub branch: Option<String>,
    /// Commit id, when reported.
    pub head: Option<String>,
    /// Whether this worktree is bare.
    pub bare: bool,
    /// Whether this worktree is detached.
    pub detached: bool,
}

/// Parse `git worktree list --porcelain -z`: records are separated by NUL-NUL, and each
/// record's lines are NUL-terminated.
pub fn parse_worktree_list(data: &str) -> Vec<Wt> {
    let mut out: Vec<Wt> = Vec::new();
    for record in data.split("\u{0}\u{0}") {
        let lines: Vec<&str> = record
            .split('\u{0}')
            .filter(|l| !l.trim().is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        // Lines: `worktree <path>` (plus optional `bare`/`detached` line), `HEAD <sha>`,
        // `branch <ref>`.
        let first = lines[0].trim();
        let (path, attrs) = match first.strip_prefix("worktree ") {
            Some(rest) => match rest.split_once(' ') {
                Some((p, a)) => (p.to_string(), a.to_string()),
                None => (rest.to_string(), String::new()),
            },
            None => (first.to_string(), String::new()),
        };
        let mut wt = Wt {
            path: PathBuf::from(path),
            branch: None,
            head: None,
            bare: attrs.contains("bare"),
            detached: attrs.contains("detached"),
        };
        for line in &lines[1..] {
            let line = line.trim();
            if let Some(b) = line.strip_prefix("branch ") {
                wt.branch = Some(b.trim_start_matches("refs/heads/").to_string());
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                wt.head = Some(h.trim().to_string());
            }
        }
        out.push(wt);
    }
    out
}

/// Discover all worktrees of the repo containing `repo_root` (Task 2.1).
pub fn discover_worktrees(repo_root: &Path) -> Vec<Wt> {
    git(repo_root, &["worktree", "list", "--porcelain", "-z"])
        .map(|data| parse_worktree_list(&data))
        .unwrap_or_default()
}

/// Detect the base branch ref (Section 10): `origin/HEAD` when resolvable, else `main`,
/// else `master`.
fn detect_base_ref(repo_root: &Path) -> Option<String> {
    for candidate in ["origin/HEAD", "main", "master"] {
        if git(repo_root, &["rev-parse", "--verify", "--quiet", candidate]).is_some() {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Unix milliseconds now (UTC, stored as INTEGER, Section 3).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Everything `upsert_worktree` needs for one row.
struct WtRow<'a> {
    path: &'a str,
    branch: Option<&'a str>,
    head: Option<&'a str>,
    base_ref: Option<&'a str>,
    is_main: bool,
}

/// Upsert one worktree row (idempotent on `path`).
fn upsert_worktree(
    conn: &rusqlite::Connection,
    row: &WtRow<'_>,
    now_ms: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO worktrees(path, branch, head, base_ref, merge_base, is_main, present, \
         last_activity_ms, updated_ms) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, 1, NULL, ?6) \
         ON CONFLICT(path) DO UPDATE SET \
           branch = excluded.branch, head = excluded.head, base_ref = excluded.base_ref, \
           is_main = excluded.is_main, present = 1, updated_ms = excluded.updated_ms",
        rusqlite::params![
            row.path,
            row.branch,
            row.head,
            row.base_ref,
            row.is_main as i64,
            now_ms
        ],
    )?;
    Ok(())
}

/// Replace one worktree's changed-file set with `rows` of `(path, origin)`.
fn replace_files(
    conn: &rusqlite::Connection,
    worktree_id: i64,
    rows: &[(String, String)],
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM worktree_files WHERE worktree_id = ?1",
        [worktree_id],
    )?;
    for (path, origin) in rows {
        conn.execute(
            "INSERT OR IGNORE INTO worktree_files(worktree_id, path, origin) VALUES (?1, ?2, ?3)",
            rusqlite::params![worktree_id, path, origin],
        )?;
    }
    Ok(())
}

/// Sync worktrees + changed-file sets into the store (Task 2.1 entry point).
/// The commit history of one worktree (Section 9): `sha` -> the set of paths each commit
/// introduced, for commits in `merge_base..HEAD` on the plan branch.
///
/// Returns an empty map when git returns nothing (the worktree is at the base ref).
pub fn plan_commit_history(wt: &Path, base_ref: Option<&str>) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let Some(base) = base_ref else {
        return out;
    };
    // Reverse the log so an earlier commit is listed first; newest wins on collision.
    let Some(data) = git_ro(
        wt,
        &[
            "log",
            "--no-merges",
            "--reverse",
            "--format=%H",
            "--name-only",
            &format!("{base}...HEAD"),
        ],
    ) else {
        return out;
    };
    let mut active: Option<String> = None;
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // A line that parses as a 40-char hex SHA starts a new commit (git
        // `--format=%H` emits lowercase hex; anything else is a path line).
        if line.len() == 40 && line.chars().all(|c| c.is_ascii_hexdigit()) {
            active = Some(line.to_string());
            out.entry(line.to_string()).or_default();
        } else if let Some(sha) = active.as_ref() {
            out.entry(sha.clone()).or_default().push(line.to_string());
        }
    }
    out
}

pub fn sync_worktrees(store: &Store, repo_root: &Path) -> Result<(), crate::store::StoreError> {
    let wts = discover_worktrees(repo_root);
    let base_ref = detect_base_ref(repo_root);
    let ms = now_ms();

    // Phase A (single write txn): upsert rows and mark all as not-present for now.
    store.write(|conn| {
        for wt in &wts {
            let path_s = wt.path.display().to_string();
            // is_main: the worktree whose branch is the base branch (main/master) OR whose
            // path is the repo root itself. Both heuristics agree in the common case.
            let is_main = match base_ref.as_deref() {
                Some(b) => wt.branch.as_deref() == Some(b) || wt.path == repo_root,
                None => wt.path == repo_root,
            };
            upsert_worktree(
                conn,
                &WtRow {
                    path: &path_s,
                    branch: wt.branch.as_deref(),
                    head: wt.head.as_deref(),
                    base_ref: base_ref.as_deref(),
                    is_main,
                },
                ms,
            )?;
        }
        Ok(())
    })?;

    // Phase B: per-worktree git probes (slow) resolved to ids, then one write txn.
    for wt in &wts {
        let head = wt
            .head
            .clone()
            .or_else(|| git(&wt.path, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()));
        let mb = head.as_deref().and_then(|h| {
            base_ref
                .as_deref()
                .and_then(|b| git(&wt.path, &["merge-base", b, h]).map(|s| s.trim().to_string()))
        });
        let rows = changed_file_set(&wt.path, wt.branch.as_deref(), base_ref.as_deref());
        store.write(|conn| {
            let id: i64 = conn.query_row(
                "SELECT id FROM worktrees WHERE path = ?1",
                [&wt.path.display().to_string()],
                |r| r.get(0),
            )?;
            conn.execute(
                "UPDATE worktrees SET head = ?2, merge_base = ?3, updated_ms = ?4 WHERE id = ?1",
                rusqlite::params![id, head, mb, ms],
            )?;
            replace_files(conn, id, &rows)?;
            Ok(())
        })?;
    }

    // Phase C: mark not-present any stored row no longer discovered.
    let known: Vec<String> = wts.iter().map(|w| w.path.display().to_string()).collect();
    store.write(|conn| {
        let mut stmt = conn.prepare("SELECT id, path FROM worktrees WHERE present = 1")?;
        let mut rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut gone = Vec::new();
        for r in rows.by_ref() {
            let (id, path) = r?;
            if !known.contains(&path) {
                gone.push(id);
            }
        }
        drop(rows);
        for id in gone {
            conn.execute(
                "UPDATE worktrees SET present = 0, updated_ms = ?2 WHERE id = ?1",
                rusqlite::params![id, ms],
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

/// The changed-file set of one worktree (Section 5/9):
/// committed-vs-merge-base UNION staged UNION unstaged UNION untracked.
///
/// Public so the daemon IPC handler can recompute a single plan's worktree changed set
/// (Task 3) without re-syncing the whole repo. Ordering and tagging match the private
/// `changed_files`, which now defers here for the internal sync path.
pub fn changed_file_set(
    wt: &Path,
    branch: Option<&str>,
    base_ref: Option<&str>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    // Committed changes on the branch vs merge base.
    if let Some(b) = base_ref {
        // Branch name when known; else HEAD works for the current checkout.
        let tip = branch.unwrap_or("HEAD");
        if let Some(diff) = git_ro(wt, &["diff", "--name-only", &format!("{b}...{tip}")]) {
            for line in diff.lines() {
                if !line.trim().is_empty() {
                    out.push((line.trim().to_string(), "committed".to_string()));
                }
            }
        }
    }
    // Status: staged / unstaged / untracked via porcelain v2 -z.
    if let Some(data) = git_ro(
        wt,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    ) {
        for tok_str in data.split('\u{0}') {
            let t = tok_str.trim_start();
            if t.is_empty() {
                continue;
            }
            let mut parts = t.splitn(2, ' ');
            let code = parts.next().unwrap_or("");
            let rest = parts.next().unwrap_or("");
            match code {
                "1" => {
                    // ordinary change: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
                    let f: Vec<&str> = rest.split_whitespace().collect();
                    let xy = f.first().copied().unwrap_or("");
                    let path = f.last().copied().unwrap_or("");
                    if path.is_empty() {
                        continue;
                    }
                    let x = xy.chars().next().unwrap_or('.');
                    let y = xy.chars().nth(1).unwrap_or('.');
                    if x != '.' && x != '?' {
                        out.push((path.to_string(), "staged".to_string()));
                    }
                    if y != '.' && y != '?' {
                        out.push((path.to_string(), "unstaged".to_string()));
                    }
                }
                "2" => {
                    // rename/copy: 2 <XY> ... <path><NUL><origpath>; -z keeps both.
                    let f: Vec<&str> = rest.split_whitespace().collect();
                    let xy = f.first().copied().unwrap_or("");
                    let path = f.last().copied().unwrap_or("");
                    let x = xy.chars().next().unwrap_or('.');
                    let y = xy.chars().nth(1).unwrap_or('.');
                    if !path.is_empty() && x != '.' && x != '?' {
                        out.push((path.to_string(), "staged".to_string()));
                    }
                    if !path.is_empty() && y != '.' && y != '?' {
                        out.push((path.to_string(), "unstaged".to_string()));
                    }
                }
                "u" => {
                    // unmerged: u <XY> ... <path>
                    let f: Vec<&str> = rest.split_whitespace().collect();
                    if let Some(path) = f.last() {
                        out.push((path.to_string(), "unstaged".to_string()));
                    }
                }
                "?" if !rest.trim().is_empty() => {
                    // untracked: ? <path>
                    out.push((rest.trim().to_string(), "untracked".to_string()));
                }
                _ => {}
            }
        }
    }
    out
}
