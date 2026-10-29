//! Plan discovery per worktree (Section 8.1, Task 3.2): scan plan globs per adapter,
//! parse, upsert plans + tasks, mark vanished plans `gone`, then recompute each
//! task's `observed` state from git.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::plans::derive::{self, DeriveInput, ObservedOutcome};
use crate::plans::ParsedPlan;
use crate::sources::git;
use crate::store::{EvidenceRow, PlanRow, Store, StoreError};

/// Adapter that turns a.framework's plan files into the canonical [`ParsedPlan`].
pub trait PlanAdapter {
    fn name(&self) -> &str;
    fn globs(&self) -> &[String];
    fn parse(&self, text: &str) -> ParsedPlan;
}

impl<T: PlanAdapter + ?Sized> PlanAdapter for &T {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn globs(&self) -> &[String] {
        (**self).globs()
    }
    fn parse(&self, text: &str) -> ParsedPlan {
        (**self).parse(text)
    }
}

impl<T: PlanAdapter + ?Sized> PlanAdapter for Box<T> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn globs(&self) -> &[String] {
        (**self).globs()
    }
    fn parse(&self, text: &str) -> ParsedPlan {
        (**self).parse(text)
    }
}

/// Superpowers adapter: default glob `docs/superpowers/plans/*.md`.
#[derive(Clone, Debug)]
pub struct SuperpowersAdapter {
    pub globs: Vec<String>,
}

impl Default for SuperpowersAdapter {
    fn default() -> Self {
        Self {
            globs: vec!["docs/superpowers/plans/*.md".to_string()],
        }
    }
}

impl PlanAdapter for SuperpowersAdapter {
    fn name(&self) -> &str {
        "superpowers"
    }
    fn globs(&self) -> &[String] {
        &self.globs
    }
    fn parse(&self, text: &str) -> ParsedPlan {
        crate::plans::superpowers::parse(text)
    }
}

/// Generic checklist adapter.
#[derive(Clone, Debug)]
pub struct ChecklistAdapter {
    pub globs: Vec<String>,
}

impl Default for ChecklistAdapter {
    fn default() -> Self {
        Self {
            globs: vec![
                "TODO.md".to_string(),
                "PLAN.md".to_string(),
                "docs/plans/**/*.md".to_string(),
            ],
        }
    }
}

impl PlanAdapter for ChecklistAdapter {
    fn name(&self) -> &str {
        "checklist"
    }
    fn globs(&self) -> &[String] {
        &self.globs
    }
    fn parse(&self, text: &str) -> ParsedPlan {
        crate::plans::checklist::parse(text)
    }
}

/// `plans.source` value for spec/design documents.
pub const SPEC_SOURCE: &str = "spec";

/// Spec adapter: default glob `docs/**/*.md`. Same task/declared-state rules as plans
/// (superpowers headings first, then checklist boxes); a spec with zero tasks is stored
/// `unknown` by `sync_plans`. Uses the silent parsers, not `parse_plan`.
#[derive(Clone, Debug)]
pub struct SpecAdapter {
    pub globs: Vec<String>,
}

impl Default for SpecAdapter {
    fn default() -> Self {
        Self {
            globs: vec!["docs/**/*.md".to_string()],
        }
    }
}

impl PlanAdapter for SpecAdapter {
    fn name(&self) -> &str {
        SPEC_SOURCE
    }
    fn globs(&self) -> &[String] {
        &self.globs
    }
    fn parse(&self, text: &str) -> ParsedPlan {
        let sp = crate::plans::superpowers::parse(text);
        if !sp.tasks.is_empty() {
            return sp;
        }
        let cl = crate::plans::checklist::parse(text);
        if !cl.tasks.is_empty() {
            return cl;
        }
        ParsedPlan {
            title: if sp.title.is_empty() {
                cl.title
            } else {
                sp.title
            },
            ..Default::default()
        }
    }
}

/// Convenience: the default adapter set.
pub fn default_adapters() -> Vec<Box<dyn PlanAdapter>> {
    vec![
        Box::new(SuperpowersAdapter::default()),
        Box::new(ChecklistAdapter::default()),
        Box::new(SpecAdapter::default()),
    ]
}

/// Glob helper for the daemon: every default plan glob.
pub fn plan_globs() -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for a in default_adapters() {
        for g in a.globs() {
            out.insert(g.clone());
        }
    }
    out.into_iter().collect()
}

// Module-level helper exposed via `src/plans/mod.rs` re-export (daemon calls `crate::plans::plan_globs()`).
// (kept here for compatibility; mod.rs re-exports below conceptually)
#[allow(dead_code)]
fn _mod_plan_globs() {}

/// Sync plans for one worktree: discover, parse, upsert, mark gone, and derive observed.
pub fn sync_plans<A, I>(
    store: &Store,
    worktree_id: i64,
    worktree_path: impl AsRef<Path>,
    adapters: I,
) -> Result<(), StoreError>
where
    I: IntoIterator<Item = A>,
    A: PlanAdapter,
{
    let wt_path = worktree_path.as_ref();
    let wt_path_str = wt_path.to_string_lossy().to_string();

    // Existing plan rows for this worktree (relocated/gone detection).
    let existing: Vec<String> = store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM plans WHERE worktree_id = ?1")?;
        let rows = stmt.query_map([worktree_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })?;

    let mut present: HashSet<String> = HashSet::new();
    let mut processed: Vec<(String, String, crate::plans::ParsedPlan)> = Vec::new();
    // Relative paths already claimed by a non-spec adapter in this scan (claimed-path dedupe).
    let mut claimed: HashSet<String> = HashSet::new();

    let now = git::now_ms();

    for adapter in adapters {
        let adapter = &adapter;
        for glob in adapter.globs() {
            for file in discover_files(wt_path, glob) {
                let rel = match file.strip_prefix(wt_path) {
                    Ok(r) => r.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };
                // Spec-only skips: claimed-path dedupe (non-spec adapters run first)
                // and the /artifacts/ segment that no plan glob claims.
                if adapter.name() == SPEC_SOURCE {
                    if claimed.contains(&rel) {
                        continue;
                    }
                    if rel.contains("/artifacts/") {
                        continue;
                    }
                }
                let Ok(text) = fs::read_to_string(&file) else {
                    continue;
                };
                let parsed = adapter.parse(&text);
                let id = derive::plan_id(adapter.name(), &wt_path_str, &rel);
                let title = if parsed.title.trim().is_empty() {
                    file.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("plan")
                        .to_string()
                } else {
                    parsed.title.clone()
                };
                let declared_state = parsed.declared_state();
                let state = if declared_state == "done" {
                    "done"
                } else if adapter.name() == SPEC_SOURCE && parsed.tasks.is_empty() {
                    "unknown"
                } else {
                    "active"
                };
                let content_hash = sha256_hex(&text);

                let row = PlanRow {
                    id: &id,
                    source: adapter.name(),
                    title: &title,
                    file: Some(&rel),
                    worktree_id: Some(worktree_id),
                    content_hash: Some(&content_hash),
                    state,
                };
                store.upsert_plan(&row, now)?;

                for (ordinal, task) in parsed.tasks.iter().enumerate() {
                    let mut t = task.clone();
                    t.set_plan_id(&id);
                    let row = t.to_task_row();
                    store.upsert_task(&row)?;
                    let _ = ordinal;
                }

                if adapter.name() != SPEC_SOURCE {
                    claimed.insert(rel.clone());
                }
                present.insert(id.clone());
                processed.push((id, rel, parsed));
            }
        }
    }

    // Mark vanished plans gone.
    store.write(|conn| {
        for id in &existing {
            if !present.contains(id) {
                conn.execute(
                    "UPDATE plans SET state = 'gone', last_seen_ms = ?2 WHERE id = ?1",
                    rusqlite::params![id, now],
                )?;
            }
        }
        Ok(())
    })?;

    // Derive observed state for each plan.
    let wt_info: Option<(Option<String>, Option<String>, Option<String>)> = store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT merge_base, branch, base_ref FROM worktrees WHERE id = ?1")?;
            let mut rows = stmt.query_map([worktree_id], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            if let Some(r) = rows.next() {
                Ok(Some(r?))
            } else {
                Ok(None)
            }
        })
        .ok()
        .flatten();

    let (merge_base, branch, base_ref) = wt_info.unwrap_or((None, None, None));
    let changed = git::changed_file_set(wt_path, branch.as_deref(), base_ref.as_deref());
    let mut changed_paths: Vec<String> = changed.iter().map(|c| c.0.clone()).collect();
    changed_paths.sort();
    changed_paths.dedup();

    let branch_commits = merge_base
        .as_deref()
        .map(|mb| git::commits_between(wt_path, mb, "HEAD"))
        .unwrap_or_default();

    for (plan_id, rel, parsed) in &processed {
        if parsed.tasks.is_empty() {
            continue;
        }
        let plan_first = git::first_commit_for_path(wt_path, rel);
        for (ordinal, task) in parsed.tasks.iter().enumerate() {
            let task_id = format!("{}:{}", plan_id, ordinal + 1);
            let input = DeriveInput {
                expected_paths: task.expected_paths.clone(),
                changed_paths: changed_paths.clone(),
                branch_commits: branch_commits.clone(),
                plan_first_commit_sha: plan_first.clone(),
            };
            let outcome: ObservedOutcome = derive::derive_observed(&input);
            let _ = store.clear_evidence(&task_id, "commit");

            // ts per commit sha
            let mut seen_sha: BTreeSet<String> = BTreeSet::new();
            for ev in &outcome.evidence {
                if !seen_sha.insert(ev.reference.clone()) {
                    continue;
                }
                let ts = branch_commits
                    .iter()
                    .find(|c| c.sha == ev.reference)
                    .map(|c| c.ts_ms)
                    .unwrap_or_else(git::now_ms);
                let erow = EvidenceRow {
                    task_id: &task_id,
                    kind: ev.kind,
                    reference: &ev.reference,
                    ts_ms: ts,
                    confidence: ev.confidence,
                };
                let _ = store.append_evidence(&erow);
            }
            let _ = store.set_task_observed(&task_id, outcome.state, None);
        }
    }

    Ok(())
}

fn discover_files(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, pattern: &str, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                if name == ".git" {
                    continue;
                }
                if path.is_dir() {
                    walk(root, &path, pattern, out);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    let rel_s = rel.to_string_lossy().replace('\\', "/");
                    if glob_match(pattern, &rel_s) {
                        out.push(path);
                    }
                }
            }
        }
    }
    walk(root, root, pattern, &mut out);
    out.sort();
    out
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    matches_seq(&p, &t)
}

fn matches_seq(p: &[char], t: &[char]) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    if p[0] == '*' {
        if p.len() >= 2 && p[1] == '*' {
            if matches_seq(&p[2..], t) {
                return true;
            }
            // Allow `**/` to match zero path segments, so that globs such as
            // `docs/**/*.md` also match files directly under `docs/`.
            // `**/` matches zero or more directories (standard globstar); this
            // applies to every glob, not just spec globs.
            if p.len() >= 3 && p[2] == '/' && matches_seq(&p[3..], t) {
                return true;
            }
            if !t.is_empty() {
                return matches_seq(p, &t[1..]);
            }
            return false;
        }
        // single * : cannot cross slash
        if matches_seq(&p[1..], t) {
            return true;
        }
        if !t.is_empty() && t[0] != '/' {
            return matches_seq(p, &t[1..]);
        }
        return false;
    }
    if !t.is_empty() && (p[0] == '?' && t[0] != '/' || p[0] == t[0]) {
        return matches_seq(&p[1..], &t[1..]);
    }
    false
}

#[cfg(test)]
mod glob_match_tests {
    use super::glob_match;

    #[test]
    fn globstar_matches_zero_segments() {
        assert!(glob_match("docs/**/*.md", "docs/notes.md"));
    }

    #[test]
    fn globstar_matches_one_and_many_segments() {
        assert!(glob_match("docs/**/*.md", "docs/a/notes.md"));
        assert!(glob_match("docs/**/*.md", "docs/a/b/notes.md"));
    }

    #[test]
    fn globstar_does_not_overmatch() {
        assert!(!glob_match("docs/**/*.md", "docsx/notes.md"));
        assert!(!glob_match("docs/**/*.md", "docs/notes.txt"));
        assert!(!glob_match("docs/**/*.md", "notdocs/notes.md"));
    }

    #[test]
    fn single_star_does_not_cross_slash() {
        assert!(glob_match(
            "docs/superpowers/plans/*.md",
            "docs/superpowers/plans/x.md"
        ));
        assert!(!glob_match(
            "docs/superpowers/plans/*.md",
            "docs/superpowers/plans/sub/x.md"
        ));
    }

    #[test]
    fn exact_pattern_matches_only_itself() {
        assert!(glob_match("TODO.md", "TODO.md"));
        assert!(!glob_match("TODO.md", "TODO.txt"));
        assert!(!glob_match("TODO.md", "a/TODO.md"));
    }

    #[test]
    fn checklist_docs_plans_matches_direct_file_deliberate_widening() {
        // Deliberate, documented widening: `**/` matches zero directories, so
        // `docs/plans/**/*.md` now also matches files directly under
        // `docs/plans/` (e.g. `docs/plans/roadmap.md`), which are indexed as
        // source="checklist". This is intentional, not an accident.
        assert!(glob_match("docs/plans/**/*.md", "docs/plans/roadmap.md"));
        assert!(glob_match("docs/plans/**/*.md", "docs/plans/sub/x.md"));
    }
}
