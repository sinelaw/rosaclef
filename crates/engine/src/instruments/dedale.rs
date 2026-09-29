//! "Dédale": generative sequencer instrument.
//!
//! Each held key (up to four) runs its own tempo-synced phrase: a euclidean
//! rhythm of `pulses` over `steps`, and a random walk over a scale rooted at
//! the key. The phrase is generated from `seed` and mutates a little every
//! cycle (`variation`). Notes are played by a small internal voice pool with
//! four sounds: pluck, FM bell, bass and metallic percussion.

use super::{Instrument, NoteKind};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

const MAX_STEPS: usize = 32;
const GENERATORS: usize = 4;
const POOL: usize = 16;
const CONTROL: usize = 16;
const OUT_SCALE: f32 = 0.7;

const MINOR: &[i32] = &[0, 2, 3, 5, 7, 8, 10];
const MAJOR: &[i32] = &[0, 2, 4, 5, 7, 9, 11];
const DORIAN: &[i32] = &[0, 2, 3, 5, 7, 9, 10];
const PHRYGIAN: &[i32] = &[0, 1, 3, 5, 7, 8, 10];
const PENTATONIC: &[i32] = &[0, 2, 4, 7, 9];
const HARMONIC: &[i32] = &[0, 2, 3, 5, 7, 8, 11];
const WHOLE: &[i32] = &[0, 2, 4, 6, 8, 10];

fn scale(name: &str) -> &'static [i32] {
    match name {
        "major" => MAJOR,
        "dorian" => DORIAN,
        "phrygian" => PHRYGIAN,
        "pentatonic" => PENTATONIC,
        "harmonic" => HARMONIC,
        "whole" => WHOLE,
        _ => MINOR,
    }
}

/// Semitone offset of a scale degree (degrees may be negative).
#[inline]
fn degree_to_semis(sc: &[i32], deg: i32) -> i32 {
    let len = sc.len() as i32;
    let oct = deg.div_euclid(len);
    let idx = deg.rem_euclid(len) as usize;
    oct * 12 + sc[idx]
}

/// Euclidean (Bjorklund-equivalent) rhythm: is `step` a pulse? Step 0 always is.
#[inline]
fn euclid(step: usize, steps: usize, pulses: usize) -> bool {
    let pulses = pulses.min(steps);
    (step * pulses) % steps < pulses
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

#[inline]
fn saw(ph: f32, dt: f32) -> f32 {
    2.0 * ph - 1.0 - blep(ph, dt)
}

#[inline]
fn wrap(ph: &mut f32, dt: f32) {
    *ph += dt;
    if *ph >= 1.0 {
        *ph -= 1.0;
    }
}

/// Fast sine for a phase in [0, 1) (parabolic approximation with correction).
#[inline]
fn fsin(ph: f32) -> f32 {
    // Wrap to [0, 1) without `floor` (a libm call on baseline x86-64).
    let mut x = ph - (ph as i32) as f32;
    if x < 0.0 {
        x += 1.0;
    }
    let t = 2.0 * x - 1.0; // -1..1, sin(pi*t) = -sin(2*pi*x)
    let y = 4.0 * t * (1.0 - t.abs());
    -(y * (0.775 + 0.225 * y.abs()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sound {
    Pluck,
    Bell,
    Bass,
    Perc,
}

impl Sound {
    fn parse(s: &str) -> Sound {
        match s {
            "bell" => Sound::Bell,
            "bass" => Sound::Bass,
            "perc" => Sound::Perc,
            _ => Sound::Pluck,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Params {
    step_beats: f32,
    steps: usize,
    pulses: usize,
    density: f32,
    range: f32,
    variation: f32,
    gate: f32,
    accent: f32,
    seed: u32,
    tone: f32,
    decay: f32,
    gain: f32,
    scale: &'static [i32],
    sound: Sound,
}

/// One running phrase (a held key).
#[derive(Clone)]
struct Generator {
    active: bool,
    key: u8,
    vel: f32,
    age: u64,
    /// Position inside the current step, in steps (0..1).
    phase: f64,
    step: usize,
    deg: [i32; MAX_STEPS],
    roll: [f32; MAX_STEPS],
    acc: [f32; MAX_STEPS],
    rng: Rng,
    /// (seed, scale length, range) the phrase was generated for.
    made_for: (u32, usize, i32),
}

impl Generator {
    fn new() -> Generator {
        Generator {
            active: false,
            key: 0,
            vel: 0.0,
            age: 0,
            phase: 0.0,
            step: 0,
            deg: [0; MAX_STEPS],
            roll: [0.0; MAX_STEPS],
            acc: [0.0; MAX_STEPS],
            rng: Rng::new(1),
            made_for: (u32::MAX, 0, 0),
        }
    }

    fn max_degree(p: &Params) -> i32 {
        (p.range * p.scale.len() as f32).round() as i32
    }

    /// Build the phrase deterministically from the seed.
    fn generate(&mut self, p: &Params) {
        let mut rng = Rng::new(p.seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15));
        for _ in 0..4 {
            rng.next_u32();
        }
        let len = p.scale.len() as i32;
        let max = Self::max_degree(p);
        let mut d = 0i32;
        for i in 0..MAX_STEPS {
            if i > 0 {
                d = walk(&mut rng, d, len, max);
            }
            self.deg[i] = d;
            self.roll[i] = rng.unit();
            self.acc[i] = rng.unit();
        }
        self.made_for = (p.seed, p.scale.len(), max);
    }

    /// Mutate the phrase at the end of a cycle.
    fn mutate(&mut self, p: &Params) {
        if p.variation <= 0.0 {
            return;
        }
        let len = p.scale.len() as i32;
        let max = Self::max_degree(p);
        let steps = p.steps.min(MAX_STEPS);
        for i in 1..steps {
            // Only sounding steps mutate, so the variation is always audible.
            if !euclid(i, steps, p.pulses) {
                continue;
            }
            if self.rng.unit() < p.variation * 0.5 {
                let r = self.rng.unit();
                let delta = if r < 0.3 {
                    -1
                } else if r < 0.6 {
                    1
                } else if r < 0.75 {
                    -2
                } else if r < 0.9 {
                    2
                } else {
                    // Snap back towards the root.
                    -self.deg[i] / 2
                };
                self.deg[i] = reflect(self.deg[i] + delta, max);
                if i % 4 == 0 && len >= 7 {
                    self.deg[i] = snap_even(self.deg[i], max);
                }
            }
            if self.rng.unit() < p.variation * 0.4 {
                self.roll[i] = self.rng.unit();
            }
            if self.rng.unit() < p.variation * 0.2 {
                self.acc[i] = self.rng.unit();
            }
        }
    }
}

#[inline]
fn reflect(d: i32, max: i32) -> i32 {
    if max <= 0 {
        return 0;
    }
    let mut d = d;
    if d > max {
        d = 2 * max - d;
    }
    if d < -max {
        d = -2 * max - d;
    }
    d.clamp(-max, max)
}

/// Nearest even degree (a triad tone in heptatonic scales).
#[inline]
fn snap_even(d: i32, max: i32) -> i32 {
    if d.rem_euclid(2) == 0 {
        d
    } else if d < max {
        d + 1
    } else {
        d - 1
    }
}

/// One random-walk move.
fn walk(rng: &mut Rng, d: i32, len: i32, max: i32) -> i32 {
    if max <= 0 {
        return 0;
    }
    let r = rng.unit();
    let next = if r < 0.14 {
        d - 2
    } else if r < 0.38 {
        d - 1
    } else if r < 0.46 {
        d
    } else if r < 0.7 {
        d + 1
    } else if r < 0.82 {
        d + 2
    } else if r < 0.9 {
        // A leap of a third to a fifth.
        let leap = 2 + (rng.unit() * 3.0) as i32;
        if rng.unit() < 0.5 {
            d + leap
        } else {
            d - leap
        }
    } else if r < 0.96 {
        // Gravity towards the root.
        d - d.signum() * (d.abs() / 2).max(1)
    } else {
        // Octave jump.
        if d > 0 {
            d - len
        } else {
            d + len
        }
    };
    reflect(next, max)
}

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    sound: Option<Sound>,
    freq: f32,
    amp: f32,
    pan_l: f32,
    pan_r: f32,
    /// Samples until the gate closes.
    gate_left: u32,
    attack: f32,
    env: f32,
    decay_coef: f32,
    release_coef: f32,
    released: bool,
    attacking: bool,
    /// Fast envelope (filter / FM index / noise burst).
    fenv: f32,
    fenv_coef: f32,
    ph: [f32; 3],
    svf: Svf,
    svf2: Svf,
    noise: u32,
    counter: usize,
    age: u64,
}

impl Voice {
    #[inline]
    fn noise(&mut self) -> f32 {
        let mut x = self.noise.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.noise = x;
        (x >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }
}

pub struct Dedale {
    sr: f32,
    bpm: f32,
    p: Params,
    gens: Vec<Generator>,
    voices: Vec<Voice>,
    clock: u64,
    /// Steps per sample.
    step_inc: f64,
}

impl Dedale {
    pub fn new(sr: f32) -> Dedale {
        let mut d = Dedale {
            sr,
            bpm: 120.0,
            p: Params {
                step_beats: 0.25,
                steps: 16,
                pulses: 7,
                density: 0.9,
                range: 1.5,
                variation: 0.3,
                gate: 0.5,
                accent: 0.4,
                seed: 7,
                tone: 0.5,
                decay: 0.35,
                gain: 0.6,
                scale: MINOR,
                sound: Sound::Pluck,
            },
            gens: vec![Generator::new(); GENERATORS],
            voices: vec![Voice::default(); POOL],
            clock: 0,
            step_inc: 0.0,
        };
        d.update_rate();
        d
    }

    fn update_rate(&mut self) {
        let step_seconds = self.p.step_beats as f64 * 60.0 / self.bpm.max(1.0) as f64;
        self.step_inc = 1.0 / (step_seconds * self.sr as f64).max(1.0);
    }

    fn step_samples(&self) -> f32 {
        (1.0 / self.step_inc) as f32
    }

    /// Play the current step of generator `g` (if it is a sounding pulse).
    fn fire_step(&mut self, g: usize) {
        let p = self.p;
        let steps = p.steps.clamp(2, MAX_STEPS);
        let step_samples = self.step_samples();
        let gen = &mut self.gens[g];
        let s = gen.step % steps;
        // The first step of a cycle always plays; other pulses by `density`.
        if !euclid(s, steps, p.pulses) || (s != 0 && gen.roll[s] >= p.density) {
            return;
        }
        let max = Generator::max_degree(&p);
        let deg = gen.deg[s].clamp(-max, max);
        let pitch = (gen.key as i32 + degree_to_semis(p.scale, deg)).clamp(0, 127) as f32;
        // Accents: downbeats and a few random steps are louder.
        let down = s.is_multiple_of(4);
        let rand_acc = gen.acc[s] < 0.25;
        let mut v = 1.0 - 0.55 * p.accent;
        if down {
            v += 0.55 * p.accent;
        } else if rand_acc {
            v += 0.35 * p.accent;
        }
        let vel = (gen.vel * v).clamp(0.05, 1.0);
        let gate_samples = (p.gate * step_samples).max(16.0) as u32;
        // Pan and noise come from the phrase too, so a phrase repeats exactly.
        let pan = (gen.acc[s] * 2.0 - 1.0) * 0.35;
        let noise_seed = (gen.roll[s].to_bits() ^ (s as u32).wrapping_mul(0x9E37_79B9)) | 1;
        self.start_voice(pitch, vel, gate_samples, pan, noise_seed);
    }

    fn start_voice(&mut self, pitch: f32, vel: f32, gate_samples: u32, pan: f32, noise_seed: u32) {
        self.clock += 1;
        let p = self.p;
        let sr = self.sr;
        // A free voice, otherwise the quietest.
        let i = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => {
                let mut best = 0;
                let mut lvl = f32::MAX;
                for (j, v) in self.voices.iter().enumerate() {
                    let l = v.env * v.amp;
                    if l < lvl {
                        lvl = l;
                        best = j;
                    }
                }
                best
            }
        };
        let (pl, pr) = pan_gains(pan);
        let v = &mut self.voices[i];
        v.active = true;
        v.sound = Some(p.sound);
        v.freq = midi_to_hz(pitch);
        v.amp = vel * vel.sqrt();
        v.pan_l = pl;
        v.pan_r = pr;
        v.gate_left = gate_samples;
        v.released = false;
        v.age = self.clock;
        v.counter = 0;
        v.noise = noise_seed;
        v.svf.reset();
        v.svf2.reset();
        v.ph = [0.0, 0.37, 0.0];
        // Attack: short ramp from the current level (smooth when a voice is stolen).
        let attack_time = match p.sound {
            Sound::Bass => 0.004,
            Sound::Bell => 0.0015,
            _ => 0.001,
        };
        v.attack = 1.0 / (attack_time * sr);
        v.attacking = true;
        let (decay, fdecay) = match p.sound {
            Sound::Pluck => (p.decay, 0.04 + 0.25 * p.decay),
            Sound::Bell => (p.decay * 1.6, 0.05 + 0.35 * p.decay),
            Sound::Bass => (p.decay * 1.2, 0.05 + 0.3 * p.decay),
            Sound::Perc => (p.decay * 0.5, 0.012 + 0.05 * p.decay),
        };
        v.decay_coef = settle_coef(decay.max(0.01), sr);
        v.release_coef = settle_coef((0.35 * decay).clamp(0.03, 1.2), sr);
        v.fenv = 1.0;
        v.fenv_coef = settle_coef(fdecay, sr);
    }
}

impl Instrument for Dedale {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        let steps = (f("steps").round() as usize).clamp(2, MAX_STEPS);
        self.p = Params {
            step_beats: f("rate").clamp(0.02, 8.0),
            steps,
            pulses: (f("pulses").round() as usize).clamp(1, MAX_STEPS),
            density: f("density").clamp(0.0, 1.0),
            range: f("range").clamp(0.0, 3.0),
            variation: f("variation").clamp(0.0, 1.0),
            gate: f("gate").clamp(0.02, 1.0),
            accent: f("accent").clamp(0.0, 1.0),
            seed: f("seed").round().max(0.0) as u32,
            tone: f("tone").clamp(0.0, 1.0),
            decay: f("decay").clamp(0.01, 8.0),
            gain: f("gain").max(0.0),
            scale: scale(d.option("scale")),
            sound: Sound::parse(d.option("voice")),
        };
        self.bpm = ctx.bpm;
        self.update_rate();
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                // Same key already running: restart it; else a free or the oldest slot.
                let slot = self
                    .gens
                    .iter()
                    .position(|g| g.active && g.key == key)
                    .or_else(|| self.gens.iter().position(|g| !g.active))
                    .unwrap_or_else(|| {
                        let mut best = 0;
                        for (i, g) in self.gens.iter().enumerate() {
                            if g.age < self.gens[best].age {
                                best = i;
                            }
                        }
                        best
                    });
                let p = self.p;
                let g = &mut self.gens[slot];
                g.active = true;
                g.key = key;
                g.vel = velocity.clamp(0.0, 1.0);
                g.age = self.clock;
                g.phase = 0.0;
                g.step = 0;
                g.generate(&p);
                g.rng = Rng::new((p.seed + 1).wrapping_mul(0x85EB_CA6B) ^ (key as u32).wrapping_mul(0xC2B2_AE35) ^ (slot as u32 + 1));
                self.fire_step(slot);
            }
            NoteKind::Off { key } => {
                for g in self.gens.iter_mut().filter(|g| g.active && g.key == key) {
                    g.active = false;
                }
            }
            NoteKind::AllOff => {
                for g in &mut self.gens {
                    g.active = false;
                }
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.gate_left = 0;
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let sr = self.sr;
        let inv_sr = 1.0 / sr;
        let gain = self.p.gain * OUT_SCALE;
        let tone = self.p.tone;

        // Sample-accurate sequencing interleaved with rendering: render up to
        // the next step boundary of any generator, then fire it.
        let mut pos = 0;
        while pos < n {
            // Frames until the earliest step boundary.
            let mut until = n - pos;
            for g in self.gens.iter().filter(|g| g.active) {
                let frames = ((1.0 - g.phase) / self.step_inc).ceil().max(1.0) as usize;
                until = until.min(frames);
            }
            let end = pos + until;
            // Advance the generators.
            let mut fire = [false; GENERATORS];
            for (gi, g) in self.gens.iter_mut().enumerate() {
                if !g.active {
                    continue;
                }
                g.phase += self.step_inc * until as f64;
                if g.phase >= 1.0 - 1e-9 {
                    g.phase = (g.phase - 1.0).max(0.0);
                    fire[gi] = true;
                }
            }

            // Render the voices for this span.
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let Some(sound) = v.sound else {
                    v.active = false;
                    continue;
                };
                let f = v.freq;
                let dt = (f * inv_sr).min(0.45);
                let g = v.amp * gain;
                for i in pos..end {
                    // Envelope.
                    if v.attacking {
                        v.env += v.attack;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.attacking = false;
                        }
                    } else if v.released {
                        v.env *= v.release_coef;
                    } else {
                        v.env *= v.decay_coef;
                    }
                    if v.gate_left > 0 {
                        v.gate_left -= 1;
                        if v.gate_left == 0 {
                            v.released = true;
                        }
                    } else if !v.released {
                        v.released = true;
                    }
                    // Flushed to zero before it reaches the (slow) subnormal range.
                    v.fenv = if v.fenv > 1e-6 { v.fenv * v.fenv_coef } else { 0.0 };
                    let s = match sound {
                        Sound::Pluck => {
                            if v.counter.is_multiple_of(CONTROL) {
                                let fc = f * (1.2 + tone * 5.0) * (1.0 + (4.0 + 10.0 * tone) * v.fenv) + 80.0;
                                v.svf.set(fc.min(18000.0), 0.15 + 0.2 * tone, sr);
                            }
                            let dt2 = dt * 1.0035;
                            let x = saw(v.ph[0], dt) + 0.6 * saw(v.ph[1], dt2);
                            wrap(&mut v.ph[0], dt);
                            wrap(&mut v.ph[1], dt2);
                            v.svf.process(x, FilterMode::Lowpass) * 0.7
                        }
                        Sound::Bell => {
                            let index = (0.6 + 3.5 * tone) * (0.25 + 0.75 * v.fenv);
                            let m = fsin(v.ph[1]);
                            let c = fsin(v.ph[0] + index * m * 0.159);
                            // A soft upper partial for shimmer.
                            let h = fsin(v.ph[2]) * 0.18 * v.fenv;
                            wrap(&mut v.ph[0], dt);
                            wrap(&mut v.ph[1], (dt * 3.5).min(0.49));
                            wrap(&mut v.ph[2], (dt * 5.4).min(0.49));
                            (c + h) * 0.75
                        }
                        Sound::Bass => {
                            if v.counter.is_multiple_of(CONTROL) {
                                let fc = f * (1.5 + 5.0 * tone) * (1.0 + 3.0 * v.fenv) + 60.0;
                                v.svf.set(fc.min(12000.0), 0.3, sr);
                            }
                            let x = saw(v.ph[0], dt) * 0.7 + fsin(v.ph[1]) * 0.9;
                            wrap(&mut v.ph[0], dt);
                            v.ph[1] = v.ph[0];
                            v.svf.process(x, FilterMode::Lowpass) * 1.35
                        }
                        Sound::Perc => {
                            if v.counter.is_multiple_of(CONTROL) {
                                let fc = (f * 2.0 * (0.6 + 0.8 * tone)).clamp(120.0, 12000.0);
                                v.svf.set(fc, 0.75, sr);
                                v.svf2.set((f * 4.0 + 2000.0 * tone).min(16000.0), 0.3, sr);
                            }
                            let nz = v.noise();
                            let body = v.svf.process(nz, FilterMode::Bandpass) * 1.6;
                            let click = v.svf2.process(nz, FilterMode::Highpass) * v.fenv * (0.3 + 0.5 * tone);
                            // Metallic partials: three inharmonic squares, only in the attack.
                            let m = if v.fenv > 0.0 {
                                let sq = |ph: f32| if ph < 0.5 { 1.0 } else { -1.0 };
                                (sq(v.ph[0]) + sq(v.ph[1]) * 0.8 + sq(v.ph[2]) * 0.6) * 0.06 * v.fenv * (0.4 + tone)
                            } else {
                                0.0
                            };
                            wrap(&mut v.ph[0], (dt * 1.47).min(0.49));
                            wrap(&mut v.ph[1], (dt * 2.13).min(0.49));
                            wrap(&mut v.ph[2], (dt * 3.31).min(0.49));
                            body + click + m
                        }
                    };
                    v.counter += 1;
                    let out = s * v.env * g;
                    left[i] += out * v.pan_l;
                    right[i] += out * v.pan_r;
                }
                if v.released && v.env < 1e-4 {
                    v.active = false;
                    v.env = 0.0;
                }
            }

            // Fire the steps that start at `end`.
            for (gi, &f) in fire.iter().enumerate() {
                if !f {
                    continue;
                }
                let p = self.p;
                let steps = p.steps.clamp(2, MAX_STEPS);
                let g = &mut self.gens[gi];
                g.step += 1;
                if g.step >= steps {
                    g.step = 0;
                    let max = Generator::max_degree(&p);
                    if g.made_for != (p.seed, p.scale.len(), max) {
                        g.generate(&p);
                    } else {
                        g.mutate(&p);
                    }
                }
                self.fire_step(gi);
            }
            pos = end;
        }
    }
}
