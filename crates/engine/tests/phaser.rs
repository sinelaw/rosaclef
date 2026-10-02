//! The phaser: swept all-pass notches, and no delay (fast notes stay crisp).

use rosaclef_core::Device;
use rosaclef_engine::effects;
use rosaclef_engine::Ctx;

const SR: f32 = 48000.0;

fn phaser(params: &[(&str, f64)]) -> Box<dyn effects::Effect> {
    let mut d = Device::new("phaser");
    for (k, v) in params {
        d.params.insert(k.to_string(), *v);
    }
    effects::create(&d, &Ctx { sr: SR, bpm: 120.0 }).expect("phaser exists")
}

fn run(fx: &mut dyn effects::Effect, input: &[f32]) -> Vec<f32> {
    let mut l = input.to_vec();
    let mut r = input.to_vec();
    for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
        fx.process(a, b);
    }
    l
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn a_dry_mix_passes_the_sound_unchanged() {
    let tone: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin()).collect();
    let out = run(&mut *phaser(&[("mix", 0.0)]), &tone);
    assert!(out.iter().zip(&tone).all(|(a, b)| (a - b).abs() < 1e-6));
}

#[test]
fn notches_cut_a_tone_at_the_sweep_frequency() {
    // A static sweep (no depth) at 1 kHz: a tone the stages turn by 180°
    // cancels against the dry signal; a tone far below passes.
    let tone = |hz: f32| -> Vec<f32> {
        (0..(SR as usize / 2))
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin())
            .collect()
    };
    let settings = [
        ("depth", 0.0),
        ("freq", 1000.0),
        ("stages", 2.0),
        ("mix", 1.0),
    ];
    // Two stages: 180° at the break frequency, where dry and wet cancel.
    let at = run(&mut *phaser(&settings), &tone(1000.0));
    let away = run(&mut *phaser(&settings), &tone(100.0));
    let (at, away) = (rms(&at[4800..]), rms(&away[4800..]));
    println!("1 kHz {at:.4}, 100 Hz {away:.4}");
    assert!(at < 0.05 && away > 0.5, "notch {at}, pass {away}");
}

#[test]
fn an_impulse_comes_out_at_once() {
    // A chorus answers late (its delay line); a phaser's energy stays at the front.
    let mut imp = vec![0.0f32; 4800];
    imp[0] = 1.0;
    let out = run(&mut *phaser(&[("mix", 1.0), ("stages", 6.0)]), &imp);
    let early: f32 = out[..96].iter().map(|v| v * v).sum();
    let all: f32 = out.iter().map(|v| v * v).sum();
    println!("energy in the first 2 ms: {:.1}%", 100.0 * early / all);
    assert!(early / all > 0.9, "{early} of {all}");
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn feedback_stays_stable() {
    let noise: Vec<f32> = (0..48000)
        .map(|i| (((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / 16777216.0) - 0.5)
        .collect();
    let out = run(
        &mut *phaser(&[("feedback", 0.9), ("rate", 5.0), ("depth", 1.0)]),
        &noise,
    );
    assert!(out.iter().all(|v| v.is_finite() && v.abs() < 20.0));
}
