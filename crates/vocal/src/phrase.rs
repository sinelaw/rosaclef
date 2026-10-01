//! The phrases a project's rendering voices need (see
//! [`rosaclef_core::phrase`]), each as a small [`Song`] a singing engine can
//! take: the phrase's notes alone, timed from [`LEAD`] seconds before its
//! first note, so the engine's audio lines up with the render's path.

use crate::line::{self, Performed, Song, Time};
use rosaclef_core::expand::{self, Sounding};
use rosaclef_core::phrase::{self, Phrase, LEAD};
use rosaclef_core::Project;

/// A phrase to render.
#[derive(Clone, Debug)]
pub struct Job {
    /// The voice (from the user's voices settings) that sings it.
    pub voice: String,
    /// Where the render goes, in the project folder.
    pub path: String,
    pub song: Song,
}

/// Every phrase the project's rendering voices sing, once each.
pub fn jobs(p: &Project) -> Vec<Job> {
    let mut out: Vec<Job> = vec![];
    for c in p.channels.iter().filter(|c| renders(c)) {
        let voice = c.instrument.option("voice");
        for index in 0..p.patterns.len() {
            for verse in 1..=expand::verse_count(p, index) + 1 {
                let notes = expand::pattern(p, index, verse);
                let phonemes = rosaclef_phonetics::pronounce_all(p, &notes);
                for ph in phrase::phrases(p, &notes, &c.id, voice) {
                    let path = phrase::path(voice, &ph.key);
                    if out.iter().all(|j| j.path != path) {
                        let song = phrase_song(p, &notes, &phonemes, &ph);
                        out.push(Job {
                            voice: voice.to_string(),
                            path,
                            song,
                        });
                    }
                }
            }
        }
    }
    out
}

fn renders(c: &rosaclef_core::Channel) -> bool {
    let d = &c.instrument;
    d.kind == "voice" && d.option("engine") == "render" && !d.option("voice").is_empty()
}

/// A phrase's notes as a song of their own, starting LEAD before the first.
fn phrase_song(p: &Project, notes: &[Sounding], phonemes: &[Vec<String>], ph: &Phrase) -> Song {
    let beat_secs = 60.0 / p.transport.bpm.max(1.0);
    let time = |beat: f64| {
        let sec = LEAD + (beat - ph.start) * beat_secs;
        Time {
            beat: sec / beat_secs,
            sec,
        }
    };
    let performed: Vec<Performed> = ph
        .notes
        .iter()
        .map(|&i| {
            let n = &notes[i];
            line::performed(p, n, &phonemes[i], time(n.start), time(n.start + n.length))
        })
        .collect();
    Song::from_notes(p, performed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosaclef_core::validate::parse_and_validate;
    use serde_json::json;

    #[test]
    fn each_phrase_once_timed_from_its_lead() {
        let text = json!({
            "format": "rosaclef/1", "meta": {"title": "t"}, "transport": {"bpm": 120},
            "channels": [
                {"id": "vox", "name": "Vox", "instrument": {"type": "voice", "options": {"engine": "render", "voice": "alto"}}},
                {"id": "guide", "name": "Guide", "instrument": {"type": "voice"}}
            ],
            "patterns": [
                {"id": "v", "name": "V", "length": 8, "notes": [
                    {"channel": "vox", "pitch": 60, "start": 2, "length": 1},
                    {"channel": "vox", "pitch": 62, "start": 3, "length": 1},
                    {"channel": "guide", "pitch": 62, "start": 3, "length": 1}
                ], "lyrics": [{"channel": "vox", "verses": {"1": "Hel-lo", "2": "Good-bye"}}]},
                {"id": "w", "name": "W", "length": 8, "uses": [{"pattern": "v", "start": 0}]}
            ],
            "mixer": {"inserts": [{"name": "Master"}]}
        });
        let p = parse_and_validate(&text.to_string()).project.unwrap();
        let jobs = jobs(&p);
        // Verse 1 and verse 2 of the one phrase; "w" uses "v" and needs nothing new.
        assert_eq!(
            jobs.len(),
            2,
            "{:?}",
            jobs.iter().map(|j| &j.path).collect::<Vec<_>>()
        );
        let song = &jobs[0].song;
        assert_eq!(jobs[0].voice, "alto");
        assert_eq!(song.notes.len(), 2);
        assert!((song.notes[0].start.sec - LEAD).abs() < 1e-9);
        assert!((song.notes[1].start.sec - (LEAD + 0.5)).abs() < 1e-9);
        assert_eq!(song.lines.len(), 1);
        assert_eq!(
            song.notes[1].syllable.as_ref().unwrap().phonemes.join(" "),
            "l oʊ"
        );
    }
}
