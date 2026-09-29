//! "Nébula": granular texture instrument.
//!
//! Each voice runs a grain scheduler over a source buffer: either one of the
//! built-in sources (synthesised deterministically in `set_device` when the
//! source changes) or a project sample. Grains are Hann-windowed, read with
//! Hermite interpolation, transposed by the note, scattered in position,
//! pitch and stereo, optionally reversed. The voice envelope and an output
//! low-pass (`tone`) shape the cloud.

use super::{pick_voice, Instrument, NoteKind};
use crate::dsp::*;
use crate::samples::{SampleBank, SampleData, SampleRef};
use crate::Ctx;
use rosaclef_core::Device;
use std::sync::Arc;

const VOICES: usize = 12;
const MAX_GRAINS: usize = 32;
const WIN_SIZE: usize = 1024;
const SOURCE_SECONDS: f32 = 4.0;
const BLOCK: usize = 128;
/// Built-in sources are normalised to this RMS.
const SOURCE_RMS: f32 = 0.2;

#[derive(Clone, Copy, Default)]
struct Grain {
    active: bool,
    /// Samples to wait (inside the current block) before starting.
    delay: usize,
    pos: f64,
    step: f64,
    w: f32,
    dw: f32,
    gl: f32,
    gr: f32,
}

#[derive(Clone)]
struct Params {
    position: f32,
    spray: f32,
    grain_size: f32,
    density: f32,
    scatter: f32,
    drift: f32,
    spread: f32,
    reverse: f32,
    tone: f32,
    root: f32,
    attack: f32,
    release: f32,
    gain: f32,
}

#[derive(Clone)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    age: u64,
    pending: Option<(u8, f32)>,
    env: Adsr,
    grains: [Grain; MAX_GRAINS],
    /// Output samples until the next grain.
    next_grain: f32,
    rng: Rng,
    // Drift: smooth random wandering of the position.
    drift_from: f32,
    drift_to: f32,
    drift_t: f32,
    drift_rate: f32,
}

impl Voice {
    fn new() -> Voice {
        Voice {
            active: false,
            key: 0,
            velocity: 0.0,
            age: 0,
            pending: None,
            env: Adsr::default(),
            grains: [Grain::default(); MAX_GRAINS],
            next_grain: 0.0,
            rng: Rng::new(1),
            drift_from: 0.0,
            drift_to: 0.0,
            drift_t: 0.0,
            drift_rate: 0.0,
        }
    }

    fn start(&mut self, p: &Params, key: u8, velocity: f32, age: u64, seed: u32, sr: f32) {
        self.active = true;
        self.key = key;
        self.velocity = velocity;
        self.age = age;
        self.pending = None;
        self.rng = Rng::new(seed);
        for g in &mut self.grains {
            g.active = false;
        }
        self.next_grain = 0.0;
        self.drift_from = self.rng.bipolar();
        self.drift_to = self.rng.bipolar();
        self.drift_t = self.rng.unit();
        self.drift_rate = 0.0;
        self.env = Adsr::default();
        set_env(&mut self.env, p, sr);
        self.env.trigger();
    }

    fn drift_value(&self) -> f32 {
        let t = self.drift_t;
        let s = t * t * (3.0 - 2.0 * t);
        self.drift_from + (self.drift_to - self.drift_from) * s
    }

    fn advance_drift(&mut self, n: usize, sr: f32) {
        // One segment every ~1.5–4 s.
        if self.drift_rate == 0.0 {
            self.drift_rate = 1.0 / ((1.5 + 2.5 * self.rng.unit()) * sr);
        }
        self.drift_t += self.drift_rate * n as f32;
        if self.drift_t >= 1.0 {
            self.drift_t -= 1.0;
            self.drift_from = self.drift_to;
            self.drift_to = self.rng.bipolar();
            self.drift_rate = 1.0 / ((1.5 + 2.5 * self.rng.unit()) * sr);
        }
    }

    /// Start a grain `delay` samples into the current block.
    fn spawn(&mut self, p: &Params, src: &SampleData, sr: f32, delay: usize) {
        let Some(slot) = self.grains.iter().position(|g| !g.active) else { return };
        let len = src.len() as f64;
        if len < 4.0 {
            return;
        }
        let drift = self.drift_value() * p.drift * 0.35;
        let rng = &mut self.rng;
        let semis = self.key as f32 - p.root + p.scatter * rng.bipolar();
        let step = 2f64.powf(semis as f64 / 12.0) * (src.sample_rate / sr) as f64;
        let size = (p.grain_size * sr).max(16.0);
        let span = size as f64 * step;
        let mut c = p.position + drift + p.spray * 0.5 * rng.bipolar();
        // Reflect into 0..1.
        c = c.rem_euclid(2.0);
        if c > 1.0 {
            c = 2.0 - c;
        }
        let start = c as f64 * (len - span).max(0.0);
        let reverse = rng.unit() < p.reverse;
        let pan = p.spread * rng.bipolar();
        let (gl, gr) = pan_gains(pan);
        self.grains[slot] = Grain {
            active: true,
            delay,
            pos: if reverse { start + span } else { start },
            step: if reverse { -step } else { step },
            w: 0.0,
            dw: 1.0 / size,
            gl,
            gr,
        };
    }

    /// Render one block (<= BLOCK) of this voice into `out_l`/`out_r` (added).
    #[allow(clippy::too_many_arguments)]
    fn render(&mut self, p: &Params, src: &SampleData, win: &[f32; WIN_SIZE + 1], sr: f32, norm: f32, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        self.advance_drift(n, sr);
        // Schedule the grains that start in this block.
        let interval = sr / p.density.max(0.1);
        while (self.next_grain as usize) < n {
            let at = self.next_grain.max(0.0) as usize;
            self.spawn(p, src, sr, at);
            self.next_grain += interval * (0.65 + 0.7 * self.rng.unit());
        }
        self.next_grain -= n as f32;

        let mut bl = [0f32; BLOCK];
        let mut br = [0f32; BLOCK];
        let ch_l: &[f32] = &src.channels[0];
        let ch_r: &[f32] = src.channels.get(1).unwrap_or(&src.channels[0]);
        let stereo = src.channels.len() > 1;
        for g in self.grains.iter_mut().filter(|g| g.active) {
            let from = g.delay.min(n);
            g.delay = 0;
            for i in from..n {
                let wp = g.w * WIN_SIZE as f32;
                let wi = (wp as usize).min(WIN_SIZE - 1);
                let wf = wp - wi as f32;
                let w = win[wi] + (win[wi + 1] - win[wi]) * wf;
                let sl = hermite(ch_l, g.pos) * w;
                let sr_ = if stereo { hermite(ch_r, g.pos) * w } else { sl };
                bl[i] += sl * g.gl;
                br[i] += sr_ * g.gr;
                g.pos += g.step;
                g.w += g.dw;
                if g.w >= 1.0 {
                    g.active = false;
                    break;
                }
            }
        }
        let vel_gain = (0.25 + 0.75 * self.velocity) * p.gain * norm;
        for i in 0..n {
            let e = self.env.next() * vel_gain;
            out_l[i] += bl[i] * e;
            out_r[i] += br[i] * e;
        }
    }
}

fn set_env(env: &mut Adsr, p: &Params, sr: f32) {
    env.set(p.attack.max(0.002), 0.001, 1.0, p.release.max(0.01), sr);
}

pub struct Nebula {
    sr: f32,
    p: Params,
    voices: Vec<Voice>,
    rng: Rng,
    clock: u64,
    source: String,
    sample_path: String,
    builtin_name: String,
    builtin: Option<SampleRef>,
    user: Option<SampleRef>,
    window: Box<[f32; WIN_SIZE + 1]>,
    lp_l: Svf,
    lp_r: Svf,
    tone: f32,
    dc_l: DcBlock,
    dc_r: DcBlock,
    dc_coef: f32,
}

impl Nebula {
    pub fn new(sr: f32) -> Nebula {
        let mut window = Box::new([0f32; WIN_SIZE + 1]);
        for (i, w) in window.iter_mut().enumerate() {
            let t = i as f32 / WIN_SIZE as f32;
            *w = 0.5 - 0.5 * (TAU * t).cos();
        }
        Nebula {
            sr,
            p: Params {
                position: 0.3,
                spray: 0.2,
                grain_size: 0.12,
                density: 20.0,
                scatter: 0.0,
                drift: 0.2,
                spread: 0.7,
                reverse: 0.0,
                tone: 9000.0,
                root: 60.0,
                attack: 0.4,
                release: 1.5,
                gain: 0.7,
            },
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            rng: Rng::new(0xe6b1a),
            clock: 0,
            source: String::new(),
            sample_path: String::new(),
            builtin_name: String::new(),
            builtin: None,
            user: None,
            window,
            lp_l: Svf::default(),
            lp_r: Svf::default(),
            tone: 0.0,
            dc_l: DcBlock::default(),
            dc_r: DcBlock::default(),
            dc_coef: DcBlock::coef(sr),
        }
    }

    fn current(&self) -> Option<SampleRef> {
        if self.source == "sample" {
            self.user.clone()
        } else {
            self.builtin.clone()
        }
    }
}

impl Instrument for Nebula {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        self.p = Params {
            position: f("position").clamp(0.0, 1.0),
            spray: f("spray").clamp(0.0, 1.0),
            grain_size: f("grainSize").clamp(0.005, 1.0),
            density: f("density").clamp(0.5, 200.0),
            scatter: f("scatter").max(0.0),
            drift: f("drift").clamp(0.0, 1.0),
            spread: f("spread").clamp(0.0, 1.0),
            reverse: f("reverse").clamp(0.0, 1.0),
            tone: f("tone").clamp(20.0, 20000.0),
            root: f("root"),
            attack: f("attack"),
            release: f("release"),
            gain: f("gain"),
        };
        let source = d.option("source").to_string();
        if source != "sample" && (source != self.builtin_name || self.builtin.is_none()) {
            self.builtin = Some(Arc::new(synth_source(&source, self.sr)));
            self.builtin_name = source.clone();
        }
        self.source = source;
        let path = d.option("sample").to_string();
        if path != self.sample_path {
            self.sample_path = path;
            self.user = None;
        }
        let sr = self.sr;
        for v in self.voices.iter_mut().filter(|v| v.active && v.pending.is_none() && !v.env.is_released()) {
            set_env(&mut v.env, &self.p, sr);
        }
    }

    fn set_samples(&mut self, bank: &SampleBank) {
        self.user = if self.sample_path.is_empty() { None } else { bank.get(&self.sample_path) };
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = pick_voice(&self.voices, |v| v.active, |v| v.env.is_released(), |v| v.age);
                let seed = self.rng.next_u32();
                let v = &mut self.voices[i];
                if v.active && v.env.level > 1e-4 {
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
        let Some(src) = self.current() else {
            // No source: let voices finish silently.
            for v in self.voices.iter_mut() {
                v.active = false;
            }
            return;
        };
        let sr = self.sr;
        // Loudness compensation for overlapping grains.
        let overlap = self.p.density * self.p.grain_size;
        let norm = 1.0 / (1.0 + 0.5 * overlap).sqrt();
        let mut done = 0;
        while done < left.len() {
            let n = (left.len() - done).min(BLOCK);
            let mut bl = [0f32; BLOCK];
            let mut br = [0f32; BLOCK];
            let mut any = false;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                any = true;
                v.render(&self.p, &src, &self.window, sr, norm, &mut bl[..n], &mut br[..n]);
                if v.env.is_idle() {
                    match v.pending.take() {
                        Some((key, vel)) => {
                            let seed = self.rng.next_u32();
                            let age = v.age;
                            v.start(&self.p, key, vel.abs(), age, seed, sr);
                            if vel < 0.0 {
                                v.env.release();
                            }
                        }
                        None => v.active = false,
                    }
                }
            }
            // Smooth the tone control (per block, in the log domain).
            let target = self.p.tone.min(sr * 0.45);
            self.tone = if self.tone <= 0.0 { target } else { self.tone * (target / self.tone).powf(0.15) };
            self.lp_l.set(self.tone, 0.1, sr);
            self.lp_r.set(self.tone, 0.1, sr);
            if !any && bl[..n].iter().all(|x| *x == 0.0) {
                // Keep the filters settled while idle.
                self.lp_l.reset();
                self.lp_r.reset();
            } else {
                let r = self.dc_coef;
                for i in 0..n {
                    let l = self.lp_l.process(bl[i], FilterMode::Lowpass);
                    let rr = self.lp_r.process(br[i], FilterMode::Lowpass);
                    left[done + i] += self.dc_l.process(l, r);
                    right[done + i] += self.dc_r.process(rr, r);
                }
            }
            done += n;
        }
    }
}

// ------------------------------------------------------------- sources

/// Synthesise a built-in grain source (deterministic).
fn synth_source(name: &str, sr: f32) -> SampleData {
    let n = (SOURCE_SECONDS * sr) as usize;
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    match name {
        "bowl" => bowl(&mut l, &mut r, sr),
        "ember" => ember(&mut l, &mut r, sr),
        "strings" => strings(&mut l, &mut r, sr),
        "air" => air(&mut l, &mut r, sr),
        _ => choir(&mut l, &mut r, sr),
    }
    finish(&mut l, &mut r, sr);
    SampleData { sample_rate: sr, channels: vec![l, r] }
}

/// Remove DC, fade the edges and normalise the level.
fn finish(l: &mut [f32], r: &mut [f32], sr: f32) {
    let coef = DcBlock::coef(sr);
    for ch in [&mut *l, &mut *r] {
        let mean = ch.iter().sum::<f32>() / ch.len().max(1) as f32;
        let mut dc = DcBlock::default();
        for x in ch.iter_mut() {
            *x = dc.process(*x - mean, coef);
        }
    }
    let fade = (0.01 * sr) as usize;
    let n = l.len();
    for i in 0..fade.min(n / 2) {
        let g = i as f32 / fade as f32;
        l[i] *= g;
        r[i] *= g;
        l[n - 1 - i] *= g;
        r[n - 1 - i] *= g;
    }
    let sum2: f32 = l.iter().chain(r.iter()).map(|x| x * x).sum();
    let rms = (sum2 / (2 * n).max(1) as f32).sqrt();
    let peak = l.iter().chain(r.iter()).fold(0f32, |m, x| m.max(x.abs()));
    let g = (SOURCE_RMS / rms.max(1e-9)).min(0.95 / peak.max(1e-9));
    for x in l.iter_mut().chain(r.iter_mut()) {
        *x *= g;
    }
}

const C4: f32 = 261.625_58;

/// Smooth random walk in -1..1 (for vibrato drift and the like).
struct Wander {
    from: f32,
    to: f32,
    t: f32,
    rate: f32,
}

impl Wander {
    fn new(rng: &mut Rng, hz: f32, sr: f32) -> Wander {
        Wander { from: rng.bipolar(), to: rng.bipolar(), t: rng.unit(), rate: hz / sr }
    }
    #[inline]
    fn next(&mut self, rng: &mut Rng) -> f32 {
        self.t += self.rate;
        if self.t >= 1.0 {
            self.t -= 1.0;
            self.from = self.to;
            self.to = rng.bipolar();
        }
        let s = self.t * self.t * (3.0 - 2.0 * self.t);
        self.from + (self.to - self.from) * s
    }
}

/// Detuned saw section with vibrato: returns per-sample (left, right) sums.
struct SawSection {
    freq: Vec<f32>,
    phase: Vec<f32>,
    vib_rate: Vec<f32>,
    vib_depth: Vec<f32>,
    vib_phase: Vec<f32>,
    pan_l: Vec<f32>,
    pan_r: Vec<f32>,
    wander: Vec<Wander>,
}

impl SawSection {
    fn new(rng: &mut Rng, notes: &[f32], detune: f32, vib: (f32, f32), vib_depth: f32, sr: f32) -> SawSection {
        let k = notes.len();
        let mut s = SawSection { freq: vec![], phase: vec![], vib_rate: vec![], vib_depth: vec![], vib_phase: vec![], pan_l: vec![], pan_r: vec![], wander: vec![] };
        for (i, f) in notes.iter().enumerate() {
            s.freq.push(f * cents(detune * rng.bipolar()));
            s.phase.push(rng.unit());
            s.vib_rate.push(vib.0 + (vib.1 - vib.0) * rng.unit());
            s.vib_depth.push(vib_depth * (0.6 + 0.4 * rng.unit()));
            s.vib_phase.push(rng.unit());
            let pos = if k <= 1 { 0.0 } else { i as f32 / (k - 1) as f32 * 2.0 - 1.0 };
            let (pl, pr) = pan_gains(pos * 0.8);
            s.pan_l.push(pl);
            s.pan_r.push(pr);
            let hz = 0.4 + 0.5 * rng.unit();
            s.wander.push(Wander::new(rng, hz, sr));
        }
        s
    }

    #[inline]
    fn next(&mut self, rng: &mut Rng, sr: f32) -> (f32, f32) {
        let (mut l, mut r) = (0.0, 0.0);
        for i in 0..self.freq.len() {
            self.vib_phase[i] = (self.vib_phase[i] + self.vib_rate[i] / sr).fract();
            let w = self.wander[i].next(rng);
            let c = self.vib_depth[i] * (TAU * self.vib_phase[i]).sin() + 4.0 * w;
            let dt = (self.freq[i] * cents(c) / sr).min(0.49);
            let s = osc(Wave::Saw, self.phase[i], dt, rng);
            self.phase[i] += dt;
            if self.phase[i] >= 1.0 {
                self.phase[i] -= 1.0;
            }
            l += s * self.pan_l[i];
            r += s * self.pan_r[i];
        }
        (l, r)
    }
}

/// Sustained "aah" choir at C4 (with a few voices an octave below).
fn choir(l: &mut [f32], r: &mut [f32], sr: f32) {
    let mut rng = Rng::new(0xc401);
    let notes = [C4, C4, C4, C4, C4 * 0.5, C4 * 0.5, C4, C4 * 0.5];
    let mut sec = SawSection::new(&mut rng, &notes, 9.0, (4.6, 5.8), 18.0, sr);
    // (centre, Q, gain) of the "aah" formants.
    let formants = [(780.0, 6.0, 1.0), (1150.0, 8.0, 0.7), (2800.0, 12.0, 0.32), (3500.0, 14.0, 0.18)];
    let mut bank_l: Vec<Biquad> = vec![Biquad::default(); formants.len()];
    let mut bank_r: Vec<Biquad> = vec![Biquad::default(); formants.len()];
    for (i, (f, q, _)) in formants.iter().enumerate() {
        bank_l[i].set(BiquadKind::Bandpass, *f, *q, 0.0, sr);
        bank_r[i].set(BiquadKind::Bandpass, *f * 1.02, *q, 0.0, sr);
    }
    let mut body_l = OnePole::default();
    let mut body_r = OnePole::default();
    body_l.set(900.0, sr);
    body_r.set(900.0, sr);
    let mut breath = Biquad::default();
    breath.set(BiquadKind::Bandpass, 3000.0, 1.0, 0.0, sr);
    let mut swell = Wander::new(&mut rng, 0.35, sr);
    for i in 0..l.len() {
        let (sl, sr_) = sec.next(&mut rng, sr);
        let mut ol = body_l.process(sl) * 0.15;
        let mut or = body_r.process(sr_) * 0.15;
        for (k, (_, _, g)) in formants.iter().enumerate() {
            ol += bank_l[k].process(sl) * g;
            or += bank_r[k].process(sr_) * g;
        }
        let b = breath.process(rng.bipolar()) * 0.04;
        let a = 1.0 + 0.12 * swell.next(&mut rng);
        l[i] = (ol + b) * a;
        r[i] = (or + b) * a;
    }
}

/// Singing bowl: inharmonic beating partials, struck then rubbed.
fn bowl(l: &mut [f32], r: &mut [f32], sr: f32) {
    let mut rng = Rng::new(0xb0e1);
    let ratios = [1.0, 2.76, 5.18, 8.23, 11.9, 16.1];
    let amps = [1.0, 0.62, 0.38, 0.24, 0.13, 0.08];
    let taus = [9.0, 5.5, 3.2, 2.2, 1.5, 1.1];
    struct P {
        inc: [f64; 2],
        ph: [f64; 4],
        amp: f32,
        dec: f32,
    }
    let mut parts: Vec<P> = ratios
        .iter()
        .zip(amps.iter())
        .zip(taus.iter())
        .map(|((r, a), t)| {
            let f = (C4 * r) as f64;
            let beat = 0.5 + 1.8 * rng.unit() as f64;
            P {
                inc: [(f - beat * 0.5) / sr as f64, (f + beat * 0.5) / sr as f64],
                ph: [rng.unit() as f64, rng.unit() as f64, rng.unit() as f64, rng.unit() as f64],
                amp: *a,
                dec: (-1.0 / (t * sr)).exp(),
            }
        })
        .collect();
    let mut strike = [1f32; 6];
    let rub_coef = (-1.0 / (1.2 * sr)).exp();
    let mut rub = 1.0f32;
    let mut sway = Wander::new(&mut rng, 0.2, sr);
    for i in 0..l.len() {
        rub *= rub_coef;
        let sustain = 0.35 * (1.0 - rub);
        let s = sway.next(&mut rng);
        let (mut ol, mut or) = (0.0f32, 0.0f32);
        for (k, p) in parts.iter_mut().enumerate() {
            strike[k] *= p.dec;
            let a = p.amp * (strike[k] + sustain / (1.0 + k as f32 * 0.6));
            // Left and right hear the beating pair with different phases.
            let t = std::f64::consts::TAU;
            let x0 = (p.ph[0] * t).sin() + (p.ph[1] * t).sin();
            let x1 = (p.ph[2] * t).sin() + (p.ph[3] * t).sin() * (1.0 + 0.1 * s as f64);
            p.ph[0] = (p.ph[0] + p.inc[0]).fract();
            p.ph[1] = (p.ph[1] + p.inc[1]).fract();
            p.ph[2] = (p.ph[2] + p.inc[0]).fract();
            p.ph[3] = (p.ph[3] + p.inc[1]).fract();
            ol += x0 as f32 * a;
            or += x1 as f32 * a;
        }
        // Soft mallet at the very start.
        let att = (i as f32 / (0.004 * sr)).min(1.0);
        l[i] = ol * att;
        r[i] = or * att;
    }
}

/// Ember: low warm hum, roaring noise bed and fire crackles.
fn ember(l: &mut [f32], r: &mut [f32], sr: f32) {
    let mut rng = Rng::new(0xe3be);
    let f0 = C4 * 0.5;
    let mut ph = [[0f32; 10]; 2];
    let det = [cents(-4.0), cents(4.0)];
    let mut breathe = Wander::new(&mut rng, 0.25, sr);
    let mut roar_lp = [OnePole::default(), OnePole::default()];
    roar_lp[0].set(700.0, sr);
    roar_lp[1].set(760.0, sr);
    let mut roar_amp = Wander::new(&mut rng, 0.6, sr);
    let mut hp = [OnePole::default(), OnePole::default()];
    hp[0].set(1800.0, sr);
    hp[1].set(1800.0, sr);
    let mut crackle = 0.0f32;
    let mut crackle_coef = 0.0f32;
    let mut cpan = (1.0f32, 1.0f32);
    let crackle_p = 22.0 / sr;
    let mut sub_ph = 0.0f32;
    for i in 0..l.len() {
        let b = 1.0 + 0.25 * breathe.next(&mut rng);
        let mut hum = [0f32; 2];
        for c in 0..2 {
            for (k, p) in ph[c].iter_mut().enumerate() {
                let kk = (k + 1) as f32;
                hum[c] += (*p * TAU).sin() / kk.powf(1.6);
                *p = (*p + f0 * kk * det[c] / sr).fract();
            }
        }
        let sub = (sub_ph * TAU).sin() * 0.35;
        sub_ph = (sub_ph + f0 * 0.5 / sr).fract();
        let ra = 0.12 * (1.0 + 0.6 * roar_amp.next(&mut rng));
        let roar = [roar_lp[0].process(rng.bipolar()) * ra, roar_lp[1].process(rng.bipolar()) * ra];
        if rng.unit() < crackle_p {
            let a = rng.unit();
            crackle = 0.4 + 1.6 * a * a;
            crackle_coef = (-1.0 / ((0.0008 + 0.004 * rng.unit()) * sr)).exp();
            cpan = pan_gains(rng.bipolar() * 0.9);
        }
        let cn = rng.bipolar() * crackle;
        crackle *= crackle_coef;
        let cl = cn * cpan.0;
        let cr = cn * cpan.1;
        let cl = cl - hp[0].process(cl);
        let cr = cr - hp[1].process(cr);
        l[i] = (hum[0] * 0.55 + sub) * b + roar[0] * 3.0 + cl * 0.5;
        r[i] = (hum[1] * 0.55 + sub) * b + roar[1] * 3.0 + cr * 0.5;
    }
}

/// String ensemble at C4/C3: detuned saws, soft attack, warm filtering.
fn strings(l: &mut [f32], r: &mut [f32], sr: f32) {
    let mut rng = Rng::new(0x5751);
    let notes = [C4, C4, C4, C4, C4, C4 * 0.5, C4 * 0.5, C4 * 0.5, C4, C4 * 0.5];
    let mut sec = SawSection::new(&mut rng, &notes, 11.0, (5.0, 6.2), 9.0, sr);
    let mut lp = [Svf::default(), Svf::default()];
    lp[0].set(3000.0, 0.1, sr);
    lp[1].set(3200.0, 0.1, sr);
    let mut body = [Biquad::default(), Biquad::default()];
    body[0].set(BiquadKind::Peak, 1100.0, 1.0, 4.0, sr);
    body[1].set(BiquadKind::Peak, 1000.0, 1.0, 4.0, sr);
    let mut low = [Biquad::default(), Biquad::default()];
    low[0].set(BiquadKind::Highpass, 90.0, 0.7, 0.0, sr);
    low[1].set(BiquadKind::Highpass, 90.0, 0.7, 0.0, sr);
    let attack = 0.35 * sr;
    let mut bow = Wander::new(&mut rng, 0.5, sr);
    for i in 0..l.len() {
        let (sl, sr_) = sec.next(&mut rng, sr);
        let a = (i as f32 / attack).min(1.0);
        let a = a * a * (3.0 - 2.0 * a) * (1.0 + 0.08 * bow.next(&mut rng));
        l[i] = low[0].process(body[0].process(lp[0].process(sl, FilterMode::Lowpass))) * a;
        r[i] = low[1].process(body[1].process(lp[1].process(sr_, FilterMode::Lowpass))) * a;
    }
}

/// Breathy band-passed noise with resonances on C harmonics.
fn air(l: &mut [f32], r: &mut [f32], sr: f32) {
    let mut rng = Rng::new(0xa112);
    let harmonics = [(1.0, 1.0), (2.0, 0.85), (3.0, 0.6), (4.0, 0.5), (6.0, 0.35), (8.0, 0.25)];
    let mut res_l: Vec<Biquad> = vec![Biquad::default(); harmonics.len()];
    let mut res_r: Vec<Biquad> = vec![Biquad::default(); harmonics.len()];
    for (i, (h, _)) in harmonics.iter().enumerate() {
        res_l[i].set(BiquadKind::Bandpass, C4 * h, 45.0, 0.0, sr);
        res_r[i].set(BiquadKind::Bandpass, C4 * h * cents(3.0), 45.0, 0.0, sr);
    }
    let mut broad = [Biquad::default(), Biquad::default()];
    broad[0].set(BiquadKind::Bandpass, 1800.0, 0.8, 0.0, sr);
    broad[1].set(BiquadKind::Bandpass, 2200.0, 0.8, 0.0, sr);
    let mut breath = Wander::new(&mut rng, 0.3, sr);
    let mut flutter = Wander::new(&mut rng, 3.0, sr);
    for i in 0..l.len() {
        let nl = rng.bipolar();
        let nr = rng.bipolar();
        let (mut tl, mut tr) = (0.0, 0.0);
        for (k, (_, g)) in harmonics.iter().enumerate() {
            tl += res_l[k].process(nl) * g;
            tr += res_r[k].process(nr) * g;
        }
        let a = 0.65 + 0.35 * breath.next(&mut rng);
        let f = 1.0 + 0.1 * flutter.next(&mut rng);
        l[i] = (broad[0].process(nl) * 0.35 + tl * 7.0) * a * f;
        r[i] = (broad[1].process(nr) * 0.35 + tr * 7.0) * a * f;
    }
}
