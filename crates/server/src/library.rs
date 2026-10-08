//! HTTP routes of the project library and the file manager. The logic lives
//! in `rosaclef_studio::library` (shared with the browser build); this module
//! adds what only the server does: opening projects (each tab may have its
//! own), following an open project that is renamed, and serving requests.
//! Routes about the open project act on the request's project (its
//! [`Scope`]); library routes name projects of the library, never paths.

use crate::folder::Folder;
use crate::server::{forbidden, same_origin, Scope, Shared};
use anyhow::{anyhow, bail, Result};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path as UrlPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rosaclef_studio::library::{self, rewrite_refs};
use rosaclef_studio::{archive, slug};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

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
        .route("/api/projects/import-zip", post(import_zip))
        .route("/api/projects/export", get(export_project))
        .route("/api/projects/{name}", delete(delete_project))
        .route("/api/import-midi", post(import_midi_into))
        .route("/api/files", get(list_files).delete(delete_file))
        .route("/api/files/rename", post(rename_file))
        .route("/api/trash/empty", post(empty_trash))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
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
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| anyhow!("task failed: {e}"))?
}

/// Whether a project of the library is open (in any tab, or for its agent).
fn is_open(app: &Shared, dir: &Path) -> bool {
    app.open_at(&dir.canonicalize().unwrap_or(dir.to_path_buf()))
        .is_some()
}

async fn list_projects(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
) -> Response {
    let cur = proj.folder().dir;
    let (library, current) = (app.library.clone(), cur.clone());
    let res = blocking(move || Ok(library.list(&current))).await;
    match res {
        Ok(projects) => Json(json!({
            "library": app.library.dir.display().to_string(),
            "current": cur.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "currentFolder": cur.display().to_string(),
            "projects": projects,
            "demoTitle": rosaclef_studio::folder::demo_title(),
        }))
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateReq {
    name: String,
    #[serde(default)]
    demo: bool,
}

async fn create_project(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<CreateReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = req.name.trim().to_string();
    match blocking(move || library.create(&name, req.demo).map(|f| f.name())).await {
        Ok(name) => Json(json!({"name": name})).into_response(),
        Err(e) if e.to_string().contains("already exists") => conflict(e),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct OpenReq {
    /// A project of the library, by name…
    name: Option<String>,
    /// …or by its folder (a project of the library, or one open already).
    path: Option<String>,
}

/// `POST /api/projects/open`: open a project (or find it open) for the tab
/// asking. The answer's `session` scopes the tab's requests to it
/// (`/s/{session}/...`); other tabs keep their own projects.
async fn open_project(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<OpenReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let dir = match app.openable(req.name.as_deref(), req.path.as_deref()) {
        Ok(d) => d,
        Err(e) => return bad(e),
    };
    let a = app.clone();
    match blocking(move || a.open_dir(&dir)).await {
        Ok(p) => {
            let folder = p.folder();
            Json(json!({
                "ok": true,
                "session": p.key,
                "folder": folder.dir.display().to_string(),
                "name": folder.name(),
            }))
            .into_response()
        }
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct RenameReq {
    name: String,
    to: String,
}

async fn duplicate_project(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<RenameReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let (name, to) = (req.name.clone(), req.to.trim().to_string());
    let to2 = to.clone();
    match blocking(move || library.duplicate(&name, &to2)).await {
        Ok(()) => Json(json!({"name": to})).into_response(),
        Err(e) if e.to_string().contains("already exists") => conflict(e),
        Err(e) => bad(e),
    }
}

async fn rename_project(
    State(app): State<Shared>,
    headers: HeaderMap,
    Json(req): Json<RenameReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    let (name, to) = (req.name.clone(), req.to.trim().to_string());
    let to2 = to.clone();
    let res = blocking(move || {
        let src = a.library.project_dir(&name)?;
        let dst = a.library.checked_target(&to2)?;
        let src_canon = src.canonicalize().unwrap_or(src.clone());
        if let Some(open) = a.open_at(&src_canon) {
            // An open project: move it and follow it (agent, watcher, clients).
            a.rename_open(&open, &dst)?;
            let mut p = open.project();
            if p.meta.title == name {
                p.meta.title = to2.clone();
                let issues = rosaclef_core::validate::validate(&p);
                open.apply(p, issues, "files", 0, None)?;
            }
            Ok(())
        } else {
            a.library.rename(&name, &to2)
        }
    })
    .await;
    match res {
        Ok(()) => Json(json!({"name": to})).into_response(),
        Err(e) if e.to_string().contains("already exists") => conflict(e),
        Err(e) => bad(e),
    }
}

async fn delete_project(
    State(app): State<Shared>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    let res = blocking(move || {
        let dir = a.library.project_dir(&name)?;
        if is_open(&a, &dir) {
            bail!("{name:?} is open; close its tabs (and its agent) before deleting it");
        }
        a.library.trash(&name)
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
        if let Some(n) = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            return n.to_string();
        }
        let stem = self
            .filename
            .as_deref()
            .and_then(|f| Path::new(f).file_stem())
            .map(|s| s.to_string_lossy().to_string());
        stem.unwrap_or_else(|| fallback.to_string())
    }
}

fn import_reply(res: Result<(String, Vec<String>)>) -> Response {
    match res {
        Ok((name, warnings)) => Json(json!({"name": name, "warnings": warnings})).into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")).into_response(),
    }
}

async fn import_lmms(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = library::sanitize_name(&q.project_name("LMMS import"));
    import_reply(
        blocking(move || {
            let im =
                rosaclef_import::lmms::import(&body, &rosaclef_import::lmms::Options::new(&name))?;
            library.save_imported(&name, &im)
        })
        .await,
    )
}

async fn import_midi(
    State(app): State<Shared>,
    scope: Extension<Scope>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    if q.into.as_deref() == Some("current") {
        return import_midi_into(scope, headers, Query(q), body).await;
    }
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = library::sanitize_name(&q.project_name("MIDI import"));
    import_reply(
        blocking(move || {
            let im =
                rosaclef_import::midi::import(&body, &rosaclef_import::midi::Options::new(&name))?;
            library.save_imported(&name, &im)
        })
        .await,
    )
}

/// `POST /api/projects/import-zip`: a project folder packed as a zip archive
/// (see `GET /api/projects/export`) becomes a new project.
async fn import_zip(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let library = app.library.clone();
    let name = q.name.clone().unwrap_or_default();
    import_reply(blocking(move || archive::import(&library, &name, &body)).await)
}

#[derive(Deserialize)]
struct ExportQuery {
    /// A project of the library (default: the request's project).
    name: Option<String>,
}

/// `GET /api/projects/export?name=`: the project folder as a zip archive.
async fn export_project(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let folder = match q.name.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => match app.library.project_dir(n) {
            Ok(d) => Folder::on_disk(d),
            Err(e) => return bad(e),
        },
        None => proj.folder(),
    };
    let file = format!("{}.zip", slug(&folder.name()));
    match blocking(move || archive::export(&folder)).await {
        Ok(zip) => (
            [
                (header::CONTENT_TYPE, "application/zip".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{file}\""),
                ),
            ],
            zip,
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
}

/// `POST /api/import-midi?into=current`: add a MIDI file's parts to the
/// open song as new channels, patterns, tracks and inserts (one undo step).
async fn import_midi_into(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    if q.into.as_deref().unwrap_or("current") != "current" {
        return bad("only into=current is supported");
    }
    let a = proj.clone();
    let res = blocking(move || {
        let im =
            rosaclef_import::midi::import(&body, &rosaclef_import::midi::Options::new("import"))?;
        let mut project = a.project();
        let before = project.channels.len();
        let mut warnings = im.warnings;
        warnings.extend(rosaclef_import::merge_into(&mut project, im.project));
        let issues = rosaclef_core::validate::validate(&project);
        if issues
            .iter()
            .any(|i| i.severity == rosaclef_core::validate::Severity::Error)
        {
            let msgs: Vec<String> = issues.iter().take(3).map(|i| i.to_string()).collect();
            bail!("the merged project would be invalid:\n{}", msgs.join("\n"));
        }
        let added = project.channels.len() - before;
        a.apply(project, issues, "import", 0, None)?;
        Ok((added, warnings))
    })
    .await;
    match res {
        Ok((channels, warnings)) => {
            Json(json!({"channels": channels, "warnings": warnings})).into_response()
        }
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")).into_response(),
    }
}

#[derive(Deserialize)]
struct FilesQuery {
    dir: Option<String>,
}

async fn list_files(
    Extension(Scope(proj)): Extension<Scope>,
    Query(q): Query<FilesQuery>,
) -> Response {
    let (f, project) = (proj.folder(), proj.project());
    let folder = f.dir.display().to_string();
    let res = blocking(move || {
        library::files(
            &f,
            q.dir.as_deref().unwrap_or("").trim_matches('/'),
            &project,
        )
    })
    .await;
    match res {
        Ok(list) => Json(json!({"folder": folder, "files": list})).into_response(),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct FileRenameReq {
    path: String,
    to: String,
}

async fn rename_file(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Json(req): Json<FileRenameReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = proj.clone();
    let res = blocking(move || {
        let _guard = a.busy.lock();
        let new_rel = library::rename_path(&a.folder(), &req.path, &req.to)?;
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

async fn delete_file(
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Query(q): Query<PathQuery>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = proj.clone();
    match blocking(move || library::trash_path(&a.folder(), &q.path)).await {
        Ok(p) => Json(json!({"trashed": p})).into_response(),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct TrashReq {
    /// `library` (deleted projects) or `project` (the request's project's deleted files).
    scope: String,
}

/// `POST /api/trash/empty`: delete the trash for good.
async fn empty_trash(
    State(app): State<Shared>,
    Extension(Scope(proj)): Extension<Scope>,
    headers: HeaderMap,
    Json(req): Json<TrashReq>,
) -> Response {
    if !same_origin(&headers) {
        return forbidden();
    }
    let a = app.clone();
    let res = blocking(move || match req.scope.as_str() {
        "library" => a.library.empty_trash(),
        "project" => library::empty_trash(&proj.folder()),
        s => bail!("unknown trash {s:?} (library or project)"),
    })
    .await;
    match res {
        Ok(n) => Json(json!({"removed": n})).into_response(),
        Err(e) => bad(e),
    }
}
