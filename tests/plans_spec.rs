//! Spec-source tracking: docs/**/*.md indexed as source="spec"; plan dirs excluded;
//! zero-task specs are "unknown"; plan behaviour unchanged.

mod common;

use std::path::{Path, PathBuf};

use repo_assist::plans::discover::{default_adapters, sync_plans, PlanAdapter, SpecAdapter};
use repo_assist::sources::git::sync_worktrees;
use repo_assist::store::Store;

struct Harness {
    _repo: common::GitRepo,
    store: Store,
    wt: PathBuf,
    path: String,
    wt_id: i64,
}

fn setup() -> Harness {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let wt = repo.paths[0].clone();
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = Store::open(&db).expect("open store");
    sync_worktrees(&store, repo.path()).expect("sync_worktrees");
    let (wt_id, path): (i64, String) = store
        .read(|conn| {
            conn.query_row(
                "SELECT id, path FROM worktrees WHERE path LIKE '%/wt-a'",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
        })
        .expect("worktree row");
    Harness {
        _repo: repo,
        store,
        wt,
        path,
        wt_id,
    }
}

fn write(h: &Harness, rel: &str, text: &str) {
    let p = h.wt.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn sync(h: &Harness) {
    sync_plans(&h.store, h.wt_id, Path::new(&h.path), default_adapters()).expect("sync_plans");
}

/// (source, file, state) for every plan row of the worktree, ordered by file.
fn rows(h: &Harness) -> Vec<(String, String, String)> {
    h.store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT source, COALESCE(file, ''), state FROM plans WHERE worktree_id = ?1 ORDER BY file",
            )?;
            let mut out = Vec::new();
            for r in stmt.query_map([h.wt_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })? {
                out.push(r?);
            }
            Ok(out)
        })
        .expect("rows")
}

/// (source, state) of the single row for `file`, if any.
fn row_for(h: &Harness, file: &str) -> Option<(String, String)> {
    rows(h)
        .into_iter()
        .find(|r| r.1 == file)
        .map(|r| (r.0, r.2))
}

#[test]
fn spec_adapter_is_registered_last_with_default_glob() {
    assert_eq!(SpecAdapter::default().globs, vec!["docs/**/*.md"]);
    assert_eq!(SpecAdapter::default().name(), "spec");
    let names: Vec<String> = default_adapters()
        .iter()
        .map(|a| a.name().to_string())
        .collect();
    assert_eq!(names, vec!["superpowers", "checklist", "spec"]);
    // Bus trigger glob set must include spec files.
    assert!(repo_assist::plans::plan_globs().contains(&"docs/**/*.md".to_string()));
}

#[test]
fn spec_with_headings_but_no_checkboxes_is_unknown() {
    let h = setup();
    write(
        &h,
        "docs/h.md",
        "# H\n\n### Task 1: a\n### Task 2: b\n### Task 3: c\n",
    );
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/h.md"),
        Some(("spec".into(), "unknown".into()))
    );
}

#[test]
fn spec_with_headings_and_mixed_boxes_is_active() {
    let h = setup();
    write(
        &h,
        "docs/h.md",
        "# H\n\n### Task 1: a\n- [x] done\n- [ ] todo\n",
    );
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/h.md"),
        Some(("spec".into(), "active".into()))
    );
}

#[test]
fn spec_with_headings_and_all_boxes_checked_is_done() {
    let h = setup();
    write(&h, "docs/h.md", "# H\n\n### Task 1: a\n- [x] done\n");
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/h.md"),
        Some(("spec".into(), "done".into()))
    );
}

#[test]
fn spec_with_link_bullet_and_no_real_checkboxes_is_unknown() {
    let h = setup();
    write(
        &h,
        "docs/h.md",
        "# H\n\n### Task 1: a\n- [spec](http://x)\n",
    );
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/h.md"),
        Some(("spec".into(), "unknown".into()))
    );
}

#[test]
fn zero_task_spec_is_unknown() {
    let h = setup();
    write(
        &h,
        "docs/status.md",
        "# Status\n\nJust prose, nothing machine-readable.\n",
    );
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/status.md"),
        Some(("spec".to_string(), "unknown".to_string()))
    );
}

#[test]
fn machine_readable_spec_gets_the_plan_rule() {
    let h = setup();
    write(
        &h,
        "docs/open.md",
        "# Open\n\n### Task 1: a\n- [x] a\n- [ ] b\n",
    );
    write(
        &h,
        "docs/finished.md",
        "# Finished\n\n### Task 1: a\n- [x] a\n",
    );
    write(&h, "docs/list.md", "# List\n\n- [ ] one\n- [x] two\n");
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/open.md"),
        Some(("spec".into(), "active".into()))
    );
    assert_eq!(
        row_for(&h, "docs/finished.md"),
        Some(("spec".into(), "done".into()))
    );
    assert_eq!(
        row_for(&h, "docs/list.md"),
        Some(("spec".into(), "active".into()))
    );
}

#[test]
fn plan_files_are_excluded_from_spec_globs() {
    let h = setup();
    write(
        &h,
        "docs/superpowers/plans/with-task.md",
        "# P\n\n### Task 1: a\n- [ ] a\n",
    );
    write(
        &h,
        "docs/superpowers/plans/zero.md",
        "# Zero\n\nprose only\n",
    );
    write(
        &h,
        "docs/superpowers/plans/artifacts/ledger.md",
        "# Ledger\n\nE1 | x\n",
    );
    write(&h, "docs/notes.md", "# Notes\n");
    sync(&h);
    let all = rows(&h);
    assert!(
        !all.iter()
            .any(|r| r.0 == "spec" && r.1.starts_with("docs/superpowers/plans/")),
        "plan dir leaked into specs: {all:?}"
    );
    assert!(
        !all.iter().any(|r| r.1.contains("artifacts/")),
        "artifacts must not be indexed at all: {all:?}"
    );
    assert_eq!(
        all.iter()
            .filter(|r| r.1 == "docs/superpowers/plans/with-task.md")
            .count(),
        1,
        "exactly one row (no double-indexing): {all:?}"
    );
    assert_eq!(
        row_for(&h, "docs/notes.md").map(|r| r.0),
        Some("spec".to_string())
    );
}

#[test]
fn zero_task_plans_keep_active_state() {
    let h = setup();
    write(
        &h,
        "docs/superpowers/plans/zero.md",
        "# Zero\n\nprose only\n",
    );
    write(&h, "TODO.md", "# Todo\n\nnothing yet\n");
    sync(&h);
    assert_eq!(
        row_for(&h, "docs/superpowers/plans/zero.md"),
        Some(("superpowers".to_string(), "active".to_string()))
    );
    assert_eq!(
        row_for(&h, "TODO.md"),
        Some(("checklist".to_string(), "active".to_string()))
    );
}

#[test]
fn repos_own_implementation_plan_is_tracked_as_a_real_state_spec() {
    let h = setup();
    write(
        &h,
        "docs/impl.md",
        include_str!("../docs/2026-10-19-repo-assist-implementation-plan.md"),
    );
    sync(&h);
    let (source, state) = row_for(&h, "docs/impl.md").expect("row");
    eprintln!("implementation-plan doc landed as source={source} state={state}");
    assert_eq!(source, "spec");
    assert!(state == "active" || state == "done", "state was {state}");
    let tasks: i64 = h
        .store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE plan_id IN (SELECT id FROM plans WHERE file = 'docs/impl.md' AND worktree_id = ?1)",
                [h.wt_id],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert!(tasks > 0, "expected parsed tasks, got {tasks}");
}
