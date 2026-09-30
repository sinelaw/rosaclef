//! Voice to notes: turns a recorded take into music.
//!
//! - **Melody** — a sung, hummed or whistled line becomes notes. The pitch is
//!   tracked every 10 ms on a ~16 kHz copy of the take by YIN (de Cheveigné
//!   & Kawahara, 2002) and checked against the spectrum (subharmonic
//!   summation): a frame counts as sung when YIN is sure of it or both
//!   agree. The sung stretches are cut into syllables where the level dips
//!   and comes back, and each syllable into notes by a dynamic-programming
//!   fit of whole semitones with a cost per note change, so glides, scoops
//!   and vibrato do not turn into little notes of their own. The fit is
//!   made at five detail levels, in the singer's own tuning.
//! - **Beatbox** — vocal percussion becomes drum hits. Onsets are peaks of the
//!   spectral flux; the first 50 ms of each hit are summed up by a *tone*
//!   (the spectral centroid, pulled down by the share of energy below
//!   200 Hz), which separates kicks (a low "b" or "boom"), snares ("pf",
//!   "k") and hats ("ts", "t").
//!
//! The results are raw — seconds and fractional MIDI pitches. The studio
//! quantizes them, snaps them to a scale and maps them onto channels
//! (`web/src/voice.js`), so those settings change instantly without
//! analyzing the take again. `classify` is mirrored there too.

use rosaclef_engine::samples::SampleData;
use serde::Serialize;

/// Seconds between analysis frames (the pitch contour and the level).
pub const STEP: f32 = 0.01;
/// The pitch range followed: C2 (hummed) to about C7 (whistled).
const FMIN: f32 = 62.0;
const FMAX: f32 = 2200.0;
/// Notes shorter than this are dropped (clicks, breaths); the studio's
/// clean-up merges or drops longer blips.
const MIN_NOTE: f32 = 0.05;

/// Default tone boundaries between the drums (Hz).
pub const KICK_BELOW: f32 = 900.0;
pub const HAT_ABOVE: f32 = 4200.0;

/// A note found in a melody take.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct VoiceNote {
    /// Seconds from the start of the take.
    pub start: f32,
    pub end: f32,
    /// Fractional MIDI pitch (the median of the note's steady part).
    pub pitch: f32,
    /// 0..1, from the note's level relative to the loudest note.
    pub velocity: f32,
}

/// A percussive hit found in a beatbox take.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Hit {
    /// Seconds from the start of the take.
    pub time: f32,
    /// Onset strength relative to the strongest hit (0..1); the studio's
    /// sensitivity control drops the weak ones.
    pub strength: f32,
    /// 0..1, from the hit's peak level relative to the loudest hit.
    pub velocity: f32,
    /// Spectral centroid of the hit's first 50 ms (Hz).
    pub centroid: f32,
    /// Share of the energy below 200 Hz and above 5 kHz.
    pub low: f32,
    pub high: f32,
    /// What `classify` sorts on (Hz).
    pub tone: f32,
    /// "kick", "snare" or "hat" with the default boundaries.
    pub kind: &'static str,
}

/// What a take contains.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    /// "melody" or "drums".
    pub mode: &'static str,
    /// Length of the take (s).
    pub duration: f32,
    /// Seconds per value of `contour` and `level`.
    pub step: f32,
    /// Level per frame, 0..1 relative to the loudest frame.
    pub level: Vec<f32>,
    /// Melody: fractional MIDI pitch per frame, 0 where there is no pitch.
    pub contour: Vec<f32>,
    /// Melody: the notes at the default detail level.
    pub notes: Vec<VoiceNote>,
    /// Melody: the notes at every detail level (`DETAIL_CHANGES`), from
    /// the smoothest to the most detailed.
    pub details: Vec<Vec<VoiceNote>>,
    pub hits: Vec<Hit>,
}

/// Analyze a take: `mode` is "melody" (singing, humming, whistling) or
/// "drums" (beatbox).
pub fn transcribe(data: &SampleData, mode: &str) -> Transcription {
    let x = mono(data);
    let sr = data.sample_rate.max(1.0);
    let duration = x.len() as f32 / sr;
    let level = levels(&x, sr);
    if mode == "drums" {
        Transcription {
            mode: "drums",
            duration,
            step: STEP,
            level,
            contour: vec![],
            notes: vec![],
            details: vec![],
            hits: beatbox(&x, sr),
        }
    } else {
        let (contour, details) = melody(&x, sr);
        Transcription {
            mode: "melody",
            duration,
            step: STEP,
            level,
            contour,
            notes: details[DEFAULT_DETAIL].clone(),
            details,
            hits: vec![],
        }
    }
}

/// The drum a tone stands for, given the two boundaries (Hz).
pub fn classify(tone: f32, kick_below: f32, hat_above: f32) -> &'static str {
    if tone < kick_below {
        "kick"
    } else if tone > hat_above {
        "hat"
    } else {
        "snare"
    }
}

// ------------------------------------------------------------------ common

fn mono(d: &SampleData) -> Vec<f32> {
    match d.channels.len() {
        0 => vec![],
        1 => d.channels[0].clone(),
        n => (0..d.len())
            .map(|i| d.channels.iter().map(|c| c[i]).sum::<f32>() / n as f32)
            .collect(),
    }
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// RMS level every `STEP` over a 20 ms window centered on the frame.
fn rms_frames(x: &[f32], sr: f32) -> Vec<f32> {
    let hop = STEP * sr;
    let half = (0.01 * sr) as usize;
    let n = (x.len() as f32 / hop).ceil() as usize;
    (0..n)
        .map(|i| {
            let c = (i as f32 * hop) as usize;
            rms(&x[c.saturating_sub(half)..(c + half).min(x.len())])
        })
        .collect()
}

/// Frame levels relative to the loudest frame (for drawing).
fn levels(x: &[f32], sr: f32) -> Vec<f32> {
    let r = rms_frames(x, sr);
    let max = r.iter().cloned().fold(0.0, f32::max);
    r.iter()
        .map(|v| if max > 0.0 { round3(v / max) } else { 0.0 })
        .collect()
}

fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Low-pass (windowed sinc) and keep every `factor`-th sample.
fn decimate(x: &[f32], factor: usize) -> Vec<f32> {
    if factor <= 1 {
        return x.to_vec();
    }
    let m = 8 * factor;
    let fc = 0.45 / factor as f32;
    let taps: Vec<f32> = (0..=2 * m)
        .map(|j| {
            let t = j as f32 - m as f32;
            let sinc = if t == 0.0 {
                2.0 * fc
            } else {
                (2.0 * std::f32::consts::PI * fc * t).sin() / (std::f32::consts::PI * t)
            };
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * j as f32 / (2 * m) as f32).cos();
            sinc * w
        })
        .collect();
    (0..x.len() / factor)
        .map(|k| {
            let c = (k * factor) as isize;
            let mut acc = 0.0;
            for (j, h) in taps.iter().enumerate() {
                let i = c + j as isize - m as isize;
                if i >= 0 && (i as usize) < x.len() {
                    acc += h * x[i as usize];
                }
            }
            acc
        })
        .collect()
}

fn hz_to_midi(f: f32) -> f32 {
    69.0 + 12.0 * (f / 440.0).log2()
}

// ------------------------------------------------------------------ melody

/// YIN pitch per frame: (frequency in Hz, aperiodicity 0..1). The
/// frequency is that of the best dip even when the frame is not periodic
/// (0 only where the frame does not fit the take); voicing is decided later.
fn yin(x: &[f32], sr: f32, frames: usize) -> Vec<(f32, f32)> {
    let tau_min = ((sr / FMAX) as usize).max(2);
    let tau_max = (sr / FMIN).ceil() as usize;
    let w = tau_max;
    let hop = STEP * sr;
    let mut dn = vec![1f32; tau_max + 2];
    (0..frames)
        .map(|i| {
            let center = (i as f32 * hop) as isize;
            let start = center - ((w + tau_max) / 2) as isize;
            if start < 0 || start as usize + w + tau_max + 1 > x.len() {
                return (0.0, 1.0);
            }
            let s = &x[start as usize..start as usize + w + tau_max + 1];
            let mut running = 0.0;
            for tau in 1..=tau_max + 1 {
                let mut acc = 0.0;
                for j in 0..w {
                    let v = s[j] - s[j + tau];
                    acc += v * v;
                }
                running += acc;
                dn[tau] = if running > 0.0 {
                    acc * tau as f32 / running
                } else {
                    1.0
                };
            }
            // The first dip under the threshold, followed to its bottom;
            // otherwise the deepest dip.
            let mut best = None;
            let mut tau = tau_min;
            while tau < tau_max {
                if dn[tau] < 0.15 {
                    while tau + 1 < tau_max && dn[tau + 1] < dn[tau] {
                        tau += 1;
                    }
                    best = Some(tau);
                    break;
                }
                tau += 1;
            }
            let tau = best.unwrap_or_else(|| {
                (tau_min..tau_max)
                    .min_by(|&a, &b| {
                        dn[a]
                            .partial_cmp(&dn[b])
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(tau_min)
            });
            // Parabolic interpolation around the dip.
            let (a, b, c) = (dn[tau - 1], dn[tau], dn[tau + 1]);
            let den = a - 2.0 * b + c;
            let shift = if den.abs() > 1e-9 {
                ((a - c) / (2.0 * den)).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            (sr / (tau as f32 + shift), b.clamp(0.0, 1.0))
        })
        .collect()
}

/// A second opinion from the spectrum: the pitch (fractional MIDI, 0 for a
/// silent frame) whose harmonics hold the most energy — subharmonic
/// summation (Hermes, 1988), with the half-harmonics subtracted so that
/// an octave down does not win.
fn harmonic_sum(x: &[f32], sr: f32, frames: usize) -> Vec<f32> {
    let n = ((0.064 * sr) as usize).next_power_of_two();
    let tw = twiddles(n);
    let win: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect();
    let (mut re, mut im) = (vec![], vec![]);
    let bin = sr / n as f32;
    let lo = hz_to_midi(FMIN);
    let hi = hz_to_midi(FMAX);
    let candidates: Vec<(f32, f32)> = (0..)
        .map(|k| lo + k as f32 * 0.1)
        .take_while(|m| *m <= hi)
        .map(|m| (m, 440.0 * 2f32.powf((m - 69.0) / 12.0)))
        .collect();
    let hop = STEP * sr;
    let mut seg = vec![0f32; n];
    (0..frames)
        .map(|i| {
            let center = (i as f32 * hop) as isize;
            for (j, v) in seg.iter_mut().enumerate() {
                let k = center - (n / 2) as isize + j as isize;
                *v = if k >= 0 && (k as usize) < x.len() {
                    x[k as usize]
                } else {
                    0.0
                };
            }
            let p = power(&seg, &win, n, &tw, &mut re, &mut im);
            let peak = p.iter().cloned().fold(0.0, f32::max).sqrt();
            if peak <= 1e-6 {
                return 0.0;
            }
            let a: Vec<f32> = p
                .iter()
                .map(|v| (1.0 + 20.0 * v.sqrt() / peak).ln())
                .collect();
            let at = |f: f32| {
                let k = f / bin;
                let k0 = k.floor() as usize;
                if k0 + 1 >= a.len() {
                    return 0.0;
                }
                let t = k - k0 as f32;
                a[k0] * (1.0 - t) + a[k0 + 1] * t
            };
            let mut best = (0.0f32, f32::MIN);
            for &(m, f0) in &candidates {
                let mut score = 0.0;
                let mut w = 1.0;
                for h in 1..=8 {
                    let f = f0 * h as f32;
                    if f >= sr / 2.0 {
                        break;
                    }
                    score += w * (at(f) - 0.5 * at(f - 0.5 * f0));
                    w *= 0.86;
                }
                if score > best.1 {
                    best = (m, score);
                }
            }
            best.0
        })
        .collect()
}

/// Median of the values in `run` within `half` frames of `i`.
fn median_around(v: &[f32], run: (usize, usize), i: usize, half: usize) -> f32 {
    let a = i.saturating_sub(half).max(run.0);
    let b = (i + half + 1).min(run.1);
    median(&mut v[a..b].to_vec())
}

/// Most a frame can cost in `fit_notes` (squared semitones): a frame far
/// from every note (mid-glide, an octave slip) should not decide much.
const CAP: f32 = 1.5;
/// The cost of a note change in `fit_notes` (squared semitones) at each
/// detail level, from "smooth" to "every note". A higher cost absorbs
/// more glides and ornaments; a lower one keeps quicker notes.
pub const DETAIL_CHANGES: [f32; 5] = [16.0, 11.0, 8.0, 5.5, 4.0];
/// The detail level of `Transcription::notes`: no stray notes on the test
/// phrases (see the tests), at the price of merging very quick runs.
pub const DEFAULT_DETAIL: usize = 2;

/// The whole-semitone notes that best follow a stretch of pitches: the
/// piecewise-constant fit minimizing the (capped, clarity-weighted) squared
/// distance to the pitches plus `change` per note change — dynamic
/// programming over the semitones in range. Glides, scoops and vibrato cost
/// more as extra notes than they save; a real note, even a quick one, saves
/// far more than a change costs.
fn fit_notes(p: &[f32], weight: &[f32], change: f32) -> Vec<i32> {
    if p.is_empty() {
        return vec![];
    }
    let lo = p.iter().cloned().fold(f32::MAX, f32::min).floor() as i32 - 1;
    let hi = p.iter().cloned().fold(f32::MIN, f32::max).ceil() as i32 + 1;
    let n = (hi - lo + 1) as usize;
    let dist = |v: f32, s: usize| {
        let d = v - (lo + s as i32) as f32;
        (d * d).min(CAP)
    };
    let mut cost: Vec<f32> = (0..n)
        .map(|s| weight[0].max(0.05) * dist(p[0], s))
        .collect();
    // For each frame after the first: whether a state was entered by a
    // change, and from which state the cheapest change came.
    let mut switched = vec![false; p.len() * n];
    let mut from = vec![0usize; p.len()];
    for t in 1..p.len() {
        let (best, best_cost) =
            cost.iter().enumerate().fold(
                (0, f32::MAX),
                |acc, (s, &c)| if c < acc.1 { (s, c) } else { acc },
            );
        from[t] = best;
        let w = weight[t].max(0.05);
        for s in 0..n {
            let stay = cost[s];
            let jump = best_cost + change;
            if jump < stay {
                cost[s] = jump;
                switched[t * n + s] = true;
            }
            cost[s] += w * dist(p[t], s);
        }
    }
    let mut s = cost
        .iter()
        .enumerate()
        .fold(
            (0, f32::MAX),
            |acc, (s, &c)| if c < acc.1 { (s, c) } else { acc },
        )
        .0;
    let mut out = vec![0i32; p.len()];
    for t in (0..p.len()).rev() {
        out[t] = lo + s as i32;
        if t > 0 && switched[t * n + s] {
            s = from[t];
        }
    }
    out
}

/// Pitch contour (fractional MIDI per frame, 0 = none) and the notes at
/// each detail level (`DETAIL_CHANGES`).
fn melody(x: &[f32], sr: f32) -> (Vec<f32>, Vec<Vec<VoiceNote>>) {
    let factor = ((sr / 16000.0).floor() as usize).max(1);
    let y = decimate(x, factor);
    let ysr = sr / factor as f32;
    let frames = (x.len() as f32 / sr / STEP).ceil() as usize;
    let yin = yin(&y, ysr, frames);
    let spectral = harmonic_sum(&y, ysr, frames);
    let level = rms_frames(&y, ysr);
    let level_at = |i: usize| level.get(i).copied().unwrap_or(0.0);
    let loud = level.iter().cloned().fold(0.0, f32::max);
    // A sung stretch must reach -26 dB under the loudest frame and lasts
    // while it stays above -32 dB.
    let (gate_on, gate_off) = ((loud * 0.05).max(0.003), (loud * 0.025).max(0.002));

    // A frame is sung when YIN is sure of it, or fairly sure and the
    // spectrum agrees on the pitch (within a semitone).
    let mut clarity = vec![0f32; frames];
    let raw: Vec<Option<f32>> = (0..frames)
        .map(|i| {
            let (f, ap) = yin[i];
            if f <= 0.0 || level_at(i) <= gate_off {
                return None;
            }
            let m = hz_to_midi(f);
            let agree = spectral[i] > 0.0 && (m - spectral[i]).abs() < 1.0;
            let sung = ap < 0.1 || (agree && ap < 0.4);
            clarity[i] = (1.0 - ap) * if agree { 1.0 } else { 0.6 };
            sung.then_some(m)
        })
        .collect();

    // Runs of sung frames that get loud enough.
    let mut runs: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < frames {
        if raw[i].is_none() {
            i += 1;
            continue;
        }
        let a = i;
        while i < frames && raw[i].is_some() {
            i += 1;
        }
        if (a..i).any(|k| level_at(k) > gate_on) {
            runs.push((a, i));
        }
    }

    // The pitch with single-frame octave slips removed (a median of 5).
    let vals: Vec<f32> = raw.iter().map(|p| p.unwrap_or(0.0)).collect();
    let mut fine = vec![0f32; frames];
    for &run in &runs {
        for (k, f) in fine.iter_mut().enumerate().take(run.1).skip(run.0) {
            *f = median_around(&vals, run, k, 2);
        }
    }

    // Cut each run into syllables where the level dips and comes back
    // (a new syllable on the same pitch).
    let mut pieces: Vec<(usize, usize)> = vec![];
    for &(a, b) in &runs {
        let mut start = a;
        let mut peak = level_at(a);
        let mut fall: Option<usize> = None;
        let mut dip = 0.0f32;
        for k in a + 1..b {
            let l = level_at(k);
            if let Some(f) = fall {
                if l > dip * 2.5 && l > peak * 0.25 {
                    pieces.push((start, f));
                    start = k;
                    peak = l;
                    fall = None;
                    continue;
                }
            }
            if l < peak * 0.5 {
                if fall.is_none() {
                    fall = Some(k);
                    dip = l;
                }
                dip = dip.min(l);
            } else {
                fall = None;
            }
            peak = peak.max(l);
        }
        pieces.push((start, b));
    }
    pieces.retain(|(a, b)| b > a);

    // Frames where the pitch moves fast (a glide or a scoop, faster than
    // vibrato) say little about which note is sung.
    let weight: Vec<f32> = (0..frames)
        .map(|k| {
            let Some(&(pa, pb)) = pieces.iter().find(|(a, b)| k >= *a && k < *b) else {
                return 0.0;
            };
            let (u, v) = (k.saturating_sub(2).max(pa), (k + 2).min(pb - 1));
            let slope = if v > u {
                (fine[v] - fine[u]).abs() / (v - u) as f32
            } else {
                0.0
            };
            clarity[k] / (1.0 + (slope / 0.12).powi(2))
        })
        .collect();
    // Then each syllable into notes with the piecewise-constant fit, for a
    // singer `tuning` semitones off equal temperament.
    let segment = |tuning: f32, change: f32| {
        let mut spans: Vec<(usize, usize)> = vec![];
        for &(pa, pb) in &pieces {
            let tuned: Vec<f32> = fine[pa..pb].iter().map(|p| p - tuning).collect();
            let states = fit_notes(&tuned, &weight[pa..pb], change);
            let mut from = pa;
            for k in pa + 1..=pb {
                if k == pb || states[k - pa] != states[k - pa - 1] {
                    spans.push((from, k));
                    from = k;
                }
            }
        }
        spans
    };
    let middle = |a: usize, b: usize| {
        let trim = if b - a >= 10 { (b - a) / 5 } else { 0 };
        median(&mut fine[a + trim..b - trim].to_vec())
    };
    // The singer's own tuning, from the notes found in equal temperament:
    // how far their steady pitches sit from the semitones (a circular mean
    // weighted by length), used when the notes agree on it.
    let first = segment(0.0, DETAIL_CHANGES[DEFAULT_DETAIL]);
    let (mut cs, mut sn, mut total) = (0.0f32, 0.0f32, 0.0f32);
    for &(a, b) in &first {
        let p = middle(a, b);
        let ang = 2.0 * std::f32::consts::PI * (p - p.round());
        let w = (b - a) as f32;
        cs += w * ang.cos();
        sn += w * ang.sin();
        total += w;
    }
    let agreement = if total > 0.0 {
        (cs * cs + sn * sn).sqrt() / total
    } else {
        0.0
    };
    let offset = sn.atan2(cs) / (2.0 * std::f32::consts::PI);
    // At least three notes and a second of singing, or it is not a habit.
    let enough = first.len() >= 3 && total * STEP >= 1.0;
    let tuning = if enough && agreement > 0.7 && offset.abs() > 0.1 {
        offset
    } else {
        0.0
    };

    struct Span {
        a: usize,
        b: usize,
        peak: f32,
        pitch: f32,
    }
    let notes_for = |spans: Vec<(usize, usize)>| -> Vec<VoiceNote> {
        let spans: Vec<Span> = spans
            .into_iter()
            .map(|(a, b)| Span {
                a,
                b,
                peak: (a..b).map(level_at).fold(0.0, f32::max),
                // The pitch of the steady middle (glides in and out trimmed).
                pitch: middle(a, b) - tuning,
            })
            .collect();
        // A short, quiet stretch pressed against a much louder note is its
        // tail or the consonant between two syllables, not a note.
        let tail = |i: usize| {
            let s = &spans[i];
            let touching = |j: usize| {
                let o = &spans[j];
                (o.a.saturating_sub(s.b) <= 3 || s.a.saturating_sub(o.b) <= 3)
                    && o.peak > s.peak * 4.0
            };
            (s.b - s.a) as f32 * STEP < 0.15
                && ((i > 0 && touching(i - 1)) || (i + 1 < spans.len() && touching(i + 1)))
        };
        let keep: Vec<bool> = (0..spans.len()).map(|i| !tail(i)).collect();
        let loudest = spans.iter().map(|s| s.peak).fold(0.0, f32::max).max(1e-9);
        spans
            .iter()
            .zip(keep)
            .filter(|(s, keep)| *keep && (s.b - s.a) as f32 * STEP >= MIN_NOTE)
            .map(|(s, _)| {
                let db = 20.0 * (s.peak / loudest).max(1e-6).log10();
                VoiceNote {
                    start: round3(s.a as f32 * STEP),
                    end: round3(s.b as f32 * STEP),
                    pitch: (s.pitch * 100.0).round() / 100.0,
                    velocity: round3((0.35 + 0.65 * (1.0 + db / 30.0)).clamp(0.2, 1.0)),
                }
            })
            .collect()
    };
    let details: Vec<Vec<VoiceNote>> = DETAIL_CHANGES
        .iter()
        .enumerate()
        .map(|(i, &change)| {
            if i == DEFAULT_DETAIL && tuning == 0.0 {
                notes_for(first.clone())
            } else {
                notes_for(segment(tuning, change))
            }
        })
        .collect();
    let contour = fine.iter().map(|v| (v * 100.0).round() / 100.0).collect();
    (contour, details)
}

// ------------------------------------------------------------------ beatbox

/// In-place radix-2 FFT (`re.len()` a power of two).
fn fft(re: &mut [f32], im: &mut [f32], twiddle: &[(f32, f32)]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = n / len;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = twiddle[k * step];
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * wr - im[b] * wi;
                let ti = re[b] * wi + im[b] * wr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

fn twiddles(n: usize) -> Vec<(f32, f32)> {
    (0..n / 2)
        .map(|k| {
            let a = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
            (a.cos() as f32, a.sin() as f32)
        })
        .collect()
}

/// Power spectrum (bins 0..=n/2) of `x` windowed by `win`, zero-padded to n.
fn power(
    x: &[f32],
    win: &[f32],
    n: usize,
    tw: &[(f32, f32)],
    re: &mut Vec<f32>,
    im: &mut Vec<f32>,
) -> Vec<f32> {
    re.clear();
    re.resize(n, 0.0);
    im.clear();
    im.resize(n, 0.0);
    for (i, v) in x.iter().take(n).enumerate() {
        re[i] = v * win[i.min(win.len() - 1)];
    }
    fft(re, im, tw);
    (0..=n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}

/// Onset flux a hit needs at least (keeps steady tones out).
const FLUX_FLOOR: f32 = 0.5;
/// Band edges (Hz) for the onset flux.
const BANDS: [f32; 4] = [250.0, 1000.0, 4000.0, 9000.0];

/// Onset times (s) and strengths from the spectral flux.
fn onsets(x: &[f32], sr: f32) -> Vec<(f32, f32)> {
    let n = if sr > 60000.0 { 4096 } else { 2048 };
    let hop = ((0.005 * sr) as usize).max(1);
    let tw = twiddles(n);
    let win: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect();
    let (mut re, mut im) = (vec![], vec![]);
    let frames = if x.len() > n {
        (x.len() - n) / hop + 1
    } else {
        0
    };
    // The flux is averaged per band, then summed: a kick moves a few low
    // bins, a hat hundreds of high ones, and both should count alike.
    let band_of: Vec<usize> = (0..=n / 2)
        .map(|k| {
            BANDS
                .iter()
                .filter(|&&edge| k as f32 * sr / n as f32 >= edge)
                .count()
        })
        .collect();
    let mut width = vec![0f32; BANDS.len() + 1];
    for &b in &band_of {
        width[b] += 1.0;
    }
    // Each frame is compared with the one two hops back, widened over
    // neighbouring bins ("SuperFlux", Böck & Widmer 2013): the ripple of a
    // decaying tail then stays under it, while a new hit rises above.
    let bins = n / 2 + 1;
    let mut back: [Vec<f32>; 2] = [vec![0.0; bins], vec![0.0; bins]];
    let mut rise = vec![0f32; BANDS.len() + 1];
    let mut flux = Vec::with_capacity(frames);
    let mut energy = Vec::with_capacity(frames);
    for f in 0..frames {
        let seg = &x[f * hop..f * hop + n];
        let p = power(seg, &win, n, &tw, &mut re, &mut im);
        rise.iter_mut().for_each(|r| *r = 0.0);
        let cur: Vec<f32> = p.iter().map(|v| (1.0 + 10.0 * v.sqrt()).ln()).collect();
        let reference = &back[f % 2];
        for k in 0..bins {
            let r = reference[k.saturating_sub(1)]
                .max(reference[k])
                .max(reference[(k + 1).min(bins - 1)]);
            rise[band_of[k]] += (cur[k] - r).max(0.0);
        }
        back[f % 2] = cur;
        let acc: f32 = rise
            .iter()
            .zip(&width)
            .map(|(r, w)| if *w > 0.0 { r / w } else { 0.0 })
            .sum();
        flux.push(if f < 2 { 0.0 } else { acc });
        energy.push(rms(seg));
    }
    let max_flux = flux.iter().cloned().fold(0.0, f32::max);
    let loud = energy.iter().cloned().fold(0.0, f32::max);
    if max_flux <= 0.0 {
        return vec![];
    }
    // A peak: the largest within ±30 ms, above the local mean by a margin,
    // with some level, and 60 ms after the previous onset.
    let around = ((0.03 * sr) as usize / hop).max(1);
    let (before, after) = ((0.1 * sr) as usize / hop, (0.05 * sr) as usize / hop);
    let gap = 0.06 * sr / hop as f32;
    let mut out: Vec<(usize, f32)> = vec![];
    for f in 1..frames {
        let v = flux[f];
        let lo = f.saturating_sub(around);
        let hi = (f + around + 1).min(frames);
        if flux[lo..hi].iter().any(|&u| u > v) {
            continue;
        }
        let (a, b) = (f.saturating_sub(before), (f + after + 1).min(frames));
        let mean = flux[a..b].iter().sum::<f32>() / (b - a) as f32;
        // The floor keeps steady tones (tiny wobbles in the flux) out.
        if v < mean * 1.3 + 0.02 * max_flux || v < FLUX_FLOOR {
            continue;
        }
        let e = energy[f..(f + around).min(frames)]
            .iter()
            .cloned()
            .fold(0.0, f32::max);
        if e < loud * 0.03 {
            continue;
        }
        if let Some(&(last, _)) = out.last() {
            if ((f - last) as f32) < gap {
                continue;
            }
        }
        out.push((f, v / max_flux));
    }
    // Refine each onset in the waveform: the start of the rise into the
    // loudest millisecond of the frame.
    let ms = ((0.001 * sr) as usize).max(1);
    out.into_iter()
        .map(|(f, s)| {
            let a = f * hop;
            let b = (a + n).min(x.len());
            let env: Vec<f32> = x[a..b]
                .chunks(ms)
                .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
                .collect();
            let (peak_at, peak) =
                env.iter().enumerate().fold(
                    (0, 0.0f32),
                    |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc },
                );
            let mut at = peak_at;
            while at > 0 && env[at - 1] > peak * 0.25 {
                at -= 1;
            }
            ((a + at * ms) as f32 / sr, s)
        })
        .collect()
}

/// Onsets described and classified.
fn beatbox(x: &[f32], sr: f32) -> Vec<Hit> {
    // Onsets are found on a copy peaking at -6 dB, whatever the input gain.
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak <= 1e-6 {
        return vec![];
    }
    let found = onsets(&x.iter().map(|v| v * 0.5 / peak).collect::<Vec<f32>>(), sr);
    let len = (0.05 * sr) as usize;
    let n = len.next_power_of_two();
    let tw = twiddles(n);
    // A flat window with a short fade at the end.
    let fade = len / 8;
    let win: Vec<f32> = (0..len)
        .map(|i| {
            if i + fade < len {
                1.0
            } else {
                (len - i) as f32 / fade as f32
            }
        })
        .collect();
    let (mut re, mut im) = (vec![], vec![]);
    let mut hits: Vec<Hit> = vec![];
    let mut peaks: Vec<f32> = vec![];
    for (i, &(t, strength)) in found.iter().enumerate() {
        let a = (t * sr) as usize;
        // The segment stops at the next onset.
        let next = found
            .get(i + 1)
            .map(|&(u, _)| (u * sr) as usize)
            .unwrap_or(x.len());
        let b = (a + len).min(next).min(x.len());
        if b <= a {
            continue;
        }
        let seg = &x[a..b];
        let p = power(seg, &win, n, &tw, &mut re, &mut im);
        let bin = sr / n as f32;
        let (mut total, mut low, mut high, mut moment) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (k, &v) in p.iter().enumerate().skip(1) {
            let f = k as f32 * bin;
            let v = v as f64;
            total += v;
            moment += v * f as f64;
            if f < 200.0 {
                low += v;
            } else if f > 5000.0 {
                high += v;
            }
        }
        if total <= 0.0 {
            continue;
        }
        let centroid = (moment / total) as f32;
        let low = (low / total) as f32;
        let high = (high / total) as f32;
        let tone = centroid * (1.0 - low);
        peaks.push(seg.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        hits.push(Hit {
            time: round3(t),
            strength: round3(strength),
            velocity: 0.0,
            centroid: centroid.round(),
            low: round3(low),
            high: round3(high),
            tone: tone.round(),
            kind: classify(tone, KICK_BELOW, HAT_ABOVE),
        });
    }
    let loudest = peaks.iter().cloned().fold(0.0, f32::max).max(1e-9);
    for (h, p) in hits.iter_mut().zip(peaks) {
        let db = 20.0 * (p / loudest).max(1e-6).log10();
        h.velocity = round3((0.4 + 0.6 * (1.0 + db / 24.0)).clamp(0.25, 1.0));
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: f32 = 48000.0;

    fn take(x: Vec<f32>, sr: f32) -> SampleData {
        SampleData {
            sample_rate: sr,
            channels: vec![x],
        }
    }

    /// A voice-like tone: harmonics falling off, a little vibrato, soft edges.
    fn sing(out: &mut Vec<f32>, sr: f32, at: f32, dur: f32, midi: f32, amp: f32) {
        let f0 = 440.0 * 2f32.powf((midi - 69.0) / 12.0);
        let a = (at * sr) as usize;
        let n = (dur * sr) as usize;
        if out.len() < a + n {
            out.resize(a + n, 0.0);
        }
        let mut phase = 0.0f32;
        for i in 0..n {
            let t = i as f32 / sr;
            let vib = 1.0 + 0.006 * (2.0 * PI * 5.5 * t).sin();
            phase += 2.0 * PI * f0 * vib / sr;
            let edge = (t / 0.02).min(1.0).min((dur - t) / 0.02).max(0.0);
            let mut v = 0.0;
            for h in 1..=6 {
                v += (phase * h as f32).sin() / (h * h) as f32;
            }
            out[a + i] += amp * edge * v;
        }
    }

    fn rounded(notes: &[VoiceNote]) -> Vec<i32> {
        notes.iter().map(|n| n.pitch.round() as i32).collect()
    }

    /// A take closer to a real singer: a vowel-like spectrum (the
    /// fundamental weaker than the next harmonics), scoops into the notes,
    /// wide vibrato, pitch jitter and shimmer, legato glides between notes,
    /// consonant bursts, breaths, a pitch drop at phrase ends, room noise
    /// and mains hum. `notes` are (midi, seconds); 0 is a rest.
    fn singer(sr: f32, notes: &[(f32, f32)], seed: u32) -> Vec<f32> {
        singer_with(sr, notes, seed, 0.0, 0.45)
    }

    /// `singer`, sung `detune` semitones off and with vibrato of `vibrato`
    /// semitones.
    fn singer_with(
        sr: f32,
        notes: &[(f32, f32)],
        seed: u32,
        detune: f32,
        vibrato: f32,
    ) -> Vec<f32> {
        let mut rng = Rng(seed);
        let lead = 0.4;
        let total: f32 = lead + notes.iter().map(|n| n.1).sum::<f32>() + 0.6;
        let n = (total * sr) as usize;
        let mut out = vec![0.0f32; n];
        // Room noise (about -50 dB) and 50 Hz hum.
        for (i, v) in out.iter_mut().enumerate() {
            *v += 0.001 * rng.noise() + 0.0015 * (2.0 * PI * 50.0 * i as f32 / sr).sin();
        }
        // A breath before the phrase.
        for i in 0..(0.25 * sr) as usize {
            let t = i as f32 / sr;
            out[(0.05 * sr) as usize + i] += 0.012 * rng.noise() * (PI * t / 0.25).sin();
        }
        let mut phase = 0.0f32;
        let mut t = lead;
        let mut jitter = 0.0f32;
        for (k, &(midi, dur)) in notes.iter().enumerate() {
            if midi <= 0.0 {
                t += dur;
                continue;
            }
            let prev = if k > 0 { notes[k - 1].0 } else { 0.0 };
            let next = notes.get(k + 1).map(|n| n.0).unwrap_or(0.0);
            // A consonant before notes that follow a rest or start the phrase.
            if prev <= 0.0 {
                let a = ((t - 0.05) * sr) as usize;
                for i in 0..(0.04 * sr) as usize {
                    let v = rng.noise();
                    out[a + i] += 0.03 * (v - 0.7 * rng.noise()) * (1.0 - i as f32 / (0.04 * sr));
                }
            }
            let a = (t * sr) as usize;
            let len = (dur * sr) as usize;
            for i in 0..len {
                let tt = i as f32 / sr;
                // Scoop up from -70 cents over 70 ms; glide the last 90 ms
                // into the next note, or fall 1.5 semitones at a phrase end.
                // Scoops and glides take a smaller share of quick notes.
                let glide = (dur * 0.25).min(0.09);
                let mut m = midi - 0.7 * (1.0 - (tt / (glide * 0.8)).min(1.0));
                let left = dur - tt;
                if left < glide {
                    let g = 1.0 - left / glide;
                    m += if next > 0.0 {
                        (next - midi) * g * 0.5
                    } else {
                        -1.5 * g * g
                    };
                }
                if tt < glide && prev > 0.0 {
                    m += (prev - midi) * 0.5 * (1.0 - tt / glide);
                }
                let vib = if tt > 0.15 {
                    vibrato * (2.0 * PI * 5.3 * tt).sin()
                } else {
                    0.0
                };
                jitter = (jitter + 0.004 * rng.noise()).clamp(-0.12, 0.12);
                let f0 = 440.0 * 2f32.powf((m + vib + jitter + detune - 69.0) / 12.0);
                phase += 2.0 * PI * f0 / sr;
                // Legato notes run into each other; others have soft edges.
                let attack = if prev > 0.0 {
                    1.0
                } else {
                    (tt / 0.03).min(1.0)
                };
                let release = if next > 0.0 {
                    1.0
                } else {
                    (left / 0.06).min(1.0)
                };
                let shimmer = 1.0 + 0.08 * (2.0 * PI * 3.1 * tt + k as f32).sin();
                let mut v = 0.0;
                for h in 1..=10 {
                    let fh = f0 * h as f32;
                    // A vowel ("ah"): formants near 750 and 1200 Hz.
                    let formant = (-((fh - 750.0) / 350.0).powi(2)).exp()
                        + 0.6 * (-((fh - 1200.0) / 400.0).powi(2)).exp()
                        + 0.15;
                    v += formant * (phase * h as f32).sin() / h as f32;
                }
                out[a + i] += 0.12 * attack * release * shimmer * v;
            }
            t += dur;
        }
        out
    }

    /// (expected, found) pitch lists for a sung phrase, and the notes.
    fn eval_phrase(
        sr: f32,
        notes: &[(f32, f32)],
        seed: u32,
    ) -> (Vec<i32>, Vec<i32>, Vec<VoiceNote>) {
        let x = singer(sr, notes, seed);
        let t = transcribe(&take(x, sr), "melody");
        let want: Vec<i32> = notes
            .iter()
            .filter(|n| n.0 > 0.0)
            .map(|n| n.0 as i32)
            .collect();
        (want, rounded(&t.notes), t.notes)
    }

    const PHRASES: &[&[(f32, f32)]] = &[
        &[
            (60.0, 0.45),
            (62.0, 0.45),
            (64.0, 0.45),
            (65.0, 0.45),
            (67.0, 0.8),
        ],
        &[
            (67.0, 0.3),
            (64.0, 0.3),
            (0.0, 0.25),
            (64.0, 0.3),
            (62.0, 0.3),
            (60.0, 0.9),
        ],
        &[
            (57.0, 0.6),
            (60.0, 0.4),
            (62.0, 0.4),
            (64.0, 0.7),
            (0.0, 0.3),
            (62.0, 0.35),
            (60.0, 1.0),
        ],
        &[(48.0, 0.5), (52.0, 0.5), (55.0, 0.5), (60.0, 0.9)],
        &[
            (72.0, 0.25),
            (71.0, 0.25),
            (69.0, 0.25),
            (67.0, 0.25),
            (65.0, 0.25),
            (64.0, 0.6),
        ],
    ];

    /// Held out from tuning `DETAIL_CHANGES`: (phrase, detune, vibrato).
    const HELD_OUT: &[(&[(f32, f32)], f32, f32)] = &[
        // Sixteenths at 120 bpm.
        (
            &[
                (64.0, 0.125),
                (62.0, 0.125),
                (60.0, 0.125),
                (62.0, 0.125),
                (64.0, 0.125),
                (65.0, 0.125),
                (67.0, 0.3),
            ],
            0.0,
            0.2,
        ),
        // Repeated notes with rests between them.
        (
            &[
                (62.0, 0.2),
                (0.0, 0.1),
                (62.0, 0.2),
                (0.0, 0.1),
                (62.0, 0.2),
                (0.0, 0.1),
                (67.0, 0.6),
            ],
            0.0,
            0.3,
        ),
        // Octave leaps.
        (
            &[(57.0, 0.5), (69.0, 0.5), (57.0, 0.5), (64.0, 0.8)],
            0.0,
            0.45,
        ),
        // Heavy vibrato on long notes.
        (&[(65.0, 1.0), (63.0, 1.0), (60.0, 1.2)], 0.0, 0.8),
        // A singer 35 cents flat, and one 30 cents sharp.
        (
            &[(60.0, 0.4), (62.0, 0.4), (64.0, 0.4), (60.0, 0.8)],
            -0.35,
            0.45,
        ),
        (
            &[
                (69.0, 0.35),
                (67.0, 0.35),
                (65.0, 0.35),
                (64.0, 0.35),
                (62.0, 0.7),
            ],
            0.3,
            0.45,
        ),
        // Whistled (high), and hummed (low).
        (
            &[(84.0, 0.3), (86.0, 0.3), (88.0, 0.3), (91.0, 0.6)],
            0.0,
            0.3,
        ),
        (
            &[(45.0, 0.5), (47.0, 0.5), (48.0, 0.5), (52.0, 0.8)],
            0.0,
            0.3,
        ),
    ];

    /// Prints how the held-out phrases come out: `cargo test -p
    /// rosaclef-studio --release held_out_report -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn held_out_report() {
        let (mut exact, mut extra, mut missing, mut total) = (0, 0, 0, 0);
        for (i, (ph, detune, vib)) in HELD_OUT.iter().enumerate() {
            for seed in [4, 5, 6] {
                let x = singer_with(44100.0, ph, seed + i as u32 * 7, *detune, *vib);
                let t = transcribe(&take(x, 44100.0), "melody");
                let want: Vec<i32> = ph
                    .iter()
                    .filter(|n| n.0 > 0.0)
                    .map(|n| n.0 as i32)
                    .collect();
                let got = rounded(&t.notes);
                total += 1;
                if want == got {
                    exact += 1;
                } else {
                    println!(
                        "held-out {i} seed {seed}: want {want:?}\n                   got  {got:?}"
                    );
                }
                extra += got.len().saturating_sub(want.len());
                missing += want.len().saturating_sub(got.len());
            }
        }
        println!("held-out exact {exact}/{total} · extra notes {extra} · missing {missing}");
    }

    #[test]
    fn sung_phrases_come_out_without_stray_notes() {
        // Glides, scoops, vibrato, consonants and breaths: no notes between
        // the sung ones.
        for (i, ph) in PHRASES.iter().enumerate() {
            let (want, got, notes) = eval_phrase(44100.0, ph, 1 + i as u32 * 10);
            assert_eq!(got, want, "phrase {i}: {notes:?}");
        }
        // Phrases held out from tuning: no stray notes at the default level
        // either, the pitches right (a flat and a sharp singer, heavy
        // vibrato, whistling, humming), at most quick runs merged.
        let mut exact = 0;
        for (i, (ph, detune, vib)) in HELD_OUT.iter().enumerate() {
            let x = singer_with(44100.0, ph, 4 + i as u32 * 7, *detune, *vib);
            let t = transcribe(&take(x, 44100.0), "melody");
            let want: Vec<i32> = ph
                .iter()
                .filter(|n| n.0 > 0.0)
                .map(|n| n.0 as i32)
                .collect();
            let got = rounded(&t.notes);
            assert!(
                got.len() <= want.len(),
                "held-out {i}: {got:?} for {want:?}"
            );
            exact += (got == want) as usize;
            if i == 0 {
                // The run of sixteenths needs the most detailed level.
                assert_eq!(
                    rounded(&t.details[DETAIL_CHANGES.len() - 1]),
                    want,
                    "held-out {i}"
                );
            }
        }
        assert!(
            exact >= HELD_OUT.len() - 2,
            "{exact}/{} held-out phrases exact",
            HELD_OUT.len()
        );
    }

    /// Every test phrase (training then held out) with its (want, notes per
    /// detail level).
    fn all_phrases() -> Vec<(String, Vec<i32>, Vec<Vec<i32>>)> {
        let mut out = vec![];
        for (i, ph) in PHRASES.iter().enumerate() {
            for seed in [1, 2, 3] {
                let x = singer(44100.0, ph, seed + i as u32 * 10);
                let t = transcribe(&take(x, 44100.0), "melody");
                let want = ph
                    .iter()
                    .filter(|n| n.0 > 0.0)
                    .map(|n| n.0 as i32)
                    .collect();
                out.push((
                    format!("phrase {i}/{seed}"),
                    want,
                    t.details.iter().map(|d| rounded(d)).collect(),
                ));
            }
        }
        for (i, (ph, detune, vib)) in HELD_OUT.iter().enumerate() {
            for seed in [4, 5, 6] {
                let x = singer_with(44100.0, ph, seed + i as u32 * 7, *detune, *vib);
                let t = transcribe(&take(x, 44100.0), "melody");
                let want = ph
                    .iter()
                    .filter(|n| n.0 > 0.0)
                    .map(|n| n.0 as i32)
                    .collect();
                out.push((
                    format!("held-out {i}/{seed}"),
                    want,
                    t.details.iter().map(|d| rounded(d)).collect(),
                ));
            }
        }
        out
    }

    /// Prints each detail level's score: `cargo test -p rosaclef-studio
    /// --release detail_report -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn detail_report() {
        let all = all_phrases();
        for level in 0..DETAIL_CHANGES.len() {
            let (mut exact, mut extra, mut missing) = (0, 0, 0);
            for (_, want, got) in &all {
                let got = &got[level];
                exact += (got == want) as usize;
                extra += got.len().saturating_sub(want.len());
                missing += want.len().saturating_sub(got.len());
            }
            println!(
                "detail {level}: exact {exact}/{} · extra {extra} · missing {missing}",
                all.len()
            );
        }
    }

    /// Prints how each phrase comes out: `cargo test -p rosaclef-studio
    /// --release sung_phrases_report -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn sung_phrases_report() {
        let (mut exact, mut extra, mut missing) = (0, 0, 0);
        for (i, ph) in PHRASES.iter().enumerate() {
            for seed in [1, 2, 3] {
                let (want, got, notes) = eval_phrase(44100.0, ph, seed + i as u32 * 10);
                if want == got {
                    exact += 1;
                }
                extra += got.len().saturating_sub(want.len());
                missing += want.len().saturating_sub(got.len());
                println!("phrase {i} seed {seed}: want {want:?}\n                  got  {got:?}");
                if want != got {
                    for n in &notes {
                        println!(
                            "                    {:.2}-{:.2} {:.2}",
                            n.start, n.end, n.pitch
                        );
                    }
                }
            }
        }
        println!(
            "exact {exact}/{} · extra notes {extra} · missing {missing}",
            PHRASES.len() * 3
        );
    }

    #[test]
    fn a_sung_arpeggio_becomes_three_notes() {
        let mut x = vec![0.0; (0.2 * SR) as usize];
        sing(&mut x, SR, 0.2, 0.4, 60.0, 0.3);
        sing(&mut x, SR, 0.7, 0.4, 64.0, 0.3);
        sing(&mut x, SR, 1.2, 0.5, 67.0, 0.3);
        x.resize((2.0 * SR) as usize, 0.0);
        let t = transcribe(&take(x, SR), "melody");
        assert_eq!(rounded(&t.notes), vec![60, 64, 67], "{:?}", t.notes);
        for (n, at) in t.notes.iter().zip([0.2, 0.7, 1.2]) {
            assert!((n.start - at).abs() < 0.04, "{n:?} should start at {at}");
            assert!((n.pitch - n.pitch.round()).abs() < 0.2, "{n:?} is in tune");
        }
        assert!((t.notes[2].end - 1.7).abs() < 0.05);
        assert_eq!(t.contour.len(), t.level.len());
    }

    #[test]
    fn a_legato_line_is_cut_where_the_pitch_moves() {
        // One continuous tone stepping C4 → D4 → E4 with no gap.
        let mut x = vec![];
        let steps = [(60.0, 0.3), (62.0, 0.3), (64.0, 0.4)];
        let mut phase = 0.0f32;
        for (midi, dur) in steps {
            let f0 = 440.0 * 2f32.powf((midi - 69.0) / 12.0);
            for _ in 0..(dur * SR) as usize {
                phase += 2.0 * PI * f0 / SR;
                x.push(0.3 * (phase.sin() + 0.5 * (2.0 * phase).sin()));
            }
        }
        let t = transcribe(&take(x, SR), "melody");
        assert_eq!(rounded(&t.notes), vec![60, 62, 64], "{:?}", t.notes);
        assert!((t.notes[1].start - 0.3).abs() < 0.05, "{:?}", t.notes);
    }

    #[test]
    fn repeated_syllables_on_one_pitch_are_separate_notes() {
        // "da da da" on A3: the level dips between the syllables.
        let f0 = 220.0;
        let n = (1.5 * SR) as usize;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / SR;
                let within = t % 0.5;
                let env = if within < 0.38 { 1.0 } else { 0.08 };
                0.3 * env * ((2.0 * PI * f0 * t).sin() + 0.4 * (4.0 * PI * f0 * t).sin())
            })
            .collect();
        let t = transcribe(&take(x, SR), "melody");
        assert_eq!(rounded(&t.notes), vec![57, 57, 57], "{:?}", t.notes);
    }

    #[test]
    fn whistles_and_low_hums_are_followed() {
        let mut x = vec![];
        // A pure whistle on A6.
        sing(&mut x, 44100.0, 0.1, 0.5, 93.0, 0.3);
        x.resize((0.8 * 44100.0) as usize, 0.0);
        let t = transcribe(&take(x, 44100.0), "melody");
        assert_eq!(rounded(&t.notes), vec![93], "{:?}", t.notes);

        let mut x = vec![];
        sing(&mut x, SR, 0.1, 0.6, 43.0, 0.3); // G2, hummed
        x.resize((0.9 * SR) as usize, 0.0);
        let t = transcribe(&take(x, SR), "melody");
        assert_eq!(rounded(&t.notes), vec![43], "{:?}", t.notes);
    }

    #[test]
    fn softer_notes_get_lower_velocities() {
        let mut x = vec![];
        sing(&mut x, SR, 0.1, 0.4, 62.0, 0.4);
        sing(&mut x, SR, 0.6, 0.4, 65.0, 0.08);
        x.resize((1.2 * SR) as usize, 0.0);
        let t = transcribe(&take(x, SR), "melody");
        assert_eq!(t.notes.len(), 2, "{:?}", t.notes);
        assert!(
            t.notes[0].velocity > t.notes[1].velocity + 0.2,
            "{:?}",
            t.notes
        );
    }

    struct Rng(u32);
    impl Rng {
        fn noise(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
        }
    }

    /// Beatbox-like hits: a low thump, a band of noise, a hiss.
    fn hit(out: &mut [f32], sr: f32, at: f32, kind: &str, amp: f32, rng: &mut Rng) {
        let a = (at * sr) as usize;
        let (mut lp1, mut lp2, mut hp_in, mut hp) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut d1 = [0.0f32; 3];
        let mut phase = 0.0f32;
        let len = (0.4 * sr) as usize;
        for i in 0..len {
            if a + i >= out.len() {
                break;
            }
            let t = i as f32 / sr;
            let fade = ((len - i) as f32 / (0.02 * sr)).min(1.0);
            let v = match kind {
                "kick" => {
                    let f = 50.0 + 110.0 * (-t / 0.03).exp();
                    phase += 2.0 * PI * f / sr;
                    phase.sin() * (-t / 0.09).exp() + 0.05 * rng.noise() * (-t / 0.004).exp()
                }
                "snare" => {
                    // Noise band-passed around 1-3 kHz, plus a little body.
                    let n = rng.noise();
                    let k = 1.0 - (-2.0 * PI * 3000.0 / sr).exp();
                    lp1 += (n - lp1) * k;
                    lp2 += (lp1 - lp2) * k;
                    let c = 1.0 - (-2.0 * PI * 1000.0 / sr).exp();
                    hp += (lp2 - hp_in) - hp * c;
                    hp_in = lp2;
                    phase += 2.0 * PI * 190.0 / sr;
                    (4.0 * hp + 0.1 * phase.sin()) * (-t / 0.07).exp()
                }
                _ => {
                    // Noise differentiated three times: mostly above 5 kHz.
                    let mut v = rng.noise();
                    for d in d1.iter_mut() {
                        let o = v - *d;
                        *d = v;
                        v = o;
                    }
                    0.4 * v * (-t / 0.03).exp()
                }
            };
            out[a + i] += amp * fade * v;
        }
    }

    fn beat(sr: f32, pattern: &[(f32, &str)], len: f32) -> Vec<f32> {
        let mut x = vec![0.0; (len * sr) as usize];
        let mut rng = Rng(0x9e3779b9);
        for &(t, k) in pattern {
            hit(&mut x, sr, t, k, 0.5, &mut rng);
        }
        // A quiet room.
        for v in x.iter_mut() {
            *v += 0.0005 * rng.noise();
        }
        x
    }

    #[test]
    fn a_beatbox_loop_becomes_kicks_snares_and_hats() {
        let pattern = [
            (0.10, "kick"),
            (0.35, "hat"),
            (0.60, "snare"),
            (0.85, "hat"),
            (1.10, "kick"),
            (1.35, "kick"),
            (1.60, "snare"),
            (1.85, "hat"),
        ];
        for sr in [44100.0, SR] {
            let t = transcribe(&take(beat(sr, &pattern, 2.2), sr), "drums");
            let got: Vec<&str> = t.hits.iter().map(|h| h.kind).collect();
            let want: Vec<&str> = pattern.iter().map(|p| p.1).collect();
            assert_eq!(got, want, "{:#?}", t.hits);
            for (h, p) in t.hits.iter().zip(pattern) {
                assert!((h.time - p.0).abs() < 0.012, "{h:?} should be at {}", p.0);
                assert!(h.strength > 0.2, "{h:?}");
            }
        }
    }

    #[test]
    fn quiet_takes_and_steady_tones() {
        // The same loop recorded 30 dB lower is found all the same.
        let pattern = [(0.1, "kick"), (0.4, "snare"), (0.7, "hat")];
        let x: Vec<f32> = beat(SR, &pattern, 1.0).iter().map(|v| v * 0.03).collect();
        let t = transcribe(&take(x, SR), "drums");
        assert_eq!(
            t.hits.iter().map(|h| h.kind).collect::<Vec<_>>(),
            vec!["kick", "snare", "hat"],
            "{:?}",
            t.hits
        );
        // A held tone is not a drum roll.
        let x: Vec<f32> = (0..(0.8 * SR) as usize)
            .map(|i| 0.3 * (i as f32 * 0.05).sin())
            .collect();
        let t = transcribe(&take(x, SR), "drums");
        assert!(t.hits.len() <= 1, "{:?}", t.hits);
    }

    #[test]
    fn accents_come_out_as_velocity() {
        let sr = SR;
        let mut x = vec![0.0; (1.0 * sr) as usize];
        let mut rng = Rng(7);
        hit(&mut x, sr, 0.1, "snare", 0.6, &mut rng);
        hit(&mut x, sr, 0.5, "snare", 0.12, &mut rng);
        let t = transcribe(&take(x, sr), "drums");
        assert_eq!(t.hits.len(), 2, "{:?}", t.hits);
        assert!(
            t.hits[0].velocity > t.hits[1].velocity + 0.2,
            "{:?}",
            t.hits
        );
    }

    #[test]
    fn silence_has_nothing_in_it() {
        let x = vec![0.0; (1.0 * SR) as usize];
        let t = transcribe(&take(x.clone(), SR), "melody");
        assert!(t.notes.is_empty());
        let t = transcribe(&take(x, SR), "drums");
        assert!(t.hits.is_empty());
        let t = transcribe(&take(vec![], SR), "drums");
        assert!(t.hits.is_empty() && t.level.is_empty());
    }

    #[test]
    fn stereo_takes_are_mixed_down() {
        let mut l = vec![];
        sing(&mut l, SR, 0.1, 0.4, 69.0, 0.3);
        l.resize((0.7 * SR) as usize, 0.0);
        let r = l.clone();
        let t = transcribe(
            &SampleData {
                sample_rate: SR,
                channels: vec![l, r],
            },
            "melody",
        );
        assert_eq!(rounded(&t.notes), vec![69]);
    }

    #[test]
    fn classify_uses_the_boundaries() {
        assert_eq!(classify(300.0, KICK_BELOW, HAT_ABOVE), "kick");
        assert_eq!(classify(2000.0, KICK_BELOW, HAT_ABOVE), "snare");
        assert_eq!(classify(8000.0, KICK_BELOW, HAT_ABOVE), "hat");
        assert_eq!(classify(2000.0, 2500.0, HAT_ABOVE), "kick");
    }
}
