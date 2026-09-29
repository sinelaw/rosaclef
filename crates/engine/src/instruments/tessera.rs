//! "Tessera": wavetable synthesizer.
//!
//! The selected table is generated procedurally when it changes (never per
//! note): 32 frames, each defined by a harmonic spectrum (built directly, or
//! measured with an FFT from a time-domain recipe) and rendered with an
//! inverse FFT into a mip-map of ten band-limited levels, one per octave. A
//! note reads the level whose highest harmonic stays below Nyquist, so high
//! notes do not alias. Frames and samples are linearly interpolated.
//!
//! Tables (frame 0 → frame 31):
//! - `analog`: sine → triangle → saw → square → narrow pulse.
//! - `digital`: a narrow band of harmonics sweeping upwards, then sparse,
//!   bright harmonic combs.
//! - `vocal`: formant vowel sweep a-e-i-o-u over a glottal-like source.
//! - `growl`: two-modulator FM with a rising index, lightly saturated.
//! - `glass`: sparse high partials, brightness rising along the table.
//! - `pulse`: pulse width sweeping from square to a thin needle.
//!
//! Signal path per voice: up to 7 unison oscillators (warped phase) + sine
//! sub → state-variable filter → drive → amplitude envelope. A per-voice mod
//! envelope (instant attack, `modDecay`) moves position and cutoff; a sine
//! LFO moves the position.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

const FRAMES: usize = 32;
const LEVELS: usize = 10;
/// Samples per frame of the fullest mip level.
const N0: usize = 4096;
/// Harmonics of the fullest mip level (level k keeps `H0 >> k`).
const H0: usize = 512;
const MIN_LEN: usize = 128;
const MAX_UNISON: usize = 7;
const CONTROL: usize = 16;
const CHUNK: usize = 128;
const STEAL_TIME: f32 = 0.004;
/// Highest harmonic frequency allowed, as a fraction of the sample rate.
const BAND_LIMIT: f32 = 0.48;

fn level_len(k: usize) -> usize {
    (N0 >> k).max(MIN_LEN)
}

fn level_harmonics(k: usize) -> usize {
    (H0 >> k).max(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Table {
    Analog,
    Digital,
    Vocal,
    Growl,
    Glass,
    Pulse,
}

impl Table {
    fn parse(s: &str) -> Table {
        match s {
            "digital" => Table::Digital,
            "vocal" => Table::Vocal,
            "growl" => Table::Growl,
            "glass" => Table::Glass,
            "pulse" => Table::Pulse,
            _ => Table::Analog,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Warp {
    None,
    Bend,
    Sync,
    Fold,
    Mirror,
}

impl Warp {
    fn parse(s: &str) -> Warp {
        match s {
            "bend" => Warp::Bend,
            "sync" => Warp::Sync,
            "fold" => Warp::Fold,
            "mirror" => Warp::Mirror,
            _ => Warp::None,
        }
    }
}

// ------------------------------------------------------------ table builder

/// Scratch space and FFT tables for generating wavetables.
struct Builder {
    cos: Vec<f32>,
    sin: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    /// Cosine and sine coefficient of each harmonic (index 0 unused).
    a: Vec<f32>,
    b: Vec<f32>,
    /// Fixed pseudo-random phase per harmonic (same in every frame, so frames
    /// crossfade without cancelling).
    phase: Vec<f32>,
}

impl Builder {
    fn new() -> Builder {
        let mut rng = Rng::new(0x7e55e7a);
        Builder {
            cos: (0..N0 / 2).map(|k| (TAU * k as f32 / N0 as f32).cos()).collect(),
            sin: (0..N0 / 2).map(|k| (TAU * k as f32 / N0 as f32).sin()).collect(),
            re: vec![0.0; N0],
            im: vec![0.0; N0],
            a: vec![0.0; H0 + 1],
            b: vec![0.0; H0 + 1],
            phase: (0..=H0).map(|_| rng.unit() * TAU).collect(),
        }
    }

    /// In-place radix-2 FFT of `re/im[..n]` (`inverse`: e^{+i}, unscaled).
    fn fft(&mut self, n: usize, inverse: bool) {
        let (re, im) = (&mut self.re[..n], &mut self.im[..n]);
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let sign = if inverse { 1.0 } else { -1.0 };
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = N0 / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let wr = self.cos[k * step];
                    let wi = sign * self.sin[k * step];
                    let (x, y) = (start + k, start + k + half);
                    let tr = re[y] * wr - im[y] * wi;
                    let ti = re[y] * wi + im[y] * wr;
                    re[y] = re[x] - tr;
                    im[y] = im[x] - ti;
                    re[x] += tr;
                    im[x] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// Measure the harmonic spectrum of the waveform in `re[..N0]`.
    fn analyze(&mut self) {
        self.im.fill(0.0);
        self.fft(N0, false);
        let s = 2.0 / N0 as f32;
        for h in 1..=H0 {
            self.a[h] = self.re[h] * s;
            self.b[h] = -self.im[h] * s;
        }
    }

    /// Render harmonics `1..=harm` into `re[..n]`.
    fn synthesize(&mut self, n: usize, harm: usize) {
        self.re[..n].fill(0.0);
        self.im[..n].fill(0.0);
        for h in 1..=harm.min(n / 2 - 1) {
            self.re[h] = self.a[h];
            self.im[h] = -self.b[h];
        }
        self.fft(n, true);
    }

    fn add_phased(&mut self, h: usize, amp: f32) {
        let ph = self.phase[h];
        self.a[h] += amp * ph.cos();
        self.b[h] += amp * ph.sin();
    }

    /// Fill `a/b` with the spectrum of frame position `t` in 0..1.
    fn spectrum(&mut self, table: Table, t: f32) {
        self.a.fill(0.0);
        self.b.fill(0.0);
        match table {
            Table::Analog => {
                let seg = (t * 4.0).min(3.999);
                let w = seg.fract();
                match seg as usize {
                    0 => self.blend(|h| shape_sine(h), |h| shape_triangle(h), w),
                    1 => self.blend(|h| shape_triangle(h), |h| shape_saw(h), w),
                    2 => self.blend(|h| shape_saw(h), |h| shape_pulse(h, 0.5), w),
                    _ => {
                        let duty = 0.5 - 0.4 * w;
                        self.blend(|h| shape_pulse(h, duty), |_| (0.0, 0.0), 0.0)
                    }
                }
            }
            Table::Pulse => {
                let duty = 0.5 - 0.47 * t;
                self.blend(|h| shape_pulse(h, duty), |_| (0.0, 0.0), 0.0);
            }
            Table::Digital => {
                if t < 0.5 {
                    // A one-octave-wide band of harmonics sweeping upwards.
                    let u = t * 2.0;
                    let center = (u * 6.5).exp2();
                    for h in 1..=H0 {
                        let d = ((h as f32).log2() - center.log2()) / 0.5;
                        let amp = (-d * d).exp() * (1.0 + 0.3 * u) + if h == 1 { 0.45 } else { 0.0 };
                        let sign = if h % 2 == 0 { -1.0 } else { 1.0 };
                        self.b[h] = amp * sign;
                    }
                } else {
                    // Sparse combs: harmonics 1, 1+s, 1+2s... with a growing step.
                    let u = (t - 0.5) * 2.0;
                    let step = 2 + (u * 6.99) as usize;
                    for h in 1..=H0 {
                        if (h - 1) % step == 0 && h <= 192 {
                            self.b[h] = if h == 1 { 1.0 } else { 0.9 / (h as f32).powf(0.45) };
                        }
                    }
                }
            }
            Table::Vocal => {
                const VOWELS: [[f32; 3]; 5] = [[730.0, 1090.0, 2440.0], [530.0, 1840.0, 2480.0], [270.0, 2290.0, 3010.0], [570.0, 840.0, 2410.0], [300.0, 870.0, 2240.0]];
                const GAIN: [f32; 3] = [1.0, 0.6, 0.35];
                const BW: [f32; 3] = [110.0, 150.0, 210.0];
                /// Reference fundamental of the table (formants move with pitch).
                const F0: f32 = 150.0;
                let seg = (t * 4.0).min(3.999);
                let (i, w) = (seg as usize, seg.fract());
                let mut f = [0f32; 3];
                for k in 0..3 {
                    f[k] = VOWELS[i][k] + (VOWELS[i + 1][k] - VOWELS[i][k]) * w;
                }
                for h in 1..=H0 {
                    let hz = h as f32 * F0;
                    if hz > 9000.0 {
                        break;
                    }
                    let mut env = 0.03;
                    for k in 0..3 {
                        let x = (hz - f[k]) / BW[k];
                        env += GAIN[k] / (1.0 + x * x);
                    }
                    self.add_phased(h, env / (h as f32).powf(0.6));
                }
            }
            Table::Glass => {
                const PARTIALS: [usize; 16] = [1, 2, 5, 7, 11, 13, 17, 21, 26, 31, 37, 43, 53, 61, 71, 83];
                let center = t * 15.0;
                for (k, &h) in PARTIALS.iter().enumerate() {
                    let d = (k as f32 - center) / 2.5;
                    let amp = if k == 0 { 1.0 } else { 0.9 * (-d * d).exp() + 0.25 / (1.0 + k as f32) };
                    self.add_phased(h, amp);
                }
            }
            Table::Growl => {
                // Two-modulator FM (ratios 2 and 3) with a rising index.
                let index = 0.4 + 9.0 * t;
                let drive = 1.0 + 1.5 * t;
                let norm = drive.tanh();
                for n in 0..N0 {
                    let ph = TAU * n as f32 / N0 as f32;
                    let m = 0.7 * (2.0 * ph).sin() + 0.3 * t * (3.0 * ph).sin();
                    let x = (ph + index * m).sin();
                    self.re[n] = (drive * x).tanh() / norm;
                }
                self.analyze();
            }
        }
    }

    fn blend(&mut self, x: impl Fn(usize) -> (f32, f32), y: impl Fn(usize) -> (f32, f32), w: f32) {
        for h in 1..=H0 {
            let (a0, b0) = x(h);
            let (a1, b1) = y(h);
            self.a[h] = a0 + (a1 - a0) * w;
            self.b[h] = b0 + (b1 - b0) * w;
        }
    }

    /// Generate every frame and mip level of `table` into `data`.
    fn build(&mut self, table: Table, data: &mut [Vec<f32>]) {
        for f in 0..FRAMES {
            self.spectrum(table, f as f32 / (FRAMES - 1) as f32);
            for (k, level) in data.iter_mut().enumerate() {
                let n = level_len(k);
                self.synthesize(n, level_harmonics(k));
                let dst = &mut level[f * (n + 1)..(f + 1) * (n + 1)];
                dst[..n].copy_from_slice(&self.re[..n]);
                dst[n] = dst[0];
            }
        }
        // Normalize each frame to the peak of its fullest level.
        for f in 0..FRAMES {
            let n = level_len(0);
            let peak = data[0][f * (n + 1)..(f + 1) * (n + 1)].iter().fold(0f32, |m, x| m.max(x.abs()));
            let g = if peak > 1e-9 { 1.0 / peak } else { 0.0 };
            for (k, level) in data.iter_mut().enumerate() {
                let n = level_len(k);
                for x in &mut level[f * (n + 1)..(f + 1) * (n + 1)] {
                    *x *= g;
                }
            }
        }
    }
}

/// (cos, sin) Fourier coefficients of the basic shapes.
fn shape_sine(h: usize) -> (f32, f32) {
    (0.0, if h == 1 { 1.0 } else { 0.0 })
}

fn shape_triangle(h: usize) -> (f32, f32) {
    if h % 2 == 0 {
        return (0.0, 0.0);
    }
    let sign = if (h / 2) % 2 == 0 { 1.0 } else { -1.0 };
    (0.0, sign * 8.0 / (std::f32::consts::PI * std::f32::consts::PI * (h * h) as f32))
}

fn shape_saw(h: usize) -> (f32, f32) {
    let sign = if h % 2 == 1 { 1.0 } else { -1.0 };
    (0.0, sign * 2.0 / (std::f32::consts::PI * h as f32))
}

/// Pulse of the given duty cycle (+1 then -1), DC removed.
fn shape_pulse(h: usize, duty: f32) -> (f32, f32) {
    let k = 2.0 / (std::f32::consts::PI * h as f32);
    let x = TAU * h as f32 * duty;
    (k * x.sin(), k * (1.0 - x.cos()))
}

// ------------------------------------------------------------------- engine

#[derive(Clone)]
struct Params {
    position: f32,
    pos_env: f32,
    pos_lfo: f32,
    lfo_rate: f32,
    warp: Warp,
    warp_amt: f32,
    unison: usize,
    detune: f32,
    width: f32,
    sub: f32,
    mode: FilterMode,
    cutoff: f32,
    resonance: f32,
    filter_env: f32,
    mod_coef: f32,
    drive: f32,
    env_times: (f32, f32, f32, f32),
    gain: f32,
}

#[derive(Clone, Default)]
struct Voice {
    active: bool,
    key: u8,
    velocity: f32,
    age: u64,
    freq: f32,
    phase: [f32; MAX_UNISON],
    sub_phase: f32,
    lfo_phase: f32,
    menv: f32,
    pos: f32,
    pos_inc: f32,
    fresh: bool,
    counter: usize,
    env: Adsr,
    svf_l: Svf,
    svf_r: Svf,
    fade: f32,
    stealing: bool,
    pending: Option<(u8, f32, bool)>,
}

impl Voice {
    fn start(&mut self, key: u8, velocity: f32, age: u64, p: &Params, sr: f32, rng: &mut Rng) {
        self.active = true;
        self.key = key;
        self.velocity = velocity;
        self.age = age;
        self.freq = midi_to_hz(key as f32);
        for ph in &mut self.phase {
            *ph = if p.unison > 1 { rng.unit() } else { 0.0 };
        }
        self.sub_phase = 0.0;
        self.lfo_phase = 0.0;
        self.menv = 1.0;
        self.pos = 0.0;
        self.pos_inc = 0.0;
        self.fresh = true;
        self.counter = 0;
        self.env = Adsr::default();
        let (a, d, s, r) = p.env_times;
        self.env.set(a, d, s, r, sr);
        self.env.trigger();
        self.svf_l.reset();
        self.svf_r.reset();
        self.fade = 1.0;
        self.stealing = false;
        self.pending = None;
    }
}

/// Fast tanh (Padé), exact at 0 and saturating at ±1 beyond ±3.
#[inline]
fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Read a table level at phase `ph` between frames `fa` and `fa + 1`.
#[inline]
fn read(level: &[f32], n: usize, fa: usize, ff: f32, ph: f32) -> f32 {
    let x = ph * n as f32;
    let i = (x as usize).min(n - 1);
    let fr = x - i as f32;
    let a = &level[fa * (n + 1) + i..fa * (n + 1) + i + 2];
    let b = &level[(fa + 1) * (n + 1) + i..(fa + 1) * (n + 1) + i + 2];
    let sa = a[0] + (a[1] - a[0]) * fr;
    let sb = b[0] + (b[1] - b[0]) * fr;
    sa + (sb - sa) * ff
}

pub struct Tessera {
    sr: f32,
    p: Params,
    table: Option<Table>,
    data: Vec<Vec<f32>>,
    builder: Builder,
    voices: Vec<Voice>,
    rng: Rng,
    clock: u64,
    buf_l: [f32; CHUNK],
    buf_r: [f32; CHUNK],
    dc_l: DcBlock,
    dc_r: DcBlock,
}

impl Tessera {
    pub fn new(sr: f32) -> Tessera {
        Tessera {
            sr,
            p: Params {
                position: 0.25,
                pos_env: 0.0,
                pos_lfo: 0.0,
                lfo_rate: 0.5,
                warp: Warp::None,
                warp_amt: 0.0,
                unison: 3,
                detune: 14.0,
                width: 0.8,
                sub: 0.0,
                mode: FilterMode::Lowpass,
                cutoff: 12000.0,
                resonance: 0.1,
                filter_env: 0.0,
                mod_coef: settle_coef(0.6, sr),
                drive: 0.0,
                env_times: (0.005, 0.3, 0.7, 0.25),
                gain: 0.55,
            },
            table: None,
            data: (0..LEVELS).map(|k| vec![0.0; FRAMES * (level_len(k) + 1)]).collect(),
            builder: Builder::new(),
            voices: vec![Voice::default(); MAX_VOICES],
            rng: Rng::new(0x7e55e7a),
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
        let n_uni = p.unison;
        // Unison detune ratios and pans.
        let mut det = [1f32; MAX_UNISON];
        let mut pl = [1f32; MAX_UNISON];
        let mut pr = [1f32; MAX_UNISON];
        for u in 0..n_uni {
            let x = if n_uni == 1 { 0.0 } else { u as f32 / (n_uni - 1) as f32 * 2.0 - 1.0 };
            det[u] = cents(x * p.detune * 0.5);
            let (l, r) = pan_gains(x * p.width);
            pl[u] = l;
            pr[u] = r;
        }
        let norm = 1.0 / (n_uni as f32).sqrt();
        let amt = p.warp_amt;
        let warp = if amt > 0.0005 { p.warp } else { Warp::None };
        // How much the warp raises the waveform's frequency content.
        let bend_k = 0.5 - 0.4 * amt;
        let sync_mult = 1.0 + 7.0 * amt;
        let warp_factor = match warp {
            Warp::Bend => 0.5 / bend_k,
            Warp::Sync => sync_mult,
            Warp::Mirror => 2.0,
            Warp::Fold => 1.0 + 2.0 * amt,
            Warp::None => 1.0,
        };
        let fold_mix = (amt * 4.0).min(1.0);
        let fold_gain = 1.0 + 5.0 * amt;
        let drive_g = p.drive * 8.0;
        let drive_norm = if drive_g > 0.0 { 1.0 / fast_tanh(drive_g) } else { 1.0 };
        let mod_coef_c = p.mod_coef.powi(CONTROL as i32);
        let lfo_inc_c = p.lfo_rate * CONTROL as f32 / sr;
        let steal_step = 1.0 / (STEAL_TIME * sr);
        let data = &self.data;
        let (bl, br) = (&mut self.buf_l[..n], &mut self.buf_r[..n]);

        for v in self.voices.iter_mut().filter(|v| v.active) {
            let mut i = 0;
            'voice: while i < n {
                let mut dt = [0f32; MAX_UNISON];
                for u in 0..n_uni {
                    dt[u] = (v.freq * det[u] / sr).min(0.49);
                }
                let f_top = v.freq * det[n_uni - 1].max(det[0]) * warp_factor;
                let mut k = 0;
                while k + 1 < LEVELS && level_harmonics(k) as f32 * f_top > BAND_LIMIT * sr {
                    k += 1;
                }
                let level = &data[k][..];
                let len = level_len(k);
                let vel_gain = (0.35 + 0.65 * v.velocity) * p.gain * 0.6;
                let sub_dt = v.freq * 0.5 / sr;
                while i < n {
                    if v.counter % CONTROL == 0 {
                        let lfo = (v.lfo_phase * TAU).sin();
                        v.lfo_phase = (v.lfo_phase + lfo_inc_c).fract();
                        let target = (p.position + p.pos_env * v.menv + p.pos_lfo * 0.5 * lfo).clamp(0.0, 1.0);
                        if v.fresh {
                            v.pos = target;
                            v.fresh = false;
                        }
                        v.pos_inc = (target - v.pos) / CONTROL as f32;
                        let oct = p.filter_env * 6.0 * v.menv;
                        let fc = (p.cutoff * oct.exp2()).clamp(20.0, 20000.0);
                        v.svf_l.set(fc, p.resonance, sr);
                        v.svf_r.set(fc, p.resonance, sr);
                        v.menv *= mod_coef_c;
                    }
                    v.counter += 1;
                    v.pos += v.pos_inc;
                    let fpos = v.pos.clamp(0.0, 1.0) * (FRAMES - 1) as f32;
                    let fa = (fpos as usize).min(FRAMES - 2);
                    let ff = fpos - fa as f32;

                    let mut l = 0.0;
                    let mut r = 0.0;
                    for u in 0..n_uni {
                        let ph = v.phase[u];
                        let s = match warp {
                            Warp::None | Warp::Fold => read(level, len, fa, ff, ph),
                            Warp::Bend => {
                                let q = if ph < bend_k { 0.5 * ph / bend_k } else { 0.5 + 0.5 * (ph - bend_k) / (1.0 - bend_k) };
                                read(level, len, fa, ff, q)
                            }
                            Warp::Sync => {
                                let q = ph * sync_mult;
                                read(level, len, fa, ff, q - q.floor())
                            }
                            Warp::Mirror => {
                                let q = if ph < 0.5 { 2.0 * ph } else { 2.0 - 2.0 * ph };
                                let a = read(level, len, fa, ff, ph);
                                let m = read(level, len, fa, ff, q.min(0.999_999));
                                a + (m - a) * amt
                            }
                        };
                        let ph = ph + dt[u];
                        v.phase[u] = if ph >= 1.0 { ph - 1.0 } else { ph };
                        l += s * pl[u];
                        r += s * pr[u];
                    }
                    l *= norm;
                    r *= norm;
                    if warp == Warp::Fold {
                        let fl = (l * fold_gain * std::f32::consts::FRAC_PI_2).sin();
                        let fr = (r * fold_gain * std::f32::consts::FRAC_PI_2).sin();
                        l += (fl - l) * fold_mix;
                        r += (fr - r) * fold_mix;
                    }
                    if p.sub > 0.0 {
                        let s = (v.sub_phase * TAU).sin() * p.sub;
                        v.sub_phase += sub_dt;
                        if v.sub_phase >= 1.0 {
                            v.sub_phase -= 1.0;
                        }
                        l += s;
                        r += s;
                    }
                    let mut yl = v.svf_l.process(l, p.mode);
                    let mut yr = v.svf_r.process(r, p.mode);
                    if drive_g > 0.0 {
                        yl = fast_tanh(yl * drive_g) * drive_norm;
                        yr = fast_tanh(yr * drive_g) * drive_norm;
                    }
                    let g = v.env.next() * vel_gain * v.fade;
                    bl[i] += yl * g;
                    br[i] += yr * g;
                    i += 1;
                    if v.stealing {
                        v.fade -= steal_step;
                        if v.fade <= 0.0 {
                            match v.pending.take() {
                                Some((key, vel, released)) => {
                                    v.start(key, vel, v.age, p, sr, &mut self.rng);
                                    if released {
                                        v.env.release();
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
            if v.active && !v.stealing && v.env.is_idle() {
                v.active = false;
            }
        }
    }
}

impl Instrument for Tessera {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let sr = self.sr;
        let f = |k: &str| d.param(k) as f32;
        self.p = Params {
            position: f("position").clamp(0.0, 1.0),
            pos_env: f("positionEnv"),
            pos_lfo: f("positionLfo"),
            lfo_rate: f("lfoRate"),
            warp: Warp::parse(d.option("warp")),
            warp_amt: f("warp").clamp(0.0, 1.0),
            unison: (f("unison").round() as usize).clamp(1, MAX_UNISON),
            detune: f("detune"),
            width: f("width").clamp(0.0, 1.0),
            sub: f("sub"),
            mode: FilterMode::parse(d.option("filter")),
            cutoff: f("cutoff"),
            resonance: f("resonance"),
            filter_env: f("filterEnv"),
            mod_coef: settle_coef(f("modDecay"), sr),
            drive: f("drive").clamp(0.0, 1.0),
            env_times: (f("attack"), f("decay"), f("sustain"), f("release")),
            gain: f("gain"),
        };
        let (a, dd, s, r) = self.p.env_times;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.env.set(a, dd, s, r, sr);
        }
        let table = Table::parse(d.option("table"));
        if self.table != Some(table) {
            self.builder.build(table, &mut self.data);
            self.table = Some(table);
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = pick_voice(&self.voices, |v| v.active, |v| v.stealing || v.env.is_released(), |v| v.age);
                let v = &mut self.voices[i];
                if v.active {
                    v.stealing = true;
                    v.age = self.clock;
                    v.pending = Some((key, velocity, false));
                } else {
                    v.start(key, velocity, self.clock, &self.p, self.sr, &mut self.rng);
                }
            }
            NoteKind::Off { key } => {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    if let Some((k, _, released)) = &mut v.pending {
                        if *k == key {
                            *released = true;
                        }
                    } else if v.key == key {
                        v.env.release();
                    }
                }
            }
            NoteKind::AllOff => {
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    if let Some((_, _, released)) = &mut v.pending {
                        *released = true;
                    } else {
                        v.env.release();
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
