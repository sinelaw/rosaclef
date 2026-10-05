//! SoundFont 2 files (and SF3: the same with Ogg Vorbis samples).
//!
//! The engine plays soundfont presets with the `soundfont` instrument, but it
//! never reads files or decodes audio on the audio thread. A host prepares a
//! [`LoadedPreset`]: it parses the soundfont ([`SoundFont::parse`]), resolves
//! a preset into regions and decodes their samples ([`SoundFont::load`]),
//! then hands the result to [`crate::Engine::set_preset`]. The browser does
//! this in a worker and passes the preset to the audio worklet as bytes
//! ([`LoadedPreset::to_bytes`]).
//!
//! # Split soundfonts
//!
//! A soundfont can be stored whole, or split for lazy loading by
//! `tools/split_soundfont.py`: an index (the file with an empty `smpl`
//! chunk) plus the sample data cut into [`PIECE`]-byte pieces. Sample
//! headers still point into the whole sample data, so
//! [`SoundFont::pieces`] tells which pieces a preset needs.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

/// Size of the sample-data pieces of a split soundfont.
pub const PIECE: usize = 1 << 20;

/// Number of SoundFont generators.
pub const GENS: usize = 61;

/// Generator numbers (SoundFont 2.04, section 8.1.2).
pub mod gen {
    pub const START_OFFSET: usize = 0;
    pub const END_OFFSET: usize = 1;
    pub const LOOP_START_OFFSET: usize = 2;
    pub const LOOP_END_OFFSET: usize = 3;
    pub const START_COARSE: usize = 4;
    pub const MOD_LFO_TO_PITCH: usize = 5;
    pub const VIB_LFO_TO_PITCH: usize = 6;
    pub const MOD_ENV_TO_PITCH: usize = 7;
    pub const FILTER_FC: usize = 8;
    pub const FILTER_Q: usize = 9;
    pub const MOD_LFO_TO_FILTER: usize = 10;
    pub const MOD_ENV_TO_FILTER: usize = 11;
    pub const END_COARSE: usize = 12;
    pub const MOD_LFO_TO_VOLUME: usize = 13;
    pub const PAN: usize = 17;
    pub const MOD_LFO_DELAY: usize = 21;
    pub const MOD_LFO_FREQ: usize = 22;
    pub const VIB_LFO_DELAY: usize = 23;
    pub const VIB_LFO_FREQ: usize = 24;
    pub const MOD_ENV_DELAY: usize = 25;
    pub const MOD_ENV_ATTACK: usize = 26;
    pub const MOD_ENV_HOLD: usize = 27;
    pub const MOD_ENV_DECAY: usize = 28;
    pub const MOD_ENV_SUSTAIN: usize = 29;
    pub const MOD_ENV_RELEASE: usize = 30;
    pub const KEY_TO_MOD_ENV_HOLD: usize = 31;
    pub const KEY_TO_MOD_ENV_DECAY: usize = 32;
    pub const VOL_ENV_DELAY: usize = 33;
    pub const VOL_ENV_ATTACK: usize = 34;
    pub const VOL_ENV_HOLD: usize = 35;
    pub const VOL_ENV_DECAY: usize = 36;
    pub const VOL_ENV_SUSTAIN: usize = 37;
    pub const VOL_ENV_RELEASE: usize = 38;
    pub const KEY_TO_VOL_ENV_HOLD: usize = 39;
    pub const KEY_TO_VOL_ENV_DECAY: usize = 40;
    pub const INSTRUMENT: usize = 41;
    pub const KEY_RANGE: usize = 43;
    pub const VEL_RANGE: usize = 44;
    pub const LOOP_START_COARSE: usize = 45;
    pub const KEYNUM: usize = 46;
    pub const VELOCITY: usize = 47;
    pub const ATTENUATION: usize = 48;
    pub const LOOP_END_COARSE: usize = 50;
    pub const COARSE_TUNE: usize = 51;
    pub const FINE_TUNE: usize = 52;
    pub const SAMPLE_ID: usize = 53;
    pub const SAMPLE_MODES: usize = 54;
    pub const SCALE_TUNING: usize = 56;
    pub const EXCLUSIVE_CLASS: usize = 57;
    pub const ROOT_KEY: usize = 58;
}

/// Default generator values.
fn defaults() -> [i32; GENS] {
    let mut g = [0i32; GENS];
    g[gen::FILTER_FC] = 13500;
    for i in [
        gen::MOD_LFO_DELAY,
        gen::VIB_LFO_DELAY,
        gen::MOD_ENV_DELAY,
        gen::MOD_ENV_ATTACK,
        gen::MOD_ENV_HOLD,
        gen::MOD_ENV_DECAY,
        gen::MOD_ENV_RELEASE,
        gen::VOL_ENV_DELAY,
        gen::VOL_ENV_ATTACK,
        gen::VOL_ENV_HOLD,
        gen::VOL_ENV_DECAY,
        gen::VOL_ENV_RELEASE,
    ] {
        g[i] = -12000;
    }
    g[gen::SCALE_TUNING] = 100;
    g[gen::KEYNUM] = -1;
    g[gen::VELOCITY] = -1;
    g[gen::ROOT_KEY] = -1;
    g
}

/// Generators that only make sense in instrument zones (ignored in presets).
fn instrument_only(g: usize) -> bool {
    matches!(
        g,
        gen::START_OFFSET
            | gen::END_OFFSET
            | gen::LOOP_START_OFFSET
            | gen::LOOP_END_OFFSET
            | gen::START_COARSE
            | gen::END_COARSE
            | gen::LOOP_START_COARSE
            | gen::LOOP_END_COARSE
            | gen::KEYNUM
            | gen::VELOCITY
            | gen::SAMPLE_MODES
            | gen::EXCLUSIVE_CLASS
            | gen::ROOT_KEY
    )
}

/// A modulator (SoundFont 2.04, section 8.2): `amount × source × amount
/// source` added to generator `dest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modulator {
    pub src: u16,
    pub dest: u16,
    pub amount: i16,
    pub amt_src: u16,
    pub trans: u16,
}

impl Modulator {
    /// Modulators with the same sources, destination and transform replace
    /// each other.
    fn same(&self, o: &Modulator) -> bool {
        self.src == o.src
            && self.dest == o.dest
            && self.amt_src == o.amt_src
            && self.trans == o.trans
    }

    /// The contribution to `dest` for a note (no controllers move here: CC
    /// sources read their usual default values).
    pub fn value(&self, key: u8, vel: u8) -> f32 {
        if self.src & 0x7f == 0 && self.src & 0x80 == 0 {
            return 0.0;
        }
        let v = self.amount as f32 * source(self.src, key, vel) * source(self.amt_src, key, vel);
        if self.trans == 2 {
            v.abs()
        } else {
            v
        }
    }
}

/// The default modulators that matter for notes: velocity to attenuation
/// and velocity to filter cutoff (the controller ones act on values the
/// engine handles itself: volume, expression, pan, pitch bend).
const DEFAULT_MODS: [Modulator; 2] = [
    Modulator {
        src: 0x0502,
        dest: gen::ATTENUATION as u16,
        amount: 960,
        amt_src: 0,
        trans: 0,
    },
    Modulator {
        src: 0x0102,
        dest: gen::FILTER_FC as u16,
        amount: -2400,
        amt_src: 0x0D02,
        trans: 0,
    },
];

fn concave(x: f32) -> f32 {
    if x >= 1.0 {
        1.0
    } else {
        (-(5.0 / 12.0) * (1.0 - x.max(0.0)).log10()).min(1.0)
    }
}

/// Value of a modulator source for a note, mapped by its curve: 0..1
/// (unipolar) or -1..1 (bipolar). "No controller" is 1.
fn source(src: u16, key: u8, vel: u8) -> f32 {
    let index = src & 0x7f;
    let raw = if src & 0x80 != 0 {
        match index {
            7 => 100.0,
            10 | 8 => 64.0,
            11 => 127.0,
            91 => 40.0,
            _ => 0.0,
        }
    } else {
        match index {
            0 => return 1.0,
            2 => vel as f32,
            3 => key as f32,
            14 => 64.0,
            16 => 2.0,
            _ => 0.0,
        }
    };
    let mut x = raw / 127.0;
    if src & 0x100 != 0 {
        x = 1.0 - x;
    }
    let curve = |x: f32| match src >> 10 {
        1 => concave(x),
        2 => 1.0 - concave(1.0 - x),
        3 => {
            if x >= 0.5 {
                1.0
            } else {
                0.0
            }
        }
        _ => x,
    };
    if src & 0x200 != 0 {
        if src >> 10 == 3 {
            if x >= 0.5 {
                1.0
            } else {
                -1.0
            }
        } else if x >= 0.5 {
            curve(2.0 * x - 1.0)
        } else {
            -curve(1.0 - 2.0 * x)
        }
    } else {
        curve(x)
    }
}

/// Add `m` to `list`, replacing an identical modulator.
fn put(list: &mut Vec<Modulator>, m: Modulator) {
    match list.iter_mut().find(|o| o.same(&m)) {
        Some(o) => *o = m,
        None => list.push(m),
    }
}

/// One zone: its generators (as written) and key/velocity ranges.
#[derive(Clone, Debug, Default)]
struct Zone {
    gens: Vec<(u16, i16)>,
    mods: Vec<Modulator>,
    keys: (u8, u8),
    vels: (u8, u8),
    /// Instrument (preset zones) or sample (instrument zones).
    link: Option<u16>,
}

fn range(amount: i16) -> (u8, u8) {
    let [lo, hi] = amount.to_le_bytes();
    (lo.min(127), hi.min(127))
}

impl Zone {
    fn new(gens: Vec<(u16, i16)>, mods: Vec<Modulator>, link_gen: usize) -> Zone {
        let mut z = Zone {
            keys: (0, 127),
            vels: (0, 127),
            ..Default::default()
        };
        for &(op, amount) in &gens {
            match op as usize {
                gen::KEY_RANGE => z.keys = range(amount),
                gen::VEL_RANGE => z.vels = range(amount),
                g if g == link_gen => z.link = Some(amount as u16),
                _ => {}
            }
        }
        z.gens = gens;
        z.mods = mods;
        z
    }
}

#[derive(Clone, Debug)]
pub struct PresetInfo {
    pub name: String,
    pub bank: u16,
    pub program: u16,
    zones: Vec<Zone>,
}

#[derive(Clone, Debug)]
struct InstrumentInfo {
    zones: Vec<Zone>,
}

#[derive(Clone, Debug)]
pub struct SampleHeader {
    pub name: String,
    /// Frames (SF2) or bytes of Ogg data (SF3) in the sample data.
    pub start: u32,
    pub end: u32,
    /// Absolute frames (SF2); frames from the sample start (SF3).
    pub loop_start: u32,
    pub loop_end: u32,
    pub rate: u32,
    pub root: u8,
    pub correction: i8,
    pub kind: u16,
}

impl SampleHeader {
    /// Ogg Vorbis compressed (SF3).
    pub fn compressed(&self) -> bool {
        self.kind & 0x10 != 0
    }
    /// ROM samples are not in the file.
    pub fn rom(&self) -> bool {
        self.kind & 0x8000 != 0
    }
    /// Byte range in the sample data.
    pub fn bytes(&self) -> (usize, usize) {
        if self.compressed() {
            (self.start as usize, self.end as usize)
        } else {
            (self.start as usize * 2, self.end as usize * 2)
        }
    }
}

/// A parsed soundfont: presets, instruments and sample headers, plus the
/// sample data when the file holds it.
#[derive(Clone, Debug)]
pub struct SoundFont {
    pub presets: Vec<PresetInfo>,
    instruments: Vec<InstrumentInfo>,
    pub samples: Vec<SampleHeader>,
    /// The `smpl` chunk (empty for the index of a split soundfont).
    pub data: Vec<u8>,
}

/// A decoded sample, ready to play.
#[derive(Debug)]
pub struct FontSample {
    pub rate: u32,
    pub root: u8,
    pub correction: i8,
    /// Loop, in frames of `data`.
    pub loop_start: u32,
    pub loop_end: u32,
    pub data: Vec<i16>,
}

/// A preset zone combined with an instrument zone: which sample plays over
/// which keys and velocities, and the summed generator values.
#[derive(Clone, Debug)]
pub struct Region {
    pub keys: (u8, u8),
    pub vels: (u8, u8),
    /// Index into [`LoadedPreset::samples`].
    pub sample: u32,
    pub gens: [i32; GENS],
    pub mods: Vec<Modulator>,
}

impl Region {
    pub fn get(&self, g: usize) -> i32 {
        self.gens[g]
    }
}

/// A preset ready to play: its regions and their decoded samples.
#[derive(Debug, Default)]
pub struct LoadedPreset {
    pub name: String,
    pub regions: Vec<Region>,
    pub samples: Vec<Arc<FontSample>>,
}

// ------------------------------------------------------------------ parsing

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn name(b: &[u8]) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

/// Sub-chunks of a RIFF list body: (id, body).
fn chunks(b: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = vec![];
    let mut i = 0;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32le(b, i + 4) as usize;
        let start = i + 8;
        let end = start.saturating_add(len).min(b.len());
        out.push((id, &b[start..end]));
        i = start.saturating_add(len + (len & 1));
    }
    out
}

/// Records of a pdta sub-chunk, without the terminal record.
fn records(b: &[u8], size: usize) -> Vec<&[u8]> {
    b.chunks_exact(size).collect()
}

impl SoundFont {
    /// Parse a soundfont (a whole SF2/SF3 file, or the index of a split one).
    pub fn parse(bytes: &[u8]) -> Result<SoundFont, String> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"sfbk" {
            return Err("not a SoundFont file".into());
        }
        let mut pdta: HashMap<&[u8], &[u8]> = HashMap::new();
        let mut data = vec![];
        for (id, body) in chunks(&bytes[12..]) {
            if id != b"LIST" || body.len() < 4 {
                continue;
            }
            let kind = &body[..4];
            for (id2, b2) in chunks(&body[4..]) {
                match kind {
                    b"sdta" if id2 == b"smpl" => data = b2.to_vec(),
                    b"pdta" => {
                        pdta.insert(id2, b2);
                    }
                    _ => {}
                }
            }
        }
        let get = |id: &[u8], size: usize| -> Result<Vec<&[u8]>, String> {
            let b = pdta
                .get(id)
                .ok_or_else(|| format!("missing {} chunk", String::from_utf8_lossy(id)))?;
            let r = records(b, size);
            if r.is_empty() {
                return Err(format!("empty {} chunk", String::from_utf8_lossy(id)));
            }
            Ok(r)
        };
        let phdr = get(b"phdr", 38)?;
        let pbag = get(b"pbag", 4)?;
        let pmod = get(b"pmod", 10).unwrap_or_default();
        let pgen = get(b"pgen", 4)?;
        let inst = get(b"inst", 22)?;
        let ibag = get(b"ibag", 4)?;
        let imod = get(b"imod", 10).unwrap_or_default();
        let igen = get(b"igen", 4)?;
        let shdr = get(b"shdr", 46)?;

        // Zones of the bags [from, to).
        let zones = |bags: &[&[u8]],
                     gens: &[&[u8]],
                     mods: &[&[u8]],
                     from: usize,
                     to: usize,
                     link: usize| {
            let mut out = vec![];
            for b in from..to.min(bags.len().saturating_sub(1)) {
                let g0 = u16le(bags[b], 0) as usize;
                let g1 = (u16le(bags[b + 1], 0) as usize).min(gens.len());
                let list: Vec<(u16, i16)> = (g0.min(g1)..g1)
                    .map(|g| (u16le(gens[g], 0), u16le(gens[g], 2) as i16))
                    .collect();
                let m0 = u16le(bags[b], 2) as usize;
                let m1 = (u16le(bags[b + 1], 2) as usize).min(mods.len());
                let mlist: Vec<Modulator> = (m0.min(m1)..m1)
                    .map(|m| {
                        let r = mods[m];
                        Modulator {
                            src: u16le(r, 0),
                            dest: u16le(r, 2),
                            amount: u16le(r, 4) as i16,
                            amt_src: u16le(r, 6),
                            trans: u16le(r, 8),
                        }
                    })
                    .collect();
                out.push(Zone::new(list, mlist, link));
            }
            out
        };
        let presets = (0..phdr.len() - 1)
            .map(|i| PresetInfo {
                name: name(&phdr[i][0..20]),
                program: u16le(phdr[i], 20),
                bank: u16le(phdr[i], 22),
                zones: zones(
                    &pbag,
                    &pgen,
                    &pmod,
                    u16le(phdr[i], 24) as usize,
                    u16le(phdr[i + 1], 24) as usize,
                    gen::INSTRUMENT,
                ),
            })
            .collect();
        let instruments = (0..inst.len() - 1)
            .map(|i| InstrumentInfo {
                zones: zones(
                    &ibag,
                    &igen,
                    &imod,
                    u16le(inst[i], 20) as usize,
                    u16le(inst[i + 1], 20) as usize,
                    gen::SAMPLE_ID,
                ),
            })
            .collect();
        let samples = shdr[..shdr.len() - 1]
            .iter()
            .map(|s| SampleHeader {
                name: name(&s[0..20]),
                start: u32le(s, 20),
                end: u32le(s, 24),
                loop_start: u32le(s, 28),
                loop_end: u32le(s, 32),
                rate: u32le(s, 36),
                root: s[40],
                correction: s[41] as i8,
                kind: u16le(s, 44),
            })
            .collect();
        Ok(SoundFont {
            presets,
            instruments,
            samples,
            data,
        })
    }

    /// Whether the sample data is in separate pieces.
    pub fn is_split(&self) -> bool {
        self.data.is_empty()
    }

    /// The preset for a bank and program. A missing drum kit falls back to
    /// kit 0, a missing melodic program to bank 0.
    pub fn find(&self, bank: u16, program: u16) -> Option<usize> {
        let at = |b: u16, p: u16| {
            self.presets
                .iter()
                .position(|x| x.bank == b && x.program == p)
        };
        at(bank, program)
            .or_else(|| {
                if bank == 128 {
                    at(128, 0)
                } else {
                    at(0, program)
                }
            })
            .or_else(|| (!self.presets.is_empty()).then_some(0))
    }

    /// Resolve a preset into regions; `Region::sample` is a sample index of
    /// the soundfont here.
    pub fn regions(&self, preset: usize) -> Vec<Region> {
        let Some(p) = self.presets.get(preset) else {
            return vec![];
        };
        let mut out = vec![];
        let (pglobal, pzones) = split_global(&p.zones);
        for pz in pzones {
            let Some(inst) = pz.link.and_then(|i| self.instruments.get(i as usize)) else {
                continue;
            };
            let (iglobal, izones) = split_global(&inst.zones);
            for iz in izones {
                let Some(sample) = iz.link else { continue };
                let Some(header) = self.samples.get(sample as usize) else {
                    continue;
                };
                if header.rom() {
                    continue;
                }
                let keys = intersect(&[
                    pglobal.map(|z| z.keys),
                    Some(pz.keys),
                    iglobal.map(|z| z.keys),
                    Some(iz.keys),
                ]);
                let vels = intersect(&[
                    pglobal.map(|z| z.vels),
                    Some(pz.vels),
                    iglobal.map(|z| z.vels),
                    Some(iz.vels),
                ]);
                let (Some(keys), Some(vels)) = (keys, vels) else {
                    continue;
                };
                let mut g = defaults();
                for z in iglobal.into_iter().chain([iz]) {
                    for &(op, amount) in &z.gens {
                        if (op as usize) < GENS {
                            g[op as usize] = amount as i32;
                        }
                    }
                }
                // Preset generators add to the instrument's (a zone's own
                // value replaces the preset global one).
                let mut add = [0i32; GENS];
                for z in pglobal.into_iter().chain([pz]) {
                    for &(op, amount) in &z.gens {
                        let op = op as usize;
                        if op < GENS
                            && !instrument_only(op)
                            && !matches!(op, gen::KEY_RANGE | gen::VEL_RANGE | gen::INSTRUMENT)
                        {
                            add[op] = amount as i32;
                        }
                    }
                }
                for i in 0..GENS {
                    if !matches!(i, gen::KEY_RANGE | gen::VEL_RANGE | gen::SAMPLE_ID) {
                        g[i] += add[i];
                    }
                }
                // Modulators: defaults, replaced by the instrument's (its
                // zone's over its global zone's); the preset's add on top.
                let mut mods = DEFAULT_MODS.to_vec();
                for z in iglobal.into_iter().chain([iz]) {
                    for m in &z.mods {
                        put(&mut mods, *m);
                    }
                }
                let mut pmods = vec![];
                for z in pglobal.into_iter().chain([pz]) {
                    for m in &z.mods {
                        put(&mut pmods, *m);
                    }
                }
                mods.extend(pmods);
                mods.retain(|m| m.amount != 0 && (m.dest as usize) < GENS);
                out.push(Region {
                    keys,
                    vels,
                    sample: sample as u32,
                    gens: g,
                    mods,
                });
            }
        }
        out
    }

    /// The pieces of a split soundfont that hold a preset's samples.
    pub fn pieces(&self, preset: usize) -> Vec<usize> {
        self.pieces_except(preset, &[])
    }

    /// The pieces holding a preset's samples, except samples in `have`.
    pub fn pieces_except(&self, preset: usize, have: &[u32]) -> Vec<usize> {
        let mut out: Vec<usize> = vec![];
        for r in self.regions(preset) {
            if have.contains(&r.sample) {
                continue;
            }
            let (a, b) = self.samples[r.sample as usize].bytes();
            if b > a {
                for k in a / PIECE..=(b - 1) / PIECE {
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// The bytes of sample `index`, from the file's own data or from pieces.
    fn sample_bytes<'a>(
        &'a self,
        index: usize,
        pieces: &'a dyn Pieces,
    ) -> Result<std::borrow::Cow<'a, [u8]>, String> {
        let h = &self.samples[index];
        let (a, b) = h.bytes();
        if b < a {
            return Err(format!("sample {:?} has a bad range", h.name));
        }
        if !self.is_split() {
            return self
                .data
                .get(a..b)
                .map(std::borrow::Cow::Borrowed)
                .ok_or_else(|| format!("sample {:?} lies outside the sample data", h.name));
        }
        let mut out = Vec::with_capacity(b - a);
        let mut pos = a;
        while pos < b {
            let k = pos / PIECE;
            let p = pieces
                .piece(k)
                .ok_or_else(|| format!("sample piece {k} is not loaded"))?;
            let from = pos - k * PIECE;
            let to = (b - k * PIECE).min(PIECE).min(p.len());
            if to <= from {
                return Err(format!("sample piece {k} is too short"));
            }
            out.extend_from_slice(&p[from..to]);
            pos = k * PIECE + to;
        }
        Ok(std::borrow::Cow::Owned(out))
    }

    /// Decode sample `index`.
    pub fn decode(&self, index: usize, pieces: &dyn Pieces) -> Result<FontSample, String> {
        let h = &self.samples[index];
        let bytes = self.sample_bytes(index, pieces)?;
        let (data, loop_start, loop_end) = if h.compressed() {
            (decode_ogg(&bytes)?, h.loop_start, h.loop_end)
        } else {
            let data: Vec<i16> = bytes
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            (
                data,
                h.loop_start.saturating_sub(h.start),
                h.loop_end.saturating_sub(h.start),
            )
        };
        let len = data.len() as u32;
        Ok(FontSample {
            rate: h.rate.clamp(400, 384_000),
            root: h.root,
            correction: h.correction,
            loop_start: loop_start.min(len),
            loop_end: loop_end.min(len),
            data,
        })
    }

    /// Decode samples, on every core when built with `parallel`.
    fn decode_all(&self, samples: &[u32], pieces: &dyn Pieces) -> Vec<Result<FontSample, String>> {
        let decode = |i: usize| self.decode(samples[i] as usize, pieces);
        #[cfg(feature = "parallel")]
        {
            use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
            use std::sync::Mutex;
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
            let threads = cores.min(samples.len());
            if threads > 1 {
                let next = AtomicUsize::new(0);
                let out: Mutex<Vec<Option<Result<FontSample, String>>>> =
                    Mutex::new((0..samples.len()).map(|_| None).collect());
                std::thread::scope(|scope| {
                    for _ in 0..threads {
                        scope.spawn(|| loop {
                            let i = next.fetch_add(1, Relaxed);
                            if i >= samples.len() {
                                break;
                            }
                            let r = decode(i);
                            out.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
                        });
                    }
                });
                return out
                    .into_inner()
                    .unwrap_or_else(|e| e.into_inner())
                    .into_iter()
                    .map(|r| r.expect("decoded"))
                    .collect();
            }
        }
        (0..samples.len()).map(decode).collect()
    }

    /// Resolve a preset and decode its samples. `cache` keeps decoded
    /// samples by index so presets that share samples share memory.
    pub fn load(
        &self,
        preset: usize,
        pieces: &dyn Pieces,
        cache: &mut HashMap<u32, Arc<FontSample>>,
    ) -> Result<LoadedPreset, String> {
        let mut regions = self.regions(preset);
        // Decode the samples not in the cache, in region order (so the first
        // error is the same as one at a time).
        let mut todo: Vec<u32> = vec![];
        for r in &regions {
            if !cache.contains_key(&r.sample) && !todo.contains(&r.sample) {
                todo.push(r.sample);
            }
        }
        for (i, s) in todo.iter().zip(self.decode_all(&todo, pieces)) {
            cache.insert(*i, Arc::new(s?));
        }
        let mut samples = vec![];
        let mut local: HashMap<u32, u32> = HashMap::new();
        for r in &mut regions {
            let idx = match local.get(&r.sample) {
                Some(i) => *i,
                None => {
                    samples.push(cache[&r.sample].clone());
                    local.insert(r.sample, samples.len() as u32 - 1);
                    samples.len() as u32 - 1
                }
            };
            r.sample = idx;
        }
        Ok(LoadedPreset {
            name: self.presets[preset].name.clone(),
            regions,
            samples,
        })
    }
}

/// The loaded pieces of a split soundfont.
pub trait Pieces: Sync {
    fn piece(&self, k: usize) -> Option<&[u8]>;
}

/// No pieces (a whole soundfont file).
impl Pieces for () {
    fn piece(&self, _k: usize) -> Option<&[u8]> {
        None
    }
}

impl Pieces for HashMap<usize, Vec<u8>> {
    fn piece(&self, k: usize) -> Option<&[u8]> {
        self.get(&k).map(|v| v.as_slice())
    }
}

fn split_global(zones: &[Zone]) -> (Option<&Zone>, &[Zone]) {
    match zones.first() {
        Some(z) if z.link.is_none() => (Some(z), &zones[1..]),
        _ => (None, zones),
    }
}

fn intersect(ranges: &[Option<(u8, u8)>]) -> Option<(u8, u8)> {
    let mut lo = 0u8;
    let mut hi = 127u8;
    for (a, b) in ranges.iter().flatten() {
        lo = lo.max(*a);
        hi = hi.min(*b);
    }
    (lo <= hi).then_some((lo, hi))
}

/// Decode an Ogg Vorbis stream to 16-bit mono (the first channel).
fn decode_ogg(bytes: &[u8]) -> Result<Vec<i16>, String> {
    let mut r = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
        .map_err(|e| format!("Ogg Vorbis: {e}"))?;
    let ch = r.ident_hdr.audio_channels.max(1) as usize;
    let mut out = vec![];
    loop {
        match r.read_dec_packet_itl() {
            Ok(Some(pkt)) => out.extend(pkt.iter().step_by(ch)),
            Ok(None) => break,
            Err(e) => return Err(format!("Ogg Vorbis: {e}")),
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ bytes

const MAGIC: &[u8; 4] = b"RCP2";
/// Largest preset accepted from bytes, in sample frames (1 GB of audio).
const MAX_FRAMES: usize = 1 << 29;

impl LoadedPreset {
    /// Everything but the sample data: name, regions and the samples'
    /// rates, loops and lengths.
    pub fn header(&self) -> Vec<u8> {
        let mut out = vec![];
        out.extend_from_slice(MAGIC);
        let name = self.name.as_bytes();
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&(self.regions.len() as u32).to_le_bytes());
        for r in &self.regions {
            out.extend_from_slice(&[r.keys.0, r.keys.1, r.vels.0, r.vels.1]);
            out.extend_from_slice(&r.sample.to_le_bytes());
            for g in r.gens {
                out.extend_from_slice(&g.to_le_bytes());
            }
            out.extend_from_slice(&(r.mods.len() as u32).to_le_bytes());
            for m in &r.mods {
                for v in [m.src, m.dest, m.amount as u16, m.amt_src, m.trans] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        out.extend_from_slice(&(self.samples.len() as u32).to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&s.rate.to_le_bytes());
            out.extend_from_slice(&[s.root, s.correction as u8, 0, 0]);
            out.extend_from_slice(&s.loop_start.to_le_bytes());
            out.extend_from_slice(&s.loop_end.to_le_bytes());
            out.extend_from_slice(&(s.data.len() as u32).to_le_bytes());
        }
        out
    }

    /// The header followed by every sample's data (16-bit little endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.header();
        for s in &self.samples {
            for v in &s.data {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<LoadedPreset, String> {
        let (mut builder, mut at) = PresetBuilder::from_header(b)?;
        for i in 0..builder.samples.len() {
            let data = builder.sample_mut(i).expect("in range");
            let n = data.len() * 2;
            let bytes = b.get(at..at + n).ok_or("corrupt preset data")?;
            for (d, c) in data.iter_mut().zip(bytes.chunks_exact(2)) {
                *d = i16::from_le_bytes([c[0], c[1]]);
            }
            at += n;
        }
        Ok(builder.finish())
    }
}

/// A preset being received: the header is parsed, then the data is written
/// into each sample's buffer (allocated on first use, so a receiver can take
/// a large preset in small steps), then [`PresetBuilder::finish`].
pub struct PresetBuilder {
    name: String,
    regions: Vec<Region>,
    samples: Vec<FontSample>,
    lens: Vec<usize>,
}

impl PresetBuilder {
    /// Parse a header; returns the builder and the header's length.
    pub fn from_header(b: &[u8]) -> Result<(PresetBuilder, usize), String> {
        let bad = || "corrupt preset data".to_string();
        let mut i = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let s = b
                .get(i..i.checked_add(n).ok_or_else(bad)?)
                .ok_or_else(bad)?;
            i += n;
            Ok(s)
        };
        if take(4)? != MAGIC {
            return Err(bad());
        }
        let n = u32le(take(4)?, 0) as usize;
        let name = String::from_utf8_lossy(take(n)?).to_string();
        let nr = u32le(take(4)?, 0) as usize;
        let mut regions = Vec::with_capacity(nr.min(1 << 16));
        for _ in 0..nr {
            let h = take(8)?;
            let mut gens = [0i32; GENS];
            let g = take(GENS * 4)?;
            for (k, v) in gens.iter_mut().enumerate() {
                *v = u32le(g, k * 4) as i32;
            }
            let nm = u32le(take(4)?, 0) as usize;
            let mut mods = Vec::with_capacity(nm.min(256));
            for _ in 0..nm {
                let m = take(10)?;
                let dest = u16le(m, 2);
                if dest as usize >= GENS {
                    return Err(bad());
                }
                mods.push(Modulator {
                    src: u16le(m, 0),
                    dest,
                    amount: u16le(m, 4) as i16,
                    amt_src: u16le(m, 6),
                    trans: u16le(m, 8),
                });
            }
            regions.push(Region {
                keys: (h[0], h[1]),
                vels: (h[2], h[3]),
                sample: u32le(h, 4),
                gens,
                mods,
            });
        }
        let ns = u32le(take(4)?, 0) as usize;
        let mut samples = Vec::with_capacity(ns.min(1 << 16));
        let mut lens = Vec::with_capacity(ns.min(1 << 16));
        let mut total = 0usize;
        for _ in 0..ns {
            let h = take(20)?;
            let len = u32le(h, 16) as usize;
            total += len;
            if total > MAX_FRAMES {
                return Err("the preset is too large".into());
            }
            samples.push(FontSample {
                rate: u32le(h, 0).clamp(400, 384_000),
                root: h[4],
                correction: h[5] as i8,
                loop_start: u32le(h, 8).min(len as u32),
                loop_end: u32le(h, 12).min(len as u32),
                data: vec![],
            });
            lens.push(len);
        }
        if regions.iter().any(|r| r.sample as usize >= samples.len()) {
            return Err(bad());
        }
        Ok((
            PresetBuilder {
                name,
                regions,
                samples,
                lens,
            },
            i,
        ))
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// The buffer of sample `i`, to fill with its data.
    pub fn sample_mut(&mut self, i: usize) -> Option<&mut [i16]> {
        let len = *self.lens.get(i)?;
        let s = &mut self.samples[i];
        if s.data.len() != len {
            s.data = vec![0; len];
        }
        Some(s.data.as_mut_slice())
    }

    pub fn finish(self) -> LoadedPreset {
        LoadedPreset {
            name: self.name,
            regions: self.regions,
            samples: self.samples.into_iter().map(Arc::new).collect(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny SF2: one preset (bank 0, program 0) with one instrument zone
    /// playing a looped 440 Hz sine recorded at root key 69.
    pub fn tiny_sf2() -> Vec<u8> {
        let rate = 22050u32;
        let frames = 2205u32; // 44 periods... 0.1 s
        let mut pcm = vec![];
        for i in 0..frames {
            let v = (2.0 * std::f64::consts::PI * 440.0 * i as f64 / rate as f64).sin();
            pcm.extend_from_slice(&((v * 20000.0) as i16).to_le_bytes());
        }
        pcm.extend(std::iter::repeat_n(0u8, 92));
        let chunk = |id: &[u8], body: &[u8]| {
            let mut c = id.to_vec();
            c.extend((body.len() as u32).to_le_bytes());
            c.extend_from_slice(body);
            if body.len() & 1 == 1 {
                c.push(0);
            }
            c
        };
        let name20 = |s: &str| {
            let mut n = s.as_bytes().to_vec();
            n.resize(20, 0);
            n
        };
        let mut phdr = vec![];
        for (n, bag) in [("Sine", 0u16), ("EOP", 1)] {
            phdr.extend(name20(n));
            phdr.extend([0u8; 4]); // program 0, bank 0
            phdr.extend(bag.to_le_bytes());
            phdr.extend([0u8; 12]);
        }
        let pbag = [0u16, 0, 1, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let pgen = [41u16, 0, 0, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let mut inst = name20("Sine");
        inst.extend(0u16.to_le_bytes());
        inst.extend(name20("EOI"));
        inst.extend(1u16.to_le_bytes());
        let ibag = [0u16, 0, 3, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        // sampleModes 1 (loop), release 0.2 s (-2786 tc), sampleID 0.
        let igen = [54u16, 1, 38, (-2786i16) as u16, 53, 0, 0, 0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let mut shdr = name20("sine");
        for v in [0u32, frames, 0, frames, rate] {
            shdr.extend(v.to_le_bytes());
        }
        shdr.extend([69u8, 0]);
        shdr.extend(0u16.to_le_bytes());
        shdr.extend(1u16.to_le_bytes());
        shdr.extend(name20("EOS"));
        shdr.extend([0u8; 26]);
        let mut pdta = b"pdta".to_vec();
        for (id, body) in [
            (&b"phdr"[..], phdr),
            (b"pbag", pbag),
            (b"pmod", vec![0; 10]),
            (b"pgen", pgen),
            (b"inst", inst),
            (b"ibag", ibag),
            (b"imod", vec![0; 10]),
            (b"igen", igen),
            (b"shdr", shdr),
        ] {
            pdta.extend(chunk(id, &body));
        }
        let mut sdta = b"sdta".to_vec();
        sdta.extend(chunk(b"smpl", &pcm));
        let mut info = b"INFO".to_vec();
        info.extend(chunk(b"ifil", &[2, 0, 1, 0]));
        let mut body = b"sfbk".to_vec();
        body.extend(chunk(b"LIST", &info));
        body.extend(chunk(b"LIST", &sdta));
        body.extend(chunk(b"LIST", &pdta));
        chunk(b"RIFF", &body)
    }

    #[test]
    fn parses_and_loads_a_preset() {
        let sf = SoundFont::parse(&tiny_sf2()).unwrap();
        assert_eq!(sf.presets.len(), 1);
        assert_eq!(sf.presets[0].name, "Sine");
        let p = sf.find(0, 0).unwrap();
        let regions = sf.regions(p);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].get(gen::SAMPLE_MODES), 1);
        assert_eq!(regions[0].get(gen::VOL_ENV_RELEASE), -2786);
        assert_eq!(
            regions[0].get(gen::FILTER_FC),
            13500,
            "defaults fill the rest"
        );
        let loaded = sf.load(p, &(), &mut HashMap::new()).unwrap();
        assert_eq!(loaded.samples[0].data.len(), 2205);
        assert_eq!(loaded.samples[0].loop_end, 2205);

        let back = LoadedPreset::from_bytes(&loaded.to_bytes()).unwrap();
        assert_eq!(back.name, "Sine");
        assert_eq!(back.regions[0].gens, loaded.regions[0].gens);
        assert_eq!(back.regions[0].mods, loaded.regions[0].mods);
        assert_eq!(loaded.regions[0].mods.len(), 2, "the default modulators");
        let vel_atten = loaded.regions[0].mods[0];
        assert_eq!(vel_atten.value(60, 127), 0.0);
        assert!(
            (vel_atten.value(60, 64) - 119.0).abs() < 2.0,
            "about -12 dB at half velocity"
        );
        assert_eq!(back.samples[0].data, loaded.samples[0].data);
        assert!(LoadedPreset::from_bytes(&loaded.to_bytes()[..100]).is_err());
    }
}
