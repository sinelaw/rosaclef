//! Tests for the additive and granular instruments.

use rosaclef_core::{Channel, Device, InsertIx, Project};
use rosaclef_engine::instruments::{self, NoteKind};
use rosaclef_engine::render::{render_note, Audio};
use rosaclef_engine::samples::SampleData;
use rosaclef_engine::{Ctx, Engine, MAX_BLOCK};

const SR: f32 = 48000.0;
const KINDS: [&str; 2] = ["additive", "granular"];

fn project(dev: &Device) -> Project {
    let mut p = Project::empty("t");
    p.channels.push(Channel {
        id: "x".into(),
        name: "x".into(),
        color: "#ffffff".into(),
        instrument: dev.clone(),
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx::MASTER,
        arp: None,
    });
    p.mixer.inserts[0].effects.clear();
    p
}

/// Play `pitches` together for `hold` seconds, then render `tail` seconds of release.
fn play(engine: &mut Engine, pitches: &[u8], velocity: f32, hold: f32, tail: f32) -> Audio {
    for &k in pitches {
        engine.note_on("x", k, velocity);
    }
    let mut out = Audio {
        sample_rate: SR,
        ..Default::default()
    };
    let hold_n = (hold * SR) as usize;
    let total = ((hold + tail) * SR) as usize;
    let mut bl = [0f32; MAX_BLOCK];
    let mut br = [0f32; MAX_BLOCK];
    let mut done = 0;
    let mut released = false;
    while done < total {
        if !released && done >= hold_n {
            for &k in pitches {
                engine.note_off("x", k);
            }
            released = true;
        }
        let mut n = (total - done).min(MAX_BLOCK);
        if !released {
            n = n.min(hold_n - done);
        }
        engine.process(&mut bl[..n], &mut br[..n]);
        out.left.extend_from_slice(&bl[..n]);
        out.right.extend_from_slice(&br[..n]);
        done += n;
    }
    out
}

fn note(dev: &Device, pitch: u8, hold: f32, tail: f32) -> Audio {
    let mut e = Engine::new(SR);
    e.set_project(project(dev));
    play(&mut e, &[pitch], 0.9, hold, tail)
}

fn mean(x: &[f32]) -> f32 {
    x.iter().sum::<f32>() / x.len().max(1) as f32
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

/// YIN-style fundamental estimate (Hz) of a mono excerpt.
fn pitch(x: &[f32], lo: f32, hi: f32) -> f32 {
    let w = 4096;
    let x = &x[..w + (SR / lo) as usize + 2];
    let min_lag = (SR / hi) as usize;
    let max_lag = (SR / lo) as usize;
    let mut d = vec![0f32; max_lag + 2];
    for (lag, dv) in d.iter_mut().enumerate().skip(1) {
        *dv = (0..w).map(|i| (x[i] - x[i + lag]).powi(2)).sum();
    }
    // Cumulative mean normalised difference.
    let mut cmnd = vec![1f32; d.len()];
    let mut run = 0.0;
    for lag in 1..d.len() {
        run += d[lag];
        cmnd[lag] = d[lag] * lag as f32 / run.max(1e-12);
    }
    let mut best = min_lag;
    for lag in min_lag..=max_lag {
        if cmnd[lag] < 0.2 {
            // Walk to the local minimum.
            let mut l = lag;
            while l < max_lag && cmnd[l + 1] < cmnd[l] {
                l += 1;
            }
            best = l;
            break;
        }
        if cmnd[lag] < cmnd[best] {
            best = lag;
        }
    }
    // Parabolic refinement.
    let (a, b, c) = (cmnd[best - 1], cmnd[best], cmnd[best + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-9 {
        0.5 * (a - c) / den
    } else {
        0.0
    };
    SR / (best as f32 + off)
}

fn mono(a: &Audio) -> Vec<f32> {
    a.left
        .iter()
        .zip(&a.right)
        .map(|(l, r)| 0.5 * (l + r))
        .collect()
}

fn check_basic(name: &str, a: &Audio) {
    assert!(
        a.left.iter().chain(&a.right).all(|x| x.is_finite()),
        "{name}: non-finite output"
    );
    let pk = a.peak();
    assert!((0.15..=0.8).contains(&pk), "{name}: peak {pk}");
    let dc = mean(&a.left).abs().max(mean(&a.right).abs());
    assert!(dc < 0.01 * pk.max(0.1), "{name}: DC offset {dc}");
}

#[test]
fn engines_sound_with_defaults() {
    for kind in KINDS {
        let a = render_note(&Device::new(kind), 60, 0.9, 3.0, SR);
        eprintln!("{kind}: peak {:.3} rms {:.3}", a.peak(), a.rms());
        check_basic(kind, &a);
        assert!(a.rms() > 0.02, "{kind}: rms {}", a.rms());
    }
}

/// Zero crossings per second.
fn zcr(x: &[f32]) -> f32 {
    let n = x
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    n as f32 * SR / x.len() as f32
}

#[test]
fn pitch_tracks_the_keyboard() {
    for kind in KINDS {
        let mut dev = Device::new(kind);
        // Keep the output low-pass out of the way of the transposed spectrum.
        let top = "cutoff";
        dev.params.insert(top.into(), 20000.0);
        let at = (0.8 * SR) as usize;
        let a = mono(&note(&dev, 60, 1.6, 0.1))[at..].to_vec();
        let b = mono(&note(&dev, 72, 1.6, 0.1))[at..].to_vec();
        let (fa, fb) = (pitch(&a, 60.0, 1500.0), pitch(&b, 60.0, 1500.0));
        let (za, zb) = (
            zcr(&a[..(0.6 * SR) as usize]),
            zcr(&b[..(0.6 * SR) as usize]),
        );
        eprintln!(
            "{kind}: C4 -> {fa:.1} Hz, C5 -> {fb:.1} Hz; zero crossings {za:.0}/s -> {zb:.0}/s"
        );
        // Everything moves up an octave.
        let ratio = zb / za;
        assert!(
            (1.85..2.15).contains(&ratio),
            "{kind}: zero-crossing ratio {ratio}"
        );
        // Both notes are a C (root 60; the sources are tuned to C).
        for f in [fa, fb] {
            let oct = (f / 261.63).log2();
            assert!(
                (oct - oct.round()).abs() < 0.03,
                "{kind}: estimated {f} Hz is not a C"
            );
        }
        if kind == "additive" {
            assert!((fb / fa - 2.0).abs() < 0.04, "additive: {fa} -> {fb}");
        }
    }
}

#[test]
fn every_preset_renders_within_bounds_and_releases() {
    let presets: Vec<_> = rosaclef_core::presets::all()
        .into_iter()
        .filter(|p| KINDS.contains(&p.kind))
        .collect();
    assert_eq!(presets.len(), 12);
    for p in presets {
        let dev = p.device();
        let attack = dev.param("attack") as f32;
        let release = dev.param("release") as f32;
        // Level: hold past the attack.
        let seconds = (attack * 1.5 + 1.2) / 0.6;
        let a = render_note(&dev, 60, 0.9, seconds, SR);
        eprintln!("{:<22} peak {:.3} rms {:.3}", p.name, a.peak(), a.rms());
        check_basic(p.name, &a);
        // Release reaches silence.
        let b = note(&dev, 60, attack + 0.5, release * 2.5 + 0.5);
        let end = &b.left[b.left.len() - (0.1 * SR) as usize..];
        assert!(peak(end) < 1e-3, "{}: tail still at {}", p.name, peak(end));
    }
}

#[test]
fn note_edges_are_click_free() {
    for kind in KINDS {
        let mut dev = Device::new(kind);
        dev.params.insert("attack".into(), 0.001);
        dev.params.insert("release".into(), 0.001);
        let a = note(&dev, 60, 0.5, 0.3);
        // Start: the first millisecond ramps from silence.
        let first = peak(&a.left[..8]).max(peak(&a.right[..8]));
        assert!(first < 0.05, "{kind}: onset jumps to {first}");
        // End: the release is short but smooth, then silent.
        let off = (0.5 * SR) as usize;
        let d = a
            .left
            .windows(2)
            .skip(off)
            .take((0.02 * SR) as usize)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        let steady = a
            .left
            .windows(2)
            .skip(off - 4800)
            .take(4800)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        assert!(
            d <= steady * 1.5 + 1e-3,
            "{kind}: release step {d} vs steady {steady}"
        );
        assert!(
            peak(&a.left[a.left.len() - 4800..]) < 1e-4,
            "{kind}: not silent after release"
        );
    }
}

/// Largest second difference relative to its RMS (a click detector).
fn spikiness(x: &[f32]) -> f32 {
    let d2: Vec<f32> = x
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .collect();
    let rms = (d2.iter().map(|x| x * x).sum::<f32>() / d2.len() as f32).sqrt();
    d2.iter().fold(0f32, |m, x| m.max(*x)) / rms.max(1e-12)
}

/// Overlapping staggered notes, one every 40 blocks.
fn staggered(kind: &str, notes: u8) -> Vec<f32> {
    let mut e = Engine::new(SR);
    e.set_project(project(&Device::new(kind)));
    let mut out = vec![];
    let mut bl = [0f32; MAX_BLOCK];
    let mut br = [0f32; MAX_BLOCK];
    for i in 0..notes {
        e.note_on("x", 40 + ((i as u32 * 7) % 40) as u8, 0.8);
        for _ in 0..40 {
            e.process(&mut bl, &mut br);
            out.extend_from_slice(&bl);
        }
    }
    out
}

#[test]
fn voice_stealing_is_bounded_and_smooth() {
    for kind in KINDS {
        // 40 overlapping notes: far more than the voice count.
        let out = staggered(kind, 40);
        assert!(out.iter().all(|x| x.is_finite()), "{kind}: non-finite");
        let pk = peak(&out);
        assert!(pk < 4.0, "{kind}: peak {pk} while stealing");
        // Stealing must not add discontinuities beyond what the same
        // material shows without stealing (the first 10 notes).
        let base = spikiness(&staggered(kind, 10));
        let steal = spikiness(&out);
        eprintln!("{kind}: stealing peak {pk:.3}, spikiness {steal:.1} (no stealing {base:.1})");
        assert!(
            steal < base * 1.6,
            "{kind}: click while stealing ({steal} vs {base})"
        );
    }
}

#[test]
fn every_additive_spectrum_and_granular_source_sounds() {
    for spec in rosaclef_core::catalog::SPECTRA {
        let mut dev = Device::new("additive");
        dev.options.insert("spectrum".into(), spec.to_string());
        let a = render_note(&dev, 60, 0.9, 2.0, SR);
        eprintln!("additive {spec:<8} peak {:.3}", a.peak());
        check_basic(spec, &a);
    }
    for src in rosaclef_core::catalog::TEXTURE_SOURCES
        .iter()
        .filter(|s| **s != "sample")
    {
        let mut dev = Device::new("granular");
        dev.options.insert("source".into(), src.to_string());
        let a = render_note(&dev, 60, 0.9, 2.0, SR);
        eprintln!("granular {src:<8} peak {:.3}", a.peak());
        check_basic(src, &a);
    }
}

#[test]
fn granular_plays_a_project_sample() {
    let mut dev = Device::new("granular");
    dev.options.insert("source".into(), "sample".into());
    dev.options
        .insert("sample".into(), "samples/tone.wav".into());
    let mut e = Engine::new(SR);
    e.set_project(project(&dev));
    // Silent until the sample arrives.
    let silent = play(&mut e, &[60], 0.9, 0.3, 0.1);
    assert!(silent.peak() == 0.0);
    // A 220 Hz tone at 44.1 kHz: playing the root must give 220 Hz back.
    let sr_in = 44100.0;
    let tone: Vec<f32> = (0..(2.0 * sr_in) as usize)
        .map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / sr_in).sin())
        .collect();
    e.set_sample(
        "samples/tone.wav",
        SampleData {
            sample_rate: sr_in,
            channels: vec![tone],
        },
    );
    let a = play(&mut e, &[60], 0.9, 1.5, 2.0);
    assert!(a.peak() > 0.05, "peak {}", a.peak());
    let f = pitch(&mono(&a)[(0.9 * SR) as usize..], 60.0, 1500.0);
    assert!((f / 220.0 - 1.0).abs() < 0.02, "sample played at {f} Hz");
}

/// `cargo test -p rosaclef-engine --release --test additive_granular -- --ignored --nocapture`
#[test]
#[ignore]
fn realtime_factor() {
    let heavy = [
        ("additive", "defaults", None),
        ("additive", "Opaline Veil Pad", Some("Opaline Veil Pad")),
        ("additive", "Seraphine Choir", Some("Seraphine Choir")),
        ("granular", "defaults", None),
        (
            "granular",
            "Stardust Shimmer Pad",
            Some("Stardust Shimmer Pad"),
        ),
    ];
    for src in ["choir", "bowl", "ember", "strings", "air"] {
        let mut dev = Device::new("granular");
        dev.options.insert("source".into(), src.into());
        let t = std::time::Instant::now();
        let inst = instruments::create(&dev, &Ctx { sr: SR, bpm: 120.0 });
        println!(
            "granular source {src:<8} built in {:.1} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
        drop(inst);
    }
    for (kind, label, preset) in heavy {
        let dev = match preset {
            Some(n) => rosaclef_core::presets::find(n).unwrap().device(),
            None => Device::new(kind),
        };
        let ctx = Ctx { sr: SR, bpm: 120.0 };
        let mut inst = instruments::create(&dev, &ctx).unwrap();
        for k in [48u8, 55, 60, 64, 67, 71, 74, 79] {
            inst.handle(NoteKind::On {
                key: k,
                velocity: 0.8,
            });
        }
        let frames = (5.0 * SR) as usize;
        let mut bl = [0f32; MAX_BLOCK];
        let mut br = [0f32; MAX_BLOCK];
        let t = std::time::Instant::now();
        let mut done = 0;
        let mut acc = 0.0;
        while done < frames {
            bl.fill(0.0);
            br.fill(0.0);
            inst.render(&mut bl, &mut br);
            acc += bl[0];
            done += MAX_BLOCK;
        }
        let secs = t.elapsed().as_secs_f64();
        println!(
            "{kind:<7} {label:<20} 8 notes x 5 s: {:.1}x realtime ({acc:.1})",
            5.0 / secs
        );
    }
}

#[test]
fn extreme_settings_stay_finite() {
    for kind in KINDS {
        let spec = rosaclef_core::catalog::device(kind).unwrap();
        for corner in 0..3 {
            let mut dev = Device::new(kind);
            for p in spec.params {
                let v = match corner {
                    0 => p.min,
                    1 => p.max,
                    _ => {
                        if p.key.contains("ttack") || p.key.contains("elease") {
                            p.min
                        } else {
                            p.max
                        }
                    }
                };
                dev.params.insert(p.key.into(), v);
            }
            for pitch in [0u8, 60, 127] {
                let a = note(&dev, pitch, 0.4, 0.2);
                assert!(
                    a.left.iter().chain(&a.right).all(|x| x.is_finite()),
                    "{kind} corner {corner} pitch {pitch}: non-finite"
                );
                assert!(
                    a.peak() < 3.0,
                    "{kind} corner {corner} pitch {pitch}: peak {}",
                    a.peak()
                );
            }
        }
    }
}
