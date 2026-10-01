//! Patterns compiled for the sequencer: their sounding notes (uses resolved,
//! see [`rosaclef_core::expand`]) and, per verse, the syllable each note
//! sings.

use crate::instruments::{LyricId, Sung};
use rosaclef_core::arp;
use rosaclef_core::expand::{self, Sounding};
use rosaclef_core::Project;

#[derive(Clone, Copy)]
pub(crate) struct CNote {
    pub channel: usize,
    pub key: u8,
    /// Start without swing.
    pub start: f64,
    /// On an off-beat 16th: delayed by the swing amount.
    pub swung: bool,
    pub length: f64,
    pub velocity: f32,
    /// Its place among the pattern's sounding notes.
    pub index: u32,
}

impl CNote {
    #[inline]
    pub fn start_at(&self, swing_shift: f64) -> f64 {
        if self.swung {
            self.start + swing_shift
        } else {
            self.start
        }
    }
}

pub(crate) struct CPattern {
    pub id: String,
    pub length: f64,
    pub notes: Vec<CNote>,
    /// Per verse (from 1; the last entry is any later verse), the syllable
    /// of each sounding note. Empty without lyrics.
    verses: Vec<Vec<Option<LyricId>>>,
}

impl CPattern {
    /// The syllable a note sings in `verse`.
    #[inline]
    pub fn lyric(&self, note: &CNote, verse: u32) -> Option<LyricId> {
        let n = self.verses.len();
        if n == 0 {
            return None;
        }
        let v = (verse.max(1) as usize).min(n) - 1;
        self.verses[v].get(note.index as usize).copied().flatten()
    }
}

/// Every pattern, and the table of syllables their notes refer to.
pub(crate) fn compile(project: &Project) -> (Vec<CPattern>, Vec<Sung>) {
    let mut table = vec![];
    let patterns = (0..project.patterns.len())
        .map(|i| compile_one(project, i, &mut table))
        .collect();
    (patterns, table)
}

fn compile_one(project: &Project, index: usize, table: &mut Vec<Sung>) -> CPattern {
    let p = &project.patterns[index];
    let sounding = expand::pattern(project, index, 1);
    let notes = played(project, &sounding);
    let sung = sounding.iter().any(|n| n.lyric.is_some());
    let verses = if sung {
        // One more than the verses written: the verse every later one sings.
        let count = expand::verse_count(project, index) + 1;
        (1..=count)
            .map(|v| syllables(project, &expand::pattern(project, index, v), table))
            .collect()
    } else {
        vec![]
    };
    CPattern {
        id: p.id.clone(),
        length: p.length.max(1e-3),
        notes,
        verses,
    }
}

/// The notes a pattern's sounding notes play, sorted: a channel with an
/// arpeggiator plays its notes as runs, each run note keeping its source's
/// index (so it sings that note's syllable).
fn played(project: &Project, sounding: &[Sounding]) -> Vec<CNote> {
    let mut out = vec![];
    for (channel, ch) in project.channels.iter().enumerate() {
        let mine: Vec<usize> = (0..sounding.len())
            .filter(|&k| sounding[k].channel == ch.id)
            .collect();
        let held = |k: usize| arp::Held {
            start: sounding[k].start,
            length: sounding[k].length,
            pitch: sounding[k].pitch,
        };
        match &ch.arp {
            None => out.extend(
                mine.iter()
                    .map(|&k| note(project, channel, held(k), sounding[k].velocity, k)),
            ),
            Some(a) => {
                let input: Vec<arp::Held> = mine.iter().map(|&k| held(k)).collect();
                out.extend(arp::arpeggiate(a, &input).into_iter().map(|h| {
                    let k = mine[h.source];
                    let run = arp::Held {
                        start: h.start,
                        length: h.length,
                        pitch: h.pitch,
                    };
                    note(project, channel, run, sounding[k].velocity, k)
                }));
            }
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

fn note(project: &Project, channel: usize, n: arp::Held, velocity: f64, index: usize) -> CNote {
    let sixteenth = n.start * 4.0;
    let on_grid = (sixteenth - sixteenth.round()).abs() < 1e-6;
    let swung = on_grid && (sixteenth.round() as i64) % 2 == 1;
    let shift = if project.channels[channel].instrument.is_pitched() {
        project.transport.transpose.clamp(-12, 12)
    } else {
        0
    };
    CNote {
        channel,
        key: (n.pitch + shift).clamp(0, 127) as u8,
        start: n.start,
        swung,
        length: n.length.max(1e-4),
        velocity: velocity as f32,
        index: index as u32,
    }
}

/// The syllable of each sounding note, added to the table.
fn syllables(project: &Project, notes: &[Sounding], table: &mut Vec<Sung>) -> Vec<Option<LyricId>> {
    notes
        .iter()
        .map(|n| {
            let lyric = n.lyric.as_ref()?;
            let line = lyric.line.lyrics(project);
            table.push(Sung {
                token: lyric.token.clone(),
                lang: line.language().to_string(),
                mode: line.mode,
            });
            Some((table.len() - 1) as LyricId)
        })
        .collect()
}
