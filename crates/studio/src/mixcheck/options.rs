//! What a mix check is asked: one set of options for every interface. The
//! command line turns its flags into the same JSON object the HTTP endpoint
//! takes (`POST /api/mixcheck`) and the studio's Mix check panel sends, and
//! [`Options::from_json`] reads it — so every interface has the same
//! capabilities and gets the same report.

use serde_json::Value;

/// What the report covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Levels,
    Audibility,
    Masking,
    Dynamics,
    GainReduction,
    Clashes,
    Spectrum,
    Stereo,
}

pub const CHECKS: [(&str, Check); 8] = [
    ("levels", Check::Levels),
    ("audibility", Check::Audibility),
    ("masking", Check::Masking),
    ("dynamics", Check::Dynamics),
    ("gainreduction", Check::GainReduction),
    ("clashes", Check::Clashes),
    ("spectrum", Check::Spectrum),
    ("stereo", Check::Stereo),
];

/// The report's rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum By {
    Bar,
    Section,
    /// Every N written beats.
    Beats(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threshold {
    Strict,
    Normal,
    Loose,
}

impl Threshold {
    pub fn name(self) -> &'static str {
        match self {
            Threshold::Strict => "strict",
            Threshold::Normal => "normal",
            Threshold::Loose => "loose",
        }
    }
    /// How far under the mix (LU) the lead may sit before it is buried,
    /// however audible.
    pub fn lead_floor_db(self) -> f64 {
        match self {
            Threshold::Strict => -8.0,
            Threshold::Normal => -10.0,
            Threshold::Loose => -13.0,
        }
    }
}

/// The most findings a report lists.
pub const MAX_FINDINGS: usize = 100;

#[derive(Clone, Debug)]
pub struct Options {
    /// `--range BAR:BAR` (bars from 1, both included).
    pub range: Option<String>,
    /// `--beats B:B` (written song beats).
    pub beats: Option<String>,
    /// `--section NAME`.
    pub section: Option<String>,
    pub by: By,
    /// Channel ids, insert indices or names, `master`.
    pub focus: Vec<String>,
    pub checks: Vec<Check>,
    /// JSON Patch operations applied in memory before rendering.
    pub what_if: Vec<Value>,
    pub threshold: Threshold,
    pub max_findings: usize,
    /// Re-measure each suggestion under its patch (one render each).
    pub verify: bool,
    /// The master's loudness over time (every 200 ms).
    pub history: bool,
    /// A delivery target (spotify, apple, youtube, ebu-r128, …).
    pub target: Option<String>,
    /// A reference recording to compare with (level-matched).
    pub reference: Option<String>,
    pub preroll: Option<f64>,
    pub sample_rate: f32,
    /// Use (and fill) the render cache.
    pub cache: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            range: None,
            beats: None,
            section: None,
            by: By::Bar,
            focus: vec![],
            checks: CHECKS.iter().map(|c| c.1).collect(),
            what_if: vec![],
            threshold: Threshold::Normal,
            max_findings: 10,
            verify: false,
            history: false,
            target: None,
            reference: None,
            preroll: None,
            sample_rate: 48000.0,
            cache: true,
        }
    }
}

const FIELDS: &[&str] = &[
    "range",
    "beats",
    "section",
    "by",
    "focus",
    "checks",
    "whatIf",
    "threshold",
    "maxFindings",
    "verify",
    "history",
    "target",
    "reference",
    "prerollBeats",
    "sampleRate",
    "cache",
    "project",
    "text",
    "apply",
];

/// A list given as an array of strings or one comma-separated string.
fn list(v: &Value, key: &str) -> Result<Vec<String>, String> {
    match v {
        Value::String(s) => Ok(s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()),
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::String(s) => Ok(s.trim().to_string()),
                Value::Number(n) => Ok(n.to_string()),
                _ => Err(format!("{key}: expected strings")),
            })
            .collect(),
        Value::Null => Ok(vec![]),
        _ => Err(format!("{key}: expected a list")),
    }
}

impl Options {
    /// Read a request object (`{"range": "52:59", "by": "bar", "focus": ["rbass"], …}`).
    pub fn from_json(v: &Value) -> Result<Options, String> {
        let Some(m) = v.as_object() else {
            return Err("a mix check request is a JSON object".into());
        };
        if let Some(k) = m.keys().find(|k| !FIELDS.contains(&k.as_str())) {
            return Err(format!(
                "unknown field {k:?} (fields: {})",
                FIELDS.join(", ")
            ));
        }
        let s = |k: &str| -> Result<Option<String>, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(x)) if x.trim().is_empty() => Ok(None),
                Some(Value::String(x)) => Ok(Some(x.trim().to_string())),
                Some(Value::Number(n)) => Ok(Some(n.to_string())),
                _ => Err(format!("{k}: expected a string")),
            }
        };
        let b = |k: &str| -> Result<bool, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(false),
                Some(Value::Bool(x)) => Ok(*x),
                _ => Err(format!("{k}: expected true or false")),
            }
        };
        let mut o = Options {
            range: s("range")?,
            beats: s("beats")?,
            section: s("section")?,
            verify: b("verify")?,
            history: b("history")?,
            target: s("target")?,
            reference: s("reference")?,
            ..Options::default()
        };
        if let Some(by) = s("by")? {
            o.by = parse_by(&by)?;
        }
        if let Some(f) = m.get("focus") {
            o.focus = list(f, "focus")?;
        }
        if let Some(c) = m.get("checks") {
            let names = list(c, "checks")?;
            if !names.is_empty() {
                o.checks = names
                    .iter()
                    .map(|n| parse_check(n))
                    .collect::<Result<_, _>>()?;
            }
        }
        if let Some(w) = m.get("whatIf") {
            o.what_if = ops_of(w, "whatIf")?;
        }
        if let Some(t) = s("threshold")? {
            o.threshold = match t.as_str() {
                "strict" => Threshold::Strict,
                "normal" => Threshold::Normal,
                "loose" => Threshold::Loose,
                _ => return Err(format!("threshold: {t:?} is not strict, normal or loose")),
            };
        }
        let num = |k: &str| -> Result<Option<f64>, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_f64()
                    .map(Some)
                    .ok_or_else(|| format!("{k}: expected a number")),
            }
        };
        if let Some(n) = num("maxFindings")? {
            if n < 0.0 {
                return Err("maxFindings: expected a count".into());
            }
            o.max_findings = (n as usize).min(MAX_FINDINGS);
        }
        if let Some(n) = num("prerollBeats")? {
            if !(0.0..=64.0).contains(&n) {
                return Err("prerollBeats: 0 … 64".into());
            }
            o.preroll = Some(n);
        }
        if let Some(n) = num("sampleRate")? {
            if !(8000.0..=192000.0).contains(&n) {
                return Err("sampleRate: 8000 … 192000".into());
            }
            o.sample_rate = n as f32;
        }
        if let Some(Value::Bool(c)) = m.get("cache") {
            o.cache = *c;
        }
        if let Some(t) = &o.target {
            if super::targets::find(t).is_none() {
                return Err(format!(
                    "target: {t:?} is not one of {}",
                    super::targets::names().join(", ")
                ));
            }
        }
        Ok(o)
    }

    pub fn has(&self, c: Check) -> bool {
        self.checks.contains(&c)
    }
}

/// JSON Patch operations: an array, one operation, or either as JSON text.
pub fn ops_of(v: &Value, key: &str) -> Result<Vec<Value>, String> {
    match v {
        Value::Null => Ok(vec![]),
        Value::Array(a) => Ok(a.clone()),
        Value::Object(_) => Ok(vec![v.clone()]),
        Value::String(s) if s.trim().is_empty() => Ok(vec![]),
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(inner @ (Value::Array(_) | Value::Object(_))) => ops_of(&inner, key),
            _ => Err(format!("{key}: not a JSON Patch")),
        },
        _ => Err(format!("{key}: expected an array of JSON Patch operations")),
    }
}

pub fn parse_by(s: &str) -> Result<By, String> {
    match s.trim() {
        "bar" | "bars" => Ok(By::Bar),
        "section" | "sections" => Ok(By::Section),
        x => {
            let n = x
                .trim_end_matches("-beats")
                .trim_end_matches("beats")
                .trim_end_matches('-')
                .trim();
            match n.parse::<f64>() {
                Ok(v) if v > 0.0 => Ok(By::Beats(v)),
                _ => Err(format!(
                    "by: {s:?} is not bar, section or N-beats (e.g. 8-beats)"
                )),
            }
        }
    }
}

pub fn parse_check(s: &str) -> Result<Check, String> {
    let s = s.trim().to_ascii_lowercase();
    let alias = match s.as_str() {
        "gr" | "gain-reduction" => "gainreduction",
        "level" => "levels",
        "clash" => "clashes",
        "spe" | "spec" => "spectrum",
        "phase" | "correlation" => "stereo",
        x => x,
    };
    CHECKS
        .iter()
        .find(|c| c.0 == alias)
        .map(|c| c.1)
        .ok_or_else(|| {
            format!(
                "checks: {s:?} is not one of {}",
                CHECKS.iter().map(|c| c.0).collect::<Vec<_>>().join(", ")
            )
        })
}

/// The command line's flags (`--range 52:59 --focus rbass --text …`) as a
/// request object, for shells without their own parser (the browser-only
/// studio's). Returns the request and whether `--text` was asked.
/// `--what-if` takes inline JSON here.
pub fn request_from_args(words: &[String]) -> Result<(Value, bool), String> {
    let mut req = serde_json::Map::new();
    let mut text = false;
    let mut it = words.iter();
    while let Some(w) = it.next() {
        let Some(flag) = w.strip_prefix("--") else {
            return Err(format!("{w:?}: expected a --flag"));
        };
        let (name, inline) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (flag, None),
        };
        let mut value = || -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| format!("--{name} needs a value"))
        };
        let key = match name {
            "text" => {
                text = true;
                continue;
            }
            "json" => {
                text = false;
                continue;
            }
            "verify" | "history" => {
                req.insert(name.into(), Value::Bool(true));
                continue;
            }
            "no-cache" => {
                req.insert("cache".into(), Value::Bool(false));
                continue;
            }
            "range" | "beats" | "section" | "by" | "focus" | "checks" | "threshold" | "target"
            | "reference" => name,
            "what-if" => {
                let ops: Value = serde_json::from_str(&value()?)
                    .map_err(|e| format!("--what-if: not JSON ({e})"))?;
                req.insert("whatIf".into(), ops);
                continue;
            }
            "max-findings" | "preroll" | "sample-rate" => {
                let v = value()?;
                let n: f64 = v
                    .parse()
                    .map_err(|_| format!("--{name}: not a number: {v}"))?;
                let k = match name {
                    "max-findings" => "maxFindings",
                    "preroll" => "prerollBeats",
                    _ => "sampleRate",
                };
                req.insert(k.into(), serde_json::json!(n));
                continue;
            }
            other => return Err(format!("unknown flag --{other}")),
        };
        req.insert(key.into(), Value::String(value()?));
    }
    Ok((Value::Object(req), text))
}
