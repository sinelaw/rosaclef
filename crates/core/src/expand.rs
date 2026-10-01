//! Patterns as they sound: references resolved, lyrics on their notes.
//!
//! A pattern's notes are its own plus those of the patterns it [`Use`]s —
//! each a range of another pattern, moved in time and pitch, maybe onto
//! another channel. Uses nest; a cycle is cut (validation reports it).
//!
//! Lyrics bind at every level: a pattern's [`Lyrics`] line gives its words,
//! in time order, to the notes on its channel that its uses did not already
//! give words. So a hook brings its own words wherever it is used, and the
//! verse around it fills in the rest. Notes a use moves to another channel
//! leave their words behind (a doubling on another instrument).
//!
//! The engine, the score, the exporters and `rosaclef summary` all read this,
//! so they agree on what plays.

use crate::lyrics::{self, Lyrics, Token};
use crate::model::{Pattern, Project, Use};

/// Nesting deeper than this is cut (validation reports cycles).
const MAX_DEPTH: usize = 16;

/// A note as it sounds in a pattern, in that pattern's beats.
#[derive(Clone, Debug, PartialEq)]
pub struct Sounding {
    pub channel: String,
    pub pitch: i32,
    pub start: f64,
    pub length: f64,
    pub velocity: f64,
    /// The written note it comes from.
    pub origin: Origin,
    pub lyric: Option<Lyric>,
}

/// Where a sounding note is written: `patterns[pattern].notes[note]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub pattern: usize,
    pub note: usize,
}

/// The lyric token a note sings, and the line it comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct Lyric {
    pub token: Token,
    pub line: LineRef,
}

/// A lyric line (`patterns[pattern].lyrics[index]`), the verse sung and the
/// note's beat in that pattern (where fixed timing is anchored).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineRef {
    pub pattern: usize,
    pub index: usize,
    pub verse: u32,
    pub at: f64,
}

impl LineRef {
    pub fn lyrics<'a>(&self, p: &'a Project) -> &'a Lyrics {
        &p.patterns[self.pattern].lyrics[self.index]
    }
}

/// The notes of `patterns[index]` as they sound when it sings `verse`,
/// sorted by start.
pub fn pattern(p: &Project, index: usize, verse: u32) -> Vec<Sounding> {
    let mut stack = vec![];
    expand(p, index, verse, &mut stack)
}

/// The sounding notes of the pattern with this id.
pub fn pattern_by_id(p: &Project, id: &str, verse: u32) -> Vec<Sounding> {
    match p.patterns.iter().position(|pt| pt.id == id) {
        Some(i) => pattern(p, i, verse),
        None => vec![],
    }
}

/// The highest verse any lyric line in this pattern's tree has (at least 1).
pub fn verse_count(p: &Project, index: usize) -> u32 {
    let mut stack = vec![];
    verses_in(p, index, &mut stack).max(1)
}

fn verses_in(p: &Project, index: usize, stack: &mut Vec<usize>) -> u32 {
    if stack.contains(&index) || stack.len() >= MAX_DEPTH {
        return 0;
    }
    stack.push(index);
    let pat = &p.patterns[index];
    let own = pat
        .lyrics
        .iter()
        .filter_map(|l| l.verses.keys().next_back().copied())
        .max()
        .unwrap_or(0);
    let used = pat
        .uses
        .iter()
        .filter_map(|u| position(p, &u.pattern))
        .map(|j| verses_in(p, j, stack))
        .max()
        .unwrap_or(0);
    stack.pop();
    own.max(used)
}

fn position(p: &Project, id: &str) -> Option<usize> {
    p.patterns.iter().position(|pt| pt.id == id)
}

fn expand(p: &Project, index: usize, verse: u32, stack: &mut Vec<usize>) -> Vec<Sounding> {
    if stack.contains(&index) || stack.len() >= MAX_DEPTH {
        return vec![];
    }
    stack.push(index);
    let mut out = gather(p, index, verse, stack);
    for (k, line) in p.patterns[index].lyrics.iter().enumerate() {
        bind(&mut out, index, k, line, verse);
    }
    stack.pop();
    out
}

/// A pattern's own notes and its uses' (with their words), sorted by start;
/// its own lyrics not yet bound.
fn gather(p: &Project, index: usize, verse: u32, stack: &mut Vec<usize>) -> Vec<Sounding> {
    let pat = &p.patterns[index];
    let mut out = own_notes(pat, index);
    for u in &pat.uses {
        let Some(j) = position(p, &u.pattern) else {
            continue;
        };
        let inner = expand(p, j, u.verse.unwrap_or(verse), stack);
        out.extend(place(p, u, p.patterns[j].length, inner));
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

fn own_notes(pat: &Pattern, index: usize) -> Vec<Sounding> {
    pat.notes
        .iter()
        .enumerate()
        .map(|(k, n)| Sounding {
            channel: n.channel.clone(),
            pitch: n.pitch,
            start: n.start,
            length: n.length,
            velocity: n.velocity,
            origin: Origin {
                pattern: index,
                note: k,
            },
            lyric: None,
        })
        .collect()
}

/// The used pattern's notes in its range, moved into the using pattern.
fn place(p: &Project, u: &Use, used_length: f64, notes: Vec<Sounding>) -> Vec<Sounding> {
    let to = u.to.unwrap_or(used_length);
    notes
        .into_iter()
        .filter(|n| n.start >= u.from - 1e-9 && n.start < to - 1e-9)
        .map(|mut n| {
            n.length = n.length.min(to - n.start);
            n.start = u.start + (n.start - u.from);
            if !u.channel.is_empty() && u.channel != n.channel {
                // A doubling on another instrument: the words stay with the voice.
                n.channel = u.channel.clone();
                n.lyric = None;
            }
            if pitched(p, &n.channel) {
                n.pitch = (n.pitch + u.transpose).clamp(0, 127);
            }
            n.velocity = (n.velocity * u.velocity).clamp(0.0, 1.0);
            n
        })
        .collect()
}

fn pitched(p: &Project, channel: &str) -> bool {
    p.channel(channel)
        .map(|c| c.instrument.is_pitched())
        .unwrap_or(true)
}

/// Give a lyric line's tokens, in order, to the notes on its channel that
/// have no words yet.
fn bind(out: &mut [Sounding], pattern: usize, index: usize, line: &Lyrics, verse: u32) {
    let Some((sung, text)) = line.verse_text(verse) else {
        return;
    };
    let tokens = lyrics::parse(text).unwrap_or_default();
    let free = out
        .iter_mut()
        .filter(|n| n.channel == line.channel && n.lyric.is_none());
    for (n, token) in free.zip(tokens) {
        n.lyric = Some(Lyric {
            token,
            line: LineRef {
                pattern,
                index,
                verse: sung,
                at: n.start,
            },
        });
    }
}

/// The sounding notes of a pattern as text, one per line (`rosaclef summary
/// --expand`): beat, channel, pitch, length, velocity, the syllable sung and
/// where a used note comes from.
pub fn listing(p: &Project, index: usize, verse: u32) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for n in pattern(p, index, verse) {
        let word = n
            .lyric
            .as_ref()
            .map(|l| l.token.reads())
            .unwrap_or_default();
        let from = if n.origin.pattern == index {
            String::new()
        } else {
            format!("  (from {})", p.patterns[n.origin.pattern].id)
        };
        let _ = writeln!(
            s,
            "{:>8}  {:<12} {:>3} {:<4} len {:<6} vel {:<5} {word}{from}",
            crate::format::format_f64(n.start),
            n.channel,
            n.pitch,
            note_name(n.pitch),
            crate::format::format_f64(n.length),
            crate::format::format_f64(n.velocity),
        );
    }
    s
}

fn note_name(pitch: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[pitch.rem_euclid(12) as usize],
        pitch.div_euclid(12) - 1
    )
}

/// How many tokens a verse has for how many notes: (tokens, free notes).
/// Used by validation to point out words without notes and the reverse.
pub fn fit(p: &Project, index: usize, line: usize, verse: u32) -> (usize, usize) {
    let pat = &p.patterns[index];
    let l = &pat.lyrics[line];
    let mut stack = vec![index];
    let mut notes = gather(p, index, verse, &mut stack);
    for (k, earlier) in pat.lyrics.iter().enumerate().take(line) {
        bind(&mut notes, index, k, earlier, verse);
    }
    let free = notes
        .iter()
        .filter(|n| n.channel == l.channel && n.lyric.is_none() && n.start < pat.length)
        .count();
    let tokens = l
        .verses
        .get(&verse)
        .map(|t| lyrics::parse(t).map(|v| v.len()).unwrap_or(0))
        .unwrap_or(0);
    (tokens, free)
}

#[cfg(test)]
mod tests;
