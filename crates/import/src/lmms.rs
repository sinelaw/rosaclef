//! LMMS project import (`.mmp` XML, `.mmpz` compressed XML).
//!
//! ## Time
//! LMMS counts **192 ticks per 4/4 bar**, i.e. 48 ticks per quarter note,
//! whatever the time signature (a 3/4 bar is 144 ticks). Rosaclef counts in
//! quarter-note beats, so `beats = ticks / 48`.
//!
//! ## Pitch
//! LMMS numbers keys from C0 = 0, so its default key 57 is A4 — the note an
//! instrument track with the default base note (57) plays at 440 Hz. MIDI 69
//! is A4, hence `midi = key + 12`. A track's base note transposes playback
//! (`key == basenote` sounds as A4) and the project's master pitch shifts
//! everything, so in full: `midi = key + 12 + (57 − basenote) + masterpitch`
//! — the same offset LMMS's own MIDI exporter applies.
//!
//! ## Pattern length
//! LMMS 1.x does not save a pattern's length: a melody pattern spans its
//! notes rounded up to whole bars (at least one), a beat pattern its
//! `steps` (16 per 4/4 bar). Newer versions save `len`, which wins.
//!
//! ## Mapping
//! | LMMS | Rosaclef |
//! |---|---|
//! | `head` bpm, time signature | `transport` |
//! | instrument track (`type="0"`) | channel + one pattern per distinct LMMS pattern, placed as clips |
//! | Beat+Bassline track (`type="1"`) | one multi-channel pattern per B&B, a clip wherever its `bbtco` appears |
//! | sample track (`type="2"`) | audio clips (files copied into `samples/`) |
//! | automation tracks (`type="5"`, `"6"`) | lanes for tempo, track volume / pan, FX channel and master volume |
//! | FX mixer channels | mixer inserts (name, volume, mute, effects) |
//! | track effects | prepended to the track's FX channel when it is the only user, else a new insert |
//! | arpeggiator | the channel's `arp` (the notes stay as written) |
//! | chord stacking | written out as notes |
//! | TripleOscillator | `analog` (clean: no drift or drive, filter from the track's) |
//! | Kicker | `drum` kick |
//! | AudioFileProcessor | `sampler` (Vault Sampler); DrumSynth `.ds` patches become `drum` voices |
//! | LB302 | `analog` acid bass (ladder / screamer filter, mono, legato glide) |
//! | Sf2 Player | `soundfont` (Grand Orchestra) playing the same General MIDI patch |
//! | OpulenZ (OPL2) | `fm` with the two-operator `duo` algorithm |
//! | Mallets | `fm` "Crystal Mallet" |
//! | NES, BitInvader | `analog` with the nearest waveforms |
//! | LADSPA / LV2 effects | the built-in effect of the same family (reverb, delay, chorus, ...) |
//! | other instruments | `analog` fallback, with a warning |
//!
//! Instrument parameter automation, per-clip mutes, sends between FX
//! channels and unsupported plugins are skipped with a warning.

use crate::{
    beats, clamp, clean_va, color, ensure_valid, has_instrument, pad_tracks, set_option, set_param,
    va_gain, Ids, Imported, SampleCopy, Warnings, MAX_INSERTS,
};
use anyhow::{anyhow, bail, Context, Result};
use rosaclef_core::automation::AutomationTarget;
use rosaclef_core::{
    Arpeggio, AutomationLane, AutomationPoint, Channel, Clip, Device, Insert, InsertIx, Note,
    Pattern, Project, Track, TrackIx,
};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;
use std::io::Read;
use std::path::{Path, PathBuf};

/// LMMS ticks per quarter note (192 per 4/4 bar).
pub const TICKS_PER_BEAT: f64 = 48.0;
/// `midi = key + KEY_TO_MIDI` at the default base note.
pub const KEY_TO_MIDI: i32 = 12;
/// LMMS's default base note (A4).
pub const DEFAULT_BASE_NOTE: i32 = 57;
/// Length of a step note (`len <= 0` in Beat+Bassline patterns): one 16th.
const STEP_BEATS: f64 = 0.25;

/// Import settings.
#[derive(Clone, Debug)]
pub struct Options {
    /// Title of the new project.
    pub title: String,
    /// Folder of the `.mmp` file: relative sample paths are resolved against it.
    pub source_dir: Option<PathBuf>,
    /// LMMS sample libraries to search for relative sample paths
    /// (factory samples such as `drums/kick01.ogg`).
    pub sample_dirs: Vec<PathBuf>,
}

impl Options {
    pub fn new(title: &str) -> Options {
        Options {
            title: title.to_string(),
            source_dir: None,
            sample_dirs: default_sample_dirs(),
        }
    }
}

/// Where LMMS keeps its factory and user samples on common installs.
pub fn default_sample_dirs() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("/usr/share/lmms/samples"),
        PathBuf::from("/usr/local/share/lmms/samples"),
    ];
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        v.push(home.join("lmms/samples"));
        v.push(home.join("Documents/lmms/samples"));
    }
    v
}

/// Decode a project file: plain XML (`.mmp`) or Qt `qCompress` data
/// (`.mmpz`: a 4-byte big-endian uncompressed length, then a zlib stream).
pub fn decode(bytes: &[u8]) -> Result<String> {
    let trimmed = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let first = trimmed.iter().find(|b| !b.is_ascii_whitespace()).copied();
    if first == Some(b'<') {
        return String::from_utf8(trimmed.to_vec()).context("the project file is not valid UTF-8");
    }
    if bytes.len() < 6 {
        bail!("not an LMMS project (file too short)");
    }
    let expected = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let mut out = Vec::with_capacity(expected.min(256 << 20));
    flate2::read::ZlibDecoder::new(&bytes[4..])
        .take(512 << 20)
        .read_to_end(&mut out)
        .map_err(|e| anyhow!("not an LMMS project (neither XML nor qCompress data): {e}"))?;
    let text = String::from_utf8(out).context("the decompressed project is not valid UTF-8")?;
    if !text.trim_start().starts_with('<') {
        bail!("not an LMMS project (decompressed data is not XML)");
    }
    Ok(text)
}

/// Compress XML the way LMMS writes `.mmpz` (Qt `qCompress`).
pub fn encode_mmpz(xml: &str) -> Vec<u8> {
    use std::io::Write;
    let mut out = (xml.len() as u32).to_be_bytes().to_vec();
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(xml.as_bytes()).expect("in-memory write");
    out.extend(enc.finish().expect("in-memory write"));
    out
}

/// Import an `.mmp` / `.mmpz` project.
pub fn import(bytes: &[u8], opts: &Options) -> Result<Imported> {
    let text = decode(bytes)?;
    let doc = Document::parse_with_options(
        &text,
        ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .context("parsing the LMMS project XML")?;
    let root = doc.root_element();
    if root.tag_name().name() != "lmms-project" {
        bail!(
            "not an LMMS project (root element <{}>)",
            root.tag_name().name()
        );
    }
    let mut im = Importer::new(opts);
    im.run(root)?;
    let Importer {
        project,
        warn,
        samples,
        ..
    } = im;
    ensure_valid(&project)?;
    Ok(Imported {
        project,
        samples,
        warnings: warn.finish(),
    })
}

// ---------------------------------------------------------------- XML helpers

fn is(n: &Node, names: &[&str]) -> bool {
    n.is_element() && names.contains(&n.tag_name().name())
}

fn child<'a, 'i>(n: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is(c, names))
}

fn children<'a, 'i>(
    n: Node<'a, 'i>,
    names: &'static [&'static str],
) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| is(c, names))
}

/// A numeric setting: an attribute, or (when automated) a child element
/// with a `value` attribute.
fn num(n: Node, name: &str) -> Option<f64> {
    if let Some(v) = n.attribute(name) {
        return v.trim().parse().ok();
    }
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
        .and_then(|c| c.attribute("value"))
        .and_then(|v| v.trim().parse().ok())
}

fn num_or(n: Node, name: &str, d: f64) -> f64 {
    num(n, name).filter(|v| v.is_finite()).unwrap_or(d)
}

fn flag(n: Node, name: &str) -> bool {
    num(n, name).map(|v| v != 0.0).unwrap_or(false)
}

fn text<'a>(n: Node<'a, '_>, name: &str) -> &'a str {
    n.attribute(name).unwrap_or("")
}

/// The plugin a LADSPA / LV2 / VST effect hosts, from its `<key>`
/// (e.g. "calf Reverb", "tap_limiter").
fn plugin_name(effect: Node) -> String {
    let Some(key) = child(effect, &["key"]) else {
        return String::new();
    };
    let attr = |n: &str| {
        children(key, &["attribute"])
            .find(|a| text(*a, "name") == n)
            .map(|a| text(a, "value").to_string())
            .unwrap_or_default()
    };
    let (file, plugin) = (attr("file"), attr("plugin"));
    let uri = attr("uri");
    if !plugin.is_empty() && !file.is_empty() && !plugin.contains(&file) {
        format!("{file} {plugin}")
    } else if !plugin.is_empty() {
        plugin
    } else if !uri.is_empty() {
        uri
    } else {
        file
    }
}

/// A LADSPA plugin's control values by port number (left channel):
/// `<port011 data="1"/>` is channel 0, port 11.
fn ladspa_ports(effect: Node) -> HashMap<u32, f64> {
    let Some(controls) = child(effect, &["ladspacontrols"]) else {
        return HashMap::new();
    };
    controls
        .children()
        .filter(|c| c.is_element())
        .filter_map(|c| {
            let rest = c.tag_name().name().strip_prefix("port0")?;
            let value = c
                .attribute("data")
                .or_else(|| child(c, &["data"])?.attribute("value"))?;
            Some((rest.parse().ok()?, value.trim().parse().ok()?))
        })
        .collect()
}

/// Rosaclef's reverb that matches a plugin keeping `dry` and adding a
/// tail of `wet` (linear) decaying in `decay` seconds, and the gain the
/// insert must add (the reverb crossfades: dry · (1 − mix/2) + 3 · mix ·
/// tail). `tail_db` is the plugin's tail energy relative to the dry at
/// unit wet, measured in LMMS renders; Rosaclef's tail measures about
/// −5.2 + 5.5 · size dB.
fn reverb(decay: f64, dry: f64, wet: f64, tail_db: f64) -> (Device, f64) {
    let mut d = Device::new("reverb");
    // Combs of ~30 ms with feedback 0.7 + 0.28 · size.
    let size = clamp((10f64.powf(-0.09 / decay.max(0.05)) - 0.7) / 0.28, 0.0, 1.0);
    set_param(&mut d, "size", size);
    let tail = wet * 10f64.powf((tail_db - (-5.2 + 5.5 * size)) / 20.0);
    let ratio = tail / dry.max(1e-3);
    let mix = (ratio / (3.0 + 0.5 * ratio)).min(1.0);
    set_param(&mut d, "mix", mix);
    (d, dry / (1.0 - mix / 2.0))
}

/// The built-in effect of a plugin's family, judged by its name, and the
/// gain the insert must add. Settings are read from the ports of plugins
/// whose layout is known (TAP Reverberator, Calf Reverb, Phaser, Chorus,
/// Flanger); others get the family's defaults.
fn generic_effect(label: &str, wet: f64, ports: &HashMap<u32, f64>) -> Option<(Vec<Device>, f64)> {
    let l = label.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| l.contains(w));
    let port = |n: u32, d: f64| ports.get(&n).copied().unwrap_or(d);
    let db = |v: f64| 10f64.powf(v / 20.0);
    let mut d;
    let mut gain = 1.0;
    let mut before: Vec<Device> = vec![];
    if has(&["tap_reverb"]) {
        // Decay [ms], dry [dB], wet [dB], ..., band-pass filter (port 5).
        // Measured in LMMS on steady tones with its band-pass on: the wet
        // path adds in step with the dry signal above ~1 kHz (+6 dB at 0 dB
        // wet) and nothing below ~300 Hz, while fast notes stay crisp. So:
        // a high shelf for that lift and only a light tail.
        let (dry, wet) = (db(port(1, 0.0)), wet * db(port(2, 0.0)));
        let lift = 20.0 * ((dry + wet) / dry.max(1e-3)).log10();
        if port(5, 1.0) != 0.0 {
            // A wide bell and a shelf: fitted to its measured lift (0 at
            // 220 Hz, +2 at 880, +6 at 1760, +6.3 at 3520 for 0 dB wet).
            let mut eq = Device::new("eq");
            set_param(&mut eq, "mid", lift);
            set_param(&mut eq, "midFreq", 1700.0);
            set_param(&mut eq, "midQ", 0.7);
            set_param(&mut eq, "high", lift);
            set_param(&mut eq, "highFreq", 4000.0);
            before.push(eq);
        } else {
            gain *= (dry + wet) / dry.max(1e-3);
        }
        d = Device::new("reverb");
        let decay = port(0, 2500.0) / 1000.0;
        set_param(
            &mut d,
            "size",
            clamp((10f64.powf(-0.09 / decay.max(0.05)) - 0.7) / 0.28, 0.0, 1.0),
        );
        let mix = 0.25 * wet.min(1.0);
        set_param(&mut d, "mix", mix);
        gain *= dry / (1.0 - mix / 2.0);
    } else if l.contains("calf") && has(&["reverb"]) {
        // Decay time [s], ..., amount, dry.
        let (r, g) = reverb(port(7, 1.5), port(12, 1.0), wet * port(11, 0.25), 2.6);
        (d, gain) = (r, g);
    } else if has(&["reverb", "verb", "plate", "hall"]) {
        let (r, g) = reverb(1.5, 1.0, wet * 0.5, 2.0);
        (d, gain) = (r, g);
    } else if has(&["delay", "echo"]) {
        d = Device::new("delay");
        set_param(&mut d, "mix", 0.25 * wet);
    } else if has(&["phas"]) {
        // All-pass notches, no delay (a chorus would smear fast notes).
        d = Device::new("phaser");
        if l.contains("calf") {
            // Base freq [Hz], depth [cents], rate [Hz], feedback, stages,
            // stereo phase [°], ..., amount, dry: the dry kept, the
            // phased signal added.
            set_param(&mut d, "freq", port(4, 1000.0));
            set_param(&mut d, "depth", port(5, 4000.0) / 1200.0 / 6.0);
            set_param(&mut d, "rate", port(6, 0.25));
            set_param(&mut d, "feedback", port(7, 0.0));
            set_param(&mut d, "stages", port(8, 6.0));
            set_param(&mut d, "stereo", port(9, 180.0) / 360.0);
            let (amount, dry) = (wet * port(11, 1.0), port(12, 1.0));
            let ratio = amount / dry.max(1e-3);
            let mix = (2.0 * ratio / (1.0 + ratio)).min(1.0);
            set_param(&mut d, "mix", mix);
            gain = dry / (1.0 - mix / 2.0);
        } else {
            set_param(&mut d, "mix", wet);
        }
    } else if has(&["flang", "chorus", "ensemble", "vibrato"]) {
        d = Device::new("chorus");
        if l.contains("calf") && has(&["chorus", "flanger"]) {
            // Calf keeps `dry` and adds `amount` of the effect; the chorus
            // outputs (dry · (1 − mix/2) + wet · mix/2) · (1 + mix/5).
            let (amount, dry) = (wet * port(11, 1.0), port(12, 1.0));
            let ratio = amount / dry.max(1e-3);
            let mix = (2.0 * ratio / (1.0 + ratio)).min(1.0);
            set_param(&mut d, "mix", mix);
            gain = dry / ((1.0 - mix / 2.0) * (1.0 + mix / 5.0));
        } else {
            set_param(&mut d, "mix", 0.4 * wet);
        }
    } else if has(&["limit", "maximi"]) {
        d = Device::new("limiter");
    } else if has(&["compress", "dynamic", "expander", "gate"]) {
        d = Device::new("compressor");
    } else if has(&[
        "distort",
        "overdrive",
        "satur",
        "fuzz",
        "crush",
        "tube",
        "shaper",
        "exciter",
    ]) {
        d = Device::new("drive");
        set_param(&mut d, "amount", 0.4);
        set_param(&mut d, "mix", wet);
    } else if has(&["bass"]) {
        d = Device::new("eq");
        set_param(&mut d, "low", 6.0);
    } else if has(&["eq", "equal", "shelf", "treble"]) {
        d = Device::new("eq");
    } else if has(&["filter", "pass", "lpf", "hpf", "moog", "ladder"]) {
        d = Device::new("filter");
        let mode = if has(&["high", "hpf"]) {
            "highpass"
        } else if has(&["band"]) {
            "bandpass"
        } else {
            "lowpass"
        };
        set_option(&mut d, "mode", mode);
        set_param(&mut d, "mix", wet);
    } else {
        return None;
    }
    before.push(d);
    Some((before, gain))
}

/// Which `drum` voice a DrumSynth patch is, judged by its file name.
fn drumsynth_kind(path: &str) -> Option<&'static str> {
    let name = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    Some(
        if has(&[
            "hat_o", "hato", "openhat", "open", "ohh", "cymbal", "crash", "ride", "splash",
        ]) {
            "openhat"
        } else if has(&["hat", "hh"]) {
            "hat"
        } else if has(&["kick", "bd", "bassdrum"]) {
            "kick"
        } else if has(&["snare", "sd"]) {
            "snare"
        } else if has(&["clap"]) {
            "clap"
        } else if has(&["tom", "conga", "bongo"]) {
            "tom"
        } else if has(&["rim", "clave", "stick"]) {
            "rim"
        } else if has(&["cowbell", "bell", "agogo"]) {
            "cowbell"
        } else if has(&["shake", "maraca", "cabasa", "tamb"]) {
            "shaker"
        } else {
            return None;
        },
    )
}

/// `drum` decay and gain that match LMMS's factory DrumSynth patches of
/// a kind (medians over the 345 tr606/tr808/tr909/... patches whose
/// kind the file name tells, rendered by LMMS 1.2.2 and Rosaclef).
fn drumsynth_level(kind: &str) -> (f64, f64) {
    match kind {
        "kick" => (0.35, 1.39),
        "snare" => (0.23, 1.30),
        "clap" => (0.19, 1.32),
        "hat" => (0.48, 1.24),
        "openhat" => (0.36, 0.86),
        "rim" => (0.57, 1.01),
        "cowbell" => (0.22, 0.59),
        "shaker" => (0.40, 0.64),
        _ => (0.11, 0.61),
    }
}

/// Decode standard base64 (BitInvader's `sampleShape`).
fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            acc |= val(*c)? << (18 - 6 * i);
        }
        let n = chunk.len().saturating_sub(1);
        for i in 0..n {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// The fm synth's `duo` gain that matches the level of the measured two-operator
/// LMMS calibration (`duo` is about 7 dB louder at the same gain).
const TWO_OP_GAIN: f64 = 0.43;

/// A two-operator FM patch: modulator ratio, modulation index (radians),
/// how fast the index decays, feedback (0..1) and the carrier envelope.
struct TwoOp {
    ratio: f64,
    index: f64,
    index_decay: f64,
    feedback: f64,
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
}

/// The fm synth playing a two-operator patch with its `duo` algorithm: operator 6
/// modulates carriers 1 and 2 (index = 20 × level²); its envelope decays to
/// 12 % of the index.
fn two_op_fm(p: TwoOp) -> Device {
    let mut d = Device::new("fm");
    set_option(&mut d, "algorithm", "duo");
    let level = (p.index / 20.0).clamp(0.0, 1.0).sqrt();
    for (k, v) in [
        ("op1Ratio", 1.0),
        ("op2Ratio", 1.0),
        ("op6Ratio", p.ratio),
        ("op1Level", 1.0),
        ("op2Level", 1.0),
        ("op6Level", level),
        ("op1Attack", p.attack),
        ("op1Decay", p.decay),
        ("op1Sustain", p.sustain),
        ("op2Attack", p.attack),
        ("op2Decay", p.decay),
        ("op2Sustain", p.sustain),
        ("op6Attack", 0.001),
        ("op6Decay", p.index_decay),
        ("op6Sustain", 0.12),
        ("release", p.release),
        ("feedback", p.feedback),
        ("detune", 0.0),
    ] {
        set_param(&mut d, k, v);
    }
    d
}

/// The basic waveform closest to a drawn single-cycle wave (best
/// correlation over every phase shift).
fn nearest_wave(shape: &[f64]) -> &'static str {
    let n = shape.len();
    if n < 4 {
        return "saw";
    }
    let normalize = |v: Vec<f64>| {
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let v: Vec<f64> = v.iter().map(|x| x - mean).collect();
        let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-12 {
            v.iter().map(|x| x / norm).collect()
        } else {
            v
        }
    };
    let shape = normalize(shape.to_vec());
    let phase = |i: usize| i as f64 / n as f64;
    type Wave = fn(f64) -> f64;
    let refs: [(&str, Wave); 4] = [
        ("sine", |t| (std::f64::consts::TAU * t).sin()),
        ("triangle", |t| 1.0 - 4.0 * (t - 0.5).abs()),
        ("square", |t| if t < 0.5 { 1.0 } else { -1.0 }),
        ("saw", |t| 2.0 * t - 1.0),
    ];
    let mut best = ("saw", f64::MIN);
    for (name, f) in refs {
        let r = normalize((0..n).map(|i| f(phase(i))).collect());
        let score = (0..n)
            .map(|s| (0..n).map(|i| shape[i] * r[(i + s) % n]).sum::<f64>().abs())
            .fold(0.0, f64::max);
        if score > best.1 {
            best = (name, score);
        }
    }
    best.0
}

// ---------------------------------------------------------------- importer

#[derive(Clone, Copy, PartialEq, Debug)]
enum PitchMode {
    /// `midi = key + 12 + (57 − basenote) + masterpitch + transpose`.
    Normal,
    /// Drums that ignore the key: every note plays at 60.
    Fixed,
    /// Drums that follow the key: 60 ± distance from the base note.
    Relative,
}

#[derive(Clone, Debug)]
struct ChannelInfo {
    id: String,
    base_note: i32,
    transpose: i32,
    mode: PitchMode,
    /// Whether the project's master pitch applies (`usemasterpitch`).
    master_pitch: bool,
    /// Chord stacking: semitone offsets each note is expanded into.
    chord: Vec<i32>,
}

/// LMMS chords (the chord creator and arpeggiator table), by index.
const CHORDS: &[&[i32]] = &[
    &[0],              // octave
    &[0, 4, 7],        // Major
    &[0, 4, 6],        // Majb5
    &[0, 3, 7],        // minor
    &[0, 3, 6],        // minb5
    &[0, 2, 7],        // sus2
    &[0, 5, 7],        // sus4
    &[0, 4, 8],        // aug
    &[0, 5, 8],        // augsus4
    &[0, 3, 6, 9],     // tri
    &[0, 4, 7, 9],     // 6
    &[0, 5, 7, 9],     // 6sus4
    &[0, 4, 7, 14],    // 6add9
    &[0, 3, 7, 9],     // m6
    &[0, 3, 7, 9, 14], // m6add9
    &[0, 4, 7, 10],    // 7
    &[0, 5, 7, 10],    // 7sus4
    &[0, 4, 8, 10],    // 7#5
    &[0, 4, 6, 10],    // 7b5
];

/// An automatable LMMS control (an element with an `id`) that maps onto
/// a Rosaclef automation target: `value = lmms value × scale`.
#[derive(Clone, Debug)]
struct Model {
    target: AutomationTarget,
    scale: f64,
    color: String,
}

/// One LMMS automation pattern, for one control.
#[derive(Clone, Debug)]
struct AutoClip {
    name: String,
    /// Song position and length in ticks.
    start: f64,
    len: f64,
    /// 0 discrete, 1 linear, 2 cubic (approximated as linear).
    progression: i32,
    /// (tick, value), sorted by tick.
    points: Vec<(f64, f64)>,
}

impl AutoClip {
    fn value_at(&self, tick: f64) -> f64 {
        let i = self.points.partition_point(|p| p.0 <= tick);
        if i == 0 {
            return self.points[0].1;
        }
        let a = self.points[i - 1];
        match self.points.get(i) {
            Some(b) if self.progression != 0 && b.0 > a.0 => {
                a.1 + (b.1 - a.1) * (tick - a.0) / (b.0 - a.0)
            }
            _ => a.1,
        }
    }
}

struct Importer<'o> {
    opts: &'o Options,
    project: Project,
    warn: Warnings,
    samples: Vec<SampleCopy>,
    sample_map: HashMap<PathBuf, String>,
    sample_dests: HashSet<String>,
    channel_ids: Ids,
    pattern_ids: Ids,
    /// Identical instrument-track patterns become one Rosaclef pattern.
    dedupe: HashMap<String, String>,
    master_pitch: i32,
    /// LMMS ticks per bar at the project time signature.
    ticks_per_bar: f64,
    bar_beats: f64,
    loud_notes: usize,
    /// Automatable controls by LMMS id.
    models: HashMap<String, Model>,
    /// How many tracks feed each FX channel.
    fx_users: HashMap<usize, usize>,
    /// `mastervol` / 100 and the master FX channel's volume (their product
    /// is the master insert's volume).
    master_gain: f64,
    master_fx_volume: f64,
    /// Level the instrument mapped last could not reach with its own gain
    /// (multiplies the channel volume).
    makeup: f64,
    /// Broadband gain of the effect chain mapped last (multiplies the
    /// volume of the insert it lands on).
    fx_gain: f64,
}

impl<'o> Importer<'o> {
    fn new(opts: &'o Options) -> Importer<'o> {
        let mut project = Project::empty(&opts.title);
        project.schema = "./project.schema.json".into();
        project.patterns.clear();
        project.playlist.tracks.clear();
        project.mixer.inserts.truncate(1);
        Importer {
            opts,
            project,
            warn: Warnings::default(),
            samples: vec![],
            sample_map: HashMap::new(),
            sample_dests: HashSet::new(),
            channel_ids: Ids::default(),
            pattern_ids: Ids::default(),
            dedupe: HashMap::new(),
            master_pitch: 0,
            ticks_per_bar: 192.0,
            bar_beats: 4.0,
            loud_notes: 0,
            models: HashMap::new(),
            fx_users: HashMap::new(),
            master_gain: 1.0,
            master_fx_volume: 1.0,
            makeup: 1.0,
            fx_gain: 1.0,
        }
    }

    fn run(&mut self, root: Node) -> Result<()> {
        let version = text(root, "creatorversion");
        self.project.meta.description = if version.is_empty() {
            "Imported from LMMS".into()
        } else {
            format!("Imported from LMMS {version}")
        };

        // Tempo and time signature.
        if let Some(head) = child(root, &["head"]) {
            let bpm = num_or(head, "bpm", 120.0);
            if !(20.0..=999.0).contains(&bpm) {
                self.warn.add(format!(
                    "tempo {bpm} BPM is outside 20..999 and was clamped"
                ));
            }
            self.project.transport.bpm = clamp(bpm, 20.0, 999.0);
            let num_ = num_or(head, "timesig_numerator", 4.0).max(1.0);
            let den = num_or(head, "timesig_denominator", 4.0).max(1.0);
            self.ticks_per_bar = 192.0 * num_ / den;
            self.bar_beats = 4.0 * num_ / den;
            let bpb = self.bar_beats;
            if bpb.fract() != 0.0 || !(1.0..=32.0).contains(&bpb) {
                self.warn.add(format!("time signature {num_}/{den} is not a whole number of beats per bar; using {} beats per bar", bpb.round().clamp(1.0, 32.0)));
            }
            self.project.transport.beats_per_bar = bpb.round().clamp(1.0, 32.0) as u32;
            self.master_pitch = num_or(head, "masterpitch", 0.0).round() as i32;
            let master_vol = num_or(head, "mastervol", 100.0);
            self.master_gain = master_vol / 100.0;
            self.project.mixer.inserts[0].volume = clamp(master_vol / 100.0, 0.0, 2.0);
        } else {
            self.warn
                .add("the project has no <head>; using 120 BPM in 4/4");
        }
        let song = child(root, &["song"])
            .ok_or_else(|| anyhow!("not an LMMS song project (no <song> element)"))?;
        for t in song
            .descendants()
            .filter(|n| is(n, &["instrumenttrack", "sampletrack"]))
        {
            let fx = num(t, "fxch").or_else(|| num(t, "mixch")).unwrap_or(0.0);
            *self.fx_users.entry(fx.max(0.0) as usize).or_insert(0) += 1;
        }
        self.build_mixer(song);
        if let Some(head) = child(root, &["head"]) {
            self.register(head, "bpm", AutomationTarget::Tempo, 1.0, "#d4af37");
            let scale = self.master_fx_volume / 100.0;
            self.register(
                head,
                "mastervol",
                AutomationTarget::InsertVolume(InsertIx::MASTER),
                scale,
                "#d4af37",
            );
        }

        let container = child(song, &["trackcontainer"])
            .ok_or_else(|| anyhow!("the project has no song track container"))?;
        let tracks: Vec<Node> = children(container, &["track"]).collect();
        let bb_tracks: Vec<Node> = tracks
            .iter()
            .copied()
            .filter(|t| text(*t, "type") == "1")
            .collect();
        let mut bb_patterns: Option<Vec<Option<String>>> = None;
        let mut automation: Vec<Node> = vec![];
        for t in &tracks {
            match text(*t, "type") {
                "0" | "" => self.instrument_track(*t),
                "1" => {
                    if bb_patterns.is_none() {
                        bb_patterns = Some(self.build_bb(&bb_tracks));
                    }
                    let idx = bb_tracks.iter().position(|b| b == t).unwrap_or(0);
                    let pat = bb_patterns
                        .as_ref()
                        .and_then(|v| v.get(idx).cloned())
                        .flatten();
                    self.bb_track(*t, pat);
                }
                "2" => self.sample_track(*t),
                "5" | "6" => automation.push(*t),
                other => self.warn.add(format!(
                    "track \"{}\" of unsupported type {other} was skipped",
                    text(*t, "name")
                )),
            }
        }
        // The global automation track (tempo, master volume) sits in <song>.
        automation
            .extend(children(song, &["track"]).filter(|t| matches!(text(*t, "type"), "5" | "6")));
        self.automation(&automation);
        if self.loud_notes > 0 {
            self.warn.add(format!(
                "{} note(s) louder than 100% were clamped to full velocity",
                self.loud_notes
            ));
        }
        // Keep a limiter last on the master, as in every Rosaclef project.
        let master = &mut self.project.mixer.inserts[0];
        if master
            .effects
            .last()
            .map(|e| e.kind != "limiter")
            .unwrap_or(true)
        {
            master.effects.push(Device::new("limiter"));
        }
        pad_tracks(&mut self.project, 8);
        Ok(())
    }

    // ------------------------------------------------------------ mixer

    fn build_mixer(&mut self, song: Node) {
        let Some(mixer) = child(song, &["fxmixer", "mixer"]) else {
            return;
        };
        let chans: Vec<Node> = children(mixer, &["fxchannel", "mixerchannel"]).collect();
        let max = chans
            .iter()
            .filter_map(|c| num(*c, "num"))
            .fold(0.0, f64::max) as usize;
        if max + 1 > MAX_INSERTS {
            self.warn.add(format!(
                "the FX mixer has {} channels; only the first {MAX_INSERTS} were imported",
                max + 1
            ));
        }
        let n = (max + 1).min(MAX_INSERTS);
        while self.project.mixer.inserts.len() < n {
            let i = self.project.mixer.inserts.len();
            self.project
                .mixer
                .inserts
                .push(Insert::new(&format!("FX {i}")));
        }
        for c in chans {
            let i = num_or(c, "num", 0.0) as usize;
            if i >= n {
                continue;
            }
            let name = text(c, "name");
            let vol = num_or(c, "volume", 1.0);
            let effects = child(c, &["fxchain"])
                .map(|fx| self.effects(fx, &format!("FX channel \"{name}\"")))
                .unwrap_or_default();
            for s in children(c, &["send"]) {
                let to = num_or(s, "channel", 0.0) as usize;
                if to != 0 {
                    self.warn.add(format!("send from FX {i} to FX {to} was flattened: every Rosaclef insert feeds the master"));
                }
            }
            let ins = &mut self.project.mixer.inserts[i];
            if !name.is_empty() {
                ins.name = name.to_string();
            }
            ins.volume = clamp(ins.volume * vol * self.fx_gain, 0.0, 2.0);
            ins.mute = flag(c, "muted");
            ins.effects.extend(effects);
            let scale = if i == 0 {
                self.master_fx_volume = vol * self.fx_gain;
                self.master_gain * self.fx_gain
            } else {
                self.fx_gain
            };
            let target = AutomationTarget::InsertVolume(InsertIx(i as u32));
            self.register(c, "volume", target, scale, "#d4af37");
        }
    }

    /// Set an instrument's output gain, the rest of the level going to the
    /// channel volume when it exceeds the knob's range.
    fn level(&mut self, d: &mut Device, gain: f64) {
        set_param(d, "gain", gain);
        let got = d.param("gain");
        if got > 0.0 && gain > got {
            self.makeup = gain / got;
        }
    }

    /// Remember an automatable control (`<name id=".." value=".."/>`).
    fn register(&mut self, n: Node, name: &str, target: AutomationTarget, scale: f64, color: &str) {
        if let Some(id) = child(n, &[name]).and_then(|c| c.attribute("id")) {
            self.models.insert(
                id.to_string(),
                Model {
                    target,
                    scale,
                    color: color.to_string(),
                },
            );
        }
    }

    /// The insert for an LMMS FX channel number.
    fn insert(&mut self, fx: f64, owner: &str) -> InsertIx {
        let i = fx.max(0.0) as usize;
        if i < self.project.mixer.inserts.len() {
            return InsertIx(i as u32);
        }
        self.warn.add(format!(
            "{owner} is routed to FX {i}, which does not exist; routed to the master"
        ));
        InsertIx::MASTER
    }

    /// Map an LMMS effect chain onto built-in effects.
    fn effects(&mut self, chain: Node, owner: &str) -> Vec<Device> {
        let mut out = vec![];
        self.fx_gain = 1.0;
        for e in children(chain, &["effect"]) {
            let name = text(e, "name");
            let wet = clamp(num_or(e, "wet", 1.0), 0.0, 1.0);
            // The plugin's own settings live in the first element that is not <key>.
            let settings = e
                .children()
                .find(|c| c.is_element() && c.tag_name().name() != "key");
            let get = |k: &str, d: f64| settings.map(|s| num_or(s, k, d)).unwrap_or(d);
            let mut dev = match name {
                "reverbsc" => {
                    let mut d = Device::new("reverb");
                    set_param(&mut d, "size", get("size", 0.89));
                    set_param(&mut d, "damping", 1.0 - (get("color", 10000.0) / 20000.0));
                    set_param(&mut d, "mix", wet);
                    d
                }
                "delay" => {
                    let mut d = Device::new("delay");
                    let secs = get("DelayTimeSamples", 0.5);
                    set_param(&mut d, "time", secs * self.project.transport.bpm / 60.0);
                    set_param(
                        &mut d,
                        "feedback",
                        get("FeebackAmount", get("FeedbackAmount", 0.5)),
                    );
                    set_param(&mut d, "mix", wet * 0.5);
                    d
                }
                "flanger" => {
                    let mut d = Device::new("chorus");
                    set_param(&mut d, "rate", get("LfoFrequency", 0.25));
                    set_param(&mut d, "mix", wet * 0.5);
                    d
                }
                "bassbooster" => {
                    // LMMS computes (in + lowpass(in) · ratio) · gain with a
                    // one-pole low-pass of coefficient freq / (freq + 1): a
                    // low shelf plus a broadband gain, which goes to the
                    // insert's volume.
                    let mut d = Device::new("eq");
                    set_param(
                        &mut d,
                        "low",
                        20.0 * (1.0 + get("ratio", 2.0).max(0.0)).log10(),
                    );
                    let coef = get("freq", 100.0).max(10.0);
                    set_param(&mut d, "lowFreq", 44100.0 / (TAU * (coef + 0.5)));
                    if num_or(e, "on", 1.0) != 0.0 {
                        self.fx_gain *= 1.0 + wet * (get("gain", 1.0).max(0.0) - 1.0);
                    }
                    d
                }
                "dualfilter" => {
                    let mut d = Device::new("filter");
                    set_param(&mut d, "cutoff", get("cut1", 7000.0));
                    set_param(&mut d, "resonance", (get("res1", 0.5) - 0.5) / 4.0);
                    let mode = match get("filter1", 0.0) as i32 {
                        1 => "highpass",
                        2 | 3 => "bandpass",
                        _ => "lowpass",
                    };
                    set_option(&mut d, "mode", mode);
                    set_param(&mut d, "mix", wet);
                    d
                }
                "waveshaper" | "bitcrush" => {
                    let mut d = Device::new("drive");
                    set_param(&mut d, "amount", 0.5);
                    set_param(&mut d, "mix", wet);
                    d
                }
                "dynamicsprocessor" | "compressor" => Device::new("compressor"),
                "eq" => Device::new("eq"),
                "" => continue,
                other => {
                    // Plugin hosts name the plugin in <key>: map it by family.
                    let plugin = plugin_name(e);
                    let label = if plugin.is_empty() {
                        other.to_string()
                    } else {
                        plugin
                    };
                    match generic_effect(&label, wet, &ladspa_ports(e)) {
                        Some((devices, gain)) => {
                            let on = num_or(e, "on", 1.0) != 0.0;
                            if on {
                                self.fx_gain *= gain;
                            }
                            let kinds: Vec<&str> = devices.iter().map(|d| d.kind.as_str()).collect();
                            self.warn.add(format!(
                                "effect \"{label}\" was approximated by the built-in \"{}\"",
                                kinds.join("\" + \"")
                            ));
                            for mut d in devices {
                                d.enabled = on;
                                out.push(d);
                            }
                        }
                        None => self.warn.add(format!(
                            "effect \"{label}\" on {owner} has no Rosaclef equivalent and was skipped"
                        )),
                    }
                    continue;
                }
            };
            self.warn.add(format!(
                "LMMS effect \"{name}\" was approximated by the built-in \"{}\"",
                dev.kind
            ));
            dev.enabled = num_or(e, "on", 1.0) != 0.0;
            out.push(dev);
        }
        out
    }

    // ------------------------------------------------------------ channels

    /// Create the channel for an instrument track.
    fn add_channel(&mut self, track: Node, has_steps: bool) -> Option<ChannelInfo> {
        let name = text(track, "name");
        let name = if name.is_empty() { "Instrument" } else { name };
        let it = child(track, &["instrumenttrack"])?;
        let base_note = num_or(it, "basenote", DEFAULT_BASE_NOTE as f64).round() as i32;
        self.makeup = 1.0;
        let (instrument, mode, mut transpose) = self.instrument(name, it, has_steps);
        // The track's pitch knob, in cents.
        transpose += (num_or(it, "pitch", 0.0) / 100.0).round() as i32;
        let vol = num_or(it, "vol", 100.0);
        if vol > 150.0 {
            self.warn.add(format!(
                "track \"{name}\": volume {vol}% was clamped to 150%"
            ));
        }
        let fx = num(it, "fxch").or_else(|| num(it, "mixch")).unwrap_or(0.0);
        let mut mixer = self.insert(fx, &format!("track \"{name}\""));
        // Track-level effects run before the FX channel: prepend them to
        // its chain when no other track uses it, else give them an insert
        // of their own that copies the FX channel.
        if let Some(chain) = child(it, &["fxchain"]) {
            let fx = self.effects(chain, &format!("track \"{name}\""));
            let fx_gain = self.fx_gain;
            if !fx.is_empty() {
                let only_user = self.fx_users.get(&mixer.index()).copied().unwrap_or(0) <= 1;
                if mixer != InsertIx::MASTER && only_user {
                    let ins = &mut self.project.mixer.inserts[mixer.index()];
                    ins.effects.splice(0..0, fx);
                    ins.volume = clamp(ins.volume * fx_gain, 0.0, 2.0);
                } else if self.project.mixer.inserts.len() < MAX_INSERTS {
                    let mut ins = Insert::new(&format!("{name} FX"));
                    ins.effects = fx;
                    if mixer != InsertIx::MASTER {
                        let shared = self.project.mixer.inserts[mixer.index()].clone();
                        self.warn.add(format!("track \"{name}\": its effects got an insert of their own, a copy of FX {} (\"{}\"), which other tracks share", mixer.0, shared.name));
                        ins.volume = shared.volume;
                        ins.mute = shared.mute;
                        ins.effects.extend(shared.effects);
                    }
                    ins.volume = clamp(ins.volume * fx_gain, 0.0, 2.0);
                    mixer = InsertIx(self.project.mixer.inserts.len() as u32);
                    self.project.mixer.inserts.push(ins);
                } else {
                    self.warn.add(format!(
                        "track \"{name}\": no free insert for its effects; they were skipped"
                    ));
                }
            }
        }
        let id = self.channel_ids.make(name);
        let track_color = text(track, "color");
        let color = if track_color.len() == 7
            && track_color.starts_with('#')
            && track_color[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            track_color.to_lowercase()
        } else {
            color(self.project.channels.len())
        };
        if flag(track, "solo") {
            self.warn
                .add(format!("track \"{name}\" was soloed; solo is not imported"));
        }
        let volume = AutomationTarget::ChannelVolume(id.clone());
        self.register(it, "vol", volume, 0.01 * self.makeup, &color);
        self.register(
            it,
            "pan",
            AutomationTarget::ChannelPan(id.clone()),
            0.01,
            &color,
        );
        let arp = child(it, &["arpeggiator"])
            .filter(|a| flag(*a, "arp-enabled"))
            .map(|a| self.arpeggio(a, name));
        let chord = match child(it, &["chordcreator"]).filter(|c| flag(*c, "chord-enabled")) {
            Some(_) if arp.is_some() => {
                self.warn.add(format!(
                    "track \"{name}\": chord stacking under the arpeggiator was ignored"
                ));
                vec![]
            }
            Some(c) => {
                let chord = self.chord(num_or(c, "chord", 0.0), name);
                let octaves = num_or(c, "chordrange", 1.0).round().clamp(1.0, 9.0) as i32;
                (0..octaves)
                    .flat_map(|o| chord.iter().map(move |k| k + 12 * o))
                    .collect()
            }
            None => vec![],
        };
        self.project.channels.push(Channel {
            id: id.clone(),
            name: name.to_string(),
            color,
            instrument,
            volume: clamp(vol / 100.0 * self.makeup, 0.0, 1.5),
            pan: clamp(num_or(it, "pan", 0.0) / 100.0, -1.0, 1.0),
            mute: flag(track, "muted"),
            mixer,
            arp,
            layer_of: None,
        });
        Some(ChannelInfo {
            id,
            base_note,
            transpose,
            mode,
            master_pitch: num_or(it, "usemasterpitch", 1.0) != 0.0,
            chord,
        })
    }

    /// The intervals of LMMS chord number `index`.
    fn chord(&mut self, index: f64, track: &str) -> &'static [i32] {
        match CHORDS.get(index.max(0.0) as usize) {
            Some(c) => c,
            None => {
                self.warn.add(format!(
                    "track \"{track}\": LMMS chord #{index} is not supported; octaves were used"
                ));
                CHORDS[0]
            }
        }
    }

    /// The channel arpeggiator that plays like LMMS's (the same chord
    /// table, in order; times converted at the song tempo).
    fn arpeggio(&mut self, a: Node, track: &str) -> Arpeggio {
        let index = num_or(a, "arp", 0.0).max(0.0) as usize;
        let chord = match rosaclef_core::arp::CHORDS
            .get(index)
            .filter(|_| index < CHORDS.len())
        {
            Some(c) => c.0.to_string(),
            None => {
                self.warn.add(format!(
                    "track \"{track}\": LMMS chord #{index} is not supported; the arpeggio runs in octaves"
                ));
                "octave".to_string()
            }
        };
        // `arptime` is in milliseconds (a tempo-synced knob saves its
        // current value too).
        let ms = num_or(a, "arptime", 200.0);
        let rate = clamp(
            ms / 1000.0 * self.project.transport.bpm / 60.0,
            rosaclef_core::arp::RATE_MIN,
            rosaclef_core::arp::RATE_MAX,
        );
        for (k, what) in [
            ("arpskip", "skip"),
            ("arpmiss", "miss"),
            ("arpcycle", "cycle"),
        ] {
            if num_or(a, k, 0.0) != 0.0 {
                self.warn.add(format!(
                    "track \"{track}\": the arpeggiator's {what} setting was ignored"
                ));
            }
        }
        let direction = match num_or(a, "arpdir", 0.0) as i32 {
            1 => "down",
            2 => "updown",
            3 => "downup",
            4 => "random",
            _ => "up",
        };
        Arpeggio {
            chord,
            octaves: num_or(a, "arprange", 1.0)
                .round()
                .clamp(1.0, rosaclef_core::arp::OCTAVES_MAX as f64) as u32,
            rate,
            direction: direction.into(),
            gate: clamp(
                num_or(a, "arpgate", 100.0) / 100.0,
                rosaclef_core::arp::GATE_MIN,
                rosaclef_core::arp::GATE_MAX,
            ),
            mode: if num_or(a, "arpmode", 0.0) as i32 == 1 {
                "sort"
            } else {
                "free"
            }
            .into(),
        }
    }

    /// Map an LMMS instrument onto a built-in one. Returns the device, how
    /// note keys map to pitches, and a transposition in semitones.
    fn instrument(&mut self, track: &str, it: Node, has_steps: bool) -> (Device, PitchMode, i32) {
        let inst = child(it, &["instrument"]);
        let kind = inst.map(|i| text(i, "name")).unwrap_or("");
        let settings = inst.and_then(|i| i.children().find(|c| c.is_element()));
        let get = |k: &str, d: f64| settings.map(|s| num_or(s, k, d)).unwrap_or(d);
        match kind {
            "tripleoscillator" => {
                let mut d = clean_va();
                let osc: Vec<(usize, f64, f64, f64, f64)> = (0..3)
                    .map(|i| {
                        let fine =
                            (get(&format!("finel{i}"), 0.0) + get(&format!("finer{i}"), 0.0)) / 2.0;
                        (
                            i,
                            get(&format!("vol{i}"), 33.0),
                            get(&format!("wavetype{i}"), 0.0),
                            get(&format!("coarse{i}"), 0.0),
                            fine,
                        )
                    })
                    .filter(|o| o.1 > 0.0)
                    .collect();
                let wave = |w: f64| match w as i32 {
                    0 => "sine",
                    1 | 5 => "triangle",
                    3 => "pulse",
                    6 => "noise",
                    _ => "saw",
                };
                if osc.iter().any(|o| o.2 as i32 == 7) {
                    self.warn.add(format!("track \"{track}\": a user-defined TripleOscillator waveform was replaced by a saw"));
                }
                let mut transpose = 0;
                if let Some(a) = osc.first() {
                    transpose = a.3.round() as i32;
                    set_option(&mut d, "wave1", wave(a.2));
                    match osc.get(1) {
                        Some(b) => {
                            set_option(&mut d, "wave2", wave(b.2));
                            set_param(&mut d, "osc2Semi", b.3 - a.3);
                            set_param(&mut d, "osc2Detune", b.4 - a.4);
                            set_param(&mut d, "osc2Mix", b.1 / a.1.max(1.0));
                        }
                        None => set_param(&mut d, "osc2Mix", 0.0),
                    }
                    if let Some(c) = osc.get(2) {
                        if (c.3 - (a.3 - 12.0)).abs() < 0.5 {
                            set_param(&mut d, "sub", c.1 / a.1.max(1.0));
                            if wave(c.2) != "sine" {
                                set_option(&mut d, "subWave", "square");
                            }
                        } else {
                            self.warn.add(format!("track \"{track}\": TripleOscillator's third oscillator was dropped (the analog synth has two plus a sub)"));
                        }
                    }
                    // TripleOscillator sums its oscillators (100% = full scale).
                    let total: f64 = osc.iter().map(|o| o.1).sum();
                    let gain = va_gain(&d, total / 100.0);
                    self.level(&mut d, gain);
                } else {
                    set_param(&mut d, "gain", 0.0);
                }
                for m in ["modalgo1", "modalgo2"] {
                    if get(m, 2.0) as i32 != 2 {
                        self.warn.add(format!("track \"{track}\": TripleOscillator modulation (PM/AM/FM/sync) was approximated by mixing"));
                    }
                }
                if !(-24..=24).contains(&transpose) {
                    transpose = transpose.clamp(-48, 48);
                }
                self.envelope(it, &mut d);
                (d, PitchMode::Normal, transpose)
            }
            "kicker" => {
                let mut d = Device::new("drum");
                let noise = get("noise", 0.0);
                set_option(&mut d, "kind", if noise > 0.5 { "snare" } else { "kick" });
                set_param(
                    &mut d,
                    "tune",
                    12.0 * (get("endfreq", 40.0).max(5.0) / 45.0).log2(),
                );
                set_param(&mut d, "decay", get("decay", 440.0) / 440.0);
                set_param(&mut d, "snap", get("click", 0.4));
                set_param(&mut d, "drive", get("dist", 0.8) / 10.0);
                // Measured against LMMS renders: `drum` at 1.2 × Kicker's gain.
                self.level(&mut d, 1.2 * get("gain", 1.0));
                self.warn.add(format!(
                    "track \"{track}\": Kicker was approximated by the Ebony Drum Machine"
                ));
                let follows = get("startnote", 1.0) != 0.0 || get("endnote", 0.0) != 0.0;
                (
                    d,
                    if follows {
                        PitchMode::Relative
                    } else {
                        PitchMode::Fixed
                    },
                    0,
                )
            }
            "audiofileprocessor"
                if settings
                    .map(|s| text(s, "src").to_lowercase().ends_with(".ds"))
                    .unwrap_or(false) =>
            {
                // A DrumSynth patch is synthesized by LMMS, not an audio
                // file: play the `drum` voice it is closest to.
                let src = settings.map(|s| text(s, "src")).unwrap_or("");
                let mut d = Device::new("drum");
                let kind = drumsynth_kind(src);
                let (decay, gain) = drumsynth_level(kind.unwrap_or("tom"));
                set_option(&mut d, "kind", kind.unwrap_or("tom"));
                set_param(&mut d, "decay", decay);
                self.level(&mut d, gain * get("amp", 100.0) / 100.0);
                self.warn.add(format!(
                    "track \"{track}\": DrumSynth patch \"{src}\" was approximated by the Ebony Drum Machine {}",
                    kind.unwrap_or("tom")
                ));
                (d, PitchMode::Relative, 0)
            }
            "audiofileprocessor" => {
                let mut d = Device::new("sampler");
                let src = settings.map(|s| text(s, "src")).unwrap_or("");
                match self.sample(src, &format!("track \"{track}\"")) {
                    Some(path) => set_option(&mut d, "sample", &path),
                    None => self.warn.add(format!("track \"{track}\": AudioFileProcessor has no sample file (embedded sample data is not supported)")),
                }
                // key == basenote plays the sample at its original pitch; that key maps to MIDI 69.
                set_param(&mut d, "root", 69.0);
                let (s, e) = (get("sframe", 0.0), get("eframe", 1.0));
                if s < e {
                    set_param(&mut d, "start", s);
                    set_param(&mut d, "end", e);
                }
                // Measured against LMMS: unity at 100% amplification.
                self.level(&mut d, get("amp", 100.0) / 100.0);
                let looped = get("looped", 0.0) as i32 != 0;
                set_option(
                    &mut d,
                    "mode",
                    if looped {
                        "loop"
                    } else if has_steps {
                        "oneshot"
                    } else {
                        "pitched"
                    },
                );
                if get("reversed", 0.0) != 0.0 {
                    self.warn.add(format!("track \"{track}\": reversed playback is not supported; the sample plays forwards"));
                }
                (d, PitchMode::Normal, 0)
            }
            "lb302" => {
                // The analog synth from its acid preset: one oscillator
                // into the 4-pole ladder (24 dB, `db24`) or the 2-pole
                // screamer (12 dB), a snappy filter envelope, monophonic, and
                // legato glide when LB302's slide is on.
                let mut d = rosaclef_core::presets::find("Emerald Acid Bass")
                    .filter(|p| p.kind == "analog")
                    .map(|p| p.device())
                    .unwrap_or_else(|| Device::new("analog"));
                // LB302 shapes: 0 saw, 1 triangle, 2 square, 3 round square,
                // 4 moog, 5 sine, 6 exponential, 7 noise, 8-11 band-limited
                // saw / square / triangle / moog.
                let shape = get("shape", 0.0) as i32;
                let w = match shape {
                    1 | 5 | 6 | 10 => "triangle",
                    2 | 3 | 9 => "pulse",
                    _ => "saw",
                };
                set_option(&mut d, "wave1", w);
                if shape == 7 {
                    set_param(&mut d, "noise", 1.0);
                    self.warn.add(format!(
                        "track \"{track}\": LB302's noise shape became the analog synth's noise over a saw"
                    ));
                }
                set_param(&mut d, "osc2Mix", 0.0);
                set_param(&mut d, "sub", 0.0);
                set_option(
                    &mut d,
                    "filter",
                    if get("db24", 0.0) != 0.0 {
                        "ladder"
                    } else {
                        "screamer"
                    },
                );
                let cutoff = 80.0 * 2f64.powf(get("vcf_cut", 0.75) * 7.0);
                set_param(&mut d, "cutoff", cutoff);
                set_param(&mut d, "resonance", get("vcf_res", 0.75) * 0.95);
                set_param(&mut d, "drive", get("dist", 0.0).max(0.1));
                set_param(&mut d, "filterEnv", 0.2 + get("vcf_mod", 0.1) * 0.8);
                set_param(&mut d, "filterDecay", 0.05 + get("vcf_dec", 0.1) * 1.2);
                set_param(&mut d, "filterSustain", 0.0);
                set_param(&mut d, "attack", 0.002);
                set_param(&mut d, "decay", 0.4);
                set_param(&mut d, "sustain", 0.6);
                set_param(&mut d, "release", 0.05);
                let slide = get("slide", 0.0) != 0.0;
                set_option(&mut d, "mode", if slide { "legato" } else { "mono" });
                set_param(
                    &mut d,
                    "glide",
                    if slide {
                        0.02 + get("slide_dec", 0.6) * 0.2
                    } else {
                        0.0001
                    },
                );
                set_param(&mut d, "gain", 0.6);
                self.warn.add(format!(
                    "track \"{track}\": LB302 was approximated by Bronze Bass (analog)"
                ));
                (d, PitchMode::Normal, 0)
            }
            "malletsstk" => {
                let d = rosaclef_core::presets::find("Crystal Mallet")
                    .filter(|p| p.kind == "fm")
                    .map(|p| p.device())
                    .unwrap_or_else(|| Device::new("fm"));
                self.warn.add(format!(
                    "track \"{track}\": Mallets was approximated by Silver Keys (fm)"
                ));
                (d, PitchMode::Normal, 0)
            }
            "sf2player" if has_instrument("soundfont") => {
                // The same General MIDI patch on the built-in soundfont.
                let mut d = Device::new("soundfont");
                let patch = get("patch", 0.0).clamp(0.0, 127.0) as u8;
                let program = if get("bank", 0.0) as i32 == 128 {
                    rosaclef_core::gm::kit(patch)
                } else {
                    rosaclef_core::gm::PROGRAMS[patch as usize]
                };
                set_option(&mut d, "program", program);
                set_param(&mut d, "gain", get("gain", 1.0));
                let src = settings.map(|s| text(s, "src")).unwrap_or("");
                let file = Path::new(src)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.warn.add(format!(
                    "track \"{track}\": SoundFont \"{file}\" was replaced by the built-in General MIDI \"{program}\""
                ));
                if get("reverbOn", 0.0) != 0.0 || get("chorusOn", 0.0) != 0.0 {
                    self.warn.add(format!(
                        "track \"{track}\": the Sf2 Player's own reverb and chorus were skipped"
                    ));
                }
                (d, PitchMode::Normal, 0)
            }
            "OPL2" | "opl2" | "opulenz" => {
                // Two-operator FM: operator 1 modulates operator 2 (or both
                // sound, in additive mode). Levels count up to 63.
                const MUL: [f64; 16] = [
                    0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 10.0, 12.0, 12.0, 15.0,
                    15.0,
                ];
                let mul = |k: &str| MUL[get(k, 1.0).clamp(0.0, 15.0) as usize];
                let index = if get("fm", 1.0) != 0.0 {
                    6.0 * get("op1_lvl", 40.0) / 63.0
                } else {
                    0.0
                };
                // Envelope knobs count down the chip's rates (0 fastest);
                // sustain counts up from -45 dB in 3 dB steps. A rate r
                // decays 96 dB in about 39 s / 2^(r-1); Rosaclef times are
                // to -40 dB.
                let rate = |k: &str, d: f64| 15.0 - get(k, d).clamp(0.0, 15.0);
                let fall = |r: f64| {
                    if r < 1.0 {
                        30.0
                    } else {
                        39.28 / 2f64.powf(r - 1.0) * 40.0 / 96.0
                    }
                };
                let attack = match rate("op2_a", 14.0) {
                    r if r >= 15.0 => 0.0005,
                    r if r < 1.0 => 8.0,
                    r => 2.826 / 2f64.powf(r - 1.0),
                };
                let mut d = two_op_fm(TwoOp {
                    ratio: mul("op1_mul") / mul("op2_mul"),
                    index,
                    index_decay: fall(rate("op1_d", 14.0)),
                    feedback: get("feedback", 0.0) / 7.0,
                    attack,
                    decay: fall(rate("op2_d", 14.0)),
                    sustain: 10f64.powf(-3.0 * (15.0 - get("op2_s", 3.0)) / 20.0),
                    release: 0.75 * fall(rate("op2_r", 10.0)),
                });
                // Measured against LMMS: a full-level carrier peaks at
                // about -26 dBFS; levels are 0.75 dB steps.
                let carrier = 10f64.powf(-(63.0 - get("op2_lvl", 63.0)) * 0.75 / 20.0);
                self.level(&mut d, TWO_OP_GAIN * 0.17 * carrier);
                self.warn.add(format!(
                    "track \"{track}\": OpulenZ (OPL2) was approximated by Silver Keys (fm, two-operator duo)"
                ));
                (d, PitchMode::Normal, 0)
            }
            "nes" => {
                // Two pulse channels, a triangle and noise: the first two
                // enabled become the analog synth's oscillators.
                let mut d = clean_va();
                let voices: Vec<(&str, f64, f64)> =
                    [(1, "pulse"), (2, "pulse"), (3, "triangle"), (4, "noise")]
                        .into_iter()
                        .filter(|(i, _)| get(&format!("on{i}"), 0.0) != 0.0)
                        .map(|(i, w)| {
                            (
                                w,
                                get(&format!("vol{i}"), 15.0),
                                get(&format!("crs{i}"), 0.0),
                            )
                        })
                        .collect();
                let mut transpose = 0;
                match voices.first() {
                    Some(a) => {
                        transpose = a.2.round() as i32;
                        set_option(&mut d, "wave1", a.0);
                        match voices.get(1) {
                            Some(b) => {
                                set_option(&mut d, "wave2", b.0);
                                set_param(&mut d, "osc2Semi", b.2 - a.2);
                                set_param(&mut d, "osc2Detune", 0.0);
                                set_param(&mut d, "osc2Mix", b.1 / a.1.max(1.0));
                            }
                            None => set_param(&mut d, "osc2Mix", 0.0),
                        }
                        let gain = va_gain(&d, 0.3 * (1.0 + 0.5 * d.param("osc2Mix")));
                        set_param(&mut d, "gain", gain);
                    }
                    None => set_param(&mut d, "gain", 0.0),
                }
                self.envelope(it, &mut d);
                self.warn.add(format!(
                    "track \"{track}\": the NES synth was approximated by Bronze Bass (analog); sweeps and vibrato were dropped"
                ));
                (d, PitchMode::Normal, transpose)
            }
            "bitinvader" => {
                // A drawn single-cycle wave (base64 floats): use the basic
                // waveform it resembles most.
                let mut d = clean_va();
                let len = get("sampleLength", 128.0).max(0.0) as usize;
                let shape: Option<Vec<f64>> = settings
                    .and_then(|s| base64(text(s, "sampleShape")))
                    .map(|b| {
                        b.chunks_exact(4)
                            .take(len)
                            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64)
                            .filter(|v| v.is_finite())
                            .collect()
                    });
                let wave = match shape {
                    Some(s) if s.len() >= 4 => {
                        let every = s.len().div_ceil(256);
                        nearest_wave(&s.iter().step_by(every).copied().collect::<Vec<_>>())
                    }
                    _ => "saw",
                };
                set_option(
                    &mut d,
                    "wave1",
                    if wave == "square" { "pulse" } else { wave },
                );
                set_param(&mut d, "osc2Mix", 0.0);
                // Measured against LMMS: BitInvader peaks near -6 dBFS.
                let gain = va_gain(&d, 0.45);
                self.level(&mut d, gain);
                self.envelope(it, &mut d);
                self.warn.add(format!(
                    "track \"{track}\": BitInvader's drawn waveform was approximated by a {wave} wave in Bronze Bass (analog)"
                ));
                (d, PitchMode::Normal, 0)
            }
            other => {
                let mut d = clean_va();
                let gain = va_gain(&d, 0.375);
                set_param(&mut d, "gain", gain);
                self.envelope(it, &mut d);
                let what = if other.is_empty() {
                    "an empty instrument".to_string()
                } else {
                    format!("instrument \"{other}\"")
                };
                self.warn.add(format!(
                    "track \"{track}\": {what} is not supported; replaced by Bronze Bass (analog)"
                ));
                (d, PitchMode::Normal, 0)
            }
        }
    }

    /// Volume envelope and filter of an instrument track (`<eldata>`).
    /// LMMS envelope knobs (0..2) map to seconds as `5 · v²`.
    fn envelope(&mut self, it: Node, d: &mut Device) {
        let secs = |v: f64| 5.0 * v * v;
        let ed = child(it, &["eldata"]);
        let vol_env = ed
            .and_then(|e| child(e, &["elvol"]))
            .filter(|v| num_or(*v, "amt", 0.0) > 0.0);
        match vol_env {
            Some(v) => {
                set_param(d, "attack", secs(num_or(v, "att", 0.0)).max(0.001));
                // No hold stage in Rosaclef: the decay starts later instead.
                let hold = secs(num_or(v, "hold", 0.0));
                set_param(d, "decay", (hold + secs(num_or(v, "dec", 0.5))).max(0.001));
                // LMMS 1.x saves `sustain`, the level (measured: the
                // amplitude is its square); older files `sus`, inverted.
                let sustain = match num(v, "sustain") {
                    Some(s) => s.clamp(0.0, 1.0).powi(2),
                    None => 1.0 - num_or(v, "sus", 0.5),
                };
                set_param(d, "sustain", sustain);
                set_param(d, "release", secs(num_or(v, "rel", 0.1)).max(0.001));
            }
            None => {
                // No volume envelope: full level while the note is held.
                set_param(d, "attack", 0.002);
                set_param(d, "sustain", 1.0);
                set_param(d, "release", 0.01);
            }
        }
        set_param(d, "filterEnv", 0.0);
        match ed.filter(|e| flag(*e, "fwet")) {
            Some(e) => {
                set_param(d, "cutoff", num_or(e, "fcut", 14000.0));
                set_param(d, "resonance", (num_or(e, "fres", 0.5) - 0.5) / 4.0);
                let mode = match num_or(e, "ftype", 0.0) as i32 {
                    1 => "highpass",
                    2 | 3 => "bandpass",
                    0 | 6 | 7 | 8 => "lowpass",
                    t => {
                        self.warn.add(format!(
                            "instrument filter type {t} was approximated by a low-pass"
                        ));
                        "lowpass"
                    }
                };
                set_option(d, "filter", mode);
            }
            None => {
                set_param(d, "cutoff", 20000.0);
                set_param(d, "resonance", 0.0);
            }
        }
    }

    // ------------------------------------------------------------ notes

    fn pitch(&self, key: i32, ch: &ChannelInfo) -> i32 {
        match ch.mode {
            PitchMode::Normal => {
                let master = if ch.master_pitch {
                    self.master_pitch
                } else {
                    0
                };
                key + KEY_TO_MIDI + (DEFAULT_BASE_NOTE - ch.base_note) + master + ch.transpose
            }
            PitchMode::Fixed => 60,
            PitchMode::Relative => 60 + key - ch.base_note + ch.transpose,
        }
    }

    /// Notes of an LMMS pattern, in beats relative to the pattern start,
    /// with the track's chord stacking written out (its arpeggio is the
    /// channel's, played by the engine).
    fn notes(&mut self, pattern: Node, ch: &ChannelInfo, track: &str) -> Vec<Note> {
        let mut out = vec![];
        for n in children(pattern, &["note"]) {
            let key = num_or(n, "key", DEFAULT_BASE_NOTE as f64).round() as i32;
            let pitch = self.pitch(key, ch);
            let len = num_or(n, "len", 48.0);
            let vol = num_or(n, "vol", 100.0);
            if vol > 100.0 {
                self.loud_notes += 1;
            }
            out.push(Note {
                channel: ch.id.clone(),
                pitch,
                start: beats(num_or(n, "pos", 0.0).max(0.0), TICKS_PER_BEAT),
                length: if len > 0.0 {
                    beats(len, TICKS_PER_BEAT)
                } else {
                    STEP_BEATS
                },
                velocity: clamp(vol / 100.0, 0.0, 1.0),
            });
        }
        if ch.chord.len() > 1 {
            out = out
                .into_iter()
                .flat_map(|n| {
                    ch.chord.iter().map(move |k| Note {
                        pitch: n.pitch + k,
                        ..n.clone()
                    })
                })
                .collect();
        }
        let before = out.len();
        out.retain(|n| (0..=127).contains(&n.pitch));
        let dropped = before - out.len();
        if dropped > 0 {
            self.warn.add(format!(
                "track \"{track}\": {dropped} note(s) outside the MIDI range were dropped"
            ));
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        out
    }

    /// Length of an LMMS pattern in ticks: its saved `len` (newer LMMS),
    /// else what LMMS 1.x computes on load (`Pattern::updateLength`). A
    /// pattern with any note of positive length is a melody pattern and
    /// spans its notes; otherwise it is a beat pattern of `steps` 16ths
    /// (and at least its last step). Both round up to whole bars.
    fn pattern_ticks(&self, pattern: Node) -> f64 {
        let len = num_or(pattern, "len", 0.0);
        if len > 0.0 {
            return len;
        }
        let notes: Vec<(f64, f64)> = children(pattern, &["note"])
            .map(|n| (num_or(n, "pos", 0.0).max(0.0), num_or(n, "len", 48.0)))
            .collect();
        let melody = notes.iter().any(|n| n.1 > 0.0);
        let end = if melody {
            notes
                .iter()
                .filter(|n| n.1 > 0.0)
                .map(|n| n.0 + n.1)
                .fold(0.0, f64::max)
        } else {
            let steps = num_or(pattern, "steps", 0.0);
            let last = notes.iter().map(|n| n.0 + 1.0).fold(0.0, f64::max);
            if steps > 0.0 {
                // One step is a 16th: 12 ticks.
                (steps * 12.0).max(last)
            } else {
                last
            }
        };
        (end / self.ticks_per_bar - 1e-9).ceil().max(1.0) * self.ticks_per_bar
    }

    fn has_steps(track: Node) -> bool {
        track
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "note")
            .any(|n| num_or(n, "len", 48.0) <= 0.0)
    }

    fn add_track(&mut self, track: Node) -> TrackIx {
        let name = text(track, "name");
        let ix = TrackIx(self.project.playlist.tracks.len() as u32);
        self.project.playlist.tracks.push(Track {
            name: if name.is_empty() {
                format!("Track {}", ix.0 + 1)
            } else {
                name.to_string()
            },
            mute: flag(track, "muted"),
        });
        ix
    }

    // ------------------------------------------------------------ tracks

    fn instrument_track(&mut self, track: Node) {
        let name = text(track, "name").to_string();
        let Some(ch) = self.add_channel(track, Self::has_steps(track)) else {
            self.warn.add(format!(
                "track \"{name}\" has no instrument settings and was skipped"
            ));
            return;
        };
        let tix = self.add_track(track);
        let color = self
            .project
            .channels
            .last()
            .map(|c| c.color.clone())
            .unwrap_or_else(|| color(0));
        let mut count = 0;
        for pat in children(track, &["pattern", "midiclip"]) {
            let start = beats(num_or(pat, "pos", 0.0).max(0.0), TICKS_PER_BEAT);
            let length = beats(self.pattern_ticks(pat), TICKS_PER_BEAT);
            if flag(pat, "muted") {
                self.warn.add(format!(
                    "track \"{name}\": a muted clip at beat {start} was skipped"
                ));
                continue;
            }
            let notes = self.notes(pat, &ch, &name);
            let key = format!("{}|{length}|{notes:?}", ch.id);
            let id = match self.dedupe.get(&key) {
                Some(id) => id.clone(),
                None => {
                    count += 1;
                    let label = text(pat, "name");
                    let pname = if !label.is_empty() {
                        label.to_string()
                    } else if count == 1 {
                        name.clone()
                    } else {
                        format!("{name} {count}")
                    };
                    let id = self.pattern_ids.make(&pname);
                    self.project.patterns.push(Pattern {
                        id: id.clone(),
                        name: pname,
                        color: color.clone(),
                        length,
                        notes,
                        drums: None,
                    });
                    self.dedupe.insert(key, id.clone());
                    id
                }
            };
            self.project.playlist.clips.push(Clip {
                pattern: id,
                sample: String::new(),
                track: tix,
                start,
                length,
                offset: 0.0,
                gain: 1.0,
                mixer: InsertIx::MASTER,
            });
        }
    }

    /// Build one pattern per Beat+Bassline from the B&B track container
    /// (saved inside the first B&B track). Returns the pattern id per B&B.
    fn build_bb(&mut self, bb_tracks: &[Node]) -> Vec<Option<String>> {
        let n = bb_tracks.len();
        let container = bb_tracks.iter().find_map(|t| {
            child(*t, &["bbtrack", "patterntrack"]).and_then(|b| child(b, &["trackcontainer"]))
        });
        let mut notes: Vec<Vec<Note>> = vec![vec![]; n];
        let mut ticks: Vec<f64> = vec![0.0; n];
        if let Some(c) = container {
            for inner in children(c, &["track"]) {
                let name = text(inner, "name").to_string();
                if !matches!(text(inner, "type"), "0" | "") {
                    self.warn.add(format!(
                        "Beat+Bassline track \"{name}\" is not an instrument track and was skipped"
                    ));
                    continue;
                }
                let Some(ch) = self.add_channel(inner, Self::has_steps(inner)) else {
                    continue;
                };
                for (ord, pat) in children(inner, &["pattern", "midiclip"]).enumerate() {
                    // B&B k's pattern sits at bar k of the container.
                    let pos = num_or(pat, "pos", -1.0);
                    let k = if pos >= 0.0 {
                        (pos / self.ticks_per_bar).round() as usize
                    } else {
                        ord
                    };
                    if k >= n {
                        continue;
                    }
                    ticks[k] = ticks[k].max(self.pattern_ticks(pat));
                    let mut ns = self.notes(pat, &ch, &name);
                    notes[k].append(&mut ns);
                }
            }
        } else if n > 0 {
            self.warn
                .add("the Beat+Bassline editor's tracks were not found; B&B clips are empty");
        }
        (0..n)
            .map(|k| {
                let used = bb_tracks[k]
                    .children()
                    .any(|c| is(&c, &["bbtco", "patternclip"]));
                if notes[k].is_empty() && !used {
                    return None;
                }
                let bars = (ticks[k] / self.ticks_per_bar).ceil().max(1.0);
                let name = text(bb_tracks[k], "name");
                let name = if name.is_empty() {
                    format!("Beat/Bassline {k}")
                } else {
                    name.to_string()
                };
                let id = self.pattern_ids.make(&name);
                let mut ns = std::mem::take(&mut notes[k]);
                ns.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
                self.project.patterns.push(Pattern {
                    id: id.clone(),
                    name,
                    color: color(self.project.patterns.len() + 3),
                    length: bars * self.bar_beats,
                    notes: ns,
                    drums: None,
                });
                Some(id)
            })
            .collect()
    }

    fn bb_track(&mut self, track: Node, pattern: Option<String>) {
        let tix = self.add_track(track);
        let name = text(track, "name").to_string();
        for tco in children(track, &["bbtco", "patternclip"]) {
            let start = beats(num_or(tco, "pos", 0.0).max(0.0), TICKS_PER_BEAT);
            let length = beats(num_or(tco, "len", self.ticks_per_bar), TICKS_PER_BEAT);
            if flag(tco, "muted") {
                self.warn.add(format!(
                    "track \"{name}\": a muted clip at beat {start} was skipped"
                ));
                continue;
            }
            let Some(id) = pattern.clone() else { continue };
            if length > 0.0 {
                self.project.playlist.clips.push(Clip {
                    pattern: id,
                    sample: String::new(),
                    track: tix,
                    start,
                    length,
                    offset: 0.0,
                    gain: 1.0,
                    mixer: InsertIx::MASTER,
                });
            }
        }
    }

    fn sample_track(&mut self, track: Node) {
        let name = text(track, "name").to_string();
        let tix = self.add_track(track);
        let st = child(track, &["sampletrack"]);
        let vol = st.map(|s| num_or(s, "vol", 100.0)).unwrap_or(100.0);
        let fx = st
            .and_then(|s| num(s, "fxch").or_else(|| num(s, "mixch")))
            .unwrap_or(0.0);
        let mixer = self.insert(fx, &format!("sample track \"{name}\""));
        if st.map(|s| num_or(s, "pan", 0.0) != 0.0).unwrap_or(false) {
            self.warn.add(format!(
                "sample track \"{name}\": panning is not imported (audio clips have no pan)"
            ));
        }
        if let Some(chain) = st.and_then(|s| child(s, &["fxchain"])) {
            if chain.children().any(|c| is(&c, &["effect"])) {
                self.warn.add(format!("sample track \"{name}\": track effects were skipped (put them on its mixer insert)"));
            }
        }
        for tco in children(track, &["sampletco", "sampleclip"]) {
            let start = beats(num_or(tco, "pos", 0.0).max(0.0), TICKS_PER_BEAT);
            let length = beats(num_or(tco, "len", 0.0), TICKS_PER_BEAT);
            if flag(tco, "muted") {
                self.warn.add(format!(
                    "sample track \"{name}\": a muted clip at beat {start} was skipped"
                ));
                continue;
            }
            if length <= 0.0 {
                continue;
            }
            let Some(path) = self.sample(text(tco, "src"), &format!("sample track \"{name}\""))
            else {
                self.warn.add(format!("sample track \"{name}\": a clip without a sample file (embedded data) was skipped"));
                continue;
            };
            // A negative start offset means the clip's left edge was trimmed.
            let offset = beats((-num_or(tco, "off", 0.0)).max(0.0), TICKS_PER_BEAT);
            self.project.playlist.clips.push(Clip {
                pattern: String::new(),
                sample: path,
                track: tix,
                start,
                length,
                offset,
                gain: clamp(vol / 100.0, 0.0, 4.0),
                mixer,
            });
        }
    }

    // ------------------------------------------------------------ automation

    /// Turn the automation tracks into lanes. Every LMMS automation
    /// pattern drives the controls it lists (`<object id>`); while it is
    /// the latest one to have started it sets their value, and after its
    /// end the value holds. Before the first pattern a control keeps its
    /// saved value.
    fn automation(&mut self, tracks: &[Node]) {
        let mut by_control: Vec<(String, Vec<AutoClip>)> = vec![];
        for t in tracks {
            let patterns: Vec<Node> =
                children(*t, &["automationpattern", "automationclip"]).collect();
            if flag(*t, "muted") {
                if !patterns.is_empty() {
                    self.warn.add("a muted automation track was skipped");
                }
                continue;
            }
            for ap in patterns {
                let mut points: Vec<(f64, f64)> = children(ap, &["time"])
                    .map(|n| (num_or(n, "pos", 0.0).max(0.0), num_or(n, "value", 0.0)))
                    .collect();
                // Sorted by tick; a repeated tick keeps the last value.
                points.sort_by(|a, b| a.0.total_cmp(&b.0));
                points.reverse();
                points.dedup_by(|a, b| a.0 == b.0);
                points.reverse();
                let mut ids: Vec<&str> = children(ap, &["object"])
                    .filter_map(|o| o.attribute("id"))
                    .collect();
                ids.dedup();
                if points.is_empty() || ids.is_empty() {
                    continue;
                }
                let name = text(ap, "name");
                if flag(ap, "mute") || flag(ap, "muted") {
                    self.warn.add(format!(
                        "the muted automation pattern \"{name}\" was skipped"
                    ));
                    continue;
                }
                let last = points.last().map(|p| p.0).unwrap_or(0.0);
                let len = num_or(ap, "len", 0.0);
                let clip = AutoClip {
                    name: name.to_string(),
                    start: num_or(ap, "pos", 0.0).max(0.0),
                    len: if len > 0.0 { len } else { last },
                    progression: num_or(ap, "prog", 0.0) as i32,
                    points,
                };
                for id in ids {
                    match by_control.iter_mut().find(|c| c.0 == id) {
                        Some(c) => c.1.push(clip.clone()),
                        None => by_control.push((id.to_string(), vec![clip.clone()])),
                    }
                }
            }
        }

        let mut lane_ids = Ids::default();
        let mut cubic = false;
        for (id, mut clips) in by_control {
            clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            let Some(model) = self.models.get(&id).cloned() else {
                self.warn.add(format!(
                    "automation \"{}\" was skipped: Rosaclef cannot automate that control",
                    clips[0].name
                ));
                continue;
            };
            let Ok(info) = model.target.resolve(&self.project) else {
                continue;
            };
            let target = model.target.to_string();
            if self.project.automation.iter().any(|l| l.target == target) {
                self.warn.add(format!(
                    "automation \"{}\" was skipped: another lane already drives {target}",
                    clips[0].name
                ));
                continue;
            }
            cubic |= clips.iter().any(|c| c.progression == 2);
            let base = model.target.base_value(&self.project).unwrap_or(info.min);
            let conv = |v: f64| clamp(v * model.scale, info.min, info.max);
            let at = |tick: f64| beats(tick, TICKS_PER_BEAT);
            let point = |beat: f64, value: f64| AutomationPoint {
                beat,
                value,
                curve: 0.0,
            };
            let mut pts: Vec<AutomationPoint> = vec![];
            if clips[0].start > 0.0 {
                pts.push(point(0.0, base));
            }
            for (i, c) in clips.iter().enumerate() {
                // The pattern rules until the next one starts or it ends.
                let stop = match clips.get(i + 1) {
                    Some(next) => (next.start - c.start).min(c.len),
                    None => c.len,
                };
                if let Some(last) = pts.last().copied() {
                    if last.beat < at(c.start) {
                        pts.push(point(at(c.start), last.value));
                    }
                }
                let mut prev = conv(c.value_at(0.0));
                pts.push(point(at(c.start), prev));
                for &(tick, v) in &c.points {
                    if tick <= 0.0 || tick >= stop {
                        continue;
                    }
                    let v = conv(v);
                    if c.progression == 0 {
                        // Discrete: a vertical step.
                        if v != prev {
                            pts.push(point(at(c.start + tick), prev));
                            pts.push(point(at(c.start + tick), v));
                        }
                    } else {
                        pts.push(point(at(c.start + tick), v));
                    }
                    prev = v;
                }
                if stop > 0.0 {
                    pts.push(point(at(c.start + stop), conv(c.value_at(stop))));
                }
            }
            // Drop points in the middle of flat stretches, and a flat end
            // (a lane holds its last value).
            let pts: Vec<AutomationPoint> = (0..pts.len())
                .filter(|&i| {
                    i == 0
                        || pts[i - 1].value != pts[i].value
                        || pts.get(i + 1).is_some_and(|n| n.value != pts[i].value)
                })
                .map(|i| pts[i])
                .collect();
            if pts.iter().all(|p| p.value == base) {
                continue;
            }
            // LMMS names patterns after their first control, which goes
            // stale when they are reconnected: name the lane by its target.
            let name = info.label.clone();
            self.project.automation.push(AutomationLane {
                id: lane_ids.make(&name),
                name,
                target,
                color: model.color.clone(),
                mute: false,
                points: pts,
            });
        }
        if cubic {
            self.warn
                .add("cubic automation curves were approximated by straight lines");
        }
    }

    // ------------------------------------------------------------ samples

    /// Resolve an LMMS sample path and schedule its copy into `samples/`.
    /// Unresolvable files still get a `samples/<name>` reference (and a
    /// warning) so the producer can drop the file in later.
    fn sample(&mut self, src: &str, owner: &str) -> Option<String> {
        let src = src.trim();
        if src.is_empty() {
            return None;
        }
        let rel = Path::new(src);
        let base = rel
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "sample.wav".into());
        let mut candidates = vec![];
        if rel.is_absolute() {
            candidates.push(rel.to_path_buf());
        } else {
            if let Some(d) = &self.opts.source_dir {
                candidates.push(d.join(rel));
                candidates.push(d.join(&base));
                candidates.push(d.join("samples").join(&base));
            }
            for d in &self.opts.sample_dirs {
                candidates.push(d.join(rel));
            }
        }
        let found = candidates.into_iter().find(|p| p.is_file());
        if let Some(from) = &found {
            if let Some(dest) = self.sample_map.get(from) {
                return Some(dest.clone());
            }
        }
        let dest = self.unique_dest(&base);
        match found {
            Some(from) => {
                self.sample_map.insert(from.clone(), dest.clone());
                self.samples.push(SampleCopy {
                    from,
                    to: dest.clone(),
                });
            }
            None => {
                self.sample_map.insert(PathBuf::from(src), dest.clone());
                self.warn.add(format!(
                    "{owner}: sample \"{src}\" was not found; copy it to {dest}"
                ));
            }
        }
        Some(dest)
    }

    fn unique_dest(&mut self, name: &str) -> String {
        let clean: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || "._- ".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let clean = clean.trim_start_matches('.');
        let clean = if clean.is_empty() {
            "sample.wav"
        } else {
            clean
        };
        let (stem, ext) = match clean.rfind('.') {
            Some(i) if i > 0 => (&clean[..i], &clean[i..]),
            _ => (clean, ""),
        };
        let mut dest = format!("samples/{stem}{ext}");
        let mut n = 2;
        while self.sample_dests.contains(&dest) {
            dest = format!("samples/{stem}-{n}{ext}");
            n += 1;
        }
        self.sample_dests.insert(dest.clone());
        dest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_both_framings() {
        let xml = "<?xml version=\"1.0\"?>\n<lmms-project/>";
        assert_eq!(decode(xml.as_bytes()).unwrap(), xml);
        assert_eq!(decode(&encode_mmpz(xml)).unwrap(), xml);
        assert!(decode(b"\x00\x00\x00\x10garbage-garbage").is_err());
    }
}
