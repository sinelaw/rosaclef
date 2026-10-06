//! Built-in insert effects.

use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

pub trait Effect: Send {
    fn set_device(&mut self, dev: &Device, ctx: &Ctx);
    /// Process a stereo block in place.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    fn reset(&mut self) {}
    /// Dynamics processors (compressor, limiter): the gain reduction they
    /// applied since the last call, in dB — its largest value and its mean
    /// over the samples — and start counting again. `None` for the rest.
    fn take_gain_reduction(&mut self) -> Option<(f32, f32)> {
        None
    }
    /// Measure the gain reduction (offline analysis only: off, it costs
    /// the real-time path nothing).
    fn set_metering(&mut self, _on: bool) {}
}

/// Gain reduction seen by a dynamics processor since it was last read
/// (while metering is on).
#[derive(Clone, Copy, Debug, Default)]
struct GrMeter {
    on: bool,
    max: f32,
    sum: f64,
    n: u32,
}

impl GrMeter {
    #[inline]
    fn add(&mut self, db: f32) {
        self.max = self.max.max(db);
        self.sum += db as f64;
        self.n = self.n.saturating_add(1);
    }
    fn take(&mut self) -> Option<(f32, f32)> {
        let m = std::mem::replace(
            self,
            GrMeter {
                on: self.on,
                ..Default::default()
            },
        );
        Some((
            m.max,
            if m.n > 0 {
                (m.sum / m.n as f64) as f32
            } else {
                0.0
            },
        ))
    }
}

pub fn create(dev: &Device, ctx: &Ctx) -> Option<Box<dyn Effect>> {
    let sr = ctx.sr;
    let mut fx: Box<dyn Effect> = match dev.kind.as_str() {
        "eq" => Box::new(Eq3::default()),
        "filter" => Box::new(Filter::default()),
        "delay" => Box::new(Delay::new(sr)),
        "reverb" => Box::new(Reverb::new(sr)),
        "chorus" => Box::new(Chorus::new(sr)),
        "phaser" => Box::new(Phaser::new(sr)),
        "drive" => Box::new(Drive::default()),
        "compressor" => Box::new(Compressor::default()),
        "limiter" => Box::new(Limiter::new(sr)),
        _ => return None,
    };
    fx.set_device(dev, ctx);
    Some(fx)
}

#[inline]
fn mix(dry: f32, wet: f32, m: f32) -> f32 {
    dry + (wet - dry) * m
}

// ------------------------------------------------------------------------ EQ

#[derive(Default)]
pub struct Eq3 {
    bands: [[Biquad; 3]; 2],
}

impl Effect for Eq3 {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        for ch in &mut self.bands {
            ch[0].set(
                BiquadKind::LowShelf,
                d.param("lowFreq") as f32,
                0.707,
                d.param("low") as f32,
                ctx.sr,
            );
            ch[1].set(
                BiquadKind::Peak,
                d.param("midFreq") as f32,
                d.param("midQ") as f32,
                d.param("mid") as f32,
                ctx.sr,
            );
            ch[2].set(
                BiquadKind::HighShelf,
                d.param("highFreq") as f32,
                0.707,
                d.param("high") as f32,
                ctx.sr,
            );
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (buf, bands) in [left, right].into_iter().zip(self.bands.iter_mut()) {
            for x in buf.iter_mut() {
                let a = bands[0].process(*x);
                let b = bands[1].process(a);
                *x = bands[2].process(b);
            }
        }
    }
    fn reset(&mut self) {
        self.bands.iter_mut().flatten().for_each(Biquad::reset);
    }
}

// -------------------------------------------------------------------- Filter

pub struct Filter {
    svf: [Svf; 2],
    mode: FilterMode,
    mix: f32,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            svf: Default::default(),
            mode: FilterMode::Lowpass,
            mix: 1.0,
        }
    }
}

impl Effect for Filter {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        self.mode = FilterMode::parse(d.option("mode"));
        self.mix = d.param("mix") as f32;
        for s in &mut self.svf {
            s.set(
                d.param("cutoff") as f32,
                d.param("resonance") as f32,
                ctx.sr,
            );
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (mode, m) = (self.mode, self.mix);
        for (buf, svf) in [left, right].into_iter().zip(self.svf.iter_mut()) {
            for x in buf.iter_mut() {
                *x = mix(*x, svf.process(*x, mode), m);
            }
        }
    }
    fn reset(&mut self) {
        self.svf.iter_mut().for_each(Svf::reset);
    }
}

// --------------------------------------------------------------------- Delay

/// Fractional delay line.
struct DelayLine {
    buf: Vec<f32>,
    w: usize,
}

impl DelayLine {
    fn new(len: usize) -> DelayLine {
        DelayLine {
            buf: vec![0.0; len.max(4)],
            w: 0,
        }
    }
    #[inline]
    fn write(&mut self, x: f32) {
        self.buf[self.w] = flush(x);
        self.w += 1;
        if self.w == self.buf.len() {
            self.w = 0;
        }
    }
    /// Read `d` samples behind the last write (linear interpolation).
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let d = d.clamp(1.0, (n - 2) as f32);
        let pos = self.w as f32 - d;
        let pos = if pos < 0.0 { pos + n as f32 } else { pos };
        // `pos` is in [0, n); the checks guard float rounding and the wrap
        // (an integer `%` per read is costly on the audio thread).
        let i = (pos as usize).min(n - 1);
        let f = pos - i as f32;
        let a = self.buf[i];
        let b = self.buf[if i + 1 == n { 0 } else { i + 1 }];
        a + (b - a) * f
    }
    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|x| *x = 0.0);
    }
}

pub struct Delay {
    sr: f32,
    lines: [DelayLine; 2],
    tone: [OnePole; 2],
    time: f32,
    target_time: f32,
    feedback: f32,
    mix: f32,
    pingpong: bool,
}

impl Delay {
    fn new(sr: f32) -> Delay {
        let len = (sr * 6.0) as usize;
        Delay {
            sr,
            lines: [DelayLine::new(len), DelayLine::new(len)],
            tone: Default::default(),
            time: 0.0,
            target_time: 0.0,
            feedback: 0.35,
            mix: 0.25,
            pingpong: true,
        }
    }
}

impl Effect for Delay {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        let seconds = d.param("time") as f32 * 60.0 / ctx.bpm.max(1.0);
        self.target_time = (seconds * self.sr).min(self.sr * 5.9);
        if self.time == 0.0 {
            self.time = self.target_time;
        }
        self.feedback = d.param("feedback") as f32;
        self.mix = d.param("mix") as f32;
        self.pingpong = d.option("mode") == "pingpong";
        for t in &mut self.tone {
            t.set(d.param("tone") as f32, self.sr);
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let slew = 1.0 - settle_coef(0.08, self.sr);
        for i in 0..left.len() {
            self.time += (self.target_time - self.time) * slew;
            let (xl, xr) = (left[i], right[i]);
            let dl = self.tone[0].process(self.lines[0].read(self.time));
            let dr = self.tone[1].process(self.lines[1].read(self.time));
            if self.pingpong {
                let mono = (xl + xr) * 0.5;
                self.lines[0].write(mono + dr * self.feedback);
                self.lines[1].write(dl * self.feedback);
            } else {
                self.lines[0].write(xl + dl * self.feedback);
                self.lines[1].write(xr + dr * self.feedback);
            }
            left[i] = xl + dl * self.mix;
            right[i] = xr + dr * self.mix;
        }
    }
    fn reset(&mut self) {
        self.lines.iter_mut().for_each(DelayLine::clear);
        self.tone.iter_mut().for_each(OnePole::reset);
    }
}

// -------------------------------------------------------------------- Reverb

struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Comb {
    fn new(n: usize) -> Comb {
        Comb {
            buf: vec![0.0; n.max(1)],
            i: 0,
            store: 0.0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.i];
        self.store = flush(y * (1.0 - damp) + self.store * damp);
        self.buf[self.i] = flush(x + self.store * feedback);
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn new(n: usize) -> Allpass {
        Allpass {
            buf: vec![0.0; n.max(1)],
            i: 0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = flush(x + b * 0.5);
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        b - x
    }
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

/// Freeverb-style reverb with pre-delay and a gentle input high-cut.
pub struct Reverb {
    sr: f32,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    pre: DelayLine,
    pre_time: f32,
    hicut: OnePole,
    lowcut: [OnePole; 2],
    feedback: f32,
    damp: f32,
    width: f32,
    mix: f32,
}

impl Reverb {
    fn new(sr: f32) -> Reverb {
        let scale = sr / 44100.0;
        let mk = |extra: usize| -> (Vec<Comb>, Vec<Allpass>) {
            (
                COMBS
                    .iter()
                    .map(|n| Comb::new(((n + extra) as f32 * scale) as usize))
                    .collect(),
                ALLPASSES
                    .iter()
                    .map(|n| Allpass::new(((n + extra) as f32 * scale) as usize))
                    .collect(),
            )
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(SPREAD);
        let mut hicut = OnePole::default();
        hicut.set(9000.0, sr);
        let mut lowcut = [OnePole::default(), OnePole::default()];
        lowcut.iter_mut().for_each(|l| l.set(120.0, sr));
        Reverb {
            sr,
            combs: [cl, cr],
            allpasses: [al, ar],
            pre: DelayLine::new((sr * 0.3) as usize),
            pre_time: 1.0,
            hicut,
            lowcut,
            feedback: 0.84,
            damp: 0.2,
            width: 1.0,
            mix: 0.25,
        }
    }
}

impl Effect for Reverb {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.feedback = 0.7 + d.param("size") as f32 * 0.28;
        self.damp = d.param("damping") as f32 * 0.4;
        self.width = d.param("width") as f32;
        self.mix = d.param("mix") as f32;
        self.pre_time = (d.param("predelay") as f32 * self.sr).max(1.0);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let wet1 = self.width * 0.5 + 0.5;
        let wet2 = (1.0 - self.width) * 0.5;
        for i in 0..left.len() {
            let (xl, xr) = (left[i], right[i]);
            self.pre.write((xl + xr) * 0.5);
            let input = self.hicut.process(self.pre.read(self.pre_time)) * 0.03;
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let mut s = 0.0;
                for c in &mut self.combs[ch] {
                    s += c.process(input, self.feedback, self.damp);
                }
                for a in &mut self.allpasses[ch] {
                    s = a.process(s);
                }
                // Remove mud from the tail.
                out[ch] = s - self.lowcut[ch].process(s);
            }
            let wl = out[0] * wet1 + out[1] * wet2;
            let wr = out[1] * wet1 + out[0] * wet2;
            left[i] = xl * (1.0 - self.mix * 0.5) + wl * self.mix * 3.0;
            right[i] = xr * (1.0 - self.mix * 0.5) + wr * self.mix * 3.0;
        }
    }
}

// -------------------------------------------------------------------- Chorus

pub struct Chorus {
    sr: f32,
    lines: [DelayLine; 2],
    phase: f32,
    rate: f32,
    depth: f32,
    mix: f32,
}

impl Chorus {
    fn new(sr: f32) -> Chorus {
        let n = (sr * 0.06) as usize;
        Chorus {
            sr,
            lines: [DelayLine::new(n), DelayLine::new(n)],
            phase: 0.0,
            rate: 0.6,
            depth: 0.5,
            mix: 0.4,
        }
    }
}

impl Effect for Chorus {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.rate = d.param("rate") as f32;
        self.depth = d.param("depth") as f32;
        self.mix = d.param("mix") as f32;
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let base = 0.012 * self.sr;
        let depth = 0.006 * self.sr * self.depth;
        let inc = self.rate / self.sr;
        for i in 0..left.len() {
            let (xl, xr) = (left[i], right[i]);
            self.lines[0].write(xl);
            self.lines[1].write(xr);
            let ml = (self.phase * TAU).sin();
            let mr = ((self.phase + 0.25) * TAU).sin();
            let wl = self.lines[0].read(base + depth * ml);
            let wr = self.lines[1].read(base + depth * mr);
            self.phase = (self.phase + inc).fract();
            left[i] = mix(xl, wl, self.mix * 0.5) * (1.0 + self.mix * 0.2);
            right[i] = mix(xr, wr, self.mix * 0.5) * (1.0 + self.mix * 0.2);
        }
    }
}

// -------------------------------------------------------------------- Phaser

/// All-pass stages swept by an LFO, summed with the dry signal.
pub struct Phaser {
    sr: f32,
    phase: f32,
    rate: f32,
    depth: f32,
    freq: f32,
    feedback: f32,
    stages: usize,
    stereo: f32,
    mix: f32,
    /// Per channel: each stage's previous input and output, and the
    /// fed-back output.
    x1: [[f32; 12]; 2],
    y1: [[f32; 12]; 2],
    last: [f32; 2],
    coef: [f32; 2],
    counter: usize,
}

impl Phaser {
    fn new(sr: f32) -> Phaser {
        Phaser {
            sr,
            phase: 0.0,
            rate: 0.4,
            depth: 0.6,
            freq: 400.0,
            feedback: 0.0,
            stages: 6,
            stereo: 0.5,
            mix: 1.0,
            x1: [[0.0; 12]; 2],
            y1: [[0.0; 12]; 2],
            last: [0.0; 2],
            coef: [0.0; 2],
            counter: 0,
        }
    }

    /// First-order all-pass coefficient for a break frequency.
    fn coefficient(&self, f: f32) -> f32 {
        let t = (std::f32::consts::PI * f.clamp(20.0, self.sr * 0.45) / self.sr).tan();
        (t - 1.0) / (t + 1.0)
    }
}

impl Effect for Phaser {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.rate = d.param("rate") as f32;
        self.depth = d.param("depth") as f32;
        self.freq = d.param("freq") as f32;
        self.feedback = d.param("feedback") as f32;
        self.stages = (d.param("stages").round() as usize).clamp(2, 12);
        self.stereo = d.param("stereo") as f32;
        self.mix = d.param("mix") as f32;
    }
    fn reset(&mut self) {
        self.x1 = [[0.0; 12]; 2];
        self.y1 = [[0.0; 12]; 2];
        self.last = [0.0; 2];
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let inc = self.rate / self.sr;
        let octaves = self.depth * 6.0;
        let (dry, wet) = (1.0 - self.mix * 0.5, self.mix * 0.5);
        for i in 0..left.len() {
            if self.counter % 16 == 0 {
                for ch in 0..2 {
                    let ph = self.phase + ch as f32 * self.stereo * 0.5;
                    let lfo = 0.5 - 0.5 * (ph * TAU).cos();
                    self.coef[ch] = self.coefficient(self.freq * 2f32.powf(octaves * lfo));
                }
            }
            self.counter += 1;
            self.phase = (self.phase + inc).fract();
            for (ch, buf) in [&mut *left, &mut *right].into_iter().enumerate() {
                let x = buf[i];
                let mut s = x + self.last[ch] * self.feedback;
                let a = self.coef[ch];
                for k in 0..self.stages {
                    let y = a * s + self.x1[ch][k] - a * self.y1[ch][k];
                    self.x1[ch][k] = s;
                    self.y1[ch][k] = y;
                    s = y;
                }
                self.last[ch] = s;
                buf[i] = x * dry + s * wet;
            }
        }
    }
}

// --------------------------------------------------------------------- Drive

#[derive(Default)]
pub struct Drive {
    amount: f32,
    tone: [OnePole; 2],
    dc: [DcBlock; 2],
    dc_coef: f32,
    mix: f32,
    output: f32,
}

impl Effect for Drive {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        self.amount = d.param("amount") as f32;
        self.mix = d.param("mix") as f32;
        self.output = d.param("output") as f32;
        self.dc_coef = DcBlock::coef(ctx.sr);
        for t in &mut self.tone {
            t.set(d.param("tone") as f32, ctx.sr);
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let pre = 1.0 + self.amount * 24.0;
        let norm = 1.0 / pre.sqrt().max(1.0);
        let r = self.dc_coef;
        for ((buf, tone), dc) in [left, right]
            .into_iter()
            .zip(self.tone.iter_mut())
            .zip(self.dc.iter_mut())
        {
            for x in buf.iter_mut() {
                // Asymmetric curve adds even harmonics, like a tube stage;
                // the DC it creates is removed afterwards.
                let v = *x * pre;
                let sat = if v >= 0.0 {
                    v.tanh()
                } else {
                    (v * 0.8).tanh() / 0.8
                };
                let wet = tone.process(dc.process(sat * norm * 1.6, r));
                *x = mix(*x, wet, self.mix) * self.output;
            }
        }
    }
}

// ---------------------------------------------------------------- Compressor

#[derive(Default)]
pub struct Compressor {
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    env_db: f32,
    sr: f32,
    meter: GrMeter,
}

impl Effect for Compressor {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        self.sr = ctx.sr;
        self.threshold = d.param("threshold") as f32;
        self.ratio = d.param("ratio") as f32;
        self.attack = settle_coef(d.param("attack") as f32 / 1000.0, ctx.sr);
        self.release = settle_coef(d.param("release") as f32 / 1000.0, ctx.sr);
        self.makeup = db_to_gain(d.param("makeup") as f32);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let knee = 6.0;
        for i in 0..left.len() {
            let level = gain_to_db(left[i].abs().max(right[i].abs()));
            let over = level - self.threshold;
            // Soft knee gain computer.
            let gr = if over <= -knee / 2.0 {
                0.0
            } else if over >= knee / 2.0 {
                over * (1.0 - 1.0 / self.ratio)
            } else {
                let x = over + knee / 2.0;
                (1.0 - 1.0 / self.ratio) * x * x / (2.0 * knee)
            };
            let coef = if gr > self.env_db {
                self.attack
            } else {
                self.release
            };
            self.env_db = gr + (self.env_db - gr) * coef;
            if self.meter.on {
                self.meter.add(self.env_db);
            }
            let g = db_to_gain(-self.env_db) * self.makeup;
            left[i] *= g;
            right[i] *= g;
        }
    }
    fn take_gain_reduction(&mut self) -> Option<(f32, f32)> {
        self.meter.take()
    }
    fn set_metering(&mut self, on: bool) {
        self.meter.on = on;
    }
}

// ------------------------------------------------------------------- Limiter

/// A lookahead peak limiter that never lets a sample over its ceiling, nor
/// the peaks between samples a converter or an encoder reconstructs (a
/// true-peak limiter): the gain each stretch between two samples needs —
/// its two samples and the signal between them, oversampled 4× as ITU-R
/// BS.1770 measures true peak — is held over the lookahead window (a
/// running minimum), released slowly upward, and smoothed by a moving
/// average as long as the window — so the gain is already down when a peak
/// leaves the delay line, without clipping it.
pub struct Limiter {
    sr: f32,
    /// The window (samples); the audio is delayed by `len - 1 + TP_LAG` (a
    /// stretch's need is known `TP_LAG` samples after its first sample).
    len: usize,
    buf: [Vec<f32>; 2],
    w: usize,
    /// The oversampling interpolator's phases and each channel's last
    /// samples (a ring).
    tp: [[f32; TP_TAPS]; TP_PHASES],
    hist: [[f32; TP_TAPS]; 2],
    at: usize,
    /// The running minimum of the needed gain: (sample index, gain),
    /// increasing gains.
    held: std::collections::VecDeque<(u64, f32)>,
    t: u64,
    /// The held gain, released upward.
    rel: f32,
    /// The last `len` released gains and their sum (the moving average).
    avg: Vec<f32>,
    sum: f64,
    a: usize,
    input: f32,
    ceiling: f32,
    release: f32,
    meter: GrMeter,
}

impl Limiter {
    fn new(sr: f32) -> Limiter {
        let len = ((sr * 0.0015) as usize).max(1) + 1;
        Limiter {
            sr,
            len,
            buf: [vec![0.0; len + TP_LAG], vec![0.0; len + TP_LAG]],
            w: 0,
            tp: tp_phases(),
            hist: [[0.0; TP_TAPS]; 2],
            at: 0,
            held: std::collections::VecDeque::with_capacity(len + 1),
            t: 0,
            rel: 1.0,
            avg: vec![1.0; len],
            sum: len as f64,
            a: 0,
            input: 1.0,
            ceiling: 0.966,
            release: 0.999,
            meter: GrMeter::default(),
        }
    }
}

const TP_PHASES: usize = 4;
const TP_TAPS: usize = 12;
/// How many samples back the oversampled stretch lies (the interpolator's
/// delay, rounded up): the stretch from that sample to the next.
const TP_LAG: usize = 6;

/// The 4× oversampling interpolator's phases (48 taps, a windowed sinc as
/// ITU-R BS.1770 Annex 2 describes; each phase passes DC at unity).
fn tp_phases() -> [[f32; TP_TAPS]; TP_PHASES] {
    let n = TP_PHASES * TP_TAPS;
    let mid = (n as f64 - 1.0) / 2.0;
    let mut h = [[0f32; TP_TAPS]; TP_PHASES];
    for i in 0..n {
        let t = (i as f64 - mid) / TP_PHASES as f64;
        let pi = std::f64::consts::PI;
        let sinc = if t.abs() < 1e-12 {
            1.0
        } else {
            (pi * t).sin() / (pi * t)
        };
        let x = 2.0 * pi * (i as f64 + 0.5) / n as f64;
        let w = 0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos();
        h[i % TP_PHASES][i / TP_PHASES] = (sinc * w) as f32;
    }
    for ph in &mut h {
        let s: f32 = ph.iter().sum();
        ph.iter_mut().for_each(|x| *x /= s);
    }
    h
}

impl Effect for Limiter {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.input = db_to_gain(d.param("gain") as f32);
        self.ceiling = db_to_gain(d.param("ceiling") as f32);
        self.release = settle_coef(d.param("release") as f32 / 1000.0, self.sr);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = self.len;
        let c = self.ceiling;
        for i in 0..left.len() {
            let (xl, xr) = (left[i] * self.input, right[i] * self.input);
            // The stretch `TP_LAG` samples back: its two samples, and the
            // signal between them oversampled.
            self.hist[0][self.at] = xl;
            self.hist[1][self.at] = xr;
            self.at = (self.at + 1) % TP_TAPS;
            let back = |k: usize| (self.at + TP_TAPS - 1 - k) % TP_TAPS;
            let mut peak = 0f32;
            for ch in &self.hist {
                peak = peak
                    .max(ch[back(TP_LAG)].abs())
                    .max(ch[back(TP_LAG - 1)].abs());
                for ph in &self.tp {
                    let mut acc = 0.0;
                    for (j, k) in ph.iter().enumerate() {
                        acc += k * ch[back(j)];
                    }
                    peak = peak.max(acc.abs());
                }
            }
            let need = if peak > c { c / peak } else { 1.0 };
            // The least gain any sample in the window needs.
            while self.held.back().is_some_and(|h| h.1 >= need) {
                self.held.pop_back();
            }
            self.held.push_back((self.t, need));
            while self.held.front().is_some_and(|h| h.0 + n as u64 <= self.t) {
                self.held.pop_front();
            }
            self.t += 1;
            let held = self.held.front().map(|h| h.1).unwrap_or(1.0);
            // Down at once, up at the release's pace: never above `held`.
            self.rel = if held < self.rel {
                held
            } else {
                held + (self.rel - held) * self.release
            };
            // The moving average over the window: every gain in it is at
            // most the one the delayed peak needs, so the average is too.
            self.sum += (self.rel - self.avg[self.a]) as f64;
            self.avg[self.a] = self.rel;
            self.a = (self.a + 1) % n;
            let g = (self.sum / n as f64) as f32;
            self.buf[0][self.w] = xl;
            self.buf[1][self.w] = xr;
            self.w = (self.w + 1) % (n + TP_LAG);
            let (ol, or) = (self.buf[0][self.w] * g, self.buf[1][self.w] * g);
            // Rounding aside, nothing reaches the clamp; what does is
            // metered with the rest.
            let over = ol.abs().max(or.abs());
            let applied = if over > c { g * c / over } else { g };
            left[i] = ol.clamp(-c, c);
            right[i] = or.clamp(-c, c);
            if self.meter.on {
                self.meter.add(if applied < 1.0 {
                    -gain_to_db(applied)
                } else {
                    0.0
                });
            }
        }
    }
    fn take_gain_reduction(&mut self) -> Option<(f32, f32)> {
        self.meter.take()
    }
    fn set_metering(&mut self, on: bool) {
        self.meter.on = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_holds_every_sample_to_its_ceiling_and_meters_it() {
        let sr = 48000.0;
        let mut lim = Limiter::new(sr);
        lim.ceiling = db_to_gain(-1.0);
        lim.input = db_to_gain(6.0);
        lim.release = settle_coef(0.08, sr);
        lim.set_metering(true);
        // Noise bursts and a lone one-sample spike.
        let mut seed = 1u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        };
        let n = 48000;
        let mut l: Vec<f32> = (0..n)
            .map(|i| {
                if (i / 2400) % 4 == 0 {
                    rnd() * 0.9
                } else {
                    rnd() * 0.05
                }
            })
            .collect();
        l[30000] = 1.0;
        let mut r = l.clone();
        let mut peak = 0f32;
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            lim.process(a, b);
            peak = a.iter().chain(b.iter()).fold(peak, |p, x| p.max(x.abs()));
        }
        let c = db_to_gain(-1.0);
        assert!(peak <= c + 1e-6, "peak {peak} over the ceiling {c}");
        // The bursts come in at about +5 dB over the ceiling: the meter
        // reads that much, not the smoothed fraction of it.
        let (max, _) = lim.take_gain_reduction().unwrap();
        assert!(max > 5.0, "{max}");
    }

    /// The peak between samples (8× windowed-sinc reconstruction).
    fn true_peak(x: &[f32]) -> f32 {
        let taps = 32i64;
        let mut peak = 0f32;
        for n in taps as usize..x.len() - taps as usize {
            for k in 0..8 {
                let t = n as f64 + k as f64 / 8.0;
                let mut v = 0f64;
                for m in (t as i64 - taps)..=(t as i64 + taps) {
                    let d = t - m as f64;
                    let sinc = if d.abs() < 1e-9 {
                        1.0
                    } else {
                        (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d)
                    };
                    let w = 0.5 + 0.5 * (std::f64::consts::PI * d / (taps as f64 + 1.0)).cos();
                    v += x[m as usize] as f64 * sinc * w;
                }
                peak = peak.max(v.abs() as f32);
            }
        }
        peak
    }

    #[test]
    fn the_limiter_holds_the_peaks_between_samples_too() {
        let sr = 48000.0;
        let mut lim = Limiter::new(sr);
        lim.ceiling = db_to_gain(-1.0);
        lim.input = db_to_gain(6.0);
        lim.release = settle_coef(0.08, sr);
        // 6 kHz with its peaks between the samples: the samples alone read
        // 0.7 dB under the true peak.
        let n = 9600;
        let f = 6000.0 / sr as f64;
        let mut l: Vec<f32> = (0..n)
            .map(|i| {
                (0.9 * (2.0 * std::f64::consts::PI * f * i as f64 + std::f64::consts::PI / 8.0)
                    .sin()) as f32
            })
            .collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            lim.process(a, b);
        }
        let tp = gain_to_db(true_peak(&l[2400..]));
        assert!(tp <= -0.8, "true peak {tp:.2} dB over the -1 dB ceiling");
    }
}
