//! DiffSinger segments (`.ds`, JSON): the lead line cut at rests of
//! [`SPLIT`] seconds or more, each segment with its phonemes, their
//! durations and its notes.
//!
//! - `offset`: where the segment starts, in seconds. It opens with a
//!   silence (`SP`) of up to [`PAD`] seconds before its first note and
//!   closes with one after its last.
//! - `ph_seq`: the phonemes (ARPAbet for English, romaji for Japanese, IPA
//!   otherwise), `SP` for silence, `AP` for a breath.
//! - `ph_dur`: seconds per phoneme. Timing fixed in the project is used as
//!   it is; otherwise [`timing::rule`]: onset consonants take 0.08 s each
//!   before the note, so the vowel starts on it; coda consonants take
//!   0.08 s each at the end of the syllable (after its held notes); the
//!   vowel takes the rest.
//! - `ph_num`: phonemes per group. A group starts at a syllable's vowel
//!   (its onset consonants belong to the group before), or at a silence or
//!   breath.
//! - `note_seq`, `note_dur`, `note_slur`: a note per group (`rest` for
//!   silences and breaths), and the held notes with `note_slur` 1.
//! - `text`: each group's syllable, `SP` or `AP`.

use super::{lead, monophonic, note_name, phones};
use crate::line::{Performed, Song};
use crate::timing;
use rosaclef_core::format::format_f64;
use rosaclef_core::lyrics::Sung;
use rosaclef_phonetics::ipa;
use serde_json::{json, Value};

/// Rests this long (seconds) or longer cut the line into segments.
pub const SPLIT: f64 = 0.5;
/// Silence before and after a segment's notes, in seconds.
pub const PAD: f64 = 0.5;
/// The shortest phoneme, in seconds.
const MIN_PHONE: f64 = 0.01;

pub fn write(song: &Song) -> String {
    let notes = monophonic(lead(song));
    let list: Vec<Value> = segments(&notes).into_iter().map(segment).collect();
    serde_json::to_string_pretty(&list).expect("JSON values serialize") + "\n"
}

/// Runs of notes without a rest of [`SPLIT`] seconds between them.
fn segments(notes: &[Performed]) -> Vec<&[Performed]> {
    let mut out = vec![];
    let mut first = 0;
    for i in 1..=notes.len() {
        if i == notes.len() || notes[i].start.sec - notes[i - 1].end.sec >= SPLIT - 1e-9 {
            out.push(&notes[first..i]);
            first = i;
        }
    }
    out.retain(|s| !s.is_empty());
    out
}

fn segment(notes: &[Performed]) -> Value {
    let start = (notes[0].start.sec - PAD).max(0.0);
    let end = notes[notes.len() - 1].end.sec + PAD;
    let score = Score::of(notes, start);
    let phones = settle(&score.phones, start);
    let ph_dur = durations(&phones.iter().map(|p| p.start).collect::<Vec<_>>(), end);
    let note_dur = durations(
        &score.tones.iter().map(|t| t.start).collect::<Vec<_>>(),
        end,
    );
    let groups: Vec<usize> = (0..phones.len())
        .filter(|&i| phones[i].text.is_some())
        .collect();
    let ph_num: Vec<String> = (0..groups.len())
        .map(|k| (groups.get(k + 1).unwrap_or(&phones.len()) - groups[k]).to_string())
        .collect();
    let join = |v: Vec<String>| v.join(" ");
    json!({
        "offset": round(start),
        "text": join(phones.iter().filter_map(|p| p.text.clone()).collect()),
        "ph_seq": join(phones.iter().map(|p| p.name.clone()).collect()),
        "ph_dur": join(ph_dur),
        "ph_num": join(ph_num),
        "note_seq": join(score.tones.iter().map(|t| t.name.clone()).collect()),
        "note_dur": join(note_dur),
        "note_slur": join(score.tones.iter().map(|t| u8::from(t.slur).to_string()).collect()),
    })
}

/// A phoneme and when it starts (seconds from the song's start).
#[derive(Clone, Debug)]
struct Phone {
    name: String,
    start: f64,
    /// The lyric of the group it starts; `None` inside a group.
    text: Option<String>,
}

/// A note of the segment: its name (or `rest`), its start, whether it
/// holds the syllable before.
#[derive(Clone, Debug)]
struct Tone {
    name: String,
    start: f64,
    slur: bool,
}

/// A segment's phonemes and notes, in order.
#[derive(Default)]
struct Score {
    phones: Vec<Phone>,
    tones: Vec<Tone>,
}

impl Score {
    fn of(notes: &[Performed], start: f64) -> Score {
        let mut s = Score::default();
        s.silence("SP", start);
        let mut i = 0;
        while i < notes.len() {
            let n = &notes[i];
            if i > 0 && n.start.sec - notes[i - 1].end.sec > 1e-6 {
                s.silence("SP", notes[i - 1].end.sec);
            }
            i += match n.syllable.as_ref().map(|syl| &syl.token.sung) {
                Some(Sung::Syllable { text, .. }) => s.syllable(notes, i, text),
                Some(Sung::Hold) => s.held(n),
                Some(Sung::Breath) => s.silence("AP", n.start.sec),
                None => s.silence("SP", n.start.sec),
            };
        }
        s.silence("SP", notes[notes.len() - 1].end.sec);
        s
    }

    /// A silence or breath with its rest note; one note used.
    fn silence(&mut self, name: &str, at: f64) -> usize {
        self.phones.push(Phone {
            name: name.into(),
            start: at,
            text: Some(name.into()),
        });
        self.tones.push(Tone {
            name: "rest".into(),
            start: at,
            slur: false,
        });
        1
    }

    /// A note holding the syllable before; one note used.
    fn held(&mut self, n: &Performed) -> usize {
        self.tones.push(Tone {
            name: note_name(n.pitch),
            start: n.start.sec,
            slur: true,
        });
        1
    }

    /// The syllable on `notes[i]` and the notes holding it; returns how
    /// many notes it used.
    fn syllable(&mut self, notes: &[Performed], i: usize, text: &str) -> usize {
        let n = &notes[i];
        let holds = notes[i + 1..]
            .iter()
            .take_while(|h| matches!(&h.syllable, Some(s) if s.token.sung == Sung::Hold))
            .count();
        let end = notes[i + holds].end.sec;
        let earliest = self.phones.last().map_or(n.start.sec, |p| p.start);
        let timed = phonemes(n, earliest, end);
        let group = timed
            .iter()
            .position(|(p, _)| ipa::is_vowel(p))
            .unwrap_or(0);
        let lang = n.syllable.as_ref().map_or("", |s| s.lang.as_str());
        for (k, (p, start)) in timed.iter().enumerate() {
            self.phones.push(Phone {
                name: phones::for_engine(lang, p),
                start: *start,
                text: (k == group).then(|| text.to_string()),
            });
        }
        if timed.is_empty() {
            self.phones.push(Phone {
                name: "SP".into(),
                start: n.start.sec,
                text: Some(text.to_string()),
            });
        }
        self.tones.push(Tone {
            name: note_name(n.pitch),
            start: n.start.sec,
            slur: false,
        });
        for h in &notes[i + 1..=i + holds] {
            self.held(h);
        }
        1 + holds
    }
}

/// A syllable's phonemes (IPA) and their starts: fixed in the project, or
/// by [`timing::rule`].
fn phonemes(n: &Performed, earliest: f64, end: f64) -> Vec<(String, f64)> {
    let Some(s) = n.syllable.as_ref() else {
        return vec![];
    };
    if let Some(t) = &s.timing {
        return t
            .phonemes
            .iter()
            .map(|p| (p.p.clone(), n.start.sec + p.offset))
            .collect();
    }
    let starts = timing::rule(&s.phonemes, earliest, n.start.sec, end);
    s.phonemes.iter().cloned().zip(starts).collect()
}

/// Phonemes in order, each at least [`MIN_PHONE`] after the one before,
/// none before the segment.
fn settle(phones: &[Phone], start: f64) -> Vec<Phone> {
    let mut out: Vec<Phone> = Vec::with_capacity(phones.len());
    for p in phones {
        let floor = out.last().map_or(start, |q| q.start + MIN_PHONE);
        out.push(Phone {
            start: p.start.max(floor),
            ..p.clone()
        });
    }
    out
}

/// Durations of spans starting at `starts` and ending at the next (the last
/// at `end`), in milliseconds-rounded seconds that add up exactly.
fn durations(starts: &[f64], end: f64) -> Vec<String> {
    let ms: Vec<i64> = starts
        .iter()
        .chain([end].iter())
        .map(|t| (t * 1000.0).round() as i64)
        .collect();
    ms.windows(2)
        .map(|w| format_f64((w[1] - w[0]).max(0) as f64 / 1000.0))
        .collect()
}

fn round(sec: f64) -> f64 {
    (sec * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{project, song};
    use rosaclef_core::{SyllableTiming, TimedPhoneme};

    fn field<'a>(v: &'a Value, k: &str) -> Vec<&'a str> {
        v[k].as_str().unwrap().split(' ').collect()
    }

    fn total(v: &Value, k: &str) -> f64 {
        field(v, k).iter().map(|x| x.parse::<f64>().unwrap()).sum()
    }

    #[test]
    fn segments_with_groups_notes_and_durations() {
        let v: Value = serde_json::from_str(&write(&song())).unwrap();
        let list = v.as_array().unwrap();
        assert_eq!(list.len(), 1, "no rest of half a second");
        let seg = &list[0];
        assert_eq!(seg["offset"], 0.0);
        let ph = field(seg, "ph_seq");
        assert_eq!(ph[..9], ["SP", "hh", "ax", "l", "ow", "d", "aa", "r", "k"]);
        let notes = field(seg, "note_seq");
        assert_eq!(notes[..6], ["rest", "C4", "D4", "E4", "F4", "G4"]);
        assert_eq!(field(seg, "note_slur")[5], "1", "the held note");
        assert_eq!(field(seg, "note_seq")[6], "rest", "the breath");
        assert!(ph.contains(&"AP"));
        // Onset consonants belong to the group before: SP+hh, ax+l, ow+d ...
        assert_eq!(field(seg, "ph_num")[..3], ["2", "2", "2"]);
        let groups: usize = field(seg, "ph_num")
            .iter()
            .map(|x| x.parse::<usize>().unwrap())
            .sum();
        assert_eq!(groups, ph.len());
        let non_slur = field(seg, "note_slur")
            .iter()
            .filter(|s| **s == "0")
            .count();
        assert_eq!(field(seg, "ph_num").len(), non_slur);
        assert!((total(seg, "ph_dur") - total(seg, "note_dur")).abs() < 1e-9);
        assert!((total(seg, "note_dur") - 8.5).abs() < 1e-9);
        // The vowel of "lo" starts on its note, half a second in; its onset
        // takes 0.08 s before it.
        let l = ph.iter().position(|p| *p == "l").unwrap();
        let durs = field(seg, "ph_dur");
        assert_eq!(durs[l], "0.08");
        let before: f64 = durs[..=l].iter().map(|x| x.parse::<f64>().unwrap()).sum();
        assert!((before - 0.5).abs() < 1e-9);
        assert_eq!(field(seg, "note_dur")[1], "0.5");
    }

    #[test]
    fn fixed_timing_and_rests_split_segments() {
        let mut p = project();
        p.patterns[0].lyrics[0].timing = vec![SyllableTiming {
            verse: 1,
            at: 2.0,
            phonemes: ["d", "ɑ", "ɹ", "k"]
                .iter()
                .zip([-0.05, 0.0, 0.3, 0.4])
                .map(|(p, offset)| TimedPhoneme {
                    p: p.to_string(),
                    offset,
                })
                .collect(),
        }];
        p.patterns[0]
            .notes
            .retain(|n| n.start != 4.0 || n.channel != "lead");
        p.patterns[0].lyrics[0]
            .verses
            .insert(1, "Hel-lo dark friend _ / Hi".into());
        let v: Value = serde_json::from_str(&write(&crate::line::Song::of_project(&p))).unwrap();
        let list = v.as_array().unwrap();
        assert_eq!(list.len(), 3, "half a second of rest after each hold");
        let ph = field(&list[0], "ph_seq");
        let d = ph.iter().position(|p| *p == "d").unwrap();
        let durs = field(&list[0], "ph_dur");
        // "friend" follows at 1.5 s: its onset squeezes into k's 0.1 s.
        assert_eq!(durs[d..d + 4], ["0.05", "0.3", "0.1", "0.033"]);
        assert_eq!(list[1]["offset"], 2.0);
    }
}
