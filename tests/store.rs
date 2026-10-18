//! Task 1.1 acceptance tests: open a fresh DB, migrate it, and reopen it; then refuse a DB
//! whose `user_version` is newer than this crate knows.

use std::path::Path;
use std::process::Command;

use repo_assist::store::{Store, StoreError, SCHEMA_VERSION};

/// Run a git-less `sqlite3` command on `db_path` to set the schema version, if sqlite3 exists.
fn set_user_version(db_path: &Path, version: u32) {
    if !Command::new("sqlite3").arg("--version").output().is_ok() {
        eprintln!("sqlite3 not available; skipping version-mismatch test");
        return;
    }
    let out = Command::new("sqlite3")
        .arg(db_path)
        .arg(format!("PRAGMA user_version = {version};"))
        .output()
        .expect("sqlite3 executed");
    assert!(out.status.success(), "sqlite3 failed to set user_version");
}

#[test]
fn fresh_db_is_created_migrated_and_reopenable() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("db.sqlite");

    // `open` on a fresh (nonexistent) path must create the file and migrate it to
    // `SCHEMA_VERSION` in one step, and report it did not pre-exist.
    let store = Store::open(&db_path).expect("open a fresh DB");
    assert!(!store.existed());
    assert_eq!(store.version(), SCHEMA_VERSION);

    // The worktrees table (the simplest) must exist after migration.
    let mut existed = false;
    store
        .read(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name='worktrees'",
                [],
                |row| row.get(0),
            )?;
            existed = count > 0;
            Ok(())
        })
        .expect("query the sqlite_master catalog");
    assert!(existed, "worktrees table should exist after migration");

    // The migrated file reports user_version == SCHEMA_VERSION (read back via a handle).
    let mut version_from_file: u32 = 0;
    store
        .read(|conn| {
            version_from_file =
                conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?;
            Ok(())
        })
        .expect("read user_version back through a handle");
    assert_eq!(version_from_file, SCHEMA_VERSION);
}

#[test]
fn missing_database_is_an_error_by_default() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("nonexistent");
    assert!(!db_path.exists());

    let opts = repo_assist::store::StoreOptions {
        allow_missing: true,
    };
    match Store::open_with(&db_path, opts) {
        Err(StoreError::MissingDatabase(path)) => {
            assert_eq!(Path::new(&path), db_path.as_path());
        }
        other => panic!("expected MissingDatabase, got {other:?}"),
    }

    // allow_missing: a missing file reports MissingDatabase (nothing created/migrated).
    let opts = repo_assist::store::StoreOptions {
        allow_missing: true,
    };
    match Store::open_with(&db_path, opts) {
        Err(StoreError::MissingDatabase(path)) => {
            assert_eq!(std::path::Path::new(&path), db_path.as_path());
        }
        other => panic!("expected MissingDatabase, got {other:?}"),
    }
}

#[test]
fn newer_schema_version_is_refused() {
    if !Command::new("sqlite3").arg("--version").output().is_ok() {
        eprintln!("sqlite3 not available; skipping newer-version test");
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("db.sqlite");

    // Create an empty database and pin its user_version above SCHEMA_VERSION.
    Command::new("sqlite3")
        .arg(&db_path)
        .arg("VACUUM")
        .output()
        .expect("sqlite3 create");
    set_user_version(&db_path, SCHEMA_VERSION + 1);

    match Store::open(&db_path) {
        Err(StoreError::NewerVersion(got, max)) => {
            assert_eq!(got, SCHEMA_VERSION + 1);
            assert_eq!(max, SCHEMA_VERSION);
        }
        other => panic!("expected NewerVersion, got {other:?}"),
    }
}
