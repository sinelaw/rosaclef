//! The file system the studio works on.
//!
//! The native server uses the disk ([`DiskFs`]). The browser build has no
//! disk: it keeps the same tree in memory ([`MemFs`]) and persists it to
//! IndexedDB from JavaScript. Everything above this crate (project folders,
//! the library, the file manager, rendering) is written once against [`Fs`].
//!
//! Paths are absolute. [`MemFs`] uses Unix-style paths (`/library/Song/project.json`).

mod mem;

pub use mem::{Change, MemFs};

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What [`Fs::metadata`] reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meta {
    pub is_dir: bool,
    pub len: u64,
    /// Last modification, in milliseconds since the Unix epoch.
    pub modified: f64,
}

/// A file system: the handful of operations the studio needs.
pub trait Fs: Send + Sync {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Replace a file atomically, creating its parent folders.
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    fn metadata(&self, path: &Path) -> io::Result<Meta>;

    /// The entries of a folder (full paths), sorted.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;

    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Move a file or a folder.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Copy a file.
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;

    fn remove_file(&self, path: &Path) -> io::Result<()>;

    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;

    /// The canonical form of a path (the path itself when that fails).
    fn canonicalize(&self, path: &Path) -> PathBuf;

    /// The current time, in milliseconds since the Unix epoch.
    fn now_ms(&self) -> f64;

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        String::from_utf8(self.read(path)?).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        })
    }

    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.metadata(path).map(|m| !m.is_dir).unwrap_or(false)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.metadata(path).map(|m| m.is_dir).unwrap_or(false)
    }

    /// Write `text` unless the file already holds exactly that.
    fn write_if_changed(&self, path: &Path, text: &str) -> io::Result<()> {
        if self
            .read(path)
            .map(|t| t == text.as_bytes())
            .unwrap_or(false)
        {
            return Ok(());
        }
        self.write(path, text.as_bytes())
    }
}

/// A shared handle on a file system.
pub type SharedFs = Arc<dyn Fs>;

/// The machine's own file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiskFs;

/// A shared handle on the disk.
pub fn disk() -> SharedFs {
    Arc::new(DiskFs)
}

fn millis(t: io::Result<std::time::SystemTime>) -> f64 {
    t.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

impl Fs for DiskFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path).map_err(|e| with_path(e, "reading", path))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(
            ".{}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
        ));
        std::fs::write(&tmp, bytes).map_err(|e| with_path(e, "writing", &tmp))?;
        std::fs::rename(&tmp, path).map_err(|e| with_path(e, "renaming into", path))
    }

    fn metadata(&self, path: &Path) -> io::Result<Meta> {
        let m = std::fs::metadata(path)?;
        Ok(Meta {
            is_dir: m.is_dir(),
            len: m.len(),
            modified: millis(m.modified()),
        })
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(path)?
            .flatten()
            .map(|e| e.path())
            .collect();
        out.sort();
        Ok(out)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path).map_err(|e| with_path(e, "creating", path))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| with_path(e, "copying", from))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    fn now_ms(&self) -> f64 {
        millis(Ok(std::time::SystemTime::now()))
    }
}

fn with_path(e: io::Error, what: &str, path: &Path) -> io::Error {
    io::Error::new(e.kind(), format!("{what} {}: {e}", path.display()))
}

/// True when an error only means "this file's content is not in memory yet"
/// (see [`MemFs`]): the caller may load it and try again.
pub fn is_not_loaded(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock
}
