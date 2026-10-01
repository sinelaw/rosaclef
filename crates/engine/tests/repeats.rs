//! Repeats: the song plays a passage again, and endings (voltas) only on
//! their passes — while playing and in renders.

use rosaclef_core::{Channel, Clip, Device, Ending, InsertIx, Note, Project, Repeat, TrackIx};
use rosaclef_engine::render::{render, RenderScope};
use rosaclef_engine::Engine;

const SR: f32 = 48000.0;

/// Where sound starts after silence (seconds).
fn onsets(left: &[f32]) -> Vec<f32> {
    let mut out = vec![];
    let mut quiet = usize::MAX;
    for (i, v) in left.iter().enumerate() {
        if v.abs() > 0.01 {
            if quiet > (SR * 0.2) as usize {
                out.push(i as f32 / SR);
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    out
}

/// Four bars at 120 BPM (a bar every 2 s): a hit on the downbeat of bars 1, 3
/// and 4 (beats 0, 8 and 12), and nothing else.
fn song() -> Project {
    let mut p = Project::empty("t");
    p.mixer.inserts[0].effects.clear();
    let mut dev = Device::new("drum");
    dev.options.insert("kind".into(), "rim".into());
    p.channels.push(Channel {
        id: "d".into(),
        name: "Drum".into(),
        color: "#ffffff".into(),
        instrument: dev,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx(1),
        arp: None,
    });
    let pat = &mut p.patterns[0];
    pat.length = 16.0;
    pat.notes.clear();
    for start in [0.0, 8.0, 12.0] {
        pat.notes.push(Note {
            channel: "d".into(),
            pitch: 60,
            start,
            length: 0.25,
            velocity: 1.0,
        });
    }
    p.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: 16.0,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx(0),
        verse: None,
    });
    p
}

fn near(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02)
}

#[test]
fn a_song_without_repeats_plays_through() {
    let mut e = Engine::new(SR);
    e.set_project(song());
    let a = render(&mut e, &RenderScope::Song);
    let on = onsets(&a.left);
    assert!(near(&on, &[0.0, 4.0, 6.0]), "{on:?}");
}

#[test]
fn a_repeat_with_first_and_second_endings() {
    // |: bar 1, bar 2, [1. bar 3 :| [2. bar 4
    let mut p = song();
    p.repeats.push(Repeat {
        start: 0.0,
        end: 12.0,
        times: 2,
        endings: vec![
            Ending {
                start: 8.0,
                end: 12.0,
                passes: vec![1],
            },
            Ending {
                start: 12.0,
                end: 16.0,
                passes: vec![2],
            },
        ],
    });
    let mut e = Engine::new(SR);
    e.set_project(p);
    let a = render(&mut e, &RenderScope::Song);
    // Bars 1 2 3 | 1 2 4: hits at 0 s, 4 s (1st ending), 6 s (again), 10 s (2nd ending).
    let on = onsets(&a.left);
    assert!(near(&on, &[0.0, 4.0, 6.0, 10.0]), "{on:?}");
    // Six bars play: 12 s of music, then the tail.
    assert!(
        a.duration() >= 12.0 && a.duration() < 12.0 + 8.5,
        "{}",
        a.duration()
    );
}

#[test]
fn a_passage_played_three_times() {
    let mut p = song();
    p.repeats.push(Repeat {
        start: 0.0,
        end: 4.0,
        times: 3,
        endings: vec![],
    });
    let mut e = Engine::new(SR);
    e.set_project(p);
    let a = render(&mut e, &RenderScope::Song);
    // Bar 1 three times (0, 2, 4 s), then bars 2–4 from 6 s: hits at 8 s and 10 s.
    let on = onsets(&a.left);
    assert!(near(&on, &[0.0, 2.0, 4.0, 8.0, 10.0]), "{on:?}");
}
