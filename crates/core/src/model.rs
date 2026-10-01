//! The Rosaclef project model.
//!
//! A project is a single JSON document (`project.json`) that is shared by the
//! UI, the audio engine and any coding agent working on the song. All time
//! values are measured in **beats** (quarter notes) so that edits read
//! naturally: `"start": 4` is the downbeat of bar 2 in 4/4.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Identifier of the current document format.
pub const FORMAT: &str = "rosaclef/1";

/// Index of a mixer insert (`mixer.inserts[i]`; 0 is the master bus).
#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
#[serde(transparent)]
pub struct InsertIx(pub u32);

impl InsertIx {
    pub const MASTER: InsertIx = InsertIx(0);
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of a playlist track (`playlist.tracks[i]`).
#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
#[serde(transparent)]
pub struct TrackIx(pub u32);

impl TrackIx {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl std::fmt::Display for InsertIx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for TrackIx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    /// Optional pointer to the JSON schema, for editors.
    #[serde(rename = "$schema", default, skip_serializing_if = "String::is_empty")]
    pub schema: String,
    pub format: String,
    pub meta: Meta,
    pub transport: Transport,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    #[serde(default)]
    pub playlist: Playlist,
    pub mixer: Mixer,
    /// Automation lanes: values that change over song time (tempo ramps,
    /// filter sweeps, fades). See [`crate::automation`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<AutomationLane>,
    /// How the song reads as sheet music: the key, which parts show, clefs
    /// and colored passages. It changes nothing that plays.
    #[serde(default, skip_serializing_if = "Score::is_empty")]
    pub score: Score,
}

/// Sheet-music settings (the studio's Score view). Notes stay in the
/// patterns; this only says how to write them down.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Score {
    /// Key signature: `auto` (or empty: guessed from the notes), or a key
    /// such as `C`, `Eb`, `F#`, `Am`, `C#m` (see [`SCORE_KEYS`]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// Channels whose staves are hidden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    /// Playlist tracks left out of the song's score.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_tracks: Vec<TrackIx>,
    /// Clef per channel id (see [`SCORE_CLEFS`]); unlisted channels pick one
    /// from their range.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clefs: BTreeMap<String, String>,
    /// Colored passages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<ScoreMark>,
}

impl Score {
    pub fn is_empty(&self) -> bool {
        *self == Score::default()
    }
}

/// Key signatures a score may name (minor keys end in `m`).
pub const SCORE_KEYS: &[&str] = &[
    "auto", "C", "G", "D", "A", "E", "B", "F#", "C#", "F", "Bb", "Eb", "Ab", "Db", "Gb", "Cb",
    "Am", "Em", "Bm", "F#m", "C#m", "G#m", "D#m", "A#m", "Dm", "Gm", "Cm", "Fm", "Bbm", "Ebm",
    "Abm",
];

/// Clefs a staff may use: `grand` is a piano's treble + bass pair.
pub const SCORE_CLEFS: &[&str] = &[
    "auto",
    "treble",
    "treble8vb",
    "bass",
    "alto",
    "grand",
    "percussion",
];

/// A colored passage of the score: from `start` to `end` (beats), on some
/// channels or all of them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScoreMark {
    /// Start, in beats: song time, or pattern time when `pattern` is set.
    pub start: f64,
    /// End, in beats (after `start`).
    pub end: f64,
    pub color: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// A pattern id: the passage is in that pattern's time and colors it
    /// wherever it plays. Empty: song time.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pattern: String,
    /// Channel ids the color applies to; empty = every staff.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
}

/// One automation lane: a breakpoint curve driving a single target over the
/// arrangement (song mode).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationLane {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// What the lane drives, e.g. `tempo`, `channel/pad/cutoff`,
    /// `insert/3/effect/0/mix` (see [`crate::automation::AutomationTarget`]).
    pub target: String,
    #[serde(default = "default_lane_color")]
    pub color: String,
    /// A muted lane is ignored (the target keeps its project value).
    #[serde(default, skip_serializing_if = "is_false")]
    pub mute: bool,
    /// Breakpoints, sorted by beat.
    #[serde(default)]
    pub points: Vec<AutomationPoint>,
}

/// A breakpoint of an automation lane.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationPoint {
    /// Absolute song time in beats.
    pub beat: f64,
    /// Value in the target's natural units (BPM, Hz, linear gain, 0..1, ...).
    pub value: f64,
    /// Shape of the segment that *ends* at this point (-1..1): 0 = linear,
    /// > 0 changes slowly first and fast at the end, < 0 the opposite.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub curve: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Meta {
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transport {
    pub bpm: f64,
    /// Beats (quarter notes) per bar, until the first entry of `meters`.
    #[serde(default = "default_beats_per_bar")]
    pub beats_per_bar: u32,
    /// 0 = straight, 1 = full triplet swing on 16th notes.
    #[serde(default)]
    pub swing: f64,
    /// Time-signature changes, sorted by bar. Each one holds from its bar
    /// until the next; bars before the first one have `beats_per_bar` beats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meters: Vec<Meter>,
}

/// A time-signature change. A bar lasts `4 × numerator / denominator` beats
/// (6/8 → 3 beats, 7/8 → 3.5 beats).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Meter {
    /// First bar in the new meter, counted from 1 as displayed.
    pub bar: u32,
    pub numerator: u32,
    /// A power of two, 1..32.
    pub denominator: u32,
}

impl Meter {
    /// Length of one bar in beats.
    pub fn bar_beats(&self) -> f64 {
        4.0 * self.numerator as f64 / self.denominator.max(1) as f64
    }
}

/// A run of bars in one meter (see [`Transport::meter_map`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterSpan {
    /// First bar of the run, counted from 0.
    pub bar: u32,
    /// Song time of that bar's downbeat, in beats.
    pub beat: f64,
    /// Length of each bar in beats.
    pub bar_beats: f64,
}

impl Transport {
    /// The bar grid: one span per meter, the first starting at bar 0, beat 0.
    /// Changes that are out of order or not after the previous one are
    /// skipped (validation reports them).
    pub fn meter_map(&self) -> Vec<MeterSpan> {
        let mut out = vec![MeterSpan {
            bar: 0,
            beat: 0.0,
            bar_beats: self.beats_per_bar.max(1) as f64,
        }];
        for m in &self.meters {
            let bar = m.bar.max(1) - 1;
            let bar_beats = m.bar_beats();
            if !(bar_beats > 0.0 && bar_beats.is_finite()) {
                continue;
            }
            let last = *out.last().expect("non-empty");
            if bar == 0 && out.len() == 1 {
                out[0].bar_beats = bar_beats;
            } else if bar > last.bar {
                out.push(MeterSpan {
                    bar,
                    beat: last.beat + (bar - last.bar) as f64 * last.bar_beats,
                    bar_beats,
                });
            }
        }
        out
    }

    /// The bar (counted from 0) containing `beat`, with its downbeat and length.
    pub fn bar_at(&self, beat: f64) -> (u32, f64, f64) {
        let map = self.meter_map();
        let i = map.partition_point(|s| s.beat <= beat + 1e-9).max(1) - 1;
        let s = map[i];
        let k = ((beat - s.beat) / s.bar_beats + 1e-9).floor().max(0.0);
        (s.bar + k as u32, s.beat + k * s.bar_beats, s.bar_beats)
    }

    /// Song time of the downbeat of `bar` (counted from 0).
    pub fn bar_start(&self, bar: u32) -> f64 {
        let map = self.meter_map();
        let i = map.partition_point(|s| s.bar <= bar).max(1) - 1;
        let s = map[i];
        s.beat + (bar - s.bar) as f64 * s.bar_beats
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    pub instrument: Device,
    #[serde(default = "default_channel_volume")]
    pub volume: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub mute: bool,
    /// Mixer insert this channel is routed to (0 = master).
    #[serde(default)]
    pub mixer: InsertIx,
}

/// An instrument or an effect: a device type plus its settings.
///
/// `params` holds continuous numeric settings and `options` holds discrete
/// textual settings. The legal keys, ranges and choices for every device type
/// are defined by the catalog (see [`crate::catalog`]).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Device {
    #[serde(rename = "type")]
    pub kind: String,
    /// Effects only: bypassed when false.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pattern {
    pub id: String,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    /// Length in beats.
    pub length: f64,
    #[serde(default)]
    pub notes: Vec<Note>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Note {
    pub channel: String,
    /// MIDI note number, 60 = middle C (C4).
    pub pitch: i32,
    /// Start, in beats from the beginning of the pattern.
    pub start: f64,
    /// Duration in beats.
    pub length: f64,
    #[serde(default = "default_velocity")]
    pub velocity: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Playlist {
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub clips: Vec<Clip>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Track {
    pub name: String,
    #[serde(default)]
    pub mute: bool,
}

/// A clip on the playlist. Exactly one of `pattern` or `sample` is set.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Clip {
    /// Pattern id (pattern clip). Patterns loop to fill the clip length.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pattern: String,
    /// Project-relative path of an audio file (audio clip).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sample: String,
    /// Playlist track.
    pub track: TrackIx,
    /// Start on the timeline, in beats.
    pub start: f64,
    /// Length on the timeline, in beats.
    pub length: f64,
    /// Offset into the pattern/sample where playback begins, in beats.
    #[serde(default)]
    pub offset: f64,
    /// Audio clips: linear gain.
    #[serde(default = "default_one")]
    pub gain: f64,
    /// Audio clips: mixer insert to play through (0 = master).
    #[serde(default)]
    pub mixer: InsertIx,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mixer {
    /// Insert 0 is the master bus; every other insert feeds the master.
    pub inserts: Vec<Insert>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Insert {
    pub name: String,
    /// Linear gain, 1.0 = 0 dB.
    #[serde(default = "default_one")]
    pub volume: f64,
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub effects: Vec<Device>,
}

fn default_beats_per_bar() -> u32 {
    4
}
fn default_color() -> String {
    "#c9a45c".into()
}
fn default_channel_volume() -> f64 {
    0.8
}
fn default_velocity() -> f64 {
    0.8
}
fn default_one() -> f64 {
    1.0
}
fn default_true() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(x: &f64) -> bool {
    *x == 0.0
}
fn default_lane_color() -> String {
    "#8a6bb0".into()
}

impl Project {
    /// A new, empty project with a master bus and a handful of inserts.
    pub fn empty(title: &str) -> Project {
        let mut inserts = vec![Insert::new("Master")];
        inserts[0].effects.push(Device::new("limiter"));
        for i in 1..=8 {
            inserts.push(Insert::new(&format!("Insert {i}")));
        }
        Project {
            schema: String::new(),
            format: FORMAT.into(),
            meta: Meta {
                title: title.into(),
                ..Default::default()
            },
            transport: Transport {
                bpm: 120.0,
                beats_per_bar: 4,
                swing: 0.0,
                meters: vec![],
            },
            channels: vec![],
            patterns: vec![Pattern {
                id: "pattern-1".into(),
                name: "Pattern 1".into(),
                color: default_color(),
                length: 4.0,
                notes: vec![],
            }],
            playlist: Playlist {
                tracks: (1..=8)
                    .map(|i| Track {
                        name: format!("Track {i}"),
                        mute: false,
                    })
                    .collect(),
                clips: vec![],
            },
            mixer: Mixer { inserts },
            automation: vec![],
            score: Score::default(),
        }
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn pattern(&self, id: &str) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }

    pub fn lane(&self, id: &str) -> Option<&AutomationLane> {
        self.automation.iter().find(|l| l.id == id)
    }

    /// Seconds per beat at the project tempo.
    pub fn seconds_per_beat(&self) -> f64 {
        60.0 / self.transport.bpm.max(1.0)
    }

    /// End of the arrangement in beats (end of the last clip).
    pub fn song_length(&self) -> f64 {
        self.playlist
            .clips
            .iter()
            .map(|c| c.start + c.length)
            .fold(0.0, f64::max)
    }
}

impl Insert {
    pub fn new(name: &str) -> Insert {
        Insert {
            name: name.into(),
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            effects: vec![],
        }
    }
}

impl Device {
    pub fn new(kind: &str) -> Device {
        Device {
            kind: kind.into(),
            enabled: true,
            ..Default::default()
        }
    }

    /// Numeric parameter, falling back to the catalog default.
    pub fn param(&self, key: &str) -> f64 {
        if let Some(v) = self.params.get(key) {
            return *v;
        }
        crate::catalog::device(&self.kind)
            .and_then(|d| d.param(key))
            .map(|p| p.default)
            .unwrap_or(0.0)
    }

    /// Textual option, falling back to the catalog default.
    pub fn option(&self, key: &str) -> &str {
        if let Some(v) = self.options.get(key) {
            return v;
        }
        crate::catalog::device(&self.kind)
            .and_then(|d| d.option(key))
            .map(|o| o.default)
            .unwrap_or("")
    }
}
