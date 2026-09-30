//! Standard MIDI File import (format 0 and 1; format 2 is read like 1).
//!
//! - Time: `beats = ticks / PPQ` (SMPTE time bases are converted at the file
//!   tempo, with a warning).
//! - Tempo: the first tempo event sets the project BPM; later changes become
//!   vertical steps on a `tempo` automation lane, so the song speeds up and
//!   slows down where the file does.
//! - Time signatures: the first one sets `beatsPerBar`; changes become
//!   `transport.meters` entries on the bar where they happen (a change in
//!   the middle of a bar moves to the next bar line, with a warning). Eighth
//!   and odd meters keep their exact length (7/8 bars are 3.5 beats).
//! - Notes: note-on with velocity 0 is a note-off; overlapping notes on the
//!   same key pair first-in, first-out; velocity is `v / 127`. The sustain
//!   pedal (CC 64) is applied: notes released while it is down ring until it
//!   lifts or the key is struck again. Pitch bend is reported, not applied.
//! - Channels: one Rosaclef channel per (track, MIDI channel, program) with
//!   notes, so program changes switch instruments. The instrument comes from
//!   the General MIDI program family, preferring a factory preset (see
//!   [`gm_instrument`]). MIDI channel 10 becomes one `drum` channel per GM
//!   drum group used (kick, snare, hats, toms, ...), notes remapped to pitch
//!   60 (toms keep their relative tuning).
//! - Mixing: volume × expression (CC 7, 11) and pan (CC 10) when a channel
//!   starts set its volume and pan; later changes become automation lanes.
//! - Arrangement: every channel's notes are cut into patterns of
//!   `bars_per_pattern` bars (restarting at each meter change); identical
//!   blocks share one pattern, and runs of the same block become one looping
//!   clip on the channel's own track. Each channel gets its own mixer insert.

use crate::{
    beats, clamp, color, ensure_valid, has_instrument, pad_tracks, set_option, set_param, Ids,
    Imported, Warnings, MAX_INSERTS,
};
use anyhow::{bail, Result};
use rosaclef_core::presets;
use rosaclef_core::{
    AutomationLane, AutomationPoint, Channel, Clip, Device, Insert, InsertIx, Meter, Note, Pattern,
    Project, Track, TrackIx, Transport,
};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Import settings.
#[derive(Clone, Debug)]
pub struct Options {
    pub title: String,
    /// Length of the patterns the song is cut into.
    pub bars_per_pattern: u32,
}

impl Options {
    pub fn new(title: &str) -> Options {
        Options {
            title: title.to_string(),
            bars_per_pattern: 4,
        }
    }
}

// ---------------------------------------------------------------- parser

/// Time base of a MIDI file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Division {
    /// Ticks per quarter note.
    Ppq(u16),
    /// Frames per second and ticks per frame.
    Smpte { fps: u8, ticks_per_frame: u8 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        key: u8,
    },
    Program {
        channel: u8,
        program: u8,
    },
    Controller {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// -8192..8191, 0 = centre.
    PitchBend {
        channel: u8,
        value: i16,
    },
    /// Microseconds per quarter note.
    Tempo(u32),
    /// Numerator and denominator as a power of two.
    TimeSignature {
        numerator: u8,
        denominator_pow: u8,
    },
    TrackName(String),
    InstrumentName(String),
    Other,
}

/// A parsed Standard MIDI File: events per track with absolute ticks.
#[derive(Clone, Debug)]
pub struct Smf {
    pub format: u16,
    pub division: Division,
    pub tracks: Vec<Vec<(u64, Event)>>,
    /// Problems found while parsing (truncated chunks, ...).
    pub warnings: Vec<String>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| anyhow::anyhow!("unexpected end of track data"))?;
        self.pos += 1;
        Ok(b)
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            bail!("unexpected end of track data");
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    /// Variable-length quantity (at most 4 bytes).
    fn vlq(&mut self) -> Result<u32> {
        let mut v: u32 = 0;
        for _ in 0..4 {
            let b = self.byte()?;
            v = (v << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        bail!("invalid variable-length quantity")
    }
    fn done(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// Parse a Standard MIDI File.
pub fn parse(bytes: &[u8]) -> Result<Smf> {
    let mut data = bytes;
    // RIFF-wrapped MIDI (.rmi): skip to the MThd chunk.
    if data.starts_with(b"RIFF") {
        if let Some(i) = data.windows(4).position(|w| w == b"MThd") {
            data = &data[i..];
        }
    }
    if data.len() < 14 || &data[0..4] != b"MThd" {
        bail!("not a Standard MIDI File (no MThd header)");
    }
    let hlen = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if hlen < 6 || data.len() < 8 + hlen {
        bail!("corrupt MIDI header");
    }
    let format = u16::from_be_bytes([data[8], data[9]]);
    let ntracks = u16::from_be_bytes([data[10], data[11]]);
    let div = u16::from_be_bytes([data[12], data[13]]);
    let division = if div & 0x8000 != 0 {
        let fps = (256 - (div >> 8)) as u8;
        Division::Smpte {
            fps,
            ticks_per_frame: (div & 0xff) as u8,
        }
    } else {
        if div == 0 {
            bail!("corrupt MIDI header (zero ticks per quarter note)");
        }
        Division::Ppq(div)
    };
    let mut warnings = vec![];
    let mut tracks = vec![];
    let mut pos = 8 + hlen;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let len = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let start = pos + 8;
        let end = start.saturating_add(len).min(data.len());
        if start + len > data.len() {
            warnings.push("the last MIDI track is truncated".to_string());
        }
        if id == b"MTrk" {
            let (events, err) = parse_track(&data[start..end]);
            if let Some(e) = err {
                warnings.push(format!(
                    "track {}: {e}; the rest of the track was ignored",
                    tracks.len() + 1
                ));
            }
            tracks.push(events);
        }
        pos = end;
    }
    if tracks.len() != ntracks as usize {
        warnings.push(format!(
            "the header announces {ntracks} tracks but {} were found",
            tracks.len()
        ));
    }
    if tracks.is_empty() {
        bail!("the MIDI file has no tracks");
    }
    Ok(Smf {
        format,
        division,
        tracks,
        warnings,
    })
}

/// Parse one track chunk. Returns the events read and, if the data was
/// corrupt, the error that stopped parsing.
fn parse_track(data: &[u8]) -> (Vec<(u64, Event)>, Option<String>) {
    let mut r = Reader { data, pos: 0 };
    let mut out = vec![];
    let mut tick: u64 = 0;
    let mut running: Option<u8> = None;
    let res: Result<()> = (|| {
        while !r.done() {
            tick += r.vlq()? as u64;
            let mut status = r.byte()?;
            let first_data;
            if status < 0x80 {
                // Running status: this byte is the first data byte.
                first_data = Some(status);
                status = running.ok_or_else(|| {
                    anyhow::anyhow!("data byte without a status (running status unset)")
                })?;
            } else {
                first_data = None;
            }
            let data1 = |r: &mut Reader| -> Result<u8> {
                match first_data {
                    Some(b) => Ok(b),
                    None => r.byte(),
                }
            };
            match status {
                0x80..=0xef => {
                    running = Some(status);
                    let ch = status & 0x0f;
                    let a = data1(&mut r)? & 0x7f;
                    let ev = match status & 0xf0 {
                        0x80 => {
                            r.byte()?;
                            Event::NoteOff {
                                channel: ch,
                                key: a,
                            }
                        }
                        0x90 => {
                            let v = r.byte()? & 0x7f;
                            if v == 0 {
                                Event::NoteOff {
                                    channel: ch,
                                    key: a,
                                }
                            } else {
                                Event::NoteOn {
                                    channel: ch,
                                    key: a,
                                    velocity: v,
                                }
                            }
                        }
                        0xa0 => {
                            r.byte()?;
                            Event::Other
                        }
                        0xe0 => {
                            let msb = r.byte()? & 0x7f;
                            Event::PitchBend {
                                channel: ch,
                                value: ((msb as i16) << 7 | a as i16) - 8192,
                            }
                        }
                        0xb0 => Event::Controller {
                            channel: ch,
                            controller: a,
                            value: r.byte()? & 0x7f,
                        },
                        0xc0 => Event::Program {
                            channel: ch,
                            program: a,
                        },
                        _ => Event::Other, // 0xd0 channel pressure: one data byte
                    };
                    out.push((tick, ev));
                }
                0xf0 | 0xf7 => {
                    let n = r.vlq()? as usize;
                    r.bytes(n)?;
                }
                0xff => {
                    let kind = r.byte()?;
                    let n = r.vlq()? as usize;
                    let d = r.bytes(n)?;
                    let text = || String::from_utf8_lossy(d).trim().to_string();
                    let ev = match kind {
                        0x03 => Event::TrackName(text()),
                        0x04 => Event::InstrumentName(text()),
                        0x2f => break,
                        0x51 if n >= 3 => {
                            Event::Tempo(((d[0] as u32) << 16) | ((d[1] as u32) << 8) | d[2] as u32)
                        }
                        0x58 if n >= 2 => Event::TimeSignature {
                            numerator: d[0],
                            denominator_pow: d[1],
                        },
                        _ => Event::Other,
                    };
                    if ev != Event::Other {
                        out.push((tick, ev));
                    }
                }
                _ => bail!("unexpected status byte 0x{status:02x}"),
            }
        }
        Ok(())
    })();
    (out, res.err().map(|e| e.to_string()))
}

// ---------------------------------------------------------------- instruments

/// GM program families (program / 8).
pub const GM_FAMILIES: [&str; 16] = [
    "Piano",
    "Chromatic Percussion",
    "Organ",
    "Guitar",
    "Bass",
    "Strings",
    "Ensemble",
    "Brass",
    "Reed",
    "Pipe",
    "Synth Lead",
    "Synth Pad",
    "Synth FX",
    "Ethnic",
    "Percussive",
    "Sound FX",
];

struct Choice {
    /// Preferred factory presets, by name.
    presets: &'static [&'static str],
    /// Otherwise: a preset of one of these engines (in order) with a matching tag.
    kinds: &'static [&'static str],
    tags: &'static [&'static str],
}

fn choice(program: u8) -> Choice {
    let c = |presets, kinds, tags| Choice {
        presets,
        kinds,
        tags,
    };
    match program / 8 {
        0 => c(
            &["Rhodes Lumière"],
            &["sextant", "fm", "prisme"],
            &["electric piano", "piano", "keys"],
        ),
        1 => c(
            &["Crystal Mallet", "Rosée de Cristal"],
            &["fm", "sextant", "prisme"],
            &["mallet", "bell"],
        ),
        2 => c(
            &["Nef d'Ivoire"],
            &["prisme", "sextant", "synth"],
            &["organ"],
        ),
        3 => c(
            &["Harpe de Saphir"],
            &["tessera", "prisme", "synth"],
            &["guitar", "pluck", "harp"],
        ),
        4 => c(&["Velvet Sub Bass"], &["cuivre", "synth"], &["bass"]),
        5 => c(
            &["Cordes Givrées", "Silk Unison Pad"],
            &["nebula", "prisme", "synth"],
            &["strings", "pad"],
        ),
        6 if (52..=54).contains(&program) => c(
            &["Voile de Chœur", "Séraphine"],
            &["prisme", "nebula"],
            &["choir"],
        ),
        6 => c(
            &["Cordes Givrées", "Silk Unison Pad"],
            &["nebula", "prisme", "synth"],
            &["strings", "pad"],
        ),
        7 => c(&[], &["cuivre", "synth"], &["brass"]),
        8 | 9 => c(
            &[],
            &["tessera", "cuivre", "synth"],
            &["reed", "flute", "lead"],
        ),
        10 => c(&["Gilded Lead"], &["tessera", "cuivre", "synth"], &["lead"]),
        11 => c(
            &["Silk Unison Pad", "Opaline Veil"],
            &["prisme", "nebula", "synth"],
            &["pad"],
        ),
        12 => c(
            &["Aurore Spectrale", "Poussière d'Astres"],
            &["nebula", "prisme"],
            &["texture", "cinematic", "pad"],
        ),
        13 => c(
            &["Harpe de Saphir"],
            &["tessera", "prisme", "synth"],
            &["pluck"],
        ),
        14 => c(
            &["Crystal Mallet"],
            &["fm", "sextant"],
            &["mallet", "perc", "bell"],
        ),
        _ => c(&[], &["nebula"], &["texture"]),
    }
}

/// Pick an instrument for a GM program: a named factory preset if it exists
/// in this build, else a preset of a preferred engine with a matching tag,
/// else a configured Aurum (`synth`). Returns the device and the preset name.
pub fn gm_instrument(program: u8) -> (Device, Option<&'static str>) {
    let ch = choice(program);
    let usable = |p: &&presets::Preset| has_instrument(p.kind);
    for name in ch.presets {
        if let Some(p) = presets::find(name).filter(usable) {
            return (p.device(), Some(p.name));
        }
    }
    let all = presets::all();
    for kind in ch.kinds {
        if !has_instrument(kind) {
            continue;
        }
        let hit = all.iter().find(|p| {
            p.kind == *kind
                && p.tags
                    .split(',')
                    .map(str::trim)
                    .any(|t| ch.tags.iter().any(|want| t.contains(want)))
        });
        if let Some(p) = hit {
            return (p.device(), Some(p.name));
        }
    }
    (fallback_synth(program), None)
}

/// An Aurum patch shaped roughly like the GM family.
fn fallback_synth(program: u8) -> Device {
    let mut d = Device::new("synth");
    let (w1, w2, cutoff, attack, decay, sustain, release) = match program / 8 {
        0 | 1 | 14 => ("triangle", "sine", 3500.0, 0.002, 0.9, 0.2, 0.4),
        2 => ("square", "sine", 5000.0, 0.005, 0.2, 1.0, 0.08),
        3 | 13 => ("saw", "square", 2600.0, 0.002, 0.35, 0.1, 0.2),
        4 => ("saw", "square", 700.0, 0.003, 0.25, 0.6, 0.08),
        5 | 6 | 11 | 12 => ("saw", "saw", 1800.0, 0.35, 1.0, 0.85, 0.9),
        7 => ("saw", "saw", 2400.0, 0.04, 0.3, 0.8, 0.15),
        8 | 9 => ("triangle", "sine", 3000.0, 0.03, 0.3, 0.85, 0.15),
        _ => ("saw", "square", 3200.0, 0.005, 0.3, 0.8, 0.2),
    };
    set_option(&mut d, "wave1", w1);
    set_option(&mut d, "wave2", w2);
    set_param(&mut d, "cutoff", cutoff);
    set_param(&mut d, "attack", attack);
    set_param(&mut d, "decay", decay);
    set_param(&mut d, "sustain", sustain);
    set_param(&mut d, "release", release);
    set_param(&mut d, "filterEnv", 0.2);
    set_param(&mut d, "gain", 0.5);
    d
}

/// A General MIDI drum group: display name, Atelier kind, pitch, decay
/// override and whether the mapping is an approximation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrumGroup {
    pub name: &'static str,
    pub kind: &'static str,
    pub pitch: i32,
    pub decay: Option<f64>,
    pub approx: bool,
}

/// The drum group of a GM percussion key (MIDI channel 10).
pub fn drum_group(key: u8) -> Option<DrumGroup> {
    let g = |name, kind, pitch, decay, approx| {
        Some(DrumGroup {
            name,
            kind,
            pitch,
            decay,
            approx,
        })
    };
    match key {
        35 | 36 => g("Kick", "kick", 60, None, false),
        37 => g("Rim", "rim", 60, None, false),
        38 | 40 => g("Snare", "snare", 60, None, false),
        39 => g("Clap", "clap", 60, None, false),
        42 | 44 => g("Hi-Hat", "hat", 60, None, false),
        46 => g("Open Hat", "openhat", 60, None, false),
        // Toms keep their relative tuning around 60.
        41 => g("Toms", "tom", 55, None, false),
        43 => g("Toms", "tom", 57, None, false),
        45 => g("Toms", "tom", 60, None, false),
        47 => g("Toms", "tom", 62, None, false),
        48 => g("Toms", "tom", 65, None, false),
        50 => g("Toms", "tom", 67, None, false),
        49 | 52 | 55 | 57 => g("Crash", "openhat", 60, Some(3.0), true),
        51 | 53 | 59 => g("Ride", "openhat", 60, Some(1.8), true),
        56 => g("Cowbell", "cowbell", 60, None, false),
        69 | 70 | 82 => g("Shaker", "shaker", 60, None, false),
        54 => g("Tambourine", "shaker", 60, Some(1.4), true),
        60..=68 => g("Hand Drums", "tom", 60 + key as i32 - 64, Some(0.7), true),
        75..=77 => g("Woodblock", "rim", 60 + (key as i32 - 76) * 3, None, true),
        80 | 81 => g("Triangle", "hat", 67, Some(2.5), true),
        _ => None,
    }
}

// ---------------------------------------------------------------- import

/// Where notes of one Rosaclef channel come from.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Source {
    Melodic {
        track: usize,
        channel: u8,
        program: u8,
    },
    Drum {
        track: usize,
        group: &'static str,
    },
}

impl Source {
    /// The MIDI track and channel the notes were played on.
    fn midi(&self) -> (usize, u8) {
        match self {
            Source::Melodic { track, channel, .. } => (*track, *channel),
            Source::Drum { track, .. } => (*track, 9),
        }
    }
}

struct RawNote {
    pitch: i32,
    start: u64,
    end: u64,
    velocity: u8,
}

/// Rosaclef channel gain for MIDI volume (CC 7) and expression (CC 11):
/// volume 100 at full expression is the default 0.8.
fn midi_gain(volume: u8, expression: u8) -> f64 {
    let g = 0.8 * volume as f64 / 100.0 * expression as f64 / 127.0;
    (clamp(g, 0.0, 1.5) * 1000.0).round() / 1000.0
}

/// Rosaclef pan for MIDI pan (CC 10).
fn midi_pan(pan: u8) -> f64 {
    (clamp((pan as f64 - 64.0) / 63.0, -1.0, 1.0) * 1000.0).round() / 1000.0
}

/// Mixer controllers of one MIDI track and channel: the (gain, pan) in
/// effect after each tick that has CC 7, 10 or 11 events.
#[derive(Default)]
struct Controls {
    events: Vec<(u64, u8, u8)>,
}

impl Controls {
    fn states(&self) -> Vec<(u64, f64, f64)> {
        let (mut vol, mut expr, mut pan) = (100u8, 127u8, 64u8);
        let mut out: Vec<(u64, f64, f64)> = vec![];
        for &(tick, cc, v) in &self.events {
            match cc {
                7 => vol = v,
                11 => expr = v,
                _ => pan = v,
            }
            let s = (tick, midi_gain(vol, expr), midi_pan(pan));
            match out.last_mut() {
                Some(last) if last.0 == tick => *last = s,
                _ => out.push(s),
            }
        }
        out
    }
}

/// Automation points starting at `initial` and following `changes`
/// (beat, value). A change becomes a vertical step, except that changes
/// less than `ramp` beats after the previous one continue it as a line (a
/// fade written as a stream of controller events).
fn step_points(initial: f64, changes: &[(f64, f64)], ramp: f64) -> Vec<AutomationPoint> {
    let point = |beat, value| AutomationPoint {
        beat,
        value,
        curve: 0.0,
    };
    let mut pts = vec![point(0.0, initial)];
    let mut prev = initial;
    let mut last_change = f64::NEG_INFINITY;
    for &(beat, value) in changes {
        if value == prev {
            continue;
        }
        if beat <= 1e-9 && pts.len() == 1 {
            pts[0].value = value;
        } else if beat - last_change > ramp {
            pts.push(point(beat, prev));
            pts.push(point(beat, value));
        } else {
            pts.push(point(beat, value));
        }
        prev = value;
        last_change = beat;
    }
    pts
}

/// Starts of the blocks the song is cut into: `bars` bars each, restarting
/// at every meter change, up to and including the first start after `end`.
fn block_starts(t: &Transport, bars: u32, end: f64) -> Vec<f64> {
    let map = t.meter_map();
    let mut out = vec![];
    for (i, span) in map.iter().enumerate() {
        let stop = map.get(i + 1).map(|n| n.beat).unwrap_or(f64::INFINITY);
        let step = span.bar_beats * bars as f64;
        let mut b = span.beat;
        while b < stop - 1e-9 {
            out.push((b * 1_000_000.0).round() / 1_000_000.0);
            if b > end {
                return out;
            }
            b += step;
        }
    }
    out
}

/// A time signature that Rosaclef can represent: (numerator, denominator).
fn meter_of(numerator: u8, denominator_pow: u8) -> Option<(u32, u32)> {
    let n = numerator.max(1) as u32;
    let d = 1u32 << denominator_pow.min(5);
    (n <= 64 && 4 * n <= 32 * d).then_some((n, d))
}

/// Import a Standard MIDI File.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Imported> {
    let smf = parse(bytes)?;
    let mut warn = Warnings::default();
    for w in &smf.warnings {
        warn.add(w.clone());
    }
    if smf.format == 2 {
        warn.add(
            "format 2 (independent sequences) was read like format 1: all tracks play together",
        );
    }

    // Tempo and time signature maps (the last event on a tick wins).
    let mut tempos: BTreeMap<u64, u32> = BTreeMap::new();
    let mut sigs: BTreeMap<u64, (u8, u8)> = BTreeMap::new();
    for t in &smf.tracks {
        for (tick, e) in t {
            match e {
                Event::Tempo(us) if *us > 0 => {
                    tempos.insert(*tick, *us);
                }
                Event::TimeSignature {
                    numerator,
                    denominator_pow,
                } => {
                    sigs.insert(*tick, (*numerator, *denominator_pow));
                }
                _ => {}
            }
        }
    }
    let to_bpm = |us: u32| (clamp(60_000_000.0 / us as f64, 20.0, 999.0) * 100.0).round() / 100.0;
    let bpm = tempos
        .values()
        .next()
        .map(|us| to_bpm(*us))
        .unwrap_or(120.0);
    let ticks_per_beat = match smf.division {
        Division::Ppq(p) => p as f64,
        Division::Smpte {
            fps,
            ticks_per_frame,
        } => {
            warn.add("the file uses SMPTE time; it was converted to beats at the file tempo");
            let fps = if fps == 29 { 29.97 } else { fps.max(1) as f64 };
            fps * ticks_per_frame.max(1) as f64 * 60.0 / bpm
        }
    };
    let to_beats = |tick: u64| beats(tick as f64, ticks_per_beat);

    // Meters: MIDI places time signatures on ticks, Rosaclef on bars.
    let mut first_meter = (4u32, 4u32);
    let mut meters: Vec<Meter> = vec![];
    let (mut span_bar, mut span_beat, mut span_len) = (0u32, 0.0f64, 4.0f64);
    let mut current = first_meter;
    let mut misaligned = 0usize;
    for (&tick, &(num, pow)) in &sigs {
        let Some(m) = meter_of(num, pow) else {
            warn.add(format!(
                "time signature {num}/{} is longer than 32 beats per bar and was ignored",
                1u32 << pow.min(7)
            ));
            continue;
        };
        let len = 4.0 * m.0 as f64 / m.1 as f64;
        let beat = tick as f64 / ticks_per_beat;
        if tick == 0 {
            (first_meter, current, span_len) = (m, m, len);
            continue;
        }
        if beat <= span_beat + 1e-9 {
            // Moved onto the bar line of the previous change: it replaces it.
            if let Some(last) = meters.last_mut() {
                (last.numerator, last.denominator) = m;
                (current, span_len) = (m, len);
            }
            continue;
        }
        if m == current {
            continue;
        }
        let bars = (beat - span_beat) / span_len;
        let k = if (bars - bars.round()).abs() < 1e-6 {
            bars.round()
        } else {
            misaligned += 1;
            bars.ceil()
        };
        span_bar += k as u32;
        span_beat += k * span_len;
        (current, span_len) = (m, len);
        meters.push(Meter {
            bar: span_bar + 1,
            numerator: m.0,
            denominator: m.1,
        });
    }
    if misaligned > 0 {
        warn.add(format!(
            "{misaligned} time signature change(s) fell inside a bar; they start at the next bar line"
        ));
    }
    let first_len = 4.0 * first_meter.0 as f64 / first_meter.1 as f64;
    if first_meter.1 != 4 || first_len.fract() != 0.0 {
        meters.insert(
            0,
            Meter {
                bar: 1,
                numerator: first_meter.0,
                denominator: first_meter.1,
            },
        );
    }
    if meters.len() > 4096 {
        warn.add("the file has more than 4096 time signature changes; the rest were ignored");
        meters.truncate(4096);
    }

    // Programs per track and channel, and per channel over all tracks (for
    // files that set programs on a different track than the notes).
    let mut programs: HashMap<(usize, u8), Vec<(u64, u8)>> = HashMap::new();
    let mut channel_programs: HashMap<u8, Vec<(u64, u8)>> = HashMap::new();
    for (ti, track) in smf.tracks.iter().enumerate() {
        for (tick, e) in track {
            if let Event::Program { channel, program } = e {
                programs
                    .entry((ti, *channel))
                    .or_default()
                    .push((*tick, *program));
                channel_programs
                    .entry(*channel)
                    .or_default()
                    .push((*tick, *program));
            }
        }
    }
    for v in channel_programs.values_mut() {
        v.sort_by_key(|p| p.0);
    }
    let program_at = |track: usize, channel: u8, tick: u64| -> u8 {
        programs
            .get(&(track, channel))
            .or_else(|| channel_programs.get(&channel))
            .and_then(|v| v.iter().rfind(|p| p.0 <= tick).or(v.first()))
            .map(|p| p.1)
            .unwrap_or(0)
    };

    // Pair notes (with the sustain pedal), collect names and controllers.
    let mut notes: BTreeMap<Source, Vec<RawNote>> = BTreeMap::new();
    let mut order: Vec<Source> = vec![];
    let mut track_names: Vec<String> = vec![];
    let mut controls: HashMap<(usize, u8), Controls> = HashMap::new();
    let mut bent: std::collections::BTreeSet<(usize, u8)> = Default::default();
    let mut unmapped_drums = 0usize;
    let mut unterminated = 0usize;
    let mut approx_drums: Vec<(u8, &'static str, &'static str)> = vec![];
    for (ti, track) in smf.tracks.iter().enumerate() {
        let mut name = String::new();
        let mut open: HashMap<(u8, u8), VecDeque<(u64, u8)>> = HashMap::new();
        let mut pedal = [false; 16];
        // Notes released while the pedal is down: (channel, key, start, velocity).
        let mut held: Vec<(u8, u8, u64, u8)> = vec![];
        // Finished notes: (channel, key, start, end, velocity).
        let mut done: Vec<(u8, u8, u64, u64, u8)> = vec![];
        let last_tick = track.last().map(|e| e.0).unwrap_or(0);
        let release = |held: &mut Vec<(u8, u8, u64, u8)>,
                       done: &mut Vec<(u8, u8, u64, u64, u8)>,
                       tick: u64,
                       which: &dyn Fn(u8, u8) -> bool| {
            held.retain(|&(ch, key, start, vel)| {
                if which(ch, key) {
                    done.push((ch, key, start, tick.max(start + 1), vel));
                    false
                } else {
                    true
                }
            });
        };
        for (tick, e) in track {
            let tick = *tick;
            match e {
                Event::TrackName(n) if name.is_empty() => name = n.clone(),
                Event::NoteOn {
                    channel,
                    key,
                    velocity,
                } => {
                    // Striking a key again ends its pedal-sustained note.
                    release(&mut held, &mut done, tick, &|c, k| {
                        c == *channel && k == *key
                    });
                    open.entry((*channel, *key))
                        .or_default()
                        .push_back((tick, *velocity));
                }
                Event::NoteOff { channel, key } => {
                    if let Some((start, vel)) =
                        open.get_mut(&(*channel, *key)).and_then(|q| q.pop_front())
                    {
                        if pedal[*channel as usize] && *channel != 9 {
                            held.push((*channel, *key, start, vel));
                        } else {
                            done.push((*channel, *key, start, tick, vel));
                        }
                    }
                }
                // Sustain pedal, and "reset all controllers" (which lifts it).
                Event::Controller {
                    channel,
                    controller: c @ (64 | 121),
                    value,
                } => {
                    let down = *c == 64 && *value >= 64;
                    if pedal[*channel as usize] && !down {
                        release(&mut held, &mut done, tick, &|ch, _| ch == *channel);
                    }
                    pedal[*channel as usize] = down;
                }
                Event::Controller {
                    channel,
                    controller: c @ (7 | 10 | 11),
                    value,
                } => controls
                    .entry((ti, *channel))
                    .or_default()
                    .events
                    .push((tick, *c, *value)),
                Event::PitchBend { channel, value } if *value != 0 && *channel != 9 => {
                    bent.insert((ti, *channel));
                }
                _ => {}
            }
        }
        // Pedal still down at the end: the notes ring to the end of the track.
        release(&mut held, &mut done, last_tick, &|_, _| true);
        let mut rest: Vec<((u8, u8), (u64, u8))> = open
            .into_iter()
            .flat_map(|(k, q)| q.into_iter().map(move |n| (k, n)))
            .collect();
        rest.sort_by_key(|r| (r.1 .0, r.0));
        for ((ch, key), (start, vel)) in rest {
            unterminated += 1;
            let end = if last_tick > start {
                last_tick
            } else {
                start + ticks_per_beat as u64
            };
            done.push((ch, key, start, end, vel));
        }
        done.sort_by_key(|d| d.2);
        for (ch, key, start, end, velocity) in done {
            let (src, pitch) = if ch == 9 {
                let Some(g) = drum_group(key) else {
                    unmapped_drums += 1;
                    continue;
                };
                if g.approx && !approx_drums.iter().any(|a| a.0 == key) {
                    approx_drums.push((key, g.name, g.kind));
                }
                (
                    Source::Drum {
                        track: ti,
                        group: g.name,
                    },
                    g.pitch,
                )
            } else {
                (
                    Source::Melodic {
                        track: ti,
                        channel: ch,
                        program: program_at(ti, ch, start),
                    },
                    key as i32,
                )
            };
            if !notes.contains_key(&src) {
                order.push(src.clone());
            }
            notes.entry(src).or_default().push(RawNote {
                pitch,
                start,
                end,
                velocity,
            });
        }
        track_names.push(name);
    }
    if unterminated > 0 {
        warn.add(format!(
            "{unterminated} note(s) had no note-off and end at the end of their track"
        ));
    }
    if unmapped_drums > 0 {
        warn.add(format!(
            "{unmapped_drums} drum note(s) on keys outside the General MIDI drum map were skipped"
        ));
    }
    for (key, name, kind) in &approx_drums {
        warn.add(format!(
            "GM drum key {key} ({name}) was approximated by the Atelier \"{kind}\" drum"
        ));
    }
    let bent_with_notes = bent
        .iter()
        .filter(|(t, c)| {
            order
                .iter()
                .any(|s| matches!(s, Source::Melodic { track, channel, .. } if track == t && channel == c))
        })
        .count();
    if bent_with_notes > 0 {
        warn.add(format!(
            "pitch bend on {bent_with_notes} channel(s) was ignored (notes keep their key's pitch)"
        ));
    }
    if notes.is_empty() {
        warn.add("the file contains no notes");
    }

    // Build channels, patterns, tracks, inserts and automation.
    let mut project = Project::empty(&opts.title);
    project.schema = "./project.schema.json".into();
    project.meta.description = "Imported from a MIDI file".into();
    // A format 1 conductor track (no notes) usually carries the song name.
    let conductor_has_notes = order.iter().any(|s| s.midi().0 == 0);
    if smf.format == 1 && !conductor_has_notes && !track_names[0].is_empty() {
        project.meta.title = track_names[0].clone();
    }
    project.transport.bpm = bpm;
    project.transport.beats_per_bar = first_len.round().clamp(1.0, 32.0) as u32;
    project.transport.meters = meters;
    project.patterns.clear();
    project.playlist.tracks.clear();
    project.mixer.inserts.truncate(1);

    // Tempo changes become a stepped tempo lane.
    let tempo_changes: Vec<(f64, f64)> = tempos
        .iter()
        .skip(1)
        .map(|(tick, us)| (to_beats(*tick), to_bpm(*us)))
        .collect();
    let tempo_points = step_points(bpm, &tempo_changes, 0.0);
    if tempo_points.len() > 1 {
        project.automation.push(AutomationLane {
            id: "tempo".into(),
            name: "Tempo".into(),
            target: "tempo".into(),
            color: "#d4af37".into(),
            mute: false,
            points: tempo_points,
        });
    }

    let song_end = notes
        .values()
        .flatten()
        .map(|n| to_beats(n.start))
        .fold(0.0, f64::max);
    let starts = block_starts(&project.transport, opts.bars_per_pattern.max(1), song_end);
    let mut channel_ids = Ids::default();
    let mut pattern_ids = Ids::default();
    let mut lane_ids = Ids::with(project.automation.iter().map(|l| l.id.clone()));
    let drums_tracks: std::collections::BTreeSet<usize> = order
        .iter()
        .filter_map(|s| match s {
            Source::Drum { track, .. } => Some(*track),
            _ => None,
        })
        .collect();
    let mut used_names: HashMap<String, usize> = HashMap::new();
    let no_controls = Controls::default();

    for (ci, src) in order.iter().enumerate() {
        let raw = &notes[src];
        let (base_name, instrument) = match src {
            Source::Melodic { track, program, .. } => {
                let (device, _) = gm_instrument(*program);
                let sources_in_track = order
                    .iter()
                    .filter(|s| matches!(s, Source::Melodic { track: t, .. } if t == track))
                    .count();
                let tname = &track_names[*track];
                let family = GM_FAMILIES[(program / 8) as usize];
                let name = if !tname.is_empty() && sources_in_track == 1 {
                    tname.clone()
                } else if !tname.is_empty() {
                    format!("{tname} · {family}")
                } else {
                    family.to_string()
                };
                (name, device)
            }
            Source::Drum { track, group } => {
                let key = raw_group_key(group);
                let g = drum_group(key).expect("group keys are mapped");
                let mut d = Device::new("drum");
                set_option(&mut d, "kind", g.kind);
                if let Some(decay) = g.decay {
                    set_param(&mut d, "decay", decay);
                }
                let tname = &track_names[*track];
                let name = if drums_tracks.len() > 1 && !tname.is_empty() {
                    format!("{tname} {group}")
                } else {
                    group.to_string()
                };
                (name, d)
            }
        };
        let n = used_names.entry(base_name.clone()).or_insert(0);
        *n += 1;
        let name = if *n > 1 {
            format!("{base_name} {n}")
        } else {
            base_name
        };
        let id = channel_ids.make(&name);
        let ccolor = color(ci);

        // Volume and pan: the values when the channel starts playing, and
        // lanes for later changes (up to its last note).
        let first = raw.iter().map(|n| n.start).min().unwrap_or(0);
        let last = raw.iter().map(|n| n.end).max().unwrap_or(0);
        let states = controls.get(&src.midi()).unwrap_or(&no_controls).states();
        let (volume, pan) = states
            .iter()
            .rfind(|s| s.0 <= first)
            .map(|s| (s.1, s.2))
            .unwrap_or((0.8, 0.0));
        let later: Vec<&(u64, f64, f64)> = states
            .iter()
            .filter(|s| s.0 > first && s.0 < last)
            .collect();
        let lanes: [(&str, f64, fn(&(u64, f64, f64)) -> f64); 2] =
            [("volume", volume, |s| s.1), ("pan", pan, |s| s.2)];
        for (what, initial, value) in lanes {
            let changes: Vec<(f64, f64)> =
                later.iter().map(|s| (to_beats(s.0), value(s))).collect();
            let points = step_points(initial, &changes, 0.25);
            if points.len() > 1 {
                project.automation.push(AutomationLane {
                    id: lane_ids.make(&format!("{id}-{what}")),
                    name: format!("{name} {what}"),
                    target: format!("channel/{id}/{what}"),
                    color: ccolor.clone(),
                    mute: false,
                    points,
                });
            }
        }

        let mixer = if project.mixer.inserts.len() < MAX_INSERTS {
            project.mixer.inserts.push(Insert::new(&name));
            InsertIx(project.mixer.inserts.len() as u32 - 1)
        } else {
            warn.add("more channels than mixer inserts; the rest are routed to the master");
            InsertIx::MASTER
        };
        project.channels.push(Channel {
            id: id.clone(),
            name: name.clone(),
            color: ccolor.clone(),
            instrument,
            volume,
            pan,
            mute: false,
            mixer,
        });
        let tix = TrackIx(project.playlist.tracks.len() as u32);
        project.playlist.tracks.push(Track {
            name: name.clone(),
            mute: false,
        });

        // Cut into blocks of whole bars and dedupe identical blocks.
        let mut blocks: BTreeMap<usize, Vec<Note>> = BTreeMap::new();
        for rn in raw {
            let start = to_beats(rn.start);
            let length = to_beats(rn.end.saturating_sub(rn.start)).max(1.0 / 64.0);
            let k = starts.partition_point(|b| *b <= start + 1e-9).max(1) - 1;
            let rel = ((start - starts[k]) * 1_000_000.0).round() / 1_000_000.0;
            blocks.entry(k).or_default().push(Note {
                channel: id.clone(),
                pitch: rn.pitch.clamp(0, 127),
                start: rel.max(0.0),
                length,
                velocity: clamp(rn.velocity as f64 / 127.0, 0.0, 1.0),
            });
        }
        let block_len = |k: usize| {
            let end = starts.get(k + 1).copied().unwrap_or(starts[k] + 4.0);
            ((end - starts[k]) * 1_000_000.0).round() / 1_000_000.0
        };
        let mut seen: HashMap<String, String> = HashMap::new();
        let mut letters = 0usize;
        let mut run: Option<(String, usize, usize)> = None; // (pattern, first block, blocks)
        let unique = {
            let mut keys: Vec<String> = blocks
                .iter()
                .map(|(k, v)| format!("{}|{}", block_len(*k), block_key(v)))
                .collect();
            keys.sort();
            keys.dedup();
            keys.len()
        };
        let mut clips = vec![];
        for (k, mut ns) in blocks {
            ns.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
            let length = block_len(k);
            let key = format!("{length}|{}", block_key(&ns));
            let pid = match seen.get(&key) {
                Some(p) => p.clone(),
                None => {
                    let pname = if unique == 1 {
                        name.clone()
                    } else {
                        format!("{name} {}", letter(letters))
                    };
                    letters += 1;
                    let pid = pattern_ids.make(&pname);
                    project.patterns.push(Pattern {
                        id: pid.clone(),
                        name: pname,
                        color: ccolor.clone(),
                        length,
                        notes: ns,
                    });
                    seen.insert(key, pid.clone());
                    pid
                }
            };
            run = match run {
                Some((p, first, count)) if p == pid && first + count == k => {
                    Some((p, first, count + 1))
                }
                Some(prev) => {
                    clips.push(prev);
                    Some((pid, k, 1))
                }
                None => Some((pid, k, 1)),
            };
        }
        clips.extend(run);
        for (pid, first, count) in clips {
            let start = starts[first];
            let end = starts[first + count - 1] + block_len(first + count - 1);
            project.playlist.clips.push(Clip {
                pattern: pid,
                sample: String::new(),
                track: tix,
                start,
                length: ((end - start) * 1_000_000.0).round() / 1_000_000.0,
                offset: 0.0,
                gain: 1.0,
                mixer: InsertIx::MASTER,
            });
        }
    }
    pad_tracks(&mut project, 8);
    ensure_valid(&project)?;
    Ok(Imported {
        project,
        samples: vec![],
        warnings: warn.finish(),
    })
}

/// A representative key for a drum group name.
fn raw_group_key(group: &str) -> u8 {
    (0..=127u8)
        .find(|k| drum_group(*k).map(|g| g.name == group).unwrap_or(false))
        .unwrap_or(36)
}

fn block_key(notes: &[Note]) -> String {
    let mut v: Vec<String> = notes
        .iter()
        .map(|n| format!("{}:{}:{}:{:.3}", n.pitch, n.start, n.length, n.velocity))
        .collect();
    v.sort();
    v.join(",")
}

/// A, B, ..., Z, AA, AB, ...
fn letter(i: usize) -> String {
    let mut i = i;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters() {
        assert_eq!(letter(0), "A");
        assert_eq!(letter(25), "Z");
        assert_eq!(letter(26), "AA");
    }

    #[test]
    fn vlq() {
        let mut r = Reader {
            data: &[0x81, 0x80, 0x00, 0x7f],
            pos: 0,
        };
        assert_eq!(r.vlq().unwrap(), 0x4000);
        assert_eq!(r.vlq().unwrap(), 0x7f);
    }

    #[test]
    fn every_gm_program_gets_a_valid_instrument() {
        for program in 0..128u8 {
            let (d, _) = gm_instrument(program);
            let mut p = Project::empty("t");
            p.channels.push(Channel {
                id: "c".into(),
                name: "c".into(),
                color: "#ffffff".into(),
                instrument: d,
                volume: 0.8,
                pan: 0.0,
                mute: false,
                mixer: InsertIx::MASTER,
            });
            ensure_valid(&p).unwrap();
        }
    }
}
