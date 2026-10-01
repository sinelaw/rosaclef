use super::*;
use crate::lyrics::{Sung, WordPos};
use crate::model::{Channel, Device, Note};

fn project() -> Project {
    let mut p = Project::empty("t");
    p.patterns.clear();
    for (id, kind) in [("lead", "synth"), ("bass", "synth"), ("kit", "drum")] {
        p.channels.push(Channel {
            id: id.into(),
            name: id.into(),
            color: "#c9a45c".into(),
            instrument: Device::new(kind),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            mixer: Default::default(),
            arp: None,
        });
    }
    p
}

fn note(channel: &str, pitch: i32, start: f64) -> Note {
    Note {
        channel: channel.into(),
        pitch,
        start,
        length: 1.0,
        velocity: 0.8,
    }
}

fn pattern(id: &str, length: f64, notes: Vec<Note>) -> Pattern {
    Pattern {
        id: id.into(),
        name: id.into(),
        color: "#c9a45c".into(),
        length,
        notes,
        uses: vec![],
        lyrics: vec![],
    }
}

fn use_of(pattern: &str, start: f64) -> Use {
    Use {
        pattern: pattern.into(),
        start,
        from: 0.0,
        to: None,
        transpose: 0,
        channel: String::new(),
        velocity: 1.0,
        verse: None,
    }
}

fn line(channel: &str, verses: &[(u32, &str)]) -> Lyrics {
    Lyrics {
        channel: channel.into(),
        lang: String::new(),
        mode: Default::default(),
        verses: verses.iter().map(|(k, v)| (*k, v.to_string())).collect(),
        timing: vec![],
    }
}

/// (channel, pitch, start) of each sounding note.
fn shape(notes: &[Sounding]) -> Vec<(String, i32, f64)> {
    notes
        .iter()
        .map(|n| (n.channel.clone(), n.pitch, n.start))
        .collect()
}

/// The text each note sings ("" for none, "_" for a hold).
fn words(notes: &[Sounding]) -> Vec<String> {
    notes
        .iter()
        .map(|n| match n.lyric.as_ref().map(|l| &l.token.sung) {
            Some(Sung::Syllable { text, .. }) => text.clone(),
            Some(Sung::Hold) => "_".into(),
            Some(Sung::Breath) => "(br)".into(),
            None => String::new(),
        })
        .collect()
}

#[test]
fn a_use_plays_the_pattern_moved_and_transposed() {
    let mut p = project();
    p.patterns.push(pattern(
        "hook",
        2.0,
        vec![note("lead", 60, 0.0), note("lead", 62, 1.0)],
    ));
    let mut verse = pattern("verse", 8.0, vec![note("lead", 55, 0.0)]);
    verse.uses.push(Use {
        transpose: 5,
        ..use_of("hook", 4.0)
    });
    p.patterns.push(verse);
    let notes = pattern_by_id(&p, "verse", 1);
    assert_eq!(
        shape(&notes),
        vec![
            ("lead".into(), 55, 0.0),
            ("lead".into(), 65, 4.0),
            ("lead".into(), 67, 5.0)
        ]
    );
    assert_eq!(
        notes[1].origin,
        Origin {
            pattern: 0,
            note: 0
        }
    );
}

#[test]
fn a_range_is_cut_and_can_move_to_another_channel() {
    let mut p = project();
    let mut hook = pattern(
        "hook",
        4.0,
        vec![
            note("lead", 60, 0.0),
            note("lead", 62, 2.0),
            note("lead", 64, 3.5),
        ],
    );
    hook.notes[2].length = 2.0;
    p.patterns.push(hook);
    let mut verse = pattern("verse", 8.0, vec![]);
    verse.uses.push(Use {
        from: 2.0,
        to: Some(4.0),
        channel: "bass".into(),
        transpose: -12,
        velocity: 0.5,
        ..use_of("hook", 6.0)
    });
    p.patterns.push(verse);
    let notes = pattern_by_id(&p, "verse", 1);
    assert_eq!(
        shape(&notes),
        vec![("bass".into(), 50, 6.0), ("bass".into(), 52, 7.5)]
    );
    assert_eq!(notes[1].length, 0.5, "cut at the end of the range");
    assert_eq!(notes[0].velocity, 0.4);
}

#[test]
fn drums_are_not_transposed_and_cycles_are_cut() {
    let mut p = project();
    let mut a = pattern("a", 4.0, vec![note("kit", 36, 0.0)]);
    a.uses.push(Use {
        transpose: 7,
        ..use_of("b", 1.0)
    });
    let mut b = pattern("b", 4.0, vec![note("kit", 38, 0.0)]);
    b.uses.push(use_of("a", 2.0));
    p.patterns.push(a);
    p.patterns.push(b);
    let notes = pattern_by_id(&p, "a", 1);
    // a, then b inside it, then a inside b is cut (a is already playing).
    assert_eq!(
        shape(&notes),
        vec![("kit".into(), 36, 0.0), ("kit".into(), 38, 1.0)]
    );
}

#[test]
fn syllables_fall_on_the_channels_notes_in_time_order() {
    let mut p = project();
    let mut v = pattern(
        "verse",
        4.0,
        vec![
            note("lead", 64, 2.0),
            note("lead", 60, 0.0),
            note("bass", 40, 0.0),
            note("lead", 62, 1.0),
            note("lead", 65, 3.0),
        ],
    );
    v.lyrics
        .push(line("lead", &[(1, "Hel-lo _ friend"), (2, "Good-bye")]));
    p.patterns.push(v);
    let notes = pattern_by_id(&p, "verse", 1);
    let lead: Vec<Sounding> = notes.into_iter().filter(|n| n.channel == "lead").collect();
    assert_eq!(words(&lead), ["Hel", "lo", "_", "friend"]);
    let Some(Sung::Syllable { pos, .. }) = lead[0].lyric.as_ref().map(|l| &l.token.sung) else {
        panic!("a syllable");
    };
    assert_eq!(*pos, WordPos::Begin);
    let second = pattern_by_id(&p, "verse", 2);
    let lead2: Vec<String> = words(&second)
        .into_iter()
        .zip(&second)
        .filter(|(_, n)| n.channel == "lead")
        .map(|(w, _)| w)
        .collect();
    assert_eq!(lead2, ["Good", "bye", "", ""]);
    // A verse it does not have sings the first.
    assert_eq!(words(&pattern_by_id(&p, "verse", 3))[0], "Hel");
}

#[test]
fn a_used_pattern_brings_its_words_and_the_outer_line_fills_the_rest() {
    let mut p = project();
    let mut hook = pattern(
        "hook",
        2.0,
        vec![note("lead", 67, 0.0), note("lead", 69, 1.0)],
    );
    hook.lyrics
        .push(line("lead", &[(1, "oh yeah"), (2, "oh no")]));
    p.patterns.push(hook);
    let mut verse = pattern(
        "verse",
        8.0,
        vec![note("lead", 60, 0.0), note("lead", 62, 1.0)],
    );
    verse.uses.push(use_of("hook", 2.0));
    verse.uses.push(Use {
        verse: Some(2),
        ..use_of("hook", 4.0)
    });
    verse.lyrics.push(line("lead", &[(1, "I said")]));
    p.patterns.push(verse);
    let notes = pattern_by_id(&p, "verse", 1);
    assert_eq!(words(&notes), ["I", "said", "oh", "yeah", "oh", "no"]);
    let hook_line = notes[2].lyric.as_ref().unwrap().line;
    assert_eq!(
        (hook_line.pattern, hook_line.verse, hook_line.at),
        (0, 1, 0.0)
    );
    assert_eq!(verse_count(&p, 1), 2);
}

#[test]
fn fit_counts_tokens_against_free_notes() {
    let mut p = project();
    let mut v = pattern("v", 4.0, vec![note("lead", 60, 0.0), note("lead", 62, 1.0)]);
    v.lyrics
        .push(line("lead", &[(1, "one two three"), (2, "one")]));
    p.patterns.push(v);
    assert_eq!(fit(&p, 0, 0, 1), (3, 2));
    assert_eq!(fit(&p, 0, 0, 2), (1, 2));
}

#[test]
fn notes_moved_to_another_channel_leave_their_words() {
    let mut p = project();
    let mut hook = pattern("hook", 1.0, vec![note("lead", 67, 0.0)]);
    hook.lyrics.push(line("lead", &[(1, "oh")]));
    p.patterns.push(hook);
    let mut v = pattern("v", 4.0, vec![]);
    v.uses.push(use_of("hook", 0.0));
    v.uses.push(Use {
        channel: "bass".into(),
        ..use_of("hook", 0.0)
    });
    v.uses.push(Use {
        channel: "lead".into(),
        ..use_of("hook", 2.0)
    });
    p.patterns.push(v);
    let notes = pattern_by_id(&p, "v", 1);
    let sung: Vec<(&str, bool)> = notes
        .iter()
        .map(|n| (n.channel.as_str(), n.lyric.is_some()))
        .collect();
    assert_eq!(sung, [("lead", true), ("bass", false), ("lead", true)]);
}
