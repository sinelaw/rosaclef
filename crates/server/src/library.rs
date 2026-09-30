//! The project library and the file manager.
//!
//! The library is a folder with one project per subfolder (a folder holding
//! `project.json`). The studio can list, create, open, duplicate, rename and
//! delete projects there, and import LMMS and MIDI files as new projects.
//! Deleted projects and files are moved into a `.trash/` folder (inside the
//! library, or inside the project for files), never removed.

use crate::folder::{self, Folder};
use crate::server::{forbidden, same_origin, Shared};
use anyhow::{anyhow, bail, Context, Result};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rosaclef_core::{Project, PROJECT_FILE};
use rosaclef_import::Imported;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Folder (inside the library, or inside a project) receiving deleted items.
pub const TRASH_DIR: &str = ".trash";

/// Largest file accepted by the import and upload endpoints.
const MAX_UPLOAD: usize = 512 << 20;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/open", post(open_project))
        .route("/api/projects/duplicate", post(duplicate_project))
        .route("/api/projects/rename", post(rename_project))
        .route("/api/projects/import-lmms", post(import_lmms))
        .route("/api/projects/import-midi", post(import_midi))
        .route("/api/projects/{name}", delete(delete_project))
        .route("/api/import-midi", post(import_midi_into))
        .route("/api/files", get(list_files).delete(delete_file))
        .route("/api/files/rename", post(rename_file))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
}

// ------------------------------------------------------------------ names

/// A project (folder) name: no path separators, not hidden, no traversal.
pub fn valid_name(name: &str) -> bool {
    name.trim() == name
        && !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name.chars().all(|c| c.is_alphanumeric() || " _-.()+,&'".contains(c))
}

/// Turn any text (a file name, a title) into a valid project name.
pub fn sanitize_name(s: &str) -> String {
    let mapped: String = s.chars().map(|c| if c.is_alphanumeric() || " _-()+,&'".contains(c) { c } else { '-' }).collect();
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

/// `base`, or `base 2`, `base 3`, ... — the first that is free in the library.
pub fn unique_name(library: &Path, base: &str) -> String {
    let mut name = base.to_string();
    let mut n = 2;
    while library.join(&name).exists() {
        name = format!("{base} {n}");
        n += 1;
    }
    name
}

fn check_name(name: &str) -> Result<()> {
    if !valid_name(name) {
        bail!("invalid project name {name:?}: use letters, digits, spaces and _-.()+,&' (no slashes, not starting with a dot)");
    }
    Ok(())
}

fn project_dir(library: &Path, name: &str) -> Result<PathBuf> {
    check_name(name)?;
    let dir = library.join(name);
    if !dir.join(PROJECT_FILE).is_file() {
        bail!("no project named {name:?} in the library");
    }
    Ok(dir)
}

fn millis(t: std::io::Result<std::time::SystemTime>) -> f64 {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

fn stamp() -> String {
    let d = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}", d.as_secs())
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

/// Summary of one project folder (read leniently: a broken project is still listed).
fn info(dir: &Path, current: &Path) -> Option<ProjectInfo> {
    let file = dir.join(PROJECT_FILE);
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let name = dir.file_name()?.to_string_lossy().to_string();
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let count = |k: &str| v.get(k).and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
    let clips = v.pointer("/playlist/clips").and_then(|c| c.as_array());
    let length = clips
        .map(|cs| cs.iter().map(|c| c.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0) + c.get("length").and_then(|x| x.as_f64()).unwrap_or(0.0)).fold(0.0, f64::max))
        .unwrap_or(0.0);
    let invalid = !rosaclef_core::validate::parse_and_validate(&text).is_ok();
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    Some(ProjectInfo {
        title: v.pointer("/meta/title").and_then(|x| x.as_str()).filter(|t| !t.is_empty()).unwrap_or(&name).to_string(),
        bpm: v.pointer("/transport/bpm").and_then(|x| x.as_f64()).unwrap_or(0.0),
        beats_per_bar: v.pointer("/transport/beatsPerBar").and_then(|x| x.as_f64()).unwrap_or(4.0),
        modified: millis(meta.modified()),
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
pub fn list(library: &Path, current: &Path) -> Vec<ProjectInfo> {
    let mut out: Vec<ProjectInfo> = std::fs::read_dir(library)
        .map(|rd| {
            rd.flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .filter(|e| e.path().is_dir())
                .filter_map(|e| info(&e.path(), current))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.modified.total_cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Create a project folder in the library (empty or from the demo song).
pub fn create(library: &Path, name: &str, demo: bool) -> Result<Folder> {
    check_name(name)?;
    let dir = library.join(name);
    if dir.exists() {
        bail!("{name:?} already exists in the library");
    }
    std::fs::create_dir_all(library)?;
    let f = Folder::new(&dir);
    f.init(demo)?;
    let f = Folder::new(&dir);
    if demo {
        // Name the song after its folder, so library cards tell them apart.
        let mut p = f.load()?.project.ok_or_else(|| anyhow!("the demo project is invalid"))?;
        p.meta.title = name.to_string();
        f.write_project(&p)?;
    }
    crate::guide::write(&f)?;
    Ok(f)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n == folder::STATE_DIR || n == TRASH_DIR {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        let ft = e.file_type()?;
        if ft.is_dir() {
            copy_dir(&src, &dst)?;
        } else if ft.is_file() {
            std::fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
        }
    }
    Ok(())
}

/// Copy a project (without its live state) under a new name. The copy's
/// song is titled with that name, so library cards tell the two apart.
pub fn duplicate(library: &Path, name: &str, to: &str) -> Result<()> {
    let src = project_dir(library, name)?;
    check_name(to)?;
    let dst = library.join(to);
    if dst.exists() {
        bail!("{to:?} already exists in the library");
    }
    copy_dir(&src, &dst)?;
    let f = Folder::new(&dst);
    f.init(false)?;
    if let Ok(Some(mut p)) = f.load().map(|c| c.project) {
        p.meta.title = to.to_string();
        f.write_project(&p)?;
    }
    crate::guide::write(&f)?;
    Ok(())
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

/// Move a library entry (a project folder) into the library's trash.
pub fn trash_project(library: &Path, name: &str) -> Result<PathBuf> {
    let src = project_dir(library, name)?;
    let trash = library.join(TRASH_DIR);
    std::fs::create_dir_all(&trash)?;
    let dst = trash.join(format!("{name} {}", stamp()));
    std::fs::rename(&src, &dst).with_context(|| format!("moving {} to the trash", src.display()))?;
    Ok(dst)
}

/// Write an imported project as a new library folder; returns its name.
pub fn save_imported(library: &Path, name: &str, im: &Imported) -> Result<(String, Vec<String>)> {
    let name = unique_name(library, &sanitize_name(name));
    let dir = library.join(&name);
    std::fs::create_dir_all(&dir)?;
    let f = Folder::new(&dir);
    let mut project = im.project.clone();
    project.meta.title = if project.meta.title.is_empty() { name.clone() } else { project.meta.title.clone() };
    f.write_project(&project)?;
    f.init(false)?;
    let mut warnings = im.warnings.clone();
    for s in &im.samples {
        let Some(dst) = f.resolve(&s.to) else { continue };
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Err(e) = std::fs::copy(&s.from, &dst) {
            warnings.push(format!("could not copy {} to {}: {e}", s.from.display(), s.to));
        }
    }
    crate::guide::write(&f)?;
    Ok((name, warnings))
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
    let mut out: HashSet<String> = p.playlist.clips.iter().filter(|c| !c.sample.is_empty()).map(|c| c.sample.clone()).collect();
    for ch in &p.channels {
        if let Some(s) = ch.instrument.options.get("sample").filter(|s| !s.is_empty()) {
            out.insert(s.clone());
        }
    }
    out
}

/// Non-hidden files of the project (under `sub`, recursively).
pub fn files(f: &Folder, sub: &str, project: &Project) -> Result<Vec<FileInfo>> {
    let root = if sub.is_empty() { f.dir.clone() } else { f.resolve(sub).ok_or_else(|| anyhow!("invalid folder {sub:?}"))? };
    let used = referenced(project);
    let mut out = vec![];
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with('.') || out.len() > 10_000 {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                if depth < 8 {
                    walk(&p, depth + 1, out);
                }
            } else {
                out.push(p);
            }
        }
    }
    let mut paths = vec![];
    walk(&root, 0, &mut paths);
    for p in paths {
        let Ok(rel) = p.strip_prefix(&f.dir) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let meta = std::fs::metadata(&p).ok();
        let (dir, name) = match rel.rfind('/') {
            Some(i) => (rel[..i].to_string(), rel[i + 1..].to_string()),
            None => (String::new(), rel.clone()),
        };
        out.push(FileInfo {
            kind: if crate::decode::is_audio_file(&p) { "audio".into() } else { "other".into() },
            used: used.contains(&rel),
            managed: dir.is_empty() && PROTECTED.contains(&name.as_str()),
            size: meta.as_ref().map(|m| m.len() as f64).unwrap_or(0.0),
            modified: millis(meta.map(|m| m.modified()).unwrap_or_else(|| Err(std::io::Error::other("no metadata")))),
            path: rel,
            name,
            dir,
        });
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

fn existing_file(f: &Folder, rel: &str) -> Result<PathBuf> {
    let p = f.resolve(rel).ok_or_else(|| anyhow!("invalid path {rel:?}"))?;
    if rel.split('/').any(|s| s.starts_with('.')) {
        bail!("hidden files cannot be changed here");
    }
    if !p.is_file() {
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
    if to.is_empty() || to.len() > 128 || to.starts_with('.') || to.contains(['/', '\\']) || to.chars().any(|c| c.is_control()) {
        bail!("invalid file name {to:?}");
    }
    let ext = Path::new(rel).extension().map(|e| e.to_string_lossy().to_string());
    let to = match (Path::new(to).extension(), ext) {
        (None, Some(e)) => format!("{to}.{e}"),
        _ => to.to_string(),
    };
    let new_rel = match rel.rfind('/') {
        Some(i) => format!("{}/{to}", &rel[..i]),
        None => to.clone(),
    };
    let dst = f.resolve(&new_rel).ok_or_else(|| anyhow!("invalid file name"))?;
    if dst.exists() {
        bail!("{new_rel} already exists");
    }
    std::fs::rename(&src, &dst)?;
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
    std::fs::create_dir_all(&trash)?;
    let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut dst_name = format!("{} {name}", stamp());
    let mut n = 2;
    while trash.join(&dst_name).exists() {
        dst_name = format!("{} {n} {name}", stamp());
        n += 1;
    }
    std::fs::rename(&src, trash.join(&dst_name))?;
    Ok(format!("{TRASH_DIR}/{dst_name}"))
}

// ------------------------------------------------------------------ HTTP

fn bad(e: impl std::fmt::Display) -> Response {
    (StatusCode::BAD_REQUEST, e.to_string()).into_response()
}

fn conflict(e: impl std::fmt::Display) -> Response {
    (StatusCode::CONFLICT, e.to_string()).into_response()
}

/// Run blocking file work off the async runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| anyhow!("task failed: {e}"))?
}

fn current_dir(app: &Shared) -> PathBuf {
    app.folder().dir
}

async fn list_projects(State(app): State<Shared>) -> Response {
    let (library, current) = (app.library.clone(), current_dir(&app));
    let res = blocking(move || Ok(list(&library, &current))).await;
    match res {
        Ok(projects) => {
            let cur = current_dir(&app);
            Json(json!({
                "library": app.library.display().to_string(),
                "current": cur.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                "currentFolder": cur.display().to_string(),
                "projects": projects,
            }))
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateReq {
    name: String,
    #[serde(default)]
    demo: bool,
}

async fn create_project(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<CreateReq>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = req.name.trim().to_string();
    match blocking(move || create(&library, &name, req.demo).map(|f| f.dir.file_name().unwrap_or_default().to_string_lossy().to_string())).await {
        Ok(name) => Json(json!({"name": name})).into_response(),
        Err(e) if e.to_string().contains("already exists") => conflict(e),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct NameReq {
    name: String,
}

async fn open_project(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<NameReq>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let dir = match project_dir(&app.library, &req.name) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let a = app.clone();
    let res = blocking(move || {
        let target = Folder::new(&dir);
        if target.dir == a.folder().dir {
            return Ok(false);
        }
        a.switch_to(target).map(|_| true)
    })
    .await;
    match res {
        Ok(switched) => Json(json!({"ok": true, "name": req.name, "switched": switched})).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct RenameReq {
    name: String,
    to: String,
}

async fn duplicate_project(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<RenameReq>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let (name, to) = (req.name.clone(), req.to.trim().to_string());
    let to2 = to.clone();
    match blocking(move || duplicate(&library, &name, &to2)).await {
        Ok(()) => Json(json!({"name": to})).into_response(),
        Err(e) if e.to_string().contains("already exists") => conflict(e),
        Err(e) => bad(e),
    }
}

async fn rename_project(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<RenameReq>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    let (name, to) = (req.name.clone(), req.to.trim().to_string());
    let to2 = to.clone();
    let res = blocking(move || {
        let src = project_dir(&a.library, &name)?;
        check_name(&to2)?;
        let dst = a.library.join(&to2);
        if dst.exists() {
            bail!("{to2:?} already exists in the library");
        }
        let src_canon = src.canonicalize().unwrap_or(src.clone());
        if src_canon == a.folder().dir {
            // The open project: move it and follow it (agent, watcher, clients).
            a.rename_open(&dst)?;
            let mut p = a.project();
            if p.meta.title == name {
                p.meta.title = to2.clone();
                let issues = rosaclef_core::validate::validate(&p);
                a.apply(p, issues, "files", 0, None)?;
            }
            Ok(())
        } else {
            std::fs::rename(&src, &dst)?;
            follow_title(&Folder::new(&dst), &name, &to2)
        }
    })
    .await;
    match res {
        Ok(()) => Json(json!({"name": to})).into_response(),
        Err(e) if e.to_string().contains("already exists") => conflict(e),
        Err(e) => bad(e),
    }
}

async fn delete_project(State(app): State<Shared>, headers: HeaderMap, UrlPath(name): UrlPath<String>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    let res = blocking(move || {
        let dir = project_dir(&a.library, &name)?;
        if dir.canonicalize().unwrap_or(dir.clone()) == a.folder().dir {
            bail!("{name:?} is open; open another project before deleting it");
        }
        trash_project(&a.library, &name)
    })
    .await;
    match res {
        Ok(p) => Json(json!({"trashed": p.display().to_string()})).into_response(),
        Err(e) if e.to_string().contains("is open") => conflict(e),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct ImportQuery {
    /// Name of the new project (default: from `filename`).
    name: Option<String>,
    /// Original file name, for the default project name.
    filename: Option<String>,
    /// `current`: merge into the open project instead (MIDI only).
    into: Option<String>,
}

impl ImportQuery {
    fn project_name(&self, fallback: &str) -> String {
        if let Some(n) = self.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            return n.to_string();
        }
        let stem = self.filename.as_deref().and_then(|f| Path::new(f).file_stem()).map(|s| s.to_string_lossy().to_string());
        stem.unwrap_or_else(|| fallback.to_string())
    }
}

fn import_reply(res: Result<(String, Vec<String>)>) -> Response {
    match res {
        Ok((name, warnings)) => Json(json!({"name": name, "warnings": warnings})).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")).into_response(),
    }
}

async fn import_lmms(State(app): State<Shared>, headers: HeaderMap, Query(q): Query<ImportQuery>, body: Bytes) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = sanitize_name(&q.project_name("LMMS import"));
    import_reply(
        blocking(move || {
            let im = rosaclef_import::lmms::import(&body, &rosaclef_import::lmms::Options::new(&name))?;
            save_imported(&library, &name, &im)
        })
        .await,
    )
}

async fn import_midi(State(app): State<Shared>, headers: HeaderMap, Query(q): Query<ImportQuery>, body: Bytes) -> Response {
    if q.into.as_deref() == Some("current") {
        return import_midi_into(State(app), headers, Query(q), body).await;
    }
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = sanitize_name(&q.project_name("MIDI import"));
    import_reply(
        blocking(move || {
            let im = rosaclef_import::midi::import(&body, &rosaclef_import::midi::Options::new(&name))?;
            save_imported(&library, &name, &im)
        })
        .await,
    )
}

/// `POST /api/import-midi?into=current`: add a MIDI file's parts to the
/// open song as new channels, patterns, tracks and inserts (one undo step).
async fn import_midi_into(State(app): State<Shared>, headers: HeaderMap, Query(q): Query<ImportQuery>, body: Bytes) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    if q.into.as_deref().unwrap_or("current") != "current" {
        return bad("only into=current is supported");
    }
    let a = app.clone();
    let res = blocking(move || {
        let im = rosaclef_import::midi::import(&body, &rosaclef_import::midi::Options::new("import"))?;
        let mut project = a.project();
        let before = project.channels.len();
        let mut warnings = im.warnings;
        warnings.extend(rosaclef_import::merge_into(&mut project, im.project));
        let issues = rosaclef_core::validate::validate(&project);
        if issues.iter().any(|i| i.severity == rosaclef_core::validate::Severity::Error) {
            let msgs: Vec<String> = issues.iter().take(3).map(|i| i.to_string()).collect();
            bail!("the merged project would be invalid:\n{}", msgs.join("\n"));
        }
        let added = project.channels.len() - before;
        a.apply(project, issues, "import", 0, None)?;
        Ok((added, warnings))
    })
    .await;
    match res {
        Ok((channels, warnings)) => Json(json!({"channels": channels, "warnings": warnings})).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")).into_response(),
    }
}

#[derive(Deserialize)]
struct FilesQuery {
    dir: Option<String>,
}

async fn list_files(State(app): State<Shared>, Query(q): Query<FilesQuery>) -> Response {
    let (f, project) = (app.folder(), app.project());
    let res = blocking(move || files(&f, q.dir.as_deref().unwrap_or("").trim_matches('/'), &project)).await;
    match res {
        Ok(list) => Json(json!({"folder": app.folder().dir.display().to_string(), "files": list})).into_response(),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct FileRenameReq {
    path: String,
    to: String,
}

async fn rename_file(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<FileRenameReq>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    let res = blocking(move || {
        let _guard = a.switching.lock();
        let new_rel = rename_path(&a.folder(), &req.path, &req.to)?;
        // Keep the song pointing at the file.
        let mut project = a.project();
        let n = rewrite_refs(&mut project, &req.path, &new_rel);
        if n > 0 {
            let issues = rosaclef_core::validate::validate(&project);
            a.apply(project, issues, "files", 0, None)?;
        }
        Ok((new_rel, n))
    })
    .await;
    match res {
        Ok((path, n)) => Json(json!({"path": path, "references": n})).into_response(),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn delete_file(State(app): State<Shared>, headers: HeaderMap, Query(q): Query<PathQuery>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    match blocking(move || trash_path(&a.folder(), &q.path)).await {
        Ok(p) => Json(json!({"trashed": p})).into_response(),
        Err(e) => bad(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rosaclef-library-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names() {
        for ok in ["song", "My Song 2", "Été (live)", "a.b"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", " x", "x ", ".hidden", "..", "../x", "a/b", "a\\b", "x.", "a\0b", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert_eq!(sanitize_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(sanitize_name("My Track.mmpz"), "My Track-mmpz");
        assert_eq!(sanitize_name("..."), "Imported");
        assert!(valid_name(&sanitize_name("weird/\\:*?name")));
    }

    #[test]
    fn library_lifecycle() {
        let lib = scratch("life");
        let a = create(&lib, "Alpha", false).unwrap();
        create(&lib, "Beta", true).unwrap();
        assert!(create(&lib, "Alpha", false).is_err());
        assert!(create(&lib, "../escape", false).is_err());
        let list1 = list(&lib, &a.dir);
        let names: HashSet<String> = list1.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, ["Alpha".to_string(), "Beta".to_string()].into_iter().collect());
        let alpha = list1.iter().find(|p| p.name == "Alpha").unwrap();
        assert!(alpha.current && !alpha.invalid);
        assert_eq!(alpha.bpm, 120.0);
        let beta = list1.iter().find(|p| p.name == "Beta").unwrap();
        assert_eq!(beta.title, "Beta");
        assert!(beta.channels > 0 && beta.clips > 0);

        std::fs::create_dir_all(a.dir.join(".rosaclef")).unwrap();
        std::fs::write(a.dir.join("samples/x.wav"), b"x").unwrap();
        duplicate(&lib, "Alpha", "Gamma").unwrap();
        assert!(lib.join("Gamma/samples/x.wav").is_file());
        assert!(lib.join("Gamma/AGENTS.md").is_file());
        let title = |n: &str| Folder::new(lib.join(n)).load().unwrap().project.unwrap().meta.title;
        assert_eq!(title("Gamma"), "Gamma", "the copy is titled with its own name");
        assert!(duplicate(&lib, "Alpha", "Beta").is_err());
        // Even when the song has a title of its own, the copy is told apart.
        let bf = Folder::new(lib.join("Beta"));
        let mut bp = bf.load().unwrap().project.unwrap();
        bp.meta.title = "Velvet Hour".into();
        bf.write_project(&bp).unwrap();
        duplicate(&lib, "Beta", "Beta copy").unwrap();
        assert_eq!(title("Beta"), "Velvet Hour");
        assert_eq!(title("Beta copy"), "Beta copy");
        trash_project(&lib, "Beta copy").unwrap();

        let trashed = trash_project(&lib, "Gamma").unwrap();
        assert!(trashed.starts_with(lib.join(TRASH_DIR)));
        assert!(!lib.join("Gamma").exists());
        assert_eq!(list(&lib, &a.dir).len(), 2, "the trash is not listed");
        assert_eq!(unique_name(&lib, "Alpha"), "Alpha 2");
    }

    #[test]
    fn file_manager() {
        let lib = scratch("files");
        let f = create(&lib, "Song", false).unwrap();
        std::fs::write(f.dir.join("samples/kick.wav"), b"RIFF").unwrap();
        std::fs::write(f.dir.join("renders/mix.wav"), b"RIFF").unwrap();
        std::fs::write(f.dir.join("notes.txt"), b"hi").unwrap();
        let mut p = f.load().unwrap().project.unwrap();
        let mut dev = rosaclef_core::Device::new("sampler");
        dev.options.insert("sample".into(), "samples/kick.wav".into());
        p.channels.push(rosaclef_core::Channel { id: "k".into(), name: "K".into(), color: "#ffffff".into(), instrument: dev, volume: 0.8, pan: 0.0, mute: false, mixer: rosaclef_core::InsertIx(0) });

        let list = files(&f, "", &p).unwrap();
        let find = |path: &str| list.iter().find(|x| x.path == path);
        assert!(find(".rosaclef/status.json").is_none(), "hidden files are not listed");
        let kick = find("samples/kick.wav").unwrap();
        assert!(kick.used && kick.kind == "audio" && kick.dir == "samples" && kick.size == 4.0);
        assert!(find("project.json").unwrap().managed);
        assert_eq!(find("notes.txt").unwrap().kind, "other");
        assert_eq!(files(&f, "renders", &p).unwrap().len(), 1);
        assert!(files(&f, "../", &p).is_err());

        assert_eq!(rename_path(&f, "samples/kick.wav", "boom").unwrap(), "samples/boom.wav");
        assert_eq!(rewrite_refs(&mut p, "samples/kick.wav", "samples/boom.wav"), 1);
        assert_eq!(p.channels[0].instrument.options["sample"], "samples/boom.wav");
        assert!(rename_path(&f, "project.json", "x.json").is_err());
        assert!(rename_path(&f, "samples/boom.wav", "../x.wav").is_err());
        assert!(rename_path(&f, "../Song/project.json", "x").is_err());

        let t = trash_path(&f, "renders/mix.wav").unwrap();
        assert!(t.starts_with(".trash/") && f.dir.join(&t).is_file());
        assert!(trash_path(&f, "project.json").is_err());
        assert!(trash_path(&f, ".rosaclef/status.json").is_err());
    }
}
