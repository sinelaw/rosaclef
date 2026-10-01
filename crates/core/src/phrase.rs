//! Phrases: the runs of a vocal channel's notes that a singing engine
//! renders in one go (the notes between rests, like DiffSinger's segments).
//!
//! A phrase is named by what it sings — its notes, words, the tempo and the
//! voice — so a render is reused wherever and however often the pattern
//! plays, and an edit only re-renders the phrases it touched. Renders live
//! at [`path`]; their audio starts [`LEAD`] seconds before the first note,
//! so consonants sung ahead of the beat have room.

use crate::expand::Sounding;
use crate::Project;
use std::fmt::Write;

/// Seconds of audio before a phrase's first note.
pub const LEAD: f64 = 0.3;

/// A rest at least this long (seconds) ends a phrase.
const GAP: f64 = 0.4;

/// A phrase of one channel's sounding notes.
#[derive(Clone, Debug, PartialEq)]
pub struct Phrase {
    /// Indices of its notes among the pattern's sounding notes.
    pub notes: Vec<usize>,
    /// Its first note's start and its last note's end, in pattern beats.
    pub start: f64,
    pub end: f64,
    /// What it sings, as a short stable name.
    pub key: String,
}

/// The phrases `voice` sings on `channel`, from a pattern's sounding notes
/// (sorted by start, as [`crate::expand::pattern`] gives them).
pub fn phrases(p: &Project, notes: &[Sounding], channel: &str, voice: &str) -> Vec<Phrase> {
    let gap = GAP * p.transport.bpm / 60.0;
    let mut out: Vec<Phrase> = vec![];
    for (i, n) in notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.channel == channel)
    {
        match out.last_mut() {
            Some(ph) if n.start - ph.end < gap => {
                ph.notes.push(i);
                ph.end = ph.end.max(n.start + n.length);
            }
            _ => out.push(Phrase {
                notes: vec![i],
                start: n.start,
                end: n.start + n.length,
                key: String::new(),
            }),
        }
    }
    for ph in &mut out {
        ph.key = key(p, notes, ph, voice);
    }
    out
}

/// Where a phrase's render lives in the project folder.
pub fn path(voice: &str, key: &str) -> String {
    format!("renders/voice/{voice}/{key}.wav")
}

/// A stable name for what a phrase sings (FNV-1a of a canonical text).
fn key(p: &Project, notes: &[Sounding], ph: &Phrase, voice: &str) -> String {
    let mut text = format!("{voice}|{}|{}", p.transport.bpm, p.transport.transpose);
    for &i in &ph.notes {
        let n = &notes[i];
        let _ = write!(
            text,
            "|{} {} {} {:.3}",
            n.pitch,
            n.start - ph.start,
            n.length,
            n.velocity
        );
        if let Some(l) = &n.lyric {
            let line = l.line.lyrics(p);
            let _ = write!(text, " {:?} {} {:?}", l.token, line.language(), line.mode);
            if let Some(t) = line.timing_at(l.line.verse, l.line.at) {
                let _ = write!(text, " {:?}", t.phonemes);
            }
        }
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand;
    use crate::validate::parse_and_validate;
    use serde_json::json;

    fn project(second_note_at: f64, words: &str) -> Project {
        let text = json!({
            "format": "rosaclef/1", "meta": {"title": "t"}, "transport": {"bpm": 120},
            "channels": [{"id": "vox", "name": "Vox", "instrument": {"type": "voice"}}],
            "patterns": [{"id": "v", "name": "V", "length": 16, "notes": [
                {"channel": "vox", "pitch": 60, "start": 0, "length": 1},
                {"channel": "vox", "pitch": 62, "start": second_note_at, "length": 1},
                {"channel": "vox", "pitch": 64, "start": 8, "length": 2}
            ], "lyrics": [{"channel": "vox", "verses": {"1": words}}]}],
            "mixer": {"inserts": [{"name": "Master"}]}
        });
        parse_and_validate(&text.to_string()).project.unwrap()
    }

    fn of(p: &Project) -> Vec<Phrase> {
        phrases(p, &expand::pattern(p, 0, 1), "vox", "alto")
    }

    #[test]
    fn rests_split_phrases() {
        let ph = of(&project(1.0, "Hel-lo friend"));
        assert_eq!(ph.len(), 2);
        assert_eq!(
            (ph[0].notes.clone(), ph[0].start, ph[0].end),
            (vec![0, 1], 0.0, 2.0)
        );
        assert_eq!(
            (ph[1].notes.clone(), ph[1].start, ph[1].end),
            (vec![2], 8.0, 10.0)
        );
        assert_eq!(
            path("alto", &ph[1].key),
            format!("renders/voice/alto/{}.wav", ph[1].key)
        );
    }

    #[test]
    fn a_phrase_is_named_by_what_it_sings() {
        let a = of(&project(1.0, "Hel-lo friend"));
        let b = of(&project(1.0, "Good-bye friend"));
        let c = of(&project(1.5, "Hel-lo friend"));
        assert_ne!(a[0].key, b[0].key, "other words");
        assert_eq!(a[1].key, b[1].key, "the same last phrase");
        assert_ne!(a[0].key, c[0].key, "other timing");
        let p = project(1.0, "Hel-lo friend");
        let other_voice = phrases(&p, &expand::pattern(&p, 0, 1), "vox", "bass");
        assert_ne!(a[0].key, other_voice[0].key, "another voice");
    }
}
