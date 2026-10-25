//! SQLite `user_version` based schema migrations.
//!
//! `user_version` is an integer stored in the SQLite header. We refuse to open a database
//! whose version is newer than the crate knows (Section 5), apply no migrations below the
//! current version, and treat any unknown non-zero value as fatal.

/// Highest schema version this crate can open.
pub const CURRENT_VERSION: u32 = 3;

/// Read the `user_version` from a file. Returns 0 for a missing or empty database.
pub fn current_user_version(db_path: &std::path::Path) -> Result<u32, crate::store::StoreError> {
    if !db_path.exists() {
        return Ok(0);
    }
    // Open read-only, no write side effects. rusqlite's `Connection::open` on an existing
    // file is fine; we only `PRAGMA user_version` read here.
    let conn = rusqlite::Connection::open(db_path).map_err(crate::store::StoreError::Database)?;
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(crate::store::StoreError::Database)?;
    Ok(version)
}

/// The outcome of attempting to initialize a database at `db_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitResult {
    /// Database did not exist; created and migrated to `CURRENT_VERSION`.
    Created,
    /// Database existed at an already-usable version; opened as-is.
    UpToDate,
}

/// Migrate an existing (possibly empty) database file to `CURRENT_VERSION`.
///
/// Idempotent: re-running after a prior successful migration leaves the file unchanged.
/// Refuses files whose `user_version` is greater than [`CURRENT_VERSION`] and ignores
/// nothing below it (missing tables are created by the schema).
pub fn migrate(db_path: &std::path::Path) -> Result<InitResult, crate::store::StoreError> {
    if !db_path.exists() {
        return Err(crate::store::StoreError::MissingDatabase(
            db_path.display().to_string(),
        ));
    }
    let version = current_user_version(db_path)?;
    if version > CURRENT_VERSION {
        return Err(crate::store::StoreError::NewerVersion(
            version,
            CURRENT_VERSION,
        ));
    }
    if version == CURRENT_VERSION {
        return Ok(InitResult::UpToDate);
    }
    apply_schema(db_path)?;
    Ok(InitResult::Created)
}

/// Apply the bundled schema with `user_version = CURRENT_VERSION` set atomically at close.
fn apply_schema(db_path: &std::path::Path) -> Result<(), crate::store::StoreError> {
    let schema = include_str!("schema.sql");
    let conn = rusqlite::Connection::open(db_path).map_err(crate::store::StoreError::Database)?;
    conn.execute_batch(schema)
        .map_err(crate::store::StoreError::Database)?;
    conn.execute_batch(&format!("PRAGMA user_version = {CURRENT_VERSION};"))
        .map_err(crate::store::StoreError::Database)?;
    Ok(())
}
