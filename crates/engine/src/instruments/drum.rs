//! "Atelier": synthesized drum voices (kick, snare, clap, hats, tom, rim,
//! cowbell, shaker).

use super::{pick_voice, Instrument, NoteKind};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    Kick,
    Snare,
    Clap,
    Hat,
    OpenHat,
    Tom,
    Rim,
    Cowbell,
    Shaker,
}

impl Kind {
    fn parse(s: &str) -> Kind {
        match s {
            "snare" => Kind::Snare,
            "clap" => Kind::Clap,
            "hat" => Kind::Hat,
            "openhat" => Kind::OpenHat,
            "tom" => Kind::Tom,
            "rim" => Kind::Rim,
            "cowbell" => Kind::Cowbell,
            "shaker" => Kind::Shaker,
            _ => Kind::Kick,
        }
    }
}

const HAT_RATIOS: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const MAX_DRUM_VOICES: usize = 12;
/// Onset ramp and end fade of every hit, in seconds.
const ONSET: f32 = 0.0004;
const FADE: f32 = 0.005;

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    age: u64,
    t: f32,
    len: f32,
    pm: f32,
    vel: f32,
    phase: [f32; 6],
    f1: Svf,
    f2: Svf,
    rng: Rng0,
}

#[derive(Clone)]
struct Rng0(Rng);
impl Default for Rng0 {
    fn default() -> Self {
        Rng0(Rng::new(12345))
    }
}

pub struct Drum {
    sr: f32,
    kind: Kind,
    tune: f32,
    decay: f32,
    tone: f32,
    snap: f32,
    drive: f32,
    gain: f32,
    voices: Vec<Voice>,
    clock: u64,
}

impl Drum {
    pub fn new(sr: f32) -> Drum {
        Drum {
            sr,
            kind: Kind::Kick,
            tune: 0.0,
            decay: 1.0,
            tone: 0.5,
            snap: 0.5,
            drive: 0.0,
            gain: 0.8,
            voices: vec![Voice::default(); MAX_DRUM_VOICES],
            clock: 0,
        }
    }

    fn voice_len(&self) -> f32 {
        let d = self.decay;
        match self.kind {
            Kind::Kick => 0.75 * d + 0.05,
            Kind::Snare => 0.9 * d,
            Kind::Clap => 1.0 * d + 0.05,
            Kind::Hat => 0.3 * d,
            Kind::OpenHat => 2.2 * d,
            Kind::Tom => 1.8 * d,
            Kind::Rim => 0.15 * d,
            Kind::Cowbell => 1.8 * d,
            Kind::Shaker => 0.5 * d,
        }
    }
}

impl Instrument for Drum {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.kind = Kind::parse(d.option("kind"));
        self.tune = d.param("tune") as f32;
        self.decay = d.param("decay") as f32;
        self.tone = d.param("tone") as f32;
        self.snap = d.param("snap") as f32;
        self.drive = d.param("drive") as f32;
        self.gain = d.param("gain") as f32;
    }

    fn handle(&mut self, ev: NoteKind) {
        if let NoteKind::On { key, velocity } = ev {
            self.clock += 1;
            let len = self.voice_len();
            let sr = self.sr;
            let pm = 2f32.powf((self.tune + key as f32 - 60.0) / 12.0);
            let tone = self.tone;
            let kind = self.kind;
            let i = pick_voice(&self.voices, |v| v.active, |_| true, |v| v.age);
            let v = &mut self.voices[i];
            v.active = true;
            v.age = self.clock;
            v.t = 0.0;
            v.len = len;
            v.pm = pm;
            v.vel = velocity;
            v.phase = [0.0; 6];
            v.rng = Rng0(Rng::new(0x9e37_79b9 ^ self.clock as u32));
            v.f1.reset();
            v.f2.reset();
            match kind {
                Kind::Kick => v.f1.set(2500.0, 0.1, sr),
                Kind::Snare => {
                    v.f1.set(1400.0 + tone * 4000.0, 0.15, sr);
                    v.f2.set(9000.0, 0.0, sr);
                }
                Kind::Clap => v.f1.set((900.0 + tone * 1400.0) * pm.sqrt(), 0.45, sr),
                Kind::Hat | Kind::OpenHat => {
                    v.f1.set((7500.0 + tone * 4500.0) * pm.sqrt(), 0.35, sr);
                    v.f2.set(6500.0, 0.0, sr);
                }
                Kind::Tom => v.f1.set(3000.0, 0.0, sr),
                Kind::Rim => v.f1.set(2200.0 * pm, 0.6, sr),
                Kind::Cowbell => v.f1.set(1200.0 + tone * 1600.0, 0.4, sr),
                Kind::Shaker => v.f1.set(5500.0 + tone * 5000.0, 0.1, sr),
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sr;
        let dt = 1.0 / sr;
        let (kind, decay, tone, snap, drive) = (self.kind, self.decay, self.tone, self.snap, self.drive);
        let drive_gain = 1.0 + drive * 8.0;
        let drive_norm = 1.0 / (1.0 + drive * 2.0);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let amp = (0.2 + 0.8 * v.vel) * self.gain;
            for i in 0..left.len() {
                let t = v.t;
                let noise = v.rng.0.bipolar();
                let s = match kind {
                    Kind::Kick => {
                        let f_end = 48.0 * v.pm;
                        let f_start = f_end * (3.0 + snap * 6.0);
                        let tp = 0.022 + (1.0 - snap) * 0.03;
                        let f = f_end + (f_start - f_end) * (-t / tp).exp();
                        v.phase[0] = (v.phase[0] + f * dt).fract();
                        let body = (v.phase[0] * TAU).sin();
                        let shaped = (body * (1.0 + tone * 2.5)).tanh() / (1.0 + tone * 2.5).tanh();
                        // Punchy body with a short sub tail.
                        let env = (0.6 * (-t / (0.07 * decay)).exp() + 0.4 * (-t / (0.16 * decay)).exp()) * (t / 0.0015).min(1.0);
                        let click = v.f1.process(noise, FilterMode::Highpass) * (-t / 0.004).exp() * snap * 0.6;
                        shaped * env + click
                    }
                    Kind::Snare => {
                        let f = 185.0 * v.pm * (1.0 + 0.6 * (-t / 0.012).exp());
                        v.phase[0] = (v.phase[0] + f * dt).fract();
                        v.phase[1] = (v.phase[1] + f * 1.78 * dt).fract();
                        let body = ((v.phase[0] * TAU).sin() + 0.5 * (v.phase[1] * TAU).sin()) * (-t / (0.08 * decay)).exp();
                        // One band-limited noise source feeds both the sustained hiss and the
                        // stick crack (raw white noise here sounded fizzy and harsh).
                        let band = v.f2.process(v.f1.process(noise, FilterMode::Highpass), FilterMode::Lowpass);
                        let hiss = band * (-t / (0.17 * decay)).exp();
                        let crack = band * (-t / 0.003).exp() * snap;
                        body * (0.9 - tone * 0.4) + hiss * (0.55 + tone * 0.6) + crack * 0.6
                    }
                    Kind::Clap => {
                        let mut env = 0.0;
                        for k in 0..3 {
                            let start = k as f32 * (0.009 + 0.004 * (1.0 - snap));
                            if t >= start {
                                // Each burst re-attacks over 0.3 ms rather than in one sample.
                                let u = t - start;
                                env += (u * (1.0 / 0.0003)).min(1.0) * (-u / 0.0045).exp();
                            }
                        }
                        let tail_start = 0.028;
                        if t >= tail_start {
                            env += 0.75 * (-(t - tail_start) / (0.16 * decay)).exp();
                        }
                        v.f1.process(noise, FilterMode::Bandpass) * env * 2.2
                    }
                    Kind::Hat | Kind::OpenHat => {
                        let mut m = 0.0;
                        for k in 0..6 {
                            v.phase[k] = (v.phase[k] + HAT_RATIOS[k] * v.pm * 1.4 * dt).fract();
                            m += if v.phase[k] < 0.5 { 1.0 } else { -1.0 };
                        }
                        let src = m * 0.16 * (1.0 - tone * 0.4) + noise * (0.25 + tone * 0.35);
                        let bp = v.f1.process(src, FilterMode::Bandpass);
                        let hp = v.f2.process(bp, FilterMode::Highpass);
                        let tau = if kind == Kind::Hat { 0.045 } else { 0.38 } * decay;
                        hp * (-t / tau).exp() * (1.4 + snap * 0.6)
                    }
                    Kind::Tom => {
                        let f = 105.0 * v.pm * (1.0 + 0.55 * (-t / 0.06).exp());
                        v.phase[0] = (v.phase[0] + f * dt).fract();
                        let body = (v.phase[0] * TAU).sin() * (-t / (0.33 * decay)).exp();
                        let stick = v.f1.process(noise, FilterMode::Lowpass) * (-t / 0.006).exp() * snap * 0.5;
                        (body * (1.0 + tone)).tanh() + stick
                    }
                    Kind::Rim => {
                        v.phase[0] = (v.phase[0] + 1720.0 * v.pm * dt).fract();
                        v.phase[1] = (v.phase[1] + 520.0 * v.pm * dt).fract();
                        let tri = 1.0 - 4.0 * (v.phase[0] - 0.5).abs();
                        let body = (tri * 0.7 + (v.phase[1] * TAU).sin() * 0.5) * (-t / (0.022 * decay)).exp();
                        body + v.f1.process(noise, FilterMode::Bandpass) * (-t / 0.01).exp() * (0.3 + tone * 0.5)
                    }
                    Kind::Cowbell => {
                        v.phase[0] = (v.phase[0] + 540.0 * v.pm * dt).fract();
                        v.phase[1] = (v.phase[1] + 800.0 * v.pm * dt).fract();
                        let sq = (if v.phase[0] < 0.5 { 1.0 } else { -1.0 }) + (if v.phase[1] < 0.5 { 1.0 } else { -1.0 });
                        let env = 0.65 * (-t / 0.018).exp() + 0.35 * (-t / (0.3 * decay)).exp();
                        v.f1.process(sq * 0.5, FilterMode::Bandpass) * env * 1.6
                    }
                    Kind::Shaker => {
                        let env = (t / 0.012).min(1.0) * (-t / (0.08 * decay)).exp();
                        v.f1.process(noise, FilterMode::Highpass) * env * 0.9
                    }
                };
                // A 0.4 ms onset and a 5 ms fade before the voice stops: no step at
                // either end (the first sample used to jump straight to full level).
                let edge = (t * (1.0 / ONSET)).min(1.0) * ((v.len - t) * (1.0 / FADE)).clamp(0.0, 1.0);
                let s = s * edge;
                let out = if drive > 0.0 { (s * drive_gain).tanh() * drive_norm } else { s } * amp;
                left[i] += out;
                right[i] += out;
                v.t += dt;
            }
            if v.t >= v.len {
                v.active = false;
            }
        }
    }
}
