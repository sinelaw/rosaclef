//! Parsing and semantic validation of project documents.
//!
//! Errors carry a JSON path (`channels[2].instrument.params.cutoff`) and a
//! human readable message, so that an agent editing the file by hand can fix
//! mistakes quickly.

use crate::automation::AutomationTarget;
use crate::catalog::{self, Category, DeviceSpec};
use crate::model::{Device, Project, FORMAT, SCORE_CLEFS, SCORE_KEYS};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Serialize, Clone, Debug)]
pub struct Issue {
    pub severity: Severity,
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if self.path.is_empty() {
            write!(f, "{sev}: {}", self.message)
        } else {
            write!(f, "{sev}: {}: {}", self.path, self.message)
        }
    }
}

/// Result of parsing a document: the project (when it parsed) and any issues.
pub struct Checked {
    pub project: Option<Project>,
    pub issues: Vec<Issue>,
}

impl Checked {
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }
    pub fn is_ok(&self) -> bool {
        self.project.is_some() && self.errors().next().is_none()
    }
}

/// Parse JSON text into a project and validate it.
pub fn parse_and_validate(text: &str) -> Checked {
    let de = &mut serde_json::Deserializer::from_str(text);
    match serde_path_to_error::deserialize::<_, Project>(de) {
        Ok(project) => {
            let issues = validate(&project);
            Checked {
                project: Some(project),
                issues,
            }
        }
        Err(err) => {
            let path = err.path().to_string();
            let path = if path == "." { String::new() } else { path };
            let inner = err.into_inner();
            Checked {
                project: None,
                issues: vec![Issue {
                    severity: Severity::Error,
                    path,
                    message: inner.to_string(),
                }],
            }
        }
    }
}

/// Parse an already-decoded JSON value into a project and validate it.
pub fn value_and_validate(value: serde_json::Value) -> Checked {
    match serde_path_to_error::deserialize::<_, Project>(value) {
        Ok(project) => {
            let issues = validate(&project);
            Checked {
                project: Some(project),
                issues,
            }
        }
        Err(err) => {
            let path = err.path().to_string();
            let path = if path == "." { String::new() } else { path };
            Checked {
                project: None,
                issues: vec![Issue {
                    severity: Severity::Error,
                    path,
                    message: err.into_inner().to_string(),
                }],
            }
        }
    }
}

struct V {
    issues: Vec<Issue>,
}

impl V {
    fn err(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Error,
            path: path.into(),
            message: message.into(),
        });
    }
    fn warn(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Warning,
            path: path.into(),
            message: message.into(),
        });
    }
    fn range(&mut self, path: &str, v: f64, min: f64, max: f64) {
        if !v.is_finite() || v < min || v > max {
            self.err(
                path,
                format!("value {v} is outside the allowed range {min}..{max}"),
            );
        }
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Semantic checks that the type system alone cannot express.
pub fn validate(p: &Project) -> Vec<Issue> {
    let mut v = V { issues: vec![] };

    if p.format != FORMAT {
        v.err(
            "format",
            format!("unsupported format {:?}, expected {:?}", p.format, FORMAT),
        );
    }
    v.range("transport.bpm", p.transport.bpm, 20.0, 999.0);
    if p.transport.beats_per_bar == 0 || p.transport.beats_per_bar > 32 {
        v.err("transport.beatsPerBar", "must be between 1 and 32");
    }
    v.range("transport.swing", p.transport.swing, 0.0, 1.0);
    if !(-12..=12).contains(&p.transport.transpose) {
        v.err(
            "transport.transpose",
            "must be between -12 and 12 semitones",
        );
    }
    let mut prev_bar = 0u32;
    for (i, m) in p.transport.meters.iter().enumerate() {
        let path = format!("transport.meters[{i}]");
        if m.bar == 0 {
            v.err(format!("{path}.bar"), "bars are counted from 1");
        } else if m.bar <= prev_bar {
            v.err(
                format!("{path}.bar"),
                format!(
                    "meters must be sorted by bar, one per bar ({} comes after {prev_bar})",
                    m.bar
                ),
            );
        } else {
            prev_bar = m.bar;
        }
        if m.numerator == 0 || m.numerator > 64 {
            v.err(format!("{path}.numerator"), "must be between 1 and 64");
        }
        if !m.denominator.is_power_of_two() || m.denominator > 32 {
            v.err(
                format!("{path}.denominator"),
                "must be 1, 2, 4, 8, 16 or 32",
            );
        } else if m.bar_beats() > 32.0 {
            v.err(path, "a bar can be at most 32 beats long");
        }
    }
    if p.transport.meters.len() > 4096 {
        v.err(
            "transport.meters",
            "at most 4096 meter changes are supported",
        );
    }

    // Mixer.
    if p.mixer.inserts.is_empty() {
        v.err(
            "mixer.inserts",
            "there must be at least one insert (index 0 is the master)",
        );
    }
    if p.mixer.inserts.len() > 128 {
        v.err("mixer.inserts", "at most 128 inserts are supported");
    }
    for (i, ins) in p.mixer.inserts.iter().enumerate() {
        let path = format!("mixer.inserts[{i}]");
        v.range(&format!("{path}.volume"), ins.volume, 0.0, 2.0);
        v.range(&format!("{path}.pan"), ins.pan, -1.0, 1.0);
        for (j, fx) in ins.effects.iter().enumerate() {
            check_device(
                &mut v,
                &format!("{path}.effects[{j}]"),
                fx,
                Category::Effect,
            );
        }
    }
    let n_inserts = p.mixer.inserts.len();

    // Channels.
    let mut channel_ids = HashSet::new();
    for (i, ch) in p.channels.iter().enumerate() {
        let path = format!("channels[{i}]");
        if !valid_id(&ch.id) {
            v.err(
                format!("{path}.id"),
                format!(
                    "invalid id {:?}: use 1-64 characters from [A-Za-z0-9_.-]",
                    ch.id
                ),
            );
        }
        if !channel_ids.insert(ch.id.as_str()) {
            v.err(
                format!("{path}.id"),
                format!("duplicate channel id {:?}", ch.id),
            );
        }
        if !valid_color(&ch.color) {
            v.err(format!("{path}.color"), "colors are #rrggbb hex strings");
        }
        v.range(&format!("{path}.volume"), ch.volume, 0.0, 1.5);
        v.range(&format!("{path}.pan"), ch.pan, -1.0, 1.0);
        if ch.mixer.index() >= n_inserts {
            v.err(
                format!("{path}.mixer"),
                format!(
                    "mixer insert {} does not exist (there are {n_inserts})",
                    ch.mixer
                ),
            );
        }
        check_device(
            &mut v,
            &format!("{path}.instrument"),
            &ch.instrument,
            Category::Instrument,
        );
        if let Some(arp) = &ch.arp {
            check_arp(&mut v, &format!("{path}.arp"), arp);
        }
    }

    // Patterns.
    let mut pattern_ids = HashSet::new();
    for (i, pat) in p.patterns.iter().enumerate() {
        let path = format!("patterns[{i}]");
        if !valid_id(&pat.id) {
            v.err(
                format!("{path}.id"),
                format!(
                    "invalid id {:?}: use 1-64 characters from [A-Za-z0-9_.-]",
                    pat.id
                ),
            );
        }
        if !pattern_ids.insert(pat.id.as_str()) {
            v.err(
                format!("{path}.id"),
                format!("duplicate pattern id {:?}", pat.id),
            );
        }
        if !valid_color(&pat.color) {
            v.err(format!("{path}.color"), "colors are #rrggbb hex strings");
        }
        if !(pat.length > 0.0 && pat.length <= 4096.0) {
            v.err(
                format!("{path}.length"),
                "pattern length must be in (0, 4096] beats",
            );
        }
        let mut beyond = 0;
        for (j, n) in pat.notes.iter().enumerate() {
            let np = format!("{path}.notes[{j}]");
            if !channel_ids.contains(n.channel.as_str()) {
                v.err(
                    format!("{np}.channel"),
                    format!("unknown channel {:?}", n.channel),
                );
            }
            if !(0..=127).contains(&n.pitch) {
                v.err(
                    format!("{np}.pitch"),
                    "pitch must be a MIDI note number 0..127",
                );
            }
            if !(n.start >= 0.0 && n.start.is_finite()) {
                v.err(format!("{np}.start"), "start must be >= 0");
            }
            if !(n.length > 0.0 && n.length.is_finite()) {
                v.err(format!("{np}.length"), "length must be > 0");
            }
            v.range(&format!("{np}.velocity"), n.velocity, 0.0, 1.0);
            if n.start >= pat.length {
                beyond += 1;
            }
        }
        if beyond > 0 {
            v.warn(
                format!("{path}.notes"),
                format!(
                    "{beyond} note(s) start after the pattern length ({}) and will never play",
                    pat.length
                ),
            );
        }
    }

    // Playlist.
    let n_tracks = p.playlist.tracks.len();
    for (i, c) in p.playlist.clips.iter().enumerate() {
        let path = format!("playlist.clips[{i}]");
        match (c.pattern.is_empty(), c.sample.is_empty()) {
            (false, true) => {
                if !pattern_ids.contains(c.pattern.as_str()) {
                    v.err(
                        format!("{path}.pattern"),
                        format!("unknown pattern {:?}", c.pattern),
                    );
                }
            }
            (true, false) => {
                check_relative_path(&mut v, &format!("{path}.sample"), &c.sample);
                if c.mixer.index() >= n_inserts {
                    v.err(
                        format!("{path}.mixer"),
                        format!("mixer insert {} does not exist", c.mixer),
                    );
                }
            }
            _ => v.err(
                &path,
                "a clip needs exactly one of \"pattern\" or \"sample\"",
            ),
        }
        if c.track.index() >= n_tracks {
            v.err(
                format!("{path}.track"),
                format!(
                    "track {} does not exist (there are {n_tracks} tracks)",
                    c.track
                ),
            );
        }
        if !(c.start >= 0.0 && c.start.is_finite()) {
            v.err(format!("{path}.start"), "start must be >= 0");
        }
        if !(c.length > 0.0 && c.length.is_finite()) {
            v.err(format!("{path}.length"), "length must be > 0");
        }
        if !(c.offset >= 0.0 && c.offset.is_finite()) {
            v.err(format!("{path}.offset"), "offset must be >= 0");
        }
        v.range(&format!("{path}.gain"), c.gain, 0.0, 4.0);
    }

    check_automation(&mut v, p);
    check_score(&mut v, p);
    check_repeats(&mut v, p);
    v.issues
}

fn check_repeats(v: &mut V, p: &Project) {
    let mut spans: Vec<(f64, f64, usize)> = vec![];
    for (i, r) in p.repeats.iter().enumerate() {
        let path = format!("repeats[{i}]");
        if !(r.start >= 0.0 && r.start.is_finite()) {
            v.err(format!("{path}.start"), "start must be >= 0");
        }
        if !(r.end > r.start && r.end.is_finite()) {
            v.err(format!("{path}.end"), "end must come after start");
        }
        if !(1..=99).contains(&r.times) {
            v.err(
                format!("{path}.times"),
                "times must be 1 to 99 (2 = play it twice)",
            );
        }
        let mut end = r.end;
        let mut prev: Option<(f64, f64)> = None;
        let mut endings: Vec<(usize, &crate::model::Ending)> =
            r.endings.iter().enumerate().collect();
        endings.sort_by(|a, b| a.1.start.total_cmp(&b.1.start));
        for (j, e) in endings {
            let ep = format!("{path}.endings[{j}]");
            if !(e.end > e.start && e.start.is_finite() && e.end.is_finite()) {
                v.err(format!("{ep}.end"), "end must come after start");
                continue;
            }
            if e.start < r.start {
                v.err(
                    format!("{ep}.start"),
                    "an ending starts inside its repeat or right after it",
                );
            }
            if e.start > end + 1e-9 {
                v.err(
                    format!("{ep}.start"),
                    format!("an ending after the repeat must follow it directly (at beat {end})"),
                );
            }
            if let Some((_, pe)) = prev {
                if e.start < pe - 1e-9 {
                    v.err(format!("{ep}.start"), "endings must not overlap");
                }
            }
            if e.passes.is_empty() {
                v.err(
                    format!("{ep}.passes"),
                    "list the passes that play this ending (e.g. [1])",
                );
            }
            for &k in &e.passes {
                if k < 1 || k > r.times {
                    v.err(
                        format!("{ep}.passes"),
                        format!(
                            "pass {k} does not exist (the repeat plays {} times)",
                            r.times
                        ),
                    );
                }
            }
            if e.start >= r.end - 1e-9 {
                end = end.max(e.end);
            }
            prev = Some((e.start, e.end));
        }
        spans.push((r.start, end, i));
    }
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    for w in spans.windows(2) {
        if w[1].0 < w[0].1 - 1e-9 {
            v.err(
                format!("repeats[{}].start", w[1].2),
                format!(
                    "repeats must not overlap (repeats[{}] runs to beat {})",
                    w[0].2, w[0].1
                ),
            );
        }
    }
}

fn check_score(v: &mut V, p: &Project) {
    let sc = &p.score;
    if !sc.key.is_empty() && !SCORE_KEYS.contains(&sc.key.as_str()) {
        v.err(
            "score.key",
            format!(
                "unknown key {:?}: use auto or one of {}",
                sc.key,
                SCORE_KEYS[1..].join(", ")
            ),
        );
    }
    for (i, id) in sc.hidden.iter().enumerate() {
        if p.channel(id).is_none() {
            v.warn(
                format!("score.hidden[{i}]"),
                format!("channel {id:?} does not exist"),
            );
        }
    }
    for (i, t) in sc.hidden_tracks.iter().enumerate() {
        if t.index() >= p.playlist.tracks.len() {
            v.warn(
                format!("score.hiddenTracks[{i}]"),
                format!("track {t} does not exist"),
            );
        }
    }
    for (id, clef) in &sc.clefs {
        if p.channel(id).is_none() {
            v.warn(
                format!("score.clefs.{id}"),
                format!("channel {id:?} does not exist"),
            );
        }
        if !SCORE_CLEFS.contains(&clef.as_str()) {
            v.err(
                format!("score.clefs.{id}"),
                format!(
                    "unknown clef {clef:?}: use one of {}",
                    SCORE_CLEFS.join(", ")
                ),
            );
        }
    }
    for (i, m) in sc.marks.iter().enumerate() {
        let path = format!("score.marks[{i}]");
        if !(m.start >= 0.0 && m.start.is_finite()) {
            v.err(format!("{path}.start"), "start must be >= 0");
        }
        if !(m.end > m.start && m.end.is_finite()) {
            v.err(format!("{path}.end"), "end must come after start");
        }
        if !valid_color(&m.color) {
            v.err(format!("{path}.color"), "colors are #rrggbb hex strings");
        }
        if !m.pattern.is_empty() && p.pattern(&m.pattern).is_none() {
            v.err(
                format!("{path}.pattern"),
                format!("pattern {:?} does not exist", m.pattern),
            );
        }
        for (j, id) in m.channels.iter().enumerate() {
            if p.channel(id).is_none() {
                v.err(
                    format!("{path}.channels[{j}]"),
                    format!("channel {id:?} does not exist"),
                );
            }
        }
    }
}

fn check_automation(v: &mut V, p: &Project) {
    let mut lane_ids = HashSet::new();
    let mut targets: Vec<(AutomationTarget, usize)> = vec![];
    for (i, lane) in p.automation.iter().enumerate() {
        let path = format!("automation[{i}]");
        if !valid_id(&lane.id) {
            v.err(
                format!("{path}.id"),
                format!(
                    "invalid id {:?}: use 1-64 characters from [A-Za-z0-9_.-]",
                    lane.id
                ),
            );
        }
        if !lane_ids.insert(lane.id.as_str()) {
            v.err(
                format!("{path}.id"),
                format!("duplicate automation lane id {:?}", lane.id),
            );
        }
        if !valid_color(&lane.color) {
            v.err(format!("{path}.color"), "colors are #rrggbb hex strings");
        }
        let info = match lane.target.parse::<AutomationTarget>() {
            Err(e) => {
                v.err(format!("{path}.target"), e);
                None
            }
            Ok(t) => {
                if let Some((_, j)) = targets.iter().find(|(o, _)| *o == t) {
                    v.err(
                        format!("{path}.target"),
                        format!("automation[{j}] already automates {t}; use one lane per target"),
                    );
                }
                let info = match t.resolve(p) {
                    Ok(info) => Some(info),
                    Err(e) => {
                        v.err(format!("{path}.target"), format!("{}: {e}", lane.target));
                        None
                    }
                };
                targets.push((t, i));
                info
            }
        };
        if lane.points.is_empty() {
            v.err(
                format!("{path}.points"),
                "an automation lane needs at least one point",
            );
        }
        let mut prev = 0.0f64;
        for (j, pt) in lane.points.iter().enumerate() {
            let pp = format!("{path}.points[{j}]");
            if !(pt.beat >= 0.0 && pt.beat.is_finite()) {
                v.err(format!("{pp}.beat"), "beat must be >= 0");
            } else if pt.beat < prev {
                v.err(
                    format!("{pp}.beat"),
                    format!(
                        "points must be sorted by beat ({} comes after {})",
                        pt.beat, prev
                    ),
                );
            } else {
                prev = pt.beat;
            }
            match &info {
                Some(info) => v.range(&format!("{pp}.value"), pt.value, info.min, info.max),
                None if !pt.value.is_finite() => {
                    v.err(format!("{pp}.value"), "value must be a finite number")
                }
                None => {}
            }
            v.range(&format!("{pp}.curve"), pt.curve, -1.0, 1.0);
        }
    }
}

fn check_arp(v: &mut V, path: &str, arp: &crate::Arpeggio) {
    use crate::arp;
    if arp::chord(&arp.chord).is_none() {
        let names: Vec<&str> = arp::CHORDS.iter().map(|c| c.0).collect();
        v.err(
            format!("{path}.chord"),
            format!(
                "unknown chord {:?}; expected one of {}",
                arp.chord,
                names.join(", ")
            ),
        );
    }
    if !(1..=arp::OCTAVES_MAX).contains(&arp.octaves) {
        v.err(
            format!("{path}.octaves"),
            format!("octaves must be 1..={}", arp::OCTAVES_MAX),
        );
    }
    v.range(
        &format!("{path}.rate"),
        arp.rate,
        arp::RATE_MIN,
        arp::RATE_MAX,
    );
    v.range(
        &format!("{path}.gate"),
        arp.gate,
        arp::GATE_MIN,
        arp::GATE_MAX,
    );
    if !arp::DIRECTIONS.contains(&arp.direction.as_str()) {
        v.err(
            format!("{path}.direction"),
            format!("expected one of {}", arp::DIRECTIONS.join(", ")),
        );
    }
    if !arp::MODES.contains(&arp.mode.as_str()) {
        v.err(
            format!("{path}.mode"),
            format!("expected one of {}", arp::MODES.join(", ")),
        );
    }
}

fn check_relative_path(v: &mut V, path: &str, file: &str) {
    if file.starts_with('/') || file.split(['/', '\\']).any(|seg| seg == "..") {
        v.err(
            path,
            format!("{file:?} must be a path relative to the project folder (no '..')"),
        );
    }
}

fn check_device(v: &mut V, path: &str, d: &Device, category: Category) {
    let Some(spec): Option<&DeviceSpec> = catalog::device_in(&d.kind, category) else {
        let known: Vec<_> = catalog::DEVICES
            .iter()
            .filter(|s| s.category == category)
            .map(|s| s.kind)
            .collect();
        let what = if category == Category::Instrument {
            "instrument"
        } else {
            "effect"
        };
        v.err(
            format!("{path}.type"),
            format!(
                "unknown {what} type {:?}; expected one of {}",
                d.kind,
                known.join(", ")
            ),
        );
        return;
    };
    if !spec.open_params {
        for (k, val) in &d.params {
            match spec.param(k) {
                None => {
                    let keys: Vec<_> = spec.params.iter().map(|p| p.key).collect();
                    v.err(
                        format!("{path}.params.{k}"),
                        format!(
                            "unknown parameter for {:?}; expected one of {}",
                            spec.kind,
                            keys.join(", ")
                        ),
                    );
                }
                Some(ps) => {
                    v.range(&format!("{path}.params.{k}"), *val, ps.min, ps.max);
                    if ps.integer && val.fract() != 0.0 {
                        v.err(format!("{path}.params.{k}"), "must be a whole number");
                    }
                }
            }
        }
    }
    for (k, val) in &d.options {
        match spec.option(k) {
            None => {
                let keys: Vec<_> = spec.options.iter().map(|o| o.key).collect();
                v.err(
                    format!("{path}.options.{k}"),
                    format!(
                        "unknown option for {:?}; expected one of {}",
                        spec.kind,
                        keys.join(", ")
                    ),
                );
            }
            Some(os) => {
                if !os.choices.is_empty() && !os.choices.contains(&val.as_str()) {
                    v.err(
                        format!("{path}.options.{k}"),
                        format!("{val:?} is not one of {}", os.choices.join(", ")),
                    );
                }
            }
        }
    }
    if d.kind == "sampler" && !d.option("sample").is_empty() {
        check_relative_path(v, &format!("{path}.options.sample"), d.option("sample"));
    }
    if d.kind == "plugin" && d.option("path").is_empty() {
        v.err(
            format!("{path}.options.path"),
            "plugin devices need options.path",
        );
    }
}
