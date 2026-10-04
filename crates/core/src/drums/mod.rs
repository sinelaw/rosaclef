//! Drums: a drummer for the song (see `docs/drums.md`).
//!
//! The project's `drums` part says which groove to play, on which kit, and
//! what each section of the song does (groove A or B, hits, a count-in or
//! rest, a fill into the next section, a crash on its first downbeat).
//! [`write`] turns it into ordinary patterns and playlist clips: the plain
//! groove as one looping pattern, and one-bar patterns for the bars that
//! differ (crash, fill, turnaround). Writing is deterministic: the same part
//! gives the same notes.

pub mod library;

use crate::model::{Channel, Clip, Device, Note, Pattern, Project, Track, TrackIx};
use library::{Fill, Groove, FILLS, GROOVES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub use library::groove;

/// What a section plays.
pub const PLAYS: &[&str] = &["a", "b", "hits", "count", "rest"];
/// The fill into the next section.
pub const FILL_SIZES: &[&str] = &["none", "beat", "half", "bar"];
/// How the notes sit against the grid.
pub const FEELS: &[&str] = &["tight", "natural", "loose"];
/// What follows the last section.
pub const ENDINGS: &[&str] = &["hit", "none"];
/// The drum-machine kit: one Ebony Drum Machine channel per drum.
pub const EBONY: &str = "Ebony";
/// Drum roles a groove row can name.
pub const ROLES: &[&str] = &[
    "kick", "snare", "rim", "clap", "hat", "pedal", "openhat", "ride", "bell", "crash", "tom1",
    "tom2", "tom3", "cowbell", "shaker", "tamb",
];

/// The song's drum part (`drums` in `project.json`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrumPart {
    /// A groove id from the library (`rosaclef grooves`).
    pub groove: String,
    /// A General MIDI drum kit (`Standard Kit`, ...) or `Ebony`; empty: the
    /// groove's own suggestion.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kit: String,
    #[serde(default = "default_feel")]
    pub feel: String,
    /// 0..1: delays the off 16ths (straight grooves only), like the
    /// transport's swing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub swing: f64,
    /// The bar the first section starts on, counted from 1.
    #[serde(default = "one")]
    pub start: u32,
    #[serde(default = "default_ending")]
    pub ending: String,
    /// A small turnaround every 4th bar.
    #[serde(default = "yes")]
    pub variations: bool,
    /// Picks the fills and the small timing and velocity differences.
    #[serde(default = "one")]
    pub seed: u32,
    #[serde(default)]
    pub sections: Vec<DrumSection>,
    /// Grooves changed for this song: groove id → its parts' rows, replacing
    /// the library's (from the tab's grid, or from hand edits of the
    /// groove's pattern).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grooves: BTreeMap<String, GrooveEdit>,
    /// Patterns edited by hand, kept note for note when the part is written
    /// again: slot (`rock-8ths/a+crash`) → their notes, by drum role.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kept: BTreeMap<String, Kept>,
    /// Managed by Rosaclef: the patterns the last write made — pattern id →
    /// its slot and a fingerprint of its notes (to notice hand edits).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub written: BTreeMap<String, Written>,
}

/// A groove's parts as edited for one song: `[role, steps]` rows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct GrooveEdit {
    #[serde(default)]
    pub a: Vec<[String; 2]>,
    #[serde(default)]
    pub b: Vec<[String; 2]>,
}

/// A pattern edited by hand.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    /// What the pattern is ("Straight 8ths A + crash").
    pub name: String,
    pub notes: Vec<KeptNote>,
}

/// A note of a kept pattern, by drum role so it follows a kit change:
/// a role (`snare`), `gm:<key>` (a General MIDI drum key with no role), or
/// `channel:<id>:<pitch>` (a channel outside the kit).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeptNote {
    pub role: String,
    pub start: f64,
    pub length: f64,
    pub velocity: f64,
}

/// A pattern the last write made.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Written {
    pub slot: String,
    /// Fingerprint of the pattern as written; a pattern that no longer
    /// matches it was edited by hand. Empty: whatever the pattern holds is
    /// not an edit (its edits were reset, or a grid edit replaced them).
    pub print: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DrumSection {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub bars: u32,
    /// `a`, `b`, `hits`, `count` or `rest`.
    #[serde(default = "default_play")]
    pub play: String,
    /// The fill into the next section: `none`, `beat`, `half` (2 beats) or
    /// `bar`.
    #[serde(default = "default_fill", skip_serializing_if = "is_none_fill")]
    pub fill: String,
    /// A crash on the section's first downbeat.
    #[serde(default, skip_serializing_if = "is_false")]
    pub crash: bool,
    /// Another groove for this section (a half-time bridge); empty: the
    /// part's groove.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub groove: String,
}

fn default_feel() -> String {
    "natural".into()
}
fn default_ending() -> String {
    "hit".into()
}
fn default_play() -> String {
    "a".into()
}
fn default_fill() -> String {
    "none".into()
}
fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}
fn is_zero(x: &f64) -> bool {
    *x == 0.0
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_none_fill(s: &String) -> bool {
    s == "none"
}

impl DrumPart {
    /// A part for `groove` with no sections yet.
    pub fn new(groove: &str) -> DrumPart {
        DrumPart {
            groove: groove.into(),
            kit: String::new(),
            feel: default_feel(),
            swing: 0.0,
            start: 1,
            ending: default_ending(),
            variations: true,
            seed: 1,
            sections: vec![],
            grooves: BTreeMap::new(),
            kept: BTreeMap::new(),
            written: BTreeMap::new(),
        }
    }
}

/// The kit a part plays on: its own choice, or its groove's suggestion.
pub fn kit_of(part: &DrumPart) -> String {
    if !part.kit.is_empty() {
        return part.kit.clone();
    }
    groove(&part.groove)
        .map(|g| g.kit.to_string())
        .unwrap_or_else(|| "Standard Kit".into())
}

/// Is `kit` a kit a part can name?
pub fn valid_kit(kit: &str) -> bool {
    kit == EBONY || crate::gm::is_kit(kit)
}

// ------------------------------------------------------------------- roles

/// General MIDI drum key of a role.
pub fn gm_key(role: &str) -> i32 {
    match role {
        "kick" => 36,
        "rim" => 37,
        "snare" => 38,
        "clap" => 39,
        "hat" => 42,
        "pedal" => 44,
        "openhat" => 46,
        "crash" => 49,
        "ride" => 51,
        "bell" => 53,
        "tom1" => 48,
        "tom2" => 45,
        "tom3" => 43,
        "tamb" => 54,
        "cowbell" => 56,
        "shaker" => 70,
        _ => 38,
    }
}

/// An Ebony Drum Machine channel for a role: name, `kind`, the note it
/// plays, and a decay multiplier (crash and ride are long open hats, as in
/// the MIDI importer).
fn ebony(role: &str) -> (&'static str, &'static str, i32, Option<f64>) {
    match role {
        "kick" => ("Kick", "kick", 60, None),
        "snare" => ("Snare", "snare", 60, None),
        "rim" => ("Rim", "rim", 60, None),
        "clap" => ("Clap", "clap", 60, None),
        "hat" | "pedal" => ("Hi-Hat", "hat", 60, None),
        "openhat" => ("Open Hat", "openhat", 60, None),
        "ride" => ("Ride", "openhat", 60, Some(1.8)),
        "bell" => ("Ride", "openhat", 67, Some(1.8)),
        "crash" => ("Crash", "openhat", 60, Some(3.0)),
        "tom1" => ("Toms", "tom", 65, None),
        "tom2" => ("Toms", "tom", 60, None),
        "tom3" => ("Toms", "tom", 55, None),
        "cowbell" => ("Cowbell", "cowbell", 60, None),
        "tamb" => ("Shaker", "shaker", 60, Some(1.4)),
        _ => ("Shaker", "shaker", 60, None),
    }
}

/// Which limb plays a role (for the playability check): `H` a hand, `R`
/// the right foot, `L` the left foot, `-` an overdub (not the drummer).
pub fn limb(role: &str) -> char {
    match role {
        "kick" => 'R',
        "pedal" => 'L',
        "tamb" | "shaker" => '-',
        _ => 'H',
    }
}

// --------------------------------------------------------------- the grid

/// Velocity of a step letter, or `None` for a rest.
pub fn level(c: char) -> Option<f64> {
    match c {
        'X' => Some(1.0),
        'x' => Some(0.78),
        'g' => Some(0.32),
        'f' => Some(0.2),
        _ => None,
    }
}

/// The steps of a row: spaces and `|` dropped.
pub fn steps(row: &str) -> Vec<char> {
    row.chars().filter(|c| *c != ' ' && *c != '|').collect()
}

/// A groove part's rows in this song: the part's edit, else the library's.
fn part_rows(part: &DrumPart, g: &Groove, b: bool) -> Vec<(&'static str, Vec<char>)> {
    if let Some(e) = part.grooves.get(g.id) {
        let rows = if b { &e.b } else { &e.a };
        if !rows.is_empty() {
            return rows
                .iter()
                .filter(|r| ROLES.contains(&r[0].as_str()))
                .map(|r| (role_static(&r[0]), steps(&r[1])))
                .collect();
        }
    }
    g.part(b)
        .iter()
        .map(|(r, s)| (role_static(r), steps(s)))
        .collect()
}

/// One bar on the step grid: a stroke letter per role per step.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Bar {
    steps: usize,
    rows: BTreeMap<&'static str, Vec<char>>,
}

impl Bar {
    fn new(steps: usize) -> Bar {
        Bar {
            steps,
            rows: BTreeMap::new(),
        }
    }
    fn get(&self, role: &str, i: usize) -> char {
        self.rows.get(role).map(|r| r[i]).unwrap_or('.')
    }
    fn set(&mut self, role: &'static str, i: usize, c: char) {
        let n = self.steps;
        self.rows.entry(role).or_insert_with(|| vec!['.'; n])[i] = c;
    }
    /// Bar `index` of a groove part.
    fn of_groove(part: &DrumPart, g: &Groove, part_b: bool, index: usize) -> Bar {
        let n = g.steps as usize;
        let mut bar = Bar::new(n);
        for (role, s) in part_rows(part, g, part_b) {
            let bars = (s.len() / n).max(1);
            let from = (index % bars) * n;
            for i in 0..n {
                let c = s.get(from + i).copied().unwrap_or('.');
                if c != '.' {
                    bar.set(role, i, c);
                }
            }
        }
        bar
    }
    /// Replace the last steps with a fill.
    fn fill(&mut self, f: &Fill) {
        let len = (f.beats * f.steps_per_beat) as usize;
        let from = self.steps.saturating_sub(len);
        for row in self.rows.values_mut() {
            for c in &mut row[from..] {
                *c = '.';
            }
        }
        for (role, row) in f.rows {
            for (i, c) in steps(row).into_iter().enumerate() {
                if c != '.' && from + i < self.steps {
                    self.set(role_static(role), from + i, c);
                }
            }
        }
    }
    /// A crash and a kick on the downbeat; the cymbal stroke there goes.
    fn crash(&mut self) {
        for r in ["hat", "openhat", "ride", "bell"] {
            if self.get(r, 0) != '.' {
                self.set(r, 0, '.');
            }
        }
        self.set("crash", 0, 'X');
        self.set("kick", 0, 'X');
    }
    /// A small turnaround at the end of the bar: the last off-beat hat opens,
    /// or (without one) a pickup kick.
    fn turnaround(&mut self, steps_per_beat: usize) {
        let n = self.steps;
        let at = if steps_per_beat == 3 { n - 1 } else { n - 2 };
        let c = self.get("hat", at);
        if c != '.' {
            self.set("hat", at, '.');
            self.set("openhat", at, if c == 'g' { 'x' } else { c });
        } else if self.get("kick", n - 1) == '.' {
            self.set("kick", n - 1, 'x');
        }
    }
    fn is_empty(&self) -> bool {
        self.rows.values().all(|r| r.iter().all(|c| *c == '.'))
    }
}

fn role_static(role: &str) -> &'static str {
    ROLES
        .iter()
        .find(|r| **r == role)
        .copied()
        .unwrap_or("snare")
}

// ------------------------------------------------------------ the writer

/// What one bar of the song plays.
#[derive(Clone, Debug, PartialEq)]
enum Body {
    /// Bar `index` of a groove part.
    Groove {
        groove: &'static Groove,
        b: bool,
        index: usize,
    },
    Hits,
    Count,
    Rest,
    End,
}

#[derive(Clone, Debug, PartialEq)]
struct SongBar {
    /// Song time of the downbeat, in beats.
    start: f64,
    bar_beats: f64,
    body: Body,
    crash: bool,
    turn: bool,
    /// Index into [`FILLS`].
    fill: Option<usize>,
}

impl SongBar {
    fn plain(&self) -> bool {
        matches!(self.body, Body::Groove { .. }) && !self.crash && !self.turn && self.fill.is_none()
    }
    fn steps(&self) -> usize {
        match self.body {
            Body::Groove { groove, .. } => groove.steps as usize,
            _ => (self.bar_beats * 4.0).round() as usize,
        }
    }
    fn steps_per_beat(&self) -> usize {
        match self.body {
            Body::Groove { groove, .. } => groove.steps_per_beat() as usize,
            _ => 4,
        }
    }
    fn grid(&self, part: &DrumPart) -> Bar {
        let n = self.steps();
        let mut bar = match &self.body {
            Body::Groove { groove, b, index } => Bar::of_groove(part, groove, *b, *index),
            Body::Hits | Body::End => {
                let mut bar = Bar::new(n);
                bar.set("kick", 0, 'X');
                bar.set("crash", 0, 'X');
                bar
            }
            Body::Count => {
                let mut bar = Bar::new(n);
                let spb = self.steps_per_beat();
                for i in (0..n).step_by(spb) {
                    bar.set("rim", i, 'x');
                }
                bar
            }
            Body::Rest => Bar::new(n),
        };
        if self.turn {
            bar.turnaround(self.steps_per_beat());
        }
        if let Some(f) = self.fill {
            bar.fill(&FILLS[f]);
        }
        if self.crash {
            bar.crash();
        }
        bar
    }
    /// Which pattern this bar plays, stably across writes: `rock-8ths/a`
    /// (the plain groove), `rock-8ths/a+crash+fill3`, `hits`, `count`, `end`.
    fn slot(&self) -> String {
        let mut s = match &self.body {
            Body::Groove { groove, b, index } => {
                let mut s = format!("{}/{}", groove.id, if *b { "b" } else { "a" });
                if groove_bars(groove) > 1 && !self.plain() {
                    s.push_str(&format!("@{}", index + 1));
                }
                s
            }
            Body::Hits => "hits".into(),
            Body::Count => "count".into(),
            Body::Rest => "rest".into(),
            Body::End => "end".into(),
        };
        if self.crash {
            s.push_str("+crash");
        }
        if self.turn {
            s.push_str("+turn");
        }
        if let Some(f) = self.fill {
            s.push_str(&format!("+fill{f}"));
        }
        s
    }
    /// A readable name: "Straight 8ths A + crash + 1-beat fill 3".
    fn name(&self) -> String {
        let mut s = match &self.body {
            Body::Groove { groove, b, index } => {
                let mut s = format!("{} {}", groove.name, if *b { "B" } else { "A" });
                if groove_bars(groove) > 1 && !self.plain() {
                    s.push_str(&format!(" bar {}", index + 1));
                }
                s
            }
            Body::Hits => "Hits".into(),
            Body::Count => "Count-in".into(),
            Body::Rest => "Rest".into(),
            Body::End => "Ending".into(),
        };
        if self.crash {
            s.push_str(" + crash");
        }
        if self.turn {
            s.push_str(" + turnaround");
        }
        if let Some(f) = self.fill {
            let fill = &FILLS[f];
            let n = FILLS[..f]
                .iter()
                .filter(|o| o.beats == fill.beats && o.steps_per_beat == fill.steps_per_beat)
                .count()
                + 1;
            let size = match fill.beats {
                1 => "1-beat".to_string(),
                b if b as f64 >= self.bar_beats => "1-bar".to_string(),
                b => format!("{b}-beat"),
            };
            s.push_str(&format!(" + {size} fill {n}"));
        }
        s
    }
}

pub(crate) fn groove_bars(g: &Groove) -> usize {
    g.a.iter()
        .chain(g.b.iter())
        .map(|(_, r)| steps(r).len() / g.steps as usize)
        .max()
        .unwrap_or(1)
        .max(1)
}

/// FNV-1a.
fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// A number in -1..1 from a key.
fn noise(key: &str) -> f64 {
    let h = hash(key.as_bytes());
    ((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
}

/// Fingerprint of a pattern's notes, to notice hand edits.
pub fn fingerprint(p: &Pattern) -> String {
    let mut s = format!("{}", p.length);
    for n in &p.notes {
        s.push_str(&format!(
            "|{} {} {:.6} {:.6} {:.4}",
            n.channel, n.pitch, n.start, n.length, n.velocity
        ));
    }
    let h = hash(s.as_bytes());
    format!("{:08x}", (h ^ (h >> 32)) as u32)
}

/// The patterns the last write made that have been edited by hand since.
pub fn edited(p: &Project) -> Vec<String> {
    let Some(part) = &p.drums else {
        return vec![];
    };
    part.written
        .iter()
        .filter(|(id, w)| {
            !w.print.is_empty() && p.pattern(id).is_some_and(|pat| fingerprint(pat) != w.print)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Pick a fill of at most `beats` beats for a groove: the longest that fits,
/// of the wanted energy when there is one; never `avoid` when another fits.
fn pick_fill(spb: u32, beats: u32, energy: u32, key: &str, avoid: Option<usize>) -> Option<usize> {
    let fits: Vec<usize> = (0..FILLS.len())
        .filter(|&i| FILLS[i].steps_per_beat == spb && FILLS[i].beats <= beats)
        .collect();
    let longest = fits.iter().map(|&i| FILLS[i].beats).max()?;
    let sized: Vec<usize> = fits
        .into_iter()
        .filter(|&i| FILLS[i].beats == longest)
        .collect();
    let wanted: Vec<usize> = sized
        .iter()
        .copied()
        .filter(|&i| FILLS[i].energy == energy)
        .collect();
    let pool = if wanted.is_empty() { sized } else { wanted };
    let mut k = (hash(key.as_bytes()) % pool.len() as u64) as usize;
    if pool.len() > 1 && Some(pool[k]) == avoid {
        k = (k + 1) % pool.len();
    }
    Some(pool[k])
}

/// Lay the part out bar by bar.
fn layout(p: &Project, part: &DrumPart) -> Result<Vec<SongBar>, String> {
    let main = groove(&part.groove).ok_or_else(|| format!("unknown groove {:?}", part.groove))?;
    if part.sections.is_empty() {
        return Err("the drum part has no sections".into());
    }
    let t = &p.transport;
    let mut bar = part.start.max(1) - 1;
    let mut out = vec![];
    let mut last_fill: HashMap<u32, usize> = HashMap::new();
    for (si, sec) in part.sections.iter().enumerate() {
        let g = if sec.groove.is_empty() {
            main
        } else {
            groove(&sec.groove).ok_or_else(|| format!("unknown groove {:?}", sec.groove))?
        };
        let next_big = part
            .sections
            .get(si + 1)
            .map(|n| n.play == "b" || n.play == "hits")
            .unwrap_or(part.ending == "hit");
        let n = sec.bars as usize;
        for i in 0..n {
            let start = t.bar_start(bar);
            let (_, _, bar_beats) = t.bar_at(start);
            let grooving = sec.play == "a" || sec.play == "b";
            if grooving && (bar_beats - g.bar_beats as f64).abs() > 1e-6 {
                return Err(format!(
                    "bar {} is {} beats long, but {:?} is in {}",
                    bar + 1,
                    crate::format::format_f64(bar_beats),
                    g.name,
                    g.meter
                ));
            }
            let body = match sec.play.as_str() {
                "a" | "b" => Body::Groove {
                    groove: g,
                    b: sec.play == "b",
                    index: i % groove_bars(g),
                },
                "hits" => Body::Hits,
                "count" => Body::Count,
                "rest" => Body::Rest,
                other => return Err(format!("unknown play {other:?}")),
            };
            let crash = sec.crash && i == 0 && grooving;
            let fill = if grooving && i + 1 == n {
                let beats = match sec.fill.as_str() {
                    "beat" => 1,
                    "half" => 2,
                    "bar" => g.bar_beats,
                    _ => 0,
                };
                if beats > 0 {
                    let spb = g.steps_per_beat();
                    let key = format!("{}/{}/fill", part.seed, si);
                    let avoid = last_fill.get(&(spb * 100 + beats)).copied();
                    let f = pick_fill(spb, beats, if next_big { 2 } else { 1 }, &key, avoid);
                    if let Some(f) = f {
                        last_fill.insert(spb * 100 + beats, f);
                    }
                    f
                } else {
                    None
                }
            } else {
                None
            };
            let turn = part.variations && grooving && !crash && fill.is_none() && (i + 1) % 4 == 0;
            out.push(SongBar {
                start,
                bar_beats,
                body,
                crash,
                turn,
                fill,
            });
            bar += 1;
        }
    }
    if part.ending == "hit" {
        let start = t.bar_start(bar);
        let (_, _, bar_beats) = t.bar_at(start);
        out.push(SongBar {
            start,
            bar_beats,
            body: Body::End,
            crash: false,
            turn: false,
            fill: None,
        });
    }
    Ok(out)
}

/// Where each role's notes go: a channel id and a pitch.
struct Kit {
    map: HashMap<&'static str, (String, i32)>,
    /// The General MIDI kit channel, when the kit is one.
    gm: Option<String>,
}

impl Kit {
    /// Where a kept note's role plays: a role, `gm:<key>` or
    /// `channel:<id>:<pitch>` (see [`KeptNote`]).
    fn resolve(&self, p: &Project, role: &str) -> Option<(String, i32)> {
        if let Some(key) = role.strip_prefix("gm:") {
            let key: i32 = key.parse().ok()?;
            if let Some(ch) = &self.gm {
                return Some((ch.clone(), key));
            }
            return self.map.get(gm_role(key)).cloned();
        }
        if let Some(rest) = role.strip_prefix("channel:") {
            let (id, pitch) = rest.rsplit_once(':')?;
            p.channel(id)?;
            return Some((id.to_string(), pitch.parse().ok()?));
        }
        self.map.get(role).cloned()
    }
}

/// The role nearest to a General MIDI drum key.
fn gm_role(key: i32) -> &'static str {
    match key {
        35 | 36 => "kick",
        37 => "rim",
        38 | 40 => "snare",
        39 => "clap",
        42 => "hat",
        44 => "pedal",
        46 => "openhat",
        49 | 52 | 55 | 57 => "crash",
        51 | 59 => "ride",
        53 => "bell",
        48 | 50 => "tom1",
        45 | 47 => "tom2",
        41 | 43 => "tom3",
        54 => "tamb",
        56 => "cowbell",
        69 | 70 | 82 => "shaker",
        _ => "snare",
    }
}

/// The base role a kept note needs a channel for (none for another channel).
fn base_role(role: &str) -> Option<&'static str> {
    if let Some(key) = role.strip_prefix("gm:") {
        return key.parse().ok().map(gm_role);
    }
    ROLES.iter().find(|r| **r == role).copied()
}

/// The role of a note the last write's kit played, so it can follow a kit
/// change (see [`KeptNote`]).
fn role_of(p: &Project, n: &Note) -> String {
    let Some(c) = p.channel(&n.channel) else {
        return format!("channel:{}:{}", n.channel, n.pitch);
    };
    if c.instrument.kind == "soundfont" && is_drum_channel(c) {
        let r = gm_role(n.pitch);
        return if gm_key(r) == n.pitch {
            r.to_string()
        } else {
            format!("gm:{}", n.pitch)
        };
    }
    if c.instrument.kind == "drum" {
        let name = c.name.to_ascii_lowercase();
        let r = match c.instrument.option("kind") {
            "kick" => "kick",
            "snare" => "snare",
            "rim" => "rim",
            "clap" => "clap",
            "hat" => "hat",
            "cowbell" => "cowbell",
            "shaker" => "shaker",
            "tom" if n.pitch >= 63 => "tom1",
            "tom" if n.pitch >= 58 => "tom2",
            "tom" => "tom3",
            "openhat" if name.contains("crash") => "crash",
            "openhat" if name.contains("ride") && n.pitch > 63 => "bell",
            "openhat" if name.contains("ride") => "ride",
            "openhat" => "openhat",
            _ => "",
        };
        if !r.is_empty() {
            return r.to_string();
        }
    }
    format!("channel:{}:{}", n.channel, n.pitch)
}

const PALETTE: [&str; 10] = [
    "#d4af37", "#c97b84", "#e8d5b0", "#8e3b46", "#3f8f7a", "#4a6fa5", "#8a6bb0", "#b08d57",
    "#d98c5f", "#6fa3a0",
];

fn unique_id(base: &str, taken: &[String]) -> String {
    let mut slug = String::new();
    for c in base.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_end_matches('-').to_string();
    slug.truncate(56);
    let slug = if slug.is_empty() { "item".into() } else { slug };
    if !taken.contains(&slug) {
        return slug;
    }
    let mut n = 2;
    while taken.contains(&format!("{slug}-{n}")) {
        n += 1;
    }
    format!("{slug}-{n}")
}

/// Add a channel routed to the first mixer insert no channel uses yet.
fn push_channel(p: &mut Project, name: &str, instrument: Device) -> String {
    let taken: Vec<String> = p.channels.iter().map(|c| c.id.clone()).collect();
    let id = unique_id(name, &taken);
    let mut mixer = 1;
    while mixer < p.mixer.inserts.len() && p.channels.iter().any(|c| c.mixer.0 as usize == mixer) {
        mixer += 1;
    }
    if mixer >= p.mixer.inserts.len() {
        mixer = 0;
    }
    p.channels.push(Channel {
        id: id.clone(),
        name: name.into(),
        color: PALETTE[p.channels.len() % PALETTE.len()].into(),
        instrument,
        volume: 0.8,
        pan: 0.0,
        mute: false,
        mixer: crate::model::InsertIx(mixer as u32),
        arp: None,
        layer_of: None,
    });
    id
}

/// Find or make the channels for the roles used, on `kit`. A General MIDI
/// kit plays on a channel with that kit, or on the kit channel the last write
/// played (`ours`), switched over; never on another of the song's kits, whose
/// own patterns would change sound.
fn kit(p: &mut Project, kit: &str, roles: &[&'static str], ours: &[String]) -> Kit {
    let mut map = HashMap::new();
    if kit == EBONY {
        for &role in roles {
            let (name, kind, pitch, decay) = ebony(role);
            let shared = matches!(kind, "openhat" | "shaker");
            let found = p
                .channels
                .iter()
                .find(|c| {
                    c.instrument.kind == "drum"
                        && c.instrument.option("kind") == kind
                        && c.name.eq_ignore_ascii_case(name)
                })
                .or_else(|| {
                    (!shared).then(|| {
                        p.channels.iter().find(|c| {
                            c.instrument.kind == "drum" && c.instrument.option("kind") == kind
                        })
                    })?
                })
                .map(|c| c.id.clone());
            let id = found.unwrap_or_else(|| {
                let mut d = Device::new("drum");
                d.options.insert("kind".into(), kind.into());
                if let Some(decay) = decay {
                    d.params.insert("decay".into(), decay);
                }
                push_channel(p, name, d)
            });
            map.insert(role, (id, pitch));
        }
    } else {
        let is_kit = |c: &Channel| {
            c.instrument.kind == "soundfont" && crate::gm::is_kit(c.instrument.option("program"))
        };
        let found = p
            .channels
            .iter()
            .position(|c| is_kit(c) && c.instrument.option("program") == kit)
            .or_else(|| {
                p.channels
                    .iter()
                    .position(|c| is_kit(c) && ours.contains(&c.id))
            });
        let id = match found {
            Some(i) => {
                p.channels[i]
                    .instrument
                    .options
                    .insert("program".into(), kit.into());
                p.channels[i].id.clone()
            }
            None => {
                let mut d = Device::new("soundfont");
                d.options.insert("program".into(), kit.into());
                push_channel(p, "Drums", d)
            }
        };
        for &role in roles {
            map.insert(role, (id.clone(), gm_key(role)));
        }
        return Kit { map, gm: Some(id) };
    }
    Kit { map, gm: None }
}

/// Timing (ms) and velocity spread of a feel.
struct Feel {
    backbeat_ms: f64,
    ghost_ms: f64,
    spread_ms: f64,
    spread_vel: f64,
}

fn feel(name: &str) -> Feel {
    match name {
        "tight" => Feel {
            backbeat_ms: 0.0,
            ghost_ms: 0.0,
            spread_ms: 0.0,
            spread_vel: 0.0,
        },
        "loose" => Feel {
            backbeat_ms: 12.0,
            ghost_ms: 8.0,
            spread_ms: 7.0,
            spread_vel: 0.08,
        },
        _ => Feel {
            backbeat_ms: 6.0,
            ghost_ms: 4.0,
            spread_ms: 3.0,
            spread_vel: 0.04,
        },
    }
}

/// The notes of a grid, `bars` bars long.
fn notes_of(
    grids: &[Bar],
    bar_beats: f64,
    spb: usize,
    kit: &Kit,
    part: &DrumPart,
    bpm: f64,
    key: &str,
) -> Vec<Note> {
    let f = feel(&part.feel);
    let beats_per_ms = bpm / 60000.0;
    let mut notes = vec![];
    for (bi, bar) in grids.iter().enumerate() {
        let step = bar_beats / bar.steps as f64;
        for (role, row) in &bar.rows {
            let Some((channel, pitch)) = kit.map.get(role) else {
                continue;
            };
            for (i, c) in row.iter().enumerate() {
                let Some(vel) = level(*c) else {
                    continue;
                };
                let mut start = bi as f64 * bar_beats + i as f64 * step;
                // Swing: off 16ths of a straight grid, like the transport's.
                if spb == 4 && i % 2 == 1 {
                    start += part.swing.clamp(0.0, 1.0) / 12.0;
                }
                let on_beat = i % spb == 0;
                let backbeat =
                    matches!(*role, "snare" | "rim" | "clap") && on_beat && i > 0 && *c != 'g';
                let mut ms = if backbeat {
                    f.backbeat_ms
                } else if *c == 'g' {
                    f.ghost_ms
                } else {
                    0.0
                };
                let nk = format!("{}/{key}/{bi}/{role}/{i}", part.seed);
                ms += f.spread_ms * noise(&nk);
                let velocity = (vel + f.spread_vel * noise(&format!("{nk}/v"))).clamp(0.05, 1.0);
                notes.push(Note {
                    channel: channel.clone(),
                    pitch: *pitch,
                    start: round((start + ms * beats_per_ms).max(0.0)),
                    length: round(step.min(0.25)),
                    velocity: round(velocity),
                });
            }
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes
}

fn round(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

/// What a write did.
#[derive(Serialize, Debug, Default)]
pub struct Report {
    /// Patterns written.
    pub patterns: usize,
    /// Clips placed.
    pub clips: usize,
    /// The playlist track the drums are on.
    pub track: usize,
    /// Patterns kept as edited by hand.
    pub kept: usize,
}

/// Take the hand edits of the patterns the last write made into the part:
/// each edited pattern is kept note for note; an edited plain groove also
/// becomes the song's version of that groove, so its crash, fill and
/// turnaround bars follow. Returns the slots taken.
/// Forget the producer's edits (`rosaclef drums --reset-edits`): the kept
/// patterns and groove edits go, and what the last write made counts as the
/// drummer's own, so the next write replaces it (instead of keeping it as an
/// edit, or leaving it in the song beside the new patterns).
pub fn reset_edits(p: &mut Project) {
    if let Some(part) = p.drums.as_mut() {
        part.kept.clear();
        part.grooves.clear();
        for w in part.written.values_mut() {
            w.print.clear();
        }
    }
}

pub fn capture(p: &mut Project) -> Vec<String> {
    let Some(part) = p.drums.clone() else {
        return vec![];
    };
    let mut part = part;
    let mut taken = vec![];
    for (id, w) in &part.written.clone() {
        let Some(pat) = p.pattern(id) else {
            continue;
        };
        if w.print.is_empty() || fingerprint(pat) == w.print {
            continue;
        }
        let notes: Vec<KeptNote> = pat
            .notes
            .iter()
            .map(|n| KeptNote {
                role: role_of(p, n),
                start: round(n.start),
                length: round(n.length),
                velocity: round(n.velocity),
            })
            .collect();
        let name = pat
            .name
            .strip_prefix("Drums · ")
            .unwrap_or(&pat.name)
            .to_string();
        // A plain groove: its grid follows the edit.
        if let Some((gid, which)) = w.slot.split_once('/') {
            if (which == "a" || which == "b") && !w.slot.contains('+') {
                if let Some(g) = groove(gid) {
                    let rows = grid_of(&part, g, which == "b", &notes, pat.length);
                    let e = part.grooves.entry(gid.to_string()).or_default();
                    let spb = g.steps_per_beat() as usize;
                    if e.a.is_empty() {
                        e.a = to_rows(&part_rows_owned(g, false), spb);
                    }
                    if e.b.is_empty() {
                        e.b = to_rows(&part_rows_owned(g, true), spb);
                    }
                    if which == "b" {
                        e.b = rows;
                    } else {
                        e.a = rows;
                    }
                }
            }
        }
        part.kept.insert(w.slot.clone(), Kept { name, notes });
        taken.push(w.slot.clone());
    }
    p.drums = Some(part);
    taken
}

fn part_rows_owned(g: &Groove, b: bool) -> Vec<(&'static str, Vec<char>)> {
    g.part(b)
        .iter()
        .map(|(r, s)| (role_static(r), steps(s)))
        .collect()
}

/// Rows as stored: `[role, steps]`, a space between beats.
fn to_rows(rows: &[(&'static str, Vec<char>)], per_beat: usize) -> Vec<[String; 2]> {
    rows.iter()
        .map(|(r, s)| [r.to_string(), spaced(s, per_beat)])
        .collect()
}

/// Steps with a space between beats.
fn spaced(steps: &[char], per_beat: usize) -> String {
    steps
        .chunks(per_beat.max(1))
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The step grid of a groove part's notes: each note on its nearest step,
/// a stroke letter by its velocity; the groove's rows stay in order, new
/// drums follow.
fn grid_of(
    part: &DrumPart,
    g: &Groove,
    b: bool,
    notes: &[KeptNote],
    length: f64,
) -> Vec<[String; 2]> {
    let old = part_rows(part, g, b);
    let total = old
        .first()
        .map(|(_, s)| s.len())
        .unwrap_or(g.steps as usize)
        .max(1);
    let step = length / total as f64;
    let mut order: Vec<&'static str> = old.iter().map(|(r, _)| *r).collect();
    let mut grid: HashMap<&'static str, Vec<(char, f64)>> = HashMap::new();
    for n in notes {
        let Some(role) = ROLES.iter().find(|r| **r == n.role).copied() else {
            continue;
        };
        if !order.contains(&role) {
            order.push(role);
        }
        let i = ((n.start / step).round() as usize) % total;
        let c = if n.velocity >= 0.9 {
            'X'
        } else if n.velocity >= 0.55 {
            'x'
        } else if n.velocity >= 0.26 {
            'g'
        } else {
            'f'
        };
        let row = grid.entry(role).or_insert_with(|| vec![('.', 0.0); total]);
        if n.velocity > row[i].1 {
            row[i] = (c, n.velocity);
        }
    }
    order
        .into_iter()
        .filter_map(|r| {
            let row = grid.get(r)?;
            let chars: Vec<char> = row.iter().map(|(c, _)| *c).collect();
            Some([r.to_string(), spaced(&chars, g.steps_per_beat() as usize)])
        })
        .collect()
}

/// Write the project's drum part into patterns and playlist clips, replacing
/// what the last write made.
pub fn write(p: &mut Project) -> Result<Report, String> {
    p.drums.as_ref().ok_or("the project has no drum part")?;
    capture(p);
    let part = p.drums.clone().ok_or("the project has no drum part")?;
    let kit_name = kit_of(&part);
    if !valid_kit(&kit_name) {
        return Err(format!("unknown kit {kit_name:?}"));
    }
    let bars = layout(p, &part)?;

    // Forget what the last write made.
    let old: Vec<String> = part.written.keys().cloned().collect();
    let mut old_track = None;
    p.playlist.clips.retain(|c| {
        let ours = old.contains(&c.pattern);
        if ours {
            old_track.get_or_insert(c.track);
        }
        !ours
    });
    // Channels the old patterns played: one this write no longer uses goes.
    let old_channels: Vec<String> = p
        .patterns
        .iter()
        .filter(|pat| old.contains(&pat.id))
        .flat_map(|pat| pat.notes.iter().map(|n| n.channel.clone()))
        .collect();
    p.patterns.retain(|pat| !old.contains(&pat.id));

    // The grids, and the channels for the roles they use.
    let grids: Vec<Bar> = bars.iter().map(|b| b.grid(&part)).collect();
    let slots: Vec<String> = bars.iter().map(|b| b.slot()).collect();
    let mut roles: Vec<&'static str> = vec![];
    for g in &grids {
        for (r, row) in &g.rows {
            if row.iter().any(|c| *c != '.') && !roles.contains(r) {
                roles.push(r);
            }
        }
    }
    for slot in &slots {
        for n in part
            .kept
            .get(slot)
            .map(|k| k.notes.as_slice())
            .unwrap_or(&[])
        {
            if let Some(r) = base_role(&n.role) {
                if !roles.contains(&r) {
                    roles.push(r);
                }
            }
        }
    }
    let kit = kit(p, &kit_name, &roles, &old_channels);

    // The track: the one the drums were on, else one named "Drums", else
    // the first empty one, else a new one.
    let track = old_track
        .map(|t| t.index())
        .or_else(|| {
            let empty = |i: usize| !p.playlist.clips.iter().any(|c| c.track.index() == i);
            let tracks = &p.playlist.tracks;
            (0..tracks.len())
                .find(|&i| empty(i) && tracks[i].name.eq_ignore_ascii_case("drums"))
                .or_else(|| (0..tracks.len()).find(|&i| empty(i)))
        })
        .unwrap_or_else(|| {
            p.playlist.tracks.push(Track {
                name: "Drums".into(),
                mute: false,
            });
            p.playlist.tracks.len() - 1
        });
    if p.playlist.tracks[track].name.starts_with("Track ") {
        p.playlist.tracks[track].name = "Drums".into();
    }

    // Patterns: one per distinct bar, and the whole groove for plain bars;
    // runs of the same bar become one looping clip.
    let bpm = p.transport.bpm;
    let mut written: BTreeMap<String, Written> = BTreeMap::new();
    let mut kept_used = 0;
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut clips: Vec<Clip> = vec![];
    let mut i = 0;
    while i < bars.len() {
        let b = &bars[i];
        if grids[i].is_empty() && !part.kept.contains_key(&slots[i]) {
            i += 1;
            continue;
        }
        let (key, name, pattern_grids, offset, mut j) = if let (
            true,
            Body::Groove {
                groove,
                b: is_b,
                index,
            },
        ) = (b.plain(), &b.body)
        {
            let n = groove_bars(groove);
            let all: Vec<Bar> = (0..n)
                .map(|k| Bar::of_groove(&part, groove, *is_b, k))
                .collect();
            let mut j = i + 1;
            while j < bars.len()
                && bars[j].plain()
                && matches!(&bars[j].body, Body::Groove { groove: g2, b: b2, index: k }
                    if g2.id == groove.id && b2 == is_b && *k == (index + j - i) % n)
                && (bars[j].start - (b.start + (j - i) as f64 * b.bar_beats)).abs() < 1e-9
            {
                j += 1;
            }
            (
                slots[i].clone(),
                b.name(),
                all,
                *index as f64 * b.bar_beats,
                j,
            )
        } else {
            (
                slots[i].clone(),
                b.name(),
                vec![grids[i].clone()],
                0.0,
                i + 1,
            )
        };
        if !b.plain() {
            while j < bars.len()
                && slots[j] == slots[i]
                && (bars[j].start - (b.start + (j - i) as f64 * b.bar_beats)).abs() < 1e-9
            {
                j += 1;
            }
        }
        let id = match ids.get(&key) {
            Some(id) => id.clone(),
            None => {
                let mut taken: Vec<String> = p.patterns.iter().map(|x| x.id.clone()).collect();
                taken.extend(ids.values().cloned());
                let id = unique_id(&format!("drums {name}"), &taken);
                let notes = match part.kept.get(&key) {
                    // Edited by hand: note for note, on this kit.
                    Some(k) => {
                        kept_used += 1;
                        let mut notes: Vec<Note> = k
                            .notes
                            .iter()
                            .filter_map(|n| {
                                let (channel, pitch) = kit.resolve(p, &n.role)?;
                                Some(Note {
                                    channel,
                                    pitch,
                                    start: n.start,
                                    length: n.length,
                                    velocity: n.velocity,
                                })
                            })
                            .collect();
                        notes.sort_by(|a, b| {
                            a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch))
                        });
                        notes
                    }
                    None => notes_of(
                        &pattern_grids,
                        b.bar_beats,
                        b.steps_per_beat(),
                        &kit,
                        &part,
                        bpm,
                        &key,
                    ),
                };
                let pat = Pattern {
                    id: id.clone(),
                    name: format!("Drums · {name}"),
                    color: "#8e3b46".into(),
                    length: pattern_grids.len() as f64 * b.bar_beats,
                    notes,
                };
                written.insert(
                    id.clone(),
                    Written {
                        slot: key.clone(),
                        print: fingerprint(&pat),
                    },
                );
                p.patterns.push(pat);
                ids.insert(key, id.clone());
                id
            }
        };
        clips.push(Clip {
            pattern: id,
            sample: String::new(),
            track: TrackIx(track as u32),
            start: round(b.start),
            length: round((j - i) as f64 * b.bar_beats),
            offset: round(offset),
            gain: 1.0,
            mixer: Default::default(),
        });
        i = j;
    }
    let report = Report {
        patterns: written.len(),
        clips: clips.len(),
        track,
        kept: kept_used,
    };
    p.playlist.clips.extend(clips);
    drop_unused(p, &old_channels);
    if let Some(d) = &mut p.drums {
        d.written = written;
    }
    Ok(report)
}

/// `POST /api/drums`: the project (JSON text) in; with `guess`, the part's
/// start and sections are first guessed from the playlist (a part is
/// started on the first groove if there is none); with `write`, the part is
/// written. Out: the project, the patterns that had been edited by hand
/// since the last write (replaced now), and what the write did.
pub fn api_write(text: &str, guess: bool, write_it: bool) -> Result<serde_json::Value, String> {
    let mut p: Project = serde_json::from_str(text).map_err(|e| e.to_string())?;
    // A valid project in, a valid project out (and no request sizes the
    // work beyond what validation allows).
    let invalid = |p: &Project| {
        crate::validate::validate(p)
            .into_iter()
            .find(|i| i.severity == crate::validate::Severity::Error)
    };
    if let Some(e) = invalid(&p) {
        return Err(format!("the project is invalid: {e}"));
    }
    if guess {
        let (start, sections) = guess_sections(&p);
        let part = p.drums.get_or_insert_with(|| DrumPart::new(GROOVES[0].id));
        part.start = start;
        part.sections = sections;
    }
    let edited = edited(&p);
    let report = if write_it { Some(write(&mut p)?) } else { None };
    if let Some(e) = invalid(&p) {
        return Err(format!("the written project is invalid: {e}"));
    }
    Ok(serde_json::json!({"project": p, "edited": edited, "report": report}))
}

/// Does a channel play drums (an Ebony channel or a General MIDI kit)?
pub fn is_drum_channel(c: &Channel) -> bool {
    c.instrument.kind == "drum"
        || (c.instrument.kind == "soundfont" && crate::gm::is_kit(c.instrument.option("program")))
}

/// Latest bar a drum part may start on (counted from 1).
pub const MAX_START: u32 = 9999;

/// Shortest section a guess makes, in bars: shorter changes join the
/// section before.
const MIN_SECTION: u32 = 4;

/// Remove the drum channels among `ids` that nothing plays any more: no
/// notes, no layer on them, no automation (a kit change leaves them behind).
fn drop_unused(p: &mut Project, ids: &[String]) {
    let unused: Vec<String> = ids
        .iter()
        .filter(|id| {
            p.channel(id).is_some_and(is_drum_channel)
                && !p
                    .patterns
                    .iter()
                    .any(|x| x.notes.iter().any(|n| &n.channel == *id))
                && !p
                    .channels
                    .iter()
                    .any(|c| c.layer_of.as_deref() == Some(id.as_str()))
                && !p
                    .automation
                    .iter()
                    .any(|l| l.target.starts_with(&format!("channel/{id}/")))
                // The score's colored passages and the film's shots name it.
                && !p.score.marks.iter().any(|m| m.channels.contains(id))
                && !p
                    .animation
                    .iter()
                    .flat_map(|a| a.shots.iter())
                    .any(|s| s.focus.contains(id))
        })
        .cloned()
        .collect();
    p.channels.retain(|c| !unused.contains(&c.id));
    p.score.hidden.retain(|c| !unused.contains(c));
    p.score.clefs.retain(|c, _| !unused.contains(c));
}

/// Sections guessed from the playlist: a new section wherever the set of
/// patterns playing changes (drum-only patterns aside), at least
/// [`MIN_SECTION`] bars long, named from the score's passage labels when one
/// starts there. Bars are counted from 1.
pub fn guess_sections(p: &Project) -> (u32, Vec<DrumSection>) {
    let ours: Vec<&String> = p.drums.iter().flat_map(|d| d.written.keys()).collect();
    let drums_only = |id: &str| {
        p.pattern(id).is_some_and(|pat| {
            !pat.notes.is_empty()
                && pat
                    .notes
                    .iter()
                    .all(|n| p.channel(&n.channel).is_some_and(is_drum_channel))
        })
    };
    let clips: Vec<&Clip> = p
        .playlist
        .clips
        .iter()
        .filter(|c| !c.pattern.is_empty() && !ours.contains(&&c.pattern) && !drums_only(&c.pattern))
        .collect();
    let end = clips.iter().map(|c| c.start + c.length).fold(0.0, f64::max);
    if end <= 0.0 {
        return (
            1,
            vec![DrumSection {
                name: "Verse".into(),
                bars: 8,
                play: "a".into(),
                fill: "none".into(),
                crash: false,
                groove: String::new(),
            }],
        );
    }
    let t = &p.transport;
    let (last_bar, _, _) = t.bar_at(end - 1e-6);
    // (A part starts by bar MAX_START; a clip far beyond does not stretch the guess.)
    let last_bar = last_bar.min(MAX_START + 999);
    let playing = |bar: u32| -> Vec<&str> {
        let (s, e) = (t.bar_start(bar), t.bar_start(bar + 1));
        let mut v: Vec<&str> = clips
            .iter()
            .filter(|c| c.start < e - 1e-9 && c.start + c.length > s + 1e-9)
            .map(|c| c.pattern.as_str())
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let first = (0..=last_bar)
        .find(|&b| !playing(b).is_empty())
        .unwrap_or(0);
    let mut cuts = vec![first];
    let mut prev = playing(first);
    for bar in first + 1..=last_bar {
        let now = playing(bar);
        if now != prev {
            cuts.push(bar);
            prev = now;
        }
    }
    cuts.push(last_bar + 1);
    // Join short sections to the one before (the first to the one after).
    while cuts.len() > 2 {
        let Some(i) = (0..cuts.len() - 1).find(|&i| cuts[i + 1] - cuts[i] < MIN_SECTION) else {
            break;
        };
        cuts.remove(if i == 0 { 1 } else { i });
    }
    // What plays in each section: its patterns, and a name — a score passage
    // label starting there, else the part of the pattern names after " · "
    // that at least a third of them share
    // ("Bass · Chorus", "Keys · Chorus").
    let spans: Vec<(u32, u32, Vec<&str>, String)> = cuts
        .windows(2)
        .map(|w| {
            let mut pats: Vec<&str> = (w[0]..w[1]).flat_map(&playing).collect();
            pats.sort();
            pats.dedup();
            let start = t.bar_start(w[0]);
            let label = p
                .score
                .marks
                .iter()
                .find(|m| {
                    m.pattern.is_empty() && !m.label.is_empty() && (m.start - start).abs() < 1e-6
                })
                .map(|m| m.label.clone());
            let name = label.unwrap_or_else(|| {
                let mut counts: Vec<(&str, usize)> = vec![];
                for id in &pats {
                    let name = p.pattern(id).map(|x| x.name.as_str()).unwrap_or("");
                    if let Some((_, suffix)) = name.rsplit_once(" · ") {
                        match counts.iter_mut().find(|(s, _)| *s == suffix) {
                            Some(c) => c.1 += 1,
                            None => counts.push((suffix, 1)),
                        }
                    }
                }
                counts
                    .iter()
                    .max_by_key(|(_, n)| *n)
                    .filter(|(_, n)| *n * 3 >= pats.len())
                    .map(|(s, _)| s.to_string())
                    .unwrap_or_default()
            });
            (w[0], w[1], pats, name)
        })
        .collect();
    // Neighbours with the same name are one section.
    let mut merged: Vec<(u32, u32, Vec<&str>, String)> = vec![];
    for s in spans {
        match merged.last_mut() {
            Some(last) if !s.3.is_empty() && last.3 == s.3 => {
                last.1 = s.1;
                for id in s.2 {
                    if !last.2.contains(&id) {
                        last.2.push(id);
                    }
                }
            }
            _ => merged.push(s),
        }
    }
    // The fuller sections (more parts playing than usual) get groove B.
    let mut sizes: Vec<usize> = merged.iter().map(|s| s.2.len()).collect();
    sizes.sort();
    let median = sizes[(sizes.len() - 1) / 2];
    let n = merged.len();
    let out = merged
        .into_iter()
        .enumerate()
        .map(|(k, (a, b, pats, name))| DrumSection {
            name: if name.is_empty() {
                format!("Section {}", k + 1)
            } else {
                name
            },
            bars: b - a,
            play: if k > 0 && pats.len() > median {
                "b"
            } else {
                "a"
            }
            .into(),
            fill: if k + 1 < n { "beat" } else { "none" }.into(),
            crash: k > 0,
            groove: String::new(),
        })
        .collect();
    (first + 1, out)
}

/// The library as JSON, for the studio (`GET /api/grooves`).
pub fn catalog() -> serde_json::Value {
    use serde_json::json;
    let rows = |rs: &[(&str, &str)]| -> serde_json::Value {
        rs.iter().map(|(r, s)| json!([r, s])).collect()
    };
    let mut kits: Vec<&str> = crate::gm::kit_names().collect();
    kits.push(EBONY);
    json!({
        "grooves": GROOVES.iter().map(|g| json!({
            "id": g.id,
            "style": g.style,
            "name": g.name,
            "meter": g.meter,
            "barBeats": g.bar_beats,
            "steps": g.steps,
            "tempo": [g.tempo.0, g.tempo.1],
            "kit": g.kit,
            "swing": g.swing,
            "a": rows(g.a),
            "b": rows(g.b),
        })).collect::<Vec<_>>(),
        "kits": kits,
        "plays": PLAYS,
        "fills": FILL_SIZES,
        "feels": FEELS,
        "endings": ENDINGS,
    })
}

/// The library as text (`rosaclef grooves`).
pub fn grooves_text() -> String {
    let mut s = String::new();
    let mut style = "";
    for g in GROOVES {
        if g.style != style {
            style = g.style;
            s.push_str(&format!("{style}\n"));
        }
        s.push_str(&format!(
            "  {:<16} {:<20} {:<4} {:>3}–{:<3} BPM  {}\n",
            g.id, g.name, g.meter, g.tempo.0, g.tempo.1, g.kit
        ));
    }
    s
}

/// Problems that would make a groove or fill unplayable by one drummer at
/// `bpm`: more than two hands at once, a foot doing two things, or a hand
/// faster than 12 strokes a second.
pub fn playability(rows: &[(&str, &str)], steps_per_beat: u32, bpm: u32) -> Vec<String> {
    let rows: Vec<(&str, Vec<char>)> = rows.iter().map(|(r, s)| (*r, steps(s))).collect();
    let n = rows.iter().map(|(_, s)| s.len()).max().unwrap_or(0);
    let hands = |i: usize| -> usize {
        rows.iter()
            .filter(|(r, s)| limb(r) == 'H' && level(s[i % s.len()]).is_some())
            .count()
    };
    let foot = |i: usize, l: char| -> bool {
        rows.iter()
            .any(|(r, s)| limb(r) == l && level(s[i % s.len()]).is_some())
    };
    let rate = steps_per_beat as f64 * bpm as f64 / 60.0;
    let mut out = vec![];
    for i in 0..n {
        if hands(i) > 2 {
            out.push(format!("step {}: {} hand strokes at once", i + 1, hands(i)));
        }
        if i + 1 < n && hands(i) + hands(i + 1) > 2 && rate > 12.0 {
            out.push(format!(
                "steps {}–{}: one hand plays {rate:.1} strokes a second",
                i + 1,
                i + 2
            ));
        }
        // Fast kicks on neighbouring steps need both feet (a double pedal),
        // so the left foot cannot keep the hat then.
        if i + 1 < n
            && rate > 11.0
            && foot(i, 'R')
            && foot(i + 1, 'R')
            && (foot(i, 'L') || foot(i + 1, 'L'))
        {
            out.push(format!(
                "steps {}–{}: double kick under the hat foot",
                i + 1,
                i + 2
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests;
