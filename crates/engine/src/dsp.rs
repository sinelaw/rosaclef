//! Small DSP building blocks shared by instruments and effects.

use std::f32::consts::PI;

pub const TAU: f32 = 2.0 * PI;

/// Zero for magnitudes far below audibility. WebAssembly has no
/// flush-to-zero mode, so a feedback path (filter state, a reverb tail)
/// decaying into the subnormal range would run many times slower.
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 {
        0.0
    } else {
        x
    }
}

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    20.0 * g.max(1e-9).log10()
}

#[inline]
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

#[inline]
pub fn cents(c: f32) -> f32 {
    2f32.powf(c / 1200.0)
}

/// Equal-power pan law: returns (left, right) gains for pan in -1..1.
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (
        a.cos() * std::f32::consts::SQRT_2,
        a.sin() * std::f32::consts::SQRT_2,
    )
}

/// Coefficient of a one-pole smoother that settles in roughly `time` seconds.
#[inline]
pub fn settle_coef(time: f32, sr: f32) -> f32 {
    if time <= 0.0 {
        0.0
    } else {
        (-4.6 / (time * sr)).exp()
    }
}

/// Fast deterministic noise.
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed.max(1))
    }
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Uniform in [-1, 1).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
    }
    /// Uniform in [0, 1).
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

/// PolyBLEP residual that band-limits a unit step at phase 0 (`dt` is the
/// phase increment per sample).
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
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

/// Band-limited (PolyBLEP) saw for a phase in [0, 1).
#[inline]
pub fn blep_saw(ph: f32, dt: f32) -> f32 {
    2.0 * ph - 1.0 - poly_blep(ph, dt)
}

/// Advance a phase in cycles, wrapping into [0, 1).
#[inline]
pub fn advance(ph: &mut f32, dt: f32) {
    *ph += dt;
    if *ph >= 1.0 {
        *ph -= 1.0;
    }
}

/// Fast sine for a phase in cycles (parabolic approximation with correction).
#[inline]
pub fn fsin(ph: f32) -> f32 {
    // Wrap to [0, 1) without `floor` (a libm call on baseline x86-64).
    let mut x = ph - (ph as i32) as f32;
    if x < 0.0 {
        x += 1.0;
    }
    let t = 2.0 * x - 1.0; // -1..1, sin(pi*t) = -sin(2*pi*x)
    let y = 4.0 * t * (1.0 - t.abs());
    -(y * (0.775 + 0.225 * y.abs()))
}

/// Cheap rational tanh approximation (Padé): exact at 0, ±1 beyond ±3, smooth.
#[inline]
pub fn ftanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Lowpass,
    Highpass,
    Bandpass,
}

impl FilterMode {
    pub fn parse(s: &str) -> FilterMode {
        match s {
            "highpass" => FilterMode::Highpass,
            "bandpass" => FilterMode::Bandpass,
            _ => FilterMode::Lowpass,
        }
    }
}

/// Topology-preserving-transform state variable filter (Zavalishin / Simper).
#[derive(Clone, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    /// `res` in 0..1.
    pub fn set(&mut self, cutoff: f32, res: f32, sr: f32) {
        let fc = cutoff.clamp(10.0, sr * 0.49);
        let g = (PI * fc / sr).tan();
        let k = 2.0 - 1.96 * res.clamp(0.0, 1.0);
        self.k = k;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    #[inline]
    pub fn process(&mut self, v0: f32, mode: FilterMode) -> f32 {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        match mode {
            FilterMode::Lowpass => v2,
            FilterMode::Bandpass => v1,
            FilterMode::Highpass => v0 - self.k * v1 - v2,
        }
    }
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

/// RBJ cookbook biquad (transposed direct form II).
#[derive(Clone, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

pub enum BiquadKind {
    LowShelf,
    HighShelf,
    Peak,
    Lowpass,
    Highpass,
    Bandpass,
}

impl Biquad {
    pub fn set(&mut self, kind: BiquadKind, freq: f32, q: f32, gain_db: f32, sr: f32) {
        let w0 = TAU * freq.clamp(10.0, sr * 0.49) / sr;
        let (sw, cw) = w0.sin_cos();
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sw / (2.0 * q.max(0.05));
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cw,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cw,
                1.0 - alpha / a,
            ),
            BiquadKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cw + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    a * ((a + 1.0) - (a - 1.0) * cw - s),
                    (a + 1.0) + (a - 1.0) * cw + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    (a + 1.0) + (a - 1.0) * cw - s,
                )
            }
            BiquadKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cw + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - s),
                    (a + 1.0) - (a - 1.0) * cw + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - s,
                )
            }
            BiquadKind::Lowpass => (
                (1.0 - cw) / 2.0,
                1.0 - cw,
                (1.0 - cw) / 2.0,
                1.0 + alpha,
                -2.0 * cw,
                1.0 - alpha,
            ),
            BiquadKind::Highpass => (
                (1.0 + cw) / 2.0,
                -(1.0 + cw),
                (1.0 + cw) / 2.0,
                1.0 + alpha,
                -2.0 * cw,
                1.0 - alpha,
            ),
            BiquadKind::Bandpass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush(self.b2 * x - self.a2 * y);
        y
    }
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One-pole low-pass.
#[derive(Clone, Default)]
pub struct OnePole {
    z: f32,
    a: f32,
}

impl OnePole {
    pub fn set(&mut self, cutoff: f32, sr: f32) {
        self.a = (-TAU * cutoff.clamp(1.0, sr * 0.49) / sr).exp();
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.z = flush(x * (1.0 - self.a) + self.z * self.a);
        self.z
    }
    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR envelope with linear attack and exponential decay/release.
#[derive(Clone)]
pub struct Adsr {
    stage: Stage,
    pub level: f32,
    attack_step: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Adsr {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 1.0,
            decay_coef: 0.0,
            sustain: 1.0,
            release_coef: 0.0,
        }
    }
}

impl Adsr {
    pub fn set(&mut self, a: f32, d: f32, s: f32, r: f32, sr: f32) {
        self.attack_step = 1.0 / (a.max(0.0005) * sr);
        self.decay_coef = settle_coef(d.max(0.001), sr);
        self.sustain = s.clamp(0.0, 1.0);
        self.release_coef = settle_coef(r.max(0.001), sr);
    }
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }
    /// Very fast fade used for voice stealing.
    pub fn kill(&mut self, sr: f32) {
        self.stage = Stage::Release;
        self.release_coef = settle_coef(0.004, sr);
    }
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }
    pub fn is_released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Idle)
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Linear parameter ramp across a block, to avoid zipper noise.
#[derive(Clone, Copy, Default)]
pub struct Ramp {
    pub current: f32,
    target: f32,
    initialized: bool,
}

impl Ramp {
    pub fn set(&mut self, v: f32) {
        self.target = v;
        if !self.initialized {
            self.current = v;
            self.initialized = true;
        }
    }
    /// Returns (start, per-sample increment) for a block of `n` samples and
    /// advances to the target.
    pub fn block(&mut self, n: usize) -> (f32, f32) {
        let start = self.current;
        let inc = if n == 0 {
            0.0
        } else {
            (self.target - start) / n as f32
        };
        self.current = self.target;
        (start, inc)
    }
}

/// Cubic Hermite interpolation of `data` at fractional index `pos`.
#[inline]
pub fn hermite(data: &[f32], pos: f64) -> f32 {
    let n = data.len();
    if n == 0 {
        return 0.0;
    }
    let i = pos.floor() as isize;
    let f = (pos - i as f64) as f32;
    let at = |k: isize| -> f32 {
        if k < 0 || k as usize >= n {
            0.0
        } else {
            data[k as usize]
        }
    };
    let (xm1, x0, x1, x2) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let c0 = x0;
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * f + c2) * f + c1) * f + c0
}

/// Soft saturation.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x.abs() < 1e-3 {
        x
    } else {
        x.tanh()
    }
}

/// DC-blocking high-pass (about 10 Hz).
#[derive(Clone, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
}

impl DcBlock {
    #[inline]
    pub fn process(&mut self, x: f32, r: f32) -> f32 {
        let y = flush(x - self.x1 + r * self.y1);
        self.x1 = x;
        self.y1 = y;
        y
    }
    /// Pole radius for a ~10 Hz corner.
    pub fn coef(sr: f32) -> f32 {
        1.0 - (TAU * 10.0 / sr)
    }
}
