//! MusicXML 4.0 (partwise) of the sung lines, for notation programs and the
//! singers that read scores (Sinsy and NNSVS, VoiSona, Synthesizer V, ACE
//! Studio).
//!
//! A part per singing channel; notes on a sixteenth-note grid, split at bar
//! lines and tied ([`grid`]); each note's lyric with `<syllabic>` (its place
//! in the word), `<text xml:lang>`, `<extend>` over the held notes and
//! `<end-line/>` / `<end-paragraph/>`. Breaths are rests. The song is
//! written as performed (repeats unrolled, one meter), so every verse is
//! lyric number 1.

mod grid;

use super::xml::{escape, tag, Xml};
use crate::line::{Line, Performed, Song};
use grid::{Piece, DIVISIONS};
use rosaclef_core::format::format_f64;
use rosaclef_core::lyrics::{Break, Sung, WordPos};

const DOCTYPE: &str = r#"<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">"#;

pub fn write(song: &Song) -> String {
    let bar = song.beats_per_bar.max(1) * DIVISIONS;
    let total = measure_count(song, bar) * bar;
    let mut x = Xml::new(DOCTYPE);
    x.open("score-partwise", &[("version", "4.0")]);
    header(&mut x, song);
    x.open("part-list", &[]);
    for (i, line) in song.lines.iter().enumerate() {
        x.open("score-part", &[("id", &part_id(i))]);
        x.leaf("part-name", &[], &line.name);
        x.close();
    }
    x.close();
    for (i, line) in song.lines.iter().enumerate() {
        let measures = grid::measures(&grid::events(&line.notes, total), bar);
        x.open("part", &[("id", &part_id(i))]);
        for (m, pieces) in measures.iter().enumerate() {
            x.open("measure", &[("number", &(m + 1).to_string())]);
            if m == 0 {
                attributes(&mut x, song, line);
                if i == 0 {
                    tempo(&mut x, song.bpm);
                }
            }
            for p in pieces {
                note(&mut x, p);
            }
            x.close();
        }
        x.close();
    }
    x.finish()
}

fn part_id(i: usize) -> String {
    format!("P{}", i + 1)
}

/// Measures every part has: enough for the last note of any line.
fn measure_count(song: &Song, bar: u32) -> u32 {
    let end = song
        .lines
        .iter()
        .flat_map(|l| &l.notes)
        .map(|n| n.end.beat)
        .fold(0.0, f64::max);
    let divisions = (end * DIVISIONS as f64).round() as u32;
    divisions.div_ceil(bar).max(1)
}

fn header(x: &mut Xml, song: &Song) {
    x.open("work", &[]);
    x.leaf("work-title", &[], &song.title);
    x.close();
    x.open("identification", &[]);
    if !song.author.is_empty() {
        x.leaf("creator", &[("type", "composer")], &song.author);
    }
    x.open("encoding", &[]);
    x.leaf("software", &[], "Rosaclef");
    x.close();
    x.close();
}

/// Divisions, key, meter and a clef that fits the line.
fn attributes(x: &mut Xml, song: &Song, line: &Line) {
    x.open("attributes", &[]);
    x.leaf("divisions", &[], &DIVISIONS.to_string());
    x.raw(&tag("key", &[], &tag("fifths", &[], "0")));
    let beats = tag("beats", &[], &song.beats_per_bar.max(1).to_string());
    x.raw(&tag("time", &[], &(beats + &tag("beat-type", &[], "4"))));
    let (sign, staff_line) = if low(line) { ("F", "4") } else { ("G", "2") };
    let clef = tag("sign", &[], sign) + &tag("line", &[], staff_line);
    x.raw(&tag("clef", &[], &clef));
    x.close();
}

/// Whether most of the line sits below G3 (a bass clef line).
fn low(line: &Line) -> bool {
    let mut pitches: Vec<i32> = line.notes.iter().map(|n| n.pitch).collect();
    pitches.sort_unstable();
    pitches.get(pitches.len() / 2).is_some_and(|p| *p < 55)
}

fn tempo(x: &mut Xml, bpm: f64) {
    let bpm = format_f64(bpm);
    x.open("direction", &[("placement", "above")]);
    let metronome = tag("beat-unit", &[], "quarter") + &tag("per-minute", &[], &bpm);
    let metronome = tag("metronome", &[], &metronome);
    x.raw(&tag("direction-type", &[], &metronome));
    x.empty("sound", &[("tempo", &bpm)]);
    x.close();
}

fn note(x: &mut Xml, p: &Piece) {
    x.open("note", &[]);
    match p.event.note {
        Some(n) => x.raw(&pitch(n.pitch)),
        None => x.empty("rest", &[]),
    }
    x.leaf("duration", &[], &p.len.to_string());
    let ties = ties(p);
    for t in &ties {
        x.empty("tie", &[("type", t)]);
    }
    x.leaf("voice", &[], "1");
    x.leaf("type", &[], p.kind);
    if p.dotted {
        x.empty("dot", &[]);
    }
    if !ties.is_empty() {
        let tied: Vec<String> = ties
            .iter()
            .map(|t| tag("tied", &[("type", t)], ""))
            .collect();
        x.raw(&tag("notations", &[], &tied.concat()));
    }
    if let Some(n) = p.event.note.filter(|_| p.first()) {
        if let Some(l) = lyric(n, p.event.held) {
            x.raw(&l);
        }
    }
    x.close();
}

/// The ties of a sounding piece: "stop" from the piece before, "start" to
/// the one after.
fn ties(p: &Piece) -> Vec<&'static str> {
    let stop = p.tied_from.then_some("stop");
    let start = p.tied_on.then_some("start");
    stop.into_iter().chain(start).collect()
}

fn pitch(midi: i32) -> String {
    const STEPS: [(&str, i32); 12] = [
        ("C", 0),
        ("C", 1),
        ("D", 0),
        ("D", 1),
        ("E", 0),
        ("F", 0),
        ("F", 1),
        ("G", 0),
        ("G", 1),
        ("A", 0),
        ("A", 1),
        ("B", 0),
    ];
    let (step, alter) = STEPS[midi.rem_euclid(12) as usize];
    let mut inner = tag("step", &[], step);
    if alter != 0 {
        inner += &tag("alter", &[], &alter.to_string());
    }
    inner += &tag("octave", &[], &(midi.div_euclid(12) - 1).to_string());
    tag("pitch", &[], &inner)
}

/// A note's `<lyric>`: its syllable, or the extender of a held syllable.
/// `held`: the next note holds this note's syllable.
fn lyric(n: &Performed, held: bool) -> Option<String> {
    let s = n.syllable.as_ref()?;
    let mut inner = match &s.token.sung {
        Sung::Syllable { text, pos, .. } => {
            let mut v = tag("syllabic", &[], syllabic(*pos));
            v += &tag("text", &[("xml:lang", &s.lang)], &escape(text));
            if held {
                v += &tag("extend", &[("type", "start")], "");
            }
            v
        }
        Sung::Hold => {
            let kind = if held { "continue" } else { "stop" };
            tag("extend", &[("type", kind)], "")
        }
        Sung::Breath => return None,
    };
    inner += match s.token.brk {
        Break::None => "",
        Break::Line => "<end-line/>",
        Break::Paragraph => "<end-paragraph/>",
    };
    Some(tag("lyric", &[("number", "1")], &inner))
}

fn syllabic(pos: WordPos) -> &'static str {
    match pos {
        WordPos::Single => "single",
        WordPos::Begin => "begin",
        WordPos::Middle => "middle",
        WordPos::End => "end",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn a_part_with_lyrics_extenders_and_rests() {
        let text = write(&song());
        let doc = roxmltree::Document::parse_with_options(
            &text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .unwrap();
        let count = |name: &str| doc.descendants().filter(|n| n.has_tag_name(name)).count();
        assert_eq!(count("part"), 1);
        assert_eq!(count("measure"), 4);
        assert_eq!(count("lyric"), 15, "13 syllables, 2 extender stops");
        assert_eq!(count("rest"), 2, "the breaths");
        assert!(text.contains(
            "<lyric number=\"1\"><syllabic>begin</syllabic><text xml:lang=\"en\">Hel</text></lyric>"
        ));
        assert!(
            text.contains("<text xml:lang=\"en\">friend</text><extend type=\"start\"/></lyric>")
        );
        assert!(text.contains("<lyric number=\"1\"><extend type=\"stop\"/><end-line/></lyric>"));
        assert!(text.contains("<pitch><step>F</step><octave>4</octave></pitch>"));
        assert!(text.contains("<sound tempo=\"120\"/>"));
    }

    #[test]
    fn spells_sharps() {
        assert_eq!(
            pitch(61),
            "<pitch><step>C</step><alter>1</alter><octave>4</octave></pitch>"
        );
    }
}
