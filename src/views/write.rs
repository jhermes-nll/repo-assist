//! Atomic, changed-only writing of rendered views (Task 1 / §11).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ViewError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(String),
}

pub fn write_views(dir: &Path, rendered: &[(&str, String)]) -> Result<Vec<PathBuf>, ViewError> {
    fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for (name, content) in rendered {
        let target = dir.join(name);
        // Changed-only: skip if bytes already match.
        if let Ok(existing) = fs::read(&target) {
            if existing == content.as_bytes() {
                continue;
            }
        }
        // Atomic: write temp in same dir, then rename.
        let tmp = dir.join(format!("{name}.tmp"));
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &target)?;
        written.push(target);
    }
    Ok(written)
}

pub fn views_dir(env: &crate::paths::Env, repo_key: &str) -> PathBuf {
    env.state_dir(repo_key).join("views")
}
