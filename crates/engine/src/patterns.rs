//! Patterns compiled for the sequencer: their sounding notes (uses resolved,
//! see [`rosaclef_core::expand`]) and, per verse, what each note sings and
//! when rendered phrases start.

use crate::instruments::{LyricId, Lyrics, PhraseId, PhraseNote, Sung};
use rosaclef_core::arp;
use rosaclef_core::expand::{self, Sounding};
use rosaclef_core::phrase::{self, LEAD};
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

/// A rendered phrase starting: its audio begins at `beat` (pattern beats,
/// before its first note, maybe before the pattern's start).
#[derive(Clone, Copy)]
pub(crate) struct Cue {
    pub beat: f64,
    /// The phrase's first note.
    pub note: f64,
    pub channel: usize,
    pub phrase: PhraseId,
}

/// What a pattern's notes sing in one verse.
struct Verse {
    /// The syllable of each sounding note.
    lyrics: Vec<Option<LyricId>>,
    cues: Vec<Cue>,
}

pub(crate) struct CPattern {
    pub id: String,
    pub length: f64,
    pub notes: Vec<CNote>,
    /// Per verse from 1 (the last entry is any later verse). Empty when
    /// nothing sings.
    verses: Vec<Verse>,
}

impl CPattern {
    fn verse(&self, verse: u32) -> Option<&Verse> {
        let n = self.verses.len();
        (n > 0).then(|| &self.verses[(verse.max(1) as usize).min(n) - 1])
    }

    /// The syllable a note sings in `verse`.
    #[inline]
    pub fn lyric(&self, note: &CNote, verse: u32) -> Option<LyricId> {
        self.verse(verse)?
            .lyrics
            .get(note.index as usize)
            .copied()
            .flatten()
    }

    /// The rendered phrases that start in `verse`.
    pub fn cues(&self, verse: u32) -> &[Cue] {
        self.verse(verse).map_or(&[], |v| &v.cues)
    }
}

/// Every pattern, and the syllables and phrases their notes refer to.
pub(crate) fn compile(project: &Project) -> (Vec<CPattern>, Lyrics) {
    let mut table = Lyrics::default();
    let patterns = (0..project.patterns.len())
        .map(|i| compile_one(project, i, &mut table))
        .collect();
    (patterns, table)
}

fn compile_one(project: &Project, index: usize, table: &mut Lyrics) -> CPattern {
    let p = &project.patterns[index];
    let sounding = expand::pattern(project, index, 1);
    let notes = played(project, &sounding);
    let renders = renderers(project);
    let sings = sounding.iter().any(|n| {
        n.lyric.is_some()
            || renders
                .iter()
                .any(|(c, _)| project.channels[*c].id == n.channel)
    });
    let verses = if sings {
        // One more than the verses written: the verse every later one sings.
        let count = expand::verse_count(project, index) + 1;
        (1..=count)
            .map(|v| {
                verse(
                    project,
                    &expand::pattern(project, index, v),
                    &renders,
                    table,
                )
            })
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

/// The channels whose voice renders phrases, with the voice's name.
fn renderers(project: &Project) -> Vec<(usize, String)> {
    project
        .channels
        .iter()
        .enumerate()
        .filter(|(_, c)| c.instrument.kind == "voice" && c.instrument.option("engine") == "render")
        .filter(|(_, c)| !c.instrument.option("voice").is_empty())
        .map(|(i, c)| (i, c.instrument.option("voice").to_string()))
        .collect()
}

/// What a verse's sounding notes sing, added to the table.
fn verse(
    project: &Project,
    notes: &[Sounding],
    renders: &[(usize, String)],
    table: &mut Lyrics,
) -> Verse {
    let mut in_phrase: Vec<Option<PhraseNote>> = vec![None; notes.len()];
    let mut cues = vec![];
    let beat_secs = 60.0 / project.transport.bpm.max(1.0);
    for (channel, voice) in renders {
        let id = &project.channels[*channel].id;
        for ph in phrase::phrases(project, notes, id, voice) {
            let pid = table.phrases.len() as PhraseId;
            table.phrases.push(phrase::path(voice, &ph.key));
            for &i in &ph.notes {
                let at = LEAD + (notes[i].start - ph.start) * beat_secs;
                in_phrase[i] = Some(PhraseNote {
                    id: pid,
                    at: at as f32,
                });
            }
            cues.push(Cue {
                beat: ph.start - LEAD / beat_secs,
                note: ph.start,
                channel: *channel,
                phrase: pid,
            });
        }
    }
    let phonemes = rosaclef_phonetics::pronounce_all(project, notes);
    let lyrics = notes
        .iter()
        .zip(phonemes)
        .zip(in_phrase)
        .map(|((n, phonemes), phrase)| {
            if n.lyric.is_none() && phrase.is_none() {
                return None;
            }
            let line = n.lyric.as_ref().map(|l| l.line.lyrics(project));
            table.sung.push(Sung {
                token: n.lyric.as_ref().map(|l| l.token.clone()),
                lang: line.map_or("en", |l| l.language()).to_string(),
                mode: line.map(|l| l.mode).unwrap_or_default(),
                phonemes,
                phrase,
            });
            Some((table.sung.len() - 1) as LyricId)
        })
        .collect();
    Verse { lyrics, cues }
}
