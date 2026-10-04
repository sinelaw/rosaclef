//! What the checks share: each channel's role and register, its notes in
//! each pattern ("parts"), the song's notes as they play, the bars, the key,
//! and helpers to word findings and build their fixes.

use super::{rule, Finding, Fix, Op, Where};
use crate::model::{Channel, Device, Pattern, Project};
use serde_json::Value;

/// What the Critic knows of a channel.
#[derive(Clone, Debug)]
pub struct ChInfo {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub kind: String,
    /// drums, bass, harmony, lead, part (other pitched), fx (transitions),
    /// gen (generative), arp (arpeggiated), sample (samplers, granular,
    /// plugins) or idle (no notes).
    pub role: &'static str,
    /// The drum machine's sound ("kick", "snare", ...), else "".
    pub drum: String,
    pub pitched: bool,
    /// Semitones its notes sound away from where they are written.
    pub shift: i32,
    pub count: usize,
    /// The lowest note it plays.
    pub low: i32,
    pub insert: usize,
    pub layer_of: String,
    /// The General MIDI program of a sampled instrument, else "".
    pub program: String,
    pub pan: f64,
    pub volume: f64,
    pub mute: bool,
}

/// A note as it sounds in the song (beats): `pitch` is sounding; `pattern`
/// is an index into `Project::patterns`.
#[derive(Clone, Debug)]
pub struct SNote {
    pub pitch: i32,
    pub start: f64,
    pub end: f64,
    pub channel: usize,
    pub pattern: usize,
}

/// One channel's notes in one pattern (`idx`: note indexes, by start).
#[derive(Clone, Debug)]
pub struct Part {
    pub pat: usize,
    pub ch: usize,
    pub idx: Vec<usize>,
}

/// Notes struck together in a part: indexes and sounding pitches, both
/// from low to high.
#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub start: f64,
    pub end: f64,
    pub idx: Vec<usize>,
    pub pitches: Vec<i32>,
}

/// A bar of the song: its downbeat and length (beats).
#[derive(Clone, Copy, Debug)]
pub struct Bar {
    pub start: f64,
    pub length: f64,
}

pub struct Ana<'a> {
    pub p: &'a Project,
    pub chans: Vec<ChInfo>,
    pub parts: Vec<Part>,
    pub song: Vec<SNote>,
    pub bars: Vec<Bar>,
    pub tonic: i32,
    pub minor: bool,
    pub fit: f64,
    pub key_label: String,
    pub key_from_score: bool,
    pub out: Vec<Finding>,
}

const PITCHED: &[&str] = &["analog", "fm", "wavetable", "additive", "soundfont"];

impl<'a> Ana<'a> {
    pub fn new(p: &'a Project) -> Ana<'a> {
        let chans: Vec<ChInfo> = p
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| channel_info(p, i, c))
            .collect();
        let mut parts = vec![];
        for (pi, pat) in p.patterns.iter().enumerate() {
            for ch in &chans {
                let mut idx: Vec<usize> = (0..pat.notes.len())
                    .filter(|&i| pat.notes[i].channel == ch.id)
                    .collect();
                if idx.is_empty() {
                    continue;
                }
                idx.sort_by(|&x, &y| {
                    let (a, b) = (&pat.notes[x], &pat.notes[y]);
                    a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch))
                });
                parts.push(Part {
                    pat: pi,
                    ch: ch.index,
                    idx,
                });
            }
        }
        let song = song_notes(p, &chans);
        let length = p.song_length();
        let mut bars = vec![];
        if length > 0.0 {
            let mut bar = 0;
            loop {
                let start = p.transport.bar_start(bar);
                if start > length - 1e-6 || bars.len() >= 4096 {
                    break;
                }
                let next = p.transport.bar_start(bar + 1);
                bars.push(Bar {
                    start,
                    length: next - start,
                });
                bar += 1;
            }
        }
        Ana {
            p,
            chans,
            parts,
            song,
            bars,
            tonic: 0,
            minor: false,
            fit: 0.0,
            key_label: String::new(),
            key_from_score: false,
            out: vec![],
        }
    }

    pub fn pattern(&self, part: &Part) -> &'a Pattern {
        &self.p.patterns[part.pat]
    }

    pub fn ch(&self, part: &Part) -> &ChInfo {
        &self.chans[part.ch]
    }

    pub fn chan(&self, id: &str) -> Option<&ChInfo> {
        self.chans.iter().find(|c| c.id == id)
    }

    /// A note's sounding pitch.
    pub fn sounding(&self, part: &Part, i: usize) -> i32 {
        self.pattern(part).notes[i].pitch + self.ch(part).shift
    }

    /// The bar (from 0) that holds a song beat.
    pub fn bar_of(&self, beat: f64) -> usize {
        self.bars.partition_point(|b| b.start <= beat + 1e-9).max(1) - 1
    }

    /// The beats per bar of patterns (they ignore meter changes).
    pub fn bar_beats(&self) -> f64 {
        self.p.transport.beats_per_bar.max(1) as f64
    }

    /// The group a channel counts in for counting parts: the source of its
    /// layer, or itself.
    pub fn group_of(&self, ch: usize) -> usize {
        let c = &self.chans[ch];
        if !c.layer_of.is_empty() {
            if let Some(src) = self.chan(&c.layer_of) {
                return src.index;
            }
        }
        ch
    }

    /// Group a part's notes into chords (notes struck within a 48th of a beat).
    pub fn chords(&self, part: &Part) -> Vec<Chord> {
        let notes = &self.pattern(part).notes;
        let mut out: Vec<Chord> = vec![];
        for &i in &part.idx {
            let n = &notes[i];
            match out.last_mut() {
                Some(last) if (n.start - last.start).abs() < 0.021 => {
                    last.idx.push(i);
                    last.end = last.end.max(n.start + n.length);
                }
                _ => out.push(Chord {
                    start: n.start,
                    end: n.start + n.length,
                    idx: vec![i],
                    pitches: vec![],
                }),
            }
        }
        let shift = self.ch(part).shift;
        for c in &mut out {
            c.idx.sort_by_key(|&i| notes[i].pitch);
            c.pitches = c.idx.iter().map(|&i| notes[i].pitch + shift).collect();
        }
        out
    }

    /// The notes to compare across parts: the song's, or each pattern's when
    /// nothing is on the playlist.
    pub fn scopes(&self) -> Vec<Vec<SNote>> {
        if !self.song.is_empty() {
            return vec![self.song.clone()];
        }
        let mut out = vec![];
        for (pi, pat) in self.p.patterns.iter().enumerate() {
            let mut list = vec![];
            for n in &pat.notes {
                let Some(ch) = self.chan(&n.channel) else {
                    continue;
                };
                if n.start >= pat.length {
                    continue;
                }
                list.push(SNote {
                    pitch: n.pitch + ch.shift,
                    start: n.start,
                    end: pat.length.min(n.start + n.length),
                    channel: ch.index,
                    pattern: pi,
                });
            }
            list.sort_by(|x, y| x.start.total_cmp(&y.start));
            if !list.is_empty() {
                out.push(list);
            }
        }
        out
    }

    // -------------------------------------------------------------- findings

    /// Record a finding.
    pub fn add(
        &mut self,
        rule_id: &'static str,
        level: &'static str,
        title: String,
        detail: String,
        at: Where,
        fix: Option<Fix>,
    ) {
        let base = format!(
            "{rule_id}|{}:{}:{}:{}",
            at.kind, at.id, at.channel, at.index
        );
        let mut key = base.clone();
        let mut n = 2;
        while self.out.iter().any(|f| f.key == key) {
            key = format!("{base}#{n}");
            n += 1;
        }
        let category = rule(rule_id).map(|r| r.category).unwrap_or("Project");
        self.out.push(Finding {
            key,
            rule: rule_id,
            category,
            level,
            title,
            detail,
            at,
            fix,
            suppressed: false,
        });
    }

    /// An issue: a finding with nothing to apply.
    pub fn note(
        &mut self,
        rule_id: &'static str,
        level: &'static str,
        title: String,
        detail: String,
        at: Where,
    ) {
        self.add(rule_id, level, title, detail, at, None);
    }

    // -------------------------------------------------------------- places

    pub fn at_part(&self, part: &Part, notes: Vec<usize>) -> Where {
        let pat = self.pattern(part);
        let ch = self.ch(part);
        Where {
            kind: "pattern",
            id: pat.id.clone(),
            channel: ch.id.clone(),
            index: -1,
            beat: -1.0,
            notes,
            label: format!("{} · {}", pat.name, ch.name),
        }
    }

    pub fn at_pattern(&self, pi: usize) -> Where {
        let pat = &self.p.patterns[pi];
        Where {
            kind: "pattern",
            id: pat.id.clone(),
            channel: String::new(),
            index: -1,
            beat: -1.0,
            notes: vec![],
            label: pat.name.clone(),
        }
    }

    pub fn at_channel(&self, ch: usize) -> Where {
        let c = &self.chans[ch];
        Where {
            kind: "channel",
            id: c.id.clone(),
            channel: c.id.clone(),
            index: -1,
            beat: -1.0,
            notes: vec![],
            label: c.name.clone(),
        }
    }

    pub fn at_insert(&self, i: usize) -> Where {
        let name = self
            .p
            .mixer
            .inserts
            .get(i)
            .map(|x| x.name.as_str())
            .unwrap_or("");
        Where {
            kind: "insert",
            id: String::new(),
            channel: String::new(),
            index: i as i64,
            beat: -1.0,
            notes: vec![],
            label: if i == 0 {
                "Master".into()
            } else {
                format!("Insert {i} · {name}")
            },
        }
    }

    pub fn at_song(&self, beat: f64, clip: i64) -> Where {
        Where {
            kind: "song",
            id: String::new(),
            channel: String::new(),
            index: clip,
            beat,
            notes: vec![],
            label: format!("Bar {}", self.bar_of(beat) + 1),
        }
    }

    pub fn at_lane(&self, i: usize) -> Where {
        let l = &self.p.automation[i];
        Where {
            kind: "lane",
            id: l.id.clone(),
            channel: String::new(),
            index: i as i64,
            beat: l.points.first().map(|p| p.beat).unwrap_or(0.0),
            notes: vec![],
            label: if l.name.is_empty() {
                l.target.clone()
            } else {
                l.name.clone()
            },
        }
    }

    /// Where song-wide notes point: a bar, or the pattern when nothing is placed.
    pub fn at_notes(&self, notes: &[SNote], beat: f64) -> Where {
        if !self.song.is_empty() {
            return self.at_song(beat, -1);
        }
        match notes.first() {
            Some(n) => self.at_pattern(n.pattern),
            None => at_project(""),
        }
    }
}

pub fn at_project(label: &str) -> Where {
    Where {
        kind: "project",
        id: String::new(),
        channel: String::new(),
        index: -1,
        beat: -1.0,
        notes: vec![],
        label: label.into(),
    }
}

// ------------------------------------------------------------------ channels

/// Does a channel play drums (the drum machine or a General MIDI kit)?
pub fn is_drums(c: &Channel) -> bool {
    crate::drums::is_drum_channel(c)
}

fn channel_info(p: &Project, index: usize, c: &Channel) -> ChInfo {
    let drum = is_drums(c);
    let kind = c.instrument.kind.as_str();
    let pitched = !drum && PITCHED.contains(&kind);
    let shift = p.transport.transpose
        + if kind == "soundfont" {
            c.instrument.param("transpose").round() as i32
        } else {
            0
        };
    let mut pitches = vec![];
    let mut onsets = 0;
    for pat in &p.patterns {
        let mut starts: Vec<f64> = vec![];
        for n in pat.notes.iter().filter(|n| n.channel == c.id) {
            pitches.push(n.pitch + shift);
            if !starts.iter().any(|s| (s - n.start).abs() < 0.021) {
                starts.push(n.start);
            }
        }
        onsets += starts.len();
    }
    pitches.sort();
    let count = pitches.len();
    let median = if count > 0 { pitches[count / 2] } else { 0 };
    let poly = if onsets > 0 {
        count as f64 / onsets as f64
    } else {
        0.0
    };
    let role = if drum {
        "drums"
    } else if kind == "transition" {
        "fx"
    } else if kind == "generative" {
        "gen"
    } else if c.arp.is_some() {
        "arp"
    } else if !pitched {
        "sample"
    } else if count == 0 {
        "idle"
    } else if poly >= 1.8 {
        "harmony"
    } else if median < 50 {
        "bass"
    } else if median >= 55 && poly < 1.3 {
        "lead"
    } else {
        "part"
    };
    ChInfo {
        index,
        id: c.id.clone(),
        name: c.name.clone(),
        kind: kind.into(),
        role,
        drum: if kind == "drum" {
            c.instrument.option("kind").to_string()
        } else {
            String::new()
        },
        pitched,
        shift,
        count,
        low: pitches.first().copied().unwrap_or(0),
        insert: c.mixer.index(),
        layer_of: c.layer_of.clone().unwrap_or_default(),
        program: if kind == "soundfont" {
            c.instrument.option("program").to_string()
        } else {
            String::new()
        },
        pan: c.pan,
        volume: c.volume,
        mute: c.mute,
    }
}

/// The notes of the song as they sound: clips expanded (muted tracks and
/// channels left out).
fn song_notes(p: &Project, chans: &[ChInfo]) -> Vec<SNote> {
    let mut out = vec![];
    for c in &p.playlist.clips {
        if c.pattern.is_empty() {
            continue;
        }
        if p.playlist
            .tracks
            .get(c.track.index())
            .map(|t| t.mute)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(pi) = p.patterns.iter().position(|x| x.id == c.pattern) else {
            continue;
        };
        let pat = &p.patterns[pi];
        let len = pat.length;
        if len <= 0.0 {
            continue;
        }
        let (w0, w1) = (c.offset, c.offset + c.length);
        let clip_end = c.start + c.length;
        let mut j = (w0 / len).floor() as i64;
        while (j as f64) * len < w1 {
            let base = j as f64 * len;
            for n in &pat.notes {
                let t = n.start + base;
                if t < w0 - 1e-9 || t >= w1 - 1e-9 || n.start >= len - 1e-9 {
                    continue;
                }
                let Some(ch) = chans.iter().find(|x| x.id == n.channel) else {
                    continue;
                };
                if ch.mute {
                    continue;
                }
                let at = c.start + t - w0;
                out.push(SNote {
                    pitch: n.pitch + ch.shift,
                    start: at,
                    end: (at + n.length).min(clip_end).min(c.start + base + len - w0),
                    channel: ch.index,
                    pattern: pi,
                });
            }
            j += 1;
        }
    }
    out.sort_by(|x, y| x.start.total_cmp(&y.start).then(x.pitch.cmp(&y.pitch)));
    out
}

// ------------------------------------------------------------------ fixes

/// Set a member (or add one): a JSON Patch `add`.
pub fn set(path: String, value: Value) -> Op {
    Op {
        op: "add",
        path,
        value: Some(value),
    }
}

pub fn remove(path: String) -> Op {
    Op {
        op: "remove",
        path,
        value: None,
    }
}

/// Remove array elements by index (from the last, so the others keep theirs).
pub fn remove_all(prefix: &str, mut idx: Vec<usize>) -> Vec<Op> {
    idx.sort_unstable_by(|a, b| b.cmp(a));
    idx.dedup();
    idx.into_iter()
        .map(|i| remove(format!("{prefix}/{i}")))
        .collect()
}

/// The path of a note's field.
pub fn note_path(pat: usize, i: usize, field: &str) -> String {
    format!("/patterns/{pat}/notes/{i}/{field}")
}

pub fn fix(label: impl Into<String>, ops: Vec<Op>) -> Option<Fix> {
    Some(Fix {
        label: label.into(),
        ops,
    })
}

/// A device as JSON (for whole effect chains).
pub fn device_json(d: &Device) -> Value {
    serde_json::to_value(d).expect("serializable")
}

// ------------------------------------------------------------------ words

const NOTE_NAMES: [&str; 12] = [
    "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
];
pub const KEY_NAMES: [&str; 12] = [
    "C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B",
];

pub fn note_name(p: i32) -> String {
    format!("{}{}", NOTE_NAMES[pc(p) as usize], p.div_euclid(12) - 1)
}

pub fn pc(p: i32) -> i32 {
    p.rem_euclid(12)
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Round to 3 decimals (the precision of beats and levels in findings).
pub fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A number as people write it: 8, 0.75, 8.841.
pub fn num(x: f64) -> String {
    format!("{}", r3(x))
}

pub fn db(gain: f64) -> String {
    if gain <= 0.00001 {
        return "−∞ dB".into();
    }
    let v = (20.0 * gain.log10() * 10.0).round() / 10.0;
    format!("{}{v} dB", if v > 0.0 { "+" } else { "" })
}

/// The frequency of a MIDI pitch (Hz).
pub fn hz(p: f64) -> f64 {
    440.0 * 2f64.powf((p - 69.0) / 12.0)
}

/// A small deterministic jitter in -1..1 (the same every time).
pub fn jitter(i: usize, salt: u32) -> f64 {
    let x = ((i as f64 + 1.0) * 12.9898 + salt as f64 * 78.233).sin() * 43758.5453;
    (x - x.floor()) * 2.0 - 1.0
}

pub fn percent(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// "a, b, c, …" from the first `n` of `names`.
pub fn list(names: &[String], n: usize) -> String {
    let mut s = names.iter().take(n).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > n {
        s.push_str(", …");
    }
    s
}
