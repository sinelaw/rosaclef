//! "Aurum": two-oscillator subtractive synthesizer.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

const MAX_UNISON: usize = 7;
const CONTROL_RATE: usize = 16;

#[derive(Clone)]
struct Params {
    wave1: Wave,
    wave2: Wave,
    mode: FilterMode,
    osc2_ratio: f32,
    osc2_mix: f32,
    sub: f32,
    unison: usize,
    spread: f32,
    cutoff: f32,
    resonance: f32,
    filter_env: f32,
    filter_decay_coef: f32,
    glide: f32,
    gain: f32,
}

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    age: u64,
    freq: f32,
    target_freq: f32,
    phase1: [f32; MAX_UNISON],
    phase2: [f32; MAX_UNISON],
    sub_phase: f32,
    env: Adsr,
    fenv: f32,
    svf_l: Svf,
    svf_r: Svf,
    counter: usize,
}

pub struct Synth {
    sr: f32,
    p: Params,
    voices: Vec<Voice>,
    rng: Rng,
    clock: u64,
    last_freq: f32,
    env_times: (f32, f32, f32, f32),
}

impl Synth {
    pub fn new(sr: f32) -> Synth {
        Synth {
            sr,
            p: Params {
                wave1: Wave::Saw,
                wave2: Wave::Saw,
                mode: FilterMode::Lowpass,
                osc2_ratio: 1.0,
                osc2_mix: 0.5,
                sub: 0.0,
                unison: 1,
                spread: 12.0,
                cutoff: 2400.0,
                resonance: 0.2,
                filter_env: 0.35,
                filter_decay_coef: 0.999,
                glide: 0.0,
                gain: 0.6,
            },
            voices: vec![Voice::default(); MAX_VOICES],
            rng: Rng::new(0x5eed),
            clock: 0,
            last_freq: 0.0,
            env_times: (0.005, 0.3, 0.7, 0.25),
        }
    }
}

impl Instrument for Synth {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let sr = self.sr;
        self.p = Params {
            wave1: Wave::parse(d.option("wave1")),
            wave2: Wave::parse(d.option("wave2")),
            mode: FilterMode::parse(d.option("filter")),
            osc2_ratio: 2f32.powf(d.param("osc2Semi") as f32 / 12.0) * cents(d.param("osc2Detune") as f32),
            osc2_mix: d.param("osc2Mix") as f32,
            sub: d.param("sub") as f32,
            unison: (d.param("unison") as usize).clamp(1, MAX_UNISON),
            spread: d.param("spread") as f32,
            cutoff: d.param("cutoff") as f32,
            resonance: d.param("resonance") as f32,
            filter_env: d.param("filterEnv") as f32,
            filter_decay_coef: settle_coef(d.param("filterDecay") as f32, sr),
            glide: d.param("glide") as f32,
            gain: d.param("gain") as f32,
        };
        self.env_times = (d.param("attack") as f32, d.param("decay") as f32, d.param("sustain") as f32, d.param("release") as f32);
        let (a, dd, s, r) = self.env_times;
        for v in &mut self.voices {
            v.env.set(a, dd, s, r, sr);
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = pick_voice(&self.voices, |v| v.active, |v| v.env.is_released(), |v| v.age);
                let target = midi_to_hz(key as f32);
                let start = if self.p.glide > 0.001 && self.last_freq > 0.0 { self.last_freq } else { target };
                self.last_freq = target;
                let (a, d, s, r) = self.env_times;
                let sr = self.sr;
                let rng = &mut self.rng;
                let v = &mut self.voices[i];
                v.active = true;
                v.key = key;
                v.velocity = velocity;
                v.age = self.clock;
                v.freq = start;
                v.target_freq = target;
                for u in 0..MAX_UNISON {
                    v.phase1[u] = rng.unit();
                    v.phase2[u] = rng.unit();
                }
                v.sub_phase = 0.0;
                v.env = Adsr::default();
                v.env.set(a, d, s, r, sr);
                v.env.trigger();
                v.fenv = 1.0;
                v.svf_l.reset();
                v.svf_r.reset();
                v.counter = 0;
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
        let p = &self.p;
        let sr = self.sr;
        let glide_coef = settle_coef(p.glide, sr);
        let n_uni = p.unison;
        // Unison detune and pan per voice slot.
        let mut detune = [1f32; MAX_UNISON];
        let mut pan_l = [1f32; MAX_UNISON];
        let mut pan_r = [1f32; MAX_UNISON];
        for u in 0..n_uni {
            let pos = if n_uni == 1 { 0.0 } else { u as f32 / (n_uni - 1) as f32 * 2.0 - 1.0 };
            detune[u] = cents(pos * p.spread);
            let (l, r) = pan_gains(pos * 0.8);
            pan_l[u] = l;
            pan_r[u] = r;
        }
        let norm = 1.0 / (n_uni as f32).sqrt();
        let mix1 = 1.0 - p.osc2_mix * 0.5;
        let mix2 = p.osc2_mix;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let vel_gain = 0.25 + 0.75 * v.velocity;
            for i in 0..left.len() {
                if v.counter % CONTROL_RATE == 0 {
                    let oct = p.filter_env * 6.0 * v.fenv * (0.5 + 0.5 * v.velocity);
                    let fc = (p.cutoff * 2f32.powf(oct)).clamp(20.0, 20000.0);
                    v.svf_l.set(fc, p.resonance, sr);
                    v.svf_r.set(fc, p.resonance, sr);
                }
                v.counter += 1;
                v.fenv *= p.filter_decay_coef;
                v.freq = v.target_freq + (v.freq - v.target_freq) * glide_coef;

                let f1 = v.freq;
                let f2 = v.freq * p.osc2_ratio;
                let mut sl = 0.0;
                let mut sr_ = 0.0;
                for u in 0..n_uni {
                    let dt1 = (f1 * detune[u] / sr).min(0.49);
                    let dt2 = (f2 * detune[u] / sr).min(0.49);
                    let s = osc(p.wave1, v.phase1[u], dt1, &mut self.rng) * mix1 + osc(p.wave2, v.phase2[u], dt2, &mut self.rng) * mix2;
                    v.phase1[u] += dt1;
                    if v.phase1[u] >= 1.0 {
                        v.phase1[u] -= 1.0;
                    }
                    v.phase2[u] += dt2;
                    if v.phase2[u] >= 1.0 {
                        v.phase2[u] -= 1.0;
                    }
                    sl += s * pan_l[u];
                    sr_ += s * pan_r[u];
                }
                sl *= norm;
                sr_ *= norm;
                if p.sub > 0.0 {
                    let s = (v.sub_phase * TAU).sin() * p.sub;
                    v.sub_phase += f1 * 0.5 / sr;
                    if v.sub_phase >= 1.0 {
                        v.sub_phase -= 1.0;
                    }
                    sl += s;
                    sr_ += s;
                }
                let env = v.env.next();
                let g = env * vel_gain * p.gain * 0.5;
                left[i] += v.svf_l.process(sl, p.mode) * g;
                right[i] += v.svf_r.process(sr_, p.mode) * g;
            }
            if v.env.is_idle() {
                v.active = false;
            }
        }
    }
}
