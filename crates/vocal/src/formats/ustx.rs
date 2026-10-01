//! OpenUtau projects (`.ustx`, YAML), which drive DiffSinger, ENUNU and
//! UTAU voicebanks: a track and a voice part per singing channel, notes at
//! 480 ticks per quarter note, one tempo (the song's first).
//!
//! Lyrics as OpenUtau's English phonemizers read them: a word on its first
//! note, `+` on the notes of its later syllables, `+~` on held notes;
//! languages typed by syllable put each syllable on its note ([`sung`]). A
//! pronunciation given in the lyrics becomes a phoneme hint, `word[ph ph]`
//! (ARPAbet for English). Breaths are `AP`, as DiffSinger banks name them;
//! notes without words sing `a`, OpenUtau's default lyric. The singer and phonemizer are left for the user to pick.

use super::sung::{self, Marks};
use super::{monophonic, phones};
use crate::line::{Line, Performed, Song};
use rosaclef_core::format::format_f64;

const TICKS: f64 = 480.0;

const MARKS: Marks = Marks {
    next: "+",
    hold: "+~",
    breath: "AP",
};

pub fn write(song: &Song) -> String {
    let mut y = header(song);
    y += "tracks:\n";
    for line in &song.lines {
        y += &track(line);
    }
    y += "voice_parts:\n";
    for (i, line) in song.lines.iter().enumerate() {
        y += &voice_part(i, line);
    }
    y += "wave_parts: []\n";
    y
}

/// A YAML string (JSON's double-quoted strings are YAML too).
fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialize")
}

fn header(song: &Song) -> String {
    let bpm = format_f64(song.bpm);
    let beats = song.beats_per_bar.max(1);
    format!(
        "name: {}\ncomment: {}\noutput_dir: Vocal\ncache_dir: UCache\nustx_version: \"0.6\"\n\
         resolution: 480\nbpm: {bpm}\nbeat_per_bar: {beats}\nbeat_unit: 4\n\
         expressions:\n{}{}\
         tempos:\n- position: 0\n  bpm: {bpm}\n\
         time_signatures:\n- bar_position: 0\n  beat_per_bar: {beats}\n  beat_unit: 4\n",
        quote(&song.title),
        quote(&song.author),
        expression("vel", "velocity"),
        expression("vol", "volume"),
    )
}

/// One of OpenUtau's standard numeric expressions (0–200, 100 by default).
fn expression(abbr: &str, name: &str) -> String {
    format!(
        "  {abbr}:\n    name: {name}\n    abbr: {abbr}\n    type: Numerical\n    min: 0\n    \
         max: 200\n    default_value: 100\n    is_flag: false\n    flag: ''\n"
    )
}

fn track(line: &Line) -> String {
    format!(
        "- singer: ''\n  phonemizer: OpenUtau.Core.DefaultPhonemizer\n  renderer_settings: {{}}\n  \
         track_name: {}\n  mute: false\n  solo: false\n  volume: 0\n  pan: 0\n",
        quote(&line.name)
    )
}

fn voice_part(index: usize, line: &Line) -> String {
    let notes = monophonic(&line.notes);
    let lyrics = sung::lyrics(&notes, &MARKS);
    let mut y = format!(
        "- name: {}\n  comment: ''\n  track_no: {index}\n  position: 0\n  notes:\n",
        quote(&line.name)
    );
    for (i, n) in notes.iter().enumerate() {
        let lyric = match &lyrics[i] {
            Some(text) => text.clone() + &hint(&notes, i),
            None => "a".to_string(),
        };
        y += &note(n, &lyric);
    }
    y
}

/// `[ph ph]` when the lyrics give the pronunciation of what note `i` spells.
fn hint(notes: &[Performed], i: usize) -> String {
    let unit = sung::unit(notes, i);
    if !unit.iter().any(|&k| sung::written(&notes[k])) {
        return String::new();
    }
    let spelled: Vec<String> = unit
        .iter()
        .filter_map(|&k| notes[k].syllable.as_ref())
        .flat_map(|s| s.phonemes.iter().map(|p| phones::for_engine(&s.lang, p)))
        .collect();
    format!("[{}]", spelled.join(" "))
}

fn note(n: &Performed, lyric: &str) -> String {
    let position = (n.start.beat * TICKS).round() as i64;
    let duration = ((n.end.beat - n.start.beat) * TICKS).round().max(1.0) as i64;
    format!(
        "  - position: {position}\n    duration: {duration}\n    tone: {}\n    lyric: {}\n    \
         pitch:\n      data:\n      - {{x: -25, y: 0, shape: io}}\n      - {{x: 25, y: 0, shape: io}}\n      \
         snap_first: true\n    \
         vibrato: {{length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0}}\n    \
         note_expressions: []\n    phoneme_expressions: []\n    phoneme_overrides: []\n",
        n.pitch,
        quote(lyric)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    fn lyrics(text: &str) -> Vec<&str> {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix("lyric: "))
            .collect()
    }

    #[test]
    fn notes_in_ticks_with_extenders_and_hints() {
        let text = write(&song());
        assert!(text.starts_with("name: \"Song\"\ncomment: \"Ann\"\n"));
        assert!(text.contains("resolution: 480\nbpm: 120\n"));
        assert!(text.contains("  track_name: \"Lead\"\n"));
        assert_eq!(
            lyrics(&text)[..9],
            [
                "\"Hello\"",
                "\"+\"",
                "\"dark\"",
                "\"friend[f r eh n d]\"",
                "\"+~\"",
                "\"AP\"",
                "\"Hi\"",
                "\"La\"",
                "\"la\""
            ]
        );
        assert!(text
            .contains("  - position: 1680\n    duration: 240\n    tone: 67\n    lyric: \"+~\"\n"));
        assert_eq!(lyrics(&text).len(), 18);
    }
}
