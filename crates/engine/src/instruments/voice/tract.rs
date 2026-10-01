//! The formant voice's sound: a glottal source (pitch, vibrato, breath)
//! through five formant resonators in cascade, plus band-passed noise for
//! consonants. Targets are approached smoothly, so phonemes blend.

use super::phones::Target;
use crate::dsp::{flush, settle_coef, FilterMode, OnePole, Rng, Svf};

/// Samples between control updates (formant coefficients, pitch).
pub const CONTROL: usize = 32;

/// A Klatt formant resonator: unity gain at DC, a peak at its frequency.
#[derive(Clone, Copy, Default)]
struct Resonator {
    a: f32,
    b: f32,
    c: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn set(&mut self, freq: f32, bw: f32, sr: f32) {
        let t = 1.0 / sr;
        let freq = freq.min(sr * 0.45);
        self.c = -(-2.0 * std::f32::consts::PI * bw * t).exp();
        self.b = 2.0
            * (-std::f32::consts::PI * bw * t).exp()
            * (2.0 * std::f32::consts::PI * freq * t).cos();
        self.a = 1.0 - self.b - self.c;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.a * x + self.b * self.y1 + self.c * self.y2;
        self.y2 = self.y1;
        self.y1 = flush(y);
        y
    }
}

/// How the tract is shaped right now (smoothed towards a [`Target`]).
#[derive(Clone, Copy)]
struct Shape {
    formants: [f32; 3],
    voicing: f32,
    noise_amp: f32,
    noise_freq: f32,
    noise_bw: f32,
    gate: f32,
}

/// Settings that colour the whole voice.
#[derive(Clone, Copy, Debug)]
pub struct Colour {
    /// Formant scale (`gender`): 1 neutral, above for a smaller tract.
    pub scale: f32,
    /// Breath noise mixed into the voice, 0..1.
    pub breath: f32,
    /// Brightness of the glottal source (low-pass cutoff in Hz).
    pub bright: f32,
}

pub struct Tract {
    sr: f32,
    res: [Resonator; 5],
    noise_bp: Svf,
    noise_k: f32,
    tilt: OnePole,
    rng: Rng,
    phase: f32,
    shape: Shape,
    /// Smoothing per control step: formants, levels.
    slow: f32,
    fast: f32,
}

impl Tract {
    pub fn new(sr: f32) -> Tract {
        let per_control = |secs: f32| settle_coef(secs, sr / CONTROL as f32);
        Tract {
            sr,
            res: [Resonator::default(); 5],
            noise_bp: Svf::default(),
            noise_k: 1.0,
            tilt: OnePole::default(),
            rng: Rng::new(0x5eed),
            phase: 0.0,
            shape: Shape {
                formants: [500.0, 1500.0, 2500.0],
                voicing: 0.0,
                noise_amp: 0.0,
                noise_freq: 1000.0,
                noise_bw: 1000.0,
                gate: 0.0,
            },
            slow: per_control(0.04),
            fast: per_control(0.008),
        }
    }

    /// Move one control step towards `target` at level `gate`, and set the
    /// filters for it.
    pub fn steer(&mut self, target: &Target, gate: f32, colour: &Colour) {
        let s = &mut self.shape;
        let (slow, fast) = (self.slow, self.fast);
        let towards = |x: &mut f32, to: f32, k: f32| *x = to + (*x - to) * k;
        for (f, to) in s.formants.iter_mut().zip(target.formants) {
            towards(f, to, slow);
        }
        towards(&mut s.voicing, target.voicing, fast);
        towards(&mut s.noise_amp, target.noise.amp, fast);
        towards(&mut s.noise_freq, target.noise.freq, fast);
        towards(&mut s.noise_bw, target.noise.bw, fast);
        towards(&mut s.gate, gate, fast);
        let bws = [80.0, 100.0, 140.0, 220.0, 260.0];
        let freqs = [s.formants[0], s.formants[1], s.formants[2], 3300.0, 3850.0];
        for ((r, f), bw) in self.res.iter_mut().zip(freqs).zip(bws) {
            r.set(f * colour.scale, bw, self.sr);
        }
        let q = (s.noise_freq / s.noise_bw.max(50.0)).clamp(0.3, 8.0);
        self.noise_k = 1.0 / q;
        self.noise_bp
            .set(s.noise_freq, (2.0 - self.noise_k) / 1.96, self.sr);
        self.tilt.set(colour.bright, self.sr);
    }

    /// Whether the tract has fallen silent.
    pub fn silent(&self) -> bool {
        self.shape.gate < 1e-4
    }

    /// Add `out.len()` samples at pitch `hz` and level `amp`.
    pub fn render(&mut self, hz: f32, amp: f32, colour: &Colour, out: &mut [f32]) {
        let dt = (hz / self.sr).min(0.45);
        let s = self.shape;
        for o in out.iter_mut() {
            let noise = self.rng.bipolar();
            let glottal = self.tilt.process(saw(self.phase, dt));
            self.phase += dt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            // Breath rides on the open phase of each glottal cycle.
            let breath = noise * colour.breath * (1.0 - self.phase) * 0.6;
            let mut v = (glottal + breath) * s.voicing;
            for r in &mut self.res {
                v = r.process(v);
            }
            let fric =
                self.noise_bp.process(noise, FilterMode::Bandpass) * self.noise_k * s.noise_amp;
            *o += (v * 0.35 + fric) * s.gate * amp;
        }
    }
}

/// A band-limited sawtooth (polyBLEP), close to the spectrum of the voice's
/// glottal pulses as they leave the lips.
#[inline]
fn saw(phase: f32, dt: f32) -> f32 {
    let mut v = 2.0 * phase - 1.0;
    if phase < dt {
        let t = phase / dt;
        v -= t + t - t * t - 1.0;
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        v -= t * t + t + t + 1.0;
    }
    -v
}
