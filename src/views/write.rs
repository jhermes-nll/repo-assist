//! Atomic, changed-only writing of rendered views (Task 1 / §11).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ViewError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(String),
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn strip_first_line(bytes: &[u8]) -> &[u8] {
    // Skip the leading header line (e.g. `generated_at: ...`) so that a changed-only
    // comparison keys on the data-derived body rather than the volatile timestamp.
    if let Some(idx) = bytes.iter().position(|&b| b == b'\n') {
        &bytes[idx + 1..]
    } else {
        &[]
    }
}

pub fn write_views(dir: &Path, rendered: &[(&str, String)]) -> Result<Vec<PathBuf>, ViewError> {
    fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for (name, content) in rendered {
        let target = dir.join(name);
        // Changed-only: skip if the data-derived body already matches (timestamp ignored).
        if let Ok(existing) = fs::read(&target) {
            if strip_first_line(&existing) == strip_first_line(content.as_bytes()) {
                continue;
            }
        }
        // Atomic: write a uniquely-named temp in the same dir, fsync, then rename.
        let pid = std::process::id();
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!("{name}.{pid}.{n}.tmp"));
        let write_result = (|| -> Result<(), ViewError> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &target)?;
            Ok(())
        })();
        if let Err(e) = write_result {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        written.push(target);
    }
    Ok(written)
}
