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
    #[serde(default = "default_beats_per_bar")]
    pub beats_per_bar: u32,
    /// 0 = straight, 1 = full triplet swing on 16th notes.
    #[serde(default)]
    pub swing: f64,
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
