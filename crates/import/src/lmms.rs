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
//! ## Mapping
//! | LMMS | Rosaclef |
//! |---|---|
//! | `head` bpm, time signature | `transport` |
//! | instrument track (`type="0"`) | channel + one pattern per distinct LMMS pattern, placed as clips |
//! | Beat+Bassline track (`type="1"`) | one multi-channel pattern per B&B, a clip wherever its `bbtco` appears |
//! | sample track (`type="2"`) | audio clips (files copied into `samples/`) |
//! | FX mixer channels | mixer inserts (name, volume, mute, some effects) |
//! | TripleOscillator | `synth` (Aurum) |
//! | Kicker | `drum` kick |
//! | AudioFileProcessor | `sampler` (Vault) |
//! | LB302 | `cuivre` (or `synth`) acid bass |
//! | other instruments | `synth` fallback, with a warning |
//!
//! Automation, per-clip mutes, sends between FX channels and unsupported
//! plugins are skipped with a warning.

use crate::{beats, clamp, color, ensure_valid, has_instrument, pad_tracks, set_option, set_param, Ids, Imported, SampleCopy, Warnings, MAX_INSERTS};
use anyhow::{anyhow, bail, Context, Result};
use rosaclef_core::{Channel, Clip, Device, Insert, InsertIx, Note, Pattern, Project, Track, TrackIx};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{HashMap, HashSet};
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
        Options { title: title.to_string(), source_dir: None, sample_dirs: default_sample_dirs() }
    }
}

/// Where LMMS keeps its factory and user samples on common installs.
pub fn default_sample_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from("/usr/share/lmms/samples"), PathBuf::from("/usr/local/share/lmms/samples")];
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
    let doc = Document::parse_with_options(&text, ParsingOptions { allow_dtd: true, ..Default::default() }).context("parsing the LMMS project XML")?;
    let root = doc.root_element();
    if root.tag_name().name() != "lmms-project" {
        bail!("not an LMMS project (root element <{}>)", root.tag_name().name());
    }
    let mut im = Importer::new(opts);
    im.run(root)?;
    let Importer { project, warn, samples, .. } = im;
    ensure_valid(&project)?;
    Ok(Imported { project, samples, warnings: warn.finish() })
}

// ---------------------------------------------------------------- XML helpers

fn is(n: &Node, names: &[&str]) -> bool {
    n.is_element() && names.contains(&n.tag_name().name())
}

fn child<'a, 'i>(n: Node<'a, 'i>, names: &[&str]) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is(c, names))
}

fn children<'a, 'i>(n: Node<'a, 'i>, names: &'static [&'static str]) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |c| is(c, names))
}

/// A numeric setting: an attribute, or (when automated) a child element
/// with a `value` attribute.
fn num(n: Node, name: &str) -> Option<f64> {
    if let Some(v) = n.attribute(name) {
        return v.trim().parse().ok();
    }
    n.children().find(|c| c.is_element() && c.tag_name().name() == name).and_then(|c| c.attribute("value")).and_then(|v| v.trim().parse().ok())
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
        }
    }

    fn run(&mut self, root: Node) -> Result<()> {
        let version = text(root, "creatorversion");
        self.project.meta.description = if version.is_empty() { "Imported from LMMS".into() } else { format!("Imported from LMMS {version}") };

        // Tempo and time signature.
        if let Some(head) = child(root, &["head"]) {
            let bpm = num_or(head, "bpm", 120.0);
            if !(20.0..=999.0).contains(&bpm) {
                self.warn.add(format!("tempo {bpm} BPM is outside 20..999 and was clamped"));
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
            self.project.mixer.inserts[0].volume = clamp(master_vol / 100.0, 0.0, 2.0);
        } else {
            self.warn.add("the project has no <head>; using 120 BPM in 4/4");
        }
        let song = child(root, &["song"]).ok_or_else(|| anyhow!("not an LMMS song project (no <song> element)"))?;
        self.build_mixer(song);

        let container = child(song, &["trackcontainer"]).ok_or_else(|| anyhow!("the project has no song track container"))?;
        let tracks: Vec<Node> = children(container, &["track"]).collect();
        let bb_tracks: Vec<Node> = tracks.iter().copied().filter(|t| text(*t, "type") == "1").collect();
        let mut bb_patterns: Option<Vec<Option<String>>> = None;
        for t in &tracks {
            match text(*t, "type") {
                "0" | "" => self.instrument_track(*t),
                "1" => {
                    if bb_patterns.is_none() {
                        bb_patterns = Some(self.build_bb(&bb_tracks));
                    }
                    let idx = bb_tracks.iter().position(|b| b == t).unwrap_or(0);
                    let pat = bb_patterns.as_ref().and_then(|v| v.get(idx).cloned()).flatten();
                    self.bb_track(*t, pat);
                }
                "2" => self.sample_track(*t),
                "5" | "6" => self.warn.add("automation tracks were skipped"),
                other => self.warn.add(format!("track \"{}\" of unsupported type {other} was skipped", text(*t, "name"))),
            }
        }
        for t in children(song, &["track"]) {
            if matches!(text(t, "type"), "5" | "6") {
                self.warn.add("automation tracks were skipped");
            }
        }
        if self.loud_notes > 0 {
            self.warn.add(format!("{} note(s) louder than 100% were clamped to full velocity", self.loud_notes));
        }
        // Keep a limiter last on the master, as in every Rosaclef project.
        let master = &mut self.project.mixer.inserts[0];
        if master.effects.last().map(|e| e.kind != "limiter").unwrap_or(true) {
            master.effects.push(Device::new("limiter"));
        }
        pad_tracks(&mut self.project, 8);
        Ok(())
    }

    // ------------------------------------------------------------ mixer

    fn build_mixer(&mut self, song: Node) {
        let Some(mixer) = child(song, &["fxmixer", "mixer"]) else { return };
        let chans: Vec<Node> = children(mixer, &["fxchannel", "mixerchannel"]).collect();
        let max = chans.iter().filter_map(|c| num(*c, "num")).fold(0.0, f64::max) as usize;
        if max + 1 > MAX_INSERTS {
            self.warn.add(format!("the FX mixer has {} channels; only the first {MAX_INSERTS} were imported", max + 1));
        }
        let n = (max + 1).min(MAX_INSERTS);
        while self.project.mixer.inserts.len() < n {
            let i = self.project.mixer.inserts.len();
            self.project.mixer.inserts.push(Insert::new(&format!("FX {i}")));
        }
        for c in chans {
            let i = num_or(c, "num", 0.0) as usize;
            if i >= n {
                continue;
            }
            let name = text(c, "name");
            let vol = num_or(c, "volume", 1.0);
            let effects = child(c, &["fxchain"]).map(|fx| self.effects(fx, &format!("FX channel \"{name}\""))).unwrap_or_default();
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
            ins.volume = clamp(ins.volume * vol, 0.0, 2.0);
            ins.mute = flag(c, "muted");
            ins.effects.extend(effects);
        }
    }

    /// The insert for an LMMS FX channel number.
    fn insert(&mut self, fx: f64, owner: &str) -> InsertIx {
        let i = fx.max(0.0) as usize;
        if i < self.project.mixer.inserts.len() {
            return InsertIx(i as u32);
        }
        self.warn.add(format!("{owner} is routed to FX {i}, which does not exist; routed to the master"));
        InsertIx::MASTER
    }

    /// Map an LMMS effect chain onto built-in effects.
    fn effects(&mut self, chain: Node, owner: &str) -> Vec<Device> {
        let mut out = vec![];
        for e in children(chain, &["effect"]) {
            let name = text(e, "name");
            let wet = clamp(num_or(e, "wet", 1.0), 0.0, 1.0);
            // The plugin's own settings live in the first element that is not <key>.
            let settings = e.children().find(|c| c.is_element() && c.tag_name().name() != "key");
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
                    set_param(&mut d, "feedback", get("FeebackAmount", get("FeedbackAmount", 0.5)));
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
                    let mut d = Device::new("eq");
                    set_param(&mut d, "low", 20.0 * get("gain", 1.0).max(0.01).log10() + 6.0);
                    set_param(&mut d, "lowFreq", get("freq", 100.0));
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
                    self.warn.add(format!("effect \"{other}\" on {owner} has no Rosaclef equivalent and was skipped"));
                    continue;
                }
            };
            self.warn.add(format!("LMMS effect \"{name}\" was approximated by the built-in \"{}\"", dev.kind));
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
        let (instrument, mode, transpose) = self.instrument(name, it, has_steps);
        let vol = num_or(it, "vol", 100.0);
        if vol > 150.0 {
            self.warn.add(format!("track \"{name}\": volume {vol}% was clamped to 150%"));
        }
        let fx = num(it, "fxch").or_else(|| num(it, "mixch")).unwrap_or(0.0);
        let mut mixer = self.insert(fx, &format!("track \"{name}\""));
        // Track-level effects get an insert of their own.
        if let Some(chain) = child(it, &["fxchain"]) {
            let fx = self.effects(chain, &format!("track \"{name}\""));
            if !fx.is_empty() {
                if self.project.mixer.inserts.len() < MAX_INSERTS {
                    if mixer != InsertIx::MASTER {
                        self.warn.add(format!("track \"{name}\": its effects moved to a new insert, which feeds the master instead of FX {}", mixer.0));
                    }
                    let mut ins = Insert::new(&format!("{name} FX"));
                    ins.effects = fx;
                    mixer = InsertIx(self.project.mixer.inserts.len() as u32);
                    self.project.mixer.inserts.push(ins);
                } else {
                    self.warn.add(format!("track \"{name}\": no free insert for its effects; they were skipped"));
                }
            }
        }
        let id = self.channel_ids.make(name);
        let track_color = text(track, "color");
        let color = if track_color.len() == 7 && track_color.starts_with('#') && track_color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            track_color.to_lowercase()
        } else {
            color(self.project.channels.len())
        };
        if flag(track, "solo") {
            self.warn.add(format!("track \"{name}\" was soloed; solo is not imported"));
        }
        self.project.channels.push(Channel {
            id: id.clone(),
            name: name.to_string(),
            color,
            instrument,
            volume: clamp(vol / 100.0, 0.0, 1.5),
            pan: clamp(num_or(it, "pan", 0.0) / 100.0, -1.0, 1.0),
            mute: flag(track, "muted"),
            mixer,
        });
        Some(ChannelInfo { id, base_note, transpose, mode })
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
                let mut d = Device::new("synth");
                let osc: Vec<(usize, f64, f64, f64, f64)> = (0..3)
                    .map(|i| {
                        let fine = (get(&format!("finel{i}"), 0.0) + get(&format!("finer{i}"), 0.0)) / 2.0;
                        (i, get(&format!("vol{i}"), 33.0), get(&format!("wavetype{i}"), 0.0), get(&format!("coarse{i}"), 0.0), fine)
                    })
                    .filter(|o| o.1 > 0.0)
                    .collect();
                let wave = |w: f64| match w as i32 {
                    0 => "sine",
                    1 | 5 => "triangle",
                    3 => "square",
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
                        } else {
                            self.warn.add(format!("track \"{track}\": TripleOscillator's third oscillator was dropped (Aurum has two plus a sub)"));
                        }
                    }
                    let total: f64 = osc.iter().map(|o| o.1).sum();
                    set_param(&mut d, "gain", 0.6 * clamp(total / 100.0, 0.2, 2.5));
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
                set_param(&mut d, "tune", 12.0 * (get("endfreq", 40.0).max(5.0) / 45.0).log2());
                set_param(&mut d, "decay", get("decay", 440.0) / 440.0);
                set_param(&mut d, "snap", get("click", 0.4));
                set_param(&mut d, "drive", get("dist", 0.8) / 10.0);
                set_param(&mut d, "gain", 0.8 * get("gain", 1.0));
                self.warn.add(format!("track \"{track}\": Kicker was approximated by the Atelier drum"));
                let follows = get("startnote", 1.0) != 0.0 || get("endnote", 0.0) != 0.0;
                (d, if follows { PitchMode::Relative } else { PitchMode::Fixed }, 0)
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
                set_param(&mut d, "gain", 0.8 * get("amp", 100.0) / 100.0);
                let looped = get("looped", 0.0) as i32 != 0;
                set_option(&mut d, "mode", if looped { "loop" } else if has_steps { "oneshot" } else { "pitched" });
                if get("reversed", 0.0) != 0.0 {
                    self.warn.add(format!("track \"{track}\": reversed playback is not supported; the sample plays forwards"));
                }
                (d, PitchMode::Normal, 0)
            }
            "lb302" => {
                let kind = if has_instrument("cuivre") { "cuivre" } else { "synth" };
                let mut d = Device::new(kind);
                let shape = get("shape", 0.0) as i32;
                let (w, noise) = match (kind, shape) {
                    ("cuivre", 1 | 5) => ("triangle", 0.0),
                    ("cuivre", 2 | 3) => ("pulse", 0.0),
                    ("cuivre", 7) => ("saw", 0.8),
                    ("cuivre", _) => ("saw", 0.0),
                    (_, 1 | 5) => ("triangle", 0.0),
                    (_, 2 | 3) => ("square", 0.0),
                    _ => ("saw", 0.0),
                };
                set_option(&mut d, "wave1", w);
                let cutoff = 80.0 * 2f64.powf(get("vcf_cut", 0.75) * 7.0);
                set_param(&mut d, "cutoff", cutoff);
                set_param(&mut d, "resonance", get("vcf_res", 0.75) * 0.8);
                set_param(&mut d, "filterEnv", 0.2 + get("vcf_mod", 0.1) * 0.8);
                set_param(&mut d, "filterDecay", 0.05 + get("vcf_dec", 0.1) * 1.2);
                set_param(&mut d, "attack", 0.002);
                set_param(&mut d, "decay", 0.4);
                set_param(&mut d, "sustain", 0.85);
                set_param(&mut d, "release", 0.03);
                let slide = get("slide", 0.0) != 0.0;
                set_param(&mut d, "glide", if slide { 0.02 + get("slide_dec", 0.6) * 0.2 } else { 0.0001 });
                if kind == "cuivre" {
                    set_param(&mut d, "mix2", 0.0);
                    set_param(&mut d, "sub", 0.0);
                    set_param(&mut d, "noise", noise);
                    set_param(&mut d, "drift", 0.1);
                    set_param(&mut d, "drive", get("dist", 0.0));
                    set_param(&mut d, "filterAttack", 0.002);
                    set_param(&mut d, "filterSustain", 0.0);
                    set_option(&mut d, "filter", if get("db24", 0.0) != 0.0 { "ladder" } else { "screamer" });
                    set_option(&mut d, "mode", if slide { "legato" } else { "mono" });
                } else {
                    set_param(&mut d, "osc2Mix", 0.0);
                }
                self.warn.add(format!("track \"{track}\": LB302 was approximated by {}", if kind == "cuivre" { "Cuivre" } else { "Aurum (synth)" }));
                (d, PitchMode::Normal, 0)
            }
            "malletsstk" => {
                let d = rosaclef_core::presets::find("Crystal Mallet").filter(|p| p.kind == "fm").map(|p| p.device()).unwrap_or_else(|| Device::new("fm"));
                self.warn.add(format!("track \"{track}\": Mallets was approximated by Lumière (fm)"));
                (d, PitchMode::Normal, 0)
            }
            other => {
                let mut d = Device::new("synth");
                self.envelope(it, &mut d);
                let what = if other.is_empty() { "an empty instrument".to_string() } else { format!("instrument \"{other}\"") };
                self.warn.add(format!("track \"{track}\": {what} is not supported; replaced by Aurum (synth)"));
                (d, PitchMode::Normal, 0)
            }
        }
    }

    /// Volume envelope and filter of an instrument track (`<eldata>`).
    /// LMMS envelope knobs (0..2) map to seconds as `5 · v²`.
    fn envelope(&mut self, it: Node, d: &mut Device) {
        let secs = |v: f64| 5.0 * v * v;
        let ed = child(it, &["eldata"]);
        let vol_env = ed.and_then(|e| child(e, &["elvol"])).filter(|v| num_or(*v, "amt", 0.0) > 0.0);
        match vol_env {
            Some(v) => {
                set_param(d, "attack", secs(num_or(v, "att", 0.0)).max(0.001));
                set_param(d, "decay", secs(num_or(v, "dec", 0.5)).max(0.001));
                set_param(d, "sustain", 1.0 - num_or(v, "sus", 0.5));
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
                        self.warn.add(format!("instrument filter type {t} was approximated by a low-pass"));
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
            PitchMode::Normal => key + KEY_TO_MIDI + (DEFAULT_BASE_NOTE - ch.base_note) + self.master_pitch + ch.transpose,
            PitchMode::Fixed => 60,
            PitchMode::Relative => 60 + key - ch.base_note,
        }
    }

    /// Notes of an LMMS pattern, in beats relative to the pattern start.
    fn notes(&mut self, pattern: Node, ch: &ChannelInfo, track: &str) -> Vec<Note> {
        let mut out = vec![];
        let mut dropped = 0;
        for n in children(pattern, &["note"]) {
            let key = num_or(n, "key", DEFAULT_BASE_NOTE as f64).round() as i32;
            let pitch = self.pitch(key, ch);
            if !(0..=127).contains(&pitch) {
                dropped += 1;
                continue;
            }
            let len = num_or(n, "len", 48.0);
            let vol = num_or(n, "vol", 100.0);
            if vol > 100.0 {
                self.loud_notes += 1;
            }
            out.push(Note {
                channel: ch.id.clone(),
                pitch,
                start: beats(num_or(n, "pos", 0.0).max(0.0), TICKS_PER_BEAT),
                length: if len > 0.0 { beats(len, TICKS_PER_BEAT) } else { STEP_BEATS },
                velocity: clamp(vol / 100.0, 0.0, 1.0),
            });
        }
        if dropped > 0 {
            self.warn.add(format!("track \"{track}\": {dropped} note(s) outside the MIDI range were dropped"));
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        out
    }

    /// Length of an LMMS pattern in ticks.
    fn pattern_ticks(&self, pattern: Node) -> f64 {
        let len = num_or(pattern, "len", 0.0);
        if len > 0.0 {
            return len;
        }
        let steps = num_or(pattern, "steps", 0.0);
        if steps > 0.0 {
            return steps * 12.0;
        }
        let end = children(pattern, &["note"]).map(|n| num_or(n, "pos", 0.0) + num_or(n, "len", 12.0).max(12.0)).fold(0.0, f64::max);
        ((end / self.ticks_per_bar).ceil().max(1.0)) * self.ticks_per_bar
    }

    fn has_steps(track: Node) -> bool {
        track.descendants().filter(|n| n.is_element() && n.tag_name().name() == "note").any(|n| num_or(n, "len", 48.0) <= 0.0)
    }

    fn add_track(&mut self, track: Node) -> TrackIx {
        let name = text(track, "name");
        let ix = TrackIx(self.project.playlist.tracks.len() as u32);
        self.project.playlist.tracks.push(Track { name: if name.is_empty() { format!("Track {}", ix.0 + 1) } else { name.to_string() }, mute: flag(track, "muted") });
        ix
    }

    // ------------------------------------------------------------ tracks

    fn instrument_track(&mut self, track: Node) {
        let name = text(track, "name").to_string();
        let Some(ch) = self.add_channel(track, Self::has_steps(track)) else {
            self.warn.add(format!("track \"{name}\" has no instrument settings and was skipped"));
            return;
        };
        let tix = self.add_track(track);
        let color = self.project.channels.last().map(|c| c.color.clone()).unwrap_or_else(|| color(0));
        let mut count = 0;
        for pat in children(track, &["pattern", "midiclip"]) {
            let start = beats(num_or(pat, "pos", 0.0).max(0.0), TICKS_PER_BEAT);
            let length = beats(self.pattern_ticks(pat), TICKS_PER_BEAT);
            if flag(pat, "muted") {
                self.warn.add(format!("track \"{name}\": a muted clip at beat {start} was skipped"));
                continue;
            }
            let notes = self.notes(pat, &ch, &name);
            let key = format!("{}|{length}|{notes:?}", ch.id);
            let id = match self.dedupe.get(&key) {
                Some(id) => id.clone(),
                None => {
                    count += 1;
                    let label = text(pat, "name");
                    let pname = if !label.is_empty() { label.to_string() } else if count == 1 { name.clone() } else { format!("{name} {count}") };
                    let id = self.pattern_ids.make(&pname);
                    self.project.patterns.push(Pattern { id: id.clone(), name: pname, color: color.clone(), length, notes });
                    self.dedupe.insert(key, id.clone());
                    id
                }
            };
            self.project.playlist.clips.push(Clip { pattern: id, sample: String::new(), track: tix, start, length, offset: 0.0, gain: 1.0, mixer: InsertIx::MASTER });
        }
    }

    /// Build one pattern per Beat+Bassline from the B&B track container
    /// (saved inside the first B&B track). Returns the pattern id per B&B.
    fn build_bb(&mut self, bb_tracks: &[Node]) -> Vec<Option<String>> {
        let n = bb_tracks.len();
        let container = bb_tracks.iter().find_map(|t| child(*t, &["bbtrack", "patterntrack"]).and_then(|b| child(b, &["trackcontainer"])));
        let mut notes: Vec<Vec<Note>> = vec![vec![]; n];
        let mut ticks: Vec<f64> = vec![0.0; n];
        if let Some(c) = container {
            for inner in children(c, &["track"]) {
                let name = text(inner, "name").to_string();
                if !matches!(text(inner, "type"), "0" | "") {
                    self.warn.add(format!("Beat+Bassline track \"{name}\" is not an instrument track and was skipped"));
                    continue;
                }
                let Some(ch) = self.add_channel(inner, Self::has_steps(inner)) else { continue };
                for (ord, pat) in children(inner, &["pattern", "midiclip"]).enumerate() {
                    // B&B k's pattern sits at bar k of the container.
                    let pos = num_or(pat, "pos", -1.0);
                    let k = if pos >= 0.0 { (pos / self.ticks_per_bar).round() as usize } else { ord };
                    if k >= n {
                        continue;
                    }
                    ticks[k] = ticks[k].max(self.pattern_ticks(pat));
                    let mut ns = self.notes(pat, &ch, &name);
                    notes[k].append(&mut ns);
                }
            }
        } else if n > 0 {
            self.warn.add("the Beat+Bassline editor's tracks were not found; B&B clips are empty");
        }
        (0..n)
            .map(|k| {
                let used = bb_tracks[k].children().any(|c| is(&c, &["bbtco", "patternclip"]));
                if notes[k].is_empty() && !used {
                    return None;
                }
                let bars = (ticks[k] / self.ticks_per_bar).ceil().max(1.0);
                let name = text(bb_tracks[k], "name");
                let name = if name.is_empty() { format!("Beat/Bassline {k}") } else { name.to_string() };
                let id = self.pattern_ids.make(&name);
                let mut ns = std::mem::take(&mut notes[k]);
                ns.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
                self.project.patterns.push(Pattern { id: id.clone(), name, color: color(self.project.patterns.len() + 3), length: bars * self.bar_beats, notes: ns });
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
                self.warn.add(format!("track \"{name}\": a muted clip at beat {start} was skipped"));
                continue;
            }
            let Some(id) = pattern.clone() else { continue };
            if length > 0.0 {
                self.project.playlist.clips.push(Clip { pattern: id, sample: String::new(), track: tix, start, length, offset: 0.0, gain: 1.0, mixer: InsertIx::MASTER });
            }
        }
    }

    fn sample_track(&mut self, track: Node) {
        let name = text(track, "name").to_string();
        let tix = self.add_track(track);
        let st = child(track, &["sampletrack"]);
        let vol = st.map(|s| num_or(s, "vol", 100.0)).unwrap_or(100.0);
        let fx = st.and_then(|s| num(s, "fxch").or_else(|| num(s, "mixch"))).unwrap_or(0.0);
        let mixer = self.insert(fx, &format!("sample track \"{name}\""));
        if st.map(|s| num_or(s, "pan", 0.0) != 0.0).unwrap_or(false) {
            self.warn.add(format!("sample track \"{name}\": panning is not imported (audio clips have no pan)"));
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
                self.warn.add(format!("sample track \"{name}\": a muted clip at beat {start} was skipped"));
                continue;
            }
            if length <= 0.0 {
                continue;
            }
            let Some(path) = self.sample(text(tco, "src"), &format!("sample track \"{name}\"")) else {
                self.warn.add(format!("sample track \"{name}\": a clip without a sample file (embedded data) was skipped"));
                continue;
            };
            // A negative start offset means the clip's left edge was trimmed.
            let offset = beats((-num_or(tco, "off", 0.0)).max(0.0), TICKS_PER_BEAT);
            self.project.playlist.clips.push(Clip { pattern: String::new(), sample: path, track: tix, start, length, offset, gain: clamp(vol / 100.0, 0.0, 4.0), mixer });
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
        let base = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "sample.wav".into());
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
                self.samples.push(SampleCopy { from, to: dest.clone() });
            }
            None => {
                self.sample_map.insert(PathBuf::from(src), dest.clone());
                self.warn.add(format!("{owner}: sample \"{src}\" was not found; copy it to {dest}"));
            }
        }
        Some(dest)
    }

    fn unique_dest(&mut self, name: &str) -> String {
        let clean: String = name.chars().map(|c| if c.is_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
        let clean = clean.trim_start_matches('.');
        let clean = if clean.is_empty() { "sample.wav" } else { clean };
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
