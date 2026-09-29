//! "Sextant": six-operator phase-modulation (FM) synthesizer.
//!
//! Six sine operators, each with its own frequency ratio, output level and
//! ADSR envelope (the release is shared). The `algorithm` option routes them.
//! Modulators are always evaluated before the operators they modulate (a
//! modulator has a higher number than its target), so every algorithm is a
//! single pass per sample. Operator 6 is always the top of the routing and
//! carries the self-feedback loop.
//!
//! `A→B` means operator A phase-modulates operator B; carriers are heard.
//!
//! | algorithm | routing                         | carriers | depth | character                              |
//! |-----------|---------------------------------|----------|-------|----------------------------------------|
//! | `stack`   | 6→5→4→3→2→1                     | 1        | 1.0   | one deep chain: harsh, metallic, bass  |
//! | `twin`    | 3→2→1, 6→5→4                    | 1, 4     | 1.0   | two 3-op stacks (layered timbres)      |
//! | `triad`   | 2→1, 4→3, 6→5                   | 1, 3, 5  | 1.0   | three 2-op pairs (classic tine pianos) |
//! | `ep`      | 2→1, 4→3, 6→5→3                 | 1, 3     | 1.0   | body pair + bright tine pair on op 3   |
//! | `bell`    | 3→2→1, 5→4, 6→4                 | 1, 4     | 1.0   | inharmonic ratios on 4 feed a 2nd bell |
//! | `organ`   | —                               | 1–6      | —     | six additive drawbars (6 has feedback) |
//! | `pad`     | 4→1, 5→2, 6→3                   | 1, 2, 3  | 0.5   | three gently modulated carriers        |
//! | `brass`   | 6→5→4→1, 3→1                    | 1, 2     | 1.0   | feedback stack + plain carrier 2       |
//!
//! Modulation index (radians) = 8 × level² × depth × velocity × key scaling ×
//! envelope; the quadratic taper gives fine control of subtle indices.
//! Feedback uses the average of the last two outputs of operator 6 (stable
//! up to full feedback). Carrier outputs are normalized by their count.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

const OPS: usize = 6;
const TOP: usize = OPS - 1;
const SINE_BITS: u32 = 12;
const SINE_LEN: usize = 1 << SINE_BITS;
/// Largest modulation index, in cycles (8 radians).
const MOD_DEPTH: f32 = 8.0 / TAU;
/// Largest self-feedback, in cycles (about 1.9 radians).
const FB_DEPTH: f32 = 0.3;
/// Voice-steal fade time.
const STEAL_TIME: f32 = 0.004;
const CHUNK: usize = 128;

/// Per-operator cents offsets (× `detune`). Operator 1 stays in tune.
const DETUNE_SPREAD: [f32; OPS] = [0.0, 0.55, -0.8, 1.0, -0.45, 0.7];

/// Parameter keys per operator (no formatting when settings change, which
/// automation does on the audio thread).
const OP_KEYS: [[&str; 5]; OPS] = [
    ["op1Ratio", "op1Level", "op1Attack", "op1Decay", "op1Sustain"],
    ["op2Ratio", "op2Level", "op2Attack", "op2Decay", "op2Sustain"],
    ["op3Ratio", "op3Level", "op3Attack", "op3Decay", "op3Sustain"],
    ["op4Ratio", "op4Level", "op4Attack", "op4Decay", "op4Sustain"],
    ["op5Ratio", "op5Level", "op5Attack", "op5Decay", "op5Sustain"],
    ["op6Ratio", "op6Level", "op6Attack", "op6Decay", "op6Sustain"],
];

struct Algorithm {
    name: &'static str,
    /// `mods[i]`: bit j set when operator j modulates operator i (j > i).
    mods: [u8; OPS],
    carriers: u8,
    depth: f32,
}

const fn b(i: usize) -> u8 {
    1 << i
}

// Operators are 0-based here (op 1 = index 0).
const ALGORITHMS: [Algorithm; 8] = [
    Algorithm { name: "stack", mods: [b(1), b(2), b(3), b(4), b(5), 0], carriers: b(0), depth: 1.0 },
    Algorithm { name: "twin", mods: [b(1), b(2), 0, b(4), b(5), 0], carriers: b(0) | b(3), depth: 1.0 },
    Algorithm { name: "triad", mods: [b(1), 0, b(3), 0, b(5), 0], carriers: b(0) | b(2) | b(4), depth: 1.0 },
    Algorithm { name: "ep", mods: [b(1), 0, b(3) | b(4), 0, b(5), 0], carriers: b(0) | b(2), depth: 1.0 },
    Algorithm { name: "bell", mods: [b(1), b(2), 0, b(4) | b(5), 0, 0], carriers: b(0) | b(3), depth: 1.0 },
    Algorithm { name: "organ", mods: [0; OPS], carriers: 0b11_1111, depth: 1.0 },
    Algorithm { name: "pad", mods: [b(3), b(4), b(5), 0, 0, 0], carriers: b(0) | b(1) | b(2), depth: 0.5 },
    Algorithm { name: "brass", mods: [b(2) | b(3), 0, 0, b(4), b(5), 0], carriers: b(0) | b(1), depth: 1.0 },
];

#[derive(Clone)]
struct Params {
    ratio: [f32; OPS],
    level: [f32; OPS],
    attack: [f32; OPS],
    decay: [f32; OPS],
    sustain: [f32; OPS],
    release: f32,
    feedback: f32,
    vel_sens: f32,
    detune: f32,
    gain: f32,
    algo: usize,
}

impl Params {
    fn set_envs(&self, env: &mut [Adsr; OPS], sr: f32) {
        for (i, e) in env.iter_mut().enumerate() {
            e.set(self.attack[i], self.decay[i], self.sustain[i], self.release, sr);
        }
    }
}

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    age: u64,
    freq: f32,
    phase: [f32; OPS],
    env: [Adsr; OPS],
    fb: [f32; 2],
    /// Output multiplier: 1 normally, ramps to 0 while being stolen.
    fade: f32,
    stealing: bool,
    /// Note waiting for the steal fade to finish: (key, velocity, released).
    pending: Option<(u8, f32, bool)>,
}

impl Voice {
    fn start(&mut self, key: u8, velocity: f32, age: u64, p: &Params, sr: f32) {
        self.active = true;
        self.key = key;
        self.velocity = velocity;
        self.age = age;
        self.freq = midi_to_hz(key as f32);
        self.phase = [0.0; OPS];
        self.fb = [0.0; 2];
        self.fade = 1.0;
        self.stealing = false;
        self.pending = None;
        for e in &mut self.env {
            *e = Adsr::default();
        }
        p.set_envs(&mut self.env, sr);
        for e in &mut self.env {
            e.trigger();
        }
    }

    fn release(&mut self) {
        for e in &mut self.env {
            e.release();
        }
    }

    fn is_released(&self) -> bool {
        self.stealing || self.env.iter().all(|e| e.is_released())
    }
}

/// Per-voice values derived from the parameters, refreshed every block.
struct Derived {
    dt: [f32; OPS],
    /// `m[i][j]`: modulation (cycles) that operator j's output adds to operator i.
    m: [[f32; OPS]; OPS],
    fb: f32,
    cl: [f32; OPS],
    cr: [f32; OPS],
}

fn derive(v: &Voice, p: &Params, sr: f32) -> Derived {
    let algo = &ALGORITHMS[p.algo];
    let vel = 1.0 - p.vel_sens + p.vel_sens * v.velocity;
    // Less modulation on high notes (keeps the top end sweet and alias-free).
    let key_scale = (1.0 - (v.key as f32 - 60.0) / 84.0).clamp(0.4, 1.25);
    let n_car = algo.carriers.count_ones().max(1) as f32;
    let amp = p.gain * (0.55 + 0.45 * v.velocity) / n_car;
    let width = (p.detune / 12.0).min(1.0) * 0.45;
    let mut d = Derived { dt: [0.0; OPS], m: [[0.0; OPS]; OPS], fb: 0.0, cl: [0.0; OPS], cr: [0.0; OPS] };
    let mut scale = [0f32; OPS];
    let mut car_ix = 0;
    for i in 0..OPS {
        let f = v.freq * p.ratio[i] * cents(p.detune * DETUNE_SPREAD[i]);
        d.dt[i] = f / sr;
        // Fade out operators approaching Nyquist instead of letting them alias.
        let guard = ((0.47 * sr - f) / (0.07 * sr)).clamp(0.0, 1.0);
        let lvl = p.level[i];
        scale[i] = MOD_DEPTH * lvl * lvl * algo.depth * vel * key_scale * guard;
        if algo.carriers & b(i) != 0 {
            let pos = if n_car > 1.0 { car_ix as f32 / (n_car - 1.0) * 2.0 - 1.0 } else { 0.0 };
            // Alternate sides so neighbouring carriers do not bunch up.
            let pan = if car_ix % 2 == 0 { pos } else { -pos } * width;
            let (l, r) = pan_gains(pan);
            d.cl[i] = lvl * guard * amp * l;
            d.cr[i] = lvl * guard * amp * r;
            car_ix += 1;
        }
    }
    for i in 0..OPS {
        for (j, s) in scale.iter().enumerate().skip(i + 1) {
            if algo.mods[i] & b(j) != 0 {
                d.m[i][j] = *s;
            }
        }
    }
    d.fb = p.feedback * FB_DEPTH * 0.5;
    d
}

#[inline]
fn sine(table: &[f32], phase: f32) -> f32 {
    // Floor without a libm call (the phase may be negative when modulated).
    let x = phase * SINE_LEN as f32;
    let xi = x as i32 - (x < 0.0) as i32;
    let f = x - xi as f32;
    let i = (xi as usize) & (SINE_LEN - 1);
    let a = table[i];
    a + (table[i + 1] - a) * f
}

pub struct Sextant {
    sr: f32,
    p: Params,
    sine: Vec<f32>,
    voices: Vec<Voice>,
    clock: u64,
    buf_l: [f32; CHUNK],
    buf_r: [f32; CHUNK],
    dc_l: DcBlock,
    dc_r: DcBlock,
}

impl Sextant {
    pub fn new(sr: f32) -> Sextant {
        let sine = (0..=SINE_LEN).map(|i| (i as f32 / SINE_LEN as f32 * TAU).sin()).collect();
        Sextant {
            sr,
            p: Params {
                ratio: [1.0, 1.0, 2.0, 1.0, 3.0, 1.0],
                level: [1.0, 0.5, 0.8, 0.4, 0.6, 0.3],
                attack: [0.002; OPS],
                decay: [0.8, 0.8, 0.5, 0.5, 0.5, 0.5],
                sustain: [0.6, 0.6, 0.2, 0.2, 0.2, 0.2],
                release: 0.4,
                feedback: 0.0,
                vel_sens: 0.6,
                detune: 3.0,
                gain: 0.6,
                algo: 3,
            },
            sine,
            voices: vec![Voice::default(); MAX_VOICES],
            clock: 0,
            buf_l: [0.0; CHUNK],
            buf_r: [0.0; CHUNK],
            dc_l: DcBlock::default(),
            dc_r: DcBlock::default(),
        }
    }

    fn render_chunk(&mut self, n: usize) {
        let sr = self.sr;
        let p = &self.p;
        let table = &self.sine[..];
        let carriers = ALGORITHMS[p.algo].carriers;
        let steal_step = 1.0 / (STEAL_TIME * sr);
        let (bl, br) = (&mut self.buf_l[..n], &mut self.buf_r[..n]);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let mut i = 0;
            'voice: while i < n {
                let d = derive(v, p, sr);
                while i < n {
                    let mut raw = [0f32; OPS];
                    // Top operator with feedback.
                    let s = sine(table, v.phase[TOP] + d.fb * (v.fb[0] + v.fb[1]));
                    raw[TOP] = s * v.env[TOP].next();
                    v.fb[1] = v.fb[0];
                    v.fb[0] = raw[TOP];
                    for k in (0..TOP).rev() {
                        let mut pm = 0.0;
                        for (m, x) in d.m[k].iter().zip(&raw).skip(k + 1) {
                            pm += m * x;
                        }
                        raw[k] = sine(table, v.phase[k] + pm) * v.env[k].next();
                    }
                    let mut l = 0.0;
                    let mut r = 0.0;
                    for (k, x) in raw.iter().enumerate() {
                        l += x * d.cl[k];
                        r += x * d.cr[k];
                        let ph = v.phase[k] + d.dt[k];
                        v.phase[k] = if ph >= 1.0 { ph - (ph as i32) as f32 } else { ph };
                    }
                    bl[i] += l * v.fade;
                    br[i] += r * v.fade;
                    i += 1;
                    if v.stealing {
                        v.fade -= steal_step;
                        if v.fade <= 0.0 {
                            match v.pending.take() {
                                Some((key, vel, released)) => {
                                    v.start(key, vel, v.age, p, sr);
                                    if released {
                                        v.release();
                                    }
                                    continue 'voice;
                                }
                                None => {
                                    v.active = false;
                                    break 'voice;
                                }
                            }
                        }
                    }
                }
            }
            if v.active && !v.stealing && (0..OPS).filter(|k| carriers & b(*k) != 0).all(|k| v.env[k].is_idle()) {
                v.active = false;
            }
        }
    }
}

impl Instrument for Sextant {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        let mut p = self.p.clone();
        for (i, keys) in OP_KEYS.iter().enumerate() {
            p.ratio[i] = f(keys[0]);
            p.level[i] = f(keys[1]).clamp(0.0, 1.0);
            p.attack[i] = f(keys[2]);
            p.decay[i] = f(keys[3]);
            p.sustain[i] = f(keys[4]);
        }
        p.release = f("release");
        p.feedback = f("feedback").clamp(0.0, 1.0);
        p.vel_sens = f("velocity").clamp(0.0, 1.0);
        p.detune = f("detune");
        p.gain = f("gain");
        let algo = d.option("algorithm");
        p.algo = ALGORITHMS.iter().position(|a| a.name == algo).unwrap_or(3);
        let sr = self.sr;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            // Keep the current stage; only update the times and levels.
            for (i, e) in v.env.iter_mut().enumerate() {
                e.set(p.attack[i], p.decay[i], p.sustain[i], p.release, sr);
            }
        }
        self.p = p;
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = pick_voice(&self.voices, |v| v.active, |v| v.is_released(), |v| v.age);
                let v = &mut self.voices[i];
                if v.active {
                    // Steal with a short fade; the new note starts when it ends.
                    v.stealing = true;
                    v.age = self.clock;
                    v.pending = Some((key, velocity, false));
                } else {
                    v.start(key, velocity, self.clock, &self.p, self.sr);
                }
            }
            NoteKind::Off { key } => {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    if let Some((k, _, released)) = &mut v.pending {
                        if *k == key {
                            *released = true;
                        }
                    } else if v.key == key {
                        v.release();
                    }
                }
            }
            NoteKind::AllOff => {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    if let Some((_, _, released)) = &mut v.pending {
                        *released = true;
                    } else {
                        v.release();
                    }
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let r = DcBlock::coef(self.sr);
        let mut pos = 0;
        while pos < left.len() {
            let n = (left.len() - pos).min(CHUNK);
            self.buf_l[..n].fill(0.0);
            self.buf_r[..n].fill(0.0);
            self.render_chunk(n);
            for i in 0..n {
                left[pos + i] += self.dc_l.process(self.buf_l[i], r);
                right[pos + i] += self.dc_r.process(self.buf_r[i], r);
            }
            pos += n;
        }
    }
}
