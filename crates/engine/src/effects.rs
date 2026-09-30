//! Built-in insert effects.

use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;

pub trait Effect: Send {
    fn set_device(&mut self, dev: &Device, ctx: &Ctx);
    /// Process a stereo block in place.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    fn reset(&mut self) {}
}

pub fn create(dev: &Device, ctx: &Ctx) -> Option<Box<dyn Effect>> {
    let sr = ctx.sr;
    let mut fx: Box<dyn Effect> = match dev.kind.as_str() {
        "eq" => Box::new(Eq3::default()),
        "filter" => Box::new(Filter::default()),
        "delay" => Box::new(Delay::new(sr)),
        "reverb" => Box::new(Reverb::new(sr)),
        "chorus" => Box::new(Chorus::new(sr)),
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
        self.buf[self.w] = x;
        self.w = (self.w + 1) % self.buf.len();
    }
    /// Read `d` samples behind the last write (linear interpolation).
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let d = d.clamp(1.0, (n - 2) as f32);
        let pos = self.w as f32 - d;
        let pos = if pos < 0.0 { pos + n as f32 } else { pos };
        let i = pos as usize;
        let f = pos - i as f32;
        let a = self.buf[i % n];
        let b = self.buf[(i + 1) % n];
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
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.i] = x + self.store * feedback;
        self.i = (self.i + 1) % self.buf.len();
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
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
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
            let g = db_to_gain(-self.env_db) * self.makeup;
            left[i] *= g;
            right[i] *= g;
        }
    }
}

// ------------------------------------------------------------------- Limiter

pub struct Limiter {
    sr: f32,
    look: usize,
    buf: [Vec<f32>; 2],
    w: usize,
    gain: f32,
    input: f32,
    ceiling: f32,
    release: f32,
    attack: f32,
}

impl Limiter {
    fn new(sr: f32) -> Limiter {
        let look = (sr * 0.0015) as usize;
        Limiter {
            sr,
            look,
            buf: [vec![0.0; look + 1], vec![0.0; look + 1]],
            w: 0,
            gain: 1.0,
            input: 1.0,
            ceiling: 0.966,
            release: 0.999,
            attack: settle_coef(0.0015 / 3.0, sr),
        }
    }
}

impl Effect for Limiter {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        self.input = db_to_gain(d.param("gain") as f32);
        self.ceiling = db_to_gain(d.param("ceiling") as f32);
        self.release = settle_coef(d.param("release") as f32 / 1000.0, self.sr);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = self.look + 1;
        for i in 0..left.len() {
            let (xl, xr) = (left[i] * self.input, right[i] * self.input);
            let peak = xl.abs().max(xr.abs());
            let target = if peak > self.ceiling {
                self.ceiling / peak
            } else {
                1.0
            };
            let coef = if target < self.gain {
                self.attack
            } else {
                self.release
            };
            self.gain = target + (self.gain - target) * coef;
            self.buf[0][self.w] = xl;
            self.buf[1][self.w] = xr;
            self.w = (self.w + 1) % n;
            let c = self.ceiling;
            left[i] = (self.buf[0][self.w] * self.gain).clamp(-c, c);
            right[i] = (self.buf[1][self.w] * self.gain).clamp(-c, c);
        }
    }
}
