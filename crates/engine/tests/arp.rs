//! The channel arpeggiator: a held note plays as a run of notes, the
//! pattern keeps the note as written.

use rosaclef_core::{Arpeggio, Channel, Clip, Device, InsertIx, Note, Project, TrackIx};
use rosaclef_engine::render::{render, Audio, RenderScope};
use rosaclef_engine::Engine;

const SR: f32 = 48000.0;

/// Fundamental estimate from the autocorrelation of 8192 samples from `at` seconds.
fn pitch_at(left: &[f32], at: f32) -> f32 {
    let start = (at * SR) as usize;
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

/// One sine channel holding A3 (220 Hz) for a bar at 120 BPM (a beat is 0.5 s).
fn song(arp: Option<Arpeggio>) -> Project {
    let mut p = Project::empty("t");
    p.transport.bpm = 120.0;
    p.mixer.inserts[0].effects.clear();
    let mut synth = Device::new("synth");
    synth.options.insert("wave1".into(), "sine".into());
    synth.params.insert("osc2Mix".into(), 0.0);
    synth.params.insert("sustain".into(), 1.0);
    p.channels.push(Channel {
        id: "x".into(),
        name: "X".into(),
        color: "#ffffff".into(),
        instrument: synth,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx::MASTER,
        arp,
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

fn near(f: f32, hz: f32) -> bool {
    (f - hz).abs() < hz * 0.03
}

#[test]
fn a_held_note_runs_up_its_octaves() {
    // Two octaves, a note per beat: A3, A4, A3, A4.
    let arp = Arpeggio {
        octaves: 2,
        rate: 1.0,
        ..Arpeggio::default()
    };
    let out = play(song(Some(arp)));
    let heard: Vec<f32> = [0.1, 0.6, 1.1, 1.6]
        .iter()
        .map(|t| pitch_at(&out.left, *t))
        .collect();
    println!("beats 1-4: {heard:?} Hz");
    assert!(
        near(heard[0], 220.0)
            && near(heard[1], 440.0)
            && near(heard[2], 220.0)
            && near(heard[3], 440.0),
        "{heard:?}"
    );
}

#[test]
fn without_an_arpeggio_the_note_holds() {
    let out = play(song(None));
    let heard: Vec<f32> = [0.1, 0.6, 1.1]
        .iter()
        .map(|t| pitch_at(&out.left, *t))
        .collect();
    assert!(heard.iter().all(|f| near(*f, 220.0)), "{heard:?}");
}

#[test]
fn a_short_gate_leaves_gaps() {
    // A note per beat held for a tenth of it: silence fills the rest of the beat.
    let arp = Arpeggio {
        rate: 1.0,
        gate: 0.1,
        ..Arpeggio::default()
    };
    let out = play(song(Some(arp)));
    let peak = |a: f32, b: f32| {
        out.left[(a * SR) as usize..(b * SR) as usize]
            .iter()
            .fold(0f32, |m, x| m.max(x.abs()))
    };
    let (on, gap) = (peak(0.0, 0.05), peak(0.3, 0.45));
    println!("note {on:.3}, gap {gap:.4}");
    assert!(on > 0.05 && gap < on * 0.05, "note {on}, gap {gap}");
}
