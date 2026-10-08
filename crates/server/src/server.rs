//! HTTP + WebSocket server: project sync, file watching, rendering, samples,
//! plugins, native audio and the agent terminal.
//!
//! Several projects can be open at once, one per browser tab (or several tabs
//! on one). Each open project is a [`Proj`]: its folder, document, clients,
//! agent terminal, watcher and jobs, and an unguessable `key`. Requests for a
//! project are scoped by that key — `/s/{key}/api/...`, `/s/{key}/files/...`,
//! `/s/{key}/ws` — and only ever reach that project's state: nothing a
//! request carries (a job id, a file path, a folder) can name another
//! project's. Requests without a scope act on the home project (the folder
//! `rosaclef serve` opened). A project is opened from the library by name
//! (`POST /api/projects/open`), never by an arbitrary path, and is closed
//! again once no tab and no agent has used it for a while.

use crate::folder::{self, Folder};
use crate::terminal::{AgentEnv, Terminal};
use anyhow::{bail, Result};
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use rosaclef_core::validate::{self, Issue, Severity};
use rosaclef_core::{format, Project};
use rosaclef_engine::render::RenderScope;
use rosaclef_engine::Engine;
use rosaclef_fs::Fs;
use rosaclef_studio::fonts::{DirFonts, Fonts};
use rosaclef_studio::library::{unique_sample_path, Library};
use rosaclef_studio::render::{levels_db, render_project_with};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

pub struct Config {
    pub folder: Folder,
    /// Folder holding the project library (one project per subfolder).
    pub library: PathBuf,
    pub host: String,
    pub port: u16,
    pub web: PathBuf,
}

static FONTS: std::sync::OnceLock<Arc<Fonts>> = std::sync::OnceLock::new();

/// Read soundfonts from `web/soundfonts`.
pub fn init_fonts(web: &std::path::Path) {
    let _ = FONTS.set(Arc::new(Fonts::new(Arc::new(DirFonts(
        web.join("soundfonts"),
    )))));
}

/// The soundfonts `soundfont` instruments play (loaded presets are cached).
pub fn fonts() -> Arc<Fonts> {
    FONTS
        .get_or_init(|| {
            Arc::new(Fonts::new(Arc::new(DirFonts(
                crate::default_web_dir().join("soundfonts"),
            ))))
        })
        .clone()
}

/// Give an engine access to native plugins.
pub fn install_plugin_host(engine: &mut Engine) {
    engine.set_plugin_host(Arc::new(rosaclef_clap::ClapHost::new()));
}

/// Sequence number of `.rosaclef/context.json` writes.
static CONTEXT_SEQ: AtomicU64 = AtomicU64::new(0);

fn now_rfc3339() -> String {
    rosaclef_studio::rfc3339(rosaclef_fs::DiskFs.now_ms())
}

struct Doc {
    project: Project,
    rev: u64,
    /// Hash of the last text we wrote or loaded, to ignore our own writes.
    last_hash: u64,
    issues: Vec<Issue>,
}

/// A message fanned out to every connected UI (except `exclude`).
#[derive(Clone)]
struct Broadcast {
    exclude: u64,
    text: Arc<str>,
}

pub struct App {
    pub(crate) library: Library,
    url: String,
    plugins: Mutex<Option<Vec<rosaclef_clap::PluginDescriptor>>>,
    next_client: AtomicU64,
    /// The last job id given (ids are unique across projects; a job is only
    /// ever found through the project it was started for).
    next_job: AtomicU64,
    /// The projects open now; the first is the home project, never closed.
    open: Mutex<Vec<Arc<Proj>>>,
    /// Serializes opening, closing and moving open projects, so one folder is
    /// never open twice.
    opening: Mutex<()>,
}

/// One open project.
pub(crate) struct Proj {
    /// The project's handle in scoped URLs (`/s/{key}/...`): 128 random bits,
    /// so a page (or agent) can only reach a project it was given.
    pub(crate) key: String,
    /// The project folder (it moves when the project is renamed); take a
    /// snapshot with [`Proj::folder`].
    folder: RwLock<Folder>,
    doc: Mutex<Doc>,
    /// Messages for this project's clients (and no other's).
    tx: broadcast::Sender<Broadcast>,
    /// This project's agent terminal, running in its folder.
    term: Arc<Terminal>,
    #[cfg(feature = "device-audio")]
    native: Mutex<Option<crate::device::Native>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// This project's long jobs (`/api/jobs`): how each one ended.
    jobs: Mutex<Vec<Ended>>,
    /// Connected UI clients (WebSockets).
    clients: AtomicUsize,
    /// When the last client left (or the project was opened).
    idle_since: Mutex<Option<Instant>>,
    /// Set once the project is closed: clients that still reach it go away.
    closed: AtomicBool,
    /// Serializes file operations on the project with renames of its folder.
    pub(crate) busy: Mutex<()>,
    /// The project itself, for tasks that must not keep it open.
    me: std::sync::Weak<Proj>,
}

/// How a job of `/api/jobs` stands: running, or its answer.
struct Ended {
    id: u32,
    outcome: Option<Result<Value, (StatusCode, String)>>,
}

/// Finished jobs kept for their pages to collect.
const KEEP_JOBS: usize = 16;

/// How long a project nobody uses (no tab, no running agent, no job) stays open.
const IDLE_CLOSE: Duration = Duration::from_secs(60);

/// The project a request is for: the one its `/s/{key}/` prefix names, else
/// the home project. Handlers of project data take it and nothing else.
#[derive(Clone)]
pub(crate) struct Scope(pub(crate) Arc<Proj>);

pub(crate) type Shared = Arc<App>;

pub async fn run(cfg: Config) -> Result<()> {
    let checked = cfg.folder.load()?;
    let text = cfg.folder.read_text()?;
    let (project, issues) = match checked.project {
        Some(p) if checked.issues.iter().all(|i| i.severity != Severity::Error) => {
            (p, checked.issues)
        }
        _ => {
            eprintln!(
                "warning: {} is invalid; starting from an empty project until it is fixed:",
                cfg.folder.project_path().display()
            );
            for i in &checked.issues {
                eprintln!("  {i}");
            }
            (Project::empty("Untitled"), checked.issues)
        }
    };
    let url = format!("http://{}:{}", cfg.host, cfg.port);
    let home = Proj::new(
        cfg.folder.clone(),
        &url,
        project,
        issues,
        folder::hash(&text),
    );
    let app = Arc::new(App {
        library: Library::new(rosaclef_fs::disk(), cfg.library.clone(), &crate::exe()),
        url: url.clone(),
        plugins: Mutex::new(None),
        next_client: AtomicU64::new(1),
        next_job: AtomicU64::new(0),
        open: Mutex::new(vec![home.clone()]),
        opening: Mutex::new(()),
    });
    write_status(&home);
    spawn_watcher(&home)?;
    spawn_reaper(app.clone());
    #[cfg(feature = "device-audio")]
    spawn_native_status(app.clone());

    let web = ServeDir::new(&cfg.web).append_index_html_on_directories(true);
    let router = Router::new()
        .route("/ws", get(ws_handler))
        .route("/ws/term", get(term_handler))
        .route("/api/project", get(get_project).put(put_project))
        .route(
            "/api/schema",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    rosaclef_core::schema::schema_text(),
                )
            }),
        )
        .route("/api/catalog", get(get_catalog))
        .route("/api/plugins", get(get_plugins))
        .route("/api/plugins/params", get(get_plugin_params))
        .route(
            "/api/samples",
            get(list_samples)
                .post(upload_sample)
                .layer(axum::extract::DefaultBodyLimit::max(512 << 20)),
        )
        .route("/api/peaks", get(get_peaks))
        .route("/api/transcribe", get(get_transcription))
        .route(
            "/api/grooves",
            get(|| async { Json(rosaclef_core::drums::catalog()) }),
        )
        .route(
            "/api/drums",
            post(write_drums).layer(axum::extract::DefaultBodyLimit::max(256 << 20)),
        )
        .route(
            "/api/drums/pattern",
            post(make_drum_pattern).layer(axum::extract::DefaultBodyLimit::max(256 << 20)),
        )
        .route(
            "/api/critic",
            get(|| async { Json(rosaclef_core::critic::catalog()) })
                .post(critique)
                .layer(axum::extract::DefaultBodyLimit::max(256 << 20)),
        )
        .route(
            "/api/mixcheck",
            get(|| async { Json(rosaclef_studio::mixcheck::catalog()) })
                .post(mixcheck)
                .layer(axum::extract::DefaultBodyLimit::max(256 << 20)),
        )
        .route("/api/render", post(render))
        // The same as jobs: the answer at once is the job's id, and the job
        // says how far it has come, then what it found.
        .route(
            "/api/jobs/mixcheck",
            post(start_mixcheck).layer(axum::extract::DefaultBodyLimit::max(256 << 20)),
        )
        .route("/api/jobs/render", post(start_render))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/agents", get(get_agents))
        .route("/api/info", get(get_info))
        .nest("/files", Router::new().fallback(serve_file))
        .merge(crate::library::routes())
        .fallback_service(web)
        .with_state(app.clone());
    // Before routing: `/s/{key}/...` names the project, and is routed as `/...`.
    let service = tower::Layer::layer(
        &axum::middleware::from_fn_with_state(app.clone(), scope_requests),
        router,
    );

    let listener = tokio::net::TcpListener::bind((cfg.host.as_str(), cfg.port)).await?;
    println!();
    println!("  ✦ Rosaclef studio");
    println!("    project  {}", cfg.folder.dir.display());
    println!("    library  {}", cfg.library.display());
    println!("    open     {url}");
    println!();
    axum::serve(
        listener,
        axum::ServiceExt::<Request>::into_make_service(service),
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------------ helpers

/// 128 random bits, in hex.
fn new_key() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system's random numbers");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Whether two keys are the same, in a time that does not tell how much of
/// a guess was right.
fn same_key(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Find the project a request names and route the request without the name:
/// `/s/{key}/rest` is served as `/rest` for that project. A key that names no
/// open project is refused (it is never taken as another project's).
async fn scope_requests(State(app): State<Shared>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let proj = if let Some(rest) = path.strip_prefix("/s/") {
        let (key, tail) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(p) = app.find(key) else {
            return (
                StatusCode::NOT_FOUND,
                "this project is not open in the studio (any more); open it again",
            )
                .into_response();
        };
        let rest = match req.uri().query() {
            Some(q) => format!("/{tail}?{q}"),
            None => format!("/{tail}"),
        };
        match rest.parse::<Uri>() {
            Ok(u) => *req.uri_mut() = u,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid path").into_response(),
        }
        p
    } else {
        app.home()
    };
    req.extensions_mut().insert(Scope(proj));
    next.run(req).await
}

/// Reject cross-site requests (a web page on another origin must not be able
/// to drive the terminal or rewrite the project).
pub(crate) fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let origin_host = origin.split("://").nth(1).unwrap_or("");
    origin_host == host
}

pub(crate) fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "cross-origin request refused").into_response()
}

/// A client edit made for a project that is no longer open.
#[derive(Debug)]
pub(crate) struct Stale;

impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the edit was made for another project folder (the project moved)"
        )
    }
}

impl std::error::Error for Stale {}

impl App {
    /// The home project (what `rosaclef serve` opened).
    pub(crate) fn home(&self) -> Arc<Proj> {
        self.open.lock()[0].clone()
    }

    /// The open project with this key.
    fn find(&self, key: &str) -> Option<Arc<Proj>> {
        if key.is_empty() {
            return None;
        }
        self.open
            .lock()
            .iter()
            .find(|p| same_key(&p.key, key))
            .cloned()
    }

    /// The open project in `dir` (a canonical path).
    pub(crate) fn open_at(&self, dir: &Path) -> Option<Arc<Proj>> {
        self.open
            .lock()
            .iter()
            .find(|p| p.folder().dir == dir)
            .cloned()
    }

    /// The folder a request may open: a project of the library named `name`,
    /// or the project at `path` — which must be open already (the home
    /// project may live outside the library) or a project of the library.
    /// Nothing else on disk can be opened.
    pub(crate) fn openable(&self, name: Option<&str>, path: Option<&str>) -> Result<PathBuf> {
        let dir = match (
            name.filter(|n| !n.is_empty()),
            path.filter(|p| !p.is_empty()),
        ) {
            (Some(n), _) => self.library.project_dir(n)?,
            (None, Some(p)) => {
                let dir = Path::new(p);
                if !dir.is_absolute() {
                    bail!("{p:?} is not an absolute path");
                }
                let canon = dir
                    .canonicalize()
                    .map_err(|_| anyhow::anyhow!("no project at {p}"))?;
                if self.open_at(&canon).is_some() {
                    return Ok(canon);
                }
                let library = self
                    .library
                    .dir
                    .canonicalize()
                    .unwrap_or(self.library.dir.clone());
                let name = canon.file_name().map(|n| n.to_string_lossy().to_string());
                match name {
                    Some(n) if canon.parent() == Some(library.as_path()) => {
                        self.library.project_dir(&n)?
                    }
                    _ => bail!("{p} is not a project of the library {}", library.display()),
                }
            }
            (None, None) => bail!("name the project to open"),
        };
        Ok(dir.canonicalize().unwrap_or(dir))
    }

    /// Open the project in `dir` (from [`App::openable`]), or find it open.
    /// Blocking (it reads and validates the project): call it from
    /// `spawn_blocking`.
    pub(crate) fn open_dir(&self, dir: &Path) -> Result<Arc<Proj>> {
        let _guard = self.opening.lock();
        if let Some(p) = self.open_at(dir) {
            return Ok(p);
        }
        let target = Folder::on_disk(dir);
        if !target.project_path().is_file() {
            bail!(
                "{} has no {}",
                target.dir.display(),
                rosaclef_core::PROJECT_FILE
            );
        }
        target.init(false)?;
        let text = target.read_text()?;
        let checked = validate::parse_and_validate(&text);
        if !checked.is_ok() {
            let msgs: Vec<String> = checked.errors().take(3).map(|i| i.to_string()).collect();
            bail!(
                "the project is invalid and cannot be opened:\n{}",
                msgs.join("\n")
            );
        }
        let project = checked.project.expect("checked");
        crate::guide::write(&target, &crate::exe())?;
        let proj = Proj::new(
            target.clone(),
            &self.url,
            project,
            checked.issues,
            folder::hash(&text),
        );
        write_status(&proj);
        spawn_watcher(&proj)?;
        self.open.lock().push(proj.clone());
        println!("  ⇄ opened {}", target.dir.display());
        Ok(proj)
    }

    /// Rename an open project's folder and follow it (its clients hear
    /// `switched` with the new folder).
    pub(crate) fn rename_open(&self, proj: &Proj, to: &Path) -> Result<()> {
        let _guard = self.opening.lock();
        let _busy = proj.busy.lock();
        let (target, project) = {
            // No edit can be written while the folder moves.
            let doc = proj.doc.lock();
            let old = proj.folder();
            std::fs::rename(&old.dir, to)?;
            let target = Folder::on_disk(to);
            *proj.folder.write() = target.clone();
            (target, doc.project.clone())
        };
        proj.repoint(&target, &project, &self.url);
        println!(
            "  ⇄ renamed {} to {}",
            proj.key_hint(),
            target.dir.display()
        );
        Ok(())
    }

    /// Close the projects nobody has used for a while (never the home project).
    fn close_idle(&self) {
        let _guard = self.opening.lock();
        let closing: Vec<Arc<Proj>> = {
            let mut open = self.open.lock();
            let mut closing = Vec::new();
            let mut i = 1;
            while i < open.len() {
                if open[i].idle_for() >= IDLE_CLOSE {
                    // Clients count themselves in before looking at `closed`.
                    open[i].closed.store(true, Ordering::SeqCst);
                    if open[i].clients.load(Ordering::SeqCst) == 0 {
                        closing.push(open.remove(i));
                        continue;
                    }
                    open[i].closed.store(false, Ordering::SeqCst);
                }
                i += 1;
            }
            closing
        };
        for p in closing {
            p.shut_down();
            println!("  ✕ closed {}", p.folder().dir.display());
        }
    }
}

impl Proj {
    fn new(
        folder: Folder,
        url: &str,
        project: Project,
        issues: Vec<Issue>,
        hash: u64,
    ) -> Arc<Proj> {
        let key = new_key();
        let (tx, _) = broadcast::channel(256);
        Arc::new_cyclic(|me| Proj {
            me: me.clone(),
            term: Arc::new(Terminal::new(AgentEnv {
                dir: folder.dir.clone(),
                url: scoped_url(url, &key),
            })),
            key,
            folder: RwLock::new(folder),
            doc: Mutex::new(Doc {
                project,
                rev: 1,
                last_hash: hash,
                issues,
            }),
            tx,
            #[cfg(feature = "device-audio")]
            native: Mutex::new(None),
            watcher: Mutex::new(None),
            jobs: Mutex::new(Vec::new()),
            clients: AtomicUsize::new(0),
            idle_since: Mutex::new(Some(Instant::now())),
            closed: AtomicBool::new(false),
            busy: Mutex::new(()),
        })
    }

    fn self_weak(&self) -> std::sync::Weak<Proj> {
        self.me.clone()
    }

    /// The start of the key, for logs (the whole key is a capability).
    fn key_hint(&self) -> String {
        format!("project {}…", &self.key[..6])
    }

    fn broadcast(&self, exclude: u64, v: Value) {
        let _ = self.tx.send(Broadcast {
            exclude,
            text: v.to_string().into(),
        });
    }

    /// Snapshot of the project folder.
    pub(crate) fn folder(&self) -> Folder {
        self.folder.read().clone()
    }

    pub(crate) fn project(&self) -> Project {
        self.doc.lock().project.clone()
    }

    pub(crate) fn send_all(&self, v: Value) {
        self.broadcast(0, v);
    }

    /// How long nothing has kept the project open: no client, no running
    /// agent, no running job.
    fn idle_for(&self) -> Duration {
        let mut since = self.idle_since.lock();
        if self.clients.load(Ordering::SeqCst) > 0
            || self.term.running()
            || self.jobs.lock().iter().any(|j| j.outcome.is_none())
        {
            // The clock starts again once nothing keeps it open.
            *since = None;
            return Duration::ZERO;
        }
        since.get_or_insert_with(Instant::now).elapsed()
    }

    /// Stop everything the project runs: watcher, agent, native engine.
    fn shut_down(&self) {
        self.closed.store(true, Ordering::SeqCst);
        *self.watcher.lock() = None;
        #[cfg(feature = "device-audio")]
        {
            *self.native.lock() = None;
        }
        self.term.shut_down();
    }

    /// Accept a new project version from a client or the HTTP API.
    /// `expect_folder` (sent by UI clients) guards against edits that were
    /// made for the project's folder before it moved.
    pub(crate) fn apply(
        &self,
        project: Project,
        issues: Vec<Issue>,
        origin: &str,
        exclude: u64,
        expect_folder: Option<&str>,
    ) -> Result<u64> {
        let rev = {
            // The doc lock also serializes against renames.
            let mut doc = self.doc.lock();
            let folder = self.folder();
            if let Some(f) = expect_folder {
                if f != folder.dir.display().to_string() {
                    return Err(Stale.into());
                }
            }
            let text = folder.write_project(&project)?;
            doc.rev += 1;
            doc.last_hash = folder::hash(&text);
            doc.project = project.clone();
            doc.issues = issues.clone();
            doc.rev
        };
        self.broadcast(exclude, json!({"t": "project", "rev": rev, "origin": origin, "project": project, "issues": issues}));
        write_status(self);
        self.update_native(&project);
        Ok(rev)
    }

    /// The `welcome` message (and `switched`, which carries the same data
    /// after the project's folder moved).
    fn welcome(&self, t: &str, client: u64) -> Value {
        let folder = self.folder();
        let doc = self.doc.lock();
        json!({
            "t": t,
            "client": client,
            "session": self.key,
            "rev": doc.rev,
            "project": doc.project,
            "issues": doc.issues,
            "folder": folder.dir.display().to_string(),
            "name": folder.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "samples": folder.list_samples(),
            "native": native_status(self),
        })
    }

    /// After the folder moved: watcher, status, native engine, agent
    /// terminal, and a `switched` message so this project's clients follow.
    fn repoint(&self, target: &Folder, project: &Project, url: &str) {
        {
            let mut w = self.watcher.lock();
            if w.is_some() {
                *w = None;
                drop(w);
                if let Err(e) = spawn_watcher_on(self, target) {
                    eprintln!("warning: cannot watch {}: {e}", target.dir.display());
                }
            }
        }
        write_status(self);
        self.update_native(project);
        self.term.set_env(AgentEnv {
            dir: target.dir.clone(),
            url: scoped_url(url, &self.key),
        });
        if let Err(e) = self.term.restart() {
            eprintln!(
                "warning: could not restart the agent in {}: {e}",
                target.dir.display()
            );
        }
        self.broadcast(0, self.welcome("switched", 0));
    }

    #[cfg(feature = "device-audio")]
    fn update_native(&self, project: &Project) {
        if let Some(n) = self.native.lock().as_ref() {
            n.set_project(project.clone(), &self.folder());
        }
    }

    #[cfg(not(feature = "device-audio"))]
    fn update_native(&self, _project: &Project) {}
}

/// The base URL of one project's HTTP API (what its agent gets as `$ROSACLEF_URL`).
fn scoped_url(url: &str, key: &str) -> String {
    format!("{url}/s/{key}")
}

/// Close idle projects now and then.
fn spawn_reaper(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            let a = app.clone();
            let _ = tokio::task::spawn_blocking(move || a.close_idle()).await;
        }
    });
}

fn write_status(app: &Proj) {
    let doc = app.doc.lock();
    let ok = doc.issues.iter().all(|i| i.severity != Severity::Error);
    let v = json!({"ok": ok, "rev": doc.rev, "issues": doc.issues});
    let _ = folder::write_atomic(
        &app.folder().state_path("status.json"),
        (serde_json::to_string_pretty(&v).unwrap() + "\n").as_bytes(),
    );
}

fn write_invalid_status(app: &Proj, issues: &[Issue]) {
    let rev = app.doc.lock().rev;
    let v = json!({
        "ok": false,
        "rev": rev,
        "note": "project.json on disk is invalid; the studio keeps using the last valid version until it is fixed",
        "issues": issues
    });
    let _ = folder::write_atomic(
        &app.folder().state_path("status.json"),
        (serde_json::to_string_pretty(&v).unwrap() + "\n").as_bytes(),
    );
}

// ---------------------------------------------------------------- watching

/// A file watcher on `dir` that forwards changed paths to `tx`.
fn make_watcher(
    tx: tokio::sync::mpsc::UnboundedSender<PathBuf>,
    dir: &std::path::Path,
) -> Result<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })?;
    watcher.watch(dir, RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn spawn_watcher(proj: &Arc<Proj>) -> Result<()> {
    let folder = proj.folder();
    spawn_watcher_on(proj, &folder)
}

/// Watch `folder` for the project: agent edits of project.json, new samples.
/// The watcher lives on the project; dropping it (a rename, closing the
/// project) ends the task, which holds the project only weakly.
fn spawn_watcher_on(proj: &Proj, folder: &Folder) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let watcher = make_watcher(tx, &folder.dir)?;
    *proj.watcher.lock() = Some(watcher);
    let me = proj.self_weak();
    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            // Debounce bursts of events (editors write in several steps).
            tokio::time::sleep(Duration::from_millis(60)).await;
            let mut paths = vec![first];
            while let Ok(p) = rx.try_recv() {
                paths.push(p);
            }
            let Some(proj) = me.upgrade() else { break };
            // Events from a folder the project just left no longer match.
            let folder = proj.folder();
            let project_path = folder.project_path();
            let samples_dir = folder.dir.join(folder::SAMPLES_DIR);
            if paths.iter().any(|p| {
                p.file_name() == project_path.file_name() && p.parent() == project_path.parent()
            }) {
                reload_from_disk(&proj);
            }
            if paths.iter().any(|p| p.starts_with(&samples_dir)) {
                proj.broadcast(0, json!({"t": "samples", "samples": folder.list_samples()}));
            }
        }
    });
    Ok(())
}

fn reload_from_disk(app: &Proj) {
    enum Outcome {
        Changed(u64, Box<Project>, Vec<Issue>),
        Invalid(Vec<Issue>),
    }
    // Read and compare under the doc lock so a concurrent project switch
    // cannot pair the old folder's text with the new document.
    let outcome = {
        let mut doc = app.doc.lock();
        let Ok(text) = app.folder().read_text() else {
            return;
        };
        let h = folder::hash(&text);
        if doc.last_hash == h {
            return;
        }
        doc.last_hash = h;
        let checked = validate::parse_and_validate(&text);
        if checked.is_ok() {
            let project = checked.project.unwrap();
            if doc.project == project {
                return;
            }
            doc.rev += 1;
            doc.project = project.clone();
            doc.issues = checked.issues.clone();
            Outcome::Changed(doc.rev, Box::new(project), checked.issues)
        } else {
            Outcome::Invalid(checked.issues)
        }
    };
    match outcome {
        Outcome::Changed(rev, project, issues) => {
            println!("  ↻ project.json changed on disk (rev {rev})");
            app.broadcast(0, json!({"t": "project", "rev": rev, "origin": "disk", "project": project, "issues": issues}));
            write_status(app);
            app.update_native(&project);
        }
        Outcome::Invalid(issues) => {
            println!(
                "  ✗ project.json on disk is invalid ({} issue(s))",
                issues.len()
            );
            app.broadcast(0, json!({"t": "invalid", "issues": issues}));
            write_invalid_status(app, &issues);
        }
    }
}

// --------------------------------------------------------------- websocket

async fn ws_handler(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    ws.max_message_size(64 << 20)
        .on_upgrade(move |socket| client(app, proj, socket))
}

/// One UI client of one project: it hears that project's messages and its
/// edits go to that project, whatever they say.
async fn client(app: Shared, proj: Arc<Proj>, socket: WebSocket) {
    let id = app.next_client.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    // Count in before looking at `closed` (see `App::close_idle`).
    proj.clients.fetch_add(1, Ordering::SeqCst);
    let leave = |proj: &Proj| {
        if proj.clients.fetch_sub(1, Ordering::SeqCst) == 1 {
            *proj.idle_since.lock() = Some(Instant::now());
        }
    };
    if proj.closed.load(Ordering::SeqCst) {
        // Closed while this client was on its way: it opens the project again.
        let _ = sink
            .send(Message::Text(
                json!({"t": "closed", "message": "the project was closed; opening it again"})
                    .to_string()
                    .into(),
            ))
            .await;
        leave(&proj);
        return;
    }
    let welcome = proj.welcome("welcome", id);
    if sink
        .send(Message::Text(welcome.to_string().into()))
        .await
        .is_err()
    {
        leave(&proj);
        return;
    }
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut sub = proj.tx.subscribe();
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                m = sub.recv() => match m {
                    Ok(b) => {
                        if b.exclude != id && sink.send(Message::Text(b.text.to_string().into())).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                m = out_rx.recv() => match m {
                    Some(text) => if sink.send(Message::Text(text.into())).await.is_err() { break; },
                    None => break,
                },
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let reply = handle_client_message(&proj, id, v).await;
        if let Some(r) = reply {
            let _ = out_tx.send(r.to_string());
        }
    }
    send_task.abort();
    leave(&proj);
}

async fn handle_client_message(proj: &Arc<Proj>, id: u64, v: Value) -> Option<Value> {
    let t = v.get("t").and_then(|t| t.as_str()).unwrap_or("");
    match t {
        "put" => {
            let checked =
                validate::value_and_validate(v.get("project").cloned().unwrap_or(Value::Null));
            if !checked.is_ok() {
                return Some(json!({"t": "rejected", "issues": checked.issues}));
            }
            // UI clients say which folder the edit is for.
            let expect = v.get("folder").and_then(|f| f.as_str());
            match proj.apply(checked.project.unwrap(), checked.issues, "ui", id, expect) {
                Ok(rev) => Some(json!({"t": "ack", "rev": rev})),
                Err(e) if e.is::<Stale>() => None,
                Err(e) => Some(json!({"t": "error", "message": e.to_string()})),
            }
        }
        "context" => {
            let mut ctx =
                rosaclef_core::context::normalize(v.get("context").cloned().unwrap_or(Value::Null));
            ctx.seq = CONTEXT_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
            ctx.updated_at = now_rfc3339();
            for e in &mut ctx.recent_edits {
                if e.at.is_empty() {
                    e.at = ctx.updated_at.clone();
                }
            }
            let _ = folder::write_atomic(
                &proj.folder().state_path("context.json"),
                (serde_json::to_string_pretty(&ctx).unwrap() + "\n").as_bytes(),
            );
            None
        }
        #[cfg(feature = "device-audio")]
        t if t.starts_with("native.") => crate::device::handle(proj.clone(), t, &v).await,
        _ => None,
    }
}

pub(crate) fn native_status(app: &Proj) -> Value {
    #[cfg(feature = "device-audio")]
    {
        if let Some(n) = app.native.lock().as_ref() {
            return n.status_json();
        }
        json!({"available": true, "enabled": false})
    }
    #[cfg(not(feature = "device-audio"))]
    {
        let _ = app;
        json!({"available": false, "enabled": false})
    }
}

// ------------------------------------------------------------ native audio

#[cfg(feature = "device-audio")]
impl Proj {
    pub(crate) fn native(&self) -> &Mutex<Option<crate::device::Native>> {
        &self.native
    }
    pub(crate) fn apply_edit(&self, project: Project) {
        let issues = validate::validate(&project);
        let _ = self.apply(project, issues, "studio", 0, None);
    }
}

/// Each project's native engine meters, to that project's clients.
#[cfg(feature = "device-audio")]
fn spawn_native_status(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        loop {
            tick.tick().await;
            let open: Vec<Arc<Proj>> = app.open.lock().clone();
            for p in open {
                let msg = p.native.lock().as_ref().map(|n| n.meters_json());
                if let Some(m) = msg {
                    p.broadcast(0, m);
                }
            }
        }
    });
}

// -------------------------------------------------------------------- HTTP

async fn get_project(Extension(Scope(proj)): Extension<Scope>) -> impl IntoResponse {
    let text = format::to_string(&proj.doc.lock().project);
    ([(header::CONTENT_TYPE, "application/json")], text)
}

async fn put_project(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let checked = validate::parse_and_validate(&String::from_utf8_lossy(&body));
    if !checked.is_ok() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"ok": false, "issues": checked.issues})),
        )
            .into_response();
    }
    match proj.apply(checked.project.unwrap(), checked.issues, "api", 0, None) {
        Ok(rev) => Json(json!({"ok": true, "rev": rev})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn cached_plugins(app: &App, rescan: bool) -> Vec<rosaclef_clap::PluginDescriptor> {
    let mut cache = app.plugins.lock();
    if rescan || cache.is_none() {
        *cache = Some(rosaclef_clap::scan(&rosaclef_clap::default_search_paths()));
    }
    cache.clone().unwrap_or_default()
}

async fn get_catalog(State(app): State<Shared>) -> impl IntoResponse {
    let plugins = tokio::task::spawn_blocking({
        let app = app.clone();
        move || cached_plugins(&app, false)
    })
    .await
    .unwrap_or_default();
    Json(
        json!({"devices": rosaclef_core::catalog::DEVICES, "presets": rosaclef_core::presets::all(), "plugins": plugins, "arp": rosaclef_core::arp::catalog(), "collections": [rosaclef_core::gm::collection()]}),
    )
}

#[derive(Deserialize)]
struct PluginsQuery {
    rescan: Option<bool>,
}

async fn get_plugins(
    State(app): State<Shared>,
    Query(q): Query<PluginsQuery>,
) -> impl IntoResponse {
    let rescan = q.rescan.unwrap_or(false);
    let plugins = tokio::task::spawn_blocking(move || cached_plugins(&app, rescan))
        .await
        .unwrap_or_default();
    Json(json!({"plugins": plugins}))
}

#[derive(Deserialize)]
struct ParamsQuery {
    path: String,
    id: Option<String>,
}

async fn get_plugin_params(Query(q): Query<ParamsQuery>) -> Response {
    let res = tokio::task::spawn_blocking(move || {
        rosaclef_clap::params(&q.path, q.id.as_deref().unwrap_or(""))
    })
    .await;
    match res {
        Ok(Ok(params)) => Json(json!({"params": params})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_samples(Extension(Scope(proj)): Extension<Scope>) -> impl IntoResponse {
    Json(json!({"samples": proj.folder().list_samples()}))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

async fn upload_sample(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let folder = proj.folder();
    let rel = unique_sample_path(&folder, &q.name);
    let path = folder.dir.join(&rel);
    if let Err(e) = folder::write_atomic(&path, &body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    Json(json!({"path": rel})).into_response()
}

#[derive(Deserialize)]
struct PeaksQuery {
    path: String,
    n: Option<usize>,
}

async fn get_peaks(
    Extension(Scope(proj)): Extension<Scope>,
    Query(q): Query<PeaksQuery>,
) -> Response {
    let Some(path) = proj.folder().resolve(&q.path) else {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    };
    if !path.is_file() {
        return (StatusCode::NOT_FOUND, format!("{} does not exist", q.path)).into_response();
    }
    let n = q.n.unwrap_or(1024);
    let res = tokio::task::spawn_blocking(move || {
        crate::decode::decode_file(&rosaclef_fs::DiskFs, &path)
            .map(|d| (d.duration(), d.sample_rate, crate::decode::peaks(&d, n)))
    })
    .await;
    match res {
        Ok(Ok((duration, sr, peaks))) => {
            Json(json!({"duration": duration, "sampleRate": sr, "peaks": peaks})).into_response()
        }
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct TranscribeQuery {
    path: String,
    /// "melody" (default) or "drums".
    mode: Option<String>,
}

/// Voice to notes: the notes (or drum hits) in a recorded take.
async fn get_transcription(
    Extension(Scope(proj)): Extension<Scope>,
    Query(q): Query<TranscribeQuery>,
) -> Response {
    let Some(path) = proj.folder().resolve(&q.path) else {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    };
    if !path.is_file() {
        return (StatusCode::NOT_FOUND, format!("{} does not exist", q.path)).into_response();
    }
    let mode = q.mode.unwrap_or_default();
    let res = tokio::task::spawn_blocking(move || {
        crate::decode::decode_file(&rosaclef_fs::DiskFs, &path)
            .map(|d| rosaclef_studio::transcribe::transcribe(&d, &mode))
    })
    .await;
    match res {
        Ok(Ok(t)) => Json(t).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct DrumsQuery {
    guess: Option<bool>,
    write: Option<bool>,
}

/// Write a drum part: the project in the body, the written project out (the
/// studio commits it as one undoable edit).
async fn write_drums(headers: HeaderMap, Query(q): Query<DrumsQuery>, body: String) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let (guess, write) = (q.guess.unwrap_or(false), q.write.unwrap_or(true));
    match tokio::task::spawn_blocking(move || rosaclef_core::drums::api_write(&body, guess, write))
        .await
    {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct DrumPatternQuery {
    id: String,
}

/// Make a drum pattern from its recipe: the project in the body, the
/// project with the pattern's notes made out (one undoable edit in the studio).
async fn make_drum_pattern(
    headers: HeaderMap,
    Query(q): Query<DrumPatternQuery>,
    body: String,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    match tokio::task::spawn_blocking(move || rosaclef_core::drums::api_pattern(&body, &q.id)).await
    {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// The Critic: the project in (`{"project", "fix"}`), its findings out —
/// and the fixed project when `fix` names fixes to apply.
async fn critique(headers: HeaderMap, body: String) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    match tokio::task::spawn_blocking(move || rosaclef_core::critic::api(&body)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn mixcheck(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let res = tokio::task::spawn_blocking(move || run_mixcheck(&proj, &body)).await;
    answer(res.unwrap_or_else(|e| Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))))
}

/// A mix check of the project; `body` is the request (`/api/mixcheck`).
fn run_mixcheck(proj: &Proj, body: &str) -> Result<Value, (StatusCode, String)> {
    let project = proj.project();
    let folder = proj.folder();
    let fonts = fonts();
    let env = rosaclef_studio::mixcheck::Env {
        folder: &folder,
        fonts: &fonts,
        setup: &install_plugin_host,
        progress: &|_| {},
        disk_cache: true,
        any_file: false,
    };
    rosaclef_studio::mixcheck::api(&env, &project, body)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.0))
}

fn answer(res: Result<Value, (StatusCode, String)>) -> Response {
    match res {
        Ok(v) => Json(v).into_response(),
        Err((code, e)) => (code, e).into_response(),
    }
}

#[derive(Deserialize)]
struct RenderReq {
    pattern: Option<String>,
    loops: Option<u32>,
    bits: Option<u16>,
    #[serde(rename = "sampleRate")]
    sample_rate: Option<u32>,
}

async fn render(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Json(req): Json<RenderReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let res = tokio::task::spawn_blocking(move || run_render(&proj, req)).await;
    answer(res.unwrap_or_else(|e| Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))))
}

/// Render the project (or a pattern of it) into renders/.
fn run_render(proj: &Proj, req: RenderReq) -> Result<Value, (StatusCode, String)> {
    let project = proj.project();
    let folder = proj.folder();
    let scope = match req.pattern.clone().filter(|p| !p.is_empty()) {
        Some(id) => RenderScope::Pattern {
            id,
            loops: req.loops.unwrap_or(1),
        },
        None => RenderScope::Song,
    };
    let bits = req.bits.filter(|b| [16, 24, 32].contains(b)).unwrap_or(24);
    let title = rosaclef_studio::slug(&project.meta.title);
    let name = match &req.pattern {
        Some(p) if !p.is_empty() => format!("{title}-{p}"),
        _ => title,
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rel = format!("{}/{name}-{stamp}.wav", folder::RENDERS_DIR);
    let (audio, warnings) = render_project_with(
        &folder,
        project,
        &scope,
        req.sample_rate.unwrap_or(48000) as f32,
        &fonts(),
        install_plugin_host,
        rosaclef_studio::jobs::exported,
    );
    folder::write_atomic(
        &folder.dir.join(&rel),
        &rosaclef_engine::render::encode_wav(&audio, bits),
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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

/// Start `work` as a job of the project on its own thread; the answer is its
/// id at once (`{"job": id}`), and `GET /api/jobs/{id}` — for the same
/// project — says how far it has come and, once it ends, its result (or error).
fn start_job(
    app: &App,
    proj: Arc<Proj>,
    what: &'static str,
    work: impl FnOnce(&Proj) -> Result<Value, (StatusCode, String)> + Send + 'static,
) -> Response {
    let id = (app.next_job.fetch_add(1, Ordering::Relaxed) % u32::MAX as u64) as u32 + 1;
    proj.jobs.lock().push(Ended { id, outcome: None });
    tokio::task::spawn_blocking(move || {
        let run = rosaclef_studio::jobs::start(id, what);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&proj)))
            .unwrap_or_else(|_| {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("the {what} failed"),
                ))
            });
        drop(run);
        let mut jobs = proj.jobs.lock();
        if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
            j.outcome = Some(outcome);
        }
        // Forget the oldest finished ones.
        while jobs.iter().filter(|j| j.outcome.is_some()).count() > KEEP_JOBS {
            if let Some(k) = jobs.iter().position(|j| j.outcome.is_some()) {
                jobs.remove(k);
            }
        }
    });
    (StatusCode::ACCEPTED, Json(json!({ "job": id }))).into_response()
}

async fn start_mixcheck(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    start_job(&app, proj, "mixcheck", move |p| run_mixcheck(p, &body))
}

async fn start_render(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Json(req): Json<RenderReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    start_job(&app, proj, "export", move |p| run_render(p, req))
}

/// A job of the project: how far it has come (`state` "running"), then
/// `state` "done" with its `result`, or "failed" with its `error` (and the
/// HTTP `status` the request on its own would have answered). Another
/// project's job is not found here.
async fn get_job(
    Extension(Scope(proj)): Extension<Scope>,
    axum::extract::Path(id): axum::extract::Path<u32>,
) -> Response {
    let jobs = proj.jobs.lock();
    let Some(j) = jobs.iter().find(|j| j.id == id) else {
        return (StatusCode::NOT_FOUND, format!("no job {id}")).into_response();
    };
    let mut v = serde_json::to_value(rosaclef_studio::jobs::get(id)).unwrap_or(json!({}));
    match &j.outcome {
        None => v["state"] = json!("running"),
        Some(Ok(r)) => {
            v["state"] = json!("done");
            v["result"] = r.clone();
        }
        Some(Err((code, e))) => {
            v["state"] = json!("failed");
            v["status"] = json!(code.as_u16());
            v["error"] = json!(e);
        }
    }
    Json(v).into_response()
}

async fn get_agents() -> impl IntoResponse {
    Json(json!({"agents": crate::terminal::presets()}))
}

async fn get_info(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
) -> impl IntoResponse {
    Json(json!({
        "folder": proj.folder().dir.display().to_string(),
        "library": app.library.dir.display().to_string(),
        "url": app.url,
        "version": env!("CARGO_PKG_VERSION"),
        "native": native_status(&proj),
    }))
}

// ---------------------------------------------------------------- terminal

async fn term_handler(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let term = proj.term.clone();
    ws.on_upgrade(move |socket| crate::terminal::serve(term, socket))
}

// ------------------------------------------------------------------ files

/// `/files/...`: static files of the request's project folder. Revalidated
/// on every use, since two projects may hold different files at one path.
async fn serve_file(Extension(Scope(proj)): Extension<Scope>, req: Request) -> Response {
    use tower::ServiceExt;
    let dir = proj.folder().dir;
    match ServeDir::new(dir).oneshot(req).await {
        Ok(res) => {
            let mut res = res.map(Body::new);
            res.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            res
        }
        Err(e) => match e {},
    }
}
