//! An in-memory file tree, persisted by its host.
//!
//! Every file's content is a *blob* with a numeric id. The host (the browser
//! build's worker) stores blobs and the tree separately, so a rename or a copy
//! only touches the tree, and large files need not stay in memory: a file can
//! be known by its size and blob id alone, its content loaded on demand.
//!
//! - [`MemFs::restore`] rebuilds the tree from storage, without journaling.
//! - Reading a file whose content is not loaded fails with
//!   [`std::io::ErrorKind::WouldBlock`] and records the blob in
//!   [`MemFs::take_missing`]; the host loads it ([`MemFs::provide`]) and
//!   retries the operation.
//! - Every change is journaled; [`MemFs::take_changes`] hands them to the
//!   host to persist, in order.

use crate::{Fs, Meta};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug)]
struct FileNode {
    blob: u64,
    len: u64,
    modified: f64,
    data: Option<Arc<Vec<u8>>>,
}

#[derive(Clone, Debug)]
enum Node {
    Dir { modified: f64 },
    File(FileNode),
}

/// One change to persist.
#[derive(Clone, Debug)]
pub enum Change {
    /// A file now holds `blob`. `data` is set when the blob is new (its
    /// content must be stored); a rename or a copy reuses a stored blob.
    File { path: String, blob: u64, len: u64, modified: f64, data: Option<Arc<Vec<u8>>> },
    Dir { path: String, modified: f64 },
    Remove { path: String },
}

#[derive(Default)]
struct Inner {
    nodes: BTreeMap<String, Node>,
    next_blob: u64,
    now: f64,
    journal: Vec<Change>,
    missing: BTreeSet<u64>,
}

/// An in-memory [`Fs`].
pub struct MemFs {
    inner: Mutex<Inner>,
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}

fn not_found(p: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("{p}: no such file or folder"))
}

fn err(kind: io::ErrorKind, msg: String) -> io::Error {
    io::Error::new(kind, msg)
}

/// `/a/b` for any absolute path; `..` and `.` are resolved lexically.
fn key(path: &Path) -> io::Result<String> {
    let mut parts: Vec<String> = vec![];
    let mut absolute = false;
    for c in path.components() {
        match c {
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            Component::Prefix(_) => return Err(err(io::ErrorKind::InvalidInput, format!("{}: unsupported path", path.display()))),
        }
    }
    if !absolute {
        return Err(err(io::ErrorKind::InvalidInput, format!("{}: paths must be absolute", path.display())));
    }
    Ok(format!("/{}", parts.join("/")))
}

fn parent_key(k: &str) -> Option<String> {
    if k == "/" {
        return None;
    }
    let i = k.rfind('/').unwrap_or(0);
    Some(if i == 0 { "/".to_string() } else { k[..i].to_string() })
}

/// Prefix shared by everything inside folder `k`.
fn inside(k: &str) -> String {
    if k == "/" {
        "/".into()
    } else {
        format!("{k}/")
    }
}

impl Inner {
    fn keys_under(&self, k: &str) -> Vec<String> {
        let prefix = inside(k);
        self.nodes.range(prefix.clone()..).take_while(|(p, _)| p.starts_with(&prefix)).map(|(p, _)| p.clone()).filter(|p| p != "/").collect()
    }

    fn mkdirs(&mut self, k: &str) -> io::Result<()> {
        match self.nodes.get(k) {
            Some(Node::Dir { .. }) => return Ok(()),
            Some(Node::File(_)) => return Err(err(io::ErrorKind::AlreadyExists, format!("{k}: a file is in the way"))),
            None => {}
        }
        if let Some(p) = parent_key(k) {
            self.mkdirs(&p)?;
        }
        let modified = self.now;
        self.nodes.insert(k.to_string(), Node::Dir { modified });
        self.journal.push(Change::Dir { path: k.to_string(), modified });
        Ok(())
    }

    fn require_parent(&self, k: &str) -> io::Result<()> {
        match parent_key(k).and_then(|p| self.nodes.get(&p).cloned()) {
            Some(Node::Dir { .. }) => Ok(()),
            _ => Err(not_found(&parent_key(k).unwrap_or_default())),
        }
    }

    fn put_file(&mut self, k: &str, f: FileNode, new_data: bool) {
        self.journal.push(Change::File {
            path: k.to_string(),
            blob: f.blob,
            len: f.len,
            modified: f.modified,
            data: if new_data { f.data.clone() } else { None },
        });
        self.nodes.insert(k.to_string(), Node::File(f));
    }

    fn remove(&mut self, k: &str) {
        if self.nodes.remove(k).is_some() {
            self.journal.push(Change::Remove { path: k.to_string() });
        }
    }
}

impl MemFs {
    pub fn new() -> MemFs {
        let mut inner = Inner { next_blob: 1, ..Default::default() };
        inner.nodes.insert("/".into(), Node::Dir { modified: 0.0 });
        MemFs { inner: Mutex::new(inner) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Set the clock (the browser has no system time inside WebAssembly).
    pub fn set_now(&self, ms: f64) {
        self.lock().now = ms;
    }

    /// Rebuild an entry from storage (not journaled). A file's content can
    /// be given now or later through [`MemFs::provide`].
    pub fn restore_dir(&self, path: &str, modified: f64) {
        let Ok(k) = key(Path::new(path)) else { return };
        self.lock().nodes.insert(k, Node::Dir { modified });
    }

    pub fn restore_file(&self, path: &str, blob: u64, len: u64, modified: f64) {
        let Ok(k) = key(Path::new(path)) else { return };
        let mut g = self.lock();
        g.next_blob = g.next_blob.max(blob + 1);
        g.nodes.insert(k, Node::File(FileNode { blob, len, modified, data: None }));
    }

    /// Load a blob's content into every file that holds it.
    pub fn provide(&self, blob: u64, data: Vec<u8>) {
        let data = Arc::new(data);
        let mut g = self.lock();
        g.missing.remove(&blob);
        for n in g.nodes.values_mut() {
            if let Node::File(f) = n {
                if f.blob == blob {
                    f.data = Some(data.clone());
                }
            }
        }
    }

    /// Blobs that reads asked for but were not loaded (cleared).
    pub fn take_missing(&self) -> Vec<u64> {
        std::mem::take(&mut self.lock().missing).into_iter().collect()
    }

    /// Changes since the last call, in order.
    pub fn take_changes(&self) -> Vec<Change> {
        std::mem::take(&mut self.lock().journal)
    }

    /// Unloaded blobs of the files `want` selects (by path).
    pub fn unloaded(&self, want: impl Fn(&str) -> bool) -> Vec<u64> {
        let g = self.lock();
        let set: BTreeSet<u64> = g
            .nodes
            .iter()
            .filter_map(|(p, n)| match n {
                Node::File(f) if f.data.is_none() && want(p) => Some(f.blob),
                _ => None,
            })
            .collect();
        set.into_iter().collect()
    }

    /// Drop the in-memory content of the files `drop` selects; they are
    /// loaded again when needed. (Pending changes keep their own copy.)
    pub fn evict(&self, drop: impl Fn(&str) -> bool) {
        let mut g = self.lock();
        for (p, n) in g.nodes.iter_mut() {
            if let Node::File(f) = n {
                if f.data.is_some() && drop(p) {
                    f.data = None;
                }
            }
        }
    }

    /// The blob of a file and whether its content is loaded.
    pub fn blob(&self, path: &Path) -> Option<(u64, bool)> {
        let k = key(path).ok()?;
        match self.lock().nodes.get(&k) {
            Some(Node::File(f)) => Some((f.blob, f.data.is_some())),
            _ => None,
        }
    }

    /// Every blob some file holds.
    pub fn blobs(&self) -> BTreeSet<u64> {
        self.lock()
            .nodes
            .values()
            .filter_map(|n| match n {
                Node::File(f) => Some(f.blob),
                _ => None,
            })
            .collect()
    }
}

impl Fs for MemFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let k = key(path)?;
        let mut g = self.lock();
        match g.nodes.get(&k) {
            Some(Node::File(f)) => match &f.data {
                Some(d) => Ok(d.as_ref().clone()),
                None => {
                    let blob = f.blob;
                    g.missing.insert(blob);
                    Err(err(io::ErrorKind::WouldBlock, format!("{k}: content not loaded yet")))
                }
            },
            Some(Node::Dir { .. }) => Err(err(io::ErrorKind::InvalidInput, format!("{k} is a folder"))),
            None => Err(not_found(&k)),
        }
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let k = key(path)?;
        let mut g = self.lock();
        if let Some(Node::Dir { .. }) = g.nodes.get(&k) {
            return Err(err(io::ErrorKind::InvalidInput, format!("{k} is a folder")));
        }
        if let Some(p) = parent_key(&k) {
            g.mkdirs(&p)?;
        }
        let blob = g.next_blob;
        g.next_blob += 1;
        let f = FileNode { blob, len: bytes.len() as u64, modified: g.now, data: Some(Arc::new(bytes.to_vec())) };
        g.put_file(&k, f, true);
        Ok(())
    }

    fn metadata(&self, path: &Path) -> io::Result<Meta> {
        let k = key(path)?;
        match self.lock().nodes.get(&k) {
            Some(Node::Dir { modified }) => Ok(Meta { is_dir: true, len: 0, modified: *modified }),
            Some(Node::File(f)) => Ok(Meta { is_dir: false, len: f.len, modified: f.modified }),
            None => Err(not_found(&k)),
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let k = key(path)?;
        let g = self.lock();
        match g.nodes.get(&k) {
            Some(Node::Dir { .. }) => {}
            Some(Node::File(_)) => return Err(err(io::ErrorKind::InvalidInput, format!("{k} is not a folder"))),
            None => return Err(not_found(&k)),
        }
        let prefix = inside(&k);
        Ok(g.keys_under(&k).into_iter().filter(|p| !p[prefix.len()..].contains('/')).map(PathBuf::from).collect())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let k = key(path)?;
        self.lock().mkdirs(&k)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (a, b) = (key(from)?, key(to)?);
        if a == b {
            return Ok(());
        }
        if a == "/" {
            return Err(err(io::ErrorKind::InvalidInput, "cannot move /".into()));
        }
        let mut g = self.lock();
        let node = g.nodes.get(&a).cloned().ok_or_else(|| not_found(&a))?;
        g.require_parent(&b)?;
        match node {
            Node::File(f) => {
                match g.nodes.get(&b) {
                    Some(Node::Dir { .. }) => return Err(err(io::ErrorKind::AlreadyExists, format!("{b} is a folder"))),
                    Some(Node::File(_)) => g.remove(&b),
                    None => {}
                }
                g.remove(&a);
                g.put_file(&b, f, false);
            }
            Node::Dir { modified } => {
                if b.starts_with(&inside(&a)) {
                    return Err(err(io::ErrorKind::InvalidInput, format!("cannot move {a} into itself")));
                }
                if g.nodes.contains_key(&b) {
                    return Err(err(io::ErrorKind::AlreadyExists, format!("{b} already exists")));
                }
                let under = g.keys_under(&a);
                let moved: Vec<(String, Node)> = under.iter().map(|p| (format!("{b}{}", &p[a.len()..]), g.nodes[p].clone())).collect();
                for p in under.iter().rev() {
                    g.remove(p);
                }
                g.remove(&a);
                g.nodes.insert(b.clone(), Node::Dir { modified });
                g.journal.push(Change::Dir { path: b, modified });
                for (p, n) in moved {
                    match n {
                        Node::Dir { modified } => {
                            g.journal.push(Change::Dir { path: p.clone(), modified });
                            g.nodes.insert(p, Node::Dir { modified });
                        }
                        Node::File(f) => g.put_file(&p, f, false),
                    }
                }
            }
        }
        Ok(())
    }

    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (a, b) = (key(from)?, key(to)?);
        let mut g = self.lock();
        let Some(Node::File(f)) = g.nodes.get(&a).cloned() else {
            return Err(not_found(&a));
        };
        g.require_parent(&b)?;
        if let Some(Node::Dir { .. }) = g.nodes.get(&b) {
            return Err(err(io::ErrorKind::AlreadyExists, format!("{b} is a folder")));
        }
        let modified = g.now;
        g.put_file(&b, FileNode { modified, ..f }, false);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let k = key(path)?;
        let mut g = self.lock();
        match g.nodes.get(&k) {
            Some(Node::File(_)) => {
                g.remove(&k);
                Ok(())
            }
            Some(Node::Dir { .. }) => Err(err(io::ErrorKind::InvalidInput, format!("{k} is a folder"))),
            None => Err(not_found(&k)),
        }
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        let k = key(path)?;
        if k == "/" {
            return Err(err(io::ErrorKind::InvalidInput, "refusing to remove /".into()));
        }
        let mut g = self.lock();
        match g.nodes.get(&k) {
            Some(Node::Dir { .. }) => {}
            Some(Node::File(_)) => return Err(err(io::ErrorKind::InvalidInput, format!("{k} is not a folder"))),
            None => return Err(not_found(&k)),
        }
        for p in g.keys_under(&k).into_iter().rev() {
            g.remove(&p);
        }
        g.remove(&k);
        Ok(())
    }

    fn canonicalize(&self, path: &Path) -> PathBuf {
        key(path).map(PathBuf::from).unwrap_or_else(|_| path.to_path_buf())
    }

    fn now_ms(&self) -> f64 {
        self.lock().now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> &Path {
        Path::new(s)
    }

    #[test]
    fn files_and_folders() {
        let fs = MemFs::new();
        fs.set_now(1000.0);
        fs.write(p("/lib/song/project.json"), b"{}").unwrap();
        assert!(fs.is_dir(p("/lib/song")));
        assert_eq!(fs.read(p("/lib/./song/../song/project.json")).unwrap(), b"{}");
        assert_eq!(fs.metadata(p("/lib/song/project.json")).unwrap(), Meta { is_dir: false, len: 2, modified: 1000.0 });
        fs.create_dir_all(p("/lib/song/samples")).unwrap();
        fs.write(p("/lib/song/samples/a.wav"), b"RIFF").unwrap();
        assert_eq!(fs.read_dir(p("/lib/song")).unwrap(), vec![PathBuf::from("/lib/song/project.json"), PathBuf::from("/lib/song/samples")]);
        assert!(fs.read(p("/lib/song/nope")).is_err());
        assert!(fs.write(p("/lib/song/samples"), b"x").is_err(), "a folder is not a file");
        assert!(fs.write(p("relative"), b"x").is_err());

        // Rename a folder: everything moves, blobs are reused.
        fs.take_changes();
        let (blob, _) = fs.blob(p("/lib/song/samples/a.wav")).unwrap();
        fs.rename(p("/lib/song"), p("/lib/tune")).unwrap();
        assert!(!fs.exists(p("/lib/song")));
        assert_eq!(fs.blob(p("/lib/tune/samples/a.wav")).unwrap().0, blob);
        let changes = fs.take_changes();
        assert!(changes.iter().all(|c| !matches!(c, Change::File { data: Some(_), .. })), "a rename stores no new content");
        assert!(fs.rename(p("/lib/tune"), p("/lib/tune/inner")).is_err());

        fs.copy(p("/lib/tune/samples/a.wav"), p("/lib/tune/b.wav")).unwrap();
        assert_eq!(fs.read(p("/lib/tune/b.wav")).unwrap(), b"RIFF");
        fs.remove_file(p("/lib/tune/b.wav")).unwrap();
        fs.remove_dir_all(p("/lib/tune")).unwrap();
        assert_eq!(fs.read_dir(p("/lib")).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn lazy_content() {
        let fs = MemFs::new();
        fs.restore_dir("/s", 0.0);
        fs.restore_file("/s/big.wav", 7, 3, 5.0);
        assert!(fs.is_file(p("/s/big.wav")));
        let e = fs.read(p("/s/big.wav")).unwrap_err();
        assert!(crate::is_not_loaded(&e));
        assert_eq!(fs.take_missing(), vec![7]);
        fs.provide(7, b"abc".to_vec());
        assert_eq!(fs.read(p("/s/big.wav")).unwrap(), b"abc");
        fs.evict(|p| p.ends_with(".wav"));
        assert_eq!(fs.blob(p("/s/big.wav")), Some((7, false)));
        assert_eq!(fs.unloaded(|_| true), vec![7]);
        // New blobs never collide with restored ones.
        fs.write(p("/s/new.wav"), b"x").unwrap();
        assert_eq!(fs.blob(p("/s/new.wav")).unwrap().0, 8);
        assert!(!fs.take_changes().is_empty());
    }
}
