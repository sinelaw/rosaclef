use super::*;
use rosaclef_core::lyrics::Sung;
use rosaclef_core::{Channel, Clip, Device, Lyrics, Note, Pattern, Repeat, TrackIx};

/// Lead sings "Hel-lo" (verse 1) / "Good-bye" (verse 2) on two quarter
/// notes; a drum plays along. 120 BPM: a beat is half a second.
fn project() -> Project {
    let mut p = Project::empty("Song");
    p.transport.transpose = 2;
    for (id, kind) in [("lead", "synth"), ("kit", "drum")] {
        p.channels.push(Channel {
            id: id.into(),
            name: id.to_uppercase(),
            color: "#c9a45c".into(),
            instrument: Device::new(kind),
            volume: 0.8,
            pan: 0.0,
            mute: false,
            mixer: Default::default(),
            arp: None,
        });
    }
    let note = |channel: &str, pitch, start| Note {
        channel: channel.into(),
        pitch,
        start,
        length: 1.0,
        velocity: 0.8,
    };
    p.patterns = vec![Pattern {
        id: "v".into(),
        name: "V".into(),
        color: "#c9a45c".into(),
        length: 2.0,
        notes: vec![
            note("lead", 60, 0.0),
            note("lead", 62, 1.0),
            note("kit", 36, 0.0),
        ],
        uses: vec![],
        lyrics: vec![Lyrics {
            channel: "lead".into(),
            lang: String::new(),
            mode: LyricMode::Sing,
            verses: [(1, "Hel-lo".to_string()), (2, "Good-bye".to_string())].into(),
            timing: vec![],
        }],
    }];
    p
}

fn clip(start: f64, length: f64, verse: Option<u32>) -> Clip {
    Clip {
        pattern: "v".into(),
        sample: String::new(),
        track: TrackIx(0),
        start,
        length,
        offset: 0.0,
        gain: 1.0,
        mixer: Default::default(),
        verse,
    }
}

fn texts(line: &Line) -> Vec<String> {
    line.notes
        .iter()
        .map(|n| match n.syllable.as_ref().map(|s| &s.token.sung) {
            Some(Sung::Syllable { text, .. }) => text.clone(),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn clips_sing_their_verse_and_time_runs_on() {
    let mut p = project();
    p.playlist.clips = vec![clip(0.0, 2.0, None), clip(2.0, 2.0, Some(2))];
    let song = Song::of_project(&p);
    assert_eq!(song.lines.len(), 1, "only the lead sings");
    let lead = &song.lines[0];
    assert_eq!(texts(lead), ["Hel", "lo", "Good", "bye"]);
    let starts: Vec<(f64, f64)> = lead
        .notes
        .iter()
        .map(|n| (n.start.beat, n.start.sec))
        .collect();
    assert_eq!(starts, [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (3.0, 1.5)]);
    assert_eq!(lead.notes[0].pitch, 62, "transposed");
    let kick = song.notes.iter().find(|n| n.channel == "kit").unwrap();
    assert_eq!(kick.pitch, 36, "drums are not transposed");
    let hello = lead.notes[0].syllable.as_ref().unwrap();
    assert_eq!(hello.phonemes.join(" "), "h ə");
    assert_eq!(
        lead.notes[1].syllable.as_ref().unwrap().phonemes.join(" "),
        "l oʊ"
    );
}

#[test]
fn a_repeat_sings_the_next_verse_and_unrolls_time() {
    let mut p = project();
    p.playlist.clips = vec![clip(0.0, 2.0, None)];
    p.repeats.push(Repeat {
        start: 0.0,
        end: 2.0,
        times: 2,
        endings: vec![],
    });
    let song = Song::of_project(&p);
    let lead = &song.lines[0];
    assert_eq!(texts(lead), ["Hel", "lo", "Good", "bye"]);
    assert_eq!(lead.notes[3].start.beat, 3.0);
    assert!((lead.notes[3].end.sec - 2.0).abs() < 1e-9);
}

#[test]
fn a_pattern_alone() {
    let p = project();
    let song = Song::of_pattern(&p, "v", 2).unwrap();
    assert_eq!(texts(&song.lines[0]), ["Good", "bye"]);
    assert!(Song::of_pattern(&p, "nope", 1).is_none());
}
