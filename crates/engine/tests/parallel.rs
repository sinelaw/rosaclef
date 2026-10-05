//! A parallel render (the `parallel` feature) is the same audio, bit for bit,
//! as one on a single thread.
#![cfg(feature = "parallel")]

use rosaclef_core::{
    catalog, presets, validate, AutomationLane, AutomationPoint, Channel, Clip, Device, InsertIx,
    Note, Project, TrackIx,
};
use rosaclef_engine::render::{render, Audio, RenderScope};
use rosaclef_engine::Engine;

const SR: f32 = 48000.0;

/// Every instrument type that needs no files (two presets each), and every
/// effect, on inserts shared by several channels, with some channels routed
/// straight to the master; tempo and volume automation.
fn project() -> Project {
    let mut p = Project::empty("parallel");
    p.transport.bpm = 132.0;
    p.patterns[0].length = 8.0;
    let mut kinds: Vec<&str> = vec![];
    for (i, preset) in presets::all()
        .into_iter()
        .filter(|pr| !["soundfont", "sampler", "plugin"].contains(&pr.kind))
        .filter(|pr| {
            let n = kinds.iter().filter(|k| **k == pr.kind).count();
            kinds.push(pr.kind);
            n < 2
        })
        .enumerate()
    {
        let id = format!("c{i}");
        p.channels.push(Channel {
            id: id.clone(),
            name: preset.name.into(),
            color: "#ffffff".into(),
            instrument: preset.device(),
            volume: 0.5,
            pan: (i as f64 * 0.37).sin(),
            mute: false,
            mixer: InsertIx((i % 4) as u32),
            arp: None,
            layer_of: None,
        });
        for k in 0..6 {
            p.patterns[0].notes.push(Note {
                channel: id.clone(),
                pitch: 48 + ((i * 5 + k * 7) % 24) as i32,
                start: k as f64 * 1.25 + (i % 3) as f64 * 0.25,
                length: 0.5 + (k % 3) as f64 * 0.75,
                velocity: 0.6 + 0.05 * k as f64,
            });
        }
    }
    assert!(p.channels.len() >= 12, "{} channels", p.channels.len());
    for (j, fx) in catalog::effects()
        .filter(|d| d.kind != "plugin")
        .enumerate()
    {
        p.mixer.inserts[1 + j % 3]
            .effects
            .push(Device::new(fx.kind));
    }
    p.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: 16.0,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx::MASTER,
    });
    let lane = |id: &str, target: &str, points: &[(f64, f64)]| AutomationLane {
        id: id.into(),
        name: id.into(),
        target: target.into(),
        color: "#8a6bb0".into(),
        mute: false,
        points: points
            .iter()
            .map(|&(beat, value)| AutomationPoint {
                beat,
                value,
                curve: 0.0,
            })
            .collect(),
    };
    p.automation
        .push(lane("t", "tempo", &[(0.0, 132.0), (16.0, 100.0)]));
    p.automation.push(lane(
        "v",
        "insert/2/volume",
        &[(0.0, 1.0), (8.0, 0.2), (16.0, 0.9)],
    ));
    let checked = validate::validate(&p);
    assert!(
        !checked
            .iter()
            .any(|i| i.severity == validate::Severity::Error),
        "{checked:?}"
    );
    p
}

fn render_with(parallel: bool) -> Audio {
    let mut e = Engine::new(SR);
    e.set_parallel(parallel);
    e.set_project(project());
    render(&mut e, &RenderScope::Song)
}

#[test]
fn parallel_render_is_bit_identical() {
    let one = render_with(false);
    assert!(one.peak() > 0.01, "the test song is silent");
    for _ in 0..3 {
        let many = render_with(true);
        assert_eq!(one.left.len(), many.left.len());
        let same = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
        assert!(same(&one.left, &many.left) && same(&one.right, &many.right));
    }
}
