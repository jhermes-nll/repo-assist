mod common;

use repo_assist::plans::derive::{derive_observed, CommitTouch, DeriveInput};
use repo_assist::sources::git::CommitTouch as GitCommitTouch;

fn touch(sha: &str, files: &[&str]) -> CommitTouch {
    CommitTouch {
        sha: sha.to_string(),
        ts_ms: 1000,
        files: files.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn truth_table_committed_all_paths() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into(), "b.rs".into()],
        changed_paths: vec![],
        branch_commits: vec![touch("c1", &["a.rs"]), touch("c2", &["b.rs"])],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "committed");
    assert!(!out.evidence.is_empty());
}

#[test]
fn truth_table_partial() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into(), "b.rs".into()],
        changed_paths: vec![],
        branch_commits: vec![touch("c1", &["a.rs"])],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "in_progress");
}

#[test]
fn truth_table_none_changed_but_in_commits() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into()],
        changed_paths: vec![],
        branch_commits: vec![touch("c1", &["a.rs"])],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "committed");
    assert_eq!(out.evidence[0].reference, "c1");
    assert_eq!(out.evidence[0].kind, "commit");
    assert_eq!(out.evidence[0].confidence, "inferred");
}

#[test]
fn truth_table_only_changed() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into()],
        changed_paths: vec!["a.rs".into()],
        branch_commits: vec![],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "in_progress");
    assert!(out.evidence.is_empty());
}

#[test]
fn truth_table_none() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into()],
        changed_paths: vec![],
        branch_commits: vec![],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "not_started");
}

#[test]
fn truth_table_empty_expected_paths() {
    let input = DeriveInput {
        expected_paths: vec![],
        changed_paths: vec!["a.rs".into()],
        branch_commits: vec![touch("c1", &["a.rs"])],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "unknown");
}

// silence unused import
#[allow(dead_code)]
fn _use_git_touch(_x: GitCommitTouch) {}

#[test]
fn plan_first_commit_excludes_itself_and_older() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into()],
        changed_paths: vec![],
        branch_commits: vec![
            touch("newer", &["a.rs"]),
            touch("plan", &[]),
            touch("older", &["a.rs"]),
        ],
        plan_first_commit_sha: Some("plan".into()),
    };
    let out = derive_observed(&input);
    assert_eq!(out.state, "committed");
    assert_eq!(out.evidence[0].reference, "newer");
}

#[test]
fn evidence_confidence_always_inferred() {
    let input = DeriveInput {
        expected_paths: vec!["a.rs".into()],
        changed_paths: vec![],
        branch_commits: vec![touch("c1", &["a.rs"])],
        plan_first_commit_sha: None,
    };
    let out = derive_observed(&input);
    for e in &out.evidence {
        assert_eq!(e.confidence, "inferred");
        assert_eq!(e.kind, "commit");
    }
}
