//! The on-disk project folder.
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
use std::path::{Path, PathBuf};

pub const SCHEMA_FILE: &str = "project.schema.json";
pub const STATE_DIR: &str = ".rosaclef";
pub const SAMPLES_DIR: &str = "samples";
pub const RENDERS_DIR: &str = "renders";

pub const DEMO_PROJECT: &str = include_str!("../assets/demo/project.json");

#[derive(Clone, Debug)]
pub struct Folder {
    pub dir: PathBuf,
}

impl Folder {
    pub fn new(dir: impl Into<PathBuf>) -> Folder {
        let dir: PathBuf = dir.into();
        let dir = dir.canonicalize().unwrap_or(dir);
        Folder { dir }
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
        std::fs::create_dir_all(&self.dir).with_context(|| format!("creating {}", self.dir.display()))?;
        for d in [SAMPLES_DIR, RENDERS_DIR, STATE_DIR] {
            std::fs::create_dir_all(self.dir.join(d))?;
        }
        if !self.project_path().exists() {
            let project: Project = if demo {
                serde_json::from_str(DEMO_PROJECT).expect("bundled demo is valid")
            } else {
                let title = self.dir.file_name().and_then(|n| n.to_str()).unwrap_or("Untitled").to_string();
                let mut p = Project::empty(&title);
                p.schema = format!("./{SCHEMA_FILE}");
                p
            };
            self.write_project(&project)?;
        }
        write_if_changed(&self.dir.join(SCHEMA_FILE), &schema::schema_text())?;
        write_if_changed(&self.state_path("context.schema.json"), &rosaclef_core::context::schema_text())?;
        let gitignore = self.dir.join(STATE_DIR).join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(gitignore, "*\n!context.schema.json\n")?;
        }
        Ok(())
    }

    pub fn read_text(&self) -> Result<String> {
        std::fs::read_to_string(self.project_path()).with_context(|| format!("reading {}", self.project_path().display()))
    }

    /// Serialize and atomically write the project. Returns the written text.
    pub fn write_project(&self, p: &Project) -> Result<String> {
        let text = format::to_string(p);
        write_atomic(&self.project_path(), text.as_bytes())?;
        Ok(text)
    }

    pub fn load(&self) -> Result<validate::Checked> {
        Ok(validate::parse_and_validate(&self.read_text()?))
    }

    /// List audio files under `samples/` (project-relative paths).
    pub fn list_samples(&self) -> Vec<String> {
        let mut out = vec![];
        let root = self.dir.join(SAMPLES_DIR);
        walk(&root, &mut |p| {
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

fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, f);
        } else {
            f(&p);
        }
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", path.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

pub fn write_if_changed(path: &Path, text: &str) -> Result<()> {
    if std::fs::read_to_string(path).map(|t| t == text).unwrap_or(false) {
        return Ok(());
    }
    write_atomic(path, text.as_bytes())
}

/// Stable content hash, used to recognise our own writes in the file watcher.
pub fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}
