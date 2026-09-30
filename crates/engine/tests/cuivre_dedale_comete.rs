//! Behaviour tests for the Cuivre (virtual analog), Dédale (generative
//! sequencer) and Comète (transition FX) instruments.

use rosaclef_core::{presets, Device};
use rosaclef_engine::instruments::{create, Instrument, NoteEvent, NoteKind};
use rosaclef_engine::Ctx;

const SR: f32 = 48000.0;
const BLOCK: usize = 128;

struct Out {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl Out {
    fn peak(&self) -> f32 {
        self.l.iter().chain(self.r.iter()).fold(0.0f32, |m, x| m.max(x.abs()))
    }
    fn peak_in(&self, from: f32, to: f32) -> f32 {
        let (a, b) = (secs(from).min(self.l.len()), secs(to).min(self.l.len()));
        self.l[a..b].iter().chain(self.r[a..b].iter()).fold(0.0f32, |m, x| m.max(x.abs()))
    }
    fn rms_in(&self, from: f32, to: f32) -> f32 {
        let (a, b) = (secs(from).min(self.l.len()), secs(to).min(self.l.len()));
        let n = ((b - a) * 2).max(1) as f32;
        (self.l[a..b].iter().chain(self.r[a..b].iter()).map(|x| x * x).sum::<f32>() / n).sqrt()
    }
    fn mono(&self) -> Vec<f32> {
        self.l.iter().zip(&self.r).map(|(a, b)| 0.5 * (a + b)).collect()
    }
    /// Time (s) of the last sample louder than `thr`.
    fn last_above(&self, thr: f32) -> f32 {
        let i = (0..self.l.len()).rev().find(|&i| self.l[i].abs() > thr || self.r[i].abs() > thr).unwrap_or(0);
        i as f32 / SR
    }
}

fn secs(t: f32) -> usize {
    (t * SR).round() as usize
}

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

/// Render `total` seconds; `events` are (time in seconds, event), any order.
fn play(dev: &Device, bpm: f32, events: &[(f32, NoteKind)], total: f32) -> Out {
    let mut inst: Box<dyn Instrument> = create(dev, &Ctx { sr: SR, bpm }).expect("known instrument");
    play_with(&mut *inst, events, total)
}

fn play_with(inst: &mut dyn Instrument, events: &[(f32, NoteKind)], total: f32) -> Out {
    let mut evs: Vec<(usize, NoteKind)> = events.iter().map(|(t, e)| (secs(*t), *e)).collect();
    evs.sort_by_key(|e| e.0);
    let frames = secs(total);
    let mut out = Out { l: vec![0.0; frames], r: vec![0.0; frames] };
    let mut pos = 0;
    let mut next = 0;
    let mut block_events = Vec::new();
    while pos < frames {
        let n = (frames - pos).min(BLOCK);
        block_events.clear();
        while next < evs.len() && evs[next].0 < pos + n {
            block_events.push(NoteEvent { offset: evs[next].0.saturating_sub(pos), kind: evs[next].1 });
            next += 1;
        }
        let (l, r) = (&mut out.l[pos..pos + n], &mut out.r[pos..pos + n]);
        inst.process(&block_events, l, r);
        pos += n;
    }
    out
}

fn on(key: u8) -> NoteKind {
    NoteKind::On { key, velocity: 0.9 }
}
fn off(key: u8) -> NoteKind {
    NoteKind::Off { key }
}

/// A single note held for `hold` seconds, `total` rendered.
fn note(dev: &Device, bpm: f32, key: u8, hold: f32, total: f32) -> Out {
    play(dev, bpm, &[(0.0, on(key)), (hold, off(key))], total)
}

/// Finite, bounded level, no DC offset, no click at the start, silent at the end.
fn check_clean(name: &str, o: &Out, lo: f32, hi: f32) {
    assert!(o.l.iter().chain(o.r.iter()).all(|x| x.is_finite()), "{name}: non-finite samples");
    let peak = o.peak();
    assert!(peak >= lo && peak <= hi, "{name}: peak {peak} outside {lo}..{hi}");
    let n = o.l.len() as f32;
    let dc_l = o.l.iter().sum::<f32>() / n;
    let dc_r = o.r.iter().sum::<f32>() / n;
    assert!(dc_l.abs() < 0.01 && dc_r.abs() < 0.01, "{name}: DC offset {dc_l} {dc_r}");
    let start = o.l[..8].iter().chain(o.r[..8].iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(start < 0.08, "{name}: click at note start ({start})");
    let tail = o.l.len() - 64;
    let end = o.l[tail..].iter().chain(o.r[tail..].iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(end < 2e-3, "{name}: not silent at the end ({end})");
}

/// For smooth (tonal, low-passed) sounds: no sample-to-sample jump anywhere
/// comes close to the peak level, i.e. no clicks at note start, end or steal.
fn check_no_jumps(name: &str, o: &Out, frac: f32) {
    let peak = o.peak();
    let jump = o.l.windows(2).chain(o.r.windows(2)).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(jump < frac * peak, "{name}: discontinuity {jump} (peak {peak})");
}

/// Fundamental estimate by normalized autocorrelation (first strong peak).
fn pitch_hz(x: &[f32]) -> f32 {
    let min_lag = (SR / 2000.0) as usize;
    let max_lag = (SR / 25.0) as usize;
    let n = x.len() - max_lag;
    let e0: f32 = x[..n].iter().map(|v| v * v).sum();
    let r: Vec<f32> = (0..=max_lag)
        .map(|lag| {
            if lag < min_lag {
                return 0.0;
            }
            let c: f32 = (0..n).map(|i| x[i] * x[i + lag]).sum();
            let e1: f32 = x[lag..lag + n].iter().map(|v| v * v).sum();
            c / (e0 * e1).sqrt().max(1e-12)
        })
        .collect();
    let best = r.iter().cloned().fold(0.0f32, f32::max);
    let lag = (min_lag + 1..max_lag)
        .find(|&l| r[l] > 0.9 * best && r[l] >= r[l - 1] && r[l] >= r[l + 1])
        .unwrap_or(min_lag);
    // Parabolic refinement.
    let (a, b, c) = (r[lag - 1], r[lag], r[lag + 1]);
    let d = (a - c) / (2.0 * (a - 2.0 * b + c)).min(-1e-9);
    SR / (lag as f32 - d)
}

fn pitch_at(o: &Out, from: f32, to: f32) -> f32 {
    let m = o.mono();
    pitch_hz(&m[secs(from)..secs(to)])
}

/// Onset times: envelope rising past `thr` after having fallen below `thr / 4`.
fn onsets(o: &Out, thr: f32) -> Vec<f32> {
    let m = o.mono();
    let mut env = 0.0f32;
    let mut armed = true;
    let mut out = vec![];
    for (i, x) in m.iter().enumerate() {
        env = x.abs().max(env * 0.995);
        if armed && env > thr {
            out.push(i as f32 / SR);
            armed = false;
        } else if !armed && env < thr * 0.25 {
            armed = true;
        }
    }
    out
}

fn presets_of(kind: &str) -> Vec<&'static presets::Preset> {
    presets::all().into_iter().filter(|p| p.kind == kind).collect()
}

// ------------------------------------------------------------------ Cuivre

#[test]
fn cuivre_defaults_sound_clean() {
    let dev = Device::new("cuivre");
    for key in [36u8, 60, 84] {
        let o = note(&dev, 120.0, key, 1.0, 2.5);
        check_clean(&format!("cuivre default key {key}"), &o, 0.15, 0.8);
    }
    let dev = device("cuivre", &[], &[("filter", "screamer")]);
    let o = note(&dev, 120.0, 48, 1.0, 2.5);
    check_clean("cuivre screamer", &o, 0.15, 0.8);
    // A dark patch is smooth: no clicks at note on / off, nor when notes
    // retrigger in mono mode.
    let smooth = device("cuivre", &[("cutoff", 300.0), ("filterEnv", 0.0), ("attack", 0.003), ("release", 0.02)], &[("mode", "mono")]);
    let o = play(&smooth, 120.0, &[(0.0, on(36)), (0.3, on(43)), (0.6, off(43)), (0.9, off(36)), (1.2, on(48)), (1.5, off(48))], 2.0);
    check_clean("cuivre smooth", &o, 0.05, 0.8);
    check_no_jumps("cuivre smooth", &o, 0.25);
}

#[test]
fn cuivre_tracks_pitch() {
    let dev = device("cuivre", &[("sub", 0.0), ("mix2", 0.0), ("drift", 0.0), ("cutoff", 900.0), ("keyTrack", 1.0), ("filterEnv", 0.0), ("resonance", 0.0)], &[]);
    let lo = note(&dev, 120.0, 48, 1.0, 1.0);
    let hi = note(&dev, 120.0, 60, 1.0, 1.0);
    let (f_lo, f_hi) = (pitch_at(&lo, 0.3, 0.6), pitch_at(&hi, 0.3, 0.6));
    assert!((f_lo - 130.8).abs() < 4.0, "C3 measured {f_lo} Hz");
    let ratio = f_hi / f_lo;
    assert!((ratio - 2.0).abs() < 0.06, "octave ratio {ratio} ({f_lo} -> {f_hi})");
    // Drift keeps the pitch within a few cents.
    let drifting = device("cuivre", &[("sub", 0.0), ("mix2", 0.0), ("drift", 1.0), ("cutoff", 900.0), ("keyTrack", 1.0), ("filterEnv", 0.0), ("resonance", 0.0)], &[]);
    let d = note(&drifting, 120.0, 48, 2.0, 2.0);
    for w in [0.2f32, 0.8, 1.4] {
        let f = pitch_at(&d, w, w + 0.3);
        let cents = 1200.0 * (f / 130.81).log2();
        assert!(cents.abs() < 15.0, "drift too wide: {cents} cents");
    }
}

#[test]
fn cuivre_mono_is_last_note_priority() {
    let dev = device("cuivre", &[("sub", 0.0), ("mix2", 0.0), ("drift", 0.0), ("cutoff", 900.0), ("keyTrack", 1.0), ("filterEnv", 0.0), ("resonance", 0.0)], &[("mode", "mono")]);
    // Hold C3, then G3 on top, release G3: back to C3.
    let o = play(&dev, 120.0, &[(0.0, on(48)), (0.5, on(55)), (1.0, off(55)), (1.5, off(48))], 2.0);
    let a = pitch_at(&o, 0.2, 0.45);
    let b = pitch_at(&o, 0.7, 0.95);
    let c = pitch_at(&o, 1.2, 1.45);
    assert!((a - 130.8).abs() < 4.0, "first note {a}");
    assert!((b - 196.0).abs() < 5.0, "second note {b}");
    assert!((c - 130.8).abs() < 4.0, "back to held note {c}");
    // Only one voice: the level with two keys held is the same as with one.
    let one = o.rms_in(0.3, 0.45);
    let two = o.rms_in(0.8, 0.95);
    assert!(two < one * 1.4, "mono should not stack voices ({one} vs {two})");
    // Poly stacks them.
    let poly = device("cuivre", &[("sub", 0.0), ("mix2", 0.0), ("drift", 0.0), ("cutoff", 900.0), ("keyTrack", 1.0), ("filterEnv", 0.0), ("resonance", 0.0)], &[]);
    let p = play(&poly, 120.0, &[(0.0, on(48)), (0.5, on(55)), (1.0, off(55)), (1.5, off(48))], 2.0);
    assert!(p.rms_in(0.8, 0.95) > p.rms_in(0.3, 0.45) * 1.2, "poly should stack voices");
}

#[test]
fn cuivre_legato_ties_overlapping_notes() {
    let params = [("sub", 0.0), ("mix2", 0.0), ("drift", 0.0), ("cutoff", 3000.0), ("filterEnv", 0.0), ("resonance", 0.0), ("attack", 0.005), ("decay", 0.08), ("sustain", 0.3), ("glide", 0.15)];
    let events = [(0.0, on(48)), (0.5, on(60)), (1.0, off(48)), (1.2, off(60)), (1.6, on(55)), (2.0, off(55))];
    let mono = play(&device("cuivre", &params, &[("mode", "mono")]), 120.0, &events, 2.4);
    let legato = play(&device("cuivre", &params, &[("mode", "legato")]), 120.0, &events, 2.4);
    // Mono retriggers the envelope on the second note, legato does not.
    let base_m = mono.rms_in(0.35, 0.48);
    let base_l = legato.rms_in(0.35, 0.48);
    let hit_m = mono.peak_in(0.5, 0.53) / mono.peak_in(0.4, 0.48);
    let hit_l = legato.peak_in(0.5, 0.53) / legato.peak_in(0.4, 0.48);
    assert!(hit_m > 1.8, "mono should retrigger ({hit_m})");
    assert!(hit_l < 1.3, "legato should not retrigger ({hit_l})");
    assert!((base_m - base_l).abs() < 0.3 * base_m, "same sustain level");
    // Legato glides between tied notes: shortly after the second key the
    // pitch is between the two notes, and it arrives later.
    let mid = pitch_at(&legato, 0.52, 0.58);
    let arrived = pitch_at(&legato, 0.95, 1.15);
    assert!(mid > 135.0 && mid < 255.0, "legato glide midway {mid}");
    assert!((arrived - 261.6).abs() < 8.0, "legato glide target {arrived}");
    // A detached note in legato mode does not glide.
    let detached = pitch_at(&legato, 1.62, 1.7);
    assert!((detached - 196.0).abs() < 6.0, "detached note should not glide ({detached})");
}

#[test]
fn cuivre_ladder_is_stable_at_extremes() {
    for filter in ["ladder", "screamer"] {
        for cutoff in [20.0, 20000.0] {
            for key in [24u8, 60, 108] {
                let dev = device(
                    "cuivre",
                    &[("resonance", 1.0), ("cutoff", cutoff), ("drive", 1.0), ("filterEnv", 1.0), ("keyTrack", 1.0), ("noise", 1.0), ("gain", 1.5)],
                    &[("filter", filter)],
                );
                let o = note(&dev, 120.0, key, 1.0, 1.5);
                assert!(o.l.iter().all(|x| x.is_finite()), "{filter} {cutoff} {key}: non-finite");
                assert!(o.peak() < 2.5, "{filter} {cutoff} {key}: peak {}", o.peak());
            }
        }
    }
    // Self-oscillation: with no input level the ladder still rings at resonance 1.
    let dev = device("cuivre", &[("resonance", 1.0), ("cutoff", 800.0), ("mix2", 0.0), ("sub", 0.0), ("filterEnv", 0.0), ("keyTrack", 0.0)], &[]);
    let o = note(&dev, 120.0, 36, 1.5, 1.5);
    assert!(o.rms_in(1.0, 1.4) > 0.02, "ladder should self-oscillate at resonance 1");
}

// ------------------------------------------------------------------ Dédale

#[test]
fn dedale_defaults_play_a_phrase() {
    let dev = Device::new("dedale");
    let o = note(&dev, 120.0, 60, 4.0, 6.0);
    check_clean("dedale default", &o, 0.15, 0.8);
    let hits = onsets(&o, 0.05);
    assert!(hits.len() >= 8, "expected a phrase, got onsets {hits:?}");
    // Releasing the key stops new notes; the sound dies away.
    assert!(o.peak_in(4.8, 6.0) < 1e-3, "still playing after release");
}

#[test]
fn dedale_steps_follow_rate_and_tempo() {
    for (bpm, rate, steps, pulses) in [(120.0f32, 0.25f64, 16.0f64, 4.0f64), (90.0, 0.25, 16.0, 4.0), (140.0, 0.5, 8.0, 8.0)] {
        let dev = device(
            "dedale",
            &[("rate", rate), ("steps", steps), ("pulses", pulses), ("density", 1.0), ("range", 0.0), ("accent", 0.0), ("decay", 0.05), ("gate", 0.3)],
            &[("voice", "pluck")],
        );
        let o = note(&dev, bpm, 57, 2.95, 3.5);
        let hits = onsets(&o, 0.04);
        let expected = rate as f32 * 60.0 / bpm * (steps / pulses) as f32;
        assert!(hits.len() >= 4, "bpm {bpm}: onsets {hits:?}");
        assert!(hits[0] < 0.01, "first note starts immediately ({})", hits[0]);
        for w in hits.windows(2) {
            let dt = w[1] - w[0];
            assert!((dt - expected).abs() < 0.006, "bpm {bpm} rate {rate}: interval {dt}, expected {expected} ({hits:?})");
        }
        let count = (2.95 / expected).floor() as usize + 1;
        assert!(hits.len() == count, "bpm {bpm}: {} onsets, expected {count}", hits.len());
    }
}

#[test]
fn dedale_is_deterministic_per_seed() {
    let a = device("dedale", &[("seed", 42.0)], &[]);
    let b = device("dedale", &[("seed", 43.0)], &[]);
    let r1 = note(&a, 120.0, 60, 3.0, 3.5);
    let r2 = note(&a, 120.0, 60, 3.0, 3.5);
    let r3 = note(&b, 120.0, 60, 3.0, 3.5);
    assert_eq!(r1.l, r2.l, "same seed must render identically");
    let diff: f32 = r1.l.iter().zip(&r3.l).map(|(x, y)| (x - y).abs()).sum();
    assert!(diff > 1.0, "different seeds should differ");
    // variation 0: the phrase repeats exactly every cycle (16 steps = 2 s at 120 BPM).
    let fixed = device("dedale", &[("seed", 5.0), ("variation", 0.0), ("density", 0.7)], &[]);
    let o = note(&fixed, 120.0, 60, 5.0, 5.0);
    let (c1, c2) = (secs(0.5), secs(2.5));
    let len = secs(1.0);
    let err: f32 = (0..len).map(|i| (o.l[c1 + i] - o.l[c2 + i]).abs()).fold(0.0, f32::max);
    assert!(err < 1e-3, "variation 0 should repeat exactly (max diff {err})");
}

#[test]
fn dedale_voices_and_polyphony() {
    for voice in ["pluck", "bell", "bass", "perc"] {
        let dev = device("dedale", &[], &[("voice", voice)]);
        let o = note(&dev, 120.0, if voice == "bass" { 40 } else { 64 }, 3.0, 6.0);
        check_clean(&format!("dedale {voice}"), &o, 0.15, 0.8);
    }
    // Four held keys run four phrases without blowing up.
    let dev = Device::new("dedale");
    let o = play(&dev, 120.0, &[(0.0, on(48)), (0.1, on(55)), (0.2, on(60)), (0.3, on(67)), (3.0, NoteKind::AllOff)], 5.0);
    assert!(o.peak() < 1.2 && o.l.iter().all(|x| x.is_finite()));
    assert!(o.peak_in(4.5, 5.0) < 1e-3);
}

#[test]
fn dedale_tempo_change_keeps_running() {
    let dev = device("dedale", &[("density", 1.0), ("pulses", 16.0), ("decay", 0.05), ("gate", 0.3), ("range", 0.0), ("accent", 0.0)], &[]);
    let mut inst = create(&dev, &Ctx { sr: SR, bpm: 120.0 }).unwrap();
    let a = play_with(&mut *inst, &[(0.0, on(60))], 1.0);
    inst.set_device(&dev, &Ctx { sr: SR, bpm: 60.0 });
    let b = play_with(&mut *inst, &[], 2.0);
    // 16ths at 120 BPM = 0.125 s, at 60 BPM = 0.25 s.
    let ha = onsets(&a, 0.04);
    let hb = onsets(&b, 0.04);
    assert_eq!(ha.len(), 8, "{ha:?}");
    assert!(hb.len() >= 7, "{hb:?}");
    for w in hb.windows(2) {
        assert!((w[1] - w[0] - 0.25).abs() < 0.006, "{hb:?}");
    }
}

// ------------------------------------------------------------------ Comète

#[test]
fn comete_kinds_sound_clean() {
    for kind in ["riser", "downlifter", "impact", "sweep", "subdrop"] {
        let dev = device("comete", &[("length", 4.0)], &[("kind", kind)]);
        let o = note(&dev, 120.0, 60, 0.1, 6.0);
        check_clean(&format!("comete {kind}"), &o, 0.15, 0.8);
    }
    let dev = device("comete", &[("length", 2.0), ("noise", 0.0)], &[("kind", "subdrop")]);
    let o = note(&dev, 120.0, 60, 0.1, 3.0);
    check_no_jumps("comete subdrop", &o, 0.1);
}

#[test]
fn comete_lasts_length_beats() {
    for (bpm, beats) in [(120.0f32, 4.0f64), (90.0, 4.0), (128.0, 8.0)] {
        for kind in ["riser", "downlifter", "sweep", "subdrop"] {
            let dev = device("comete", &[("length", beats), ("space", 0.0)], &[("kind", kind)]);
            // The note-off is ignored: a short note still plays the whole effect.
            let o = note(&dev, bpm, 60, 0.05, beats as f32 * 60.0 / bpm + 1.0);
            let expected = beats as f32 * 60.0 / bpm;
            let end = o.last_above(1e-3);
            assert!(end <= expected + 0.03, "{kind} at {bpm} BPM: ends at {end}, expected {expected}");
            assert!(end >= expected * 0.85, "{kind} at {bpm} BPM: ends at {end}, expected {expected}");
        }
    }
    // A riser peaks at its end.
    let dev = device("comete", &[("length", 8.0)], &[("kind", "riser")]);
    let o = note(&dev, 120.0, 60, 0.05, 5.0);
    assert!(o.rms_in(3.5, 3.95) > 3.0 * o.rms_in(0.2, 0.6), "riser should rise");
    // An impact's tail decays over its length.
    let dev = device("comete", &[("length", 4.0), ("space", 0.0)], &[("kind", "impact")]);
    let o = note(&dev, 120.0, 60, 0.05, 3.0);
    assert!(o.peak_in(0.0, 0.1) > 10.0 * o.peak_in(1.9, 2.0));
    assert!(o.last_above(1e-4) <= 2.05, "impact tail {}", o.last_above(1e-4));
}

#[test]
fn comete_note_pitch_offsets_the_effect() {
    let dev = device("comete", &[("length", 4.0), ("noise", 0.0), ("space", 0.0), ("drive", 0.0), ("pitch", 12.0)], &[("kind", "subdrop")]);
    let lo = note(&dev, 120.0, 60, 0.05, 2.0);
    let hi = note(&dev, 120.0, 72, 0.05, 2.0);
    let (a, b) = (pitch_at(&lo, 0.05, 0.12), pitch_at(&hi, 0.05, 0.12));
    assert!((b / a - 2.0).abs() < 0.1, "an octave up should double the pitch ({a} -> {b})");
    // The sub drop falls.
    let late = pitch_at(&lo, 1.5, 1.8);
    assert!(late < a * 0.6, "sub drop should fall ({a} -> {late})");
}

// ------------------------------------------------------------------ presets

fn preset_render(p: &presets::Preset) -> Out {
    let dev = p.device();
    match p.kind {
        "comete" => {
            let beats = dev.param("length") as f32;
            note(&dev, 120.0, 60, 0.1, beats * 0.5 + 5.0)
        }
        "dedale" => {
            let key = if dev.option("voice") == "bass" { 40 } else { 60 };
            note(&dev, 120.0, key, 4.0, 9.0)
        }
        _ => {
            let key = if p.tags.contains("bass") { 36 } else { 60 };
            let release = dev.param("release") as f32;
            note(&dev, 120.0, key, 2.0, 2.5 + release * 2.5)
        }
    }
}

#[test]
fn presets_render_within_bounds() {
    let verbose = std::env::var("PRESET_LEVELS").is_ok();
    for kind in ["cuivre", "dedale", "comete"] {
        let list = presets_of(kind);
        assert_eq!(list.len(), 6, "{kind}: expected 6 presets");
        let spec = rosaclef_core::catalog::device(kind).expect("catalog entry");
        for p in list {
            // Presets only list what differs from the defaults.
            for (k, v) in p.params {
                let default = spec.param(k).unwrap_or_else(|| panic!("{}: unknown param {k}", p.name)).default;
                assert!((v - default).abs() > 1e-9, "{}: {k} = default", p.name);
            }
            for (k, v) in p.options {
                let default = spec.option(k).unwrap_or_else(|| panic!("{}: unknown option {k}", p.name)).default;
                assert_ne!(*v, default, "{}: option {k} = default", p.name);
            }
            let o = preset_render(p);
            if verbose {
                println!("{:<8} {:<28} peak {:.3} rms {:.3}", kind, p.name, o.peak(), o.rms_in(0.0, o.l.len() as f32 / SR));
            }
            check_clean(p.name, &o, 0.15, 0.8);
        }
    }
}

// ------------------------------------------------------------------ performance

fn realtime_factor(dev: &Device, keys: &[u8], seconds: f32) -> f64 {
    let mut inst = create(dev, &Ctx { sr: SR, bpm: 120.0 }).unwrap();
    let events: Vec<NoteEvent> = keys.iter().map(|&k| NoteEvent { offset: 0, kind: NoteKind::On { key: k, velocity: 0.8 } }).collect();
    let frames = secs(seconds);
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let start = std::time::Instant::now();
    let mut done = 0;
    let mut first = true;
    while done < frames {
        l.fill(0.0);
        r.fill(0.0);
        inst.process(if first { &events } else { &[] }, &mut l, &mut r);
        first = false;
        done += BLOCK;
    }
    std::hint::black_box(&l);
    seconds as f64 / start.elapsed().as_secs_f64()
}

#[test]
#[ignore = "wall-clock timing; flaky on a loaded machine (run with --ignored)"]
fn realtime_performance() {
    let keys = [36u8, 43, 48, 55, 60, 64, 67, 72];
    let cases = [
        ("cuivre ladder", device("cuivre", &[("drift", 0.5)], &[])),
        ("cuivre bright", device("cuivre", &[("cutoff", 9000.0), ("resonance", 0.8)], &[])),
        ("cuivre screamer", device("cuivre", &[("noise", 0.3)], &[("filter", "screamer")])),
        ("dedale pluck", device("dedale", &[("density", 1.0), ("pulses", 16.0), ("decay", 2.0)], &[])),
        ("dedale bell", device("dedale", &[("density", 1.0), ("pulses", 16.0), ("decay", 2.0)], &[("voice", "bell")])),
        ("dedale perc", device("dedale", &[("density", 1.0), ("pulses", 16.0), ("decay", 2.0)], &[("voice", "perc")])),
        ("comete riser", device("comete", &[("length", 16.0)], &[])),
        ("comete impact", device("comete", &[("length", 16.0)], &[("kind", "impact")])),
    ];
    for (name, dev) in cases {
        let rt = realtime_factor(&dev, &keys, 5.0);
        println!("{name:<16} 8 notes x 5 s: {rt:.0}x realtime");
        assert!(rt > 20.0, "{name}: only {rt:.1}x realtime");
    }
}

