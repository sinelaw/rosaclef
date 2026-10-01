//! The song as it is performed: every note in playing order (clips looped,
//! repeats unrolled, the verse each pass sings), timed in beats and seconds,
//! with its syllable and phonemes. Exporters read this, never the project.

use rosaclef_core::automation::TempoMap;
use rosaclef_core::expand::{self, Sounding};
use rosaclef_core::form;
use rosaclef_core::lyrics::Token;
use rosaclef_core::{LyricMode, Project, SyllableTiming};
use std::collections::HashMap;

/// A moment of the performance: beats from its start (repeats unrolled) and
/// seconds (following tempo automation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Time {
    pub beat: f64,
    pub sec: f64,
}

/// A note as it is performed.
#[derive(Clone, Debug, PartialEq)]
pub struct Performed {
    pub channel: String,
    /// The pitch that sounds (the master transpose applied).
    pub pitch: i32,
    pub start: Time,
    pub end: Time,
    pub velocity: f64,
    pub syllable: Option<Syllable>,
}

/// What a note sings.
#[derive(Clone, Debug, PartialEq)]
pub struct Syllable {
    pub token: Token,
    /// IPA; empty for holds and breaths.
    pub phonemes: Vec<String>,
    /// BCP 47 language tag.
    pub lang: String,
    pub mode: LyricMode,
    /// Phoneme timing fixed in the project, if any.
    pub timing: Option<SyllableTiming>,
    /// The name of the pattern whose lyric line it comes from: the song
    /// section ("Verse", "Chorus") for formats that label them.
    pub section: String,
    /// The verse of that line it sings.
    pub verse: u32,
}

/// One vocal channel's notes, in playing order.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub channel: String,
    pub name: String,
    pub notes: Vec<Performed>,
}

/// What exporters get: the song's facts and its performed notes.
#[derive(Clone, Debug)]
pub struct Song {
    pub title: String,
    pub author: String,
    /// The starting tempo (formats with one tempo use it).
    pub bpm: f64,
    pub beats_per_bar: u32,
    /// Every performed note of every channel, sorted by start.
    pub notes: Vec<Performed>,
    /// The channels that sing, each a line.
    pub lines: Vec<Line>,
}

impl Song {
    /// The whole song, as it plays.
    pub fn of_project(p: &Project) -> Song {
        Song::new(p, performance(p))
    }

    /// One pattern played once, singing `verse`.
    pub fn of_pattern(p: &Project, id: &str, verse: u32) -> Option<Song> {
        let index = p.patterns.iter().position(|x| x.id == id)?;
        let tempo = TempoMap::new(p);
        let mut perf = Performer::new(p, &tempo);
        let length = p.patterns[index].length;
        perf.pattern(index, verse, 0.0, (0.0, length), Frame::START);
        let mut notes = perf.out;
        notes.sort_by(|a, b| a.start.beat.total_cmp(&b.start.beat));
        Some(Song::new(p, notes))
    }

    /// A song of these notes (sorted by start), with the project's facts.
    pub fn from_notes(p: &Project, notes: Vec<Performed>) -> Song {
        Song::new(p, notes)
    }

    fn new(p: &Project, notes: Vec<Performed>) -> Song {
        Song {
            title: p.meta.title.clone(),
            author: p.meta.author.clone(),
            bpm: p.transport.bpm,
            beats_per_bar: p.transport.beats_per_bar,
            lines: lines(p, &notes),
            notes,
        }
    }
}

/// Every note of the song in playing order.
pub fn performance(p: &Project) -> Vec<Performed> {
    let tempo = TempoMap::new(p);
    let mut perf = Performer::new(p, &tempo);
    let mut frame = Frame::START;
    for span in form::performance(p) {
        frame.written = span.start;
        for (i, c) in p.playlist.clips.iter().enumerate() {
            let muted = p
                .playlist
                .tracks
                .get(c.track.index())
                .is_some_and(|t| t.mute);
            if muted || c.pattern.is_empty() || c.end() <= span.start || c.start >= span.end {
                continue;
            }
            perf.clip(i, span.pass, (span.start, span.end), frame);
        }
        frame.beat += span.end - span.start;
        frame.sec += tempo.seconds_at(span.end) - tempo.seconds_at(span.start);
    }
    let mut notes = perf.out;
    notes.sort_by(|a, b| a.start.beat.total_cmp(&b.start.beat));
    notes
}

/// The channels whose notes sing, one line each, in channel order.
fn lines(p: &Project, notes: &[Performed]) -> Vec<Line> {
    p.channels
        .iter()
        .filter(|c| {
            notes
                .iter()
                .any(|n| n.channel == c.id && n.syllable.is_some())
        })
        .map(|c| Line {
            channel: c.id.clone(),
            name: c.name.clone(),
            notes: notes
                .iter()
                .filter(|n| n.channel == c.id)
                .cloned()
                .collect(),
        })
        .collect()
}

/// Where a span of written time sits in the performance: written beat
/// `written` plays at performance beat `beat`, second `sec`.
#[derive(Clone, Copy)]
struct Frame {
    written: f64,
    beat: f64,
    sec: f64,
}

impl Frame {
    const START: Frame = Frame {
        written: 0.0,
        beat: 0.0,
        sec: 0.0,
    };

    fn time(&self, tempo: &TempoMap, t: f64) -> Time {
        Time {
            beat: self.beat + (t - self.written),
            sec: self.sec + tempo.seconds_at(t) - tempo.seconds_at(self.written),
        }
    }
}

trait ClipEnd {
    fn end(&self) -> f64;
}

impl ClipEnd for rosaclef_core::Clip {
    fn end(&self) -> f64 {
        self.start + self.length
    }
}

/// Places sounding notes in performance time. Expansions (with phonemes)
/// are made once per pattern and verse.
struct Performer<'a> {
    p: &'a Project,
    tempo: &'a TempoMap,
    expanded: HashMap<(usize, u32), Vec<Voiced>>,
    out: Vec<Performed>,
}

impl<'a> Performer<'a> {
    fn new(p: &'a Project, tempo: &'a TempoMap) -> Performer<'a> {
        Performer {
            p,
            tempo,
            expanded: HashMap::new(),
            out: vec![],
        }
    }

    /// The part of clip `i` inside the written span `[lo, hi)`.
    fn clip(&mut self, i: usize, pass: u32, (lo, hi): (f64, f64), frame: Frame) {
        let c = &self.p.playlist.clips[i];
        let Some(index) = self.p.patterns.iter().position(|x| x.id == c.pattern) else {
            return;
        };
        let length = self.p.patterns[index].length.max(1e-3);
        let verse = c.verse.unwrap_or(pass);
        let (from, to) = (lo.max(c.start), hi.min(c.end()));
        let base = c.start - c.offset;
        let first = ((from - base) / length).floor().max(0.0) as i64;
        let last = ((to - base) / length).floor() as i64;
        for k in first..=last {
            let origin = base + k as f64 * length;
            self.pattern(index, verse, origin, (from, to), frame);
        }
    }

    /// Pattern `index` starting at written beat `origin`, its notes kept when
    /// they start in `[from, to)`.
    fn pattern(
        &mut self,
        index: usize,
        verse: u32,
        origin: f64,
        (from, to): (f64, f64),
        frame: Frame,
    ) {
        let length = self.p.patterns[index].length;
        let (p, tempo) = (self.p, self.tempo);
        let notes = self
            .expanded
            .entry((index, verse))
            .or_insert_with(|| with_phonemes(p, index, verse));
        for (n, phonemes) in notes.iter() {
            let t = origin + n.start;
            if n.start >= length || t < from - 1e-9 || t >= to - 1e-9 {
                continue;
            }
            let end = (t + n.length).min(to.max(t + 1e-6));
            let (start, end) = (frame.time(tempo, t), frame.time(tempo, end));
            self.out.push(performed(p, n, phonemes, start, end));
        }
    }
}

/// A sounding note and the phonemes it sings.
type Voiced = (Sounding, Vec<String>);

/// A pattern's sounding notes for a verse, each with its phonemes.
fn with_phonemes(p: &Project, index: usize, verse: u32) -> Vec<Voiced> {
    let notes = expand::pattern(p, index, verse);
    let phonemes = rosaclef_phonetics::pronounce_all(p, &notes);
    notes.into_iter().zip(phonemes).collect()
}

fn performed(p: &Project, n: &Sounding, phonemes: &[String], start: Time, end: Time) -> Performed {
    let pitched = p
        .channel(&n.channel)
        .is_none_or(|c| c.instrument.is_pitched());
    let transpose = if pitched { p.transport.transpose } else { 0 };
    Performed {
        channel: n.channel.clone(),
        pitch: (n.pitch + transpose).clamp(0, 127),
        start,
        end,
        velocity: n.velocity,
        syllable: n.lyric.as_ref().map(|l| {
            let line = l.line.lyrics(p);
            Syllable {
                token: l.token.clone(),
                phonemes: phonemes.to_vec(),
                lang: line.language().to_string(),
                mode: line.mode,
                timing: line.timing_at(l.line.verse, l.line.at).cloned(),
                section: p.patterns[l.line.pattern].name.clone(),
                verse: l.line.verse,
            }
        }),
    }
}

#[cfg(test)]
mod tests;
