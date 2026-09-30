//! Rosaclef audio engine.
//!
//! A portable, allocation-light DSP engine: sequencer, instruments, effects
//! and mixer. It performs no I/O and spawns no threads, so the same code runs
//! natively (driven by the device callback or an offline renderer) and in the
//! browser (compiled to WebAssembly and driven by an `AudioWorklet`).
//!
//! External plugins (CLAP, ...) are supported through the [`PluginHost`]
//! trait, which only native hosts implement.

mod automation;
pub mod dsp;
pub mod effects;
pub mod instruments;
pub mod render;
pub mod samples;
pub mod soundfont;

use automation::CLane;
use dsp::{hermite, pan_gains, Ramp};
use effects::Effect;
use instruments::{Instrument, NoteEvent, NoteKind};
use rosaclef_core::automation::TempoMap;
use rosaclef_core::{Device, InsertIx, Project};
use samples::{PresetKey, SampleBank, SampleData, SampleRef};
use std::sync::Arc;

/// Largest block processed in one go; larger requests are split.
pub const MAX_BLOCK: usize = 128;

/// Largest block between two automation updates (when the project has lanes).
pub const AUTOMATION_BLOCK: usize = 64;

/// Devices whose settings depend on the tempo (they are reconfigured when
/// tempo automation moves the tempo).
fn tempo_synced(kind: &str) -> bool {
    matches!(kind, "delay" | "comete" | "dedale")
}

/// Processing context handed to devices.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub sr: f32,
    pub bpm: f32,
}

// ------------------------------------------------------------------ plugins

/// A loaded external plugin instance (instrument or effect).
pub trait ExternalProcessor: Send {
    /// Set a plugin parameter by its id (as written in the project).
    fn set_param(&mut self, id: &str, value: f64);
    /// Process one block. Instruments get zeroed buffers and must write their
    /// output; effects process the buffers in place. `events` is empty for
    /// effects.
    fn process(&mut self, events: &[NoteEvent], left: &mut [f32], right: &mut [f32]);
}

/// Something that can instantiate external plugins (implemented natively).
pub trait PluginHost: Send + Sync {
    fn load(
        &self,
        dev: &Device,
        instrument: bool,
        sample_rate: f32,
        max_block: usize,
    ) -> Result<Box<dyn ExternalProcessor>, String>;
}

struct PluginInstrument {
    proc_: Box<dyn ExternalProcessor>,
    tmp_l: Vec<f32>,
    tmp_r: Vec<f32>,
}

impl Instrument for PluginInstrument {
    fn set_device(&mut self, dev: &Device, _ctx: &Ctx) {
        for (k, v) in &dev.params {
            self.proc_.set_param(k, *v);
        }
    }
    fn handle(&mut self, _ev: NoteKind) {}
    fn render(&mut self, _l: &mut [f32], _r: &mut [f32]) {}
    fn process(&mut self, events: &[NoteEvent], left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.tmp_l[..n].fill(0.0);
        self.tmp_r[..n].fill(0.0);
        self.proc_
            .process(events, &mut self.tmp_l[..n], &mut self.tmp_r[..n]);
        for i in 0..n {
            left[i] += self.tmp_l[i];
            right[i] += self.tmp_r[i];
        }
    }
}

struct PluginEffect(Box<dyn ExternalProcessor>);

impl Effect for PluginEffect {
    fn set_device(&mut self, dev: &Device, _ctx: &Ctx) {
        for (k, v) in &dev.params {
            self.0.set_param(k, *v);
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.0.process(&[], left, right);
    }
}

// ---------------------------------------------------------------- transport

#[derive(Clone, Debug, PartialEq)]
pub enum PlayMode {
    /// Loop a single pattern.
    Pattern(String),
    /// Play the playlist arrangement.
    Song,
}

// ------------------------------------------------------------------ runtime

/// Identity of a device instance: settings that require a new instance
/// when they change.
fn signature(dev: &Device) -> String {
    match dev.kind.as_str() {
        "plugin" => format!(
            "plugin|{}|{}|{}",
            dev.option("format"),
            dev.option("path"),
            dev.option("id")
        ),
        k => k.to_string(),
    }
}

/// Stable identity of a channel instance inside the engine (survives
/// project updates that reorder channels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChannelHandle(u32);

struct ChannelRt {
    handle: ChannelHandle,
    id: String,
    sig: String,
    inst: Box<dyn Instrument>,
    /// Working copy of the instrument settings (automation writes here).
    dev: Device,
    tempo_synced: bool,
    volume: f32,
    pan: f32,
    mute: bool,
    gain_l: Ramp,
    gain_r: Ramp,
    mixer: InsertIx,
    events: Vec<NoteEvent>,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    peak: f32,
}

impl ChannelRt {
    fn update_gains(&mut self) {
        let (pl, pr) = pan_gains(self.pan);
        let v = if self.mute { 0.0 } else { self.volume };
        self.gain_l.set(v * pl);
        self.gain_r.set(v * pr);
    }
}

struct FxRt {
    sig: String,
    enabled: bool,
    fx: Box<dyn Effect>,
    /// Working copy of the effect settings (automation writes here).
    dev: Device,
    tempo_synced: bool,
}

struct InsertRt {
    fx: Vec<FxRt>,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    volume: f32,
    pan: f32,
    mute: bool,
    gain_l: Ramp,
    gain_r: Ramp,
    audible: bool,
    peak: [f32; 2],
}

impl InsertRt {
    fn new() -> InsertRt {
        InsertRt {
            fx: vec![],
            buf_l: vec![0.0; MAX_BLOCK],
            buf_r: vec![0.0; MAX_BLOCK],
            volume: 1.0,
            pan: 0.0,
            mute: false,
            gain_l: Ramp::default(),
            gain_r: Ramp::default(),
            audible: true,
            peak: [0.0; 2],
        }
    }

    fn update_gains(&mut self) {
        let v = if self.mute { 0.0 } else { self.volume };
        self.gain_l.set(v * (1.0 - self.pan).min(1.0));
        self.gain_r.set(v * (1.0 + self.pan).min(1.0));
    }
}

#[derive(Clone, Copy)]
struct CNote {
    channel: usize,
    key: u8,
    /// Start without swing.
    start: f64,
    /// On an off-beat 16th: delayed by the swing amount.
    swung: bool,
    length: f64,
    velocity: f32,
}

impl CNote {
    #[inline]
    fn start_at(&self, swing_shift: f64) -> f64 {
        if self.swung {
            self.start + swing_shift
        } else {
            self.start
        }
    }
}

struct CPattern {
    id: String,
    length: f64,
    notes: Vec<CNote>,
}

struct CClip {
    pattern: Option<usize>,
    sample_path: String,
    sample: Option<SampleRef>,
    start: f64,
    end: f64,
    offset: f64,
    gain: f32,
    mixer: InsertIx,
}

#[derive(Clone, Copy)]
struct Pending {
    handle: ChannelHandle,
    key: u8,
    end: f64,
}

/// Levels reported to the UI.
#[derive(Clone, Debug, Default)]
pub struct Meters {
    /// Peak per mixer insert (left, right), since the last call.
    pub inserts: Vec<[f32; 2]>,
    /// Peak per channel (mono), since the last call.
    pub channels: Vec<f32>,
}

pub struct Engine {
    ctx: Ctx,
    project: Project,
    channels: Vec<ChannelRt>,
    inserts: Vec<InsertRt>,
    patterns: Vec<CPattern>,
    clips: Vec<CClip>,
    song_length: f64,
    swing: f64,
    /// Compiled automation lanes (song mode).
    lanes: Vec<CLane>,
    /// Automated values are currently applied (not the project's).
    auto_applied: bool,
    /// Keep automated values after the sequencer stops (render tails).
    auto_hold: bool,
    /// Tempo that tempo-synced devices were last configured with.
    synced_bpm: f32,
    tempo_map: TempoMap,
    next_handle: ChannelHandle,
    samples: SampleBank,
    plugins: Option<Arc<dyn PluginHost>>,
    /// Problems encountered while building devices (e.g. a plugin failed to load).
    pub device_errors: Vec<String>,

    playing: bool,
    mode: PlayMode,
    position: f64,
    clock: f64,
    pending: Vec<Pending>,
    /// Clicks on every beat while playing (the downbeat higher).
    metronome: bool,
    /// Count-in before the sequencer starts: its length and the beats left.
    pre_total: f64,
    pre: f64,
    /// Clicks starting in this block (frame, downbeat), and the one sounding.
    clicks: Vec<(usize, bool)>,
    click: Click,
    master_l: Vec<f32>,
    master_r: Vec<f32>,
    dc: [dsp::DcBlock; 2],
}

impl Engine {
    pub fn new(sample_rate: f32) -> Engine {
        let project = Project::empty("Untitled");
        let mut e = Engine {
            ctx: Ctx {
                sr: sample_rate,
                bpm: 120.0,
            },
            project: project.clone(),
            channels: vec![],
            inserts: vec![],
            patterns: vec![],
            clips: vec![],
            song_length: 0.0,
            swing: 0.0,
            lanes: vec![],
            auto_applied: false,
            auto_hold: false,
            synced_bpm: 120.0,
            tempo_map: TempoMap::default(),
            next_handle: ChannelHandle(1),
            samples: SampleBank::default(),
            plugins: None,
            device_errors: vec![],
            playing: false,
            mode: PlayMode::Song,
            position: 0.0,
            clock: 0.0,
            pending: vec![],
            metronome: false,
            pre_total: 0.0,
            pre: 0.0,
            clicks: vec![],
            click: Click::default(),
            master_l: vec![0.0; MAX_BLOCK],
            master_r: vec![0.0; MAX_BLOCK],
            dc: Default::default(),
        };
        e.set_project(project);
        e
    }

    pub fn sample_rate(&self) -> f32 {
        self.ctx.sr
    }

    pub fn set_plugin_host(&mut self, host: Arc<dyn PluginHost>) {
        self.plugins = Some(host);
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    // -------------------------------------------------------------- project

    /// Apply a new version of the project. Instruments and effects whose type
    /// did not change are updated in place, so sounding notes and effect
    /// tails continue.
    pub fn set_project(&mut self, project: Project) {
        self.ctx.bpm = project.transport.bpm as f32;
        self.swing = project.transport.swing;
        self.device_errors.clear();
        let ctx = self.ctx;

        // Channels.
        let mut old: Vec<ChannelRt> = std::mem::take(&mut self.channels);
        for ch in &project.channels {
            let sig = signature(&ch.instrument);
            let reuse = old.iter().position(|o| o.id == ch.id && o.sig == sig);
            let mut rt = match reuse {
                Some(i) => {
                    let mut rt = old.swap_remove(i);
                    rt.inst.set_device(&ch.instrument, &ctx);
                    rt
                }
                None => {
                    let inst = self.make_instrument(&ch.instrument, &ch.id);
                    let handle = self.next_handle;
                    self.next_handle = ChannelHandle(handle.0 + 1);
                    ChannelRt {
                        handle,
                        id: ch.id.clone(),
                        sig,
                        inst,
                        dev: Device::default(),
                        tempo_synced: false,
                        volume: 0.0,
                        pan: 0.0,
                        mute: false,
                        gain_l: Ramp::default(),
                        gain_r: Ramp::default(),
                        mixer: InsertIx::MASTER,
                        events: Vec::with_capacity(64),
                        buf_l: vec![0.0; MAX_BLOCK],
                        buf_r: vec![0.0; MAX_BLOCK],
                        peak: 0.0,
                    }
                }
            };
            rt.inst.set_samples(&self.samples);
            rt.dev = ch.instrument.clone();
            rt.tempo_synced = tempo_synced(&ch.instrument.kind);
            rt.volume = ch.volume as f32;
            rt.pan = ch.pan as f32;
            rt.mute = ch.mute;
            rt.update_gains();
            rt.mixer = clamp_insert(ch.mixer, &project);
            self.channels.push(rt);
        }

        // Inserts and their effect chains.
        let any_solo = project.mixer.inserts.iter().skip(1).any(|i| i.solo);
        self.inserts
            .resize_with(project.mixer.inserts.len(), InsertRt::new);
        for (idx, (ins, rt)) in project
            .mixer
            .inserts
            .iter()
            .zip(self.inserts.iter_mut())
            .enumerate()
        {
            let mut old_fx: Vec<Option<FxRt>> =
                std::mem::take(&mut rt.fx).into_iter().map(Some).collect();
            for (j, dev) in ins.effects.iter().enumerate() {
                let sig = signature(dev);
                let reused = old_fx.get_mut(j).and_then(|slot| {
                    if slot.as_ref().map(|f| f.sig == sig).unwrap_or(false) {
                        slot.take()
                    } else {
                        None
                    }
                });
                let mut f = match reused {
                    Some(f) => f,
                    None => {
                        let fx = make_effect(
                            dev,
                            &ctx,
                            self.plugins.as_deref(),
                            &mut self.device_errors,
                            &format!("mixer.inserts[{idx}].effects[{j}]"),
                        );
                        FxRt {
                            sig,
                            enabled: true,
                            fx,
                            dev: Device::default(),
                            tempo_synced: false,
                        }
                    }
                };
                f.fx.set_device(dev, &ctx);
                f.enabled = dev.enabled;
                f.dev = dev.clone();
                f.tempo_synced = tempo_synced(&dev.kind);
                rt.fx.push(f);
            }
            rt.volume = ins.volume as f32;
            rt.pan = ins.pan as f32;
            rt.mute = ins.mute;
            rt.update_gains();
            rt.audible = idx == 0 || !any_solo || ins.solo;
        }

        // Patterns. Swing delays off-beat 16ths when they are scheduled.
        self.patterns = project
            .patterns
            .iter()
            .map(|p| {
                let mut notes: Vec<CNote> = p
                    .notes
                    .iter()
                    .filter_map(|n| {
                        let channel = project.channels.iter().position(|c| c.id == n.channel)?;
                        let sixteenth = n.start * 4.0;
                        let on_grid = (sixteenth - sixteenth.round()).abs() < 1e-6;
                        let swung = on_grid && (sixteenth.round() as i64) % 2 == 1;
                        Some(CNote {
                            channel,
                            key: n.pitch.clamp(0, 127) as u8,
                            start: n.start,
                            swung,
                            length: n.length.max(1e-4),
                            velocity: n.velocity as f32,
                        })
                    })
                    .collect();
                notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                CPattern {
                    id: p.id.clone(),
                    length: p.length.max(1e-3),
                    notes,
                }
            })
            .collect();

        // Clips.
        let muted_tracks: Vec<bool> = project.playlist.tracks.iter().map(|t| t.mute).collect();
        self.clips = project
            .playlist
            .clips
            .iter()
            .filter(|c| !muted_tracks.get(c.track.index()).copied().unwrap_or(false))
            .map(|c| CClip {
                pattern: if c.pattern.is_empty() {
                    None
                } else {
                    self.patterns.iter().position(|p| p.id == c.pattern)
                },
                sample_path: c.sample.clone(),
                sample: if c.sample.is_empty() {
                    None
                } else {
                    self.samples.get(&c.sample)
                },
                start: c.start,
                end: c.start + c.length,
                offset: c.offset,
                gain: c.gain as f32,
                mixer: clamp_insert(c.mixer, &project),
            })
            .filter(|c| c.pattern.is_some() || !c.sample_path.is_empty())
            .collect();
        self.song_length = project.song_length();

        // Drop note-offs for channels that no longer exist.
        let handles: Vec<ChannelHandle> = self.channels.iter().map(|c| c.handle).collect();
        self.pending.retain(|p| handles.contains(&p.handle));

        if let PlayMode::Pattern(id) = &self.mode {
            if !self.patterns.iter().any(|p| &p.id == id) {
                self.mode = PlayMode::Song;
            }
        }

        // Automation: every value above is the project's own; re-apply the
        // lanes at the current position when they are active.
        self.synced_bpm = self.ctx.bpm;
        self.tempo_map = TempoMap::new(&project);
        self.project = project;
        self.compile_automation();
        self.auto_applied = false;
        if self.automation_active() || (self.auto_hold && !self.lanes.is_empty()) {
            self.apply_automation(0);
        }
    }

    fn make_instrument(&mut self, dev: &Device, channel: &str) -> Box<dyn Instrument> {
        if dev.kind == "plugin" {
            let Some(host) = &self.plugins else {
                self.device_errors.push(format!(
                    "channel {channel}: plugins are not available in this engine"
                ));
                return Box::new(instruments::Silent);
            };
            return match host.load(dev, true, self.ctx.sr, MAX_BLOCK) {
                Ok(p) => {
                    let mut inst = PluginInstrument {
                        proc_: p,
                        tmp_l: vec![0.0; MAX_BLOCK],
                        tmp_r: vec![0.0; MAX_BLOCK],
                    };
                    inst.set_device(dev, &self.ctx);
                    Box::new(inst)
                }
                Err(e) => {
                    self.device_errors.push(format!("channel {channel}: {e}"));
                    Box::new(instruments::Silent)
                }
            };
        }
        instruments::create(dev, &self.ctx).unwrap_or_else(|| Box::new(instruments::Silent))
    }

    // -------------------------------------------------------------- samples

    /// Paths of every audio file the project references.
    pub fn required_samples(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let mut add = |s: &str| {
            if !s.is_empty() && !out.iter().any(|o| o == s) {
                out.push(s.to_string());
            }
        };
        for ch in &self.project.channels {
            match ch.instrument.kind.as_str() {
                "sampler" => add(ch.instrument.option("sample")),
                "nebula" if ch.instrument.option("source") == "sample" => {
                    add(ch.instrument.option("sample"))
                }
                _ => {}
            }
        }
        for c in &self.project.playlist.clips {
            add(&c.sample);
        }
        out
    }

    /// Soundfont presets the project's channels play (see [`soundfont`]).
    pub fn required_presets(&self) -> Vec<PresetKey> {
        let mut out: Vec<PresetKey> = vec![];
        for ch in &self.project.channels {
            if ch.instrument.kind == "soundfont" {
                let k = instruments::SoundFontInst::preset_key(&ch.instrument);
                if !out.contains(&k) {
                    out.push(k);
                }
            }
        }
        out
    }

    pub fn has_preset(&self, key: &PresetKey) -> bool {
        self.samples.has_preset(key)
    }

    /// Provide a loaded soundfont preset.
    pub fn set_preset(&mut self, key: PresetKey, preset: Arc<soundfont::LoadedPreset>) {
        self.samples.insert_preset(key, preset);
        self.relink_samples();
    }

    pub fn has_sample(&self, path: &str) -> bool {
        self.samples.contains(path)
    }

    /// Provide decoded audio for a project-relative path.
    pub fn set_sample(&mut self, path: &str, data: SampleData) {
        self.samples.insert(path, data);
        self.relink_samples();
    }

    pub fn remove_sample(&mut self, path: &str) {
        self.samples.remove(path);
        self.relink_samples();
    }

    fn relink_samples(&mut self) {
        for ch in &mut self.channels {
            ch.inst.set_samples(&self.samples);
        }
        for c in &mut self.clips {
            if !c.sample_path.is_empty() {
                c.sample = self.samples.get(&c.sample_path);
            }
        }
    }

    // ------------------------------------------------------------ transport

    pub fn play(&mut self) {
        if !self.playing {
            self.playing = true;
            self.auto_hold = false;
            self.clock = 0.0;
        }
    }

    /// Play after a count-in of `beats` beats of clicks (the sequencer waits;
    /// the position reads negative until it starts).
    pub fn play_count_in(&mut self, beats: f64) {
        if !self.playing {
            self.play();
            self.pre_total = beats.max(0.0);
            self.pre = self.pre_total;
        }
    }

    /// Click on every beat while playing.
    pub fn set_metronome(&mut self, on: bool) {
        self.metronome = on;
    }

    /// Pause keeps the position. Automated values return to the project's.
    pub fn pause(&mut self) {
        self.playing = false;
        self.pre = 0.0;
        self.auto_hold = false;
        self.release_all();
        self.restore_automation();
    }

    /// Stop rewinds to the start. Automated values return to the project's.
    pub fn stop(&mut self) {
        self.playing = false;
        self.pre = 0.0;
        self.auto_hold = false;
        self.position = 0.0;
        self.release_all();
        self.restore_automation();
    }

    /// Stop sequencing but keep automated values where they are, so release
    /// and effect tails keep the song's final mix (offline renders).
    pub fn end_song(&mut self) {
        self.playing = false;
        self.auto_hold = true;
        self.release_all();
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        if mode != self.mode {
            self.mode = mode;
            self.position = 0.0;
            self.release_all();
            self.restore_automation();
        }
    }

    /// Seconds from the start of the song to `beat`, following tempo automation.
    pub fn song_seconds(&self, beat: f64) -> f64 {
        self.tempo_map.seconds_at(beat)
    }

    /// Current tempo (automated while the song plays).
    pub fn bpm(&self) -> f64 {
        self.ctx.bpm as f64
    }

    pub fn mode(&self) -> &PlayMode {
        &self.mode
    }

    /// Position in beats (within the pattern in pattern mode); during a
    /// count-in, minus the beats left before it.
    pub fn position(&self) -> f64 {
        self.position - self.pre
    }

    pub fn seek(&mut self, beat: f64) {
        self.position = beat.max(0.0);
        self.pre = 0.0;
        self.release_all();
    }

    fn release_all(&mut self) {
        self.pending.clear();
        for ch in &mut self.channels {
            ch.events.push(NoteEvent {
                offset: 0,
                kind: NoteKind::AllOff,
            });
        }
    }

    /// Live note input (UI keyboard, piano roll preview, MIDI).
    pub fn note_on(&mut self, channel: &str, key: u8, velocity: f32) {
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel) {
            ch.events.push(NoteEvent {
                offset: 0,
                kind: NoteKind::On { key, velocity },
            });
        }
    }

    pub fn note_off(&mut self, channel: &str, key: u8) {
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel) {
            ch.events.push(NoteEvent {
                offset: 0,
                kind: NoteKind::Off { key },
            });
        }
    }

    /// Current loop length in beats (0 when there is nothing to play).
    pub fn loop_length(&self) -> f64 {
        match &self.mode {
            PlayMode::Pattern(id) => self
                .patterns
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.length)
                .unwrap_or(0.0),
            PlayMode::Song => self.song_length,
        }
    }

    /// Peak levels since the previous call.
    pub fn take_meters(&mut self) -> Meters {
        let m = Meters {
            inserts: self.inserts.iter().map(|i| i.peak).collect(),
            channels: self.channels.iter().map(|c| c.peak).collect(),
        };
        for i in &mut self.inserts {
            i.peak = [0.0; 2];
        }
        for c in &mut self.channels {
            c.peak = 0.0;
        }
        m
    }

    // ------------------------------------------------------------ rendering

    /// Render audio of any length into the two output buffers (overwriting).
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len().min(out_r.len());
        let step = if self.lanes.is_empty() {
            MAX_BLOCK
        } else {
            AUTOMATION_BLOCK
        };
        let mut done = 0;
        while done < n {
            let len = (n - done).min(step);
            self.process_block(&mut out_l[done..done + len], &mut out_r[done..done + len]);
            done += len;
        }
    }

    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        for ins in &mut self.inserts {
            ins.buf_l[..n].fill(0.0);
            ins.buf_r[..n].fill(0.0);
        }
        if self.automation_active() {
            self.apply_automation(n);
        } else if self.auto_applied && !self.auto_hold {
            self.restore_automation();
        }
        self.clicks.clear();
        if self.playing {
            let from = self.count_in(n);
            if from < n {
                self.schedule(from, n);
            }
        }

        // Instruments into their inserts.
        for ch in &mut self.channels {
            ch.events.sort_by_key(|e| e.offset);
            let (bl, br) = (&mut ch.buf_l[..n], &mut ch.buf_r[..n]);
            bl.fill(0.0);
            br.fill(0.0);
            ch.inst.process(&ch.events, bl, br);
            ch.events.clear();
            let (gl, il) = ch.gain_l.block(n);
            let (gr, ir) = ch.gain_r.block(n);
            let ins = &mut self.inserts[ch.mixer.index()];
            let mut peak = ch.peak;
            for i in 0..n {
                let l = bl[i] * (gl + il * i as f32);
                let r = br[i] * (gr + ir * i as f32);
                peak = peak.max(l.abs()).max(r.abs());
                ins.buf_l[i] += l;
                ins.buf_r[i] += r;
            }
            ch.peak = peak;
        }

        // Inserts into the master.
        self.master_l[..n].fill(0.0);
        self.master_r[..n].fill(0.0);
        let (master, rest) = self.inserts.split_first_mut().expect("master insert");
        for ins in rest.iter_mut() {
            run_insert(ins, n);
            if ins.audible {
                for i in 0..n {
                    self.master_l[i] += ins.buf_l[i];
                    self.master_r[i] += ins.buf_r[i];
                }
            }
        }
        let r = dsp::DcBlock::coef(self.ctx.sr);
        for i in 0..n {
            master.buf_l[i] = self.dc[0].process(master.buf_l[i] + self.master_l[i], r);
            master.buf_r[i] = self.dc[1].process(master.buf_r[i] + self.master_r[i], r);
        }
        run_insert(master, n);
        for i in 0..n {
            let (l, r) = (master.buf_l[i], master.buf_r[i]);
            out_l[i] = if l.is_finite() { l } else { 0.0 };
            out_r[i] = if r.is_finite() { r } else { 0.0 };
        }
        // The metronome goes straight to the output, past the mixer.
        if !self.clicks.is_empty() || self.click.sounding(self.ctx.sr) {
            let mut next = 0;
            for i in 0..n {
                while next < self.clicks.len() && self.clicks[next].0 <= i {
                    self.click = Click::new(self.clicks[next].1);
                    next += 1;
                }
                let v = self.click.next(self.ctx.sr);
                out_l[i] += v;
                out_r[i] += v;
            }
        }
    }

    /// Run the count-in for up to `n` frames, clicking its beats (the first
    /// of each bar high); returns the frames it took.
    fn count_in(&mut self, n: usize) -> usize {
        if self.pre <= 0.0 {
            return 0;
        }
        let bpf = self.ctx.bpm as f64 / 60.0 / self.ctx.sr as f64;
        let frames = ((self.pre / bpf).ceil() as usize).clamp(1, n);
        let p0 = self.pre_total - self.pre;
        let p1 = p0 + frames as f64 * bpf;
        let bar = self.project.transport.beats_per_bar.max(1) as i64;
        let mut k = p0.ceil();
        while k < p1 && k < self.pre_total - 1e-9 {
            let off = (((k - p0) / bpf) as usize).min(frames - 1);
            self.clicks.push((off, (k as i64) % bar == 0));
            k += 1.0;
        }
        self.pre -= frames as f64 * bpf;
        if self.pre < 1e-9 {
            self.pre = 0.0;
        }
        frames
    }

    /// Metronome clicks on the whole beats in `b0..b1` (frame `frame` is at `b0`).
    fn beat_clicks(&mut self, frame: usize, seg: usize, b0: f64, b1: f64, bpf: f64) {
        let mut k = b0.ceil();
        while k < b1 {
            let off = frame + (((k - b0) / bpf) as usize).min(seg - 1);
            let t = &self.project.transport;
            let down = match self.mode {
                PlayMode::Pattern(_) => (k as i64) % (t.beats_per_bar.max(1) as i64) == 0,
                PlayMode::Song => (t.bar_at(k).1 - k).abs() < 1e-6,
            };
            self.clicks.push((off, down));
            k += 1.0;
        }
    }

    /// Queue the sequenced note events for the next `n` frames and mix audio
    /// clips directly into their inserts.
    fn schedule(&mut self, from: usize, n: usize) {
        let bpf = self.ctx.bpm as f64 / 60.0 / self.ctx.sr as f64;
        let loop_len = self.loop_length();
        if loop_len <= 0.0 {
            let b0 = self.position;
            self.position += bpf * (n - from) as f64;
            self.clock += bpf * (n - from) as f64;
            if self.metronome {
                self.beat_clicks(from, n - from, b0, self.position, bpf);
            }
            return;
        }
        let mut frame = from;
        while frame < n {
            if self.position >= loop_len {
                self.position = 0.0;
            }
            let to_end = ((loop_len - self.position) / bpf).ceil().max(1.0) as usize;
            let seg = (n - frame).min(to_end);
            let b0 = self.position;
            let b1 = b0 + seg as f64 * bpf;
            let c0 = self.clock;
            let c1 = c0 + seg as f64 * bpf;
            if self.metronome {
                self.beat_clicks(frame, seg, b0, b1, bpf);
            }

            // Note-offs first, so a retriggered key releases before it restarts.
            let channels = &mut self.channels;
            self.pending.retain(|p| {
                if p.end < c1 {
                    let off = frame + (((p.end - c0).max(0.0) / bpf) as usize).min(seg - 1);
                    if let Some(ch) = channels.iter_mut().find(|c| c.handle == p.handle) {
                        ch.events.push(NoteEvent {
                            offset: off,
                            kind: NoteKind::Off { key: p.key },
                        });
                    }
                    false
                } else {
                    true
                }
            });

            let emit = |note: &CNote,
                        t: f64,
                        max_len: f64,
                        channels: &mut Vec<ChannelRt>,
                        pending: &mut Vec<Pending>| {
                let off = frame + (((t - b0) / bpf) as usize).min(seg - 1);
                let ch = &mut channels[note.channel];
                ch.events.push(NoteEvent {
                    offset: off,
                    kind: NoteKind::On {
                        key: note.key,
                        velocity: note.velocity,
                    },
                });
                pending.push(Pending {
                    handle: ch.handle,
                    key: note.key,
                    end: c0 + (t - b0) + note.length.min(max_len),
                });
            };

            let shift = self.swing * (1.0 / 12.0);
            match &self.mode {
                PlayMode::Pattern(id) => {
                    if let Some(p) = self.patterns.iter().find(|p| &p.id == id) {
                        for note in &p.notes {
                            let t = note.start_at(shift);
                            if t >= b0 && t < b1 {
                                emit(note, t, f64::MAX, &mut self.channels, &mut self.pending);
                            }
                        }
                    }
                }
                PlayMode::Song => {
                    for clip in &self.clips {
                        if clip.end <= b0 || clip.start >= b1 {
                            continue;
                        }
                        if let Some(pi) = clip.pattern {
                            let p = &self.patterns[pi];
                            let base = clip.start - clip.offset;
                            let lo = b0.max(clip.start);
                            let hi = b1.min(clip.end);
                            let k0 = ((lo - base) / p.length).floor() as i64;
                            let k1 = ((hi - base) / p.length).floor() as i64;
                            for k in k0.max(0)..=k1 {
                                let origin = base + k as f64 * p.length;
                                for note in &p.notes {
                                    let start = note.start_at(shift);
                                    let t = origin + start;
                                    if t >= lo && t < hi && start < p.length {
                                        emit(
                                            note,
                                            t,
                                            clip.end - t,
                                            &mut self.channels,
                                            &mut self.pending,
                                        );
                                    }
                                }
                            }
                        } else if let Some(sample) = &clip.sample {
                            let base_spb = 60.0 / self.project.transport.bpm.max(1.0);
                            let timing = ClipTiming {
                                frame,
                                seg,
                                b0,
                                bpf,
                                spb: 60.0 / self.ctx.bpm as f64,
                                base_spb,
                            };
                            mix_audio_clip(
                                clip,
                                sample,
                                &mut self.inserts[clip.mixer.index()],
                                &timing,
                                &self.tempo_map,
                            );
                        }
                    }
                }
            }
            self.position = b1;
            self.clock = c1;
            frame += seg;
        }
    }
}

/// The metronome's voice: a short decaying tone.
#[derive(Clone, Copy, Debug)]
struct Click {
    /// Frames since it started.
    age: usize,
    down: bool,
}

impl Default for Click {
    fn default() -> Click {
        Click {
            age: usize::MAX,
            down: false,
        }
    }
}

impl Click {
    const SECONDS: f32 = 0.06;

    fn new(down: bool) -> Click {
        Click { age: 0, down }
    }

    fn sounding(&self, sr: f32) -> bool {
        (self.age as f32) < Self::SECONDS * sr
    }

    fn next(&mut self, sr: f32) -> f32 {
        if !self.sounding(sr) {
            return 0.0;
        }
        let t = self.age as f32 / sr;
        self.age += 1;
        let (freq, amp) = if self.down {
            (1760.0, 0.45)
        } else {
            (1320.0, 0.3)
        };
        amp * (-t / 0.012).exp() * (std::f32::consts::TAU * freq * t).sin()
    }
}

/// Where a scheduling segment lies, for mixing audio clips.
struct ClipTiming {
    /// First frame and length of the segment in the block.
    frame: usize,
    seg: usize,
    /// Song position at the first frame, and beats per frame.
    b0: f64,
    bpf: f64,
    /// Seconds per beat now, and at the project tempo.
    spb: f64,
    base_spb: f64,
}

/// Mix an audio clip. Audio plays at its natural speed: the source position
/// is the time elapsed since the clip start (following tempo automation) plus
/// the clip offset (beats at the project tempo).
fn mix_audio_clip(
    clip: &CClip,
    sample: &SampleData,
    ins: &mut InsertRt,
    t: &ClipTiming,
    tempo: &TempoMap,
) {
    let fade = 0.004 / t.spb;
    let sr = sample.sample_rate as f64;
    let offset_s = clip.offset * t.base_spb;
    let start_s = tempo.seconds_at(clip.start);
    let ch_l = &sample.channels[0];
    let ch_r = sample.channels.get(1).unwrap_or(ch_l);
    for i in 0..t.seg {
        let beat = t.b0 + i as f64 * t.bpf;
        if beat < clip.start || beat >= clip.end {
            continue;
        }
        let secs = if tempo.is_automated() {
            tempo.seconds_at(beat) - start_s
        } else {
            (beat - clip.start) * t.base_spb
        };
        let pos = (secs + offset_s) * sr;
        if pos >= ch_l.len() as f64 {
            continue;
        }
        let edge = ((beat - clip.start) / fade)
            .min((clip.end - beat) / fade)
            .min(1.0) as f32;
        let g = clip.gain * edge;
        ins.buf_l[t.frame + i] += hermite(ch_l, pos) * g;
        ins.buf_r[t.frame + i] += hermite(ch_r, pos) * g;
    }
}

/// Route to an existing insert (the master when the index is out of range).
fn clamp_insert(ix: InsertIx, project: &Project) -> InsertIx {
    if ix.index() < project.mixer.inserts.len() {
        ix
    } else {
        InsertIx::MASTER
    }
}

fn run_insert(ins: &mut InsertRt, n: usize) {
    let (bl, br) = (&mut ins.buf_l[..n], &mut ins.buf_r[..n]);
    for f in ins.fx.iter_mut().filter(|f| f.enabled) {
        f.fx.process(bl, br);
    }
    let (gl, il) = ins.gain_l.block(n);
    let (gr, ir) = ins.gain_r.block(n);
    for i in 0..n {
        bl[i] *= gl + il * i as f32;
        br[i] *= gr + ir * i as f32;
        ins.peak[0] = ins.peak[0].max(bl[i].abs());
        ins.peak[1] = ins.peak[1].max(br[i].abs());
    }
}

fn make_effect(
    dev: &Device,
    ctx: &Ctx,
    host: Option<&dyn PluginHost>,
    errors: &mut Vec<String>,
    path: &str,
) -> Box<dyn Effect> {
    if dev.kind == "plugin" {
        return match host.map(|h| h.load(dev, false, ctx.sr, MAX_BLOCK)) {
            Some(Ok(p)) => Box::new(PluginEffect(p)),
            Some(Err(e)) => {
                errors.push(format!("{path}: {e}"));
                Box::new(Bypass)
            }
            None => {
                errors.push(format!("{path}: plugins are not available in this engine"));
                Box::new(Bypass)
            }
        };
    }
    effects::create(dev, ctx).unwrap_or_else(|| Box::new(Bypass))
}

struct Bypass;

impl Effect for Bypass {
    fn set_device(&mut self, _dev: &Device, _ctx: &Ctx) {}
    fn process(&mut self, _l: &mut [f32], _r: &mut [f32]) {}
}
