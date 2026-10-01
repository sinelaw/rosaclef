//! UltraStar karaoke songs (`.txt`, format 1.1.0) of the lead line.
//!
//! - One tempo: `#BPM` is the song's first tempo, and as UltraStar counts a
//!   quarter of its beat, a quarter note is 4 file beats. Note times come
//!   from seconds, so tempo changes are flattened, not lost.
//! - `#GAP`: milliseconds from the start of the audio to the first note
//!   (file beat 0). `#AUDIO` names the mixdown as `rosaclef render` saves
//!   it (`my-song.wav`).
//! - Notes: `: beat length pitch text` (`R` for rap lines, `F` for spoken
//!   ones); pitch 0 is C4. A word's last note ends its text with a space;
//!   held notes sing `~`; breaths are left out.
//! - `- beat` ends each lyric line; `E` ends the song.

use super::{lead, monophonic, paragraphs};
use crate::line::{Performed, Song};
use rosaclef_core::format::{format_f64, slug};
use rosaclef_core::lyrics::Sung;
use rosaclef_core::LyricMode;

pub fn write(song: &Song) -> String {
    let notes = monophonic(lead(song));
    let gap = notes.first().map_or(0.0, |n| n.start.sec);
    let clock = Clock { gap, bpm: song.bpm };
    let mut breaks: Vec<f64> = paragraphs(song)
        .iter()
        .flatten()
        .map(|ph| ph.end().sec)
        .collect();
    breaks.pop();
    let mut out = header(song, gap);
    let mut pending = breaks.iter().peekable();
    for (i, n) in notes.iter().enumerate() {
        let Some(text) = text(&notes, i) else {
            continue;
        };
        let due = |b: &&f64| **b <= n.start.sec + 1e-6;
        if let Some(b) = pending.next_if(due) {
            out += &format!("- {}\n", clock.beat(*b));
            while pending.next_if(due).is_some() {}
        }
        let start = clock.beat(n.start.sec);
        let length = (clock.beat(n.end.sec) - start).max(1);
        out += &format!("{} {start} {length} {} {text}\n", kind(n), n.pitch - 60);
    }
    out + "E\n"
}

fn header(song: &Song, gap: f64) -> String {
    format!(
        "#VERSION:1.1.0\n#TITLE:{}\n#ARTIST:{}\n#AUDIO:{}.wav\n#BPM:{}\n#GAP:{}\n",
        song.title,
        song.author,
        slug(&song.title),
        format_f64(song.bpm),
        (gap * 1000.0).round()
    )
}

/// Seconds to file beats: a quarter of a beat at the song's tempo, from
/// the gap.
struct Clock {
    gap: f64,
    bpm: f64,
}

impl Clock {
    fn beat(&self, sec: f64) -> i64 {
        ((sec - self.gap) * self.bpm * 4.0 / 60.0).round() as i64
    }
}

fn kind(n: &Performed) -> &'static str {
    match n.syllable.as_ref().map(|s| s.mode) {
        Some(LyricMode::Rap) => "R",
        Some(LyricMode::Speak) => "F",
        _ => ":",
    }
}

/// What note `i` sings: its syllable or `~`, with a space when the word
/// ends on it; `None` for breaths and notes without words.
fn text(notes: &[Performed], i: usize) -> Option<String> {
    let sung = |k: usize| notes.get(k)?.syllable.as_ref().map(|s| &s.token.sung);
    let held_on = matches!(sung(i + 1), Some(Sung::Hold));
    let (text, ends) = match sung(i)? {
        Sung::Syllable { text, pos, .. } => (text.as_str(), pos.ends_word()),
        Sung::Hold => ("~", word_ended_before(notes, i)),
        Sung::Breath => return None,
    };
    let space = if ends && !held_on { " " } else { "" };
    Some(format!("{text}{space}"))
}

/// Whether the syllable a hold at `i` holds ends its word.
fn word_ended_before(notes: &[Performed], i: usize) -> bool {
    notes[..i]
        .iter()
        .rev()
        .find_map(|n| match n.syllable.as_ref().map(|s| &s.token.sung) {
            Some(Sung::Syllable { pos, .. }) => Some(pos.ends_word()),
            Some(Sung::Hold) => None,
            _ => Some(true),
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{project, song};

    #[test]
    fn notes_holds_and_line_breaks() {
        let text = write(&song());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[..6],
            [
                "#VERSION:1.1.0",
                "#TITLE:Song",
                "#ARTIST:Ann",
                "#AUDIO:song.wav",
                "#BPM:120",
                "#GAP:0"
            ]
        );
        assert_eq!(
            lines[6..14],
            [
                ": 0 4 0 Hel",
                ": 4 4 2 lo ",
                ": 8 4 4 dark ",
                ": 12 2 5 friend",
                ": 14 2 7 ~ ",
                "- 16",
                ": 20 4 9 Hi ",
                "- 24",
            ]
        );
        assert_eq!(lines.last(), Some(&"E"));
    }

    #[test]
    fn rap_lines() {
        let mut p = project();
        p.patterns[0].lyrics[0].mode = LyricMode::Rap;
        let text = write(&Song::of_project(&p));
        assert!(text.contains("\nR 0 4 0 Hel\n"));
        assert!(text.contains("\n: 24 4 7 La \n"), "the hook still sings");
    }
}
