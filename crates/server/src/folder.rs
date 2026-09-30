//! The on-disk project folder (see `rosaclef_studio::folder`).

pub use rosaclef_studio::folder::*;

use anyhow::Result;
use rosaclef_fs::Fs;
use std::path::Path;

/// Replace a file on disk atomically, creating its parent folders.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    Ok(rosaclef_fs::DiskFs.write(path, bytes)?)
}
