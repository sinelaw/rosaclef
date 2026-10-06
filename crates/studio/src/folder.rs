//! A project folder.
//!
//! ```text
//! my-song/
//!   project.json          the song (the single source of truth)
//!   project.schema.json   JSON schema for project.json
//!   AGENTS.md             guide for coding agents (CLAUDE.md / GEMINI.md point to it)
//!   samples/              audio files used by samplers and audio clips
//!   renders/              exported mixdowns
//!   .rosaclef/            live state written by the studio (status, UI context)
//! ```

use anyhow::{Context, Result};
use rosaclef_core::{format, schema, validate, Project, PROJECT_FILE};
use rosaclef_fs::SharedFs;
use std::path::{Path, PathBuf};

pub const SCHEMA_FILE: &str = "project.schema.json";
pub const STATE_DIR: &str = ".rosaclef";
pub const SAMPLES_DIR: &str = "samples";
pub const RENDERS_DIR: &str = "renders";

pub const DEMO_PROJECT: &str = include_str!("../assets/demo/project.json");

/// The name a project made from the demo song starts with: the song's own
/// title (`meta.title` in its project.json).
pub fn demo_title() -> String {
    let project: Project = serde_json::from_str(DEMO_PROJECT).expect("bundled demo is valid");
    crate::library::sanitize_name(&project.meta.title)
}

#[derive(Clone)]
pub struct Folder {
    pub fs: SharedFs,
    pub dir: PathBuf,
}

impl std::fmt::Debug for Folder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Folder").field("dir", &self.dir).finish()
    }
}

impl Folder {
    pub fn new(fs: SharedFs, dir: impl Into<PathBuf>) -> Folder {
        let dir: PathBuf = dir.into();
        let dir = fs.canonicalize(&dir);
        Folder { fs, dir }
    }

    /// A folder on the machine's disk.
    pub fn on_disk(dir: impl Into<PathBuf>) -> Folder {
        Folder::new(rosaclef_fs::disk(), dir)
    }

    /// The folder's name (the project's name in the library).
    pub fn name(&self) -> String {
        self.dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    pub fn project_path(&self) -> PathBuf {
        self.dir.join(PROJECT_FILE)
    }
    pub fn state_path(&self, name: &str) -> PathBuf {
        self.dir.join(STATE_DIR).join(name)
    }

    /// Resolve a project-relative path, refusing anything that escapes the folder.
    pub fn resolve(&self, rel: &str) -> Option<PathBuf> {
        let rel = rel.trim_start_matches("./");
        if rel.is_empty() || rel.starts_with('/') || rel.split(['/', '\\']).any(|s| s == "..") {
            return None;
        }
        Some(self.dir.join(rel))
    }

    /// Create the folder layout; seed `project.json` if missing.
    pub fn init(&self, demo: bool) -> Result<()> {
        let fs = &self.fs;
        fs.create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        for d in [SAMPLES_DIR, RENDERS_DIR, STATE_DIR] {
            fs.create_dir_all(&self.dir.join(d))?;
        }
        if !fs.exists(&self.project_path()) {
            let project: Project = if demo {
                serde_json::from_str(DEMO_PROJECT).expect("bundled demo is valid")
            } else {
                let title = self
                    .dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled")
                    .to_string();
                let mut p = Project::empty(&title);
                p.schema = format!("./{SCHEMA_FILE}");
                p
            };
            self.write_project(&project)?;
        }
        fs.write_if_changed(&self.dir.join(SCHEMA_FILE), &schema::schema_text())?;
        fs.write_if_changed(
            &self.state_path("context.schema.json"),
            &rosaclef_core::context::schema_text(),
        )?;
        fs.write_if_changed(
            &self.state_path("mixcheck.schema.json"),
            crate::mixcheck::SCHEMA,
        )?;
        let gitignore = self.dir.join(STATE_DIR).join(".gitignore");
        if !fs.exists(&gitignore) {
            fs.write(
                &gitignore,
                b"*\n!context.schema.json\n!mixcheck.schema.json\n",
            )?;
        }
        Ok(())
    }

    pub fn read_text(&self) -> Result<String> {
        self.fs
            .read_to_string(&self.project_path())
            .with_context(|| format!("reading {}", self.project_path().display()))
    }

    /// Serialize and atomically write the project. Returns the written text.
    pub fn write_project(&self, p: &Project) -> Result<String> {
        let text = format::to_string(p);
        self.fs.write(&self.project_path(), text.as_bytes())?;
        Ok(text)
    }

    pub fn load(&self) -> Result<validate::Checked> {
        Ok(validate::parse_and_validate(&self.read_text()?))
    }

    /// Write a file of the folder (project-relative path).
    pub fn write(&self, rel: &str, bytes: &[u8]) -> Result<()> {
        let path = self
            .resolve(rel)
            .with_context(|| format!("invalid path {rel:?}"))?;
        Ok(self.fs.write(&path, bytes)?)
    }

    /// List audio files under `samples/` (project-relative paths).
    pub fn list_samples(&self) -> Vec<String> {
        let mut out = vec![];
        let root = self.dir.join(SAMPLES_DIR);
        walk(self, &root, &mut |p| {
            if crate::decode::is_audio_file(p) {
                if let Ok(rel) = p.strip_prefix(&self.dir) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        });
        out.sort();
        out
    }
}

fn walk(f: &Folder, dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = f.fs.read_dir(dir) else {
        return;
    };
    for p in entries {
        if f.fs.is_dir(&p) {
            walk(f, &p, visit);
        } else {
            visit(&p);
        }
    }
}

/// Stable content hash, used to recognise our own writes in the file watcher.
pub fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}
