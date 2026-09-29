//! HTTP + WebSocket server: project sync, file watching, rendering, samples,
//! plugins, native audio and the agent terminal.

use crate::folder::{self, Folder};
use crate::terminal::Terminal;
use anyhow::Result;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use rosaclef_core::validate::{self, Issue, Severity};
use rosaclef_core::{format, Project};
use rosaclef_engine::render::RenderScope;
use rosaclef_engine::Engine;
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
    pub host: String,
    pub port: u16,
    pub web: PathBuf,
}

/// Give an engine access to native plugins.
pub fn install_plugin_host(engine: &mut Engine) {
    engine.set_plugin_host(Arc::new(rosaclef_clap::ClapHost::new()));
}

/// Sequence number of `.rosaclef/context.json` writes.
static CONTEXT_SEQ: AtomicU64 = AtomicU64::new(0);

fn now_rfc3339() -> String {
    // Minimal UTC formatter (no chrono dependency).
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
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
    folder: Folder,
    doc: Mutex<Doc>,
    tx: broadcast::Sender<Broadcast>,
    next_client: AtomicU64,
    term: Arc<Terminal>,
    url: String,
    plugins: Mutex<Option<Vec<rosaclef_clap::PluginDescriptor>>>,
    #[cfg(feature = "device-audio")]
    native: Mutex<Option<crate::device::Native>>,
}

type Shared = Arc<App>;

pub async fn run(cfg: Config) -> Result<()> {
    let checked = cfg.folder.load()?;
    let text = cfg.folder.read_text()?;
    let (project, issues) = match checked.project {
        Some(p) if checked.issues.iter().all(|i| i.severity != Severity::Error) => (p, checked.issues),
        _ => {
            eprintln!("warning: {} is invalid; starting from an empty project until it is fixed:", cfg.folder.project_path().display());
            for i in &checked.issues {
                eprintln!("  {i}");
            }
            (Project::empty("Untitled"), checked.issues)
        }
    };
    let url = format!("http://{}:{}", cfg.host, cfg.port);
    let (tx, _) = broadcast::channel(256);
    let app = Arc::new(App {
        folder: cfg.folder.clone(),
        doc: Mutex::new(Doc { project, rev: 1, last_hash: folder::hash(&text), issues }),
        tx,
        next_client: AtomicU64::new(1),
        term: Arc::new(Terminal::new()),
        url: url.clone(),
        plugins: Mutex::new(None),
        #[cfg(feature = "device-audio")]
        native: Mutex::new(None),
    });
    write_status(&app);

    spawn_watcher(app.clone())?;
    #[cfg(feature = "device-audio")]
    spawn_native_status(app.clone());

    let files = ServeDir::new(&cfg.folder.dir);
    let web = ServeDir::new(&cfg.web).append_index_html_on_directories(true);
    let router = Router::new()
        .route("/ws", get(ws_handler))
        .route("/ws/term", get(term_handler))
        .route("/api/project", get(get_project).put(put_project))
        .route("/api/schema", get(|| async { ([(header::CONTENT_TYPE, "application/json")], rosaclef_core::schema::schema_text()) }))
        .route("/api/catalog", get(get_catalog))
        .route("/api/plugins", get(get_plugins))
        .route("/api/plugins/params", get(get_plugin_params))
        .route("/api/samples", get(list_samples).post(upload_sample))
        .route("/api/peaks", get(get_peaks))
        .route("/api/render", post(render))
        .route("/api/agents", get(get_agents))
        .route("/api/info", get(get_info))
        .nest_service("/files", files)
        .fallback_service(web)
        .with_state(app.clone());

    let listener = tokio::net::TcpListener::bind((cfg.host.as_str(), cfg.port)).await?;
    println!();
    println!("  ✦ Rosaclef studio");
    println!("    project  {}", cfg.folder.dir.display());
    println!("    open     {url}");
    println!();
    axum::serve(listener, router).await?;
    Ok(())
}

// ------------------------------------------------------------------ helpers

/// Reject cross-site requests (a web page on another origin must not be able
/// to drive the terminal or rewrite the project).
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) else { return true };
    let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return false };
    let origin_host = origin.split("://").nth(1).unwrap_or("");
    origin_host == host
}

fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "cross-origin request refused").into_response()
}

impl App {
    fn broadcast(&self, exclude: u64, v: Value) {
        let _ = self.tx.send(Broadcast { exclude, text: v.to_string().into() });
    }

    /// Accept a new project version from a client or the HTTP API.
    fn apply(&self, project: Project, issues: Vec<Issue>, origin: &str, exclude: u64) -> Result<u64> {
        let text = self.folder.write_project(&project)?;
        let rev = {
            let mut doc = self.doc.lock();
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

    #[cfg(feature = "device-audio")]
    fn update_native(&self, project: &Project) {
        if let Some(n) = self.native.lock().as_ref() {
            n.set_project(project.clone(), &self.folder);
        }
    }

    #[cfg(not(feature = "device-audio"))]
    fn update_native(&self, _project: &Project) {}
}

fn write_status(app: &App) {
    let doc = app.doc.lock();
    let ok = doc.issues.iter().all(|i| i.severity != Severity::Error);
    let v = json!({"ok": ok, "rev": doc.rev, "issues": doc.issues});
    let _ = folder::write_atomic(&app.folder.state_path("status.json"), (serde_json::to_string_pretty(&v).unwrap() + "\n").as_bytes());
}

fn write_invalid_status(app: &App, issues: &[Issue]) {
    let rev = app.doc.lock().rev;
    let v = json!({
        "ok": false,
        "rev": rev,
        "note": "project.json on disk is invalid; the studio keeps using the last valid version until it is fixed",
        "issues": issues
    });
    let _ = folder::write_atomic(&app.folder.state_path("status.json"), (serde_json::to_string_pretty(&v).unwrap() + "\n").as_bytes());
}

// ---------------------------------------------------------------- watching

fn spawn_watcher(app: Shared) -> Result<()> {
    use notify::{RecursiveMode, Watcher};
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })?;
    watcher.watch(&app.folder.dir, RecursiveMode::Recursive)?;
    tokio::spawn(async move {
        let _keep = watcher;
        let project_path = app.folder.project_path();
        let samples_dir = app.folder.dir.join(folder::SAMPLES_DIR);
        while let Some(first) = rx.recv().await {
            // Debounce bursts of events (editors write in several steps).
            tokio::time::sleep(Duration::from_millis(60)).await;
            let mut paths = vec![first];
            while let Ok(p) = rx.try_recv() {
                paths.push(p);
            }
            if paths.iter().any(|p| p.file_name() == project_path.file_name() && p.parent() == project_path.parent()) {
                reload_from_disk(&app);
            }
            if paths.iter().any(|p| p.starts_with(&samples_dir)) {
                app.broadcast(0, json!({"t": "samples", "samples": app.folder.list_samples()}));
            }
        }
    });
    Ok(())
}

fn reload_from_disk(app: &App) {
    let Ok(text) = app.folder.read_text() else { return };
    let h = folder::hash(&text);
    if app.doc.lock().last_hash == h {
        return;
    }
    let checked = validate::parse_and_validate(&text);
    if checked.is_ok() {
        let project = checked.project.unwrap();
        let rev = {
            let mut doc = app.doc.lock();
            doc.last_hash = h;
            if doc.project == project {
                return;
            }
            doc.rev += 1;
            doc.project = project.clone();
            doc.issues = checked.issues.clone();
            doc.rev
        };
        println!("  ↻ project.json changed on disk (rev {rev})");
        app.broadcast(0, json!({"t": "project", "rev": rev, "origin": "disk", "project": project, "issues": checked.issues}));
        write_status(app);
        app.update_native(&project);
    } else {
        app.doc.lock().last_hash = h;
        println!("  ✗ project.json on disk is invalid ({} issue(s))", checked.issues.len());
        app.broadcast(0, json!({"t": "invalid", "issues": checked.issues}));
        write_invalid_status(app, &checked.issues);
    }
}

// --------------------------------------------------------------- websocket

async fn ws_handler(State(app): State<Shared>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    ws.max_message_size(64 << 20).on_upgrade(move |socket| client(app, socket))
}

async fn client(app: Shared, socket: WebSocket) {
    let id = app.next_client.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let welcome = {
        let doc = app.doc.lock();
        json!({
            "t": "welcome",
            "client": id,
            "rev": doc.rev,
            "project": doc.project,
            "issues": doc.issues,
            "folder": app.folder.dir.display().to_string(),
            "samples": app.folder.list_samples(),
            "native": native_status(&app),
        })
    };
    if sink.send(Message::Text(welcome.to_string().into())).await.is_err() {
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
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
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
            let checked = validate::value_and_validate(v.get("project").cloned().unwrap_or(Value::Null));
            if !checked.is_ok() {
                return Some(json!({"t": "rejected", "issues": checked.issues}));
            }
            match app.apply(checked.project.unwrap(), checked.issues, "ui", id) {
                Ok(rev) => Some(json!({"t": "ack", "rev": rev})),
                Err(e) => Some(json!({"t": "error", "message": e.to_string()})),
            }
        }
        "context" => {
            let mut ctx = rosaclef_core::context::normalize(v.get("context").cloned().unwrap_or(Value::Null));
            ctx.seq = CONTEXT_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
            ctx.updated_at = now_rfc3339();
            for e in &mut ctx.recent_edits {
                if e.at.is_empty() {
                    e.at = ctx.updated_at.clone();
                }
            }
            let _ = folder::write_atomic(&app.folder.state_path("context.json"), (serde_json::to_string_pretty(&ctx).unwrap() + "\n").as_bytes());
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
    pub(crate) fn folder(&self) -> &Folder {
        &self.folder
    }
    pub(crate) fn project(&self) -> Project {
        self.doc.lock().project.clone()
    }
    pub(crate) fn native(&self) -> &Mutex<Option<crate::device::Native>> {
        &self.native
    }
    pub(crate) fn send_all(&self, v: Value) {
        self.broadcast(0, v);
    }
    pub(crate) fn apply_edit(&self, project: Project) {
        let issues = validate::validate(&project);
        let _ = self.apply(project, issues, "studio", 0);
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
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"ok": false, "issues": checked.issues}))).into_response();
    }
    match app.apply(checked.project.unwrap(), checked.issues, "api", 0) {
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
    Json(json!({"devices": rosaclef_core::catalog::DEVICES, "presets": rosaclef_core::presets::all(), "plugins": plugins}))
}

#[derive(Deserialize)]
struct PluginsQuery {
    rescan: Option<bool>,
}

async fn get_plugins(State(app): State<Shared>, Query(q): Query<PluginsQuery>) -> impl IntoResponse {
    let rescan = q.rescan.unwrap_or(false);
    let plugins = tokio::task::spawn_blocking(move || cached_plugins(&app, rescan)).await.unwrap_or_default();
    Json(json!({"plugins": plugins}))
}

#[derive(Deserialize)]
struct ParamsQuery {
    path: String,
    id: Option<String>,
}

async fn get_plugin_params(Query(q): Query<ParamsQuery>) -> Response {
    let res = tokio::task::spawn_blocking(move || rosaclef_clap::params(&q.path, q.id.as_deref().unwrap_or(""))).await;
    match res {
        Ok(Ok(params)) => Json(json!({"params": params})).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_samples(State(app): State<Shared>) -> impl IntoResponse {
    Json(json!({"samples": app.folder.list_samples()}))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

pub(crate) fn unique_sample_path(folder: &Folder, name: &str) -> String {
    let clean: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("sample.wav")
        .chars()
        .map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' })
        .collect();
    let clean = if clean.trim_start_matches('.').is_empty() { "sample.wav".to_string() } else { clean };
    let (stem, ext) = match clean.rfind('.') {
        Some(i) => (clean[..i].to_string(), clean[i..].to_string()),
        None => (clean.clone(), String::new()),
    };
    let mut rel = format!("{}/{stem}{ext}", folder::SAMPLES_DIR);
    let mut n = 2;
    while folder.dir.join(&rel).exists() {
        rel = format!("{}/{stem}-{n}{ext}", folder::SAMPLES_DIR);
        n += 1;
    }
    rel
}

async fn upload_sample(State(app): State<Shared>, headers: HeaderMap, Query(q): Query<UploadQuery>, body: Bytes) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let rel = unique_sample_path(&app.folder, &q.name);
    let path = app.folder.dir.join(&rel);
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
    let Some(path) = app.folder.resolve(&q.path) else {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    };
    let n = q.n.unwrap_or(1024);
    let res = tokio::task::spawn_blocking(move || crate::decode::decode_file(&path).map(|d| (d.duration(), d.sample_rate, crate::decode::peaks(&d, n)))).await;
    match res {
        Ok(Ok((duration, sr, peaks))) => Json(json!({"duration": duration, "sampleRate": sr, "peaks": peaks})).into_response(),
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
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

async fn render(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<RenderReq>) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let project = app.doc.lock().project.clone();
    let folder = app.folder.clone();
    let res = tokio::task::spawn_blocking(move || -> Result<Value> {
        let scope = match req.pattern.clone().filter(|p| !p.is_empty()) {
            Some(id) => RenderScope::Pattern { id, loops: req.loops.unwrap_or(1) },
            None => RenderScope::Song,
        };
        let bits = req.bits.filter(|b| [16, 24, 32].contains(b)).unwrap_or(24);
        let title = crate::slug(&project.meta.title);
        let name = match &req.pattern {
            Some(p) if !p.is_empty() => format!("{title}-{p}"),
            _ => title,
        };
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let rel = format!("{}/{name}-{stamp}.wav", folder::RENDERS_DIR);
        let (audio, warnings) = crate::render_project(&folder, project, &scope, req.sample_rate.unwrap_or(48000) as f32);
        folder::write_atomic(&folder.dir.join(&rel), &rosaclef_engine::render::encode_wav(&audio, bits))?;
        Ok(json!({
            "path": rel,
            "url": format!("/files/{rel}"),
            "duration": audio.duration(),
            "peakDb": 20.0 * audio.peak().max(1e-9).log10(),
            "rmsDb": 20.0 * audio.rms().max(1e-9).log10(),
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
        "folder": app.folder.dir.display().to_string(),
        "url": app.url,
        "version": env!("CARGO_PKG_VERSION"),
        "native": native_status(&app),
    }))
}

// ---------------------------------------------------------------- terminal

async fn term_handler(State(app): State<Shared>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let env = crate::terminal::AgentEnv { dir: app.folder.dir.clone(), url: app.url.clone() };
    let term = app.term.clone();
    ws.on_upgrade(move |socket| crate::terminal::serve(term, env, socket))
}
