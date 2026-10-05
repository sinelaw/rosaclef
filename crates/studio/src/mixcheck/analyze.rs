//! One render, every measurement: the song (or a stretch of it, with a
//! pre-roll) plays through an engine with its taps on, and every tap — each
//! channel after its fader, each insert after its effects and fader, the
//! master before its effects, at its limiter and at the output — is measured
//! as it goes:
//!
//! - per **hop** (2048 frames, ~43 ms at 48 kHz): sample peak, true peak (the
//!   output), the channels' energies and correlation, and the power in the 24
//!   critical bands and the six broad bands (a 2048-point FFT of the hop,
//!   Tukey window);
//! - per **loudness block** (100 ms): the K-weighted energy (BS.1770);
//! - per hop, every compressor's and limiter's gain reduction.
//!
//! The audio itself is not kept. Measurements are made per stream, so the
//! streams of a chunk are measured on all cores, and the numbers do not
//! depend on how many there are.

use super::dsp::{self, KWeight, RealFft, TruePeak, BANDS, BARKS};
use super::timeline::{Segment, Timeline};
use rosaclef_core::Project;
use rosaclef_engine::Engine;

pub const HOP: usize = 2048;
pub const FFT: usize = 2048;
/// Values kept per stream and hop: the critical bands, the broad bands,
/// then [`F_PEAK`] … [`F_TP`].
pub const FR: usize = BARKS + BANDS + 5;
pub const F_SIX: usize = BARKS;
pub const F_PEAK: usize = BARKS + BANDS;
pub const F_LL: usize = F_PEAK + 1;
pub const F_RR: usize = F_PEAK + 2;
pub const F_LR: usize = F_PEAK + 3;
pub const F_TP: usize = F_PEAK + 4;

/// Frames rendered before the streams of a chunk are measured.
const CHUNK: usize = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKey {
    /// A channel (index in `project.channels`), after its volume and pan.
    Channel(usize),
    /// An insert (index ≥ 1), after its effects and fader.
    Insert(usize),
    /// The master before its effects.
    MasterPre,
    /// The master at its limiter's input.
    MasterLimiter,
    /// The output.
    MasterOut,
}

/// Where a hop or loudness block is in the song.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    /// The segment rendered.
    pub seg: u32,
    /// The span of the form, and the written beat.
    pub span: u32,
    pub beat: f64,
    /// Performance beat.
    pub perf: f64,
    /// Inside the range (not pre-roll).
    pub inside: bool,
}

/// A compressor's or limiter's gain reduction, per hop.
#[derive(Clone, Debug, PartialEq)]
pub struct GrSeries {
    pub insert: usize,
    pub fx: usize,
    pub kind: String,
    pub max: Vec<f32>,
    pub mean: Vec<f32>,
}

/// Everything measured in one render.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub sr: f32,
    pub lblock: usize,
    pub streams: Vec<StreamKey>,
    pub hops: Vec<Pos>,
    pub lblocks: Vec<Pos>,
    /// Per stream: `FR` values per hop (empty: silent throughout).
    pub frames: Vec<Vec<f32>>,
    /// Per stream: K-weighted energy (channels summed) per loudness block.
    pub kms: Vec<Vec<f32>>,
    pub gr: Vec<GrSeries>,
    pub segments: Vec<Segment>,
    pub preroll: f64,
    /// Problems loading devices, samples or soundfonts.
    pub warnings: Vec<String>,
}

const ZERO: [f32; FR] = [0.0; FR];

impl Analysis {
    pub fn stream(&self, key: StreamKey) -> Option<usize> {
        self.streams.iter().position(|k| *k == key)
    }
    /// Stream `s`'s values at hop `h`.
    pub fn frame(&self, s: usize, h: usize) -> &[f32] {
        let f = &self.frames[s];
        if f.is_empty() {
            &ZERO
        } else {
            &f[h * FR..(h + 1) * FR]
        }
    }
    pub fn kms_at(&self, s: usize, b: usize) -> f32 {
        self.kms[s].get(b).copied().unwrap_or(0.0)
    }
    pub fn silent(&self, s: usize) -> bool {
        self.frames[s].is_empty()
    }
}

// ------------------------------------------------------------- streams

/// One stream's measuring state.
struct Meter {
    fft: RealFft,
    /// The hop so far, per channel.
    win: [Vec<f32>; 2],
    fill: usize,
    /// Anything but silence in this hop.
    loud: bool,
    /// Scratch for the FFT.
    x: Vec<f32>,
    p: Vec<f32>,
    peak: f32,
    tp_peak: f32,
    ll: f64,
    rr: f64,
    lr: f64,
    tp: Option<TruePeak>,
    kw: [KWeight; 2],
    lb_acc: f64,
    lb_fill: usize,
    frames: Vec<f32>,
    kms: Vec<f32>,
    any: bool,
}

/// What the meters of every stream share.
struct Shape {
    window: Vec<f32>,
    /// Scales `|X|²` to a share of the mean square (one-sided).
    norm: f64,
    bark: Vec<usize>,
    six: Vec<usize>,
    lblock: usize,
}

impl Meter {
    fn new(sr: f32, true_peak: bool) -> Meter {
        Meter {
            fft: RealFft::new(FFT),
            win: [vec![0.0; HOP], vec![0.0; HOP]],
            fill: 0,
            loud: false,
            x: vec![0.0; FFT],
            p: vec![0.0; FFT / 2 + 1],
            peak: 0.0,
            tp_peak: 0.0,
            ll: 0.0,
            rr: 0.0,
            lr: 0.0,
            tp: true_peak.then(TruePeak::new),
            kw: [KWeight::new(sr), KWeight::new(sr)],
            lb_acc: 0.0,
            lb_fill: 0,
            frames: vec![],
            kms: vec![],
            any: false,
        }
    }

    /// A new segment: the state starts over (a partial hop is dropped).
    fn restart(&mut self, sr: f32) {
        let tp = self.tp.is_some();
        let frames = std::mem::take(&mut self.frames);
        let kms = std::mem::take(&mut self.kms);
        let any = self.any;
        *self = Meter::new(sr, tp);
        self.frames = frames;
        self.kms = kms;
        self.any = any;
    }

    fn push(&mut self, shape: &Shape, l: &[f32], r: &[f32]) {
        let quiet = l.iter().chain(r).all(|x| *x == 0.0);
        for i in 0..l.len() {
            let (x, y) = (l[i], r[i]);
            let at = self.fill;
            self.win[0][at] = x;
            self.win[1][at] = y;
            if !quiet {
                self.peak = self.peak.max(x.abs()).max(y.abs());
                self.ll += (x as f64) * (x as f64);
                self.rr += (y as f64) * (y as f64);
                self.lr += (x as f64) * (y as f64);
                self.loud = true;
            }
            if let Some(tp) = &mut self.tp {
                self.tp_peak = self.tp_peak.max(tp.push(x, y));
            }
            let (a, b) = (self.kw[0].run(x), self.kw[1].run(y));
            self.lb_acc += a * a + b * b;
            self.lb_fill += 1;
            if self.lb_fill == shape.lblock {
                self.kms.push((self.lb_acc / shape.lblock as f64) as f32);
                self.lb_acc = 0.0;
                self.lb_fill = 0;
            }
            self.fill += 1;
            if self.fill == HOP {
                self.finish_hop(shape);
            }
        }
    }

    fn finish_hop(&mut self, shape: &Shape) {
        let mut out = [0f32; FR];
        if self.loud {
            let (x, p) = (&mut self.x, &mut self.p);
            for ch in 0..2 {
                for (k, v) in x.iter_mut().enumerate() {
                    *v = self.win[ch][k] * shape.window[k];
                }
                self.fft.power(x, p);
                for (k, pk) in p.iter().enumerate().skip(1) {
                    let w = if k == FFT / 2 { 0.5 } else { 1.0 };
                    let v = (*pk as f64 * shape.norm * w * 0.5) as f32;
                    out[shape.bark[k]] += v;
                    out[F_SIX + shape.six[k]] += v;
                }
            }
            let n = HOP as f64;
            out[F_PEAK] = self.peak;
            out[F_LL] = (self.ll / n) as f32;
            out[F_RR] = (self.rr / n) as f32;
            out[F_LR] = (self.lr / n) as f32;
            self.any = true;
        }
        out[F_TP] = self.tp_peak;
        self.frames.extend_from_slice(&out);
        self.loud = false;
        self.fill = 0;
        self.peak = 0.0;
        self.tp_peak = 0.0;
        self.ll = 0.0;
        self.rr = 0.0;
        self.lr = 0.0;
    }
}

/// Measure each stream's chunk, on all cores where there are threads.
fn measure(meters: &mut [Meter], chunks: &[[Vec<f32>; 2]], n: usize, shape: &Shape) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism()
            .map_or(1, |x| x.get())
            .min(meters.len());
        if threads > 1 {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let slots: Vec<std::sync::Mutex<&mut Meter>> =
                meters.iter_mut().map(std::sync::Mutex::new).collect();
            std::thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= slots.len() {
                            break;
                        }
                        let mut m = slots[i].lock().unwrap_or_else(|e| e.into_inner());
                        m.push(shape, &chunks[i][0][..n], &chunks[i][1][..n]);
                    });
                }
            });
            return;
        }
    }
    for (m, c) in meters.iter_mut().zip(chunks) {
        m.push(shape, &c[0][..n], &c[1][..n]);
    }
}

// ------------------------------------------------------------ rendering

/// An engine with the project, its samples and soundfonts (made once per
/// segment, so each starts from silence).
pub trait EngineFactory {
    fn make(&mut self) -> Engine;
}

/// Render `segments` (each after `preroll` beats of pre-roll) and measure
/// every tap.
pub fn run(
    project: &Project,
    timeline: &Timeline,
    segments: &[Segment],
    preroll: f64,
    sr: f32,
    factory: &mut dyn EngineFactory,
    mut progress: impl FnMut(f64),
) -> Analysis {
    let nch = project.channels.len();
    let nins = project.mixer.inserts.len();
    let mut streams: Vec<StreamKey> = (0..nch).map(StreamKey::Channel).collect();
    streams.extend((1..nins).map(StreamKey::Insert));
    streams.extend([
        StreamKey::MasterPre,
        StreamKey::MasterLimiter,
        StreamKey::MasterOut,
    ]);
    let lblock = (sr as f64 * 0.1).round() as usize;
    let (window, wsum) = dsp::tukey(FFT, 0.5);
    let shape = Shape {
        window,
        norm: 2.0 / (FFT as f64 * wsum),
        bark: dsp::bin_bands(FFT, sr, &dsp::BARK_EDGES)
            .into_iter()
            .map(|b| b.min(BARKS - 1))
            .collect(),
        six: dsp::bin_bands(FFT, sr, &dsp::BAND_EDGES)
            .into_iter()
            .map(|b| b.min(BANDS - 1))
            .collect(),
        lblock,
    };
    let mut meters: Vec<Meter> = streams
        .iter()
        .map(|k| Meter::new(sr, *k == StreamKey::MasterOut))
        .collect();
    let mut chunks: Vec<[Vec<f32>; 2]> = streams
        .iter()
        .map(|_| [vec![0.0; CHUNK], vec![0.0; CHUNK]])
        .collect();
    let dynamics: Vec<(usize, usize, String)> = project
        .mixer
        .inserts
        .iter()
        .enumerate()
        .flat_map(|(i, ins)| {
            ins.effects
                .iter()
                .enumerate()
                .filter(|(_, d)| matches!(d.kind.as_str(), "compressor" | "limiter"))
                .map(move |(j, d)| (i, j, d.kind.clone()))
        })
        .collect();
    let mut gr: Vec<GrSeries> = dynamics
        .iter()
        .map(|(i, j, k)| GrSeries {
            insert: *i,
            fx: *j,
            kind: k.clone(),
            max: vec![],
            mean: vec![],
        })
        .collect();
    let mut hops = vec![];
    let mut lblocks = vec![];
    let mut warnings = vec![];
    let total: f64 = segments.iter().map(|s| s.to - s.from + preroll).sum();
    let mut done_beats = 0.0;

    for (si, seg) in segments.iter().enumerate() {
        let start = (seg.from - preroll).max(0.0);
        let (span, beat) = timeline.at_perf(start);
        let mut engine = factory.make();
        if si == 0 {
            warnings.extend(engine.device_errors.iter().cloned());
        }
        engine.set_taps(true);
        engine.set_mode(rosaclef_engine::PlayMode::Song);
        engine.stop();
        engine.seek_span(span, beat);
        engine.play();
        engine.chase_notes();
        engine.set_crew(true);
        for m in &mut meters {
            m.restart(sr);
        }
        let block = engine.block_frames();
        debug_assert_eq!(HOP % block, 0);
        // Where each block starts: (frame, span, beat).
        let mut clock: Vec<(usize, u32, f64)> = vec![];
        let mut frame = 0usize;
        let mut filled = 0usize;
        let mut grh: Vec<(f32, f64, usize)> = vec![(0.0, 0.0, 0); dynamics.len()];
        let mut last_perf = -1.0;
        let mut out_l = vec![0f32; block];
        let mut out_r = vec![0f32; block];
        loop {
            let (fa, pos) = engine.form_position();
            let perf = timeline.perf_of(fa, pos);
            if perf >= seg.to - 1e-6 || perf < last_perf - 1e-6 {
                break;
            }
            last_perf = perf;
            clock.push((frame, fa as u32, pos));
            engine.process(&mut out_l, &mut out_r);
            for (k, key) in streams.iter().enumerate() {
                let (l, r): (&[f32], &[f32]) = match key {
                    StreamKey::Channel(c) => engine.tap_channel(*c),
                    StreamKey::Insert(i) if engine.insert_audible(*i) => engine.tap_insert(*i),
                    StreamKey::Insert(_) => (
                        &[0.0; rosaclef_engine::MAX_BLOCK],
                        &[0.0; rosaclef_engine::MAX_BLOCK],
                    ),
                    StreamKey::MasterPre => engine.tap_master_pre(),
                    StreamKey::MasterLimiter => engine.tap_master_limiter(),
                    StreamKey::MasterOut => (&out_l, &out_r),
                };
                chunks[k][0][filled..filled + block].copy_from_slice(&l[..block]);
                chunks[k][1][filled..filled + block].copy_from_slice(&r[..block]);
            }
            for (d, (i, j, _)) in dynamics.iter().enumerate() {
                if let Some((mx, mean)) = engine.take_gain_reduction(*i, *j) {
                    let a = &mut grh[d];
                    a.0 = a.0.max(mx);
                    a.1 += mean as f64 * block as f64;
                    a.2 += block;
                }
            }
            filled += block;
            frame += block;
            if frame.is_multiple_of(HOP) {
                for (d, a) in grh.iter_mut().enumerate() {
                    gr[d].max.push(a.0);
                    gr[d].mean.push(if a.2 > 0 {
                        (a.1 / a.2 as f64) as f32
                    } else {
                        0.0
                    });
                    *a = (0.0, 0.0, 0);
                }
            }
            if filled == CHUNK {
                measure(&mut meters, &chunks, filled, &shape);
                filled = 0;
            }
            let beats = (perf - start).max(0.0);
            progress((done_beats + beats) / total.max(1e-9));
        }
        if filled > 0 {
            measure(&mut meters, &chunks, filled, &shape);
        }
        engine.set_crew(false);
        // Where each finished hop and loudness block lies (at its middle).
        let nh = frame / HOP;
        let nb = frame / lblock;
        let at = |f: usize| -> Pos {
            let i = clock.partition_point(|c| c.0 <= f).max(1) - 1;
            let (_, span, beat) = clock[i];
            let perf = timeline.perf_of(span as usize, beat);
            Pos {
                seg: si as u32,
                span,
                beat,
                perf,
                inside: perf >= seg.from - 1e-6 && perf < seg.to - 1e-6,
            }
        };
        hops.extend((0..nh).map(|h| at(h * HOP + HOP / 2)));
        lblocks.extend((0..nb).map(|b| at(b * lblock + lblock / 2)));
        // The GR of a partial last hop is dropped with it.
        for g in &mut gr {
            g.max.truncate(hops.len());
            g.mean.truncate(hops.len());
        }
        done_beats += seg.to - start;
    }
    let mut a = Analysis {
        sr,
        lblock,
        streams,
        hops,
        lblocks,
        frames: meters
            .iter_mut()
            .map(|m| {
                if m.any {
                    std::mem::take(&mut m.frames)
                } else {
                    vec![]
                }
            })
            .collect(),
        kms: meters
            .iter_mut()
            .map(|m| std::mem::take(&mut m.kms))
            .collect(),
        gr,
        segments: segments.to_vec(),
        preroll,
        warnings,
    };
    quantize(&mut a);
    a
}

/// Round every measurement to what the cache keeps (powers to 0.01 dB), so
/// a cached analysis and a fresh one give the same report.
pub fn quantize(a: &mut Analysis) {
    for f in &mut a.frames {
        for (i, v) in f.iter_mut().enumerate() {
            *v = if i % FR == F_LR {
                qsigned(*v)
            } else {
                qpow(*v)
            };
        }
    }
    for k in &mut a.kms {
        k.iter_mut().for_each(|v| *v = qpow(*v));
    }
    for g in &mut a.gr {
        g.max.iter_mut().for_each(|v| *v = qgr(*v));
        g.mean.iter_mut().for_each(|v| *v = qgr(*v));
    }
}

/// Code of a non-negative value: 0 for (near) silence, else its level in
/// hundredths of a dB above -250 dB.
pub fn enc_pow(v: f32) -> u16 {
    if v.is_nan() || v <= 1e-25 {
        return 0;
    }
    let db = 10.0 * (v as f64).log10();
    ((db + 250.0) * 100.0).round().clamp(1.0, 65535.0) as u16
}

pub fn dec_pow(c: u16) -> f32 {
    if c == 0 {
        0.0
    } else {
        10f64.powf((c as f64 / 100.0 - 250.0) / 10.0) as f32
    }
}

fn qpow(v: f32) -> f32 {
    dec_pow(enc_pow(v))
}

/// Correlation terms keep their sign.
fn qsigned(v: f32) -> f32 {
    if v < 0.0 {
        -qpow(-v)
    } else {
        qpow(v)
    }
}

pub fn enc_gr(v: f32) -> u16 {
    (v.max(0.0) as f64 * 100.0).round().min(65535.0) as u16
}

pub fn dec_gr(c: u16) -> f32 {
    c as f32 / 100.0
}

fn qgr(v: f32) -> f32 {
    dec_gr(enc_gr(v))
}

/// Measure a finished stereo recording (a reference track) the way the
/// master's output is measured: one stream, every hop inside.
pub fn measure_audio(left: &[f32], right: &[f32], sr: f32) -> Analysis {
    let lblock = (sr as f64 * 0.1).round() as usize;
    let (window, wsum) = dsp::tukey(FFT, 0.5);
    let shape = Shape {
        window,
        norm: 2.0 / (FFT as f64 * wsum),
        bark: dsp::bin_bands(FFT, sr, &dsp::BARK_EDGES)
            .into_iter()
            .map(|b| b.min(BARKS - 1))
            .collect(),
        six: dsp::bin_bands(FFT, sr, &dsp::BAND_EDGES)
            .into_iter()
            .map(|b| b.min(BANDS - 1))
            .collect(),
        lblock,
    };
    let mut m = Meter::new(sr, true);
    let n = left.len().min(right.len());
    for start in (0..n).step_by(CHUNK) {
        let end = (start + CHUNK).min(n);
        m.push(&shape, &left[start..end], &right[start..end]);
    }
    let pos = |f: usize| Pos {
        seg: 0,
        span: 0,
        beat: f as f64 / sr as f64,
        perf: f as f64 / sr as f64,
        inside: true,
    };
    let hops = (0..n / HOP).map(|h| pos(h * HOP + HOP / 2)).collect();
    let lblocks = (0..n / lblock)
        .map(|b| pos(b * lblock + lblock / 2))
        .collect();
    let mut a = Analysis {
        sr,
        lblock,
        streams: vec![StreamKey::MasterOut],
        hops,
        lblocks,
        frames: vec![if m.any {
            std::mem::take(&mut m.frames)
        } else {
            vec![]
        }],
        kms: vec![std::mem::take(&mut m.kms)],
        gr: vec![],
        segments: vec![],
        preroll: 0.0,
        warnings: vec![],
    };
    quantize(&mut a);
    a
}
