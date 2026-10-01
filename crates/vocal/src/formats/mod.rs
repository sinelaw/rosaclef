//! The files a performed [`Song`] is written to: one module per format,
//! each a pure function from the song to the file, and [`FORMATS`], the
//! table the command line, the HTTP API and the studio list them from.
//!
//! The project stores meaning ("this note holds the syllable before", "this
//! syllable ends a word"); each writer spells it with its format's markers.
//! Formats with one voice (karaoke, timed text, speech, DiffSinger) write
//! the first channel that sings; the others write every sung channel.

pub mod diffsinger;
pub mod jam;
pub mod lrc;
pub mod musicxml;
mod phones;
pub mod ssml;
mod sung;
pub mod svp;
pub mod tagged;
pub mod ttml;
pub mod ultrastar;
pub mod ustx;
mod xml;

use crate::line::{Line, Performed, Song};
use crate::words::{self, Phrase};

/// A file format a song can be written to.
pub struct Format {
    /// What `--format` and `?format=` name it by.
    pub id: &'static str,
    /// The file name extension, without the dot.
    pub extension: &'static str,
    /// The media type served for it.
    pub mime: &'static str,
    pub label: &'static str,
    /// What reads it.
    pub description: &'static str,
    write: fn(&Song) -> Vec<u8>,
}

impl Format {
    /// The song in this format.
    pub fn write(&self, song: &Song) -> Vec<u8> {
        (self.write)(song)
    }
}

/// Every format, in the order menus list them.
pub const FORMATS: &[Format] = &[
    Format {
        id: "musicxml",
        extension: "musicxml",
        mime: "application/vnd.recordare.musicxml+xml",
        label: "MusicXML",
        description:
            "Sheet music of the sung lines with lyrics (MuseScore, Sibelius, Sinsy, VoiSona)",
        write: |s| musicxml::write(s).into_bytes(),
    },
    Format {
        id: "ustx",
        extension: "ustx",
        mime: "text/yaml; charset=utf-8",
        label: "OpenUtau",
        description: "OpenUtau project (DiffSinger, ENUNU and UTAU voicebanks)",
        write: |s| ustx::write(s).into_bytes(),
    },
    Format {
        id: "svp",
        extension: "svp",
        mime: "application/json",
        label: "Synthesizer V",
        description: "Synthesizer V Studio project",
        write: |s| svp::write(s).into_bytes(),
    },
    Format {
        id: "ds",
        extension: "ds",
        mime: "application/json",
        label: "DiffSinger",
        description: "DiffSinger segments: phonemes with durations, notes",
        write: |s| diffsinger::write(s).into_bytes(),
    },
    Format {
        id: "ultrastar",
        extension: "txt",
        mime: "text/plain; charset=utf-8",
        label: "UltraStar",
        description: "UltraStar karaoke song (UltraStar Deluxe, Vocaluxe, Performous)",
        write: |s| ultrastar::write(s).into_bytes(),
    },
    Format {
        id: "lrc",
        extension: "lrc",
        mime: "text/plain; charset=utf-8",
        label: "LRC (word timing)",
        description: "Timed lyric lines with word times (karaoke players, DiffRhythm)",
        write: |s| lrc::write(s).into_bytes(),
    },
    Format {
        id: "ttml",
        extension: "ttml",
        mime: "application/ttml+xml",
        label: "TTML (word timing)",
        description: "Timed text with word spans, as music apps show lyrics",
        write: |s| ttml::write(s).into_bytes(),
    },
    Format {
        id: "jam",
        extension: "json",
        mime: "application/json",
        label: "Timed words (JSON)",
        description: "Words with start and end seconds (JAM and other song generators)",
        write: |s| jam::write(s).into_bytes(),
    },
    Format {
        id: "tagged",
        extension: "txt",
        mime: "text/plain; charset=utf-8",
        label: "Tagged lyrics",
        description: "[Verse] / [Chorus] paragraphs for song generators (ACE-Step, YuE, Suno)",
        write: |s| tagged::write(s).into_bytes(),
    },
    Format {
        id: "ssml",
        extension: "ssml",
        mime: "application/ssml+xml",
        label: "SSML",
        description: "Speech markup with pronunciations, pauses and word marks (text to speech)",
        write: |s| ssml::write(s).into_bytes(),
    },
];

/// The format with this id.
pub fn find(id: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|f| f.id == id)
}

/// The line one-voice formats write: the first channel that sings.
fn lead(song: &Song) -> &[Performed] {
    song.lines.first().map_or(&[], |l: &Line| &l.notes)
}

/// The lead line's paragraphs of lyric lines (a bar's rest ends a line).
fn paragraphs(song: &Song) -> Vec<Vec<Phrase<'_>>> {
    words::read(lead(song), song.beats_per_bar.max(1) as f64)
}

/// A line's notes one at a time: of notes starting together the first is
/// kept, and a note ends where the next starts.
fn monophonic(notes: &[Performed]) -> Vec<Performed> {
    let mut out: Vec<Performed> = vec![];
    for n in notes {
        match out.last_mut() {
            Some(prev) if n.start.beat - prev.start.beat < 1e-9 => continue,
            Some(prev) if prev.end.beat > n.start.beat => prev.end = n.start,
            _ => {}
        }
        out.push(n.clone());
    }
    out
}

/// A note's name with sharps and octave ("C#4"; 60 is C4).
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

/// Seconds rounded to milliseconds, for JSON.
fn ms(sec: f64) -> f64 {
    (sec * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn ids_are_unique_and_every_format_writes_the_song() {
        let s = song();
        for (i, f) in FORMATS.iter().enumerate() {
            assert!(FORMATS[..i].iter().all(|g| g.id != f.id), "{}", f.id);
            assert!(!f.write(&s).is_empty(), "{}", f.id);
            assert!(find(f.id).is_some());
        }
        assert!(find("nope").is_none());
    }

    #[test]
    fn one_note_at_a_time() {
        let s = song();
        let mut notes = s.lines[0].notes.clone();
        notes[1].start = notes[0].start;
        notes[2].start.beat = 0.5;
        let mono = monophonic(&notes);
        assert_eq!(mono.len(), notes.len() - 1);
        assert_eq!(mono[0].end.beat, 0.5);
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(57), "A3");
    }
}
