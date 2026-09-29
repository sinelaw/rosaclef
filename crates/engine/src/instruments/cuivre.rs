//! "Cuivre": virtual analog synthesizer modelled on circuit behaviour.
//!
//! Two PolyBLEP oscillators (saw / pulse / triangle), a square sub one
//! octave down and noise feed a 2x oversampled filter: either a 4-pole
//! zero-delay-feedback ladder with tanh stage saturation, or an aggressive
//! 2-pole "screamer" whose resonance loop has a nonlinear (van der Pol like)
//! damping and saturating integrators. Each voice has static component
//! tolerances and a slow random drift of pitch and cutoff.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;
use std::f32::consts::PI;

/// Samples between control-rate updates (pitch, drift, envelopes -> cutoff).
const CONTROL: usize = 16;
/// Held keys remembered for mono / legato note priority.
const STACK: usize = 16;
/// Overall output scaling.
const OUT_SCALE: f32 = 0.42;
/// Glide times below this jump straight to the target.
const MIN_GLIDE: f32 = 0.0015;

/// Cheap rational tanh approximation (exact +-1 at +-3, smooth).
#[inline]
fn ftanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[inline]
fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaWave {
    Saw,
    Pulse,
    Triangle,
}

impl VaWave {
    fn parse(s: &str) -> VaWave {
        match s {
            "pulse" => VaWave::Pulse,
            "triangle" => VaWave::Triangle,
            _ => VaWave::Saw,
        }
    }
}

/// Band-limited oscillator sample (DC-free) for a phase in [0, 1).
#[inline]
fn va_osc(w: VaWave, ph: f32, dt: f32, pw: f32) -> f32 {
    match w {
        VaWave::Saw => 2.0 * ph - 1.0 - blep(ph, dt),
        VaWave::Pulse => {
            let mut p2 = ph - pw;
            if p2 < 0.0 {
                p2 += 1.0;
            }
            let naive = if ph < pw { 1.0 } else { -1.0 };
            naive + blep(ph, dt) - blep(p2, dt) - (2.0 * pw - 1.0)
        }
        VaWave::Triangle => 1.0 - 4.0 * (ph - 0.5).abs(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    Ladder,
    Screamer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Voicing {
    Poly,
    Mono,
    Legato,
}

#[derive(Clone, Copy)]
struct Params {
    wave1: VaWave,
    wave2: VaWave,
    model: Model,
    voicing: Voicing,
    osc2_ratio: f32,
    mix2: f32,
    pw: f32,
    sub: f32,
    noise: f32,
    drift: f32,
    cutoff: f32,
    resonance: f32,
    drive: f32,
    key_track: f32,
    filter_env: f32,
    glide: f32,
    gain: f32,
}

#[derive(Clone)]
struct Voice {
    active: bool,
    key: u8,
    vel: f32,
    age: u64,
    /// Current (gliding) pitch in semitones and its target.
    note: f32,
    target: f32,
    ph1: f32,
    ph2: f32,
    phs: f32,
    dt1: f32,
    dt2: f32,
    env: Adsr,
    /// Filter envelope, run at control rate.
    fenv: Adsr,
    rng: Rng,
    /// Static tolerances in -1..1: osc1 pitch, osc2 pitch, cutoff, pan, resonance.
    tol: [f32; 5],
    drift: [f32; 3],
    drift_tgt: [f32; 3],
    drift_timer: i32,
    /// Filter state (ladder: 4 stages; screamer: ic1, ic2).
    s: [f32; 4],
    bp: f32,
    g: f32,
    g_inc: f32,
    k: f32,
    prev_in: f32,
    dc: DcBlock,
    counter: usize,
    fresh: bool,
}

impl Voice {
    fn new(slot: usize) -> Voice {
        let mut rng = Rng::new(0xC0FFEE ^ ((slot as u32 + 1) * 0x9E37_79B9));
        let tol = [rng.bipolar(), rng.bipolar(), rng.bipolar(), rng.bipolar(), rng.bipolar()];
        Voice {
            active: false,
            key: 0,
            vel: 0.0,
            age: 0,
            note: 60.0,
            target: 60.0,
            ph1: 0.0,
            ph2: 0.0,
            phs: 0.0,
            dt1: 0.0,
            dt2: 0.0,
            env: Adsr::default(),
            fenv: Adsr::default(),
            rng,
            tol,
            drift: [0.0; 3],
            drift_tgt: [0.0; 3],
            drift_timer: 0,
            s: [0.0; 4],
            bp: 0.0,
            g: 0.0,
            g_inc: 0.0,
            k: 0.0,
            prev_in: 0.0,
            dc: DcBlock::default(),
            counter: 0,
            fresh: true,
        }
    }

    /// Clear the audio state of a silent voice before it starts.
    fn reset(&mut self) {
        self.s = [0.0; 4];
        self.bp = 0.0;
        self.prev_in = 0.0;
        self.dc = DcBlock::default();
        self.ph1 = self.rng.unit();
        self.ph2 = self.rng.unit();
        self.phs = 0.0;
        self.fresh = true;
        self.env.level = 0.0;
        self.fenv.level = 0.0;
        // Start the drift somewhere on its path rather than always at zero.
        for j in 0..3 {
            self.drift[j] = self.rng.bipolar() * 0.5;
        }
    }

    fn is_sounding(&self) -> bool {
        self.active && !self.env.is_idle()
    }
}

/// One oversampled step of the nonlinear ZDF ladder. `gg` = g / (1 + g).
#[inline]
fn ladder(s: &mut [f32; 4], x: f32, gg: f32, k: f32) -> f32 {
    let b = 1.0 - gg;
    let g2 = gg * gg;
    let g4 = g2 * g2;
    // Linear estimate of the output solves the zero-delay feedback loop.
    let sigma = b * (g2 * gg * s[0] + g2 * s[1] + gg * s[2] + s[3]);
    let y4 = (g4 * x + sigma) / (1.0 + k * g4);
    let mut xi = x - k * y4;
    for st in s.iter_mut() {
        let v = gg * (ftanh(xi) - ftanh(*st));
        let y = *st + v;
        *st = y + v;
        xi = y;
    }
    xi
}

/// One oversampled step of the screamer (nonlinear 2-pole TPT SVF).
#[inline]
fn screamer(s: &mut [f32; 4], bp: &mut f32, x: f32, g: f32, k: f32) -> f32 {
    // Cubic damping bounds the self-oscillation (a driven resonance "screams"
    // into the saturating integrators rather than blowing up).
    let ke = (k + 0.35 * *bp * *bp).min(4.0);
    let a1 = 1.0 / (1.0 + g * (g + ke));
    let a2 = g * a1;
    let a3 = g * a2;
    let v3 = x - s[1];
    let v1 = a1 * s[0] + a2 * v3;
    let v2 = s[1] + a2 * s[0] + a3 * v3;
    s[0] = 1.8 * ftanh((2.0 * v1 - s[0]) * (1.0 / 1.8));
    s[1] = 2.0 * v2 - s[1];
    *bp = v1;
    v2
}

pub struct Cuivre {
    sr: f32,
    p: Params,
    voices: Vec<Voice>,
    clock: u64,
    last_note: Option<f32>,
    stack: [u8; STACK],
    held: usize,
    amp: (f32, f32, f32, f32),
    fenv_times: (f32, f32, f32),
}

impl Cuivre {
    pub fn new(sr: f32) -> Cuivre {
        Cuivre {
            sr,
            p: Params {
                wave1: VaWave::Saw,
                wave2: VaWave::Saw,
                model: Model::Ladder,
                voicing: Voicing::Poly,
                osc2_ratio: 1.0,
                mix2: 0.5,
                pw: 0.5,
                sub: 0.3,
                noise: 0.0,
                drift: 0.3,
                cutoff: 1200.0,
                resonance: 0.3,
                drive: 0.3,
                key_track: 0.5,
                filter_env: 0.4,
                glide: 0.0,
                gain: 0.6,
            },
            voices: (0..MAX_VOICES).map(Voice::new).collect(),
            clock: 0,
            last_note: None,
            stack: [0; STACK],
            held: 0,
            amp: (0.005, 0.3, 0.7, 0.25),
            fenv_times: (0.002, 0.3, 0.2),
        }
    }

    fn stack_remove(&mut self, key: u8) {
        if let Some(pos) = self.stack[..self.held].iter().position(|&k| k == key) {
            self.stack.copy_within(pos + 1..self.held, pos);
            self.held -= 1;
        }
    }

    fn stack_push(&mut self, key: u8) {
        self.stack_remove(key);
        if self.held == STACK {
            self.stack.copy_within(1..STACK, 0);
            self.held -= 1;
        }
        self.stack[self.held] = key;
        self.held += 1;
    }

    fn trigger(v: &mut Voice) {
        v.active = true;
        v.counter = 0;
        v.env.trigger();
        v.fenv.trigger();
    }

    fn poly_on(&mut self, key: u8, vel: f32) {
        let i = pick_voice(&self.voices, |v| v.active, |v| v.env.is_released(), |v| v.age);
        let glide = self.p.glide > MIN_GLIDE;
        let from = if glide { self.last_note } else { None };
        let v = &mut self.voices[i];
        if !v.is_sounding() {
            v.reset();
        }
        v.key = key;
        v.vel = vel;
        v.age = self.clock;
        v.target = key as f32;
        v.note = from.unwrap_or(key as f32);
        Self::trigger(v);
        self.last_note = Some(key as f32);
    }

    fn mono_on(&mut self, key: u8, vel: f32) {
        let overlapping = self.held > 0;
        self.stack_push(key);
        let glide = self.p.glide > MIN_GLIDE;
        let voicing = self.p.voicing;
        let last = self.last_note;
        let v = &mut self.voices[0];
        v.age = self.clock;
        if voicing == Voicing::Legato && overlapping && v.active && !v.env.is_released() {
            // Tied note: glide to the new pitch, envelopes keep going.
            v.key = key;
            v.target = key as f32;
            if !glide {
                v.note = key as f32;
                v.counter = 0;
            }
        } else {
            let sounding = v.is_sounding();
            if !sounding {
                v.reset();
            }
            v.key = key;
            v.vel = vel;
            v.target = key as f32;
            v.note = match (voicing, glide, last) {
                (Voicing::Mono, true, Some(n)) => {
                    if sounding {
                        v.note
                    } else {
                        n
                    }
                }
                _ => key as f32,
            };
            Self::trigger(v);
        }
        self.last_note = Some(key as f32);
    }

    fn mono_off(&mut self, key: u8) {
        self.stack_remove(key);
        let glide = self.p.glide > MIN_GLIDE;
        let voicing = self.p.voicing;
        let top = if self.held > 0 { Some(self.stack[self.held - 1]) } else { None };
        let v = &mut self.voices[0];
        if !v.active || v.key != key || v.env.is_released() {
            return;
        }
        match top {
            Some(t) => {
                v.key = t;
                v.target = t as f32;
                if !glide {
                    v.note = t as f32;
                    v.counter = 0;
                }
                if voicing == Voicing::Mono {
                    Self::trigger(v);
                }
                self.last_note = Some(t as f32);
            }
            None => {
                v.env.release();
                v.fenv.release();
            }
        }
    }
}

impl Instrument for Cuivre {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        let voicing = match d.option("mode") {
            "mono" => Voicing::Mono,
            "legato" => Voicing::Legato,
            _ => Voicing::Poly,
        };
        if voicing != self.p.voicing {
            // Changing voicing mid-note: let everything release cleanly.
            self.held = 0;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                v.env.release();
                v.fenv.release();
            }
        }
        self.p = Params {
            wave1: VaWave::parse(d.option("wave1")),
            wave2: VaWave::parse(d.option("wave2")),
            model: if d.option("filter") == "screamer" { Model::Screamer } else { Model::Ladder },
            voicing,
            osc2_ratio: 2f32.powf(f("osc2Semi").round() / 12.0) * cents(f("osc2Detune")),
            mix2: f("mix2").clamp(0.0, 1.0),
            pw: f("pulseWidth").clamp(0.05, 0.95),
            sub: f("sub").clamp(0.0, 1.0),
            noise: f("noise").clamp(0.0, 1.0),
            drift: f("drift").clamp(0.0, 1.0),
            cutoff: f("cutoff").clamp(20.0, 20000.0),
            resonance: f("resonance").clamp(0.0, 1.0),
            drive: f("drive").clamp(0.0, 1.0),
            key_track: f("keyTrack").clamp(0.0, 1.0),
            filter_env: f("filterEnv").clamp(-1.0, 1.0),
            glide: f("glide").max(0.0),
            gain: f("gain").max(0.0),
        };
        self.amp = (f("attack"), f("decay"), f("sustain"), f("release"));
        self.fenv_times = (f("filterAttack"), f("filterDecay"), f("filterSustain"));
        let (a, dd, s, r) = self.amp;
        let (fa, fd, fs) = self.fenv_times;
        let ctrl_sr = self.sr / CONTROL as f32;
        for v in &mut self.voices {
            v.env.set(a, dd, s, r, self.sr);
            v.fenv.set(fa, fd, fs, r, ctrl_sr);
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let vel = velocity.clamp(0.0, 1.0);
                if self.p.voicing == Voicing::Poly {
                    self.poly_on(key, vel);
                } else {
                    self.mono_on(key, vel);
                }
            }
            NoteKind::Off { key } => {
                if self.p.voicing == Voicing::Poly {
                    for v in self.voices.iter_mut().filter(|v| v.active && v.key == key && !v.env.is_released()) {
                        v.env.release();
                        v.fenv.release();
                    }
                } else {
                    self.mono_off(key);
                }
            }
            NoteKind::AllOff => {
                self.held = 0;
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.env.release();
                    v.fenv.release();
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = self.p;
        let sr = self.sr;
        let n = left.len().min(right.len());
        let ctrl_sr = sr / CONTROL as f32;
        let osr = 2.0 * sr;
        let glide_coef = if p.glide > MIN_GLIDE { settle_coef(p.glide, ctrl_sr) } else { 0.0 };
        let drift_k = 1.0 - settle_coef(0.8, ctrl_sr);
        let fc_max = (0.45 * sr).min(20000.0);
        let dc_r = DcBlock::coef(sr);
        let mono = p.voicing != Voicing::Poly;
        let d = p.drift;
        let ladder_in = 0.8 + 2.6 * p.drive;
        let scream_in = 0.7 + 3.5 * p.drive;
        let mix1 = 1.0 - 0.25 * p.mix2;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let (pl, pr) = if mono { (1.0, 1.0) } else { pan_gains(v.tol[3] * 0.3) };
            let amp = p.gain * OUT_SCALE * (0.35 + 0.65 * v.vel);
            for i in 0..n {
                if v.counter % CONTROL == 0 {
                    // Pitch: glide, static tolerance and slow drift.
                    v.note = if glide_coef > 0.0 { v.target + (v.note - v.target) * glide_coef } else { v.target };
                    v.drift_timer -= 1;
                    if v.drift_timer <= 0 {
                        for j in 0..3 {
                            v.drift_tgt[j] = v.rng.bipolar();
                        }
                        v.drift_timer = ((0.2 + 0.8 * v.rng.unit()) * ctrl_sr) as i32;
                    }
                    for j in 0..3 {
                        v.drift[j] += (v.drift_tgt[j] - v.drift[j]) * drift_k;
                    }
                    let c1 = d * (v.tol[0] * 3.0 + v.drift[0] * 5.0);
                    let c2 = d * (v.tol[1] * 3.0 + v.drift[1] * 5.0);
                    let f1 = midi_to_hz(v.note + c1 * 0.01);
                    v.dt1 = (f1 / sr).min(0.45);
                    v.dt2 = (f1 * p.osc2_ratio * cents(c2 - c1) / sr).min(0.45);
                    // Cutoff.
                    let fe = v.fenv.next();
                    let oct = p.filter_env * 6.0 * fe * (0.6 + 0.4 * v.vel)
                        + p.key_track * (v.note - 60.0) / 12.0
                        + d * (v.tol[2] * 0.15 + v.drift[2] * 0.12);
                    let fc = (p.cutoff * 2f32.powf(oct)).clamp(16.0, fc_max);
                    let g_new = (PI * fc / osr).tan();
                    if v.fresh {
                        v.g = g_new;
                        v.g_inc = 0.0;
                        v.fresh = false;
                    } else {
                        v.g_inc = (g_new - v.g) * (1.0 / CONTROL as f32);
                    }
                    let res = (p.resonance + d * v.tol[4] * 0.03).clamp(0.0, 1.0);
                    v.k = match p.model {
                        Model::Ladder => 4.25 * res,
                        Model::Screamer => 2.0 - 2.06 * res,
                    };
                }
                v.counter += 1;

                // Oscillators.
                let o1 = va_osc(p.wave1, v.ph1, v.dt1, p.pw);
                let o2 = va_osc(p.wave2, v.ph2, v.dt2, p.pw);
                let dts = v.dt1 * 0.5;
                let mut ps2 = v.phs + 0.5;
                if ps2 >= 1.0 {
                    ps2 -= 1.0;
                }
                let sub = (if v.phs < 0.5 { 1.0 } else { -1.0 }) + blep(v.phs, dts) - blep(ps2, dts);
                v.ph1 += v.dt1;
                if v.ph1 >= 1.0 {
                    v.ph1 -= 1.0;
                }
                v.ph2 += v.dt2;
                if v.ph2 >= 1.0 {
                    v.ph2 -= 1.0;
                }
                v.phs += dts;
                if v.phs >= 1.0 {
                    v.phs -= 1.0;
                }
                let mut x = o1 * mix1 + o2 * p.mix2 + sub * p.sub;
                if p.noise > 0.0 {
                    x += v.rng.bipolar() * p.noise * 0.8;
                }

                // 2x oversampled filter (linear interpolation up, average down).
                v.g += v.g_inc;
                let g = v.g;
                let y = match p.model {
                    Model::Ladder => {
                        let x = x * ladder_in;
                        let xa = 0.5 * (v.prev_in + x);
                        v.prev_in = x;
                        let gg = g / (1.0 + g);
                        let ya = ladder(&mut v.s, xa, gg, v.k);
                        let yb = ladder(&mut v.s, x, gg, v.k);
                        0.5 * (ya + yb) * (1.0 + 0.3 * v.k)
                    }
                    Model::Screamer => {
                        let x = ftanh(x * scream_in);
                        let xa = 0.5 * (v.prev_in + x);
                        v.prev_in = x;
                        let ya = screamer(&mut v.s, &mut v.bp, xa, g, v.k);
                        let yb = screamer(&mut v.s, &mut v.bp, x, g, v.k);
                        1.4 * ftanh(0.5 * (ya + yb) * 0.9)
                    }
                };
                let out = v.dc.process(y, dc_r) * v.env.next() * amp;
                left[i] += out * pl;
                right[i] += out * pr;
            }
            if v.env.is_idle() {
                v.active = false;
            }
        }
    }
}
