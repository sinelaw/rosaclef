//! The small song the exporters are tested on.

use crate::line::Song;
use rosaclef_core::{
    Channel, Clip, Device, LyricMode, Lyrics, Note, Pattern, Project, Repeat, TrackIx, Use,
};

/// "Song" by Ann, 120 BPM (a beat is half a second), 4/4.
///
/// - `verse` (8 beats) sings, on `lead`: verse 1 "Hel-lo dark
///   friend[f ɹ ɛ n d] _ / (br) Hi", verse 2 "Bye now friend _ / (br) Hi";
///   a kick plays on beats 0 and 4.
/// - At beat 6 it uses `hook` (2 beats), which brings its own words "La la".
/// - The playlist plays `verse` once, inside a repeat of two passes: pass 2
///   sings verse 2. The song is 16 beats (8 seconds).
pub fn project() -> Project {
    let mut p = Project::empty("Song");
    p.meta.author = "Ann".into();
    p.channels = vec![
        channel("lead", "Lead", "synth"),
        channel("kit", "Kit", "drum"),
    ];
    let verse_notes = [
        (60, 0.0, 1.0),
        (62, 1.0, 1.0),
        (64, 2.0, 1.0),
        (65, 3.0, 0.5),
        (67, 3.5, 0.5),
        (64, 4.0, 1.0),
        (69, 5.0, 1.0),
    ];
    let mut notes: Vec<Note> = verse_notes
        .iter()
        .map(|&(pitch, start, length)| note("lead", pitch, start, length))
        .collect();
    notes.push(note("kit", 36, 0.0, 0.5));
    notes.push(note("kit", 36, 4.0, 0.5));
    p.patterns = vec![
        Pattern {
            uses: vec![Use {
                pattern: "hook".into(),
                start: 6.0,
                from: 0.0,
                to: None,
                transpose: 0,
                channel: String::new(),
                velocity: 1.0,
                verse: None,
            }],
            lyrics: vec![lyrics(&[
                (1, "Hel-lo dark friend[f ɹ ɛ n d] _ / (br) Hi"),
                (2, "Bye now friend _ / (br) Hi"),
            ])],
            ..pattern("verse", "Verse", 8.0, notes)
        },
        Pattern {
            lyrics: vec![lyrics(&[(1, "La la")])],
            ..pattern(
                "hook",
                "Hook",
                2.0,
                vec![note("lead", 67, 0.0, 1.0), note("lead", 65, 1.0, 1.0)],
            )
        },
    ];
    p.playlist.clips = vec![Clip {
        pattern: "verse".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: 8.0,
        offset: 0.0,
        gain: 1.0,
        mixer: Default::default(),
        verse: None,
    }];
    p.repeats = vec![Repeat {
        start: 0.0,
        end: 8.0,
        times: 2,
        endings: vec![],
    }];
    p
}

/// The whole song performed.
pub fn song() -> Song {
    Song::of_project(&project())
}

fn channel(id: &str, name: &str, kind: &str) -> Channel {
    Channel {
        id: id.into(),
        name: name.into(),
        color: "#c9a45c".into(),
        instrument: Device::new(kind),
        volume: 0.8,
        pan: 0.0,
        mute: false,
        mixer: Default::default(),
        arp: None,
    }
}

fn note(channel: &str, pitch: i32, start: f64, length: f64) -> Note {
    Note {
        channel: channel.into(),
        pitch,
        start,
        length,
        velocity: 0.8,
    }
}

fn pattern(id: &str, name: &str, length: f64, notes: Vec<Note>) -> Pattern {
    Pattern {
        id: id.into(),
        name: name.into(),
        color: "#c9a45c".into(),
        length,
        notes,
        uses: vec![],
        lyrics: vec![],
    }
}

fn lyrics(verses: &[(u32, &str)]) -> Lyrics {
    Lyrics {
        channel: "lead".into(),
        lang: String::new(),
        mode: LyricMode::Sing,
        verses: verses.iter().map(|(k, v)| (*k, v.to_string())).collect(),
        timing: vec![],
    }
}
