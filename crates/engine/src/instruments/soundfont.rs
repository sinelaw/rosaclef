//! Grand Orchestra (`soundfont`): plays a SoundFont preset (by default the built-in General
//! MIDI soundfont).
//!
//! The preset arrives decoded ([`crate::soundfont::LoadedPreset`], through
//! the engine's sample bank); until it does, notes are silent. Each note
//! starts one voice per matching region (two for stereo samples). A voice
//! follows the SoundFont 2 synthesis model:
//!
//! - pitch: key distance from the root key × scale tuning, coarse/fine tune
//!   and sample correction, modulated by the modulation envelope and both
//!   LFOs; samples are read with 4-point Hermite interpolation, looping as
//!   the sample modes say;
//! - a 2-pole low-pass with resonance (cutoff modulated by the modulation
//!   envelope, the modulation LFO and velocity);
//! - the volume envelope (delay, attack, hold, decay, sustain, release; the
//!   attack is linear in amplitude, the rest linear in decibels), initial
//!   attenuation (scaled by 0.4, as FluidSynth and the soundfonts made for it
//!   expect) and velocity (the SoundFont default concave curve: -12 dB at
//!   half velocity);
//! - pan, and exclusive classes (an open hi-hat stops when the closed one
//!   plays).
//!
//! Envelopes, LFOs and the filter are updated every [`CHUNK`] frames; the
//! amplitude is ramped across each chunk.

use super::{pick_voice, Instrument, NoteKind};
use crate::samples::{PresetKey, SampleBank};
use crate::soundfont::{gen, FontSample, LoadedPreset, Region};
use crate::Ctx;
use rosaclef_core::Device;
use std::f32::consts::PI;
use std::sync::Arc;

const MAX: usize = 64;
const CHUNK: usize = 32;
/// Output level of a full-scale voice.
const LEVEL: f32 = 0.5;

fn timecents(tc: i32) -> f32 {
    2f32.powf(tc.clamp(-12000, 8000) as f32 / 1200.0)
}
fn abs_hz(cents: f32) -> f32 {
    8.176 * 2f32.powf(cents / 1200.0)
}
fn cb_to_amp(cb: f32) -> f32 {
    10f32.powf(-cb / 200.0)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Done,
}

/// A SoundFont envelope. `value` is 0..1: the modulation envelope uses it
/// as is; the volume envelope maps it to 0..96 dB of attenuation (except
/// during the attack, where it is the amplitude).
#[derive(Clone, Copy, Debug)]
struct Env {
    stage: Stage,
    value: f32,
    /// Seconds spent in the current stage.
    t: f32,
    delay: f32,
    attack: f32,
    hold: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Env {
    fn new(delay: f32, attack: f32, hold: f32, decay: f32, sustain: f32, release: f32) -> Env {
        Env {
            stage: Stage::Delay,
            value: 0.0,
            t: 0.0,
            delay,
            attack,
            hold,
            decay,
            sustain: sustain.clamp(0.0, 1.0),
            release,
        }
    }

    fn advance(&mut self, dt: f32) {
        let mut dt = dt;
        while dt > 0.0 {
            match self.stage {
                Stage::Delay | Stage::Hold => {
                    let len = if self.stage == Stage::Delay {
                        self.delay
                    } else {
                        self.hold
                    };
                    let left = len - self.t;
                    if left > dt {
                        self.t += dt;
                        return;
                    }
                    dt -= left.max(0.0);
                    self.t = 0.0;
                    self.stage = if self.stage == Stage::Delay {
                        Stage::Attack
                    } else {
                        Stage::Decay
                    };
                }
                Stage::Attack => {
                    self.value += dt / self.attack.max(1e-4);
                    if self.value < 1.0 {
                        return;
                    }
                    dt = (self.value - 1.0) * self.attack;
                    self.value = 1.0;
                    self.stage = Stage::Hold;
                    self.t = 0.0;
                }
                Stage::Decay => {
                    self.value -= dt / self.decay.max(1e-4);
                    if self.value <= self.sustain {
                        self.value = self.sustain;
                        self.stage = Stage::Sustain;
                    }
                    return;
                }
                Stage::Sustain | Stage::Done => return,
                Stage::Release => {
                    self.value -= dt / self.release.max(1e-4);
                    if self.value <= 0.0 {
                        self.value = 0.0;
                        self.stage = Stage::Done;
                    }
                    return;
                }
            }
        }
    }

    fn release(&mut self, volume: bool) {
        if matches!(self.stage, Stage::Release | Stage::Done) {
            return;
        }
        if volume && matches!(self.stage, Stage::Delay | Stage::Attack) {
            // Keep the amplitude: express the attack's linear level in dB.
            let amp = if self.stage == Stage::Delay {
                0.0
            } else {
                self.value
            };
            let cb = -200.0 * amp.max(1e-5).log10();
            self.value = (1.0 - cb / 960.0).max(0.0);
        }
        self.stage = Stage::Release;
    }

    /// Amplitude of the volume envelope.
    fn amplitude(&self) -> f32 {
        match self.stage {
            Stage::Delay | Stage::Done => 0.0,
            Stage::Attack => self.value,
            _ => cb_to_amp(960.0 * (1.0 - self.value)),
        }
    }
}

/// Triangle LFO, -1..1, starting upwards after its delay.
#[derive(Clone, Copy, Debug)]
struct Lfo {
    delay: f32,
    freq: f32,
    phase: f32,
}

impl Lfo {
    fn advance(&mut self, dt: f32) -> f32 {
        if self.delay > 0.0 {
            self.delay -= dt;
            return 0.0;
        }
        self.phase = (self.phase + dt * self.freq).fract();
        let p = self.phase;
        if p < 0.25 {
            p * 4.0
        } else if p < 0.75 {
            2.0 - p * 4.0
        } else {
            p * 4.0 - 4.0
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn lowpass(&mut self, sr: f32, fc: f32, q: f32) {
        let w = 2.0 * PI * (fc / sr).clamp(1e-4, 0.49);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.3));
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 - c) / 2.0 / a0;
        self.b1 = (1.0 - c) / a0;
        self.b2 = self.b0;
        self.a1 = -2.0 * c / a0;
        self.a2 = (1.0 - alpha) / a0;
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

struct Voice {
    active: bool,
    released: bool,
    note: u8,
    class: i32,
    age: u64,
    sample: Arc<FontSample>,
    pos: f64,
    /// Sample frames per output frame, before modulation.
    step: f64,
    end: f64,
    loop_start: f64,
    loop_end: f64,
    /// 0 none, 1 continuous, 3 until release.
    mode: i32,
    vol: Env,
    modenv: Env,
    mod_lfo: Lfo,
    vib_lfo: Lfo,
    mod_env_to_pitch: f32,
    mod_lfo_to_pitch: f32,
    vib_lfo_to_pitch: f32,
    mod_env_to_filter: f32,
    mod_lfo_to_filter: f32,
    mod_lfo_to_volume: f32,
    fc: f32,
    q: f32,
    filter_gain: f32,
    filtered: bool,
    filter: Biquad,
    atten: f32,
    gain_l: f32,
    gain_r: f32,
    amp: f32,
}

impl Voice {
    fn looping(&self) -> bool {
        (self.mode == 1 || (self.mode == 3 && !self.released))
            && self.loop_end > self.loop_start + 1.0
    }

    fn frame(&self, i: i64) -> f32 {
        let d = &self.sample.data;
        let mut i = i;
        if self.looping() && i >= self.loop_end as i64 {
            let len = (self.loop_end - self.loop_start) as i64;
            i = self.loop_start as i64 + (i - self.loop_start as i64) % len.max(1);
        }
        if i < 0 || i as usize >= d.len() {
            0.0
        } else {
            d[i as usize] as f32 * (1.0 / 32768.0)
        }
    }

    fn release(&mut self, scale: f32) {
        if self.released {
            return;
        }
        self.released = true;
        self.vol.release *= scale;
        self.modenv.release *= scale;
        self.vol.release(true);
        self.modenv.release(false);
    }

    /// Render up to one chunk; returns false once the voice has ended.
    fn render(&mut self, sr: f32, left: &mut [f32], right: &mut [f32]) -> bool {
        let n = left.len();
        let dt = n as f32 / sr;
        self.vol.advance(dt);
        self.modenv.advance(dt);
        let ml = self.mod_lfo.advance(dt);
        let vl = self.vib_lfo.advance(dt);
        let m = self.modenv.value;
        let cents =
            m * self.mod_env_to_pitch + ml * self.mod_lfo_to_pitch + vl * self.vib_lfo_to_pitch;
        let step = self.step * 2f64.powf(cents as f64 / 1200.0);
        if self.filtered {
            let fc = abs_hz(self.fc + m * self.mod_env_to_filter + ml * self.mod_lfo_to_filter)
                .clamp(20.0, sr * 0.45);
            self.filter.lowpass(sr, fc, self.q);
        }
        let target = self.vol.amplitude() * cb_to_amp(self.atten + ml * self.mod_lfo_to_volume);
        let from = self.amp;
        let inc = (target - from) / n as f32;
        for k in 0..n {
            if !self.looping() && self.pos >= self.end {
                self.active = false;
                break;
            }
            let i = self.pos.floor();
            let f = (self.pos - i) as f32;
            let i = i as i64;
            let (xm, x0, x1, x2) = (
                self.frame(i - 1),
                self.frame(i),
                self.frame(i + 1),
                self.frame(i + 2),
            );
            let c1 = 0.5 * (x1 - xm);
            let c2 = xm - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
            let c3 = 0.5 * (x2 - xm) + 1.5 * (x0 - x1);
            let mut y = ((c3 * f + c2) * f + c1) * f + x0;
            if self.filtered {
                y = self.filter.run(y) * self.filter_gain;
            }
            let a = from + inc * k as f32;
            left[k] += y * a * self.gain_l;
            right[k] += y * a * self.gain_r;
            self.pos += step;
            if self.looping() && self.pos >= self.loop_end {
                self.pos -= self.loop_end - self.loop_start;
            }
        }
        self.amp = target;
        if self.vol.stage == Stage::Done
            || (self.released && self.vol.stage == Stage::Release && target < 1e-5)
            || (self.vol.stage == Stage::Sustain && self.vol.value <= 0.0)
        {
            self.active = false;
        }
        self.active
    }
}

pub struct SoundFontInst {
    sr: f32,
    key: PresetKey,
    preset: Option<Arc<LoadedPreset>>,
    gain: f32,
    transpose: i32,
    release: f32,
    voices: Vec<Voice>,
    age: u64,
}

impl SoundFontInst {
    pub fn new(sr: f32) -> SoundFontInst {
        SoundFontInst {
            sr,
            key: PresetKey::gm(0, 0),
            preset: None,
            gain: 1.0,
            transpose: 0,
            release: 1.0,
            voices: Vec::with_capacity(MAX),
            age: 0,
        }
    }

    /// The preset this device plays.
    pub fn preset_key(dev: &Device) -> PresetKey {
        let (bank, program) = rosaclef_core::gm::lookup(dev.option("program")).unwrap_or((0, 0));
        PresetKey::gm(bank, program)
    }

    fn start(&mut self, r: &Region, sample: &Arc<FontSample>, key: u8, vel: u8) {
        let key_eff = if r.get(gen::KEYNUM) >= 0 {
            r.get(gen::KEYNUM).min(127) as u8
        } else {
            key
        };
        let vel = if r.get(gen::VELOCITY) > 0 {
            r.get(gen::VELOCITY).min(127) as u8
        } else {
            vel
        };
        // Generators plus modulators, as f32.
        let mut mods = [0f32; crate::soundfont::GENS];
        for m in &r.mods {
            mods[m.dest as usize] += m.value(key_eff, vel);
        }
        let gf = |i: usize| r.get(i) as f32 + mods[i];
        let g = |i: usize| gf(i).round() as i32;
        let class = g(gen::EXCLUSIVE_CLASS);
        if class != 0 {
            for v in self
                .voices
                .iter_mut()
                .filter(|v| v.active && v.class == class)
            {
                if !v.released {
                    v.vol.release = 0.005;
                    v.released = true;
                    v.vol.release(true);
                }
            }
        }

        let len = sample.data.len() as f64;
        let root = if g(gen::ROOT_KEY) >= 0 {
            g(gen::ROOT_KEY) as f32
        } else if sample.root <= 127 {
            sample.root as f32
        } else {
            60.0
        };
        let cents = (key_eff as f32 + self.transpose as f32 - root) * gf(gen::SCALE_TUNING)
            + gf(gen::COARSE_TUNE) * 100.0
            + gf(gen::FINE_TUNE)
            + sample.correction as f32;
        let step = 2f64.powf(cents as f64 / 1200.0) * sample.rate as f64 / self.sr as f64;
        let start = (g(gen::START_OFFSET) + 32768 * g(gen::START_COARSE)) as f64;
        let end = len + (g(gen::END_OFFSET) + 32768 * g(gen::END_COARSE)) as f64;
        let loop_start = sample.loop_start as f64
            + (g(gen::LOOP_START_OFFSET) + 32768 * g(gen::LOOP_START_COARSE)) as f64;
        let loop_end = sample.loop_end as f64
            + (g(gen::LOOP_END_OFFSET) + 32768 * g(gen::LOOP_END_COARSE)) as f64;

        let keyf = key_eff as i32;
        let vol = Env::new(
            timecents(g(gen::VOL_ENV_DELAY)),
            timecents(g(gen::VOL_ENV_ATTACK)),
            timecents(g(gen::VOL_ENV_HOLD) + (60 - keyf) * g(gen::KEY_TO_VOL_ENV_HOLD)),
            timecents(g(gen::VOL_ENV_DECAY) + (60 - keyf) * g(gen::KEY_TO_VOL_ENV_DECAY)),
            1.0 - gf(gen::VOL_ENV_SUSTAIN).clamp(0.0, 1440.0) / 960.0,
            timecents(g(gen::VOL_ENV_RELEASE)),
        );
        let modenv = Env::new(
            timecents(g(gen::MOD_ENV_DELAY)),
            timecents(g(gen::MOD_ENV_ATTACK)),
            timecents(g(gen::MOD_ENV_HOLD) + (60 - keyf) * g(gen::KEY_TO_MOD_ENV_HOLD)),
            timecents(g(gen::MOD_ENV_DECAY) + (60 - keyf) * g(gen::KEY_TO_MOD_ENV_DECAY)),
            1.0 - gf(gen::MOD_ENV_SUSTAIN).clamp(0.0, 1000.0) / 1000.0,
            timecents(g(gen::MOD_ENV_RELEASE)),
        );

        let fc = gf(gen::FILTER_FC);
        let q_db = gf(gen::FILTER_Q).clamp(0.0, 960.0) / 10.0;
        let mod_env_to_filter = gf(gen::MOD_ENV_TO_FILTER);
        let mod_lfo_to_filter = gf(gen::MOD_LFO_TO_FILTER);
        let filtered =
            fc < 13500.0 || q_db > 0.0 || mod_env_to_filter != 0.0 || mod_lfo_to_filter != 0.0;
        let q = 10f32.powf(q_db / 20.0);

        let pan = (gf(gen::PAN).clamp(-500.0, 500.0) / 1000.0 + 0.5) * PI / 2.0;
        let (sin, cos) = pan.sin_cos();
        let scale = std::f32::consts::SQRT_2 * LEVEL * self.gain;

        self.age += 1;
        let voice = Voice {
            active: true,
            released: false,
            note: key,
            class,
            age: self.age,
            sample: sample.clone(),
            pos: start.clamp(0.0, len),
            step,
            end: end.clamp(0.0, len),
            loop_start: loop_start.clamp(0.0, len),
            loop_end: loop_end.clamp(0.0, len),
            mode: g(gen::SAMPLE_MODES) & 3,
            vol,
            modenv,
            mod_lfo: Lfo {
                delay: timecents(g(gen::MOD_LFO_DELAY)),
                freq: abs_hz(gf(gen::MOD_LFO_FREQ)),
                phase: 0.0,
            },
            vib_lfo: Lfo {
                delay: timecents(g(gen::VIB_LFO_DELAY)),
                freq: abs_hz(gf(gen::VIB_LFO_FREQ)),
                phase: 0.0,
            },
            mod_env_to_pitch: gf(gen::MOD_ENV_TO_PITCH),
            mod_lfo_to_pitch: gf(gen::MOD_LFO_TO_PITCH),
            vib_lfo_to_pitch: gf(gen::VIB_LFO_TO_PITCH),
            mod_env_to_filter,
            mod_lfo_to_filter,
            mod_lfo_to_volume: gf(gen::MOD_LFO_TO_VOLUME),
            fc,
            q,
            filter_gain: 1.0 / q.sqrt(),
            filtered,
            filter: Biquad::default(),
            atten: (r.get(gen::ATTENUATION).max(0) as f32 * 0.4 + mods[gen::ATTENUATION]).max(0.0),
            gain_l: cos * scale,
            gain_r: sin * scale,
            amp: 0.0,
        };
        let slot = if self.voices.len() < MAX {
            self.voices.push(voice);
            return;
        } else {
            pick_voice(&self.voices, |v| v.active, |v| v.released, |v| v.age)
        };
        self.voices[slot] = voice;
    }
}

impl Instrument for SoundFontInst {
    fn set_device(&mut self, dev: &Device, _ctx: &Ctx) {
        let key = Self::preset_key(dev);
        if key != self.key {
            self.key = key;
            self.preset = None;
            self.voices.clear();
        }
        self.gain = dev.param("gain") as f32;
        self.transpose = dev.param("transpose").round() as i32;
        self.release = dev.param("release") as f32;
    }

    fn set_samples(&mut self, bank: &SampleBank) {
        let p = bank.preset(&self.key);
        let same = match (&p, &self.preset) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.voices.clear();
            self.preset = p;
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                let Some(p) = self.preset.clone() else {
                    return;
                };
                let vel = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
                let k = (key as i32 + self.transpose).clamp(0, 127) as u8;
                for r in &p.regions {
                    if (r.keys.0..=r.keys.1).contains(&k) && (r.vels.0..=r.vels.1).contains(&vel) {
                        if let Some(s) = p.samples.get(r.sample as usize) {
                            self.start(r, s, key, vel);
                        }
                    }
                }
            }
            NoteKind::Off { key } => {
                let scale = self.release;
                for v in self.voices.iter_mut().filter(|v| v.active && v.note == key) {
                    v.release(scale);
                }
            }
            NoteKind::AllOff => {
                let scale = self.release;
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.release(scale);
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sr;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let mut pos = 0;
            while pos < left.len() && v.active {
                let end = (pos + CHUNK).min(left.len());
                v.render(sr, &mut left[pos..end], &mut right[pos..end]);
                pos = end;
            }
        }
        self.voices.retain(|v| v.active);
    }
}
