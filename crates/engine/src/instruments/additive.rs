//! Ivory Organ (`additive`): additive synthesizer — up to 64 sine partials per voice.
//!
//! Each voice owns a bank of sine partials: rotating phasors (one complex
//! multiply per partial and sample) laid out in 8-lane blocks so the compiler
//! can vectorise them. A base recipe (`spectrum`) sets partial ratios and levels;
//! brightness, odd/even balance, a partial-domain low-pass with resonance and
//! its envelope, per-partial spectral decay and a slow random "shimmer" then
//! reshape the levels at control rate. Levels are ramped linearly across each
//! control chunk, so every change is click-free.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

const MAX_PARTIALS: usize = 64;
const MAX_UNISON: usize = 4;
const VOICES: usize = if MAX_VOICES < 16 { MAX_VOICES } else { 16 };
/// Control-rate chunk length (samples).
const CR: usize = 32;
/// Partials are processed in blocks of this many lanes.
const LANES: usize = 8;
const BLOCKS: usize = MAX_PARTIALS / LANES;
type Bank = [[f32; LANES]; BLOCKS];
/// Root-sum-square of the partial levels after normalisation (sets the loudness).
const LEVEL: f32 = 0.6;
/// Partials quieter than this count as silent.
const SILENT: f32 = 1e-6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spectrum {
    Saw,
    Square,
    Organ,
    Bell,
    Choir,
    Glass,
}

impl Spectrum {
    fn parse(s: &str) -> Spectrum {
        match s {
            "square" => Spectrum::Square,
            "organ" => Spectrum::Organ,
            "bell" => Spectrum::Bell,
            "choir" => Spectrum::Choir,
            "glass" => Spectrum::Glass,
            _ => Spectrum::Saw,
        }
    }
}

/// Drawbar-like organ registration: (harmonic, level).
const ORGAN: [(usize, f32); 10] = [
    (1, 1.0),
    (2, 0.8),
    (3, 0.65),
    (4, 0.5),
    (5, 0.18),
    (6, 0.35),
    (8, 0.3),
    (10, 0.1),
    (12, 0.14),
    (16, 0.1),
];
/// Bell partial ratios (hum, prime, tierce, quint, nominal, ...) and levels.
const BELL: [(f32, f32); 14] = [
    (0.5, 0.35),
    (1.0, 1.0),
    (1.19, 0.55),
    (1.5, 0.3),
    (2.0, 0.75),
    (2.51, 0.35),
    (2.66, 0.3),
    (3.01, 0.28),
    (4.1, 0.3),
    (5.43, 0.2),
    (6.79, 0.16),
    (8.3, 0.12),
    (9.9, 0.1),
    (11.6, 0.08),
];
/// "Aah" formants: (centre Hz, half bandwidth Hz, gain).
const FORMANTS: [(f32, f32, f32); 4] = [
    (750.0, 130.0, 1.0),
    (1200.0, 150.0, 0.6),
    (2700.0, 200.0, 0.25),
    (3500.0, 250.0, 0.15),
];

/// Base ratio and level of partial slot `i` (0-based) for a fundamental `f0`.
fn recipe(spec: Spectrum, i: usize, f0: f32) -> (f32, f32) {
    let k = (i + 1) as f32;
    match spec {
        Spectrum::Saw => (k, 1.0 / k),
        Spectrum::Square => (k, if i.is_multiple_of(2) { 1.0 / k } else { 0.0 }),
        Spectrum::Organ => (
            k,
            ORGAN
                .iter()
                .find(|(h, _)| *h == i + 1)
                .map(|(_, a)| *a)
                .unwrap_or(0.0),
        ),
        Spectrum::Bell => {
            if i < BELL.len() {
                BELL[i]
            } else {
                let j = (i + 1 - BELL.len()) as f32;
                (11.6 + 1.9 * j, 0.08 * 0.85f32.powf(j))
            }
        }
        Spectrum::Choir => {
            let f = f0 * k;
            let mut env = 0.06;
            for (fc, hb, g) in FORMANTS {
                let d = (f - fc) / hb;
                env += g / (1.0 + d * d);
            }
            (k, env / k.powf(0.7))
        }
        Spectrum::Glass => {
            // Sparse, increasingly spaced harmonics with slowly falling levels.
            let h = if i == 0 { 1.0 } else { (k.powf(1.55)).round() };
            let a = if i == 0 {
                1.0
            } else {
                0.55 * (i as f32).powf(-0.3)
            };
            (h, a)
        }
    }
}

#[derive(Clone)]
struct Params {
    spectrum: Spectrum,
    partials: usize,
    brightness: f32,
    cutoff: f32,
    resonance: f32,
    filter_env: f32,
    filter_decay: f32,
    odd_even: f32,
    stretch: f32,
    spectral_decay: f32,
    shimmer: f32,
    unison: usize,
    detune: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    gain: f32,
    // Derived unison layout.
    det: [f32; MAX_UNISON],
    pan_l: [f32; MAX_UNISON],
    pan_r: [f32; MAX_UNISON],
    uni_norm: f32,
}

impl Params {
    fn derive_unison(&mut self) {
        let n = self.unison;
        for u in 0..MAX_UNISON {
            let pos = if n <= 1 {
                0.0
            } else {
                u as f32 / (n - 1) as f32 * 2.0 - 1.0
            };
            self.det[u] = cents(pos * self.detune * 0.5);
            let (l, r) = pan_gains(pos * 0.75);
            self.pan_l[u] = l;
            self.pan_r[u] = r;
        }
        self.uni_norm = 1.0 / (n as f32).sqrt();
    }
}

#[derive(Clone)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    age: u64,
    /// Note waiting for this (stolen) voice to fade out.
    pending: Option<(u8, f32)>,
    f0: f32,
    /// Partial frequencies (Hz) of the centre unison copy.
    freq: [f32; MAX_PARTIALS],
    /// Normalised recipe level (0 above Nyquist).
    base: [f32; MAX_PARTIALS],
    /// Current (ramped) level.
    amp: [f32; MAX_PARTIALS],
    /// Spectral-decay state and per-chunk multiplier.
    sdec: [f32; MAX_PARTIALS],
    sdec_coef: [f32; MAX_PARTIALS],
    // Shimmer: smooth random segments per partial.
    sh_from: [f32; MAX_PARTIALS],
    sh_to: [f32; MAX_PARTIALS],
    sh_t: [f32; MAX_PARTIALS],
    sh_rate: [f32; MAX_PARTIALS],
    /// Phasor state (cos, sin) and per-sample rotation, per unison copy.
    re: [Bank; MAX_UNISON],
    im: [Bank; MAX_UNISON],
    rot_c: [Bank; MAX_UNISON],
    rot_s: [Bank; MAX_UNISON],
    fenv: f32,
    env: Adsr,
    rng: Rng,
    out_l: [f32; CR],
    out_r: [f32; CR],
    pos: usize,
    fresh: bool,
}

impl Voice {
    fn new() -> Voice {
        Voice {
            active: false,
            key: 0,
            velocity: 0.0,
            age: 0,
            pending: None,
            f0: 440.0,
            freq: [0.0; MAX_PARTIALS],
            base: [0.0; MAX_PARTIALS],
            amp: [0.0; MAX_PARTIALS],
            sdec: [1.0; MAX_PARTIALS],
            sdec_coef: [1.0; MAX_PARTIALS],
            sh_from: [0.0; MAX_PARTIALS],
            sh_to: [0.0; MAX_PARTIALS],
            sh_t: [0.0; MAX_PARTIALS],
            sh_rate: [0.0; MAX_PARTIALS],
            re: [[[1.0; LANES]; BLOCKS]; MAX_UNISON],
            im: [[[0.0; LANES]; BLOCKS]; MAX_UNISON],
            rot_c: [[[1.0; LANES]; BLOCKS]; MAX_UNISON],
            rot_s: [[[0.0; LANES]; BLOCKS]; MAX_UNISON],
            fenv: 0.0,
            env: Adsr::default(),
            rng: Rng::new(1),
            out_l: [0.0; CR],
            out_r: [0.0; CR],
            pos: CR,
            fresh: true,
        }
    }

    /// (Re)compute partial ratios, levels, increments and decay rates.
    fn configure(&mut self, p: &Params, sr: f32) {
        let b = 0.03 * p.stretch * p.stretch;
        let b_norm = 1.0 / (1.0 + b).sqrt();
        let tilt = -(1.0 - p.brightness) * 1.3;
        let even_g = 1.0 + p.odd_even.min(0.0);
        let odd_g = 1.0 - p.odd_even.max(0.0);
        let max_det = if p.unison > 1 {
            cents(p.detune * 0.5)
        } else {
            1.0
        };
        let limit = 0.45 * sr;
        let mut sum2 = 0.0;
        for i in 0..MAX_PARTIALS {
            if i >= p.partials {
                self.base[i] = 0.0;
                self.freq[i] = 0.0;
                self.amp[i] = 0.0;
                continue;
            }
            let (r0, a0) = recipe(p.spectrum, i, self.f0);
            let r = r0 * (1.0 + b * r0 * r0).sqrt() * b_norm;
            let parity = if i == 0 {
                1.0
            } else if i % 2 == 1 {
                even_g
            } else {
                odd_g
            };
            let a = a0 * r0.max(1.0).powf(tilt) * parity;
            sum2 += a * a;
            self.freq[i] = self.f0 * r;
            self.base[i] = a;
        }
        let norm = LEVEL / sum2.max(1e-12).sqrt();
        let f1 = self.freq[0];
        let sd = p.spectral_decay * p.spectral_decay;
        for i in 0..p.partials {
            let f = self.freq[i];
            self.base[i] = if f * max_det < limit {
                self.base[i] * norm
            } else {
                0.0
            };
            // Higher partials die away faster: rate grows with the distance
            // (in Hz) from the fundamental.
            let rate = sd * (f - f1).max(0.0) / 250.0;
            self.sdec_coef[i] = (-rate * CR as f32 / sr).exp();
        }
        for u in 0..p.unison {
            for i in 0..MAX_PARTIALS {
                let w = if self.base[i] > 0.0 {
                    (self.freq[i] * p.det[u] / sr).clamp(0.0, 0.49) as f64 * std::f64::consts::TAU
                } else {
                    0.0
                };
                let (s, c) = w.sin_cos();
                self.rot_c[u][i / LANES][i % LANES] = c as f32;
                self.rot_s[u][i / LANES][i % LANES] = s as f32;
            }
        }
    }

    fn start(&mut self, p: &Params, key: u8, velocity: f32, age: u64, seed: u32, sr: f32) {
        self.active = true;
        self.key = key;
        self.velocity = velocity;
        self.age = age;
        self.pending = None;
        self.f0 = midi_to_hz(key as f32);
        self.rng = Rng::new(seed);
        for u in 0..MAX_UNISON {
            for i in 0..MAX_PARTIALS {
                // The centre copy starts every partial at a zero crossing.
                let ph = if u == 0 { 0.0 } else { self.rng.unit() * TAU };
                self.re[u][i / LANES][i % LANES] = ph.cos();
                self.im[u][i / LANES][i % LANES] = ph.sin();
            }
        }
        self.configure(p, sr);
        for i in 0..MAX_PARTIALS {
            self.amp[i] = 0.0;
            self.sdec[i] = 1.0;
            self.sh_from[i] = self.rng.bipolar();
            self.sh_to[i] = self.rng.bipolar();
            self.sh_t[i] = self.rng.unit();
            self.sh_rate[i] = (0.12 + 0.8 * self.rng.unit()) * CR as f32 / sr;
        }
        self.fenv = 1.0;
        self.env = Adsr::default();
        set_env(&mut self.env, p, sr);
        self.env.trigger();
        self.pos = CR;
        self.fresh = true;
    }

    /// Update the partial levels and synthesise the next control chunk.
    fn chunk(&mut self, p: &Params, sr: f32) {
        let n = p.partials;
        // Cutoff (with envelope) for this chunk.
        let oct = p.filter_env * 6.0 * self.fenv * (0.5 + 0.5 * self.velocity);
        self.fenv *= (-4.6 * CR as f32 / (p.filter_decay.max(0.01) * sr)).exp();
        let fc = (p.cutoff * oct.exp2()).clamp(20.0, 80_000.0);
        let inv_fc = 1.0 / fc;
        let res = p.resonance * 3.0;
        const Q2: f32 = 5.0 * 5.0;
        let mut target = [0f32; MAX_PARTIALS];
        for (i, tgt) in target.iter_mut().enumerate().take(n) {
            let base = self.base[i];
            if base == 0.0 {
                continue;
            }
            let x = self.freq[i] * inv_fc;
            let x2 = x * x;
            let lp = 1.0 / (1.0 + x2 * x2);
            let d = x - 1.0 / x;
            let bump = 1.0 / (1.0 + Q2 * d * d);
            let filt = lp + res * bump;
            // Shimmer.
            let mut shim = 1.0;
            if p.shimmer > 0.0 {
                let mut t = self.sh_t[i] + self.sh_rate[i];
                if t >= 1.0 {
                    t -= 1.0;
                    self.sh_from[i] = self.sh_to[i];
                    self.sh_to[i] = self.rng.bipolar();
                }
                self.sh_t[i] = t;
                let s = t * t * (3.0 - 2.0 * t);
                let v = self.sh_from[i] + (self.sh_to[i] - self.sh_from[i]) * s;
                let depth = p.shimmer * (0.3 + 0.7 * (i as f32 / 6.0).min(1.0)) * 0.85;
                shim = (1.0 + depth * v).max(0.0);
            }
            self.sdec[i] *= self.sdec_coef[i];
            *tgt = base * filt * self.sdec[i] * shim;
        }
        if self.fresh {
            self.amp[..n].copy_from_slice(&target[..n]);
            self.fresh = false;
        }
        self.out_l = [0.0; CR];
        self.out_r = [0.0; CR];
        // Only blocks up to the last audible partial are synthesised.
        let last = (0..n)
            .rev()
            .find(|&i| self.amp[i] > SILENT || target[i] > SILENT)
            .map(|i| i + 1)
            .unwrap_or(0);
        let nb = last.div_ceil(LANES);
        if nb == 0 {
            self.amp[..n].copy_from_slice(&target[..n]);
            self.pos = 0;
            return;
        }
        let inv_cr = 1.0 / CR as f32;
        let mut a0: Bank = [[0.0; LANES]; BLOCKS];
        let mut da: Bank = [[0.0; LANES]; BLOCKS];
        for i in 0..last {
            a0[i / LANES][i % LANES] = self.amp[i];
            da[i / LANES][i % LANES] = (target[i] - self.amp[i]) * inv_cr;
        }
        for u in 0..p.unison {
            let re = &mut self.re[u];
            let im = &mut self.im[u];
            let rc = &self.rot_c[u];
            let rs = &self.rot_s[u];
            let (pl, pr) = (p.pan_l[u], p.pan_r[u]);
            for t in 0..CR {
                let tf = t as f32;
                let mut acc = [0f32; LANES];
                for b in 0..nb {
                    let (r, m, c, s, a, d) =
                        (&mut re[b], &mut im[b], &rc[b], &rs[b], &a0[b], &da[b]);
                    for j in 0..LANES {
                        let nr = r[j] * c[j] - m[j] * s[j];
                        let nm = r[j] * s[j] + m[j] * c[j];
                        r[j] = nr;
                        m[j] = nm;
                        acc[j] += nm * (a[j] + d[j] * tf);
                    }
                }
                let x = acc.iter().sum::<f32>();
                self.out_l[t] += x * pl;
                self.out_r[t] += x * pr;
            }
            // Keep the phasors on the unit circle.
            for b in 0..nb {
                let (r, m) = (&mut re[b], &mut im[b]);
                for j in 0..LANES {
                    let g = 1.5 - 0.5 * (r[j] * r[j] + m[j] * m[j]);
                    r[j] *= g;
                    m[j] *= g;
                }
            }
        }
        self.amp[..n].copy_from_slice(&target[..n]);
        self.pos = 0;
    }
}

fn set_env(env: &mut Adsr, p: &Params, sr: f32) {
    env.set(
        p.attack.max(0.0015),
        p.decay,
        p.sustain,
        p.release.max(0.005),
        sr,
    );
}

pub struct Additive {
    sr: f32,
    p: Params,
    voices: Vec<Voice>,
    rng: Rng,
    clock: u64,
}

impl Additive {
    pub fn new(sr: f32) -> Additive {
        let mut p = Params {
            spectrum: Spectrum::Saw,
            partials: 32,
            brightness: 0.6,
            cutoff: 9000.0,
            resonance: 0.15,
            filter_env: 0.2,
            filter_decay: 1.2,
            odd_even: 0.0,
            stretch: 0.0,
            spectral_decay: 0.3,
            shimmer: 0.2,
            unison: 1,
            detune: 8.0,
            attack: 0.01,
            decay: 0.8,
            sustain: 0.7,
            release: 0.8,
            gain: 0.6,
            det: [1.0; MAX_UNISON],
            pan_l: [1.0; MAX_UNISON],
            pan_r: [1.0; MAX_UNISON],
            uni_norm: 1.0,
        };
        p.derive_unison();
        Additive {
            sr,
            p,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            rng: Rng::new(0x9a15e),
            clock: 0,
        }
    }
}

impl Instrument for Additive {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        let p = &mut self.p;
        p.spectrum = Spectrum::parse(d.option("spectrum"));
        p.partials = (d.param("partials").round() as usize).clamp(1, MAX_PARTIALS);
        p.brightness = f("brightness").clamp(0.0, 1.0);
        p.cutoff = f("cutoff").max(20.0);
        p.resonance = f("resonance").clamp(0.0, 1.0);
        p.filter_env = f("filterEnv").clamp(-1.0, 1.0);
        p.filter_decay = f("filterDecay");
        p.odd_even = f("oddEven").clamp(-1.0, 1.0);
        p.stretch = f("stretch").clamp(0.0, 1.0);
        p.spectral_decay = f("spectralDecay").clamp(0.0, 1.0);
        p.shimmer = f("shimmer").clamp(0.0, 1.0);
        p.unison = (d.param("unison").round() as usize).clamp(1, MAX_UNISON);
        p.detune = f("detune");
        p.attack = f("attack");
        p.decay = f("decay");
        p.sustain = f("sustain");
        p.release = f("release");
        p.gain = f("gain");
        p.derive_unison();
        let sr = self.sr;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.configure(&self.p, sr);
            if v.pending.is_none() && !v.env.is_released() {
                set_env(&mut v.env, &self.p, sr);
            }
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = pick_voice(
                    &self.voices,
                    |v| v.active,
                    |v| v.env.is_released(),
                    |v| v.age,
                );
                let seed = self.rng.next_u32();
                let v = &mut self.voices[i];
                if v.active && v.env.level > 1e-4 {
                    // Steal: fade the old note out quickly, then start.
                    v.env.kill(self.sr);
                    v.pending = Some((key, velocity));
                    v.key = key;
                    v.age = self.clock;
                } else {
                    v.start(&self.p, key, velocity, self.clock, seed, self.sr);
                }
            }
            NoteKind::Off { key } => {
                for v in self.voices.iter_mut().filter(|v| v.active && v.key == key) {
                    if let Some((k, vel)) = v.pending {
                        // Released before it started: start it released.
                        v.pending = Some((k, -vel.abs().max(1e-6)));
                    } else if !v.env.is_released() {
                        v.env.release();
                    }
                }
            }
            NoteKind::AllOff => {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.pending = None;
                    v.env.release();
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let Additive {
            sr, p, voices, rng, ..
        } = self;
        let sr = *sr;
        for v in voices.iter_mut().filter(|v| v.active) {
            let mut vel_gain = (0.25 + 0.75 * v.velocity) * p.gain * p.uni_norm;
            for i in 0..left.len() {
                if v.pos >= CR {
                    v.chunk(p, sr);
                }
                let e = v.env.next() * vel_gain;
                left[i] += v.out_l[v.pos] * e;
                right[i] += v.out_r[v.pos] * e;
                v.pos += 1;
                if v.env.is_idle() {
                    match v.pending.take() {
                        Some((key, vel)) => {
                            v.start(p, key, vel.abs(), v.age, rng.next_u32(), sr);
                            if vel < 0.0 {
                                v.env.release();
                            }
                            vel_gain = (0.25 + 0.75 * v.velocity) * p.gain * p.uni_norm;
                        }
                        None => {
                            v.active = false;
                            break;
                        }
                    }
                }
            }
        }
    }
}
