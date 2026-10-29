//! Plan observed derivation (Section 9) — pure functions, table-tested.
//!
//! `derive()` recomputes a task's `observed` value from git state alone, exactly as the
//! findings catalog in `docs/2026-10-19-repo-assist-implementation-plan.md` describes it:
//!
//! - **committed**: every expected path appears in a commit on the plan's branch made after
//!   the plan file's first commit; evidence rows are those commits.
//! - **in_progress**: not committed, and at least one expected path is in the worktree's
//!   staged/unstaged/untracked set or in some commit on the branch.
//! - **not_started**: has expected_paths, and none of them touched.
//! - **unknown**: tasks without expected_paths.
//!
//! Evidence from inference carries `confidence=inferred`; only explicit evidence (a future
//! explicit task id) may justify `error` severity — so `derive()` only ever emits commit
//! shas, never evidence that could justify `error`.

use std::collections::{BTreeSet, HashMap};

pub use crate::sources::git::CommitTouch;

/// Input to the pure observed derivation (Section 9).
#[derive(Debug, Clone)]
pub struct DeriveInput {
    pub expected_paths: Vec<String>,
    pub changed_paths: Vec<String>,
    pub branch_commits: Vec<CommitTouch>,
    pub plan_first_commit_sha: Option<String>,
}

/// One piece of derived evidence (`confidence` is always `inferred`, kind `commit`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSeed {
    pub kind: &'static str,
    pub reference: String,
    pub confidence: &'static str,
}

/// Output of observed derivation.
#[derive(Debug, Clone)]
pub struct ObservedOutcome {
    pub state: &'static str,
    pub evidence: Vec<EvidenceSeed>,
}

/// Pure derivation matching the Section 9 truth table (new API).
pub fn derive_observed(input: &DeriveInput) -> ObservedOutcome {
    if input.expected_paths.is_empty() {
        return ObservedOutcome {
            state: "unknown",
            evidence: Vec::new(),
        };
    }

    // Reachable commits: those made after `plan_first_commit_sha`, when known.
    let ts_descending = if input.branch_commits.len() >= 2 {
        input.branch_commits[0].ts_ms >= input.branch_commits[input.branch_commits.len() - 1].ts_ms
    } else {
        true
    };
    let plan_sha = input.plan_first_commit_sha.as_deref();
    let plan_pos = plan_sha.and_then(|ps| input.branch_commits.iter().position(|c| c.sha == ps));

    let reachable: Vec<&CommitTouch> = input
        .branch_commits
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            if let Some(ps) = plan_sha {
                if c.sha == ps {
                    return false;
                }
                if let Some(pos) = plan_pos {
                    return if ts_descending { *i < pos } else { *i > pos };
                }
            }
            true
        })
        .map(|(_, c)| c)
        .collect();

    // First reachable commit (in input order) containing a given path.
    let first_commit_for = |path: &str| -> Option<&CommitTouch> {
        reachable
            .iter()
            .copied()
            .find(|c| c.files.iter().any(|f| f == path))
    };

    let committed = input
        .expected_paths
        .iter()
        .all(|p| first_commit_for(p).is_some());

    if committed {
        let mut shas: BTreeSet<String> = BTreeSet::new();
        for p in &input.expected_paths {
            if let Some(c) = first_commit_for(p) {
                shas.insert(c.sha.clone());
            }
        }
        let evidence: Vec<EvidenceSeed> = shas
            .into_iter()
            .map(|sha| EvidenceSeed {
                kind: "commit",
                reference: sha,
                confidence: "inferred",
            })
            .collect();
        return ObservedOutcome {
            state: "committed",
            evidence,
        };
    }

    let changed: std::collections::BTreeSet<&str> =
        input.changed_paths.iter().map(String::as_str).collect();
    let touched_some = input.expected_paths.iter().any(|p| {
        changed.contains(p.as_str())
            || input
                .branch_commits
                .iter()
                .any(|c| c.files.iter().any(|f| f == p))
    });
    if touched_some {
        let mut shas: BTreeSet<String> = BTreeSet::new();
        for p in &input.expected_paths {
            // only commits touching this path and reachable
            if let Some(c) = first_commit_for(p) {
                shas.insert(c.sha.clone());
            }
        }
        let evidence: Vec<EvidenceSeed> = shas
            .into_iter()
            .map(|sha| EvidenceSeed {
                kind: "commit",
                reference: sha,
                confidence: "inferred",
            })
            .collect();
        return ObservedOutcome {
            state: "in_progress",
            evidence,
        };
    }

    ObservedOutcome {
        state: "not_started",
        evidence: Vec::new(),
    }
}

use sha2::{Digest, Sha256};

/// The four observed states (Section 9), kept in one place so the store, the checks, and the
/// derivation all agree on the exact strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// The plan has `expected_paths`, but none of them are touched (staged/unstaged/untracked)
    /// and none of them appear in a branch commit.
    NotStarted,
    /// Not committed, but at least one expected path is staged/unstaged/untracked, or was
    /// touched by some commit on the branch.
    InProgress,
    /// Every expected path appears in a branch commit made after the plan file's first
    /// commit; the returned evidence shas are those commits.
    Committed,
    /// The task declares no `expected_paths`, so it cannot be observed against git state.
    Unknown,
}

impl Observed {
    /// The stable string stored in `tasks.observed` (Section 5).
    pub fn as_str(&self) -> &'static str {
        match self {
            Observed::NotStarted => "not_started",
            Observed::InProgress => "in_progress",
            Observed::Committed => "committed",
            Observed::Unknown => "unknown",
        }
    }
}

/// Compute one `plans.id` (Section 5): `sha256(source|worktree_path|relative_path)[..16]`.
///
/// `relative_path` is the plan file's path relative to its worktree root. The hash is
/// truncated to 16 hex chars: long enough to avoid accidental collisions among plans that
/// live in the same worktree, short enough to stay readable in CLI output.
pub fn plan_id(source: &str, worktree_path: &str, relative_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update(b"\x01");
    hasher.update(worktree_path.as_bytes());
    hasher.update(b"\x01");
    hasher.update(relative_path.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

/// Derive `observed` for one task (Section 9), returning `(observed, evidence_shas)`.
///
/// - `expected_paths`: the task's declared paths (empty => `Unknown`).
/// - `changed`: the worktree changed-file set, `(path, origin)` as
///   [`crate::sources::git::changed_file_set`] produces (`committed` | `staged` | `unstaged` |
///   `untracked`).
/// - `history`: commit-sha => the set of paths each commit introduced, for commits on the
///   plan's branch made after the plan file's first commit (as
///   [`crate::sources::git::plan_commit_history`] returns).
///
/// `evidence_shas` are the commit shas recorded as evidence (empty for `not_started`/`unknown`).
/// Every evidence row carries `confidence=inferred`.
pub fn derive(
    expected_paths: &[String],
    changed: &[(String, String)],
    history: &HashMap<String, Vec<String>>,
) -> (Observed, Vec<String>) {
    // Tasks without expected_paths cannot be observed against git state (Section 9).
    if expected_paths.is_empty() {
        return (Observed::Unknown, Vec::new());
    }

    // Newest (first-listed) commit that introduced each expected path.
    let mut newest_commit_per_path: HashMap<&str, String> = HashMap::new();
    for (commit, paths) in history {
        for path in paths {
            let seen = newest_commit_per_path.entry(path.as_str()).or_default();
            if seen.is_empty() {
                *seen = commit.clone();
            }
        }
    }

    // committed: every expected path appears in a branch commit after the plan's first commit.
    if expected_paths
        .iter()
        .all(|p| !newest_commit_per_path.get(p.as_str()).is_none())
    {
        let mut shas: BTreeSet<String> = BTreeSet::new();
        for p in expected_paths {
            if let Some(s) = newest_commit_per_path.get(p.as_str()) {
                shas.insert(s.clone());
            }
        }
        // Inference carries `confidence=inferred`; it never justifies `error`.
        return (Observed::Committed, shas.into_iter().collect());
    }

    // in_progress: at least one expected path is staged/unstaged/untracked, or in some branch
    // commit (but not all, so it is not yet committed).
    let changed_paths: BTreeSet<&str> = changed
        .iter()
        .filter_map(|(p, _)| Some(p.as_str()))
        .collect();
    let touched_some = expected_paths.iter().any(|p| {
        changed_paths.contains(p.as_str()) || newest_commit_per_path.contains_key(p.as_str())
    });
    if touched_some {
        let mut shas: BTreeSet<String> = BTreeSet::new();
        for p in expected_paths {
            if let Some(s) = newest_commit_per_path.get(p.as_str()) {
                shas.insert(s.clone());
            }
        }
        return (Observed::InProgress, shas.into_iter().collect());
    }

    // not_started: has expected_paths, and none of them are touched.
    (Observed::NotStarted, Vec::new())
}

/// A single truth-table row for [`derive`], used by the table test in `tests/plan_derive.rs`.
pub struct DeriveCase {
    pub name: &'static str,
    pub expected: Vec<String>,
    pub changed: Vec<(String, String)>,
    pub history: Vec<(String, Vec<String>)>,
    pub want: Observed,
}

/// Evaluate every [`DeriveCase`], asserting `(observed, evidence_shas)` matches `case.want`.
///
/// Exposed so the table test can drive `derive()` directly (it is otherwise a module-private
/// helper of the daemon's plan scanner).
pub fn run_truth_table(cases: &[DeriveCase]) -> Vec<(Observed, Vec<String>)> {
    cases
        .iter()
        .map(|c| {
            let history: HashMap<String, Vec<String>> = c.history.iter().cloned().collect();
            let (obs, shas) = derive(&c.expected, &c.changed, &history);
            (obs, shas)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    /// `derive` exactly matches Section 9's truth table for every case.
    #[test]
    fn observed_follows_section_9_truth_table() {
        // committed: every expected path appears in a branch commit.
        let committed = DeriveCase {
            name: "all paths committed",
            expected: paths(&["a.rs", "b.rs"]),
            changed: vec![("c.rs".into(), "untracked".into())],
            history: vec![
                ("c2".into(), paths(&["a.rs"])),
                ("c1".into(), paths(&["b.rs"])),
            ],
            want: Observed::Committed,
        };

        // not_started: has expected_paths, none touched.
        let not_started = DeriveCase {
            name: "expected present but none touched",
            expected: paths(&["a.rs", "b.rs"]),
            changed: vec![("c.rs".into(), "untracked".into())],
            history: vec![("c1".into(), paths(&["c.rs"]))],
            want: Observed::NotStarted,
        };

        // in_progress: some committed, one only in the changed set.
        let in_progress = DeriveCase {
            name: "one committed, one merely touched",
            expected: paths(&["a.rs", "x.rs"]),
            changed: vec![("x.rs".into(), "staged".into())],
            history: vec![("c2".into(), paths(&["a.rs"]))],
            want: Observed::InProgress,
        };

        // in_progress: not all expected paths are in branch commits, but at least one is.
        // a.rs appears in a branch commit; b.rs is neither committed nor touched.
        let in_progress_from_branch = DeriveCase {
            name: "none committed, but one in a branch commit",
            expected: paths(&["a.rs", "b.rs"]),
            changed: vec![("x.rs".into(), "untracked".into())],
            history: vec![("c2".into(), paths(&["a.rs"]))],
            want: Observed::InProgress,
        };

        // unknown: no expected_paths.
        let unknown = DeriveCase {
            name: "no expected_paths",
            expected: vec![],
            changed: vec![("a.rs".into(), "staged".into())],
            history: vec![("c1".into(), paths(&["a.rs"]))],
            want: Observed::Unknown,
        };

        let cases = vec![
            committed,
            not_started,
            in_progress,
            in_progress_from_branch,
            unknown,
        ];
        let results = run_truth_table(&cases);

        for (c, (obs, shas)) in cases.iter().zip(results.iter()) {
            assert_eq!(*obs, c.want, "case {} should be {:?}", c.name, obs);
            // committed must record at least the newest commit per path.
            if c.want == Observed::Committed {
                assert!(!shas.is_empty(), "{}: committed records evidence", c.name);
            }
        }
    }

    /// `plan_id` is a stable 16-hex-char id over `(source, worktree, relative_path)`.
    #[test]
    fn plan_id_is_stable_and_truncated() {
        let a = plan_id("superpowers", "/wt", "docs/superpowers/plans/plan.md");
        let b = plan_id("superpowers", "/wt", "docs/superpowers/plans/plan.md");
        let c = plan_id("checklist", "/wt", "docs/superpowers/plans/plan.md");
        assert_eq!(a.len(), 16);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
