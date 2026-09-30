//! The metronome, the count-in before recording, and a pattern that plays on
//! past its end while recording into it.

use rosaclef_core::{Channel, Device, InsertIx, Note, Project};
use rosaclef_engine::{Engine, PlayMode};

const SR: f32 = 48000.0;
const BLOCK: usize = 128;

/// Render `seconds` of the engine's left output.
fn run(e: &mut Engine, seconds: f32) -> Vec<f32> {
    let frames = (seconds * SR) as usize;
    let mut out = vec![];
    let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
    while out.len() < frames {
        e.process(&mut l, &mut r);
        out.extend_from_slice(&l);
    }
    out
}

/// Where sound starts after silence (seconds).
fn onsets(left: &[f32]) -> Vec<f32> {
    let mut out = vec![];
    let mut quiet = usize::MAX;
    for (i, v) in left.iter().enumerate() {
        if v.abs() > 0.01 {
            if quiet > (SR * 0.1) as usize {
                out.push(i as f32 / SR);
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    out
}

/// An empty pattern (4 beats at 120 BPM: a beat every half second), silent master.
fn silent() -> Engine {
    let mut p = Project::empty("t");
    p.mixer.inserts[0].effects.clear();
    p.patterns[0].notes.clear();
    p.patterns[0].length = 4.0;
    let mut e = Engine::new(SR);
    e.set_project(p);
    e.set_mode(PlayMode::Pattern("pattern-1".into()));
    e
}

#[test]
fn metronome_clicks_every_beat() {
    let mut e = silent();
    e.play();
    assert!(onsets(&run(&mut e, 1.0)).is_empty(), "off by default");
    e.stop();
    e.set_metronome(true);
    e.play();
    let on = onsets(&run(&mut e, 2.2));
    assert_eq!(on.len(), 5, "{on:?}");
    for (i, t) in on.iter().enumerate() {
        assert!((t - 0.5 * i as f32).abs() < 0.005, "{on:?}");
    }
}

#[test]
fn count_in_clicks_a_bar_before_the_pattern_starts() {
    let mut e = silent();
    e.play_count_in(4.0);
    assert!(e.is_playing());
    assert!((e.position() + 4.0).abs() < 1e-9);
    let pre = run(&mut e, 1.0);
    assert!(
        (e.position() + 2.0).abs() < 0.01,
        "two beats left, reads {}",
        e.position()
    );
    let on = onsets(&pre);
    assert_eq!(on.len(), 2, "{on:?}");
    // The first count-in beat is the downbeat: louder.
    let peak = |at: f32| {
        let i = (at * SR) as usize;
        pre[i..i + 2400].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    };
    assert!(peak(0.0) > peak(0.5) * 1.2);
    // Without the metronome, nothing clicks once the pattern starts.
    run(&mut e, 1.0);
    assert!(
        e.position().abs() < 0.01,
        "starts at 0, reads {}",
        e.position()
    );
    assert!(onsets(&run(&mut e, 1.0)).is_empty());
    // Stopping during a count-in drops it.
    e.stop();
    e.play_count_in(4.0);
    run(&mut e, 0.3);
    e.stop();
    assert_eq!(e.position(), 0.0);
}

/// A 4-beat pattern with a short sine blip on its first beat.
fn blip() -> Engine {
    let mut p = Project::empty("t");
    p.mixer.inserts[0].effects.clear();
    let mut dev = Device::new("synth");
    for (k, v) in [
        ("attack", 0.001),
        ("decay", 0.03),
        ("sustain", 0.0),
        ("release", 0.005),
        ("osc2Mix", 0.0),
        ("cutoff", 20000.0),
    ] {
        dev.params.insert(k.into(), v);
    }
    dev.options.insert("wave1".into(), "sine".into());
    p.channels.push(Channel {
        id: "b".into(),
        name: "Blip".into(),
        color: "#ffffff".into(),
        instrument: dev,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx(1),
    });
    p.patterns[0].notes.clear();
    p.patterns[0].length = 4.0;
    p.patterns[0].notes.push(Note {
        channel: "b".into(),
        pitch: 69,
        start: 0.0,
        length: 0.1,
        velocity: 1.0,
    });
    let mut e = Engine::new(SR);
    e.set_project(p);
    e.set_mode(PlayMode::Pattern("pattern-1".into()));
    e
}

#[test]
fn an_open_ended_pattern_plays_on_past_its_end() {
    let mut e = blip();
    e.play();
    assert_eq!(e.loop_length(), 4.0);
    // Looping: the blip on beat 1 comes round every 2 s.
    assert_eq!(onsets(&run(&mut e, 4.5)).len(), 3);
    e.stop();
    e.set_open_ended(true);
    assert_eq!(e.loop_length(), 0.0, "nothing loops");
    e.play();
    let on = onsets(&run(&mut e, 4.5));
    assert_eq!(on.len(), 1, "the pattern plays once: {on:?}");
    assert!(
        e.position() > 8.5,
        "and the playhead goes on: {}",
        e.position()
    );
    // Back to looping: the playhead wraps.
    e.set_open_ended(false);
    run(&mut e, 0.1);
    assert!(e.position() < 4.0, "wraps: {}", e.position());
}
