//! The project library and the file manager.
//!
//! The library is a folder with one project per subfolder (a folder holding
//! `project.json`). The studio can list, create, open, duplicate, rename and
//! delete projects there, and import LMMS and MIDI files as new projects.
//! Deleted projects and files are moved into a `.trash/` folder (inside the
//! library, or inside the project for files) until the trash is emptied.

use crate::folder::{self, Folder};
use anyhow::{anyhow, bail, Context, Result};
use rosaclef_core::{Project, PROJECT_FILE};
use rosaclef_fs::SharedFs;
use rosaclef_import::Imported;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Folder (inside the library, or inside a project) receiving deleted items.
pub const TRASH_DIR: &str = ".trash";

// ------------------------------------------------------------------ names

/// A project (folder) name: no path separators, not hidden, no traversal.
pub fn valid_name(name: &str) -> bool {
    name.trim() == name
        && !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || " _-.()+,&'".contains(c))
}

/// Turn any text (a file name, a title) into a valid project name.
pub fn sanitize_name(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " _-()+,&'".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut out = String::new();
    for c in mapped.chars() {
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let mut out: String = out.trim_matches(['-', ' ', '.']).chars().take(60).collect();
    out = out.trim_end_matches(['-', ' ', '.']).to_string();
    if out.is_empty() {
        "Imported".into()
    } else {
        out
    }
}

pub fn check_name(name: &str) -> Result<()> {
    if !valid_name(name) {
        bail!("invalid project name {name:?}: use letters, digits, spaces and _-.()+,&' (no slashes, not starting with a dot)");
    }
    Ok(())
}

/// Seconds since the epoch, for names in the trash.
fn stamp(fs: &SharedFs) -> String {
    format!("{}", (fs.now_ms() / 1000.0).floor() as i64)
}

/// A file name for the trash: `<stamp> <name>`, made unique.
fn trash_name(fs: &SharedFs, trash: &Path, name: &str) -> String {
    let s = stamp(fs);
    let mut dst = format!("{s} {name}");
    let mut n = 2;
    while fs.exists(&trash.join(&dst)) {
        dst = format!("{s} {n} {name}");
        n += 1;
    }
    dst
}

// ------------------------------------------------------------------ library

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub name: String,
    pub folder: String,
    pub title: String,
    pub bpm: f64,
    pub beats_per_bar: f64,
    /// Last modification of `project.json`, in ms since the epoch.
    pub modified: f64,
    pub current: bool,
    pub channels: usize,
    pub patterns: usize,
    pub clips: usize,
    pub length_beats: f64,
    /// `project.json` does not parse or validate.
    pub invalid: bool,
}

/// The project library: a folder of project folders.
#[derive(Clone)]
pub struct Library {
    pub fs: SharedFs,
    pub dir: PathBuf,
    /// How agents reach the `rosaclef` command (written into the guides).
    pub exe: String,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").field("dir", &self.dir).finish()
    }
}

impl Library {
    pub fn new(fs: SharedFs, dir: impl Into<PathBuf>, exe: &str) -> Library {
        Library {
            fs,
            dir: dir.into(),
            exe: exe.to_string(),
        }
    }

    /// The folder of a project (whether it exists or not).
    pub fn folder(&self, name: &str) -> Folder {
        Folder::new(self.fs.clone(), self.dir.join(name))
    }

    /// The folder of an existing project.
    pub fn project_dir(&self, name: &str) -> Result<PathBuf> {
        check_name(name)?;
        let dir = self.dir.join(name);
        if !self.fs.is_file(&dir.join(PROJECT_FILE)) {
            bail!("no project named {name:?} in the library");
        }
        Ok(dir)
    }

    /// `base`, or `base 2`, `base 3`, ... — the first that is free in the library.
    pub fn unique_name(&self, base: &str) -> String {
        let mut name = base.to_string();
        let mut n = 2;
        while self.fs.exists(&self.dir.join(&name)) {
            name = format!("{base} {n}");
            n += 1;
        }
        name
    }

    /// Summary of one project folder (read leniently: a broken project is still listed).
    fn info(&self, dir: &Path, current: &Path) -> Option<ProjectInfo> {
        let fs = &self.fs;
        let file = dir.join(PROJECT_FILE);
        let meta = fs.metadata(&file).ok()?;
        if meta.is_dir {
            return None;
        }
        let name = dir.file_name()?.to_string_lossy().to_string();
        let text = fs.read_to_string(&file).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let count = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        };
        let clips = v.pointer("/playlist/clips").and_then(|c| c.as_array());
        let length = clips
            .map(|cs| {
                cs.iter()
                    .map(|c| {
                        c.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0)
                            + c.get("length").and_then(|x| x.as_f64()).unwrap_or(0.0)
                    })
                    .fold(0.0, f64::max)
            })
            .unwrap_or(0.0);
        let invalid = !rosaclef_core::validate::parse_and_validate(&text).is_ok();
        let canon = fs.canonicalize(dir);
        Some(ProjectInfo {
            title: v
                .pointer("/meta/title")
                .and_then(|x| x.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or(&name)
                .to_string(),
            bpm: v
                .pointer("/transport/bpm")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0),
            beats_per_bar: v
                .pointer("/transport/beatsPerBar")
                .and_then(|x| x.as_f64())
                .unwrap_or(4.0),
            modified: meta.modified,
            current: canon == current,
            channels: count("channels"),
            patterns: count("patterns"),
            clips: clips.map(|c| c.len()).unwrap_or(0),
            length_beats: length,
            invalid,
            folder: canon.display().to_string(),
            name,
        })
    }

    /// Every project in the library, most recently modified first.
    pub fn list(&self, current: &Path) -> Vec<ProjectInfo> {
        let mut out: Vec<ProjectInfo> = self
            .fs
            .read_dir(&self.dir)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|p| {
                        !p.file_name()
                            .map(|n| n.to_string_lossy().starts_with('.'))
                            .unwrap_or(true)
                    })
                    .filter(|p| self.fs.is_dir(p))
                    .filter_map(|p| self.info(p, current))
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by(|a, b| {
            b.modified
                .total_cmp(&a.modified)
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    /// Create a project folder in the library (empty or from the demo song).
    pub fn create(&self, name: &str, demo: bool) -> Result<Folder> {
        check_name(name)?;
        let dir = self.dir.join(name);
        if self.fs.exists(&dir) {
            bail!("{name:?} already exists in the library");
        }
        self.fs.create_dir_all(&self.dir)?;
        let f = Folder::new(self.fs.clone(), &dir);
        f.init(demo)?;
        if demo {
            // Name the song after its folder, so library cards tell them apart.
            let mut p = f
                .load()?
                .project
                .ok_or_else(|| anyhow!("the demo project is invalid"))?;
            p.meta.title = name.to_string();
            f.write_project(&p)?;
        }
        crate::guide::write(&f, &self.exe)?;
        Ok(f)
    }

    fn copy_dir(&self, from: &Path, to: &Path) -> Result<()> {
        let fs = &self.fs;
        fs.create_dir_all(to)?;
        for src in fs.read_dir(from)? {
            let Some(name) = src.file_name().map(|n| n.to_os_string()) else {
                continue;
            };
            let n = name.to_string_lossy();
            if n == folder::STATE_DIR || n == TRASH_DIR {
                continue;
            }
            let dst = to.join(&name);
            if fs.is_dir(&src) {
                self.copy_dir(&src, &dst)?;
            } else {
                fs.copy(&src, &dst)
                    .with_context(|| format!("copying {}", src.display()))?;
            }
        }
        Ok(())
    }

    /// Copy a project (without its live state) under a new name. The copy's
    /// song is titled with that name, so library cards tell the two apart.
    pub fn duplicate(&self, name: &str, to: &str) -> Result<()> {
        let src = self.project_dir(name)?;
        check_name(to)?;
        let dst = self.dir.join(to);
        if self.fs.exists(&dst) {
            bail!("{to:?} already exists in the library");
        }
        self.copy_dir(&src, &dst)?;
        let f = Folder::new(self.fs.clone(), &dst);
        f.init(false)?;
        if let Ok(Some(mut p)) = f.load().map(|c| c.project) {
            p.meta.title = to.to_string();
            f.write_project(&p)?;
        }
        crate::guide::write(&f, &self.exe)?;
        Ok(())
    }

    /// Rename a project that is not open. A song titled after its folder
    /// keeps following the folder's name.
    pub fn rename(&self, name: &str, to: &str) -> Result<()> {
        let src = self.project_dir(name)?;
        let dst = self.checked_target(to)?;
        self.fs.rename(&src, &dst)?;
        follow_title(&Folder::new(self.fs.clone(), &dst), name, to)
    }

    /// Where a project renamed to `to` would live (refused when taken).
    pub fn checked_target(&self, to: &str) -> Result<PathBuf> {
        check_name(to)?;
        let dst = self.dir.join(to);
        if self.fs.exists(&dst) {
            bail!("{to:?} already exists in the library");
        }
        Ok(dst)
    }

    /// Move a library entry (a project folder) into the library's trash.
    pub fn trash(&self, name: &str) -> Result<PathBuf> {
        let src = self.project_dir(name)?;
        let trash = self.dir.join(TRASH_DIR);
        self.fs.create_dir_all(&trash)?;
        let dst = trash.join(trash_name(&self.fs, &trash, name));
        self.fs
            .rename(&src, &dst)
            .with_context(|| format!("moving {} to the trash", src.display()))?;
        Ok(dst)
    }

    /// Delete the library's trash for good. Returns how many projects were in it.
    pub fn empty_trash(&self) -> Result<usize> {
        empty_dir(&self.fs, &self.dir.join(TRASH_DIR))
    }

    /// Write an imported project as a new library folder; returns its name.
    pub fn save_imported(&self, name: &str, im: &Imported) -> Result<(String, Vec<String>)> {
        let name = self.unique_name(&sanitize_name(name));
        let dir = self.dir.join(&name);
        self.fs.create_dir_all(&dir)?;
        let f = Folder::new(self.fs.clone(), &dir);
        let mut project = im.project.clone();
        project.meta.title = if project.meta.title.is_empty() {
            name.clone()
        } else {
            project.meta.title.clone()
        };
        f.write_project(&project)?;
        f.init(false)?;
        let mut warnings = im.warnings.clone();
        for s in &im.samples {
            let Some(dst) = f.resolve(&s.to) else {
                continue;
            };
            // The sample comes from outside the library (the importer's disk).
            let copied = std::fs::read(&s.from)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| Ok(self.fs.write(&dst, &bytes)?));
            if let Err(e) = copied {
                warnings.push(format!(
                    "could not copy {} to {}: {e}",
                    s.from.display(),
                    s.to
                ));
            }
        }
        crate::guide::write(&f, &self.exe)?;
        Ok((name, warnings))
    }
}

/// A song titled after its folder keeps following the folder's name.
fn follow_title(f: &Folder, old: &str, new: &str) -> Result<()> {
    let Ok(checked) = f.load() else { return Ok(()) };
    if let Some(mut p) = checked.project {
        if p.meta.title == old || p.meta.title.is_empty() {
            p.meta.title = new.to_string();
            f.write_project(&p)?;
        }
    }
    Ok(())
}

/// Remove everything inside `dir` (kept, empty). Returns how many entries it held.
fn empty_dir(fs: &SharedFs, dir: &Path) -> Result<usize> {
    let Ok(entries) = fs.read_dir(dir) else {
        return Ok(0);
    };
    for p in &entries {
        if fs.is_dir(p) {
            fs.remove_dir_all(p)?;
        } else {
            fs.remove_file(p)?;
        }
    }
    Ok(entries.len())
}

// ------------------------------------------------------------------ files

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    /// Project-relative path.
    pub path: String,
    pub name: String,
    /// Folder part of `path` ("" at the top level).
    pub dir: String,
    pub size: f64,
    pub modified: f64,
    /// `audio` or `other`.
    pub kind: String,
    /// Referenced by the project (a sampler or an audio clip).
    pub used: bool,
    /// The studio's own files, which cannot be renamed or deleted here.
    pub managed: bool,
}

const PROTECTED: &[&str] = &[PROJECT_FILE, folder::SCHEMA_FILE];

/// Project-relative paths of every audio file the project uses.
pub fn referenced(p: &Project) -> HashSet<String> {
    let mut out: HashSet<String> = p
        .playlist
        .clips
        .iter()
        .filter(|c| !c.sample.is_empty())
        .map(|c| c.sample.clone())
        .collect();
    for ch in &p.channels {
        if let Some(s) = ch
            .instrument
            .options
            .get("sample")
            .filter(|s| !s.is_empty())
        {
            out.insert(s.clone());
        }
    }
    out
}

/// Non-hidden files of the project (under `sub`, recursively).
pub fn files(f: &Folder, sub: &str, project: &Project) -> Result<Vec<FileInfo>> {
    let fs = &f.fs;
    let root = if sub.is_empty() {
        f.dir.clone()
    } else {
        f.resolve(sub)
            .ok_or_else(|| anyhow!("invalid folder {sub:?}"))?
    };
    let used = referenced(project);
    let mut out = vec![];
    fn walk(fs: &SharedFs, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs.read_dir(dir) else {
            return;
        };
        for p in entries {
            if p.file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(true)
                || out.len() > 10_000
            {
                continue;
            }
            if fs.is_dir(&p) {
                if depth < 8 {
                    walk(fs, &p, depth + 1, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut paths = vec![];
    walk(fs, &root, 0, &mut paths);
    for p in paths {
        let Ok(rel) = p.strip_prefix(&f.dir) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let meta = fs.metadata(&p).ok();
        let (dir, name) = match rel.rfind('/') {
            Some(i) => (rel[..i].to_string(), rel[i + 1..].to_string()),
            None => (String::new(), rel.clone()),
        };
        out.push(FileInfo {
            kind: if crate::decode::is_audio_file(&p) {
                "audio".into()
            } else {
                "other".into()
            },
            used: used.contains(&rel),
            managed: dir.is_empty() && PROTECTED.contains(&name.as_str()),
            size: meta.map(|m| m.len as f64).unwrap_or(0.0),
            modified: meta.map(|m| m.modified).unwrap_or(0.0),
            path: rel,
            name,
            dir,
        });
    }
    out.sort_by(|a, b| {
        a.dir
            .cmp(&b.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn existing_file(f: &Folder, rel: &str) -> Result<PathBuf> {
    let p = f
        .resolve(rel)
        .ok_or_else(|| anyhow!("invalid path {rel:?}"))?;
    if rel.split('/').any(|s| s.starts_with('.')) {
        bail!("hidden files cannot be changed here");
    }
    if !f.fs.is_file(&p) {
        bail!("{rel} does not exist");
    }
    if PROTECTED.contains(&rel) {
        bail!("{rel} is managed by the studio and cannot be renamed or deleted");
    }
    Ok(p)
}

/// Rename a file inside its folder. `to` is a file name; the extension is
/// kept when `to` has none. Returns the new project-relative path.
pub fn rename_path(f: &Folder, rel: &str, to: &str) -> Result<String> {
    let src = existing_file(f, rel)?;
    let to = to.trim();
    if to.is_empty()
        || to.len() > 128
        || to.starts_with('.')
        || to.contains(['/', '\\'])
        || to.chars().any(|c| c.is_control())
    {
        bail!("invalid file name {to:?}");
    }
    let ext = Path::new(rel)
        .extension()
        .map(|e| e.to_string_lossy().to_string());
    let to = match (Path::new(to).extension(), ext) {
        (None, Some(e)) => format!("{to}.{e}"),
        _ => to.to_string(),
    };
    let new_rel = match rel.rfind('/') {
        Some(i) => format!("{}/{to}", &rel[..i]),
        None => to.clone(),
    };
    let dst = f
        .resolve(&new_rel)
        .ok_or_else(|| anyhow!("invalid file name"))?;
    if f.fs.exists(&dst) {
        bail!("{new_rel} already exists");
    }
    f.fs.rename(&src, &dst)?;
    Ok(new_rel)
}

/// Point every reference to `from` at `to`. Returns how many changed.
pub fn rewrite_refs(p: &mut Project, from: &str, to: &str) -> usize {
    let mut n = 0;
    for c in &mut p.playlist.clips {
        if c.sample == from {
            c.sample = to.to_string();
            n += 1;
        }
    }
    for ch in &mut p.channels {
        if let Some(s) = ch.instrument.options.get_mut("sample") {
            if s == from {
                *s = to.to_string();
                n += 1;
            }
        }
    }
    n
}

/// Move a project file into the project's `.trash/`.
pub fn trash_path(f: &Folder, rel: &str) -> Result<String> {
    let src = existing_file(f, rel)?;
    let trash = f.dir.join(TRASH_DIR);
    f.fs.create_dir_all(&trash)?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let dst_name = trash_name(&f.fs, &trash, &name);
    f.fs.rename(&src, &trash.join(&dst_name))?;
    Ok(format!("{TRASH_DIR}/{dst_name}"))
}

/// Delete a project's trash for good. Returns how many files were in it.
pub fn empty_trash(f: &Folder) -> Result<usize> {
    empty_dir(&f.fs, &f.dir.join(TRASH_DIR))
}

/// A project-relative path under `samples/` for a new file called `name`
/// (cleaned up, and made unique).
pub fn unique_sample_path(f: &Folder, name: &str) -> String {
    let clean: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("sample.wav")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "._- ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = if clean.trim_start_matches('.').is_empty() {
        "sample.wav".to_string()
    } else {
        clean
    };
    let (stem, ext) = match clean.rfind('.') {
        Some(i) => (clean[..i].to_string(), clean[i..].to_string()),
        None => (clean.clone(), String::new()),
    };
    let mut rel = format!("{}/{stem}{ext}", folder::SAMPLES_DIR);
    let mut n = 2;
    while f.fs.exists(&f.dir.join(&rel)) {
        rel = format!("{}/{stem}-{n}{ext}", folder::SAMPLES_DIR);
        n += 1;
    }
    rel
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosaclef_fs::{Fs, MemFs};
    use std::sync::Arc;

    /// Run a test against the disk and against memory.
    fn both(tag: &str, test: impl Fn(Library)) {
        let d = std::env::temp_dir().join(format!("rosaclef-library-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        test(Library::new(
            rosaclef_fs::disk(),
            d.canonicalize().unwrap(),
            "rosaclef",
        ));
        let mem = MemFs::new();
        mem.set_now(1_700_000_000_000.0);
        mem.create_dir_all(Path::new("/library")).unwrap();
        test(Library::new(Arc::new(mem), "/library", "rosaclef"));
    }

    #[test]
    fn names() {
        for ok in ["song", "My Song 2", "Été (live)", "a.b"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in [
            "",
            " x",
            "x ",
            ".hidden",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "x.",
            "a\0b",
            &"x".repeat(65),
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert_eq!(sanitize_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(sanitize_name("My Track.mmpz"), "My Track-mmpz");
        assert_eq!(sanitize_name("..."), "Imported");
        assert!(valid_name(&sanitize_name("weird/\\:*?name")));
    }

    #[test]
    fn library_lifecycle() {
        both("life", |lib| {
            let fs = lib.fs.clone();
            let a = lib.create("Alpha", false).unwrap();
            lib.create("Beta", true).unwrap();
            assert!(lib.create("Alpha", false).is_err());
            assert!(lib.create("../escape", false).is_err());
            let list1 = lib.list(&a.dir);
            let names: HashSet<String> = list1.iter().map(|p| p.name.clone()).collect();
            assert_eq!(
                names,
                ["Alpha".to_string(), "Beta".to_string()]
                    .into_iter()
                    .collect()
            );
            let alpha = list1.iter().find(|p| p.name == "Alpha").unwrap();
            assert!(alpha.current && !alpha.invalid);
            assert_eq!(alpha.bpm, 120.0);
            let beta = list1.iter().find(|p| p.name == "Beta").unwrap();
            assert_eq!(beta.title, "Beta");
            assert!(beta.channels > 0 && beta.clips > 0);

            fs.create_dir_all(&a.dir.join(".rosaclef")).unwrap();
            fs.write(&a.dir.join("samples/x.wav"), b"x").unwrap();
            lib.duplicate("Alpha", "Gamma").unwrap();
            assert!(fs.is_file(&lib.dir.join("Gamma/samples/x.wav")));
            assert!(fs.is_file(&lib.dir.join("Gamma/AGENTS.md")));
            let title = |n: &str| lib.folder(n).load().unwrap().project.unwrap().meta.title;
            assert_eq!(
                title("Gamma"),
                "Gamma",
                "the copy is titled with its own name"
            );
            assert!(lib.duplicate("Alpha", "Beta").is_err());
            // Even when the song has a title of its own, the copy is told apart.
            let bf = lib.folder("Beta");
            let mut bp = bf.load().unwrap().project.unwrap();
            bp.meta.title = "Velvet Hour".into();
            bf.write_project(&bp).unwrap();
            lib.duplicate("Beta", "Beta copy").unwrap();
            assert_eq!(title("Beta"), "Velvet Hour");
            assert_eq!(title("Beta copy"), "Beta copy");
            lib.trash("Beta copy").unwrap();

            // Renaming a project titled after its folder renames the song too.
            lib.rename("Gamma", "Delta").unwrap();
            assert_eq!(title("Delta"), "Delta");
            assert!(lib.rename("Delta", "Alpha").is_err());

            let trashed = lib.trash("Delta").unwrap();
            assert!(trashed.starts_with(lib.dir.join(TRASH_DIR)));
            assert!(!fs.exists(&lib.dir.join("Delta")));
            assert_eq!(lib.list(&a.dir).len(), 2, "the trash is not listed");
            assert_eq!(lib.unique_name("Alpha"), "Alpha 2");
            assert_eq!(lib.empty_trash().unwrap(), 2);
            assert_eq!(lib.empty_trash().unwrap(), 0);
        });
    }

    #[test]
    fn file_manager() {
        both("files", |lib| {
            let fs = lib.fs.clone();
            let f = lib.create("Song", false).unwrap();
            fs.write(&f.dir.join("samples/kick.wav"), b"RIFF").unwrap();
            fs.write(&f.dir.join("renders/mix.wav"), b"RIFF").unwrap();
            fs.write(&f.dir.join("notes.txt"), b"hi").unwrap();
            let mut p = f.load().unwrap().project.unwrap();
            let mut dev = rosaclef_core::Device::new("sampler");
            dev.options
                .insert("sample".into(), "samples/kick.wav".into());
            p.channels.push(rosaclef_core::Channel {
                id: "k".into(),
                name: "K".into(),
                color: "#ffffff".into(),
                instrument: dev,
                volume: 0.8,
                pan: 0.0,
                mute: false,
                mixer: rosaclef_core::InsertIx(0),
                arp: None,
            });

            let list = files(&f, "", &p).unwrap();
            let find = |path: &str| list.iter().find(|x| x.path == path);
            assert!(
                find(".rosaclef/status.json").is_none(),
                "hidden files are not listed"
            );
            let kick = find("samples/kick.wav").unwrap();
            assert!(kick.used && kick.kind == "audio" && kick.dir == "samples" && kick.size == 4.0);
            assert!(find("project.json").unwrap().managed);
            assert_eq!(find("notes.txt").unwrap().kind, "other");
            assert_eq!(files(&f, "renders", &p).unwrap().len(), 1);
            assert!(files(&f, "../", &p).is_err());
            assert_eq!(f.list_samples(), vec!["samples/kick.wav".to_string()]);
            assert_eq!(unique_sample_path(&f, "../kick.wav"), "samples/kick-2.wav");

            assert_eq!(
                rename_path(&f, "samples/kick.wav", "boom").unwrap(),
                "samples/boom.wav"
            );
            assert_eq!(
                rewrite_refs(&mut p, "samples/kick.wav", "samples/boom.wav"),
                1
            );
            assert_eq!(
                p.channels[0].instrument.options["sample"],
                "samples/boom.wav"
            );
            assert!(rename_path(&f, "project.json", "x.json").is_err());
            assert!(rename_path(&f, "samples/boom.wav", "../x.wav").is_err());
            assert!(rename_path(&f, "../Song/project.json", "x").is_err());

            let t = trash_path(&f, "renders/mix.wav").unwrap();
            assert!(t.starts_with(".trash/") && fs.is_file(&f.dir.join(&t)));
            assert!(trash_path(&f, "project.json").is_err());
            assert!(trash_path(&f, ".rosaclef/status.json").is_err());
            assert_eq!(empty_trash(&f).unwrap(), 1);
            assert!(!fs.exists(&f.dir.join(&t)));
        });
    }
}
