//! Gilded Risers & Impacts (`transition`): cinematic transition effects.
//!
//! A note-on fires a one-shot effect lasting `length` beats (note-offs are
//! ignored): riser, downlifter, impact, sweep or sub drop. The `intensity`
//! macro scales pitch travel, filter sweep range, drive and space together.
//! A small feedback-delay-network diffuser gives the effects their tail.

use super::{Instrument, NoteKind};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;
use std::f32::consts::PI;

const SHOTS: usize = 6;
const CONTROL: usize = 16;
const CHUNK: usize = 128;
const OUT_SCALE: f32 = 0.62;
/// Level of the summed shots going into the drive stage.
const MIX: f32 = 0.38;

/// Band-limited saw sample, then advance the phase.
#[inline]
fn saw(ph: &mut f32, dt: f32) -> f32 {
    let s = blep_saw(*ph, dt);
    advance(ph, dt);
    s
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    Riser,
    Downlifter,
    Impact,
    Sweep,
    Subdrop,
}

impl Kind {
    fn parse(s: &str) -> Kind {
        match s {
            "downlifter" => Kind::Downlifter,
            "impact" => Kind::Impact,
            "sweep" => Kind::Sweep,
            "subdrop" => Kind::Subdrop,
            _ => Kind::Riser,
        }
    }
}

#[derive(Clone, Copy)]
struct Params {
    beats: f32,
    intensity: f32,
    pitch: f32,
    noise: f32,
    tone: f32,
    space: f32,
    drive: f32,
    gain: f32,
    kind: Kind,
}

impl Params {
    fn travel(&self) -> f32 {
        self.pitch * (0.4 + self.intensity)
    }
    fn sweep_oct(&self) -> f32 {
        2.0 + 5.0 * self.intensity
    }
    fn bright(&self) -> f32 {
        0.35 + 1.3 * self.tone
    }
    fn space_eff(&self) -> f32 {
        (self.space * (0.5 + 0.8 * self.intensity)).clamp(0.0, 1.0)
    }
    fn drive_eff(&self) -> f32 {
        self.drive * (0.5 + self.intensity)
    }
}

#[derive(Clone)]
struct Shot {
    active: bool,
    kind: Kind,
    age: u64,
    /// Progress 0..1 through the effect.
    t: f64,
    /// Seconds since the trigger.
    secs: f32,
    base: f32,
    vel: f32,
    ph: [f32; 7],
    dt: [f32; 7],
    rng: Rng,
    nf_l: Svf,
    nf_r: Svf,
    tf_l: Svf,
    tf_r: Svf,
    /// Interpolated control values: amplitude, noise and tone levels, pan.
    amp: f32,
    amp_inc: f32,
    pan: (f32, f32),
    /// Separate transient layers (impact): noise burst, metal, sub.
    layers: [f32; 3],
    layers_inc: [f32; 3],
    counter: usize,
    /// Fades used for the end of the effect and for AllOff.
    end_fade: f32,
    killing: bool,
}

impl Shot {
    fn new(seed: u32) -> Shot {
        Shot {
            active: false,
            kind: Kind::Riser,
            age: 0,
            t: 0.0,
            secs: 0.0,
            base: 0.0,
            vel: 1.0,
            ph: [0.0; 7],
            dt: [0.0; 7],
            rng: Rng::new(seed),
            nf_l: Svf::default(),
            nf_r: Svf::default(),
            tf_l: Svf::default(),
            tf_r: Svf::default(),
            amp: 0.0,
            amp_inc: 0.0,
            pan: (1.0, 1.0),
            layers: [0.0; 3],
            layers_inc: [0.0; 3],
            counter: 0,
            end_fade: 1.0,
            killing: false,
        }
    }
}

struct Delay {
    buf: Vec<f32>,
    pos: usize,
}

impl Delay {
    fn new(len: usize) -> Delay {
        Delay {
            buf: vec![0.0; len.max(1)],
            pos: 0,
        }
    }
    #[inline]
    fn read(&self) -> f32 {
        self.buf[self.pos]
    }
    #[inline]
    fn write(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
    }
    #[inline]
    fn allpass(&mut self, x: f32, g: f32) -> f32 {
        let d = self.read();
        let v = x + g * d;
        self.write(v);
        d - g * v
    }
    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

/// Small stereo diffuser: two allpasses per side into a 4-line FDN.
struct Space {
    ap: [Delay; 4],
    lines: [Delay; 4],
    damp: [OnePole; 4],
    gains: [f32; 4],
    quiet: usize,
}

impl Space {
    fn new(sr: f32) -> Space {
        let ms = |m: f32| ((m * 0.001 * sr) as usize).max(1);
        Space {
            ap: [
                Delay::new(ms(2.96)),
                Delay::new(ms(2.23)),
                Delay::new(ms(3.19)),
                Delay::new(ms(2.35)),
            ],
            lines: [
                Delay::new(ms(41.3)),
                Delay::new(ms(47.9)),
                Delay::new(ms(55.1)),
                Delay::new(ms(63.7)),
            ],
            damp: Default::default(),
            gains: [0.0; 4],
            quiet: usize::MAX / 2,
        }
    }

    fn set(&mut self, rt60: f32, damp_hz: f32, sr: f32) {
        for i in 0..4 {
            let len = self.lines[i].buf.len() as f32;
            self.gains[i] = 10f32.powf(-3.0 * len / (rt60.max(0.05) * sr));
            self.damp[i].set(damp_hz, sr);
        }
    }

    #[inline]
    fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let dl = self.ap[0].allpass(l, 0.65);
        let dl = self.ap[1].allpass(dl, 0.6);
        let dr = self.ap[2].allpass(r, 0.65);
        let dr = self.ap[3].allpass(dr, 0.6);
        let o = [
            self.lines[0].read(),
            self.lines[1].read(),
            self.lines[2].read(),
            self.lines[3].read(),
        ];
        let half = 0.5 * (o[0] + o[1] + o[2] + o[3]);
        let inp = [dl, dr, -dl, dr];
        for i in 0..4 {
            let fb = self.damp[i].process((o[i] - half) * self.gains[i]);
            self.lines[i].write(fb + inp[i]);
        }
        (0.6 * (o[0] + o[2]), 0.6 * (o[1] + o[3]))
    }

    fn clear(&mut self) {
        for d in self.ap.iter_mut().chain(self.lines.iter_mut()) {
            d.clear();
        }
        for f in &mut self.damp {
            f.reset();
        }
    }
}

pub struct Transition {
    sr: f32,
    bpm: f32,
    p: Params,
    shots: Vec<Shot>,
    space: Space,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    clock: u64,
    /// Progress per sample.
    inc: f64,
}

impl Transition {
    pub fn new(sr: f32) -> Transition {
        let mut c = Transition {
            sr,
            bpm: 120.0,
            p: Params {
                beats: 8.0,
                intensity: 0.6,
                pitch: 24.0,
                noise: 0.6,
                tone: 0.5,
                space: 0.5,
                drive: 0.2,
                gain: 0.7,
                kind: Kind::Riser,
            },
            shots: (0..SHOTS)
                .map(|i| Shot::new(0xC0E7E + i as u32 * 7919))
                .collect(),
            space: Space::new(sr),
            buf_l: vec![0.0; CHUNK],
            buf_r: vec![0.0; CHUNK],
            clock: 0,
            inc: 0.0,
        };
        c.update();
        c
    }

    fn update(&mut self) {
        let secs = self.p.beats as f64 * 60.0 / self.bpm.max(1.0) as f64;
        self.inc = 1.0 / (secs * self.sr as f64).max(1.0);
        let se = self.p.space_eff();
        let rt = 0.4
            + 5.0 * se
            + if self.p.kind == Kind::Impact {
                1.5 * se
            } else {
                0.0
            };
        self.space.set(rt, 2500.0 + 6000.0 * self.p.tone, self.sr);
    }

    /// Control-rate update of a shot: filters, pitches, target levels.
    fn control(s: &mut Shot, p: &Params, sr: f32, inc: f64) {
        let t = (s.t as f32).min(1.0);
        let travel = p.travel();
        let sweep = p.sweep_oct();
        let bright = p.bright();
        let base = s.base;
        let nyq = 0.45 * sr;
        let span = CONTROL as f32;
        let dur = (1.0 / (inc * sr as f64)) as f32;
        let mut target = [0.0f32; 3];
        let amp;
        match s.kind {
            Kind::Riser | Kind::Downlifter => {
                let (pitch_shape, open, level) = if s.kind == Kind::Riser {
                    (t.powf(1.6), t.powf(1.3), 0.06 + 0.94 * t * t)
                } else {
                    let u = 1.0 - t;
                    (u * u, u.powf(1.5), u.powf(1.6))
                };
                let semis = base + 48.0 + travel * pitch_shape;
                let f = midi_to_hz(semis);
                let det = cents(8.0 * (0.4 + p.intensity));
                s.dt[0] = (f / det / sr).min(0.45);
                s.dt[1] = (f / sr).min(0.45);
                s.dt[2] = (f * det / sr).min(0.45);
                s.dt[3] = (f * 0.5 / sr).min(0.45);
                let nf = (220.0 * 2f32.powf(sweep * open) * bright).clamp(40.0, nyq);
                let res = 0.2 + 0.35 * p.intensity;
                s.nf_l.set(nf, res, sr);
                s.nf_r.set(nf * 1.03, res, sr);
                let tf =
                    (f * 2.0 + 300.0 * 2f32.powf(sweep * 0.8 * open) * bright).clamp(40.0, nyq);
                s.tf_l.set(tf, 0.15, sr);
                s.tf_r.set(tf, 0.15, sr);
                amp = level;
            }
            Kind::Sweep => {
                let w = (PI * t).sin().max(0.0);
                let nf = (150.0 * 2f32.powf(sweep * w) * bright).clamp(40.0, nyq);
                let res = 0.35 + 0.35 * p.intensity;
                s.nf_l.set(nf, res, sr);
                s.nf_r.set(nf * 1.05, res, sr);
                s.tf_l.set(nf, res, sr);
                s.tf_r.set(nf * 1.05, res, sr);
                let f = midi_to_hz(base + 36.0 + travel * 0.25 * w);
                s.dt[0] = (f / sr).min(0.45);
                s.dt[1] = (f * cents(7.0) / sr).min(0.45);
                s.dt[2] = (f * cents(-7.0) / sr).min(0.45);
                s.pan = pan_gains(0.6 * (2.0 * PI * t).sin() * (0.5 + 0.5 * p.intensity));
                amp = w.powf(0.8);
            }
            Kind::Subdrop => {
                let shape = 1.0 - (1.0 - t).powf(2.5);
                let f = midi_to_hz(base + 43.0 - travel * shape);
                s.dt[0] = (f / sr).min(0.45);
                let nf = (80.0 + 400.0 * (1.0 - t) * bright).clamp(40.0, nyq);
                s.nf_l.set(nf, 0.1, sr);
                s.nf_r.set(nf, 0.1, sr);
                amp = (1.0 - t).powf(0.8);
            }
            Kind::Impact => {
                let secs = s.secs;
                // Long tail reaching silence exactly at the end of the effect.
                let tail = (-3.5 * t).exp() * (1.0 - t);
                let f_sub = midi_to_hz(base + 31.0 + 24.0 * p.intensity * (-secs / 0.035).exp());
                s.dt[0] = (f_sub / sr).min(0.45);
                let fm = midi_to_hz(base + 57.0);
                for (j, r) in [1.0f32, 1.593, 2.135, 2.83, 3.61].iter().enumerate() {
                    s.dt[1 + j] = (fm * r / sr).min(0.45);
                }
                let nf = (200.0
                    + (1500.0 + 9000.0 * p.tone)
                        * (-secs / 0.35).exp()
                        * (0.5 + 0.5 * p.intensity))
                    .clamp(40.0, nyq);
                s.nf_l.set(nf, 0.1, sr);
                s.nf_r.set(nf * 1.07, 0.1, sr);
                let boom = (0.3 + 0.15 * dur.min(16.0)).min(3.0);
                target = [
                    (-secs / 0.09).exp() + 0.45 * tail,
                    (-secs / 0.3).exp() * (0.3 + 0.7 * p.tone),
                    (-secs / boom).exp() * tail.sqrt(),
                ];
                amp = tail.sqrt();
            }
        }
        // Attack ramp for click-free starts.
        let attack = match s.kind {
            Kind::Impact => 0.0015,
            _ => 0.01,
        };
        let ramp = (s.secs / attack).min(1.0);
        let amp = amp * ramp;
        s.amp_inc = (amp - s.amp) / span;
        for ((inc, layer), tj) in s.layers_inc.iter_mut().zip(&s.layers).zip(target) {
            *inc = (tj * ramp - layer) / span;
        }
    }
}

impl Instrument for Transition {
    fn set_device(&mut self, d: &Device, ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        self.p = Params {
            beats: f("length").clamp(0.05, 64.0),
            intensity: f("intensity").clamp(0.0, 1.0),
            pitch: f("pitch").clamp(0.0, 48.0),
            noise: f("noise").clamp(0.0, 1.0),
            tone: f("tone").clamp(0.0, 1.0),
            space: f("space").clamp(0.0, 1.0),
            drive: f("drive").clamp(0.0, 1.0),
            gain: f("gain").max(0.0),
            kind: Kind::parse(d.option("kind")),
        };
        self.bpm = ctx.bpm;
        self.update();
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let i = self
                    .shots
                    .iter()
                    .position(|s| !s.active)
                    .unwrap_or_else(|| {
                        let mut best = 0;
                        for (j, s) in self.shots.iter().enumerate() {
                            if s.age < self.shots[best].age {
                                best = j;
                            }
                        }
                        best
                    });
                if self.space.quiet > self.sr as usize && self.shots.iter().all(|s| !s.active) {
                    // The tail has been skipped while idle: start from silence.
                    self.space.clear();
                }
                self.space.quiet = 0;
                let s = &mut self.shots[i];
                let seed = s.rng.next_u32();
                *s = Shot::new(seed);
                s.active = true;
                s.kind = self.p.kind;
                s.age = self.clock;
                s.base = key as f32 - 60.0;
                s.vel = velocity.clamp(0.0, 1.0);
                for j in 0..7 {
                    s.ph[j] = s.rng.unit();
                }
                s.ph[0] = 0.0; // sub / sine layers start at zero crossing
            }
            NoteKind::Off { .. } => {}
            NoteKind::AllOff => {
                for s in self.shots.iter_mut().filter(|s| s.active) {
                    s.killing = true;
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut done = 0;
        while done < n {
            let len = (n - done).min(CHUNK);
            self.render_chunk(&mut left[done..done + len], &mut right[done..done + len]);
            done += len;
        }
    }
}

impl Transition {
    fn render_chunk(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let any = self.shots.iter().any(|s| s.active);
        if !any && self.space.quiet > self.sr as usize {
            return;
        }
        let p = self.p;
        let sr = self.sr;
        let inc = self.inc;
        let inv_sr = 1.0 / sr;
        let bl = &mut self.buf_l[..n];
        let br = &mut self.buf_r[..n];
        bl.fill(0.0);
        br.fill(0.0);
        let nm = p.noise * PI * 0.5;
        let (tone_mix, noise_mix) = (nm.cos(), nm.sin());
        let end_step = 1.0 / (0.012 * sr);
        let kill_coef = settle_coef(0.25, sr);
        let drv = p.drive_eff();
        let pre = 1.0 + 6.0 * drv;
        let post = 1.0 / ftanh(pre);
        let out_gain = p.gain * OUT_SCALE;

        for s in self.shots.iter_mut().filter(|s| s.active) {
            let vel = 0.35 + 0.65 * s.vel;
            for i in 0..n {
                if s.counter.is_multiple_of(CONTROL) {
                    Transition::control(s, &p, sr, inc);
                }
                s.counter += 1;
                s.amp += s.amp_inc;
                for j in 0..3 {
                    s.layers[j] += s.layers_inc[j];
                }
                let (nl, nr) = (s.rng.bipolar(), s.rng.bipolar());
                let (l, r) = match s.kind {
                    Kind::Riser | Kind::Downlifter => {
                        let a = saw(&mut s.ph[0], s.dt[0]);
                        let b = saw(&mut s.ph[1], s.dt[1]);
                        let c = saw(&mut s.ph[2], s.dt[2]);
                        let sub = fsin(s.ph[3]) * 0.5;
                        advance(&mut s.ph[3], s.dt[3]);
                        let tl = s.tf_l.process(a + 0.7 * b, FilterMode::Lowpass) * 0.5 + sub;
                        let tr = s.tf_r.process(c + 0.7 * b, FilterMode::Lowpass) * 0.5 + sub;
                        let zl = s.nf_l.process(nl, FilterMode::Lowpass);
                        let zr = s.nf_r.process(nr, FilterMode::Lowpass);
                        (
                            (tl * tone_mix + zl * noise_mix) * s.amp,
                            (tr * tone_mix + zr * noise_mix) * s.amp,
                        )
                    }
                    Kind::Sweep => {
                        let a = saw(&mut s.ph[0], s.dt[0]);
                        let b = saw(&mut s.ph[1], s.dt[1]);
                        let c = saw(&mut s.ph[2], s.dt[2]);
                        let tl = s.tf_l.process(a + b, FilterMode::Lowpass) * 0.45;
                        let tr = s.tf_r.process(a + c, FilterMode::Lowpass) * 0.45;
                        let zl = s.nf_l.process(nl, FilterMode::Lowpass) * 1.2;
                        let zr = s.nf_r.process(nr, FilterMode::Lowpass) * 1.2;
                        let (pl, pr) = s.pan;
                        (
                            (tl * tone_mix + zl * noise_mix) * s.amp * pl,
                            (tr * tone_mix + zr * noise_mix) * s.amp * pr,
                        )
                    }
                    Kind::Subdrop => {
                        let x = fsin(s.ph[0]);
                        advance(&mut s.ph[0], s.dt[0]);
                        let h = x * x * (0.2 + 0.4 * p.tone) - 0.15;
                        let body = x + h;
                        let nmx = p.noise * 0.35;
                        let zl = s.nf_l.process(nl, FilterMode::Lowpass) * 2.0;
                        let zr = s.nf_r.process(nr, FilterMode::Lowpass) * 2.0;
                        (
                            ((1.0 - nmx) * body + nmx * zl) * s.amp,
                            ((1.0 - nmx) * body + nmx * zr) * s.amp,
                        )
                    }
                    Kind::Impact => {
                        let sub = fsin(s.ph[0]) * s.layers[2];
                        advance(&mut s.ph[0], s.dt[0]);
                        let mut metal = 0.0;
                        for j in 1..6 {
                            metal += fsin(s.ph[j]) * (1.0 / j as f32);
                            advance(&mut s.ph[j], s.dt[j]);
                        }
                        let metal = metal * 0.3 * s.layers[1];
                        let zl = s.nf_l.process(nl, FilterMode::Lowpass) * s.layers[0] * 1.3;
                        let zr = s.nf_r.process(nr, FilterMode::Lowpass) * s.layers[0] * 1.3;
                        let tone = sub * 1.1 + metal;
                        let tm = 0.4 + 0.6 * tone_mix;
                        let zm = 0.3 + 0.7 * noise_mix;
                        (
                            (tone * tm + zl * zm) * s.amp,
                            (tone * tm - metal * 0.3 * tm + zr * zm) * s.amp,
                        )
                    }
                };
                // End-of-effect fade and AllOff fade.
                let mut fade = s.end_fade;
                if s.t >= 1.0 {
                    fade -= end_step;
                    s.end_fade = fade.max(0.0);
                }
                if s.killing {
                    s.end_fade *= kill_coef;
                }
                let g = s.end_fade.max(0.0) * vel * MIX;
                bl[i] += l * g;
                br[i] += r * g;
                s.t += inc;
                s.secs += inv_sr;
            }
            if s.end_fade <= 1e-4 {
                s.active = false;
            }
        }

        // Drive, space and output.
        let se = p.space_eff();
        let wet = se * 0.4;
        let dry = 1.0 - 0.3 * se;
        let mut peak = 0.0f32;
        for i in 0..n {
            let (mut l, mut r) = (bl[i], br[i]);
            if drv > 0.001 {
                l = ftanh(l * pre) * post;
                r = ftanh(r * pre) * post;
            }
            let (wl, wr) = self.space.process(l, r);
            let ol = (l * dry + wl * wet) * out_gain;
            let or = (r * dry + wr * wet) * out_gain;
            peak = peak.max(ol.abs()).max(or.abs());
            left[i] += ol;
            right[i] += or;
        }
        if any || peak > 1e-5 {
            self.space.quiet = 0;
        } else {
            self.space.quiet += n;
        }
    }
}
