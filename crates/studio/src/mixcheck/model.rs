//! From measurements to quantities: loudness (BS.1770 / EBU R128), peaks,
//! correlation, spectra, each element's share of the mix, and how audible
//! it is under everything else (a partial-loudness model of masking).
//!
//! **Elements.** Every channel, and the inserts that are more than one
//! channel's strip (buses with several channels, inserts playing audio
//! clips). An insert's output is shared among the channels routed to it in
//! proportion to their energy in each band: that is each channel's
//! contribution at the master.
//!
//! **Masking** (after Moore & Glasberg's partial loudness, on Zwicker's 24
//! critical bands): per hop, each element's and the rest of the mix's band
//! powers are spread across bands (Schroeder's spreading function) into
//! excitations. The element's specific loudness alone is
//! `C((E_s + E_q)^α − E_q^α)` per band (E_q: threshold in quiet); in the
//! mix it is only what it adds to the masker's loudness,
//! `C((E_s + E_m + E_q)^α − (E_m + E_q)^α)`. A hop is *audible* when the
//! partial loudness keeps a share θ of the loudness alone (θ = 0.15; strict
//! 0.25, loose 0.08). Hops too quiet to hear even alone (the end of a decay)
//! are not counted. The maskers of a hop are the parts with the strongest
//! excitation in the bands where the element is loudest.

use super::analyze::{Analysis, StreamKey, FR, F_LL, F_LR, F_PEAK, F_RR, F_SIX, F_TP};
use super::dsp::{self, BANDS, BARKS, BARK_EDGES};
use rosaclef_core::Project;

pub const ALPHA: f64 = 0.23;
/// 0 dBFS (mean square of a full-scale square wave) as dB SPL.
pub const SPL_AT_FULL_SCALE: f64 = 100.0;
/// The masker counts this much less than its excitation (dB): between a
/// noise masker (~5 dB) and a tonal one (~15–20 dB), as music mixes both.
pub const MASKING_INDEX: f64 = 10.0;
pub const SONE_C: f64 = 0.14;
/// Loudness (sones) below which a frame is not heard even alone.
pub const AUDIBLE_SONES: f64 = 0.02;

/// Threshold in quiet (Terhardt), dB SPL.
fn threshold_db(f: f64) -> f64 {
    let k = f / 1000.0;
    3.64 * k.powf(-0.8) - 6.5 * (-0.6 * (k - 3.3).powi(2)).exp() + 1e-3 * k.powi(4)
}

/// The spreading of masking across bands (power factors, `[to][from]`).
pub fn spreading() -> [[f64; BARKS]; BARKS] {
    let mut m = [[0.0; BARKS]; BARKS];
    for (z, row) in m.iter_mut().enumerate() {
        for (b, v) in row.iter_mut().enumerate() {
            let dz = z as f64 - b as f64 + 0.474;
            let sf = 15.81 + 7.5 * dz - 17.5 * (1.0 + dz * dz).sqrt();
            *v = 10f64.powf(sf.max(-100.0) / 10.0);
        }
    }
    m
}

/// The psychoacoustic constants, per band.
pub struct Ear {
    pub spread: [[f64; BARKS]; BARKS],
    /// Threshold in quiet as an excitation (relative to full scale).
    pub quiet: [f64; BARKS],
    quiet_a: [f64; BARKS],
    masker: f64,
}

impl Default for Ear {
    fn default() -> Self {
        let mut quiet = [0.0; BARKS];
        let mut quiet_a = [0.0; BARKS];
        for b in 0..BARKS {
            let t = threshold_db(dsp::bark_centre(b) as f64) - SPL_AT_FULL_SCALE;
            quiet[b] = 10f64.powf(t / 10.0);
            quiet_a[b] = quiet[b].powf(ALPHA);
        }
        Ear {
            spread: spreading(),
            quiet,
            quiet_a,
            masker: 10f64.powf(-MASKING_INDEX / 10.0),
        }
    }
}

impl Ear {
    pub fn excite(&self, p: &[f64; BARKS]) -> [f64; BARKS] {
        let mut e = [0.0; BARKS];
        for (z, out) in e.iter_mut().enumerate() {
            let row = &self.spread[z];
            *out = (0..BARKS).map(|b| row[b] * p[b]).sum();
        }
        e
    }
    /// Loudness alone and in the mix (sones), from excitations.
    pub fn loudness(&self, s: &[f64; BARKS], m: &[f64; BARKS]) -> (f64, f64) {
        let (mut alone, mut part) = (0.0, 0.0);
        for z in 0..BARKS {
            let q = self.quiet[z];
            alone += (s[z] + q).powf(ALPHA) - self.quiet_a[z];
            let mm = m[z].max(0.0) * self.masker + q;
            part += ((s[z] + mm).powf(ALPHA) - mm.powf(ALPHA)).max(0.0);
        }
        (SONE_C * alone.max(0.0), SONE_C * part)
    }
}

// ------------------------------------------------------------- loudness

/// Loudness over loudness blocks: momentary (400 ms), short-term (3 s) and
/// gated integrated values.
pub struct Loudness<'a> {
    pub a: &'a Analysis,
}

impl Loudness<'_> {
    /// Mean K-weighted energy of the `n` blocks ending at block `b` (within
    /// its segment), with `kms` giving a block's energy. A window that is not
    /// full yet (the start of a render or a recording) is not measured: 0,
    /// which every gate drops, as BS.1770 takes only whole windows.
    pub fn window(&self, kms: &dyn Fn(usize) -> f64, b: usize, n: usize) -> f64 {
        let seg = self.a.lblocks[b].seg;
        let mut sum = 0.0;
        let mut k = 0;
        let mut i = b as isize;
        while k < n && i >= 0 && self.a.lblocks[i as usize].seg == seg {
            sum += kms(i as usize);
            k += 1;
            i -= 1;
        }
        if k < n {
            0.0
        } else {
            sum / k as f64
        }
    }
}

/// Integrated loudness (BS.1770-4 gating) of 400 ms windows' energies.
pub fn integrated(windows: &[f64]) -> Option<f64> {
    let abs: Vec<f64> = windows
        .iter()
        .copied()
        .filter(|e| dsp::lufs(*e) > -70.0)
        .collect();
    if abs.is_empty() {
        return None;
    }
    let mean = abs.iter().sum::<f64>() / abs.len() as f64;
    let gate = dsp::lufs(mean) - 10.0;
    let rel: Vec<f64> = abs.into_iter().filter(|e| dsp::lufs(*e) > gate).collect();
    if rel.is_empty() {
        return None;
    }
    Some(dsp::lufs(rel.iter().sum::<f64>() / rel.len() as f64))
}

/// Loudness range (EBU Tech 3342) of short-term windows' energies.
pub fn loudness_range(short: &[f64]) -> Option<f64> {
    let abs: Vec<f64> = short
        .iter()
        .copied()
        .filter(|e| dsp::lufs(*e) > -70.0)
        .collect();
    if abs.len() < 2 {
        return None;
    }
    let mean = abs.iter().sum::<f64>() / abs.len() as f64;
    let gate = dsp::lufs(mean) - 20.0;
    let mut l: Vec<f64> = abs
        .into_iter()
        .map(dsp::lufs)
        .filter(|x| *x > gate)
        .collect();
    if l.len() < 2 {
        return None;
    }
    l.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| l[((l.len() - 1) as f64 * p).round() as usize];
    Some(pct(0.95) - pct(0.10))
}

// ---------------------------------------------------------- the mix

/// One element of the mix: a channel, or an insert.
#[derive(Clone, Debug)]
pub struct Element {
    pub id: String,
    pub name: String,
    pub channel: Option<usize>,
    pub insert: usize,
    /// The stream of its own tap.
    pub stream: usize,
    /// The units (see [`Mix::units`]) it is made of.
    pub units: Vec<usize>,
}

/// A part of the master's input that masking and shares count: a channel
/// (its share of its insert), a whole insert (audio clips, no channels), or
/// what reaches the master directly from audio clips.
#[derive(Clone, Debug)]
pub struct Unit {
    pub id: String,
    pub channel: Option<usize>,
    pub insert: usize,
}

pub struct Mix<'a> {
    pub a: &'a Analysis,
    pub p: &'a Project,
    pub units: Vec<Unit>,
    pub elements: Vec<Element>,
    pub master_pre: usize,
    pub master_lim: usize,
    pub master_out: usize,
    /// Hops and loudness blocks inside the range.
    pub inside: Vec<usize>,
    pub inside_blocks: Vec<usize>,
    /// The stream of each channel, of each insert.
    ch_stream: Vec<usize>,
    ins_stream: Vec<Option<usize>>,
    /// Channels routed to each insert.
    routed: Vec<Vec<usize>>,
    /// The residual unit (audio clips straight to the master).
    residual: Option<usize>,
}

pub fn insert_id(p: &Project, i: usize) -> String {
    let name = p
        .mixer
        .inserts
        .get(i)
        .map(|x| x.name.as_str())
        .unwrap_or("");
    format!("insert:{i}/{name}")
}

pub fn channel_id(p: &Project, c: usize) -> String {
    format!("channel:{}", p.channels[c].id)
}

impl<'a> Mix<'a> {
    pub fn new(a: &'a Analysis, p: &'a Project) -> Mix<'a> {
        let s = |k: StreamKey| a.stream(k).expect("stream");
        let nins = p.mixer.inserts.len();
        let route = |c: usize| {
            let m = p.channels[c].mixer.index();
            if m < nins {
                m
            } else {
                0
            }
        };
        let mut routed = vec![vec![]; nins];
        for c in 0..p.channels.len() {
            routed[route(c)].push(c);
        }
        let clips_on = |i: usize| {
            p.playlist
                .clips
                .iter()
                .any(|c| !c.sample.is_empty() && c.mixer.index() == i)
        };
        let mut units = vec![];
        let mut elements = vec![];
        let ch_stream: Vec<usize> = (0..p.channels.len())
            .map(|c| s(StreamKey::Channel(c)))
            .collect();
        let ins_stream: Vec<Option<usize>> = (0..nins)
            .map(|i| {
                if i == 0 {
                    None
                } else {
                    a.stream(StreamKey::Insert(i))
                }
            })
            .collect();
        // Inserts with audio clips (or no channels) count as one unit.
        let mut ins_unit = vec![None; nins];
        for i in 1..nins {
            if routed[i].is_empty() || clips_on(i) {
                ins_unit[i] = Some(units.len());
                units.push(Unit {
                    id: insert_id(p, i),
                    channel: None,
                    insert: i,
                });
            }
        }
        let mut ch_unit = vec![usize::MAX; p.channels.len()];
        for (c, cu) in ch_unit.iter_mut().enumerate() {
            let i = route(c);
            if ins_unit[i].is_none() {
                *cu = units.len();
                units.push(Unit {
                    id: channel_id(p, c),
                    channel: Some(c),
                    insert: i,
                });
            }
        }
        let residual = clips_on(0).then(|| {
            units.push(Unit {
                id: insert_id(p, 0),
                channel: None,
                insert: 0,
            });
            units.len() - 1
        });
        for c in 0..p.channels.len() {
            let i = route(c);
            elements.push(Element {
                id: channel_id(p, c),
                name: p.channels[c].name.clone(),
                channel: Some(c),
                insert: i,
                stream: ch_stream[c],
                units: match ins_unit[i] {
                    Some(u) => vec![u],
                    None => vec![ch_unit[c]],
                },
            });
        }
        for i in 1..nins {
            let bus = routed[i].len() >= 2;
            if let (Some(st), true) = (ins_stream[i], ins_unit[i].is_some() || bus) {
                elements.push(Element {
                    id: insert_id(p, i),
                    name: p.mixer.inserts[i].name.clone(),
                    channel: None,
                    insert: i,
                    stream: st,
                    units: match ins_unit[i] {
                        Some(u) => vec![u],
                        None => routed[i].iter().map(|c| ch_unit[*c]).collect(),
                    },
                });
            }
        }
        let inside = (0..a.hops.len()).filter(|h| a.hops[*h].inside).collect();
        let inside_blocks = (0..a.lblocks.len())
            .filter(|b| a.lblocks[*b].inside)
            .collect();
        Mix {
            a,
            p,
            units,
            elements,
            master_pre: s(StreamKey::MasterPre),
            master_lim: s(StreamKey::MasterLimiter),
            master_out: s(StreamKey::MasterOut),
            inside,
            inside_blocks,
            ch_stream,
            ins_stream,
            routed,
            residual,
        }
    }

    /// An insert by its tap index… the elements of insert `i`'s strip
    /// (an extra element for `--focus` on a single-channel insert).
    pub fn insert_element(&self, i: usize) -> Option<Element> {
        let st = self.ins_stream.get(i).copied().flatten()?;
        let units: Vec<usize> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.insert == i)
            .map(|(k, _)| k)
            .collect();
        Some(Element {
            id: insert_id(self.p, i),
            name: self.p.mixer.inserts[i].name.clone(),
            channel: None,
            insert: i,
            stream: st,
            units,
        })
    }

    /// Unit `u`'s value `f` (an energy) at the master in hop `h`, through
    /// `own(stream)` and its insert's share.
    fn attributed(&self, u: usize, own: &dyn Fn(usize) -> f64) -> f64 {
        let unit = &self.units[u];
        if Some(u) == self.residual {
            let t = own(self.master_pre);
            let rest: f64 = (0..self.units.len())
                .filter(|k| *k != u)
                .map(|k| self.attributed(k, own))
                .sum();
            return (t - rest).max(0.0);
        }
        match unit.channel {
            None => self.ins_stream[unit.insert].map(own).unwrap_or(0.0),
            Some(c) if unit.insert == 0 => own(self.ch_stream[c]),
            Some(c) => {
                let total: f64 = self.routed[unit.insert]
                    .iter()
                    .map(|k| own(self.ch_stream[*k]))
                    .sum();
                if total <= 0.0 {
                    return 0.0;
                }
                let out = self.ins_stream[unit.insert].map(own).unwrap_or(0.0);
                own(self.ch_stream[c]) * out / total
            }
        }
    }

    /// A unit's critical-band powers at the master in hop `h`.
    pub fn unit_bark(&self, u: usize, h: usize) -> [f64; BARKS] {
        let mut out = [0.0; BARKS];
        for (b, v) in out.iter_mut().enumerate() {
            *v = self.attributed(u, &|s| self.a.frame(s, h)[b] as f64);
        }
        out
    }

    pub fn unit_six(&self, u: usize, h: usize) -> [f64; BANDS] {
        let mut out = [0.0; BANDS];
        for (b, v) in out.iter_mut().enumerate() {
            *v = self.attributed(u, &|s| self.a.frame(s, h)[F_SIX + b] as f64);
        }
        out
    }

    /// A unit's energy (mean square) at the master in hop `h`.
    pub fn unit_ms(&self, u: usize, h: usize) -> f64 {
        self.attributed(u, &|s| {
            let f = self.a.frame(s, h);
            (f[F_LL] + f[F_RR]) as f64 * 0.5
        })
    }

    /// A unit's K-weighted energy at the master in loudness block `b`.
    pub fn unit_kms(&self, u: usize, b: usize) -> f64 {
        self.attributed(u, &|s| self.a.kms_at(s, b) as f64)
    }

    pub fn ms(&self, s: usize, h: usize) -> f64 {
        let f = self.a.frame(s, h);
        (f[F_LL] + f[F_RR]) as f64 * 0.5
    }
    pub fn peak(&self, s: usize, h: usize) -> f64 {
        self.a.frame(s, h)[F_PEAK] as f64
    }
    pub fn true_peak(&self, h: usize) -> f64 {
        self.a.frame(self.master_out, h)[F_TP] as f64
    }
    /// The correlation terms (ll, rr, lr) of stream `s` in hop `h`.
    pub fn corr_terms(&self, s: usize, h: usize) -> (f64, f64, f64) {
        let f = self.a.frame(s, h);
        (f[F_LL] as f64, f[F_RR] as f64, f[F_LR] as f64)
    }
    /// The stereo correlation of stream `s` over hops `hs`.
    pub fn correlation(&self, s: usize, hs: &[usize]) -> Option<f64> {
        let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
        for &h in hs {
            let c = self.corr_terms(s, h);
            x += c.0;
            y += c.1;
            z += c.2;
        }
        correlation(x, y, z)
    }
    /// The units, loudest first by `energy(unit, hop)` over hops `hs`, each
    /// with its share of the total.
    pub fn ranked(&self, hs: &[usize], energy: &dyn Fn(usize, usize) -> f64) -> Vec<(usize, f64)> {
        let mut e: Vec<(usize, f64)> = (0..self.units.len())
            .map(|u| (u, hs.iter().map(|h| energy(u, *h)).sum::<f64>()))
            .collect();
        let total: f64 = e.iter().map(|x| x.1).sum();
        e.sort_by(|x, y| y.1.total_cmp(&x.1));
        e.into_iter()
            .map(|(u, x)| (u, if total > 0.0 { x / total } else { 0.0 }))
            .collect()
    }
    pub fn six(&self, s: usize, h: usize) -> [f64; BANDS] {
        let f = self.a.frame(s, h);
        let mut out = [0.0; BANDS];
        for (b, v) in out.iter_mut().enumerate() {
            *v = f[F_SIX + b] as f64;
        }
        out
    }
    pub fn bark(&self, s: usize, h: usize) -> [f64; BARKS] {
        let f = &self.a.frame(s, h)[..FR];
        let mut out = [0.0; BARKS];
        for (b, v) in out.iter_mut().enumerate() {
            *v = f[b] as f64;
        }
        out
    }
}

/// Correlation of summed terms (+1 mono … −1 out of phase); None in silence.
pub fn correlation(ll: f64, rr: f64, lr: f64) -> Option<f64> {
    let d = (ll * rr).sqrt();
    (d > 1e-14).then(|| (lr / d).clamp(-1.0, 1.0))
}

/// How much quieter the mono fold-down is than the stereo signal (dB;
/// 0 = mono, about −3 for unrelated channels, very negative = cancelling).
pub fn mono_loss(ll: f64, rr: f64, lr: f64) -> Option<f64> {
    let s = ll + rr;
    (s > 1e-14).then(|| dsp::db(((s + 2.0 * lr) / 2.0).max(1e-20) / s))
}

/// The band (Hz) of critical bands `lo..=hi`.
pub fn band_hz(lo: usize, hi: usize) -> [f64; 2] {
    let top = if hi + 1 >= BARKS {
        20000.0
    } else {
        BARK_EDGES[hi + 1] as f64
    };
    [BARK_EDGES[lo].max(20.0) as f64, top]
}

// -------------------------------------------------------------- masking

/// What the masking model says of an element over the range.
#[derive(Clone, Debug, Default)]
pub struct Audibility {
    /// Hops where it plays, and where it is audible.
    pub active: usize,
    pub audible: usize,
    /// Its loudness alone and in the mix (mean sones over active hops).
    pub alone: f64,
    pub partial: f64,
    /// Maskers: unit, weight, sum of masking dB, count, band wins.
    pub maskers: Vec<Masker>,
    /// The element's long-term critical-band powers at the master.
    pub spectrum: [f64; BARKS],
}

#[derive(Clone, Debug)]
pub struct Masker {
    pub unit: usize,
    pub weight: f64,
    pub db_sum: f64,
    pub n: usize,
    pub bands: [f64; BARKS],
}

impl Audibility {
    pub fn fraction(&self) -> f64 {
        if self.active == 0 {
            1.0
        } else {
            self.audible as f64 / self.active as f64
        }
    }
}

/// Per hop: every unit's excitation and the strongest three per band.
pub struct HopExcitation {
    pub units: Vec<[f64; BARKS]>,
    pub total: [f64; BARKS],
    pub top: [[usize; 3]; BARKS],
    pub raw: Vec<[f64; BARKS]>,
}

pub fn excitations(mix: &Mix, ear: &Ear, h: usize) -> HopExcitation {
    let raw: Vec<[f64; BARKS]> = (0..mix.units.len()).map(|u| mix.unit_bark(u, h)).collect();
    let units: Vec<[f64; BARKS]> = raw.iter().map(|r| ear.excite(r)).collect();
    let mut total = [0.0; BARKS];
    let mut top = [[usize::MAX; 3]; BARKS];
    for (u, e) in units.iter().enumerate() {
        for z in 0..BARKS {
            total[z] += e[z];
            let t = &mut top[z];
            let v = e[z];
            let better = |k: usize| k == usize::MAX || v > units[k][z];
            if better(t[0]) {
                t[2] = t[1];
                t[1] = t[0];
                t[0] = u;
            } else if better(t[1]) {
                t[2] = t[1];
                t[1] = u;
            } else if better(t[2]) {
                t[2] = u;
            }
        }
    }
    HopExcitation {
        units,
        total,
        top,
        raw,
    }
}

/// Share of its own loudness a part must keep in the mix to count as
/// audible in a frame: one measure for every threshold (the threshold moves
/// the verdicts' cut-offs, not what is measured).
pub const AUDIBLE_SHARE: f64 = 0.15;

/// A change to try in the model: the element louder by `gain_db`, and a
/// masker cut by `cut[z]` (power factors per band).
#[derive(Clone, Debug)]
pub struct Change {
    pub gain_db: f64,
    pub masker: Option<(usize, [f64; BARKS])>,
}

/// The audibility of the elements over the inside hops (and, for each
/// element, of the changes in `trials`: the audible fraction under each).
pub fn audibility(
    mix: &Mix,
    ear: &Ear,
    theta: f64,
    elements: &[Element],
    trials: &[Vec<Change>],
) -> (Vec<Audibility>, Vec<Vec<f64>>) {
    let n = elements.len();
    let mut out = vec![Audibility::default(); n];
    let mut tried: Vec<Vec<(usize, usize)>> =
        trials.iter().map(|t| vec![(0, 0); t.len()]).collect();
    // Activity: within 40 dB of the element's loudest hop, above -80 dBFS.
    let level = |e: &Element, h: usize| -> f64 { e.units.iter().map(|u| mix.unit_ms(*u, h)).sum() };
    let mut loudest = vec![0f64; n];
    let mut levels: Vec<Vec<f64>> = vec![vec![]; n];
    for (i, e) in elements.iter().enumerate() {
        levels[i] = mix.inside.iter().map(|h| level(e, *h)).collect();
        loudest[i] = levels[i].iter().copied().fold(0.0, f64::max);
    }
    let mut maskers: Vec<Vec<Masker>> = vec![vec![]; n];
    for (k, &h) in mix.inside.iter().enumerate() {
        let active: Vec<usize> = (0..n)
            .filter(|i| {
                let l = levels[*i][k];
                l > 1e-8 && l > loudest[*i] * 1e-4
            })
            .collect();
        if active.is_empty() {
            continue;
        }
        let x = excitations(mix, ear, h);
        for i in active {
            let e = &elements[i];
            let mut s = [0.0; BARKS];
            for u in &e.units {
                for (sz, v) in s.iter_mut().zip(&x.units[*u]) {
                    *sz += v;
                }
            }
            let m: [f64; BARKS] = std::array::from_fn(|z| (x.total[z] - s[z]).max(0.0));
            let (alone, part) = ear.loudness(&s, &m);
            // Too quiet to hear even in silence: not masking's doing.
            if alone < AUDIBLE_SONES {
                continue;
            }
            let a = &mut out[i];
            a.active += 1;
            a.alone += alone;
            a.partial += part;
            for u in &e.units {
                for (sz, v) in a.spectrum.iter_mut().zip(&x.raw[*u]) {
                    *sz += v;
                }
            }
            let ok = part >= theta * alone;
            if ok {
                a.audible += 1;
            } else {
                // Who masks it: the strongest other part in its loudest bands.
                let smax = s.iter().copied().fold(0.0, f64::max);
                for (z, &sz) in s.iter().enumerate() {
                    if sz < smax * 0.1 || sz <= 0.0 {
                        continue;
                    }
                    let Some(&u) = x.top[z]
                        .iter()
                        .find(|u| **u != usize::MAX && !e.units.contains(*u))
                    else {
                        continue;
                    };
                    let v = x.units[u][z];
                    if v <= sz * 0.25 {
                        continue;
                    }
                    let list = &mut maskers[i];
                    let slot = match list.iter().position(|mk| mk.unit == u) {
                        Some(j) => j,
                        None => {
                            list.push(Masker {
                                unit: u,
                                weight: 0.0,
                                db_sum: 0.0,
                                n: 0,
                                bands: [0.0; BARKS],
                            });
                            list.len() - 1
                        }
                    };
                    let mk = &mut list[slot];
                    let w = sz / smax;
                    mk.weight += w;
                    mk.db_sum += dsp::db(v / sz);
                    mk.n += 1;
                    mk.bands[z] += w;
                }
            }
            // Trials.
            for (t, c) in trials.get(i).into_iter().flatten().enumerate() {
                let g = 10f64.powf(c.gain_db / 10.0);
                let mut s2 = s;
                s2.iter_mut().for_each(|v| *v *= g);
                let mut m2 = m;
                if let Some((u, cut)) = &c.masker {
                    let mut lost = [0.0; BARKS];
                    for z in 0..BARKS {
                        lost[z] = x.raw[*u][z] * (1.0 - cut[z]);
                    }
                    let le = ear.excite(&lost);
                    for z in 0..BARKS {
                        m2[z] = (m2[z] - le[z]).max(0.0);
                    }
                }
                let (al, pa) = ear.loudness(&s2, &m2);
                let r = &mut tried[i][t];
                r.0 += 1;
                if al >= AUDIBLE_SONES && pa >= theta * al {
                    r.1 += 1;
                }
            }
        }
    }
    for (i, a) in out.iter_mut().enumerate() {
        if a.active > 0 {
            a.alone /= a.active as f64;
            a.partial /= a.active as f64;
            for v in &mut a.spectrum {
                *v /= a.active as f64;
            }
        }
        let mut m = std::mem::take(&mut maskers[i]);
        m.sort_by(|x, y| y.weight.total_cmp(&x.weight));
        a.maskers = m;
    }
    let fractions = tried
        .into_iter()
        .map(|t| {
            t.into_iter()
                .map(|(n, k)| if n == 0 { 1.0 } else { k as f64 / n as f64 })
                .collect()
        })
        .collect();
    (out, fractions)
}
