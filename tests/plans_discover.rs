mod common;

use std::path::Path;

use repo_assist::plans::discover::{default_adapters, sync_plans};
use repo_assist::sources::git::sync_worktrees;
use repo_assist::store::Store;

fn git(wt: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(wt)
        .args(args)
        .status()
        .expect("git executed");
    assert!(status.success(), "git {args:?} failed in {wt:?}");
}

fn open(db: &Path) -> Store {
    Store::open(db).expect("open store")
}

#[test]
fn superpowers_plan_flip_and_gone() {
    let mut repo = common::GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let wt = repo.paths[0].clone();

    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = open(&db);
    sync_worktrees(&store, repo.path()).expect("sync_worktrees");
    let tail = "wt-a";
    let tail_pat = format!("%/{}", tail);
    let (wt_id, path, _branch, _base_ref): (i64, String, Option<String>, Option<String>) = store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT id, path, branch, base_ref FROM worktrees WHERE path LIKE ?")?;
            let mut found = None;
            for r in stmt.query_map([tail_pat.clone()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })? {
                found = Some(r?);
            }
            Ok(found.unwrap())
        })
        .expect("worktree");

    // Commit the plan file first.
    std::fs::create_dir_all(wt.join("docs/superpowers/plans")).unwrap();
    std::fs::write(
        wt.join("docs/superpowers/plans/test-plan.md"),
        "# Test Plan\n\n### Task 1: Add task file\nCreate: `task-file.txt`\n- [ ] do it\n",
    )
    .unwrap();
    git(&wt, &["add", "docs/superpowers/plans/test-plan.md"]);
    git(&wt, &["commit", "-qm", "add plan"]);

    // Sync while task file does not exist yet: nothing committed.
    sync_plans(&store, wt_id, Path::new(&path), default_adapters()).expect("sync1");

    // Now commit the task file separately, making it reachable after the plan.
    std::fs::write(wt.join("task-file.txt"), "done\n").unwrap();
    git(&wt, &["add", "task-file.txt"]);
    git(&wt, &["commit", "-qm", "add task file"]);
    let commit_sha = std::process::Command::new("git")
        .current_dir(&wt)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let commit_sha = String::from_utf8_lossy(&commit_sha.stdout)
        .trim()
        .to_string();

    // Re-sync worktrees (HEAD moved) then plans.
    sync_worktrees(&store, repo.path()).expect("sync_worktrees2");
    sync_plans(&store, wt_id, Path::new(&path), default_adapters()).expect("sync2");

    // Plan row exists.
    let plan_state: String = store
        .read(|conn| {
            conn.query_row(
                "SELECT state FROM plans WHERE worktree_id = ?1",
                [wt_id],
                |r| r.get(0),
            )
        })
        .expect("plan row");
    assert_ne!(plan_state, "gone");

    // Task observed committed.
    let observed: String = store
        .read(|conn| {
            conn.query_row(
                "SELECT observed FROM tasks WHERE plan_id IN (SELECT id FROM plans WHERE worktree_id = ?1)",
                [wt_id],
                |r| r.get(0),
            )
        })
        .expect("task row");
    assert_eq!(observed, "committed", "observed should flip to committed");

    // Evidence row with commit sha.

    let evidence_refs: Vec<String> = store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.ref FROM evidence e JOIN tasks t ON t.id=e.task_id WHERE t.plan_id IN (SELECT id FROM plans WHERE worktree_id = ?1)",
            )?;
            let mut out = Vec::new();
            for r in stmt.query_map([wt_id], |row| row.get::<_, String>(0))? {
                out.push(r?);
            }
            Ok(out)
        })
        .expect("evidence");
    assert!(
        evidence_refs.contains(&commit_sha),
        "evidence must contain {commit_sha}: {evidence_refs:?}"
    );

    // Idempotency-ish: evidence count remains after re-sync.
    // Delete plan file and re-sync -> gone.
    std::fs::remove_file(wt.join("docs/superpowers/plans/test-plan.md")).unwrap();
    sync_worktrees(&store, repo.path()).expect("sync_worktrees3");
    sync_plans(&store, wt_id, Path::new(&path), default_adapters()).expect("sync3");

    let gone_state: String = store
        .read(|conn| {
            conn.query_row(
                "SELECT state FROM plans WHERE worktree_id = ?1",
                [wt_id],
                |r| r.get(0),
            )
        })
        .expect("plan row gone");
    assert_eq!(gone_state, "gone");
}
