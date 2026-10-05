//! The studio server, in the browser.
//!
//! [`Host`] answers the same requests (`/api/...`, `/files/...`) and speaks
//! the same WebSocket protocol (`/ws`, `/ws/term`) as the native server
//! (`crates/server`), on a [`MemFs`] that the page's worker persists to
//! IndexedDB. The UI cannot tell the two apart, except that there is no
//! native audio device, no CLAP plugins, and the terminal runs the built-in
//! `rosaclef` shell ([`crate::shell`]) instead of a coding agent.

use crate::shell::Shell;
use anyhow::{anyhow, bail, Result};
use rosaclef_core::validate::{self, Issue, Severity};
use rosaclef_core::{format, Project};
use rosaclef_engine::render::{encode_wav, RenderScope};
use rosaclef_fs::{Fs, MemFs, SharedFs};
use rosaclef_studio::fonts::{FontFiles, Fonts};
use rosaclef_studio::library::{self, rewrite_refs, unique_sample_path, Library};
use rosaclef_studio::render::{levels_db, render_project, required_presets, required_samples};
use rosaclef_studio::{archive, decode, folder, slug, Folder};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

/// Where the library lives in the in-memory tree.
pub const LIBRARY: &str = "/library";
/// Remembers which project is open.
const STATE_FILE: &str = "/state.json";
/// What the agent guides call the command line (the in-browser shell).
pub const EXE: &str = "rosaclef";

/// A message for connected pages.
#[derive(Clone, Debug)]
pub struct Out {
    /// `None`: every client (except `exclude`).
    pub to: Option<u64>,
    pub exclude: u64,
    /// `ws` (project sync) or `term` (the terminal).
    pub chan: &'static str,
    /// JSON text, or terminal output.
    pub text: String,
    /// Terminal output (written as bytes), not a JSON message.
    pub raw: bool,
}

/// An HTTP-like response.
#[derive(Debug, Default)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// The body is this stored blob (served by the worker from storage).
    pub blob: Option<u64>,
}

impl Response {
    fn json(v: Value) -> Response {
        Response {
            status: 200,
            content_type: "application/json".into(),
            body: v.to_string().into_bytes(),
            blob: None,
        }
    }
    fn text(status: u16, s: impl Into<String>) -> Response {
        Response {
            status,
            content_type: "text/plain; charset=utf-8".into(),
            body: s.into().into_bytes(),
            blob: None,
        }
    }
    fn bytes(content_type: &str, body: Vec<u8>) -> Response {
        Response {
            status: 200,
            content_type: content_type.into(),
            body,
            blob: None,
        }
    }
}

/// A request failed because file contents must be loaded first.
#[derive(Debug)]
pub struct NeedContent;

impl std::fmt::Display for NeedContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "file contents are still loading")
    }
}

impl std::error::Error for NeedContent {}

struct Doc {
    project: Project,
    rev: u64,
    issues: Vec<Issue>,
}

/// Soundfont files, fetched by the worker from the site (`soundfonts/NAME`)
/// when a render needs them: a read of a missing file records it and fails
/// with [`NeedContent`], and the call is retried once the worker provides it.
#[derive(Default)]
pub struct MemFonts {
    files: std::sync::Mutex<HashMap<String, Arc<Vec<u8>>>>,
    missing: std::sync::Mutex<Vec<String>>,
}

impl MemFonts {
    pub fn provide(&self, name: &str, data: Vec<u8>) {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), Arc::new(data));
    }

    pub fn take_missing(&self) -> Vec<String> {
        std::mem::take(&mut *self.missing.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drop the sample pieces (decoded presets are cached by [`Fonts`]).
    pub fn forget_pieces(&self) {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|k, _| !k.ends_with(".bin"));
    }
}

impl FontFiles for MemFonts {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        if let Some(b) = self
            .files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return Ok(b.as_ref().clone());
        }
        let mut m = self.missing.lock().unwrap_or_else(|e| e.into_inner());
        if !m.iter().any(|n| n == name) {
            m.push(name.to_string());
        }
        Err(NeedContent.into())
    }
}

pub struct Host {
    pub mem: Arc<MemFs>,
    pub font_files: Arc<MemFonts>,
    pub fonts: Arc<Fonts>,
    pub library: Library,
    pub folder: Folder,
    doc: Doc,
    /// The producer's context (what `.rosaclef/context.json` holds natively).
    pub context: Value,
    ws_clients: Vec<u64>,
    pub(crate) shells: HashMap<u64, Shell>,
    peaks: HashMap<(u64, usize), Value>,
    pub out: Vec<Out>,
}

fn err_status(e: &anyhow::Error) -> u16 {
    if e.to_string().contains("already exists") {
        409
    } else {
        400
    }
}

// ------------------------------------------------------------------ URLs

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = if b[i] == b'%' && i + 2 < b.len() {
            std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        } else {
            None
        };
        match (b[i], hex) {
            (b'+', _) => out.push(b' '),
            (_, Some(v)) => {
                out.push(v);
                i += 2;
            }
            (c, None) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Split `/path?a=1&b=2` into a decoded path and query parameters.
pub fn parse_url(url: &str) -> (String, BTreeMap<String, String>) {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let q = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    // The path's own `+` is literal.
    (percent_decode(&path.replace('+', "%2B")), q)
}

fn body_json(body: &[u8]) -> Result<Value> {
    serde_json::from_slice(body).map_err(|e| anyhow!("invalid JSON body: {e}"))
}

fn str_field(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

impl Host {
    /// A host on `mem`, with the library under [`LIBRARY`]. Opens the last
    /// open project; on first use, creates one from the demo song.
    pub fn start(mem: Arc<MemFs>) -> Result<Host> {
        let fs: SharedFs = mem.clone();
        let library = Library::new(fs.clone(), LIBRARY, EXE);
        fs.create_dir_all(Path::new(LIBRARY))?;
        let remembered = fs
            .read_to_string(Path::new(STATE_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| str_field(&v, "current"))
            .unwrap_or_default();
        let listed = library.list(Path::new(""));
        let pick = listed
            .iter()
            .find(|p| p.name == remembered && !p.invalid)
            .or_else(|| listed.iter().find(|p| !p.invalid))
            .map(|p| p.name.clone());
        let name = match pick {
            Some(n) => n,
            None => library
                .create(&library.unique_name(&folder::demo_title()), true)?
                .name(),
        };
        let folder = library.folder(&name);
        let (project, issues) = Self::load(&folder)?;
        let font_files = Arc::new(MemFonts::default());
        let host = Host {
            mem,
            fonts: Arc::new(Fonts::new(font_files.clone())),
            font_files,
            library,
            folder,
            doc: Doc {
                project,
                rev: 1,
                issues,
            },
            context: Value::Null,
            ws_clients: vec![],
            shells: HashMap::new(),
            peaks: HashMap::new(),
            out: vec![],
        };
        host.remember()?;
        Ok(host)
    }

    fn fs(&self) -> &SharedFs {
        &self.library.fs
    }

    fn load(folder: &Folder) -> Result<(Project, Vec<Issue>)> {
        folder.init(false)?;
        rosaclef_studio::guide::write(folder, EXE)?;
        let checked = folder.load()?;
        if !checked.is_ok() {
            let msgs: Vec<String> = checked.errors().take(3).map(|i| i.to_string()).collect();
            bail!(
                "the project is invalid and cannot be opened:\n{}",
                msgs.join("\n")
            );
        }
        Ok((checked.project.expect("checked"), checked.issues))
    }

    fn remember(&self) -> Result<()> {
        let v = json!({"current": self.folder.name()});
        Ok(self
            .fs()
            .write_if_changed(Path::new(STATE_FILE), &(v.to_string() + "\n"))?)
    }

    pub fn project(&self) -> &Project {
        &self.doc.project
    }

    // ------------------------------------------------------------ messages

    fn send(&mut self, to: Option<u64>, exclude: u64, chan: &'static str, v: Value) {
        self.out.push(Out {
            to,
            exclude,
            chan,
            text: v.to_string(),
            raw: false,
        });
    }

    pub(crate) fn term_out(&mut self, client: u64, text: String) {
        self.out.push(Out {
            to: Some(client),
            exclude: 0,
            chan: "term",
            text,
            raw: true,
        });
    }

    pub(crate) fn term_json(&mut self, client: u64, v: Value) {
        self.send(Some(client), 0, "term", v);
    }

    fn welcome(&self, t: &str, client: u64) -> Value {
        json!({
            "t": t,
            "client": client,
            "rev": self.doc.rev,
            "project": self.doc.project,
            "issues": self.doc.issues,
            "folder": self.folder.dir.display().to_string(),
            "name": self.folder.name(),
            "samples": self.folder.list_samples(),
            "native": {"available": false, "enabled": false},
            "backend": "local",
        })
    }

    pub fn ws_open(&mut self, client: u64) {
        if !self.ws_clients.contains(&client) {
            self.ws_clients.push(client);
        }
        let w = self.welcome("welcome", client);
        self.send(Some(client), 0, "ws", w);
    }

    pub fn close(&mut self, client: u64) {
        self.ws_clients.retain(|c| *c != client);
        self.shells.remove(&client);
    }

    pub fn ws_message(&mut self, client: u64, text: &str) {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match v.get("t").and_then(|t| t.as_str()).unwrap_or("") {
            "put" => {
                let checked =
                    validate::value_and_validate(v.get("project").cloned().unwrap_or(Value::Null));
                if !checked.is_ok() {
                    self.send(
                        Some(client),
                        0,
                        "ws",
                        json!({"t": "rejected", "issues": checked.issues}),
                    );
                    return;
                }
                // An edit made for a project that has since been closed is dropped.
                if let Some(f) = v.get("folder").and_then(|f| f.as_str()) {
                    if f != self.folder.dir.display().to_string() {
                        return;
                    }
                }
                match self.apply(checked.project.unwrap(), checked.issues, "ui", client) {
                    Ok(rev) => self.send(Some(client), 0, "ws", json!({"t": "ack", "rev": rev})),
                    Err(e) => self.send(
                        Some(client),
                        0,
                        "ws",
                        json!({"t": "error", "message": e.to_string()}),
                    ),
                }
            }
            "context" => {
                let mut ctx = rosaclef_core::context::normalize(
                    v.get("context").cloned().unwrap_or(Value::Null),
                );
                ctx.updated_at = rosaclef_studio::rfc3339(self.fs().now_ms());
                self.context = serde_json::to_value(&ctx).unwrap_or(Value::Null);
            }
            t if t.starts_with("native.") => {
                self.send(Some(client), 0, "ws", json!({"t": "error", "message": "the Studio output needs the native Rosaclef server; this is the browser-only studio"}));
            }
            _ => {}
        }
    }

    /// Accept a new version of the song and tell every other page.
    pub fn apply(
        &mut self,
        project: Project,
        issues: Vec<Issue>,
        origin: &str,
        exclude: u64,
    ) -> Result<u64> {
        self.folder.write_project(&project)?;
        self.doc.rev += 1;
        self.doc.project = project;
        self.doc.issues = issues;
        let v = json!({"t": "project", "rev": self.doc.rev, "origin": origin, "project": self.doc.project, "issues": self.doc.issues});
        self.send(None, exclude, "ws", v);
        Ok(self.doc.rev)
    }

    /// Open another project of the library; every page reloads it.
    pub fn switch_to(&mut self, name: &str) -> Result<bool> {
        let dir = self.library.project_dir(name)?;
        let target = Folder::new(self.fs().clone(), dir);
        if target.dir == self.folder.dir {
            return Ok(false);
        }
        let (project, issues) = Self::load(&target)?;
        self.folder = target;
        self.doc = Doc {
            project,
            rev: 1,
            issues,
        };
        self.remember()?;
        let w = self.welcome("switched", 0);
        self.send(None, 0, "ws", w);
        Ok(true)
    }

    /// Fail with [`NeedContent`] unless the contents of these project files
    /// are in memory (reads record what is missing for the worker to load).
    pub fn ensure_loaded(&self, paths: &[String]) -> Result<()> {
        let mut missing = false;
        for rel in paths {
            if let Some(p) = self.folder.resolve(rel) {
                if let Some((_, false)) = self.mem.blob(&p) {
                    let _ = self.mem.read(&p); // records the blob
                    missing = true;
                }
            }
        }
        if missing {
            return Err(NeedContent.into());
        }
        Ok(())
    }

    // ------------------------------------------------------------ requests

    pub fn request(&mut self, client: u64, method: &str, url: &str, body: &[u8]) -> Response {
        let samples_before = self.folder.list_samples();
        let folder_before = self.folder.dir.clone();
        let (path, q) = parse_url(url);
        let res = self.route(client, method, &path, &q, body);
        if self.folder.dir == folder_before {
            let samples = self.folder.list_samples();
            if samples != samples_before {
                self.send(None, 0, "ws", json!({"t": "samples", "samples": samples}));
            }
        }
        match res {
            Ok(r) => r,
            Err(e) if e.is::<NeedContent>() => Response::text(503, e.to_string()),
            Err(e) => Response::text(err_status(&e), format!("{e:#}")),
        }
    }

    fn route(
        &mut self,
        client: u64,
        method: &str,
        path: &str,
        q: &BTreeMap<String, String>,
        body: &[u8],
    ) -> Result<Response> {
        let qs = |k: &str| q.get(k).cloned().unwrap_or_default();
        if let Some(rel) = path.strip_prefix("/files/") {
            return self.serve_file(rel);
        }
        Ok(match (method, path) {
            ("GET", "/api/info") => Response::json(json!({
                "folder": self.folder.dir.display().to_string(),
                "library": LIBRARY,
                "url": "",
                "version": env!("CARGO_PKG_VERSION"),
                "native": {"available": false, "enabled": false},
                "backend": "local",
            })),
            ("GET", "/api/project") => Response::bytes(
                "application/json",
                format::to_string(&self.doc.project).into_bytes(),
            ),
            ("PUT", "/api/project") => {
                let checked = validate::parse_and_validate(&String::from_utf8_lossy(body));
                if !checked.is_ok() {
                    let mut r = Response::json(json!({"ok": false, "issues": checked.issues}));
                    r.status = 422;
                    return Ok(r);
                }
                let rev = self.apply(checked.project.unwrap(), checked.issues, "api", 0)?;
                Response::json(json!({"ok": true, "rev": rev}))
            }
            ("GET", "/api/schema") => Response::bytes(
                "application/json",
                rosaclef_core::schema::schema_text().into_bytes(),
            ),
            ("GET", "/api/catalog") => Response::json(
                json!({"devices": rosaclef_core::catalog::DEVICES, "presets": rosaclef_core::presets::all(), "plugins": [], "arp": rosaclef_core::arp::catalog(), "collections": [rosaclef_core::gm::collection()]}),
            ),
            ("GET", "/api/plugins") => Response::json(json!({"plugins": []})),
            ("GET", "/api/plugins/params") => {
                Response::text(400, "CLAP plugins run in the native Rosaclef studio only")
            }
            ("GET", "/api/agents") => Response::json(json!({"agents": [{
                "id": "shell",
                "name": "Rosaclef shell",
                "command": ["rosaclef"],
                "available": true,
                "hint": "",
            }]})),
            ("GET", "/api/samples") => {
                Response::json(json!({"samples": self.folder.list_samples()}))
            }
            ("POST", "/api/samples") => {
                let rel = unique_sample_path(&self.folder, &qs("name"));
                self.folder.write(&rel, body)?;
                Response::json(json!({"path": rel}))
            }
            ("GET", "/api/peaks") => self.peaks(&qs("path"), qs("n").parse().unwrap_or(1024))?,
            ("GET", "/api/transcribe") => self.transcribe(&qs("path"), &qs("mode"))?,
            ("GET", "/api/grooves") => Response::json(rosaclef_core::drums::catalog()),
            ("POST", "/api/drums") => {
                match rosaclef_core::drums::api_write(
                    &String::from_utf8_lossy(body),
                    qs("guess") == "true",
                    qs("write") != "false",
                ) {
                    Ok(v) => Response::json(v),
                    Err(e) => Response::text(422, e),
                }
            }
            ("POST", "/api/drums/pattern") => {
                match rosaclef_core::drums::api_pattern(&String::from_utf8_lossy(body), &qs("id")) {
                    Ok(v) => Response::json(v),
                    Err(e) => Response::text(422, e),
                }
            }
            ("GET", "/api/critic") => Response::json(rosaclef_core::critic::catalog()),
            ("POST", "/api/critic") => {
                match rosaclef_core::critic::api(&String::from_utf8_lossy(body)) {
                    Ok(v) => Response::json(v),
                    Err(e) => Response::text(422, e),
                }
            }
            ("POST", "/api/render") => {
                let v = body_json(body)?;
                let r = self.render(
                    v.get("pattern").and_then(|p| p.as_str()).unwrap_or(""),
                    v.get("loops").and_then(|x| x.as_u64()).unwrap_or(1) as u32,
                    v.get("bits").and_then(|x| x.as_u64()).unwrap_or(24) as u16,
                    v.get("sampleRate")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(48000) as u32,
                    None,
                )?;
                Response::json(r)
            }
            ("GET", "/api/projects") => Response::json(json!({
                "library": "Browser storage",
                "current": self.folder.name(),
                "currentFolder": self.folder.dir.display().to_string(),
                "projects": self.library.list(&self.folder.dir),
                "demoTitle": folder::demo_title(),
            })),
            ("POST", "/api/projects") => {
                let v = body_json(body)?;
                let f = self.library.create(
                    str_field(&v, "name").trim(),
                    v.get("demo").and_then(|d| d.as_bool()).unwrap_or(false),
                )?;
                Response::json(json!({"name": f.name()}))
            }
            ("POST", "/api/projects/open") => {
                let name = str_field(&body_json(body)?, "name");
                let switched = self.switch_to(&name)?;
                Response::json(json!({"ok": true, "name": name, "switched": switched}))
            }
            ("POST", "/api/projects/duplicate") => {
                let v = body_json(body)?;
                let to = str_field(&v, "to").trim().to_string();
                self.library.duplicate(&str_field(&v, "name"), &to)?;
                Response::json(json!({"name": to}))
            }
            ("POST", "/api/projects/rename") => {
                let v = body_json(body)?;
                let (name, to) = (
                    str_field(&v, "name"),
                    str_field(&v, "to").trim().to_string(),
                );
                self.rename_project(&name, &to)?;
                Response::json(json!({"name": to}))
            }
            ("POST", "/api/projects/import-lmms") | ("POST", "/api/projects/import-midi") => {
                let lmms = path.ends_with("lmms");
                let fallback = if lmms { "LMMS import" } else { "MIDI import" };
                if !lmms && qs("into") == "current" {
                    return self.import_midi_into(body);
                }
                let name = library::sanitize_name(&import_name(q, fallback));
                let im = if lmms {
                    rosaclef_import::lmms::import(
                        body,
                        &rosaclef_import::lmms::Options::new(&name),
                    )?
                } else {
                    rosaclef_import::midi::import(
                        body,
                        &rosaclef_import::midi::Options::new(&name),
                    )?
                };
                let (name, warnings) = self.library.save_imported(&name, &im)?;
                Response::json(json!({"name": name, "warnings": warnings}))
            }
            ("POST", "/api/projects/import-zip") => {
                let (name, warnings) = archive::import(&self.library, &qs("name"), body)?;
                Response::json(json!({"name": name, "warnings": warnings}))
            }
            ("GET", "/api/projects/export") => {
                let f = match qs("name").as_str() {
                    "" => self.folder.clone(),
                    n => Folder::new(self.fs().clone(), self.library.project_dir(n)?),
                };
                self.export(&f)?
            }
            ("DELETE", p) if p.starts_with("/api/projects/") => {
                let name = &p["/api/projects/".len()..];
                let dir = self.library.project_dir(name)?;
                if dir == self.folder.dir {
                    return Ok(Response::text(
                        409,
                        format!("{name:?} is open; open another project before deleting it"),
                    ));
                }
                let trashed = self.library.trash(name)?;
                Response::json(json!({"trashed": trashed.display().to_string()}))
            }
            ("POST", "/api/import-midi") => self.import_midi_into(body)?,
            ("GET", "/api/files") => Response::json(json!({
                "folder": self.folder.dir.display().to_string(),
                "files": library::files(&self.folder, qs("dir").trim_matches('/'), &self.doc.project)?,
            })),
            ("DELETE", "/api/files") => {
                Response::json(json!({"trashed": library::trash_path(&self.folder, &qs("path"))?}))
            }
            ("POST", "/api/files/rename") => {
                let v = body_json(body)?;
                let from = str_field(&v, "path");
                let new_rel = library::rename_path(&self.folder, &from, &str_field(&v, "to"))?;
                let mut project = self.doc.project.clone();
                let n = rewrite_refs(&mut project, &from, &new_rel);
                if n > 0 {
                    let issues = validate::validate(&project);
                    self.apply(project, issues, "files", 0)?;
                }
                Response::json(json!({"path": new_rel, "references": n}))
            }
            ("POST", "/api/trash/empty") => {
                let n = match str_field(&body_json(body)?, "scope").as_str() {
                    "library" => self.library.empty_trash()?,
                    "project" => library::empty_trash(&self.folder)?,
                    s => bail!("unknown trash {s:?} (library or project)"),
                };
                Response::json(json!({"removed": n}))
            }
            _ => {
                let _ = client;
                Response::text(
                    404,
                    format!("{method} {path}: not available in the browser studio"),
                )
            }
        })
    }

    fn serve_file(&self, rel: &str) -> Result<Response> {
        let p = self
            .folder
            .resolve(rel)
            .ok_or_else(|| anyhow!("invalid path"))?;
        let content_type = content_type(&p);
        match self.mem.blob(&p) {
            Some((_, true)) => Ok(Response::bytes(content_type, self.fs().read(&p)?)),
            Some((blob, false)) => Ok(Response {
                status: 200,
                content_type: content_type.into(),
                body: vec![],
                blob: Some(blob),
            }),
            None => Ok(Response::text(404, format!("{rel} does not exist"))),
        }
    }

    fn peaks(&mut self, rel: &str, n: usize) -> Result<Response> {
        let p = self
            .folder
            .resolve(rel)
            .ok_or_else(|| anyhow!("invalid path"))?;
        let Some((blob, _)) = self.mem.blob(&p) else {
            return Ok(Response::text(404, format!("{rel} does not exist")));
        };
        if let Some(v) = self.peaks.get(&(blob, n)) {
            return Ok(Response::json(v.clone()));
        }
        self.ensure_loaded(&[rel.to_string()])?;
        let d = decode::decode_file(self.fs().as_ref(), &p).map_err(|e| anyhow!("{e:#}"));
        let d = match d {
            Ok(d) => d,
            Err(e) => return Ok(Response::text(422, e.to_string())),
        };
        let v = json!({"duration": d.duration(), "sampleRate": d.sample_rate, "peaks": decode::peaks(&d, n)});
        self.peaks.insert((blob, n), v.clone());
        Ok(Response::json(v))
    }

    /// Voice to notes: the notes (or drum hits) in a recorded take.
    fn transcribe(&mut self, rel: &str, mode: &str) -> Result<Response> {
        let p = self
            .folder
            .resolve(rel)
            .ok_or_else(|| anyhow!("invalid path"))?;
        if self.mem.blob(&p).is_none() {
            return Ok(Response::text(404, format!("{rel} does not exist")));
        }
        self.ensure_loaded(&[rel.to_string()])?;
        match decode::decode_file(self.fs().as_ref(), &p) {
            Ok(d) => Ok(Response::json(serde_json::to_value(
                rosaclef_studio::transcribe::transcribe(&d, mode),
            )?)),
            Err(e) => Ok(Response::text(422, format!("{e:#}"))),
        }
    }

    /// Render the song (or a pattern) into `renders/` (or `out`).
    pub fn render(
        &mut self,
        pattern: &str,
        loops: u32,
        bits: u16,
        sample_rate: u32,
        out: Option<&str>,
    ) -> Result<Value> {
        let project = self.doc.project.clone();
        self.ensure_loaded(&required_samples(&project))?;
        self.fonts.ensure(&required_presets(&project))?;
        let scope = if pattern.is_empty() {
            RenderScope::Song
        } else {
            if project.pattern(pattern).is_none() {
                bail!("no pattern with id {pattern:?}");
            }
            RenderScope::Pattern {
                id: pattern.to_string(),
                loops: loops.max(1),
            }
        };
        let bits = if [16, 24, 32].contains(&bits) {
            bits
        } else {
            24
        };
        let title = slug(&project.meta.title);
        let name = if pattern.is_empty() {
            title
        } else {
            format!("{title}-{pattern}")
        };
        let stamp = (self.fs().now_ms() / 1000.0).floor() as i64;
        let rel = match out {
            Some(o) => o.to_string(),
            None => format!("{}/{name}-{stamp}.wav", folder::RENDERS_DIR),
        };
        let (audio, warnings) = render_project(
            &self.folder,
            project,
            &scope,
            sample_rate.clamp(8000, 192000) as f32,
            &self.fonts,
            |_| {},
        );
        // The browser holds this memory: keep only the soundfont indexes.
        self.fonts.clear();
        self.font_files.forget_pieces();
        self.folder.write(&rel, &encode_wav(&audio, bits))?;
        let (peak_db, rms_db) = levels_db(&audio);
        Ok(json!({
            "path": rel,
            "url": format!("/files/{rel}"),
            "duration": audio.duration(),
            "peakDb": peak_db,
            "rmsDb": rms_db,
            "warnings": warnings,
        }))
    }

    fn export(&self, f: &Folder) -> Result<Response> {
        // Every file's content is needed.
        let files = library::files(f, "", &Project::empty(""))?;
        let mut missing = false;
        for file in &files {
            if let Some(p) = f.resolve(&file.path) {
                if let Some((_, false)) = self.mem.blob(&p) {
                    let _ = self.mem.read(&p);
                    missing = true;
                }
            }
        }
        if missing {
            return Err(NeedContent.into());
        }
        Ok(Response::bytes("application/zip", archive::export(f)?))
    }

    fn rename_project(&mut self, name: &str, to: &str) -> Result<()> {
        let src = self.library.project_dir(name)?;
        if src != self.folder.dir {
            return self.library.rename(name, to);
        }
        // The open project: move it and follow it.
        let dst = self.library.checked_target(to)?;
        self.fs().rename(&src, &dst)?;
        self.folder = Folder::new(self.fs().clone(), dst);
        self.remember()?;
        if self.doc.project.meta.title == name {
            let mut p = self.doc.project.clone();
            p.meta.title = to.to_string();
            let issues = validate::validate(&p);
            self.apply(p, issues, "files", 0)?;
        }
        let w = self.welcome("switched", 0);
        self.send(None, 0, "ws", w);
        Ok(())
    }

    fn import_midi_into(&mut self, body: &[u8]) -> Result<Response> {
        let im =
            rosaclef_import::midi::import(body, &rosaclef_import::midi::Options::new("import"))?;
        let mut project = self.doc.project.clone();
        let before = project.channels.len();
        let mut warnings = im.warnings;
        warnings.extend(rosaclef_import::merge_into(&mut project, im.project));
        let issues = validate::validate(&project);
        if issues.iter().any(|i| i.severity == Severity::Error) {
            let msgs: Vec<String> = issues.iter().take(3).map(|i| i.to_string()).collect();
            bail!("the merged project would be invalid:\n{}", msgs.join("\n"));
        }
        let added = project.channels.len() - before;
        self.apply(project, issues, "import", 0)?;
        Ok(Response::json(
            json!({"channels": added, "warnings": warnings}),
        ))
    }
}

fn import_name(q: &BTreeMap<String, String>, fallback: &str) -> String {
    if let Some(n) = q.get("name").map(|n| n.trim()).filter(|n| !n.is_empty()) {
        return n.to_string();
    }
    q.get("filename")
        .and_then(|f| Path::new(f).file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

fn content_type(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("wav") | Some("wave") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("ogg") | Some("oga") => "audio/ogg",
        Some("m4a") | Some("aac") => "audio/mp4",
        Some("aif") | Some("aiff") => "audio/aiff",
        Some("json") => "application/json",
        Some("md") => "text/markdown; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}
