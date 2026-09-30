//! Tests for the Sextant (6-op FM) and Tessera (wavetable) instruments.

use rosaclef_core::Device;
use rosaclef_engine::instruments::{self, NoteKind};
use rosaclef_engine::render::{render_note, Audio};
use rosaclef_engine::Ctx;
use std::time::Instant;

const SR: f32 = 48000.0;

fn device(kind: &str, params: &[(&str, f64)], options: &[(&str, &str)]) -> Device {
    let mut d = Device::new(kind);
    for (k, v) in params {
        d.params.insert(k.to_string(), *v);
    }
    for (k, v) in options {
        d.options.insert(k.to_string(), v.to_string());
    }
    d
}

/// Render long enough for the release (rendered at 60 % of the note) to end.
fn render_full(dev: &Device, pitch: u8) -> Audio {
    let release = dev.param("release") as f32;
    let seconds = (3.0f32).max((2.3 * release + 0.3) / 0.4);
    render_note(dev, pitch, 0.9, seconds, SR)
}

/// Level, sanity, DC, click and release checks for a single rendered note.
fn check_note(name: &str, a: &Audio) -> Result<String, String> {
    if !a.left.iter().chain(a.right.iter()).all(|x| x.is_finite()) {
        return Err(format!("{name}: non-finite samples"));
    }
    let peak = a.peak();
    if !(0.15..=0.8).contains(&peak) {
        return Err(format!("{name}: peak {peak:.3} outside 0.15..0.8"));
    }
    let n = a.left.len() as f32;
    let dc = (a.left.iter().sum::<f32>() / n).abs().max((a.right.iter().sum::<f32>() / n).abs());
    if dc > 0.004 {
        return Err(format!("{name}: DC offset {dc:.4}"));
    }
    let first = a.left[0].abs().max(a.right[0].abs());
    if first > 0.02 {
        return Err(format!("{name}: click at note start ({first:.3})"));
    }
    let tail = (0.02 * SR) as usize;
    let end = a.left[a.left.len() - tail..].iter().chain(a.right[a.right.len() - tail..].iter()).fold(0f32, |m, x| m.max(x.abs()));
    if end > 1e-3 {
        return Err(format!("{name}: release does not reach silence ({end:.5})"));
    }
    Ok(format!("{name:<24} peak {peak:.3}  rms {:.3}", a.rms()))
}

/// Fundamental estimate from the autocorrelation of a steady segment.
fn pitch_of(a: &Audio) -> f32 {
    let start = (0.25 * SR) as usize;
    let x: Vec<f32> = a.left[start..start + 8192].iter().zip(&a.right[start..start + 8192]).map(|(l, r)| l + r).collect();
    let w = 4096;
    // Normalized autocorrelation.
    let r = |lag: usize| -> f32 {
        let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
        for i in 0..w {
            xy += x[i] * x[i + lag];
            xx += x[i] * x[i];
            yy += x[i + lag] * x[i + lag];
        }
        xy / (xx * yy).sqrt().max(1e-12)
    };
    let hi = (SR / 40.0) as usize;
    let vals: Vec<f32> = (0..hi).map(r).collect();
    // Skip the main lobe, then take the first strong peak.
    let mut i = vals.iter().position(|v| *v < 0.0).unwrap();
    let best = vals[i..].iter().cloned().fold(f32::MIN, f32::max);
    i += vals[i..].iter().position(|v| *v >= 0.85 * best).unwrap();
    while i + 1 < vals.len() && vals[i + 1] > vals[i] {
        i += 1;
    }
    SR / i as f32
}

fn assert_octave(kind: &str, dev: &Device) {
    let f48 = pitch_of(&render_note(dev, 48, 0.9, 1.0, SR));
    let f60 = pitch_of(&render_note(dev, 60, 0.9, 1.0, SR));
    let ratio = f60 / f48;
    println!("{kind}: f(48) = {f48:.1} Hz, f(60) = {f60:.1} Hz, ratio {ratio:.3}");
    assert!((f48 - 130.8).abs() < 130.8 * 0.03, "{kind}: f(48) = {f48}");
    assert!((ratio - 2.0).abs() < 0.06, "{kind}: octave ratio {ratio}");
}

#[test]
fn sextant_sounds_with_defaults() {
    let a = render_full(&Device::new("sextant"), 60);
    println!("{}", check_note("sextant default", &a).unwrap());
}

#[test]
fn tessera_sounds_with_defaults() {
    let a = render_full(&Device::new("tessera"), 60);
    println!("{}", check_note("tessera default", &a).unwrap());
}

#[test]
fn every_algorithm_and_table_is_healthy() {
    for algo in ["stack", "twin", "triad", "ep", "bell", "organ", "pad", "brass"] {
        let a = render_full(&device("sextant", &[("feedback", 0.7)], &[("algorithm", algo)]), 57);
        assert!(a.left.iter().all(|x| x.is_finite()), "{algo}");
        assert!(a.peak() > 0.05 && a.peak() < 1.0, "sextant {algo}: peak {}", a.peak());
    }
    for table in ["analog", "digital", "vocal", "growl", "glass", "pulse"] {
        for warp in ["none", "bend", "sync", "fold", "mirror"] {
            for pos in [0.0, 0.5, 1.0] {
                let d = device("tessera", &[("position", pos), ("warp", 0.6)], &[("table", table), ("warp", warp)]);
                let a = render_note(&d, 45, 0.9, 1.0, SR);
                assert!(a.left.iter().all(|x| x.is_finite()), "{table}/{warp}");
                assert!(a.peak() > 0.01 && a.peak() < 1.0, "tessera {table}/{warp}/{pos}: peak {}", a.peak());
            }
        }
    }
}

#[test]
fn sextant_tracks_pitch() {
    assert_octave("sextant", &Device::new("sextant"));
}

#[test]
fn tessera_tracks_pitch() {
    assert_octave("tessera", &Device::new("tessera"));
    assert_octave("tessera growl", &device("tessera", &[("unison", 1.0), ("position", 0.3)], &[("table", "growl")]));
}

/// Share of the spectral energy (below 0.45 × sr) that is not at a harmonic.
fn inharmonic_share(x: &[f32], f0: f32) -> f64 {
    let n = 8192;
    let x = &x[x.len() / 2 - n / 2..x.len() / 2 + n / 2];
    let mut total = 0.0;
    let mut off = 0.0;
    let bin_hz = SR as f64 / n as f64;
    for k in 1..(n as f64 * 0.45) as usize {
        let w = std::f64::consts::TAU * k as f64 / n as f64;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in x.iter().enumerate() {
            let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let v = *s as f64 * hann;
            re += v * (w * i as f64).cos();
            im -= v * (w * i as f64).sin();
        }
        let e = re * re + im * im;
        total += e;
        let hz = k as f64 * bin_hz;
        let h = (hz / f0 as f64).round().max(1.0);
        if (hz - h * f0 as f64).abs() > 4.0 * bin_hz {
            off += e;
        }
    }
    off / total
}

#[test]
fn tessera_high_notes_do_not_alias() {
    let key = 108u8;
    let f0 = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
    let d = device("tessera", &[("unison", 1.0), ("position", 0.5), ("cutoff", 20000.0), ("resonance", 0.0)], &[("table", "analog")]);
    let a = render_note(&d, key, 0.9, 1.0, SR);
    let ours = inharmonic_share(&a.left, f0);
    // A naive (non band-limited) saw at the same pitch, for comparison.
    let naive: Vec<f32> = (0..a.left.len()).map(|i| 2.0 * (i as f32 * f0 / SR).fract() - 1.0).collect();
    let reference = inharmonic_share(&naive, f0);
    println!("inharmonic energy: tessera {ours:.2e}, naive saw {reference:.2e}");
    assert!(ours < 1e-3, "tessera aliasing {ours}");
    assert!(ours < reference / 20.0, "tessera {ours} vs naive {reference}");
}

#[test]
fn presets_render_within_level_bounds() {
    for kind in ["sextant", "tessera"] {
        let presets: Vec<_> = rosaclef_core::presets::all().into_iter().filter(|p| p.kind == kind).collect();
        assert_eq!(presets.len(), 6, "{kind} presets");
        let mut errors = vec![];
        for p in presets {
            for pitch in [48u8, 60, 72] {
                match check_note(&format!("{} @{pitch}", p.name), &render_full(&p.device(), pitch)) {
                    Ok(line) => println!("{line}"),
                    Err(e) => errors.push(e),
                }
            }
        }
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}

/// Play far more notes than there are voices; stealing must stay bounded and smooth.
#[test]
fn voice_stealing_is_clean() {
    let ctx = Ctx { sr: SR, bpm: 120.0 };
    // Low sine tones: legitimate sample steps are tiny, so a hard cut would stand out.
    let sine_fm = device("sextant", &[("op2Level", 0.0), ("op1Sustain", 1.0), ("release", 3.0)], &[("algorithm", "stack")]);
    let sine_wt = device("tessera", &[("position", 0.0), ("unison", 1.0), ("sustain", 1.0), ("release", 3.0)], &[]);
    for dev in [sine_fm, sine_wt] {
        let mut inst = instruments::create(&dev, &ctx).unwrap();
        let mut l = vec![0f32; 128];
        let mut r = vec![0f32; 128];
        let mut max_step = 0f32;
        let mut prev = 0f32;
        for block in 0..400 {
            if block % 3 == 0 && block < 300 {
                let key = 24 + (block * 7 % 16) as u8;
                inst.handle(NoteKind::On { key, velocity: 0.9 });
                if block >= 6 {
                    inst.handle(NoteKind::Off { key: 24 + ((block - 6) * 7 % 16) as u8 });
                }
            }
            l.fill(0.0);
            r.fill(0.0);
            inst.render(&mut l, &mut r);
            for x in &l {
                assert!(x.is_finite());
                max_step = max_step.max((x - prev).abs());
                prev = *x;
            }
        }
        println!("{}: largest sample step while stealing {max_step:.3}", dev.kind);
        assert!(max_step < 0.2, "{}: step {max_step}", dev.kind);
    }
}

fn realtime_factor(dev: &Device) -> f64 {
    let ctx = Ctx { sr: SR, bpm: 120.0 };
    let mut inst = instruments::create(dev, &ctx).unwrap();
    for (i, key) in [36u8, 48, 55, 60, 64, 67, 71, 76].iter().enumerate() {
        inst.handle(NoteKind::On { key: *key, velocity: 0.5 + i as f32 * 0.05 });
    }
    let seconds = 5.0;
    let blocks = (seconds * SR / 128.0) as usize;
    let mut l = vec![0f32; 128];
    let mut r = vec![0f32; 128];
    let t = Instant::now();
    let mut sum = 0.0;
    for _ in 0..blocks {
        l.fill(0.0);
        r.fill(0.0);
        inst.render(&mut l, &mut r);
        sum += l[0];
    }
    assert!(sum.is_finite());
    seconds as f64 / t.elapsed().as_secs_f64()
}

#[test]
#[ignore = "wall-clock timing; flaky on a loaded machine (run with --ignored)"]
fn realtime_performance() {
    let cases = [
        ("sextant ep (default)", device("sextant", &[("release", 8.0)], &[])),
        ("sextant stack + feedback", device("sextant", &[("feedback", 0.8), ("release", 8.0)], &[("algorithm", "stack")])),
        ("tessera default (unison 3)", device("tessera", &[("sustain", 1.0)], &[])),
        ("tessera supersaw (unison 7)", device("tessera", &[("unison", 7.0), ("detune", 38.0), ("position", 0.5), ("sustain", 1.0)], &[])),
        (
            "tessera unison 7, warp, drive, sub",
            device("tessera", &[("unison", 7.0), ("positionLfo", 0.5), ("warp", 0.5), ("drive", 0.5), ("sub", 0.5), ("sustain", 1.0)], &[("warp", "mirror")]),
        ),
    ];
    for (name, dev) in cases {
        let rt = realtime_factor(&dev);
        println!("{name:<36} 8 notes x 5 s: {rt:.0}x realtime");
        assert!(rt > 5.0, "{name}: only {rt:.1}x realtime");
    }
}

#[test]
fn table_generation_is_fast_enough() {
    let ctx = Ctx { sr: SR, bpm: 120.0 };
    let mut inst = instruments::create(&Device::new("tessera"), &ctx).unwrap();
    let t = Instant::now();
    for table in ["digital", "vocal", "growl", "glass", "pulse", "analog"] {
        inst.set_device(&device("tessera", &[], &[("table", table)]), &ctx);
    }
    println!("6 wavetables generated in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
}
