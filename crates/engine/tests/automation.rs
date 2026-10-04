//! Automation playback: tempo ramps, parameter sweeps, mix lanes, restoring
//! the project values, offline renders.

use rosaclef_core::{
    validate, AutomationLane, AutomationPoint, Channel, Clip, Device, InsertIx, Note, Project,
    TrackIx,
};
use rosaclef_engine::render::{render, RenderScope};
use rosaclef_engine::{Engine, PlayMode};

const SR: f32 = 48000.0;

fn lane(id: &str, target: &str, points: &[(f64, f64, f64)]) -> AutomationLane {
    AutomationLane {
        id: id.into(),
        name: id.into(),
        target: target.into(),
        color: "#8a6bb0".into(),
        mute: false,
        points: points
            .iter()
            .map(|&(beat, value, curve)| AutomationPoint { beat, value, curve })
            .collect(),
    }
}

/// A short sine blip every beat for `beats` beats, on insert 1, no master effects.
fn blips(beats: f64) -> Project {
    let mut p = Project::empty("t");
    p.mixer.inserts[0].effects.clear();
    let mut dev = Device::new("analog");
    for (k, v) in [
        ("attack", 0.001),
        ("decay", 0.03),
        ("sustain", 0.0),
        ("release", 0.005),
        ("osc2Mix", 0.0),
        ("sub", 0.0),
        ("drift", 0.0),
        ("drive", 0.0),
        ("cutoff", 20000.0),
    ] {
        dev.params.insert(k.into(), v);
    }
    dev.options.insert("wave1".into(), "sine".into());
    dev.options.insert("filter".into(), "lowpass".into());
    p.channels.push(Channel {
        id: "b".into(),
        name: "Blip".into(),
        color: "#ffffff".into(),
        instrument: dev,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx(1),
        arp: None,
        layer_of: None,
    });
    p.patterns[0].length = 1.0;
    p.patterns[0].notes.push(Note {
        channel: "b".into(),
        pitch: 69,
        start: 0.0,
        length: 0.1,
        velocity: 1.0,
    });
    p.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: beats,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx::MASTER,
    });
    p
}

/// A sustained saw pad over `beats` beats.
fn pad(beats: f64) -> Project {
    let mut p = Project::empty("t");
    p.mixer.inserts[0].effects.clear();
    let mut dev = Device::new("analog");
    for (k, v) in [
        ("filterEnv", 0.0),
        ("sustain", 1.0),
        ("resonance", 0.0),
        ("drift", 0.0),
        ("drive", 0.0),
        ("osc2Mix", 0.0),
        ("sub", 0.0),
        ("cutoff", 2400.0),
    ] {
        dev.params.insert(k.into(), v);
    }
    p.channels.push(Channel {
        id: "pad".into(),
        name: "Pad".into(),
        color: "#ffffff".into(),
        instrument: dev,
        volume: 0.8,
        pan: 0.0,
        mute: false,
        mixer: InsertIx(1),
        arp: None,
        layer_of: None,
    });
    p.patterns[0].length = beats;
    p.patterns[0].notes.push(Note {
        channel: "pad".into(),
        pitch: 48,
        start: 0.0,
        length: beats,
        velocity: 0.9,
    });
    p.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: beats,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx::MASTER,
    });
    p
}

fn valid(p: Project) -> Project {
    let issues: Vec<_> = validate::validate(&p)
        .into_iter()
        .filter(|i| i.severity == validate::Severity::Error)
        .collect();
    assert!(issues.is_empty(), "{issues:?}");
    p
}

/// Onset times (seconds): first sample above 0.05 after at least 10 ms of silence.
fn onsets(x: &[f32]) -> Vec<f64> {
    let mut out = vec![];
    let mut quiet = usize::MAX / 2;
    for (i, v) in x.iter().enumerate() {
        if v.abs() > 0.05 {
            if quiet > (0.01 * SR) as usize {
                out.push(i as f64 / SR as f64);
            }
            quiet = 0;
        } else if v.abs() < 0.001 {
            quiet += 1;
        }
    }
    out
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// High-frequency content: energy of the first difference relative to the signal.
fn brightness(x: &[f32]) -> f64 {
    let d: f64 = x.windows(2).map(|w| ((w[1] - w[0]) as f64).powi(2)).sum();
    let e: f64 = x.iter().map(|v| (*v as f64).powi(2)).sum();
    d / e.max(1e-12)
}

fn run(e: &mut Engine, seconds: f64) -> (Vec<f32>, Vec<f32>) {
    let n = (seconds * SR as f64) as usize;
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    e.process(&mut l, &mut r);
    (l, r)
}

#[test]
fn tempo_ramp_moves_note_onsets() {
    // 120 → 240 BPM over the first 8 beats, then 240 BPM: bpm(b) = 120 + 15 b,
    // so beat b sounds at t(b) = ∫ 60 / bpm = 4 ln(1 + b / 8) seconds.
    let mut p = blips(16.0);
    p.automation.push(lane(
        "tempo",
        "tempo",
        &[(0.0, 120.0, 0.0), (8.0, 240.0, 0.0)],
    ));
    let mut e = Engine::new(SR);
    e.set_project(valid(p));
    let a = render(&mut e, &RenderScope::Song);
    let hits = onsets(&a.left);
    assert_eq!(hits.len(), 16, "{hits:?}");
    let expected = |b: f64| {
        if b <= 8.0 {
            4.0 * (1.0 + b / 8.0).ln()
        } else {
            4.0 * 2f64.ln() + (b - 8.0) * 0.25
        }
    };
    for (b, t) in hits.iter().enumerate() {
        let want = expected(b as f64) + hits[0];
        assert!(
            (t - want).abs() < 0.002,
            "beat {b}: onset at {t:.4}s, expected {want:.4}s"
        );
    }
    // The song is shorter than at a constant 120 BPM (8 s).
    let song = 4.0 * 2f64.ln() + 2.0;
    assert!(
        a.duration() > song - 0.001 && a.duration() < song + 0.5,
        "duration {}",
        a.duration()
    );
    assert!((e.song_seconds(16.0) - song).abs() < 1e-3);
}

#[test]
fn tempo_automation_applies_in_song_mode_only() {
    let mut p = blips(16.0);
    p.automation
        .push(lane("tempo", "tempo", &[(0.0, 150.0, 0.0)]));
    let mut e = Engine::new(SR);
    e.set_project(valid(p));
    e.set_mode(PlayMode::Song);
    e.play();
    run(&mut e, 0.1);
    assert!((e.bpm() - 150.0).abs() < 1e-3);
    // One second at 150 BPM = 2.5 beats (plus the 0.1 s above).
    run(&mut e, 1.0);
    assert!(
        (e.position() - 2.75).abs() < 0.01,
        "position {}",
        e.position()
    );
    e.set_mode(PlayMode::Pattern("pattern-1".into()));
    assert!(
        (e.bpm() - 120.0).abs() < 1e-6,
        "pattern mode uses the project tempo"
    );
    e.play();
    run(&mut e, 0.1);
    assert!((e.bpm() - 120.0).abs() < 1e-6);
}

#[test]
fn cutoff_lane_sweeps_brightness() {
    let beats = 16.0;
    let mut p = pad(beats);
    p.automation.push(lane(
        "pad-cutoff",
        "channel/pad/cutoff",
        &[(0.0, 150.0, 0.0), (16.0, 9000.0, 0.4)],
    ));
    let mut e = Engine::new(SR);
    e.set_project(valid(p.clone()));
    let a = render(&mut e, &RenderScope::Song);
    let sec = SR as usize;
    let early = brightness(&a.left[sec / 2..sec * 3 / 2]);
    let late = brightness(&a.left[6 * sec..7 * sec]);
    assert!(
        late > early * 4.0,
        "brightness early {early:.5}, late {late:.5}"
    );

    // Without the lane the pad is equally bright all along.
    p.automation.clear();
    e.set_project(p);
    let b = render(&mut e, &RenderScope::Song);
    let (x, y) = (
        brightness(&b.left[sec / 2..sec * 3 / 2]),
        brightness(&b.left[6 * sec..7 * sec]),
    );
    assert!((x / y - 1.0).abs() < 0.25, "unautomated {x:.5} vs {y:.5}");
}

#[test]
fn insert_volume_lane_changes_level() {
    // Full level for 8 beats, then a step down to a quarter.
    let mut p = blips(16.0);
    p.automation.push(lane(
        "fade",
        "insert/1/volume",
        &[(0.0, 1.0, 0.0), (8.0, 1.0, 0.0), (8.0, 0.25, 0.0)],
    ));
    let mut e = Engine::new(SR);
    e.set_project(valid(p));
    let a = render(&mut e, &RenderScope::Song);
    // Beats 0..8 against 9..16 (the blip on beat 8 straddles the step: lanes
    // are evaluated every 64 frames).
    let beat = SR as usize / 2;
    let ratio = rms(&a.left[9 * beat..16 * beat]) / rms(&a.left[..8 * beat]);
    assert!((ratio - 0.25).abs() < 0.01, "level ratio {ratio}");
}

#[test]
fn channel_pan_lane_moves_the_image() {
    let mut p = blips(8.0);
    p.automation
        .push(lane("pan", "channel/b/pan", &[(0.0, -1.0, 0.0)]));
    let mut e = Engine::new(SR);
    e.set_project(valid(p));
    let a = render(&mut e, &RenderScope::Song);
    // After the first block's gain ramp from the centre, the right side is silent.
    let (l, r) = (&a.left[4800..], &a.right[4800..]);
    assert!(
        rms(l) > 0.01 && rms(r) < 1e-6,
        "hard left: L {} R {}",
        rms(l),
        rms(r)
    );
}

#[test]
fn values_are_restored_on_stop_mode_change_and_lane_removal() {
    // A lane silencing the channel; live notes show whether it is applied.
    let mut p = blips(8.0);
    p.patterns[0].notes.clear();
    p.automation
        .push(lane("mute", "channel/b/volume", &[(0.0, 0.0, 0.0)]));
    p.automation
        .push(lane("tempo", "tempo", &[(0.0, 200.0, 0.0)]));
    let p = valid(p);
    let mut e = Engine::new(SR);
    e.set_project(p.clone());
    let live_level = |e: &mut Engine| {
        e.note_on("b", 69, 1.0);
        let (l, _) = run(e, 0.02);
        e.note_off("b", 69);
        run(e, 0.1);
        rms(&l)
    };
    assert!(live_level(&mut e) > 0.05, "stopped: project volume");
    e.set_mode(PlayMode::Song);
    e.play();
    run(&mut e, 0.05);
    assert!(
        live_level(&mut e) < 1e-6,
        "playing the song: automated to silence"
    );
    assert!((e.bpm() - 200.0).abs() < 1e-3);

    e.stop();
    assert!((e.bpm() - 120.0).abs() < 1e-6);
    assert!(live_level(&mut e) > 0.05, "restored on stop");

    e.play();
    run(&mut e, 0.05);
    assert!(live_level(&mut e) < 1e-6);
    e.pause();
    assert!(live_level(&mut e) > 0.05, "restored on pause");

    e.play();
    run(&mut e, 0.05);
    e.set_mode(PlayMode::Pattern("pattern-1".into()));
    e.play();
    assert!(live_level(&mut e) > 0.05, "restored when leaving song mode");

    e.set_mode(PlayMode::Song);
    e.play();
    run(&mut e, 0.05);
    assert!(live_level(&mut e) < 1e-6);
    let mut without = p.clone();
    without.automation.clear();
    e.set_project(without);
    assert!((e.bpm() - 120.0).abs() < 1e-6);
    assert!(
        live_level(&mut e) > 0.05,
        "restored when the lane is removed"
    );

    // A muted lane is ignored.
    let mut muted = p;
    muted.automation[0].mute = true;
    e.set_project(muted);
    run(&mut e, 0.05);
    assert!(live_level(&mut e) > 0.05, "muted lane");
}

#[test]
fn render_includes_automation_and_holds_it_in_the_tail() {
    // A fade to silence over the song: the end of the render is quiet, and
    // the tail does not jump back to the project volume.
    let mut p = pad(8.0);
    p.automation.push(lane(
        "fade",
        "insert/0/volume",
        &[(0.0, 1.0, 0.0), (8.0, 0.0, 0.0)],
    ));
    let mut e = Engine::new(SR);
    e.set_project(valid(p));
    let a = render(&mut e, &RenderScope::Song);
    let sec = SR as usize;
    let start = rms(&a.left[..sec / 2]);
    let end = rms(&a.left[(3.8 * SR) as usize..4 * sec]);
    assert!(
        start > 0.05 && end < start * 0.1,
        "start {start}, end {end}"
    );
    let tail = rms(&a.left[4 * sec..]);
    assert!(tail < start * 0.05, "tail {tail}");
    // Values are restored once the render is over.
    assert!((e.bpm() - 120.0).abs() < 1e-6);
}

#[test]
fn effect_parameter_and_tempo_synced_delay() {
    let mut p = blips(8.0);
    let mut delay = Device::new("delay");
    delay.params.insert("mix".into(), 0.5);
    p.mixer.inserts[1].effects.push(delay);
    p.mixer.inserts[1].effects.push(Device::new("filter"));
    p.automation.push(lane(
        "sweep",
        "insert/1/effect/1/cutoff",
        &[(0.0, 200.0, 0.0), (8.0, 12000.0, 0.0)],
    ));
    p.automation.push(lane(
        "tempo",
        "tempo",
        &[(0.0, 90.0, 0.0), (8.0, 180.0, -0.3)],
    ));
    let mut e = Engine::new(SR);
    e.set_project(valid(p));
    let a = render(&mut e, &RenderScope::Song);
    assert!(a.left.iter().all(|x| x.is_finite()));
    assert!(a.peak() > 0.05);
}
