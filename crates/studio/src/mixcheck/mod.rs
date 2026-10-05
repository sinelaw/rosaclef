//! `rosaclef mixcheck`: fast, numeric mix diagnostics for agents and
//! producers — loudness, peaks and the limiter, masking and audibility,
//! gain reduction, harmonic clashes, the spectrum and the stereo image, over
//! any range of the song, from **one render**.
//!
//! The same request ([`Options`], read from JSON) and the same report
//! ([`Report`], `mixcheck.schema.json`) serve every interface: the command
//! line, `POST /api/mixcheck` (native server and browser-only studio) and the
//! studio's Mix check panel. See docs/mixcheck.md.
//!
//! - [`timeline`]: bars, passes of repeats, sections, ranges → segments.
//! - [`analyze`]: the tapped render with pre-roll and the measurements.
//! - [`cache`]: renders keyed on a hash of what makes the sound.
//! - [`model`]: loudness, attribution of the mix, the masking model.
//! - [`clashes`]: harmonic clashes from the notes and the levels.
//! - [`report`]: the report and how it is computed.
//! - [`findings`]: the rules, their fixes, and suggestions.
//! - [`targets`], [`reference`]: delivery targets, A/B against a recording.
//! - [`diff`]: what-if and compare deltas. [`patch`]: RFC 6902 in memory.
//! - [`text`]: the ≤ 40-line summary.

pub mod analyze;
pub mod cache;
pub mod clashes;
pub mod critic;
pub mod diff;
pub mod dsp;
pub mod findings;
pub mod model;
pub mod options;
pub mod patch;
pub mod reference;
pub mod report;
pub mod targets;
pub mod text;
pub mod timeline;

pub use options::Options;
pub use report::Report;

use crate::folder::Folder;
use crate::fonts::Fonts;
use analyze::{Analysis, EngineFactory};
use rosaclef_core::Project;
use rosaclef_engine::samples::SampleData;
use rosaclef_engine::Engine;
use serde_json::{json, Value};
use std::sync::Arc;

/// The JSON Schema of the report.
pub const SCHEMA: &str = include_str!("mixcheck.schema.json");

/// Where a mix check runs: the project's folder (samples, the cache), the
/// soundfonts, and what prepares each engine (the native plugin host).
pub struct Env<'a> {
    pub folder: &'a Folder,
    pub fonts: &'a Fonts,
    pub setup: &'a dyn Fn(&mut Engine),
    /// Told how far a render has come (0 … 1).
    pub progress: &'a dyn Fn(f64),
    /// Keep renders on disk too (`.rosaclef/mixcheck/`), not only in memory.
    pub disk_cache: bool,
}

/// An error, naming the JSON path or the option at fault.
#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error(s)
    }
}

/// The pre-roll: 8 beats, and at least 3 seconds (reverb tails, held
/// notes and compressors settle before the range starts).
fn preroll(p: &Project, o: &Options) -> f64 {
    o.preroll
        .unwrap_or_else(|| (3.0 * p.transport.bpm / 60.0).clamp(8.0, 32.0).ceil())
}

struct Factory<'a> {
    env: &'a Env<'a>,
    project: &'a Project,
    sr: f32,
    samples: Vec<(String, SampleData)>,
    warnings: Vec<String>,
}

impl EngineFactory for Factory<'_> {
    fn make(&mut self) -> Engine {
        let mut e = Engine::new(self.sr);
        e.set_parallel(true);
        (self.env.setup)(&mut e);
        e.set_project(self.project.clone());
        for (path, data) in &self.samples {
            e.set_sample(path, data.clone());
        }
        let w = self.env.fonts.provide(&mut e);
        if self.warnings.is_empty() {
            self.warnings.extend(w);
        }
        e
    }
}

/// Render and measure (or find in the cache) what `o` asks of `project`.
pub fn measure(
    env: &Env,
    project: &Project,
    o: &Options,
) -> Result<(Arc<Analysis>, timeline::Timeline, timeline::Resolved, bool), Error> {
    let t = timeline::Timeline::new(project);
    let res = timeline::resolve(
        project,
        &t,
        o.range.as_deref(),
        o.beats.as_deref(),
        o.section.as_deref(),
    )?;
    let segments = t.segments(&res.ranges);
    if segments.is_empty() {
        return Err(Error("the range holds nothing that plays".into()));
    }
    let pre = preroll(project, o);
    let samples = crate::render::required_samples(project);
    let key = cache::key(
        Some(env.folder),
        project,
        &samples,
        &segments,
        pre,
        o.sample_rate,
    );
    if o.cache {
        if let Some(a) = cache::remembered(&key) {
            return Ok((a, t, res, true));
        }
        if let Some(a) = cache::load(env.folder, &key).filter(|_| env.disk_cache) {
            let a = Arc::new(a);
            cache::remember(&key, a.clone());
            return Ok((a, t, res, true));
        }
    }
    let mut warnings = vec![];
    let mut decoded = vec![];
    for path in samples {
        match env
            .folder
            .resolve(&path)
            .ok_or_else(|| anyhow::anyhow!("invalid path"))
            .and_then(|p| crate::decode::decode_file(env.folder.fs.as_ref(), &p))
        {
            Ok(d) => decoded.push((path, d)),
            Err(e) => warnings.push(format!("sample {path}: {e}")),
        }
    }
    let mut f = Factory {
        env,
        project,
        sr: o.sample_rate,
        samples: decoded,
        warnings: vec![],
    };
    let mut a = analyze::run(
        project,
        &t,
        &segments,
        pre,
        o.sample_rate,
        &mut f,
        env.progress,
    );
    a.warnings.extend(warnings);
    a.warnings.extend(f.warnings);
    let a = Arc::new(a);
    if o.cache {
        if env.disk_cache {
            cache::store(env.folder, &key, &a);
        }
        cache::remember(&key, a.clone());
    }
    Ok((a, t, res, false))
}

/// One report, from one render (or the cache).
fn report_of(env: &Env, project: &Project, o: &Options) -> Result<Report, Error> {
    let t0 = env.folder.fs.now_ms();
    let (a, t, res, cached) = measure(env, project, o)?;
    let (mut r, ctx) = report::build(&a, project, &t, &res, o)?;
    r.findings = findings::derive(&r, &ctx, o);
    if let Some(path) = &o.reference {
        let (m, secs) = reference::measure(env.folder, path)?;
        r.reference = Some(reference::compare(path, &r.master, &m, secs));
    }
    r.render.cached = cached;
    r.render.renders = if cached { 0 } else { 1 };
    r.render.ms = (env.folder.fs.now_ms() - t0).max(0.0) as u64;
    Ok(r)
}

/// Apply JSON Patch operations to a project, in memory; the result must be
/// a valid project (errors name the JSON path).
pub fn patched(project: &Project, ops: &[Value], what: &str) -> Result<Project, Error> {
    let mut doc = serde_json::to_value(project).map_err(|e| Error(e.to_string()))?;
    patch::apply(&mut doc, ops, what)?;
    match rosaclef_core::compat::value_for_playback(doc) {
        Ok(p) => Ok(p.project),
        Err(checked) => {
            let msgs: Vec<String> = checked.errors().map(|i| i.to_string()).collect();
            Err(Error(format!(
                "{what} leaves the project invalid: {}",
                msgs.join("; ")
            )))
        }
    }
}

/// Run a mix check: what `o` asks, with its what-if patch and the
/// verification of the suggestions.
pub fn run(env: &Env, project: &Project, o: &Options) -> Result<Report, Error> {
    let t0 = env.folder.fs.now_ms();
    let mut r = if o.what_if.is_empty() {
        report_of(env, project, o)?
    } else {
        let base = report_of(env, project, o)?;
        let p2 = patched(project, &o.what_if, "whatIf")?;
        let mut r = report_of(env, &p2, o)?;
        let mut d = diff::diff(&base, &r);
        if let Some(m) = d.as_object_mut() {
            m.insert("ops".into(), json!(o.what_if.len()));
        }
        r.what_if = Some(d);
        r.render.renders += base.render.renders;
        r.render.cached &= base.render.cached;
        r
    };
    let p_now = if o.what_if.is_empty() {
        project.clone()
    } else {
        patched(project, &o.what_if, "whatIf")?
    };
    if o.verify {
        verify(env, &p_now, o, &mut r)?;
    }
    r.render.ms = (env.folder.fs.now_ms() - t0).max(0.0) as u64;
    Ok(r)
}

/// Re-measure each suggestion under its own patch (one render each, at
/// most `max_findings`).
fn verify(env: &Env, project: &Project, o: &Options, r: &mut Report) -> Result<(), Error> {
    let mut left = o.max_findings.max(1);
    for e in &mut r.elements {
        for s in &mut e.suggestions {
            if left == 0 {
                return Ok(());
            }
            left -= 1;
            let Ok(p2) = patched(project, &s.patch, "suggestion") else {
                s.verified = Some(json!({"error": "the patch does not apply"}));
                continue;
            };
            let o2 = Options {
                focus: vec![e.id.clone()],
                checks: vec![options::Check::Levels, options::Check::Audibility],
                what_if: vec![],
                verify: false,
                history: false,
                reference: None,
                target: None,
                ..o.clone()
            };
            let r2 = report_of(env, &p2, &o2)?;
            r.render.renders += r2.render.renders;
            s.verified = Some(match r2.elements.iter().find(|x| x.id == e.id) {
                Some(x) => json!({
                    "relativeToMixDb": x.relative_to_mix_db.flatten(),
                    "audibleFractionPct": x.audibility.as_ref().map(|a| a.audible_fraction_pct),
                    "verdict": x.verdict,
                }),
                None => json!({"verdict": "silent"}),
            });
        }
    }
    Ok(())
}

/// Compare two projects: the report of `b`, with what changed from `a`.
pub fn compare(
    env: &Env,
    a: &Project,
    b: &Project,
    labels: (&str, &str),
    o: &Options,
) -> Result<Report, Error> {
    let ra = run(env, a, o)?;
    let mut rb = run(env, b, o)?;
    let mut d = diff::diff(&ra, &rb);
    if let Some(m) = d.as_object_mut() {
        m.insert("a".into(), json!(labels.0));
        m.insert("b".into(), json!(labels.1));
    }
    rb.compare = Some(d);
    rb.render.renders += ra.render.renders;
    Ok(rb)
}

/// Whether a report has warnings (exit status 1).
pub fn has_warnings(r: &Report) -> bool {
    r.findings.iter().any(|f| f.severity == "warn")
}

/// A request object (the endpoint's body) run on `current` (unless it
/// carries `project`): the report, and whether it asks for the text too.
pub fn run_request(env: &Env, current: &Project, body: &str) -> Result<(Report, bool), Error> {
    let req: Value = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(body).map_err(|e| Error(format!("not JSON: {e}")))?
    };
    let o = Options::from_json(&req)?;
    let project = request_project(current, body)?;
    let r = run(env, &project, &o)?;
    Ok((r, req.get("text").and_then(|t| t.as_bool()) == Some(true)))
}

/// The HTTP endpoint (`POST /api/mixcheck`): the request object in, the
/// report out (`"text": true` adds the summary as `text`). With `apply`
/// (JSON Patch operations, such as a finding's `fix`), it renders nothing:
/// it returns `{"project": …}`, the project patched and validated — how the
/// studio applies a fix.
pub fn api(env: &Env, current: &Project, body: &str) -> Result<Value, Error> {
    if let Some(p) = apply_request(current, body)? {
        return Ok(json!({ "project": p }));
    }
    let (r, text) = run_request(env, current, body)?;
    let mut v = serde_json::to_value(&r).map_err(|e| Error(e.to_string()))?;
    if text {
        if let Some(m) = v.as_object_mut() {
            m.insert("text".into(), json!(text::summary(&r)));
        }
    }
    Ok(v)
}

/// A request's `apply`: the project patched (None without `apply`).
pub fn apply_request(current: &Project, body: &str) -> Result<Option<Project>, Error> {
    let req: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let Some(ops) = req.get("apply") else {
        return Ok(None);
    };
    let ops = options::ops_of(ops, "apply")?;
    let project = request_project(current, body)?;
    patched(&project, &ops, "apply").map(Some)
}

/// What a client can ask (GET /api/mixcheck): the checks, the delivery
/// targets, and the report's schema.
pub fn catalog() -> Value {
    json!({
        "checks": options::CHECKS.iter().map(|c| c.0).collect::<Vec<_>>(),
        "targets": targets::TARGETS.iter().map(|t| json!({
            "id": t.id, "name": t.name, "lufs": t.lufs, "truePeakDbtp": t.true_peak, "tolerance": t.tolerance,
        })).collect::<Vec<_>>(),
        "rules": findings::RULES,
        "schema": serde_json::from_str::<Value>(SCHEMA).unwrap_or(Value::Null),
    })
}

/// The project a request is about (`project` in it, or `current`), and the
/// samples it plays: for hosts that load files lazily.
pub fn request_project(current: &Project, body: &str) -> Result<Project, Error> {
    let req: Value = if body.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(body).map_err(|e| Error(format!("not JSON: {e}")))?
    };
    match req.get("project") {
        Some(v) if !v.is_null() => rosaclef_core::compat::value_for_playback(v.clone())
            .map(|p| p.project)
            .map_err(|c| {
                Error(format!(
                    "the project is invalid: {}",
                    c.errors()
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                ))
            }),
        _ => Ok(current.clone()),
    }
}
