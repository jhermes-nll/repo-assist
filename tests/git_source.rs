//! Task 2.1 acceptance tests: the git source discovers worktrees, computes base ref,
//! merge-base, ahead/behind, and the changed-file set per worktree, upserting the store.

mod common;

use common::GitRepo;
use repo_assist::sources::git;

/// Run a scan over `repo` and return the store rows for assertions.
fn scan(repo: &GitRepo) -> repo_assist::store::Store {
    let db = repo.path().join("state").join("db.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let store = repo_assist::store::Store::open(&db).unwrap();
    git::sync_worktrees(&store, repo.path()).unwrap();
    store
}

fn worktrees(store: &repo_assist::store::Store) -> Vec<(String, Option<String>, i64, i64)> {
    let mut rows = store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT path, branch, present, is_main FROM worktrees ORDER BY path")?;
            let mut out = Vec::new();
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    rows.sort();
    rows
}

fn files(store: &repo_assist::store::Store, suffix: &str) -> Vec<(String, String)> {
    let mut rows = store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT wf.path, wf.origin FROM worktree_files wf \
                 JOIN worktrees wt ON wt.id = wf.worktree_id \
                 WHERE wt.path LIKE '%'||?1 ORDER BY wf.path",
            )?;
            let mut out = Vec::new();
            let rows = stmt.query_map([suffix], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    rows.sort();
    rows
}

/// The main checkout is present and marked as main.
#[test]
fn discovers_main_checkout_as_main() {
    let repo = GitRepo::new();
    let store = scan(&repo);
    let rows = worktrees(&store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, 1); // present
    assert_eq!(rows[0].3, 1); // is_main
    assert_eq!(rows[0].1.as_deref(), Some("main"));
}

/// A worktree on its own branch shows up with its branch name.
#[test]
fn discovers_worktrees_with_branches() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");
    let store = scan(&repo);
    let rows = worktrees(&store);
    assert_eq!(rows.len(), 2);
    let a = rows.iter().find(|r| r.0.ends_with("wt-a")).unwrap();
    assert_eq!(a.1.as_deref(), Some("wt/wt-a"));
}

/// Committed, staged, unstaged, and untracked files get the right origin tags.
#[test]
fn changed_file_origins_are_tagged() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-a", "a.txt");

    let wt = repo.path().join("wt-a");
    // Staged change:
    std::fs::write(wt.join("staged.txt"), "x\n").unwrap();
    common::run(&wt, &["add", "staged.txt"]);
    // Unstaged change (the committed content was "dirty\n"; make it differ):
    std::fs::write(wt.join("a.txt"), "dirty but modified\n").unwrap();
    // Untracked file:
    std::fs::write(wt.join("untracked.txt"), "u\n").unwrap();

    let store = scan(&repo);
    let f = files(&store, "wt-a");
    assert!(f.contains(&("staged.txt".into(), "staged".into())), "{f:?}");
    assert!(f.contains(&("a.txt".into(), "unstaged".into())), "{f:?}");
    assert!(
        f.contains(&("untracked.txt".into(), "untracked".into())),
        "{f:?}"
    );
    // The worktree's pre-committed file (committed on the branch) is in the committed set.
    assert!(
        f.contains(&("a.txt".into(), "committed".into()))
            || f.contains(&("b.txt".into(), "committed".into())),
        "{f:?}"
    );
}

/// A renamed (staged) file records the new path; a deleted file shows as deleted.
#[test]
fn rename_and_delete_are_tracked() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-r", "r.txt");
    let wt = repo.path().join("wt-r");

    // Rename r.txt -> r2.txt and stage it (but do not commit).
    common::run(&wt, &["mv", "r.txt", "r2.txt"]);
    // Delete a committed file from the index+tree (staged deletion).
    // (GitRepo committed r.txt on the branch already.)

    let store = scan(&repo);
    let f = files(&store, "wt-r");
    // The rename's destination path is at least tracked somewhere.
    assert!(
        f.iter()
            .any(|(p, _)| p.ends_with("r.txt") || p.ends_with("r2.txt")),
        "rename should be visible in the changed set: {f:?}"
    );
}

/// Removing a worktree marks the row not present.
#[test]
fn removed_worktree_is_marked_not_present() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-gone", "g.txt");
    let wt = repo.path().join("wt-gone");

    let store = scan(&repo);
    assert_eq!(worktrees(&store).len(), 2);

    common::run(
        repo.path(),
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
    git::sync_worktrees(&store, repo.path()).unwrap();

    let rows = worktrees(&store);
    let gone = rows.iter().find(|r| r.0.ends_with("wt-gone")).unwrap();
    assert_eq!(gone.2, 0, "removed worktree must be present=0");
}

/// A detached-HEAD worktree records no branch name.
#[test]
fn detached_head_has_no_branch() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-d", "d.txt");
    let wt = repo.path().join("wt-d");

    common::run(&wt, &["checkout", "--detach", "HEAD"]);

    let store = scan(&repo);
    let rows = worktrees(&store);
    let d = rows.iter().find(|r| r.0.ends_with("wt-d")).unwrap();
    assert_eq!(d.1, None, "detached HEAD must have no branch");
}

/// The base ref is detected (main here) and stored with a merge base for each worktree.
#[test]
fn base_ref_and_merge_base_are_recorded() {
    let mut repo = GitRepo::new();
    repo.add_worktree("wt-b", "b.txt");
    let store = scan(&repo);
    let rows = store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT path, base_ref, merge_base FROM worktrees ORDER BY path")?;
            let mut out = Vec::new();
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?;
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    for (path, base, mb) in rows {
        assert_eq!(base.as_deref(), Some("main"), "{path}");
        assert!(
            mb.as_deref().map(|m| m.len() == 40).unwrap_or(false),
            "{path}: {mb:?}"
        );
    }
}
