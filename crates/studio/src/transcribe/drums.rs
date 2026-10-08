//! Drum transcription: beatbox takes and drum recordings into kick, tom,
//! snare, hat and open-hat hits.
//!
//! One moment can hold several drums (a kick and a hat on the same beat), so
//! onsets are found band by band — low (kicks, toms), mid (snares, claps,
//! rims), high (hats, shakers) — each band against its own typical level, so
//! a quiet hat under a loud kick still counts. Onsets of different bands
//! within 20 ms are one moment, and a moment soon after another that brought
//! nothing new (a "tsss" swelling, a hummed kick ringing on) is the same
//! sound. Each moment is then read from how much each band rose, where the
//! low part sits (a kick below a tom) and how long the highs ring (an open
//! hat), against what the take's own clear hits look like: a voice's "p"
//! is a bright kick and its "ts" sits lower than a cymbal.

use super::{power, round3, twiddles, Hit};

/// The three bands onsets are found in (Hz).
const BANDS: [(f32, f32); 3] = [(30.0, 150.0), (150.0, 5000.0), (5000.0, 16000.0)];

/// Finer bands for the shape of a moment (Hz edges).
const FINE: [f32; 11] = [
    30.0, 80.0, 150.0, 300.0, 600.0, 1200.0, 2500.0, 5000.0, 8000.0, 12000.0, 20000.0,
];

/// A moment where something was hit, with what it looks like.
#[derive(Debug, Clone)]
pub struct Moment {
    /// Seconds.
    pub time: f32,
    /// How much each band rose, relative to that band's typical hit (0..~1.5).
    pub strength: [f32; 3],
    /// The energy each fine band gained over the moment before (linear power).
    pub rise: [f32; 10],
    /// How fast the hiss and the low body die away after the hit (dB/s,
    /// measured until the next moment; 0 when there is no time to tell).
    pub high_decay: f32,
    pub low_decay: f32,
    /// The same as `high_decay` from 5 kHz up.
    pub wide_decay: f32,
    /// Peak level of the moment (for velocities).
    pub peak: f32,
}

/// Band index of each FFT bin (usize::MAX outside the bands).
fn band_of(n: usize, sr: f32, edges: &[(f32, f32)]) -> Vec<usize> {
    (0..=n / 2)
        .map(|k| {
            let f = k as f32 * sr / n as f32;
            edges
                .iter()
                .position(|(a, b)| f >= *a && f < *b)
                .unwrap_or(usize::MAX)
        })
        .collect()
}

fn percentile(mut v: Vec<f32>, p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[((v.len() - 1) as f32 * p).round() as usize]
}

/// The moments of a take.
pub fn moments(x: &[f32], sr: f32) -> Vec<Moment> {
    let peak_abs = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak_abs <= 1e-6 {
        return vec![];
    }
    let n = if sr > 60000.0 { 4096 } else { 2048 };
    let hop = ((0.005 * sr) as usize).max(1);
    let tw = twiddles(n);
    let win: Vec<f32> = (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect();
    let bins = n / 2 + 1;
    let coarse = band_of(n, sr, &BANDS);
    let fine_edges: Vec<(f32, f32)> = FINE.windows(2).map(|w| (w[0], w[1])).collect();
    let fine = band_of(n, sr, &fine_edges);
    let mut width = [0f32; 3];
    for &b in &coarse {
        if b < 3 {
            width[b] += 1.0;
        }
    }
    let frames = if x.len() > n {
        (x.len() - n) / hop + 1
    } else {
        0
    };
    let gain = 0.5 / peak_abs;
    let (mut re, mut im) = (vec![], vec![]);
    let mut seg = vec![0f32; n];
    let mut back: [Vec<f32>; 2] = [vec![0.0; bins], vec![0.0; bins]];
    let mut flux = vec![[0f32; 3]; frames];
    let mut energy = vec![[0f32; 10]; frames];
    let mut level = vec![0f32; frames];
    // The four regions (`Regions`) in a short window centred on the long
    // one's centre: dips between hits a few tens of ms apart show in it.
    let ns = n / 4;
    let tws = twiddles(ns);
    let wins: Vec<f32> = (0..ns)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / ns as f32).cos())
        .collect();
    let region_of: Vec<usize> = (0..=ns / 2)
        .map(|k| {
            let f = k as f32 * sr / ns as f32;
            REGION_EDGES
                .iter()
                .filter(|&&e| f >= e)
                .count()
                .wrapping_sub(1)
        })
        .collect();
    let mut segs = vec![0f32; ns];
    let mut short = vec![[0f32; 4]; frames];
    for f in 0..frames {
        for (i, v) in segs.iter_mut().enumerate() {
            *v = x[f * hop + (n - ns) / 2 + i] * gain;
        }
        for (k, v) in power(&segs, &wins, ns, &tws, &mut re, &mut im)
            .iter()
            .enumerate()
        {
            if region_of[k] < 4 {
                short[f][region_of[k]] += v;
            }
        }
        for (i, v) in seg.iter_mut().enumerate() {
            *v = x[f * hop + i] * gain;
        }
        let p = power(&seg, &win, n, &tw, &mut re, &mut im);
        let cur: Vec<f32> = p.iter().map(|v| (1.0 + 10.0 * v.sqrt()).ln()).collect();
        let reference = &back[f % 2];
        let mut rise = [0f32; 3];
        for k in 0..bins {
            let r = reference[k.saturating_sub(1)]
                .max(reference[k])
                .max(reference[(k + 1).min(bins - 1)]);
            if coarse[k] < 3 {
                rise[coarse[k]] += (cur[k] - r).max(0.0);
            }
            if fine[k] < 10 {
                energy[f][fine[k]] += p[k];
            }
        }
        back[f % 2] = cur;
        if f >= 2 {
            for b in 0..3 {
                flux[f][b] = rise[b] / width[b].max(1.0);
            }
        }
        level[f] = p.iter().sum::<f32>().sqrt();
    }
    if frames < 3 {
        return vec![];
    }

    // Each band's typical hit: the 90th percentile of its flux peaks.
    let ms = |s: f32| ((s * sr) / hop as f32).round() as usize;
    let mut reference = [0f32; 3];
    for b in 0..3 {
        let max = flux.iter().map(|v| v[b]).fold(0.0, f32::max);
        let peaks: Vec<f32> = (1..frames - 1)
            .filter(|&f| {
                flux[f][b] > 0.1 * max
                    && flux[f][b] >= flux[f - 1][b]
                    && flux[f][b] >= flux[f + 1][b]
            })
            .map(|f| flux[f][b])
            .collect();
        reference[b] = percentile(peaks, 0.9).max(1e-6);
    }

    // Band onsets: local peaks of each band's flux above its local mean.
    let loud = level.iter().cloned().fold(0.0, f32::max);
    let mut candidates: Vec<(usize, usize)> = vec![];
    for b in 0..3 {
        let around = ms(0.03).max(1);
        let (before, after) = (ms(0.1), ms(0.05));
        let mut last: Option<usize> = None;
        for f in 1..frames {
            let v = flux[f][b];
            if v < 0.06 * reference[b] || v < FLOOR[b] {
                continue;
            }
            let (lo, hi) = (f.saturating_sub(around), (f + around + 1).min(frames));
            if flux[lo..hi].iter().any(|u| u[b] > v) {
                continue;
            }
            let (a, z) = (f.saturating_sub(before), (f + after + 1).min(frames));
            let mean = flux[a..z].iter().map(|u| u[b]).sum::<f32>() / (z - a) as f32;
            if v < mean * 1.3 {
                continue;
            }
            if level[f..(f + around).min(frames)]
                .iter()
                .cloned()
                .fold(0.0, f32::max)
                < loud * 0.01
            {
                continue;
            }
            // Low sounds take longer to tell apart (and waver as they ring).
            if let Some(l) = last {
                if f - l < ms(if b == 0 { 0.045 } else { 0.03 }) {
                    continue;
                }
            }
            last = Some(f);
            candidates.push((f, b));
        }
    }
    candidates.sort();

    // Candidates within 20 ms are one moment.
    let mut out: Vec<Moment> = vec![];
    let mut starts: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < candidates.len() {
        let f0 = candidates[i].0;
        let mut j = i;
        while j < candidates.len() && candidates[j].0 <= f0 + ms(0.02) {
            j += 1;
        }
        i = j;
        // (The low band's onset shows later: low sounds need a longer window.)
        let mut strength = [0f32; 3];
        for b in 0..3 {
            let w1 = (f0 + ms(if b == 0 { 0.04 } else { 0.02 }) + 1).min(frames);
            strength[b] = flux[f0.saturating_sub(1)..w1]
                .iter()
                .map(|v| v[b])
                .fold(0.0, f32::max)
                / reference[b];
        }
        // The energy gained: the loudest frame just after against the quiet
        // valley before the onset (since the previous moment, from a frame
        // whose window ends before it). A low band must at least double, or
        // it is a ringing tom or kick wavering, not a new hit; the upper
        // bands need less (a snare roll's hits land on the last one's tail).
        let from = f0
            .saturating_sub(n / hop / 2 + 2)
            .max(starts.last().map(|p| p.0 + 1).unwrap_or(0))
            .min(f0);
        let post_end = (f0 + ms(0.04)).min(frames);
        let mut rise = [0f32; 10];
        for k in 0..10 {
            let after = energy[f0..post_end]
                .iter()
                .map(|e| e[k])
                .fold(0.0, f32::max);
            let before = energy[from..=f0]
                .iter()
                .map(|e| e[k])
                .fold(f32::MAX, f32::min);
            let need = if k < 3 { 2.0 } else { 1.3 };
            rise[k] = if after > need * before {
                after - before
            } else {
                0.0
            };
        }
        // A tom's pitch gliding down moves its energy from one low band to
        // the next: the low body as a whole has to rise too.
        let body = |f: usize| energy[f][0] + energy[f][1] + energy[f][2];
        let after = (f0..post_end).map(body).fold(0.0, f32::max);
        let before = (from..=f0).map(body).fold(f32::MAX, f32::min);
        if after <= 2.0 * before {
            rise[0..3].fill(0.0);
        }
        let peak = level[f0..post_end].iter().cloned().fold(0.0, f32::max);
        starts.push((f0, from));
        out.push(Moment {
            // The window's centre, less the rise time the flux needs.
            time: round3(((f0 * hop + n / 2) as f32 / sr - LAG).max(0.0)),
            strength,
            rise,
            high_decay: 0.0,
            low_decay: 0.0,
            wide_decay: 0.0,
            peak,
        });
    }
    // One sound can set off more than one onset: a "tsss" swelling, a
    // hummed kick ringing on. A moment soon after the last one is a new hit
    // only if some region dipped after the last one's peak and came back,
    // or jumped well past it (a hat on the tail of a kick); a roll's hits
    // each come out of a dip.
    let mut keep = vec![true; out.len()];
    let mut last = 0;
    for i in 1..out.len() {
        let (fp, f0) = (starts[last].0, starts[i].0);
        if f0 - fp >= ms(0.1) {
            last = i;
            continue;
        }
        let after_end = (f0 + ms(0.05)).min(frames);
        let tops: Vec<f32> = (0..4)
            .map(|r| {
                short[f0..after_end]
                    .iter()
                    .map(|e| e[r])
                    .fold(0.0, f32::max)
            })
            .collect();
        let loudest_region = tops.iter().cloned().fold(0.0, f32::max);
        // The last one just before and far quieter than this: it was the
        // lead-in of this sound (the "t" of a "ts"), which is heard where it
        // peaks.
        let lead_in = (0..4)
            .map(|r| short[fp..f0].iter().map(|e| e[r]).fold(0.0, f32::max))
            .fold(0.0, f32::max);
        if f0 - fp < ms(0.06) && loudest_region > 4.0 * lead_in {
            keep[last] = false;
            last = i;
            continue;
        }
        let new_hit = (0..4).any(|r| {
            let (mut top, mut at) = (0.0f32, fp);
            for (f, e) in short.iter().enumerate().take(f0).skip(fp) {
                if e[r] > top {
                    top = e[r];
                    at = f;
                }
            }
            let valley = short[at..=f0].iter().map(|e| e[r]).fold(f32::MAX, f32::min);
            tops[r] >= 0.1 * loudest_region
                && tops[r] > 2.0 * valley
                && (valley < 0.5 * top || tops[r] > 4.0 * top)
        });
        let onset = |m: &Moment| m.strength.iter().cloned().fold(0.0, f32::max);
        if new_hit {
            last = i;
        } else if onset(&out[i]) > onset(&out[last]) {
            // One sound: it is where its onset is clearest.
            keep[last] = false;
            last = i;
        } else {
            keep[i] = false;
        }
    }
    let mut k = keep.iter();
    out.retain(|_| *k.next().unwrap());
    let mut k = keep.iter();
    starts.retain(|_| *k.next().unwrap());

    // Decay rates of what the hit added (above the valley before it): from
    // its peak just after the hit to its lowest point before the next
    // moment (at most 150 ms on) — a sound still ringing from before does
    // not count.
    for i in 0..out.len() {
        let (f0, from) = starts[i];
        let next = starts.get(i + 1).map(|p| p.0).unwrap_or(frames).min(frames);
        let rate = |ks: std::ops::Range<usize>| {
            let raw = |f: usize| -> f32 { ks.clone().map(|k| energy[f][k]).sum() };
            let base = (from..=f0).map(raw).fold(f32::MAX, f32::min);
            let e = |f: usize| -> f32 { (raw(f) - base).max(0.0) };
            let (mut top, mut at) = (0.0f32, f0);
            for f in f0..(f0 + ms(0.04)).min(next) {
                if e(f) > top {
                    top = e(f);
                    at = f;
                }
            }
            let end = next.min(at + ms(0.15));
            if end <= at + ms(0.025) || top <= 0.0 {
                return 0.0;
            }
            let low = (at + 1..end).map(e).fold(top, f32::min).max(top * 1e-6);
            10.0 * (top / low).log10() / ((end - at) as f32 * hop as f32 / sr)
        };
        out[i].high_decay = rate(8..10);
        out[i].wide_decay = rate(7..10);
        out[i].low_decay = rate(0..3);
    }
    out
}

/// Lower edges of the four regions of `Regions` (Hz).
const REGION_EDGES: [f32; 5] = [30.0, 300.0, 1200.0, 5000.0, 20000.0];

/// Flux a band onset needs at least (keeps steady tones and hiss out).
const FLOOR: [f32; 3] = [0.02, 0.02, 0.02];
/// How far the flux peak leads the hit (s).
const LAG: f32 = -0.009;

/// Least share of a drum's typical energy that still counts as a hit.
const PRESENT: f32 = 0.02;
/// Least share of a typical snare that counts on top of a kick.
const ON_KICK: f32 = 0.2;
/// Least share of a typical hat that counts on top of a kick, or of a snare
/// in the middle of a roll.
const ON_OTHER: f32 = 0.35;
/// How much brighter than the take's kicks (noise per unit of body) a kick
/// has to be to have a snare on it.
const BRIGHTER: f32 = 5.0;
/// Hiss dying slower than this (dB/s) is an open hat's.
const OPEN_RING: f32 = 60.0;

/// A moment's rise in four regions of `FINE` (linear power): the low body of
/// kicks, toms and snares (30–300 Hz), the attack click (300–1200 Hz), the
/// noise of snares and claps (1.2–8 kHz) and the hiss of hats (8–20 kHz).
struct Regions {
    body: f32,
    click: f32,
    noise: f32,
    hiss: f32,
}

/// `low_hiss`: the share of 5-8 kHz that is hiss (the rest is noise).
fn regions(r: &[f32; 10], low_hiss: f32) -> Regions {
    Regions {
        body: r[0] + r[1] + r[2],
        click: r[3] + r[4],
        noise: r[5] + r[6] + (1.0 - low_hiss) * r[7],
        hiss: low_hiss * r[7] + r[8] + r[9],
    }
}

/// A robust ratio a/b over the moments `a/b` is meaningful for, else `dflt`.
fn learned(pairs: Vec<(f32, f32)>, q: f32, dflt: f32) -> f32 {
    let v: Vec<f32> = pairs
        .into_iter()
        .filter(|(_, b)| *b > 0.0)
        .map(|(a, b)| a / b)
        .collect();
    if v.len() >= 3 {
        percentile(v, q)
    } else {
        dflt
    }
}

/// Hits of a take (see the module doc).
pub fn hits(x: &[f32], sr: f32) -> Vec<Hit> {
    let ms = moments(x, sr);
    if ms.is_empty() {
        return vec![];
    }
    // A rise counts only if it is heard: at least -35 dB against the take's
    // loudest moment (a held tone's leakage wavers, but by next to nothing).
    let loudest_power = ms.iter().map(|m| m.peak * m.peak).fold(0.0, f32::max);
    let heard = |v: f32| if v >= loudest_power * 3e-4 { v } else { 0.0 };
    // Where 5-8 kHz belongs: a voice's "ts" sits there, a drum's snare
    // reaches up into it under cymbals that ring higher. It is the hiss as
    // far as the take's hats (moments ruled by what is above 5 kHz) have
    // their energy there rather than higher: drums have under a fifth of it
    // there (a third through a phone), voices half or more.
    let low_share = percentile(
        ms.iter()
            .filter_map(|m| {
                let top = heard(m.rise[7] + m.rise[8] + m.rise[9]);
                let rest: f32 = m.rise[..7].iter().sum();
                (top > 2.0 * rest).then(|| m.rise[7] / top)
            })
            .collect(),
        0.5,
    );
    let low_hiss = ((low_share - 0.25) / 0.2).clamp(0.0, 1.0);
    let rs: Vec<Regions> = ms
        .iter()
        .map(|m| {
            let r = regions(&m.rise, low_hiss);
            Regions {
                body: heard(r.body),
                click: heard(r.click),
                noise: heard(r.noise),
                hiss: heard(r.hiss),
            }
        })
        .collect();
    let max_of = |f: &dyn Fn(&Regions) -> f32| rs.iter().map(f).fold(0.0, f32::max).max(1e-12);
    let max_noise = max_of(&|r| r.noise);
    // How the drums spill into each other, learned from the take's clear
    // hits: a hat's noise per unit of hiss (moments that are nearly all
    // hiss) and a snare's hiss per unit of noise (a lower quantile: snares
    // without a hat on top).
    let hat_noise = learned(
        rs.iter()
            .filter(|r| r.hiss > 2.0 * (r.body + r.click) && r.hiss > 2.0 * r.noise)
            .map(|r| (r.noise, r.hiss))
            .collect(),
        0.8,
        0.2,
    );
    // (Snares ring with more noise than body: a bright "p" is no snare.)
    let snare_like = |r: &&Regions| {
        r.noise > 0.15 * max_noise && r.noise > hat_noise * r.hiss * 1.5 && r.noise > r.body
    };
    let snare_hiss = learned(
        rs.iter()
            .filter(snare_like)
            .map(|r| (r.hiss, r.noise))
            .collect(),
        0.2,
        0.3,
    );
    // Attack click per unit of low body: a kick's (moments with body and no
    // noise to speak of) and a snare's (the snare-like moments' upper half:
    // those without a kick under them).
    let kick_like = |r: &&Regions| r.body > 0.0 && r.noise < 0.02 * r.body && r.hiss < 0.5 * r.body;
    let kick_click = learned(
        rs.iter()
            .filter(kick_like)
            .map(|r| (r.click, r.body))
            .collect(),
        0.5,
        0.12,
    );
    // The noise a kick brings of its own per unit of body: next to none
    // from a drum, plenty from a "p" (a lower quantile: kicks without a
    // snare on them).
    let max_body = max_of(&|r| r.body);
    let kick_noise = learned(
        rs.iter()
            .filter(|r| r.body >= 0.1 * max_body && r.body > r.noise)
            .map(|r| (r.noise, r.body))
            .collect(),
        0.25,
        0.0,
    );
    let snare_click = learned(
        rs.iter()
            .filter(snare_like)
            .map(|r| (r.click, r.body))
            .collect(),
        0.8,
        0.3,
    );
    // The clickiest low hits (without a snare's noise): toms are told from
    // kicks by their lack of click, when the take has kicks that click.
    let clicky = learned(
        rs.iter()
            .filter(|r| r.body > 0.0 && r.noise < 0.5 * r.body)
            .map(|r| (r.click, r.body))
            .collect(),
        0.9,
        0.0,
    );

    struct Parts {
        low: f32,
        snare: f32,
        hat: f32,
    }
    let parts: Vec<Parts> = rs
        .iter()
        .map(|r| {
            // Hats and snares share the noise and the hiss: two rounds of
            // taking each one's spill out of the other.
            let mut hat = r.hiss;
            let mut snare = r.noise;
            for _ in 0..2 {
                snare = (r.noise - hat_noise * hat).max(0.0);
                hat = (r.hiss - snare_hiss * snare).max(0.0);
            }
            // The low body is a kick's (or tom's) and a snare's: they have
            // different attack clicks per unit of body, so the click tells
            // how much of the body is the kick's. Without a snare it all is.
            // (When the take's snares click like its kicks, their body is a
            // kick's: claps have next to none of their own.)
            let low = if snare <= 0.0 || snare_click <= kick_click * 1.3 {
                r.body
            } else {
                ((snare_click * r.body - r.click) / (snare_click - kick_click)).clamp(0.0, r.body)
            };
            Parts { low, snare, hat }
        })
        .collect();
    // Each drum's typical energy: the 75th percentile of its clear moments.
    let typical = |f: &dyn Fn(&Parts) -> f32| {
        let max = parts.iter().map(f).fold(0.0, f32::max);
        percentile(
            parts.iter().map(f).filter(|v| *v > 0.1 * max).collect(),
            0.75,
        )
        .max(1e-12)
    };
    let (t_low, t_snare, t_hat) = (
        typical(&|p| p.low),
        typical(&|p| p.snare),
        typical(&|p| p.hat),
    );

    let mut out: Vec<(Hit, f32)> = vec![];
    for ((m, r), p) in ms.iter().zip(&rs).zip(&parts) {
        let (low, snare, hat) = (p.low / t_low, p.snare / t_snare, p.hat / t_hat);
        let mut push = |kind: &'static str, ratio: f32, energy: f32| {
            if ratio >= PRESENT {
                out.push((
                    Hit {
                        time: m.time,
                        strength: round3(ratio.sqrt().min(1.0)),
                        velocity: 0.0,
                        kind,
                    },
                    energy,
                ));
            }
        };
        // Low energy alone, without an onset below 5 kHz to show for it, is
        // a ringing drum wavering.
        let low = if snare < PRESENT && hat < PRESENT && m.strength[0].max(m.strength[1]) < 0.15 {
            0.0
        } else {
            low
        };
        // A snare's body can read as a little kick under it (a flam, a roll
        // piling up), and so can the breath behind a "ts": a kick on a snare
        // or a hat has to be a real one.
        let low = if (snare >= ON_KICK || hat >= ON_OTHER) && low < ON_KICK {
            0.0
        } else {
            low
        };
        // A tom rings cleaner than a kick (little attack click) and higher
        // (most of its body above 150 Hz; a clean hummed kick sits lower) —
        // or, low as it may be, far cleaner than the take's kicks.
        let high_body = m.rise[2] / r.body.max(1e-12);
        let tom = r.click < 0.05 * r.body && (high_body > 0.4 || r.click < 0.25 * clicky * r.body);
        push(if tom { "tom" } else { "kick" }, low, p.low);
        // A kick's click spills a little into the snare's noise: a snare on
        // a kick has to be a real hit (ghost notes fall between the beats).
        // A hat's hiss reaches down into the noise too: a snare on a hat has
        // to be a real one as well.
        let snare = if (low >= PRESENT
            && (snare < ON_KICK || r.noise < BRIGHTER * kick_noise * r.body))
            || (hat >= ON_OTHER && snare < ON_KICK)
        {
            0.0
        } else {
            snare
        };
        push("snare", snare, p.snare);
        // Likewise the hiss of a kick's click or of a "k", or the hiss a roll
        // leaves: a hat on a kick or a snare has to be a real one.
        let hat = if (snare >= PRESENT || low >= PRESENT) && hat < ON_OTHER {
            0.0
        } else {
            hat
        };
        let decay = if low_hiss > 0.5 {
            m.wide_decay
        } else {
            m.high_decay
        };
        let open = decay > 0.0 && decay < OPEN_RING;
        push(if open { "openhat" } else { "hat" }, hat, p.hat);
    }
    // Velocities: each drum against its loudest hit.
    let mut loudest = std::collections::HashMap::new();
    for (h, e) in &out {
        let l = loudest.entry(h.kind).or_insert(0.0f32);
        *l = l.max(*e);
    }
    out.into_iter()
        .map(|(mut h, e)| {
            let db = 10.0 * (e / loudest[h.kind]).max(1e-6).log10();
            h.velocity = round3((0.4 + 0.6 * (1.0 + db / 24.0)).clamp(0.25, 1.0));
            h
        })
        .collect()
}
