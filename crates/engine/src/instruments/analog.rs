//! Bronze Bass (`analog`): virtual analog synthesizer modelled on circuit behaviour.
//!
//! Two PolyBLEP oscillators (saw / pulse / triangle / sine / noise), stacked up to
//! seven times in unison across the stereo field, a square or sine sub one
//! octave down and noise feed the filter: a 2x oversampled 4-pole
//! zero-delay-feedback ladder with tanh stage saturation, an aggressive
//! 2-pole "screamer" whose resonance loop has a nonlinear (van der Pol like)
//! damping and saturating integrators, or a clean 2-pole state-variable
//! low-, high- or band-pass. Each voice has static component tolerances and
//! a slow random drift of pitch and cutoff.
//!
//! With unison the stack is split into a left and a right sum, each through
//! its own copy of the filter (same coefficients), so the width survives.

use super::{pick_voice, Instrument, NoteKind, MAX_VOICES};
use crate::dsp::*;
use crate::Ctx;
use rosaclef_core::Device;
use std::f32::consts::PI;

/// Samples between control-rate updates (pitch, drift, envelopes -> cutoff).
const CONTROL: usize = 16;
/// Held keys remembered for mono / legato note priority.
const STACK: usize = 16;
/// Overall output scaling.
const OUT_SCALE: f32 = 0.42;
/// Glide times below this jump straight to the target.
const MIN_GLIDE: f32 = 0.0015;
/// Largest unison stack.
const MAX_UNISON: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaWave {
    Saw,
    Pulse,
    Triangle,
    Sine,
    Noise,
}

impl VaWave {
    fn parse(s: &str) -> VaWave {
        match s {
            "pulse" => VaWave::Pulse,
            "triangle" => VaWave::Triangle,
            "sine" => VaWave::Sine,
            "noise" => VaWave::Noise,
            _ => VaWave::Saw,
        }
    }
}

/// Band-limited oscillator sample (DC-free) for a phase in [0, 1).
#[inline]
fn va_osc(w: VaWave, ph: f32, dt: f32, pw: f32, rng: &mut Rng) -> f32 {
    match w {
        VaWave::Saw => blep_saw(ph, dt),
        VaWave::Pulse => {
            let mut p2 = ph - pw;
            if p2 < 0.0 {
                p2 += 1.0;
            }
            let naive = if ph < pw { 1.0 } else { -1.0 };
            naive + poly_blep(ph, dt) - poly_blep(p2, dt) - (2.0 * pw - 1.0)
        }
        VaWave::Triangle => 1.0 - 4.0 * (ph - 0.5).abs(),
        VaWave::Sine => (ph * TAU).sin(),
        VaWave::Noise => rng.bipolar(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
    Ladder,
    Screamer,
    /// Clean 2-pole state-variable filter in the given mode.
    Svf(FilterMode),
}

impl Model {
    fn parse(s: &str) -> Model {
        match s {
            "screamer" => Model::Screamer,
            "lowpass" | "highpass" | "bandpass" => Model::Svf(FilterMode::parse(s)),
            _ => Model::Ladder,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Voicing {
    Poly,
    Mono,
    Legato,
}

#[derive(Clone, Copy)]
struct Params {
    wave1: VaWave,
    wave2: VaWave,
    model: Model,
    voicing: Voicing,
    osc2_ratio: f32,
    mix2: f32,
    pw: f32,
    unison: usize,
    spread: f32,
    sub: f32,
    sub_sine: bool,
    noise: f32,
    drift: f32,
    cutoff: f32,
    resonance: f32,
    drive: f32,
    key_track: f32,
    filter_env: f32,
    glide: f32,
    gain: f32,
}

#[derive(Clone)]
struct Voice {
    active: bool,
    key: u8,
    vel: f32,
    /// Velocity gain as applied, gliding to (0.35 + 0.65 * vel) so that a
    /// retrigger at a new velocity doesn't step the output mid-waveform.
    vgain: f32,
    age: u64,
    /// Current (gliding) pitch in semitones and its target.
    note: f32,
    target: f32,
    /// Oscillator phases, one per unison copy (copy 0 without unison).
    ph1: [f32; MAX_UNISON],
    ph2: [f32; MAX_UNISON],
    phs: f32,
    dt1: f32,
    dt2: f32,
    env: Adsr,
    /// Filter envelope, run at control rate.
    fenv: Adsr,
    rng: Rng,
    /// Static tolerances in -1..1: osc1 pitch, osc2 pitch, cutoff, pan, resonance.
    tol: [f32; 5],
    drift: [f32; 3],
    drift_tgt: [f32; 3],
    drift_timer: i32,
    /// Filter states: left (or mono) and right (unison only).
    flt: [Flt; 2],
    g: f32,
    g_inc: f32,
    k: f32,
    counter: usize,
    fresh: bool,
    /// Filter currently running 2x oversampled.
    os: bool,
}

impl Voice {
    fn new(slot: usize) -> Voice {
        let mut rng = Rng::new(0xC0FFEE ^ (slot as u32 + 1).wrapping_mul(0x9E37_79B9));
        let tol = [
            rng.bipolar(),
            rng.bipolar(),
            rng.bipolar(),
            rng.bipolar(),
            rng.bipolar(),
        ];
        Voice {
            active: false,
            key: 0,
            vel: 0.0,
            vgain: 0.0,
            age: 0,
            note: 60.0,
            target: 60.0,
            ph1: [0.0; MAX_UNISON],
            ph2: [0.0; MAX_UNISON],
            phs: 0.0,
            dt1: 0.0,
            dt2: 0.0,
            env: Adsr::default(),
            fenv: Adsr::default(),
            rng,
            tol,
            drift: [0.0; 3],
            drift_tgt: [0.0; 3],
            drift_timer: 0,
            flt: Default::default(),
            g: 0.0,
            g_inc: 0.0,
            k: 0.0,
            counter: 0,
            fresh: true,
            os: false,
        }
    }

    /// Clear the audio state of a silent voice before it starts.
    fn reset(&mut self) {
        self.flt = Default::default();
        for u in 0..MAX_UNISON {
            self.ph1[u] = self.rng.unit();
            self.ph2[u] = self.rng.unit();
        }
        self.phs = 0.0;
        self.fresh = true;
        self.vgain = -1.0;
        self.env.level = 0.0;
        self.fenv.level = 0.0;
        // Start the drift somewhere on its path rather than always at zero.
        for j in 0..3 {
            self.drift[j] = self.rng.bipolar() * 0.5;
        }
    }

    fn is_sounding(&self) -> bool {
        self.active && !self.env.is_idle()
    }
}

/// The audio state of one filter path.
#[derive(Clone, Default)]
struct Flt {
    /// Ladder: 4 stages; screamer: ic1, ic2.
    s: [f32; 4],
    bp: f32,
    prev_in: f32,
    svf: Svf,
    dc: DcBlock,
}

/// Per-sample coefficients of the ladder.
#[derive(Clone, Copy)]
struct LadderCoefs {
    /// g / (1 + g): one-pole TPT gain.
    gg: f32,
    g2: f32,
    g3: f32,
    g4: f32,
    b: f32,
    k: f32,
    /// 1 / (1 + k * g^4).
    inv: f32,
}

impl LadderCoefs {
    #[inline]
    fn new(g: f32, k: f32) -> LadderCoefs {
        let gg = g / (1.0 + g);
        let g2 = gg * gg;
        let g4 = g2 * g2;
        LadderCoefs {
            gg,
            g2,
            g3: g2 * gg,
            g4,
            b: 1.0 - gg,
            k,
            inv: 1.0 / (1.0 + k * g4),
        }
    }
}

/// One step of the nonlinear zero-delay-feedback ladder (4 TPT one-poles
/// with tanh stage saturation).
#[inline]
fn ladder(s: &mut [f32; 4], x: f32, c: &LadderCoefs) -> f32 {
    // Linear estimate of the output solves the zero-delay feedback loop.
    let sigma = c.b * (c.g3 * s[0] + c.g2 * s[1] + c.gg * s[2] + s[3]);
    let y4 = (c.g4 * x + sigma) * c.inv;
    let mut xi = x - c.k * y4;
    for st in s.iter_mut() {
        let v = c.gg * (ftanh(xi) - ftanh(*st));
        let y = *st + v;
        *st = y + v;
        xi = y;
    }
    xi
}

/// One oversampled step of the screamer (nonlinear 2-pole TPT SVF).
#[inline]
fn screamer(s: &mut [f32; 4], bp: &mut f32, x: f32, g: f32, k: f32) -> f32 {
    // Cubic damping bounds the self-oscillation (a driven resonance "screams"
    // into the saturating integrators rather than blowing up).
    let ke = (k + 0.35 * *bp * *bp).min(4.0);
    let a1 = 1.0 / (1.0 + g * (g + ke));
    let a2 = g * a1;
    let a3 = g * a2;
    let v3 = x - s[1];
    let v1 = a1 * s[0] + a2 * v3;
    let v2 = s[1] + a2 * s[0] + a3 * v3;
    s[0] = 1.8 * ftanh((2.0 * v1 - s[0]) * (1.0 / 1.8));
    s[1] = 2.0 * v2 - s[1];
    *bp = v1;
    v2
}

pub struct Analog {
    sr: f32,
    p: Params,
    voices: Vec<Voice>,
    clock: u64,
    last_note: Option<f32>,
    stack: [u8; STACK],
    held: usize,
    amp: (f32, f32, f32, f32),
    fenv_times: (f32, f32, f32),
}

impl Analog {
    pub fn new(sr: f32) -> Analog {
        Analog {
            sr,
            p: Params {
                wave1: VaWave::Saw,
                wave2: VaWave::Saw,
                model: Model::Ladder,
                voicing: Voicing::Poly,
                osc2_ratio: 1.0,
                mix2: 0.5,
                pw: 0.5,
                unison: 1,
                spread: 12.0,
                sub: 0.3,
                sub_sine: false,
                noise: 0.0,
                drift: 0.3,
                cutoff: 1200.0,
                resonance: 0.3,
                drive: 0.3,
                key_track: 0.5,
                filter_env: 0.4,
                glide: 0.0,
                gain: 0.6,
            },
            voices: (0..MAX_VOICES).map(Voice::new).collect(),
            clock: 0,
            last_note: None,
            stack: [0; STACK],
            held: 0,
            amp: (0.005, 0.3, 0.7, 0.25),
            fenv_times: (0.002, 0.3, 0.2),
        }
    }

    fn stack_remove(&mut self, key: u8) {
        if let Some(pos) = self.stack[..self.held].iter().position(|&k| k == key) {
            self.stack.copy_within(pos + 1..self.held, pos);
            self.held -= 1;
        }
    }

    fn stack_push(&mut self, key: u8) {
        self.stack_remove(key);
        if self.held == STACK {
            self.stack.copy_within(1..STACK, 0);
            self.held -= 1;
        }
        self.stack[self.held] = key;
        self.held += 1;
    }

    fn trigger(v: &mut Voice) {
        v.active = true;
        v.counter = 0;
        v.env.trigger();
        v.fenv.trigger();
    }

    fn poly_on(&mut self, key: u8, vel: f32) {
        let i = pick_voice(
            &self.voices,
            |v| v.active,
            |v| v.env.is_released(),
            |v| v.age,
        );
        let glide = self.p.glide > MIN_GLIDE;
        let from = if glide { self.last_note } else { None };
        let v = &mut self.voices[i];
        if !v.is_sounding() {
            v.reset();
        }
        v.key = key;
        v.vel = vel;
        v.age = self.clock;
        v.target = key as f32;
        v.note = from.unwrap_or(key as f32);
        Self::trigger(v);
        self.last_note = Some(key as f32);
    }

    fn mono_on(&mut self, key: u8, vel: f32) {
        let overlapping = self.held > 0;
        self.stack_push(key);
        let glide = self.p.glide > MIN_GLIDE;
        let voicing = self.p.voicing;
        let last = self.last_note;
        let v = &mut self.voices[0];
        v.age = self.clock;
        if voicing == Voicing::Legato && overlapping && v.active && !v.env.is_released() {
            // Tied note: glide to the new pitch, envelopes keep going.
            v.key = key;
            v.target = key as f32;
            if !glide {
                v.note = key as f32;
                v.counter = 0;
            }
        } else {
            let sounding = v.is_sounding();
            if !sounding {
                v.reset();
            }
            v.key = key;
            v.vel = vel;
            v.target = key as f32;
            v.note = match (voicing, glide, last) {
                (Voicing::Mono, true, Some(n)) => {
                    if sounding {
                        v.note
                    } else {
                        n
                    }
                }
                _ => key as f32,
            };
            Self::trigger(v);
        }
        self.last_note = Some(key as f32);
    }

    fn mono_off(&mut self, key: u8) {
        self.stack_remove(key);
        let glide = self.p.glide > MIN_GLIDE;
        let voicing = self.p.voicing;
        let top = if self.held > 0 {
            Some(self.stack[self.held - 1])
        } else {
            None
        };
        let v = &mut self.voices[0];
        if !v.active || v.key != key || v.env.is_released() {
            return;
        }
        match top {
            Some(t) => {
                v.key = t;
                v.target = t as f32;
                if !glide {
                    v.note = t as f32;
                    v.counter = 0;
                }
                if voicing == Voicing::Mono {
                    Self::trigger(v);
                }
                self.last_note = Some(t as f32);
            }
            None => {
                v.env.release();
                v.fenv.release();
            }
        }
    }
}

impl Instrument for Analog {
    fn set_device(&mut self, d: &Device, _ctx: &Ctx) {
        let f = |k: &str| d.param(k) as f32;
        let voicing = match d.option("mode") {
            "mono" => Voicing::Mono,
            "legato" => Voicing::Legato,
            _ => Voicing::Poly,
        };
        if voicing != self.p.voicing {
            // Changing voicing mid-note: let everything release cleanly.
            self.held = 0;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                v.env.release();
                v.fenv.release();
            }
        }
        self.p = Params {
            wave1: VaWave::parse(d.option("wave1")),
            wave2: VaWave::parse(d.option("wave2")),
            model: Model::parse(d.option("filter")),
            voicing,
            osc2_ratio: 2f32.powf(f("osc2Semi").round() / 12.0) * cents(f("osc2Detune")),
            mix2: f("osc2Mix").clamp(0.0, 1.0),
            pw: f("pulseWidth").clamp(0.05, 0.95),
            unison: (f("unison").round() as usize).clamp(1, MAX_UNISON),
            spread: f("detune").clamp(0.0, 100.0),
            sub: f("sub").clamp(0.0, 1.0),
            sub_sine: d.option("subWave") == "sine",
            noise: f("noise").clamp(0.0, 1.0),
            drift: f("drift").clamp(0.0, 1.0),
            cutoff: f("cutoff").clamp(20.0, 20000.0),
            resonance: f("resonance").clamp(0.0, 1.0),
            drive: f("drive").clamp(0.0, 1.0),
            key_track: f("keyTrack").clamp(0.0, 1.0),
            filter_env: f("filterEnv").clamp(-1.0, 1.0),
            glide: f("glide").max(0.0),
            gain: f("gain").max(0.0),
        };
        self.amp = (f("attack"), f("decay"), f("sustain"), f("release"));
        self.fenv_times = (f("filterAttack"), f("filterDecay"), f("filterSustain"));
        let (a, dd, s, r) = self.amp;
        let (fa, fd, fs) = self.fenv_times;
        let ctrl_sr = self.sr / CONTROL as f32;
        for v in &mut self.voices {
            v.env.set(a, dd, s, r, self.sr);
            v.fenv.set(fa, fd, fs, r, ctrl_sr);
        }
    }

    fn handle(&mut self, ev: NoteKind) {
        match ev {
            NoteKind::On { key, velocity } => {
                self.clock += 1;
                let vel = velocity.clamp(0.0, 1.0);
                if self.p.voicing == Voicing::Poly {
                    self.poly_on(key, vel);
                } else {
                    self.mono_on(key, vel);
                }
            }
            NoteKind::Off { key } => {
                if self.p.voicing == Voicing::Poly {
                    for v in self
                        .voices
                        .iter_mut()
                        .filter(|v| v.active && v.key == key && !v.env.is_released())
                    {
                        v.env.release();
                        v.fenv.release();
                    }
                } else {
                    self.mono_off(key);
                }
            }
            NoteKind::AllOff => {
                self.held = 0;
                for v in self.voices.iter_mut().filter(|v| v.active) {
                    v.env.release();
                    v.fenv.release();
                }
            }
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = self.p;
        let sr = self.sr;
        let n = left.len().min(right.len());
        let ctrl_sr = sr / CONTROL as f32;
        let mut k = Consts {
            sr,
            ctrl_sr,
            osr: 2.0 * sr,
            glide_coef: if p.glide > MIN_GLIDE {
                settle_coef(p.glide, ctrl_sr)
            } else {
                0.0
            },
            drift_k: 1.0 - settle_coef(0.8, ctrl_sr),
            fc_max: (0.45 * sr).min(20000.0),
            os_on: 0.14 * sr,
            os_off: 0.11 * sr,
            dc_r: DcBlock::coef(sr),
            ladder_in: 0.8 + 2.6 * p.drive,
            scream_in: 0.7 + 3.5 * p.drive,
            svf_in: 1.0 + 3.0 * p.drive,
            mix1: 1.0 - 0.25 * p.mix2,
            detune: [1.0; MAX_UNISON],
            pan_l: [1.0; MAX_UNISON],
            pan_r: [1.0; MAX_UNISON],
            uni_norm: 1.0 / (p.unison as f32).sqrt(),
        };
        if p.unison > 1 {
            // Copies evenly spread over -1..1 in pitch (± spread) and pan.
            for u in 0..p.unison {
                let pos = u as f32 / (p.unison - 1) as f32 * 2.0 - 1.0;
                k.detune[u] = cents(pos * p.spread);
                let (l, r) = pan_gains(pos * 0.8);
                k.pan_l[u] = l;
                k.pan_r[u] = r;
            }
        }
        let mono = p.voicing != Voicing::Poly;
        // Velocity gain glides over ~3 ms (see Voice::vgain).
        let vg_c = settle_coef(0.003, sr);
        let out_gain = |v: &mut Voice| {
            let target = 0.35 + 0.65 * v.vel;
            if v.vgain < 0.0 {
                v.vgain = target;
            }
            let amp = p.gain * OUT_SCALE;
            // Poly voices sit slightly off-centre (component tolerance),
            // fading to the centre as drift goes to 0.
            let (pl, pr) = if mono {
                (1.0, 1.0)
            } else {
                pan_gains(v.tol[3] * 0.3 * (p.drift * 4.0).min(1.0))
            };
            (target, amp * pl, amp * pr)
        };
        if p.unison > 1 {
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let (tv, gl, gr) = out_gain(v);
                for i in 0..n {
                    v.vgain = tv + (v.vgain - tv) * vg_c;
                    let (yl, yr) = tick_wide(v, &p, &k);
                    left[i] += yl * v.vgain * gl;
                    right[i] += yr * v.vgain * gr;
                }
            }
        } else {
            // Voices are rendered in pairs: their filters are independent
            // serial chains, so the CPU can overlap them.
            let mut active = [0usize; MAX_VOICES];
            let mut count = 0;
            for (i, v) in self.voices.iter().enumerate() {
                if v.active {
                    active[count] = i;
                    count += 1;
                }
            }
            let mut j = 0;
            while j + 1 < count {
                let (lo, hi) = self.voices.split_at_mut(active[j + 1]);
                let (a, b) = (&mut lo[active[j]], &mut hi[0]);
                let (ta, al, ar) = out_gain(a);
                let (tb, bl, br) = out_gain(b);
                for i in 0..n {
                    a.vgain = ta + (a.vgain - ta) * vg_c;
                    b.vgain = tb + (b.vgain - tb) * vg_c;
                    let ya = tick(a, &p, &k) * a.vgain;
                    let yb = tick(b, &p, &k) * b.vgain;
                    left[i] += ya * al + yb * bl;
                    right[i] += ya * ar + yb * br;
                }
                j += 2;
            }
            if j < count {
                let v = &mut self.voices[active[j]];
                let (tv, gl, gr) = out_gain(v);
                for i in 0..n {
                    v.vgain = tv + (v.vgain - tv) * vg_c;
                    let y = tick(v, &p, &k) * v.vgain;
                    left[i] += y * gl;
                    right[i] += y * gr;
                }
            }
        }
        for v in self.voices.iter_mut().filter(|v| v.active) {
            if v.env.is_idle() {
                v.active = false;
            }
        }
    }
}

/// Per-block constants shared by all voices.
struct Consts {
    sr: f32,
    ctrl_sr: f32,
    osr: f32,
    glide_coef: f32,
    drift_k: f32,
    fc_max: f32,
    os_on: f32,
    os_off: f32,
    dc_r: f32,
    ladder_in: f32,
    scream_in: f32,
    svf_in: f32,
    mix1: f32,
    /// Unison copies: pitch ratio and pan gains.
    detune: [f32; MAX_UNISON],
    pan_l: [f32; MAX_UNISON],
    pan_r: [f32; MAX_UNISON],
    uni_norm: f32,
}

/// Control-rate update of a voice: glide, drift, cutoff and resonance.
fn control(v: &mut Voice, p: &Params, k: &Consts) {
    let d = p.drift;
    v.note = if k.glide_coef > 0.0 {
        v.target + (v.note - v.target) * k.glide_coef
    } else {
        v.target
    };
    v.drift_timer -= 1;
    if v.drift_timer <= 0 {
        for j in 0..3 {
            v.drift_tgt[j] = v.rng.bipolar();
        }
        v.drift_timer = ((0.2 + 0.8 * v.rng.unit()) * k.ctrl_sr) as i32;
    }
    for j in 0..3 {
        v.drift[j] += (v.drift_tgt[j] - v.drift[j]) * k.drift_k;
    }
    // Pitch: static tolerance plus slow drift, a few cents.
    let c1 = d * (v.tol[0] * 3.0 + v.drift[0] * 5.0);
    let c2 = d * (v.tol[1] * 3.0 + v.drift[1] * 5.0);
    let f1 = midi_to_hz(v.note + c1 * 0.01);
    v.dt1 = (f1 / k.sr).min(0.45);
    v.dt2 = (f1 * p.osc2_ratio * cents(c2 - c1) / k.sr).min(0.45);
    // Cutoff.
    let fe = v.fenv.next();
    let oct = p.filter_env * 6.0 * fe * (0.6 + 0.4 * v.vel)
        + p.key_track * (v.note - 60.0) / 12.0
        + d * (v.tol[2] * 0.15 + v.drift[2] * 0.12);
    let fc = (p.cutoff * 2f32.powf(oct)).clamp(16.0, k.fc_max);
    let res = (p.resonance + d * v.tol[4] * 0.03).clamp(0.0, 1.0);
    if let Model::Svf(_) = p.model {
        for f in &mut v.flt {
            f.svf.set(fc, res, k.sr);
        }
        return;
    }
    // Oversample only when the cutoff is high enough to need it (with
    // hysteresis); the TPT integrator states carry over between rates.
    let os = if v.os { fc > k.os_off } else { fc > k.os_on };
    let switched = os != v.os;
    v.os = os;
    let g_new = (PI * fc / if os { k.osr } else { k.sr }).tan();
    if v.fresh || switched {
        v.g = g_new;
        v.g_inc = 0.0;
        v.fresh = false;
    } else {
        v.g_inc = (g_new - v.g) * (1.0 / CONTROL as f32);
    }
    v.k = match p.model {
        Model::Ladder => 4.25 * res,
        Model::Screamer | Model::Svf(_) => 2.0 - 2.06 * res,
    };
}

/// Sub oscillator and noise of a voice (shared by every unison copy).
#[inline(always)]
fn sub_noise(v: &mut Voice, p: &Params) -> f32 {
    let dts = v.dt1 * 0.5;
    let sub = if p.sub_sine {
        (v.phs * TAU).sin()
    } else {
        let mut ps2 = v.phs + 0.5;
        if ps2 >= 1.0 {
            ps2 -= 1.0;
        }
        (if v.phs < 0.5 { 1.0 } else { -1.0 }) + poly_blep(v.phs, dts) - poly_blep(ps2, dts)
    };
    advance(&mut v.phs, dts);
    let mut x = sub * p.sub;
    if p.noise > 0.0 {
        x += v.rng.bipolar() * p.noise * 0.8;
    }
    x
}

/// One filter step: the ladder and screamer run 2x oversampled at high
/// cutoffs (linear interpolation up, average down).
#[inline(always)]
fn filter(f: &mut Flt, x: f32, p: &Params, k: &Consts, g: f32, kk: f32, os: bool) -> f32 {
    let y = match p.model {
        Model::Ladder => {
            let x = x * k.ladder_in;
            let c = LadderCoefs::new(g, kk);
            let y = if os {
                let ya = ladder(&mut f.s, 0.5 * (f.prev_in + x), &c);
                0.5 * (ya + ladder(&mut f.s, x, &c))
            } else {
                ladder(&mut f.s, x, &c)
            };
            f.prev_in = x;
            // Mild passband-loss compensation at high resonance.
            y * (1.0 + 0.3 * kk)
        }
        Model::Screamer => {
            let x = ftanh(x * k.scream_in);
            let y = if os {
                let ya = screamer(&mut f.s, &mut f.bp, 0.5 * (f.prev_in + x), g, kk);
                0.5 * (ya + screamer(&mut f.s, &mut f.bp, x, g, kk))
            } else {
                screamer(&mut f.s, &mut f.bp, x, g, kk)
            };
            f.prev_in = x;
            1.4 * ftanh(y * 0.9)
        }
        Model::Svf(mode) => {
            // Clean unless driven: blend towards a saturated copy.
            let x = x + p.drive * (ftanh(x * k.svf_in) - x);
            f.svf.process(x, mode)
        }
    };
    f.dc.process(y, k.dc_r)
}

/// One output sample of a voice without unison (before gain and pan).
#[inline(always)]
fn tick(v: &mut Voice, p: &Params, k: &Consts) -> f32 {
    if v.counter.is_multiple_of(CONTROL) {
        control(v, p, k);
    }
    v.counter += 1;

    let o1 = va_osc(p.wave1, v.ph1[0], v.dt1, p.pw, &mut v.rng);
    let o2 = va_osc(p.wave2, v.ph2[0], v.dt2, p.pw, &mut v.rng);
    advance(&mut v.ph1[0], v.dt1);
    advance(&mut v.ph2[0], v.dt2);
    let x = o1 * k.mix1 + o2 * p.mix2 + sub_noise(v, p);

    v.g += v.g_inc;
    let (g, kk, os) = (v.g, v.k, v.os);
    filter(&mut v.flt[0], x, p, k, g, kk, os) * v.env.next()
}

/// One stereo output sample of a voice with a unison stack.
#[inline(always)]
fn tick_wide(v: &mut Voice, p: &Params, k: &Consts) -> (f32, f32) {
    if v.counter.is_multiple_of(CONTROL) {
        control(v, p, k);
    }
    v.counter += 1;

    let (mut l, mut r) = (0.0, 0.0);
    for u in 0..p.unison {
        let dt1 = (v.dt1 * k.detune[u]).min(0.45);
        let dt2 = (v.dt2 * k.detune[u]).min(0.45);
        let o = va_osc(p.wave1, v.ph1[u], dt1, p.pw, &mut v.rng) * k.mix1
            + va_osc(p.wave2, v.ph2[u], dt2, p.pw, &mut v.rng) * p.mix2;
        advance(&mut v.ph1[u], dt1);
        advance(&mut v.ph2[u], dt2);
        l += o * k.pan_l[u];
        r += o * k.pan_r[u];
    }
    let c = sub_noise(v, p);

    v.g += v.g_inc;
    let (g, kk, os) = (v.g, v.k, v.os);
    let yl = filter(&mut v.flt[0], l * k.uni_norm + c, p, k, g, kk, os);
    let yr = filter(&mut v.flt[1], r * k.uni_norm + c, p, k, g, kk, os);
    let e = v.env.next();
    (yl * e, yr * e)
}
