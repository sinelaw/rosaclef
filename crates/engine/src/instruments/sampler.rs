//! "Vault": sample player with pitched, one-shot and loop modes.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::samples::{SampleBank, SampleRef};
use crate::Ctx;
use rosaclef_core::Device;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Pitched,
    OneShot,
    Loop,
}

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    key: u8,
    age: u64,
    pos: f64,
    step: f64,
    gain: f32,
    env: Adsr,
}

pub struct Sampler {
    sr: f32,
    path: String,
    sample: Option<SampleRef>,
    mode: Mode,
    root: f32,
    tune: f32,
    start: f32,
    end: f32,
    attack: f32,
    release: f32,
    gain: f32,
    voices: Vec<Voice>,
    clock: u64,
}

impl Sampler {
    pub fn new(sr: f32) -> Sampler {
        Sampler {
            sr,
            path: String::new(),
            sample: None,
            mode: Mode::Pitched,
            root: 60.0,
            tune: 0.0,
            start: 0.0,
            end: 1.0,
            attack: 0.001,
            release: 0.05,
            gain: 0.8,
            voices: vec![Voice::default(); MAX_VOICES],
            clock: 0,
        }
    }

    fn bounds(&self, len: usize) -> (f64, f64) {
        let a = (self.start.min(self.end) as f64 * len as f64).floor();
        let b = (self.start.max(self.end) as f64 * len as f64)
            .ceil()
            .min(len as f64);
        (a, b.max(a + 1.0))
    }
}

impl Instrument for Sampler {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let path = d.option("sample");
        if path != self.path {
            self.path = path.to_string();
            self.sample = None;
        }
        self.mode = match d.option("mode") {
            "oneshot" => Mode::OneShot,
            "loop" => Mode::Loop,
            _ => Mode::Pitched,
        };
        self.root = d.param("root") as f32;
        self.tune = d.param("tune") as f32;
        self.start = d.param("start") as f32;
        self.end = d.param("end") as f32;
        self.attack = d.param("attack") as f32;
        self.release = d.param("release") as f32;
        self.gain = d.param("gain") as f32;
    }

    fn set_samples(&mut self, bank: &SampleBank) {
        self.sample = bank.get(&self.path);
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                let Some(sample) = &self.sample else { return };
                self.clock += 1;
                let len = sample.len();
                let (a, _) = self.bounds(len);
                let ratio = sample.sample_rate as f64 / self.sr as f64;
                let semis = match self.mode {
                    Mode::OneShot => self.tune,
                    _ => key as f32 - self.root + self.tune,
                };
                let step = ratio * 2f64.powf(semis as f64 / 12.0);
                let i = pick_voice(
                    &self.voices,
                    |v| v.active,
                    |v| v.env.is_released(),
                    |v| v.age,
                );
                let v = &mut self.voices[i];
                v.active = true;
                v.key = key;
                v.age = self.clock;
                v.pos = a;
                v.step = step;
                v.gain = velocity * velocity * self.gain;
                v.env = Adsr::default();
                v.env.set(self.attack, 0.001, 1.0, self.release, self.sr);
                v.env.trigger();
            }
            NoteKind::Off { key } => {
                if self.mode == Mode::OneShot {
                    return;
                }
                for v in self.voices.iter_mut().filter(|v| v.active && v.key == key) {
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
        let Some(sample) = self.sample.clone() else {
            return;
        };
        let (a, b) = self.bounds(sample.len());
        let ch_l = &sample.channels[0];
        let ch_r = sample.channels.get(1).unwrap_or(ch_l);
        let mode = self.mode;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in 0..left.len() {
                if v.pos >= b {
                    if mode == Mode::Loop {
                        v.pos = a + (v.pos - b) % (b - a);
                    } else {
                        v.active = false;
                        break;
                    }
                }
                let e = v.env.next() * v.gain;
                left[i] += hermite(ch_l, v.pos) * e;
                right[i] += hermite(ch_r, v.pos) * e;
                v.pos += v.step;
            }
            if v.env.is_idle() {
                v.active = false;
            }
        }
    }
}
