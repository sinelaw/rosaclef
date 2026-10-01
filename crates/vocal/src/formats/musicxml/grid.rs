//! A sung line on a sixteenth-note grid, cut into notes a score can write:
//! starts and ends rounded to the grid, one note at a time, rests between
//! them, every note split at bar lines and into written values (dotted or
//! not) tied together.

use crate::line::Performed;
use rosaclef_core::lyrics::Sung;

/// Divisions of a quarter note: the grid is sixteenth notes.
pub const DIVISIONS: u32 = 4;

/// Written values, longest first: (divisions, type, dotted).
const VALUES: &[(u32, &str, bool)] = &[
    (16, "whole", false),
    (12, "half", true),
    (8, "half", false),
    (6, "quarter", true),
    (4, "quarter", false),
    (3, "eighth", true),
    (2, "eighth", false),
    (1, "16th", false),
];

/// A sound or a silence of the line, in divisions.
#[derive(Clone, Debug, PartialEq)]
pub struct Event<'a> {
    pub start: u32,
    pub len: u32,
    /// The note sung; `None` for a rest (a breath is a rest too).
    pub note: Option<&'a Performed>,
    /// The note after it in the line holds its syllable.
    pub held: bool,
}

/// One written note or rest: part of an event.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece<'a> {
    pub event: Event<'a>,
    pub len: u32,
    pub kind: &'static str,
    pub dotted: bool,
    /// Tied to the piece before / after (same note; rests are not tied).
    pub tied_from: bool,
    pub tied_on: bool,
}

impl Piece<'_> {
    /// The first piece of its event carries the lyric.
    pub fn first(&self) -> bool {
        !self.tied_from
    }
}

/// The line's sounds and rests, from 0 to `total` divisions.
pub fn events(notes: &[Performed], total: u32) -> Vec<Event<'_>> {
    let mut out: Vec<Event> = vec![];
    for (i, n) in notes.iter().enumerate() {
        if is_breath(n) {
            continue;
        }
        let start = grid(n.start.beat);
        let end = grid(n.end.beat).max(start + 1).min(total);
        if start >= total || out.last().is_some_and(|p| p.start >= start) {
            continue;
        }
        if let Some(prev) = out.last_mut() {
            prev.len = prev.len.min(start - prev.start);
        }
        let held = notes.get(i + 1).is_some_and(is_hold);
        out.push(Event {
            start,
            len: end - start,
            note: Some(n),
            held,
        });
    }
    with_rests(out, total)
}

fn grid(beat: f64) -> u32 {
    (beat.max(0.0) * DIVISIONS as f64).round() as u32
}

fn sung(n: &Performed) -> Option<&Sung> {
    n.syllable.as_ref().map(|s| &s.token.sung)
}

fn is_breath(n: &Performed) -> bool {
    matches!(sung(n), Some(Sung::Breath))
}

fn is_hold(n: &Performed) -> bool {
    matches!(sung(n), Some(Sung::Hold))
}

/// Rests in the gaps, so the events cover `[0, total)`.
fn with_rests(sounds: Vec<Event<'_>>, total: u32) -> Vec<Event<'_>> {
    let rest = |start, end: u32| Event {
        start,
        len: end - start,
        note: None,
        held: false,
    };
    let mut out = vec![];
    let mut at = 0;
    for e in sounds {
        if e.start > at {
            out.push(rest(at, e.start));
        }
        at = e.start + e.len;
        out.push(e);
    }
    if total > at {
        out.push(rest(at, total));
    }
    out
}

/// Each measure's pieces (`bar`: divisions per measure).
pub fn measures<'a>(events: &[Event<'a>], bar: u32) -> Vec<Vec<Piece<'a>>> {
    let mut out: Vec<Vec<Piece>> = vec![];
    for e in events {
        let mut at = e.start;
        let end = e.start + e.len;
        while at < end {
            let m = (at / bar) as usize;
            let stop = end.min((m as u32 + 1) * bar);
            if out.len() <= m {
                out.resize_with(m + 1, Vec::new);
            }
            for (len, kind, dotted) in values(stop - at) {
                out[m].push(Piece {
                    event: e.clone(),
                    len,
                    kind,
                    dotted,
                    tied_from: e.note.is_some() && at > e.start,
                    tied_on: false,
                });
                at += len;
            }
        }
    }
    for m in &mut out {
        tie_on(m);
    }
    tie_across(&mut out);
    out
}

/// Written values that add up to `len`, longest first.
fn values(mut len: u32) -> Vec<(u32, &'static str, bool)> {
    let mut out = vec![];
    while len > 0 {
        let v = *VALUES
            .iter()
            .find(|v| v.0 <= len)
            .expect("16ths fill any length");
        out.push(v);
        len -= v.0;
    }
    out
}

/// A piece is tied on when the next piece continues its event.
fn tie_on(pieces: &mut [Piece]) {
    for i in 1..pieces.len() {
        if pieces[i].tied_from {
            pieces[i - 1].tied_on = true;
        }
    }
}

/// The last piece of a measure is tied on when the next measure starts by
/// continuing its event.
fn tie_across(measures: &mut [Vec<Piece>]) {
    for m in 1..measures.len() {
        if measures[m].first().is_some_and(|p| p.tied_from) {
            if let Some(last) = measures[m - 1].last_mut() {
                last.tied_on = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    fn shape(m: &[Piece]) -> Vec<String> {
        m.iter()
            .map(|p| {
                let what = if p.event.note.is_some() { "n" } else { "r" };
                let dot = if p.dotted { "." } else { "" };
                let tie = if p.tied_on { "~" } else { "" };
                format!("{what}{}{dot}{tie}", p.len)
            })
            .collect()
    }

    #[test]
    fn rests_breaths_and_ties_across_the_bar() {
        let s = song();
        let ev = events(&s.lines[0].notes, 64);
        let bars = measures(&ev, 16);
        assert_eq!(bars.len(), 4);
        // Hel lo dark friend _ | (br) Hi La la
        assert_eq!(shape(&bars[0]), ["n4", "n4", "n4", "n2", "n2"]);
        assert_eq!(shape(&bars[1]), ["r4", "n4", "n4", "n4"]);
        assert!(ev[3].held && !ev[4].held);
    }

    #[test]
    fn long_notes_are_split_and_tied() {
        let s = song();
        let mut notes = s.lines[0].notes[..1].to_vec();
        notes[0].start.beat = 3.0;
        notes[0].end.beat = 5.75;
        let bars = measures(&events(&notes, 32), 16);
        assert_eq!(shape(&bars[0]), ["r12.", "n4~"]);
        assert_eq!(shape(&bars[1]), ["n6.~", "n1", "r8", "r1"]);
        assert!(bars[1][0].tied_from && !bars[1][0].first());
    }
}
