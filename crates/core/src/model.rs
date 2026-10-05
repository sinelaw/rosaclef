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
    /// Repeated passages of the arrangement, as repeat signs in a score: the
    /// song plays them `times` times, with endings for some passes. See
    /// [`crate::form`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repeats: Vec<Repeat>,
    /// The film of the song: a camera over the score's pages on a desk,
    /// moving with the music (the studio's Film view). It changes nothing
    /// that plays. See [`Animation`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Animation>,
    /// The drum part: a groove, a kit and what each section plays. Writing
    /// it makes ordinary patterns; it changes nothing that plays by itself.
    /// See [`crate::drums`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drums: Option<crate::drums::DrumPart>,
    /// What the Critic leaves out: checks turned off and findings
    /// suppressed. See [`crate::critic`].
    #[serde(default, skip_serializing_if = "CriticSettings::is_empty")]
    pub critic: CriticSettings,
}

/// The Critic's settings for a project (`critic` in project.json): like a
/// linter's configuration, they travel with the song, so the studio, the
/// command line and an agent all leave out the same things.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CriticSettings {
    /// Checks turned off, by rule id (`rosaclef critic --rules` lists them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
    /// Checks that are off by default (the classical theory ones) turned on,
    /// by rule id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<String>,
    /// Findings suppressed one by one, by their key (as `rosaclef critic`
    /// prints it): reported as suppressed, not as something to do.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppress: Vec<String>,
}

impl CriticSettings {
    pub fn is_empty(&self) -> bool {
        self.off.is_empty() && self.on.is_empty() && self.suppress.is_empty()
    }
}

/// The film of the song (the Score view's Film mode): the score's pages lie
/// on a desk and a camera in 3D space above them follows the music —
/// zooming, tilting and turning to frame one part or the whole band.
///
/// In `auto` mode the camera directs itself: it mostly shows the full score,
/// and follows a part for a short while as it comes in, takes the lead for a
/// few phrases, or plays alone. In `manual` mode it plays `shots`, and directs
/// itself wherever no shot covers the song.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Animation {
    /// `auto` or `manual` (see [`FILM_MODES`]).
    #[serde(default = "default_film_mode")]
    pub mode: String,
    /// What is filmed: `score` (the only view so far).
    #[serde(default = "default_film_view")]
    pub view: String,
    /// The desk the pages lie on (see [`FILM_SURFACES`]).
    #[serde(default = "default_film_surface")]
    pub surface: String,
    /// How much the self-directed camera moves: 0 calm … 1 restless.
    #[serde(default = "default_energy")]
    pub energy: f64,
    /// Effects over the whole film; a shot's own effects override these by
    /// type. Unlisted effects keep their default amounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<FilmEffect>,
    /// The camera's shots (manual mode), in song beats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shots: Vec<Shot>,
}

impl Default for Animation {
    fn default() -> Self {
        Animation {
            mode: default_film_mode(),
            view: default_film_view(),
            surface: default_film_surface(),
            energy: default_energy(),
            effects: vec![],
            shots: vec![],
        }
    }
}

fn default_film_mode() -> String {
    "auto".into()
}

fn default_film_view() -> String {
    "score".into()
}

fn default_film_surface() -> String {
    "walnut".into()
}

fn default_energy() -> f64 {
    0.5
}

/// One shot of the film: from `start` to `end` (song beats) the camera frames
/// some staves — `focus` channels, or a `role` — at a `frame` size, seen from
/// `tilt` and `turn`, following the playhead (or looking `at` a fixed beat).
/// It moves into the shot over `glide` beats, by `transition`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shot {
    pub start: f64,
    pub end: f64,
    /// A name for the shot (shown on the film's timeline).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Channel ids whose staves are framed. Empty: `role`, or every staff.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus: Vec<String>,
    /// The part of the band to frame when `focus` is empty (see [`FILM_ROLES`]):
    /// the camera finds which channels play it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// How much the frame holds (see [`FILM_FRAMES`]); empty: `close`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub frame: String,
    /// Closer (> 1) or farther (< 1) than the frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    /// Degrees the camera leans from looking straight down (0..75).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<f64>,
    /// Degrees the camera turns about the vertical: the music runs
    /// diagonally across the picture (-180..180).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<f64>,
    /// Shift of the framed point, as fractions of the frame `[x, y]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset: Vec<f64>,
    /// A song beat to look at for the whole shot, instead of following the
    /// playhead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
    /// Where the camera drifts to by the end of the shot (a push in, a slow
    /// turn): `zoom`, `tilt`, `turn` and `offset` at `end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<CameraMove>,
    /// How the camera comes into the shot (see [`FILM_TRANSITIONS`]); empty: `glide`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub transition: String,
    /// Beats the move into the shot takes (default: up to a bar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glide: Option<f64>,
    /// The curve of the moves (see [`FILM_EASES`]); empty: `smooth`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ease: String,
    /// Effects during the shot (they override the film's by type).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<FilmEffect>,
}

/// The camera at the end of a shot: values left out stay as at its start.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraMove {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset: Vec<f64>,
}

/// A visual effect of the film (see [`FILM_EFFECTS`]), at an `amount` of 0..1
/// (0 turns it off).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilmEffect {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default = "default_amount")]
    pub amount: f64,
}

fn default_amount() -> f64 {
    1.0
}

/// How the film is directed: `auto` (the camera directs itself) or `manual`
/// (it plays the shots, and directs itself between them).
pub const FILM_MODES: &[&str] = &["auto", "manual"];
/// What the film shows.
pub const FILM_VIEWS: &[&str] = &["score"];
/// Desks the pages may lie on.
pub const FILM_SURFACES: &[&str] = &["walnut", "oak", "slate", "felt", "marble"];
/// Frame sizes, widest first: every page, the page, the whole line (system),
/// about two bars, one bar, a beat (close enough to see the ink).
pub const FILM_FRAMES: &[&str] = &["desk", "page", "system", "medium", "close", "detail"];
/// Parts of the band a shot can frame without naming channels: the melody,
/// the drums and bass, the chords and pads, everyone.
pub const FILM_ROLES: &[&str] = &["lead", "rhythm", "background", "all"];
/// How the camera enters a shot: a smooth move, a cut, a move that rises
/// away from the desk and comes down again, a fast whip.
pub const FILM_TRANSITIONS: &[&str] = &["glide", "cut", "swoop", "whip"];
/// The curves of camera moves.
pub const FILM_EASES: &[&str] = &["smooth", "linear", "in", "out", "snap"];
/// Effects: darkness at the picture's edges, a pool of light on the framed
/// staves, a glow on the notes as they play.
pub const FILM_EFFECTS: &[&str] = &["vignette", "spotlight", "glow"];

/// A repeated passage of the arrangement (`|: … :|`), in song beats.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Repeat {
    /// Where the passage starts (the start repeat sign).
    pub start: f64,
    /// Where it ends (the end repeat sign); playing returns to `start` here.
    pub end: f64,
    /// How many times it plays in all (2 = once more).
    #[serde(default = "default_times")]
    pub times: u32,
    /// Endings (voltas) played only on some passes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endings: Vec<Ending>,
}

/// An ending of a repeat ("1.", "2."): music played only on the listed passes.
/// An ending inside the passage is skipped on the other passes; the last
/// pass's ending usually follows the end repeat sign.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ending {
    pub start: f64,
    pub end: f64,
    /// The passes (counted from 1) that play it.
    pub passes: Vec<u32>,
}

fn default_times() -> u32 {
    2
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
    /// Semitones every pitched instrument is shifted by when it plays
    /// (-12..=12; to match a singer's range). The notes stay as written;
    /// drums and audio clips are not shifted.
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub transpose: i32,
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
    /// The channel's arpeggiator, when on: every note it plays becomes a
    /// run of notes (see [`crate::arp`]). The notes stay as written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arp: Option<Arpeggio>,
    /// A layer: the id of another channel whose notes this channel also
    /// plays (through its own instrument, arpeggiator, volume and mixer
    /// route). Layering two instruments thickens a part without copying
    /// notes. A layer cannot itself be layered on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_of: Option<String>,
}

/// An arpeggiator: while a note is held it plays the notes of `chord`
/// above it, over `octaves` octaves, one every `rate` beats.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Arpeggio {
    /// Chord the notes cycle through (see [`crate::arp::CHORDS`]): "octave"
    /// plays the note itself in each octave.
    #[serde(default = "default_arp_chord")]
    pub chord: String,
    /// Octaves the run spans (1..=8).
    #[serde(default = "default_arp_octaves")]
    pub octaves: u32,
    /// Beats from one note to the next (1/64..=4; 0.25 = sixteenths).
    pub rate: f64,
    /// "up", "down", "updown", "downup" or "random".
    #[serde(default = "default_arp_direction")]
    pub direction: String,
    /// Length of each note as a fraction of `rate` (0.05..=2).
    #[serde(default = "default_arp_gate")]
    pub gate: f64,
    /// "free": every held note runs its own arpeggio; "sort": notes struck
    /// together take turns, lowest first, as one arpeggio.
    #[serde(default = "default_arp_mode")]
    pub mode: String,
}

impl Default for Arpeggio {
    fn default() -> Self {
        Arpeggio {
            chord: default_arp_chord(),
            octaves: default_arp_octaves(),
            rate: 0.25,
            direction: default_arp_direction(),
            gate: default_arp_gate(),
            mode: default_arp_mode(),
        }
    }
}

fn default_arp_chord() -> String {
    "octave".into()
}
fn default_arp_octaves() -> u32 {
    1
}
fn default_arp_direction() -> String {
    "up".into()
}
fn default_arp_gate() -> f64 {
    1.0
}
fn default_arp_mode() -> String {
    "free".into()
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
    /// A drum pattern made from a groove (the Drums tab): how it was made, so
    /// it can be made again with other settings ([`crate::drums::render_pattern`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drums: Option<crate::drums::PatternDrums>,
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
fn is_zero_i32(x: &i32) -> bool {
    *x == 0
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
                transpose: 0,
            },
            channels: vec![],
            patterns: vec![Pattern {
                id: "pattern-1".into(),
                name: "Pattern 1".into(),
                color: default_color(),
                length: 4.0,
                notes: vec![],
                drums: None,
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
            repeats: vec![],
            animation: None,
            drums: None,
            critic: CriticSettings::default(),
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
