//! Voice to notes: turns a recorded take into music.
//!
//! - **Melody** — a sung, hummed or whistled line becomes notes. The pitch is
//!   tracked with YIN (de Cheveigné & Kawahara, 2002) every 10 ms on a
//!   ~16 kHz copy of the take, then cut into notes where the pitch settles
//!   on another semitone, the voice stops, or the level dips and comes back
//!   (a new syllable on the same pitch).
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
/// Notes shorter than this are dropped (clicks, breaths).
const MIN_NOTE: f32 = 0.06;

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
    pub notes: Vec<VoiceNote>,
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
        Transcription { mode: "drums", duration, step: STEP, level, contour: vec![], notes: vec![], hits: beatbox(&x, sr) }
    } else {
        let (contour, notes) = melody(&x, sr);
        Transcription { mode: "melody", duration, step: STEP, level, contour, notes, hits: vec![] }
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
        n => (0..d.len()).map(|i| d.channels.iter().map(|c| c[i]).sum::<f32>() / n as f32).collect(),
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
    r.iter().map(|v| if max > 0.0 { round3(v / max) } else { 0.0 }).collect()
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
            let sinc = if t == 0.0 { 2.0 * fc } else { (2.0 * std::f32::consts::PI * fc * t).sin() / (std::f32::consts::PI * t) };
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

/// YIN pitch per frame: (frequency in Hz or 0, aperiodicity 0..1).
fn yin(x: &[f32], sr: f32, frames: usize) -> Vec<(f32, f32)> {
    let tau_min = ((sr / FMAX) as usize).max(2);
    let tau_max = (sr / FMIN).ceil() as usize;
    let w = tau_max;
    let hop = STEP * sr;
    let mut d = vec![0f32; tau_max + 2];
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
                d[tau] = acc;
                running += acc;
                dn[tau] = if running > 0.0 { acc * tau as f32 / running } else { 1.0 };
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
            let tau = best.unwrap_or_else(|| (tau_min..tau_max).min_by(|&a, &b| dn[a].partial_cmp(&dn[b]).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(tau_min));
            let ap = dn[tau];
            if ap > 0.35 {
                return (0.0, ap);
            }
            // Parabolic interpolation around the dip.
            let (a, b, c) = (dn[tau - 1], dn[tau], dn[tau + 1]);
            let den = a - 2.0 * b + c;
            let shift = if den.abs() > 1e-9 { ((a - c) / (2.0 * den)).clamp(-1.0, 1.0) } else { 0.0 };
            (sr / (tau as f32 + shift), ap)
        })
        .collect()
}

/// A note being followed.
struct Cur {
    start: usize,
    pitches: Vec<f32>,
    peak: f32,
    /// Frames (index, pitch) that left the note's pitch, not yet decided.
    away: Vec<(usize, f32)>,
    /// Where the level fell under half the peak, and the quietest frame since.
    fall: Option<usize>,
    dip: f32,
}

impl Cur {
    fn new(start: usize) -> Cur {
        Cur { start, pitches: vec![], peak: 0.0, away: vec![], fall: None, dip: 0.0 }
    }

    /// The pitch the note sits on (robust to vibrato and the attack).
    fn anchor(&self) -> f32 {
        let from = self.pitches.len().saturating_sub(15);
        median(&mut self.pitches[from..].to_vec())
    }

    /// Take back the frames that had wandered off (they were a glide).
    fn settle(&mut self) {
        for (_, p) in self.away.drain(..) {
            self.pitches.push(p);
        }
    }
}

/// Pitch contour (fractional MIDI per frame, 0 = none) and notes.
fn melody(x: &[f32], sr: f32) -> (Vec<f32>, Vec<VoiceNote>) {
    let factor = ((sr / 16000.0).floor() as usize).max(1);
    let y = decimate(x, factor);
    let ysr = sr / factor as f32;
    let frames = (x.len() as f32 / sr / STEP).ceil() as usize;
    let pitch = yin(&y, ysr, frames);
    let level = rms_frames(&y, ysr);
    let loud = level.iter().cloned().fold(0.0, f32::max);
    let gate = (loud * 0.03).max(0.002); // -30 dB under the loudest frame

    let raw: Vec<Option<f32>> = (0..frames)
        .map(|i| {
            let (f, _) = pitch[i];
            let l = level.get(i).copied().unwrap_or(0.0);
            (f > 0.0 && l > gate).then(|| hz_to_midi(f))
        })
        .collect();
    // Median of 5 over the voiced neighbours: removes single-frame octave slips.
    let smooth: Vec<Option<f32>> = (0..frames)
        .map(|i| {
            raw[i]?;
            let mut v: Vec<f32> = (i.saturating_sub(2)..(i + 3).min(frames)).filter_map(|j| raw[j]).collect();
            Some(median(&mut v))
        })
        .collect();

    let mut spans: Vec<(usize, usize, Vec<f32>, f32)> = vec![];
    let finish = |c: Cur, end: usize, spans: &mut Vec<(usize, usize, Vec<f32>, f32)>| {
        if end > c.start && !c.pitches.is_empty() {
            spans.push((c.start, end, c.pitches, c.peak));
        }
    };
    let mut cur: Option<Cur> = None;
    for i in 0..frames {
        let l = level.get(i).copied().unwrap_or(0.0);
        let Some(m) = smooth[i] else {
            if let Some(mut c) = cur.take() {
                c.settle();
                finish(c, i, &mut spans);
            }
            continue;
        };
        let mut c = cur.take().unwrap_or_else(|| Cur::new(i));
        if c.pitches.is_empty() {
            c.pitches.push(m);
            c.peak = l;
            cur = Some(c);
            continue;
        }
        // A new syllable: the level fell under half the peak and rose again
        // by ~8 dB. The quiet frames between the two are a gap.
        if let Some(fall) = c.fall {
            if l > c.dip * 2.5 && l > c.peak * 0.25 {
                c.settle();
                c.pitches.truncate(fall - c.start);
                finish(c, fall, &mut spans);
                let mut next = Cur::new(i);
                next.pitches.push(m);
                next.peak = l;
                cur = Some(next);
                continue;
            }
        }
        if l < c.peak * 0.5 {
            if c.fall.is_none() {
                c.fall = Some(i);
                c.dip = l;
            }
            c.dip = c.dip.min(l);
        } else {
            c.fall = None;
        }
        c.peak = c.peak.max(l);
        if (m - c.anchor()).abs() > 0.75 {
            c.away.push((i, m));
            // Settled somewhere else for 40 ms: a new note from where it left.
            if c.away.len() >= 4 {
                let at = c.away[0].0;
                let mut next = Cur::new(at);
                next.pitches = c.away.drain(..).map(|(_, p)| p).collect();
                next.peak = l;
                finish(c, at, &mut spans);
                cur = Some(next);
                continue;
            }
        } else {
            c.settle();
            c.pitches.push(m);
        }
        cur = Some(c);
    }
    if let Some(mut c) = cur.take() {
        c.settle();
        finish(c, frames, &mut spans);
    }

    // A short, quiet stretch pressed against a much louder note is its tail
    // or the consonant between two syllables, not a note.
    let tail = |i: usize| {
        let (a, b, _, peak) = &spans[i];
        let touching = |j: usize| {
            let (c, d, _, p) = &spans[j];
            (c.saturating_sub(*b) <= 3 || a.saturating_sub(*d) <= 3) && *p > peak * 4.0
        };
        (b - a) as f32 * STEP < 0.15 && ((i > 0 && touching(i - 1)) || (i + 1 < spans.len() && touching(i + 1)))
    };
    let keep: Vec<bool> = (0..spans.len()).map(|i| !tail(i)).collect();
    let loudest = spans.iter().map(|s| s.3).fold(0.0, f32::max).max(1e-9);
    let notes = spans
        .into_iter()
        .zip(keep)
        .filter(|((a, b, _, _), keep)| *keep && (b - a) as f32 * STEP >= MIN_NOTE)
        .map(|(s, _)| s)
        .map(|(a, b, mut ps, peak)| {
            // Skip the scoop into the note when there is enough of it.
            let skip = if ps.len() >= 8 { ps.len() / 4 } else { 0 };
            let pitch = median(&mut ps[skip..]);
            let db = 20.0 * (peak / loudest).max(1e-6).log10();
            VoiceNote {
                start: round3(a as f32 * STEP),
                end: round3(b as f32 * STEP),
                pitch: (pitch * 100.0).round() / 100.0,
                velocity: round3((0.35 + 0.65 * (1.0 + db / 30.0)).clamp(0.2, 1.0)),
            }
        })
        .collect();
    let contour = smooth.iter().map(|m| m.map(|v| (v * 100.0).round() / 100.0).unwrap_or(0.0)).collect();
    (contour, notes)
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
fn power(x: &[f32], win: &[f32], n: usize, tw: &[(f32, f32)], re: &mut Vec<f32>, im: &mut Vec<f32>) -> Vec<f32> {
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
    let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
    let (mut re, mut im) = (vec![], vec![]);
    let frames = if x.len() > n { (x.len() - n) / hop + 1 } else { 0 };
    // The flux is averaged per band, then summed: a kick moves a few low
    // bins, a hat hundreds of high ones, and both should count alike.
    let band_of: Vec<usize> = (0..=n / 2).map(|k| BANDS.iter().filter(|&&edge| k as f32 * sr / n as f32 >= edge).count()).collect();
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
            let r = reference[k.saturating_sub(1)].max(reference[k]).max(reference[(k + 1).min(bins - 1)]);
            rise[band_of[k]] += (cur[k] - r).max(0.0);
        }
        back[f % 2] = cur;
        let acc: f32 = rise.iter().zip(&width).map(|(r, w)| if *w > 0.0 { r / w } else { 0.0 }).sum();
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
        let e = energy[f..(f + around).min(frames)].iter().cloned().fold(0.0, f32::max);
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
            let env: Vec<f32> = x[a..b].chunks(ms).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs()))).collect();
            let (peak_at, peak) = env.iter().enumerate().fold((0, 0.0f32), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
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
    let win: Vec<f32> = (0..len).map(|i| if i + fade < len { 1.0 } else { (len - i) as f32 / fade as f32 }).collect();
    let (mut re, mut im) = (vec![], vec![]);
    let mut hits: Vec<Hit> = vec![];
    let mut peaks: Vec<f32> = vec![];
    for (i, &(t, strength)) in found.iter().enumerate() {
        let a = (t * sr) as usize;
        // The segment stops at the next onset.
        let next = found.get(i + 1).map(|&(u, _)| (u * sr) as usize).unwrap_or(x.len());
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
        SampleData { sample_rate: sr, channels: vec![x] }
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
        assert!(t.notes[0].velocity > t.notes[1].velocity + 0.2, "{:?}", t.notes);
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
        assert_eq!(t.hits.iter().map(|h| h.kind).collect::<Vec<_>>(), vec!["kick", "snare", "hat"], "{:?}", t.hits);
        // A held tone is not a drum roll.
        let x: Vec<f32> = (0..(0.8 * SR) as usize).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
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
        assert!(t.hits[0].velocity > t.hits[1].velocity + 0.2, "{:?}", t.hits);
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
        let t = transcribe(&SampleData { sample_rate: SR, channels: vec![l, r] }, "melody");
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
