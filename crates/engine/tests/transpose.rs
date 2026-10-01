//! The master transpose: pitched instruments sound shifted (sequenced and
//! live notes), drums do not.

use rosaclef_core::{Channel, Clip, Device, InsertIx, Note, Project, TrackIx};
use rosaclef_engine::render::{render, Audio, RenderScope};
use rosaclef_engine::Engine;

const SR: f32 = 48000.0;

/// Fundamental estimate from the autocorrelation of a steady segment.
fn pitch_of(left: &[f32]) -> f32 {
    let start = (0.25 * SR) as usize;
    let x = &left[start..start + 8192];
    let w = 4096;
    let r = |lag: usize| -> f32 {
        let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
        for i in 0..w {
            xy += x[i] * x[i + lag];
            xx += x[i] * x[i];
            yy += x[i + lag] * x[i + lag];
        }
        xy / (xx * yy).sqrt().max(1e-12)
    };
    let vals: Vec<f32> = (0..(SR / 40.0) as usize).map(r).collect();
    let mut i = vals.iter().position(|v| *v < 0.0).unwrap();
    let best = vals[i..].iter().cloned().fold(f32::MIN, f32::max);
    i += vals[i..].iter().position(|v| *v >= 0.85 * best).unwrap();
    while i + 1 < vals.len() && vals[i + 1] > vals[i] {
        i += 1;
    }
    SR / i as f32
}

/// One channel playing one long note (MIDI 57, A3 = 220 Hz) for a bar.
fn song(kind: &str, transpose: i32) -> Project {
    let mut p = Project::empty("t");
    p.transport.transpose = transpose;
    p.mixer.inserts[0].effects.clear();
    p.channels.push(Channel {
        id: "x".into(),
        name: "X".into(),
        color: "#ffffff".into(),
        instrument: Device::new(kind),
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx::MASTER,
        arp: None,
    });
    let pat = &mut p.patterns[0];
    pat.length = 4.0;
    pat.notes = vec![Note {
        channel: "x".into(),
        pitch: 57,
        start: 0.0,
        length: 4.0,
        velocity: 0.9,
    }];
    p.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: 4.0,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx(0),
        verse: None,
    });
    p
}

fn play(p: Project) -> Audio {
    let mut e = Engine::new(SR);
    e.set_project(p);
    render(&mut e, &RenderScope::Song)
}

fn semitones(a: f32, b: f32) -> f32 {
    12.0 * (b / a).log2()
}

#[test]
fn the_song_sounds_transposed() {
    let f0 = pitch_of(&play(song("synth", 0)).left);
    let up = pitch_of(&play(song("synth", 3)).left);
    let down = pitch_of(&play(song("synth", -5)).left);
    println!("f0 {f0:.1} Hz, +3 {up:.1} Hz, -5 {down:.1} Hz");
    assert!((f0 - 220.0).abs() < 220.0 * 0.03, "f0 = {f0}");
    assert!((semitones(f0, up) - 3.0).abs() < 0.2, "+3 → {up}");
    assert!((semitones(f0, down) + 5.0).abs() < 0.2, "-5 → {down}");
}

#[test]
fn drums_are_not_transposed() {
    let a = play(song("drum", 0));
    let b = play(song("drum", 7));
    let same = a
        .left
        .iter()
        .zip(&b.left)
        .all(|(x, y)| (x - y).abs() < 1e-6);
    assert!(
        same,
        "the drum machine plays the same with or without a transpose"
    );
}

#[test]
fn live_notes_are_transposed_and_released() {
    let mut p = song("synth", -12);
    p.playlist.clips.clear();
    let mut e = Engine::new(SR);
    e.set_project(p);
    e.note_on("x", 69, 0.9);
    let mut left = vec![];
    let mut bl = [0f32; 128];
    let mut br = [0f32; 128];
    for _ in 0..(SR as usize / 128) {
        e.process(&mut bl, &mut br);
        left.extend_from_slice(&bl);
    }
    let f = pitch_of(&left);
    println!("A4 live, an octave down: {f:.1} Hz");
    assert!((f - 220.0).abs() < 220.0 * 0.03, "{f}");
    // The transpose changes while the key is down: the key still releases its note.
    let mut q = song("synth", 2);
    q.playlist.clips.clear();
    e.set_project(q);
    e.note_off("x", 69);
    let mut tail = 0f32;
    for i in 0..(4 * SR as usize / 128) {
        e.process(&mut bl, &mut br);
        if i > 3 * SR as usize / 128 {
            tail = bl.iter().fold(tail, |m, x| m.max(x.abs()));
        }
    }
    assert!(tail < 1e-3, "the note was released ({tail})");
}
