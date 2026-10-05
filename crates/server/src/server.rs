//! HTTP + WebSocket server: project sync, file watching, rendering, samples,
//! plugins, native audio and the agent terminal.

use crate::folder::{self, Folder};
use crate::terminal::{AgentEnv, Terminal};
use anyhow::{bail, Result};
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
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
use rosaclef_studio::render::{levels_db, render_project};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
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
    /// The open project. It changes when the producer opens another one
    /// from the library; take a snapshot with [`App::folder`].
    folder: RwLock<Folder>,
    pub(crate) library: Library,
    doc: Mutex<Doc>,
    tx: broadcast::Sender<Broadcast>,
    next_client: AtomicU64,
    term: Arc<Terminal>,
    url: String,
    plugins: Mutex<Option<Vec<rosaclef_clap::PluginDescriptor>>>,
    #[cfg(feature = "device-audio")]
    native: Mutex<Option<crate::device::Native>>,
    watcher: Mutex<
        Option<(
            notify::RecommendedWatcher,
            tokio::sync::mpsc::UnboundedSender<PathBuf>,
        )>,
    >,
    /// Serializes project switches (and library operations on the open project).
    pub(crate) switching: Mutex<()>,
}

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
    let (tx, _) = broadcast::channel(256);
    let app = Arc::new(App {
        folder: RwLock::new(cfg.folder.clone()),
        library: Library::new(rosaclef_fs::disk(), cfg.library.clone(), &crate::exe()),
        doc: Mutex::new(Doc {
            project,
            rev: 1,
            last_hash: folder::hash(&text),
            issues,
        }),
        tx,
        next_client: AtomicU64::new(1),
        term: Arc::new(Terminal::new(AgentEnv {
            dir: cfg.folder.dir.clone(),
            url: url.clone(),
        })),
        url: url.clone(),
        plugins: Mutex::new(None),
        #[cfg(feature = "device-audio")]
        native: Mutex::new(None),
        watcher: Mutex::new(None),
        switching: Mutex::new(()),
    });
    write_status(&app);

    spawn_watcher(app.clone())?;
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
        .route("/api/agents", get(get_agents))
        .route("/api/info", get(get_info))
        .nest("/files", Router::new().fallback(serve_file))
        .merge(crate::library::routes())
        .fallback_service(web)
        .with_state(app.clone());

    let listener = tokio::net::TcpListener::bind((cfg.host.as_str(), cfg.port)).await?;
    println!();
    println!("  ✦ Rosaclef studio");
    println!("    project  {}", cfg.folder.dir.display());
    println!("    library  {}", cfg.library.display());
    println!("    open     {url}");
    println!();
    axum::serve(listener, router).await?;
    Ok(())
}

// ------------------------------------------------------------------ helpers

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
            "the edit was made for another project (the studio switched projects)"
        )
    }
}

impl std::error::Error for Stale {}

impl App {
    fn broadcast(&self, exclude: u64, v: Value) {
        let _ = self.tx.send(Broadcast {
            exclude,
            text: v.to_string().into(),
        });
    }

    /// Snapshot of the open project folder.
    pub(crate) fn folder(&self) -> Folder {
        self.folder.read().clone()
    }

    pub(crate) fn project(&self) -> Project {
        self.doc.lock().project.clone()
    }

    pub(crate) fn send_all(&self, v: Value) {
        self.broadcast(0, v);
    }

    /// Accept a new project version from a client or the HTTP API.
    /// `expect_folder` (sent by UI clients) guards against edits that were
    /// made for a project that has since been closed.
    pub(crate) fn apply(
        &self,
        project: Project,
        issues: Vec<Issue>,
        origin: &str,
        exclude: u64,
        expect_folder: Option<&str>,
    ) -> Result<u64> {
        let rev = {
            // The doc lock also serializes against project switches.
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
    /// after the open project changed).
    fn welcome(&self, t: &str, client: u64) -> Value {
        let folder = self.folder();
        let doc = self.doc.lock();
        json!({
            "t": t,
            "client": client,
            "rev": doc.rev,
            "project": doc.project,
            "issues": doc.issues,
            "folder": folder.dir.display().to_string(),
            "name": folder.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "samples": folder.list_samples(),
            "native": native_status(self),
        })
    }

    /// Open another project folder: load and validate it, re-point the
    /// watcher, native engine and agent terminal, and tell every client to
    /// reload. Blocking (it restarts the agent), so call it from
    /// `spawn_blocking`.
    pub(crate) fn switch_to(&self, target: Folder) -> Result<()> {
        let _guard = self.switching.lock();
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
        {
            let mut doc = self.doc.lock();
            *self.folder.write() = target.clone();
            *doc = Doc {
                project: project.clone(),
                rev: 1,
                last_hash: folder::hash(&text),
                issues: checked.issues,
            };
        }
        self.repoint(&target, &project);
        println!("  ⇄ opened {}", target.dir.display());
        Ok(())
    }

    /// Rename the open project's folder and follow it.
    pub(crate) fn rename_open(&self, to: &std::path::Path) -> Result<()> {
        let _guard = self.switching.lock();
        let (target, project) = {
            // No edit can be written while the folder moves.
            let doc = self.doc.lock();
            let old = self.folder();
            std::fs::rename(&old.dir, to)?;
            let target = Folder::on_disk(to);
            *self.folder.write() = target.clone();
            (target, doc.project.clone())
        };
        self.repoint(&target, &project);
        println!("  ⇄ renamed the open project to {}", target.dir.display());
        Ok(())
    }

    /// After the open folder changed: watcher, status, native engine, agent
    /// terminal, and a `switched` message so every client reloads.
    fn repoint(&self, target: &Folder, project: &Project) {
        {
            let mut w = self.watcher.lock();
            if let Some((_, tx)) = w.take() {
                match make_watcher(tx.clone(), &target.dir) {
                    Ok(nw) => *w = Some((nw, tx)),
                    Err(e) => eprintln!("warning: cannot watch {}: {e}", target.dir.display()),
                }
            }
        }
        write_status(self);
        self.update_native(project);
        self.term.set_env(AgentEnv {
            dir: target.dir.clone(),
            url: self.url.clone(),
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

fn write_status(app: &App) {
    let doc = app.doc.lock();
    let ok = doc.issues.iter().all(|i| i.severity != Severity::Error);
    let v = json!({"ok": ok, "rev": doc.rev, "issues": doc.issues});
    let _ = folder::write_atomic(
        &app.folder().state_path("status.json"),
        (serde_json::to_string_pretty(&v).unwrap() + "\n").as_bytes(),
    );
}

fn write_invalid_status(app: &App, issues: &[Issue]) {
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

fn spawn_watcher(app: Shared) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let watcher = make_watcher(tx.clone(), &app.folder().dir)?;
    // Kept on the app so a project switch can replace it.
    *app.watcher.lock() = Some((watcher, tx));
    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            // Debounce bursts of events (editors write in several steps).
            tokio::time::sleep(Duration::from_millis(60)).await;
            let mut paths = vec![first];
            while let Ok(p) = rx.try_recv() {
                paths.push(p);
            }
            // Events from a project that was just closed no longer match.
            let folder = app.folder();
            let project_path = folder.project_path();
            let samples_dir = folder.dir.join(folder::SAMPLES_DIR);
            if paths.iter().any(|p| {
                p.file_name() == project_path.file_name() && p.parent() == project_path.parent()
            }) {
                reload_from_disk(&app);
            }
            if paths.iter().any(|p| p.starts_with(&samples_dir)) {
                app.broadcast(0, json!({"t": "samples", "samples": folder.list_samples()}));
            }
        }
    });
    Ok(())
}

fn reload_from_disk(app: &App) {
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
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    ws.max_message_size(64 << 20)
        .on_upgrade(move |socket| client(app, socket))
}

async fn client(app: Shared, socket: WebSocket) {
    let id = app.next_client.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let welcome = app.welcome("welcome", id);
    if sink
        .send(Message::Text(welcome.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut sub = app.tx.subscribe();
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
        let reply = handle_client_message(&app, id, v).await;
        if let Some(r) = reply {
            let _ = out_tx.send(r.to_string());
        }
    }
    send_task.abort();
}

async fn handle_client_message(app: &Shared, id: u64, v: Value) -> Option<Value> {
    let t = v.get("t").and_then(|t| t.as_str()).unwrap_or("");
    match t {
        "put" => {
            let checked =
                validate::value_and_validate(v.get("project").cloned().unwrap_or(Value::Null));
            if !checked.is_ok() {
                return Some(json!({"t": "rejected", "issues": checked.issues}));
            }
            // UI clients say which project the edit is for.
            let expect = v.get("folder").and_then(|f| f.as_str());
            match app.apply(checked.project.unwrap(), checked.issues, "ui", id, expect) {
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
                &app.folder().state_path("context.json"),
                (serde_json::to_string_pretty(&ctx).unwrap() + "\n").as_bytes(),
            );
            None
        }
        #[cfg(feature = "device-audio")]
        t if t.starts_with("native.") => crate::device::handle(app.clone(), t, &v).await,
        _ => None,
    }
}

pub(crate) fn native_status(app: &App) -> Value {
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
impl App {
    pub(crate) fn native(&self) -> &Mutex<Option<crate::device::Native>> {
        &self.native
    }
    pub(crate) fn apply_edit(&self, project: Project) {
        let issues = validate::validate(&project);
        let _ = self.apply(project, issues, "studio", 0, None);
    }
}

#[cfg(feature = "device-audio")]
fn spawn_native_status(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(33));
        loop {
            tick.tick().await;
            let msg = app.native.lock().as_ref().map(|n| n.meters_json());
            if let Some(m) = msg {
                app.broadcast(0, m);
            }
        }
    });
}

// -------------------------------------------------------------------- HTTP

async fn get_project(State(app): State<Shared>) -> impl IntoResponse {
    let text = format::to_string(&app.doc.lock().project);
    ([(header::CONTENT_TYPE, "application/json")], text)
}

async fn put_project(State(app): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
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
    match app.apply(checked.project.unwrap(), checked.issues, "api", 0, None) {
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

async fn list_samples(State(app): State<Shared>) -> impl IntoResponse {
    Json(json!({"samples": app.folder().list_samples()}))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

async fn upload_sample(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let folder = app.folder();
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

async fn get_peaks(State(app): State<Shared>, Query(q): Query<PeaksQuery>) -> Response {
    let Some(path) = app.folder().resolve(&q.path) else {
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
    State(app): State<Shared>,
    Query(q): Query<TranscribeQuery>,
) -> Response {
    let Some(path) = app.folder().resolve(&q.path) else {
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

/// Mix check (`rosaclef mixcheck` over HTTP): the request object in (the
/// command line's flags as JSON), the report out. Without `project` it
/// checks the open project.
async fn mixcheck(State(app): State<Shared>, headers: HeaderMap, body: String) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let project = app.doc.lock().project.clone();
    let folder = app.folder();
    let res = tokio::task::spawn_blocking(move || {
        let fonts = fonts();
        let env = rosaclef_studio::mixcheck::Env {
            folder: &folder,
            fonts: &fonts,
            setup: &install_plugin_host,
            progress: &|_| {},
            disk_cache: true,
        };
        rosaclef_studio::mixcheck::api(&env, &project, &body)
    })
    .await;
    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e.0).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
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
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<RenderReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let project = app.doc.lock().project.clone();
    let folder = app.folder();
    let res = tokio::task::spawn_blocking(move || -> Result<Value> {
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
        let (audio, warnings) = render_project(
            &folder,
            project,
            &scope,
            req.sample_rate.unwrap_or(48000) as f32,
            &fonts(),
            install_plugin_host,
        );
        folder::write_atomic(
            &folder.dir.join(&rel),
            &rosaclef_engine::render::encode_wav(&audio, bits),
        )?;
        let (peak_db, rms_db) = levels_db(&audio);
        Ok(json!({
            "path": rel,
            "url": format!("/files/{rel}"),
            "duration": audio.duration(),
            "peakDb": peak_db,
            "rmsDb": rms_db,
            "warnings": warnings,
        }))
    })
    .await;
    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_agents() -> impl IntoResponse {
    Json(json!({"agents": crate::terminal::presets()}))
}

async fn get_info(State(app): State<Shared>) -> impl IntoResponse {
    Json(json!({
        "folder": app.folder().dir.display().to_string(),
        "library": app.library.dir.display().to_string(),
        "url": app.url,
        "version": env!("CARGO_PKG_VERSION"),
        "native": native_status(&app),
    }))
}

// ---------------------------------------------------------------- terminal

async fn term_handler(
    State(app): State<Shared>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let term = app.term.clone();
    ws.on_upgrade(move |socket| crate::terminal::serve(term, socket))
}

// ------------------------------------------------------------------ files

/// `/files/...`: static files of the *current* project folder. Revalidated
/// on every use, since two projects may hold different files at one path.
async fn serve_file(State(app): State<Shared>, req: Request) -> Response {
    use tower::ServiceExt;
    let dir = app.folder().dir;
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
