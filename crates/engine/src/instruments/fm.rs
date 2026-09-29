//! "Lumière": two-operator FM synthesizer with a detuned twin carrier.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    age: u64,
    freq: f32,
    c1: f32,
    c2: f32,
    m: f32,
    fb: f32,
    idx_env: f32,
    env: Adsr,
}

pub struct Fm {
    sr: f32,
    ratio: f32,
    index: f32,
    idx_coef: f32,
    feedback: f32,
    vel_sens: f32,
    detune: f32,
    gain: f32,
    env_times: (f32, f32, f32, f32),
    voices: Vec<Voice>,
    clock: u64,
}

impl Fm {
    pub fn new(sr: f32) -> Fm {
        Fm {
            sr,
            ratio: 2.0,
            index: 3.0,
            idx_coef: 0.999,
            feedback: 0.0,
            vel_sens: 0.6,
            detune: 4.0,
            gain: 0.6,
            env_times: (0.005, 0.3, 0.7, 0.25),
            voices: vec![Voice::default(); MAX_VOICES],
            clock: 0,
        }
    }
}

impl Instrument for Fm {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.ratio = d.param("ratio") as f32;
        self.index = d.param("index") as f32;
        self.idx_coef = settle_coef(d.param("indexDecay") as f32, self.sr);
        self.feedback = d.param("feedback") as f32;
        self.vel_sens = d.param("velocity") as f32;
        self.detune = d.param("detune") as f32;
        self.gain = d.param("gain") as f32;
        self.env_times = (d.param("attack") as f32, d.param("decay") as f32, d.param("sustain") as f32, d.param("release") as f32);
        let (a, dd, s, r) = self.env_times;
        for v in &mut self.voices {
            v.env.set(a, dd, s, r, self.sr);
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = pick_voice(&self.voices, |v| v.active, |v| v.env.is_released(), |v| v.age);
                let (a, d, s, r) = self.env_times;
                let v = &mut self.voices[i];
                *v = Voice { active: true, key, velocity, age: self.clock, freq: midi_to_hz(key as f32), idx_env: 1.0, ..Default::default() };
                v.env.set(a, d, s, r, self.sr);
                v.env.trigger();
            }
            NoteKind::Off { key } => {
                for v in self.voices.iter_mut().filter(|v| v.active && v.key == key && !v.env.is_released()) {
                    v.env.release();
                }
            }
            NoteKind::AllOff => {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.env.release();
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sr;
        let det = cents(self.detune);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let vel = 1.0 - self.vel_sens + self.vel_sens * v.velocity;
            // Higher notes get a little less modulation, like a real EP.
            let key_scale = (1.0 - (v.key as f32 - 60.0) / 96.0).clamp(0.4, 1.3);
            let dm = v.freq * self.ratio / sr;
            let dc1 = v.freq / sr;
            let dc2 = v.freq * det / sr;
            let amp = (0.3 + 0.7 * v.velocity) * self.gain * 0.45;
            for i in 0..left.len() {
                let idx = self.index * vel * key_scale * (0.12 + 0.88 * v.idx_env);
                v.idx_env *= self.idx_coef;
                let m = (v.m * TAU + v.fb * self.feedback * 2.0).sin();
                v.fb = m;
                let mod_ = m * idx;
                let s1 = (v.c1 * TAU + mod_).sin();
                let s2 = (v.c2 * TAU + mod_).sin();
                v.m = (v.m + dm).fract();
                v.c1 = (v.c1 + dc1).fract();
                v.c2 = (v.c2 + dc2).fract();
                let e = v.env.next() * amp;
                left[i] += (s1 * 0.7 + s2 * 0.3) * e;
                right[i] += (s1 * 0.3 + s2 * 0.7) * e;
            }
            if v.env.is_idle() {
                v.active = false;
            }
        }
    }
}
