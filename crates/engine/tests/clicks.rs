//! Click regressions: discontinuities found in the demo's stems.
//!
//! - Cuivre (the demo bass) stepped its output when a sounding mono voice was
//!   retriggered at a new velocity: the velocity gain changed instantly,
//!   mid-waveform.
//! - Atelier drum hits started at full level on their first sample (and each
//!   clap burst re-attacked in one sample), and voices stopped dead at the
//!   end of their length.

use rosaclef_core::{presets, Device};
use rosaclef_engine::instruments::{create, Instrument, NoteEvent, NoteKind};
use rosaclef_engine::Ctx;

const SR: f32 = 48000.0;
const BLOCK: usize = 128;

fn secs(t: f32) -> usize {
    (t * SR).round() as usize
}

/// Render `total` seconds of an instrument (left channel); `events` are (seconds, event).
fn play(dev: &Device, events: &[(f32, NoteKind)], total: f32) -> Vec<f32> {
    let mut inst: Box<dyn Instrument> = create(dev, &Ctx { sr: SR, bpm: 120.0 }).expect("known instrument");
    let mut evs: Vec<(usize, NoteKind)> = events.iter().map(|(t, e)| (secs(*t), *e)).collect();
    evs.sort_by_key(|e| e.0);
    let frames = secs(total);
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    let (mut pos, mut next) = (0, 0);
    while pos < frames {
        let n = (frames - pos).min(BLOCK);
        let mut block = Vec::new();
        while next < evs.len() && evs[next].0 < pos + n {
            block.push(NoteEvent { offset: evs[next].0.saturating_sub(pos), kind: evs[next].1 });
            next += 1;
        }
        inst.process(&block, &mut l[pos..pos + n], &mut r[pos..pos + n]);
        pos += n;
    }
    l
}

fn on(key: u8, velocity: f32) -> NoteKind {
    NoteKind::On { key, velocity }
}

/// |second difference|: large where the waveform breaks (a step or a kink).
fn d2(x: &[f32]) -> Vec<f32> {
    x.windows(3).map(|w| (w[2] - 2.0 * w[1] + w[0]).abs()).collect()
}

fn quantile(mut v: Vec<f32>, q: f32) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f32 * q) as usize]
}

#[test]
fn cuivre_mono_retrigger_at_a_new_velocity_is_smooth() {
    let dev = presets::find("Bronze Souverain").expect("preset").device();
    // Loud and soft notes alternating, each starting while the last still sounds
    // (the demo's bass line: roots, octave ghosts, approach notes).
    let mut events = vec![];
    let mut starts = vec![];
    let keys = [41u8, 53, 41, 48, 39, 51, 36, 43];
    for (i, &k) in keys.iter().enumerate() {
        let t = 0.05 + i as f32 * 0.19;
        let vel = if i % 2 == 0 { 1.0 } else { 0.3 };
        events.push((t, on(k, vel)));
        events.push((t + 0.17, NoteKind::Off { key: k }));
        starts.push(t);
    }
    let x = play(&dev, &events, 1.8);
    let d = d2(&x);
    let typical = quantile(d.clone(), 0.999);
    for (i, &t) in starts.iter().enumerate().skip(1) {
        let (a, b) = (secs(t) - 24, secs(t) + 72);
        let worst = d[a..b].iter().fold(0.0f32, |m, v| m.max(*v));
        // Measured: 1.9-4.6x typical with the gain glide, 34x without it.
        assert!(worst < 10.0 * typical, "retrigger {i} at {t}s: second difference {worst} vs typical {typical}");
    }
}

#[test]
fn drum_hits_start_and_end_without_a_step() {
    for kind in ["kick", "snare", "clap", "rim", "tom", "hat", "openhat", "shaker", "cowbell"] {
        let mut dev = Device::new("drum");
        dev.options.insert("kind".into(), kind.into());
        let x = play(&dev, &[(0.01, on(60, 1.0))], 3.0);
        let start = secs(0.01);
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.05, "{kind}: silent ({peak})");
        // Onset: the first 0.1 ms is well below the level just after it (0.4-0.8 ms);
        // a hit that starts at full level on its first sample scores about 1.
        let max_in = |from: usize, to: usize| x[start + from..start + to].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let ratio = max_in(0, 5) / max_in(20, 40).max(1e-6);
        // Measured: 0.01-0.23 with the onset ramp, 0.37-0.88 without it.
        assert!(ratio < 0.3, "{kind}: starts at {ratio} of its level 0.5 ms later");
        // End: the last sample before the voice stops is (nearly) silent.
        let last = (0..x.len()).rev().find(|&i| x[i] != 0.0).unwrap();
        assert!(x[last].abs() < 1e-3 * peak.max(1.0), "{kind}: stops at {} (peak {peak})", x[last]);
    }
}
