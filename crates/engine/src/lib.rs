//! Rosaclef audio engine.
//!
//! A portable, allocation-light DSP engine: sequencer, instruments, effects
//! and mixer. It performs no I/O and spawns no threads, so the same code runs
//! natively (driven by the device callback or an offline renderer) and in the
//! browser (compiled to WebAssembly and driven by an `AudioWorklet`).
//!
//! External plugins (CLAP, ...) are supported through the [`PluginHost`]
//! trait, which only native hosts implement.

pub mod dsp;
pub mod effects;
pub mod instruments;
pub mod render;
pub mod samples;

use dsp::{hermite, pan_gains, Ramp};
use effects::Effect;
use instruments::{Instrument, NoteEvent, NoteKind};
use rosaclef_core::{Device, InsertIx, Project};
use samples::{SampleBank, SampleData, SampleRef};
use std::sync::Arc;

/// Largest block processed in one go; larger requests are split.
pub const MAX_BLOCK: usize = 128;

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
    fn load(&self, dev: &Device, instrument: bool, sample_rate: f32, max_block: usize) -> Result<Box<dyn ExternalProcessor>, String>;
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
        self.proc_.process(events, &mut self.tmp_l[..n], &mut self.tmp_r[..n]);
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
        "plugin" => format!("plugin|{}|{}|{}", dev.option("format"), dev.option("path"), dev.option("id")),
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
    gain_l: Ramp,
    gain_r: Ramp,
    mixer: InsertIx,
    events: Vec<NoteEvent>,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    peak: f32,
}

struct FxRt {
    sig: String,
    enabled: bool,
    fx: Box<dyn Effect>,
}

struct InsertRt {
    fx: Vec<FxRt>,
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
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
            gain_l: Ramp::default(),
            gain_r: Ramp::default(),
            audible: true,
            peak: [0.0; 2],
        }
    }
}

#[derive(Clone, Copy)]
struct CNote {
    channel: usize,
    key: u8,
    start: f64,
    length: f64,
    velocity: f32,
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
    master_l: Vec<f32>,
    master_r: Vec<f32>,
    dc: [dsp::DcBlock; 2],
}

impl Engine {
    pub fn new(sample_rate: f32) -> Engine {
        let project = Project::empty("Untitled");
        let mut e = Engine {
            ctx: Ctx { sr: sample_rate, bpm: 120.0 },
            project: project.clone(),
            channels: vec![],
            inserts: vec![],
            patterns: vec![],
            clips: vec![],
            song_length: 0.0,
            swing: 0.0,
            next_handle: ChannelHandle(1),
            samples: SampleBank::default(),
            plugins: None,
            device_errors: vec![],
            playing: false,
            mode: PlayMode::Song,
            position: 0.0,
            clock: 0.0,
            pending: vec![],
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
            let (pl, pr) = pan_gains(ch.pan as f32);
            let v = if ch.mute { 0.0 } else { ch.volume as f32 };
            rt.gain_l.set(v * pl);
            rt.gain_r.set(v * pr);
            rt.mixer = clamp_insert(ch.mixer, &project);
            self.channels.push(rt);
        }

        // Inserts and their effect chains.
        let any_solo = project.mixer.inserts.iter().skip(1).any(|i| i.solo);
        self.inserts.resize_with(project.mixer.inserts.len(), InsertRt::new);
        for (idx, (ins, rt)) in project.mixer.inserts.iter().zip(self.inserts.iter_mut()).enumerate() {
            let mut old_fx: Vec<Option<FxRt>> = std::mem::take(&mut rt.fx).into_iter().map(Some).collect();
            for (j, dev) in ins.effects.iter().enumerate() {
                let sig = signature(dev);
                let reused = old_fx.get_mut(j).and_then(|slot| if slot.as_ref().map(|f| f.sig == sig).unwrap_or(false) { slot.take() } else { None });
                let mut f = match reused {
                    Some(f) => f,
                    None => {
                        let fx = make_effect(dev, &ctx, self.plugins.as_deref(), &mut self.device_errors, &format!("mixer.inserts[{idx}].effects[{j}]"));
                        FxRt { sig, enabled: true, fx }
                    }
                };
                f.fx.set_device(dev, &ctx);
                f.enabled = dev.enabled;
                rt.fx.push(f);
            }
            let v = if ins.mute { 0.0 } else { ins.volume as f32 };
            let pan = ins.pan as f32;
            rt.gain_l.set(v * (1.0 - pan).min(1.0));
            rt.gain_r.set(v * (1.0 + pan).min(1.0));
            rt.audible = idx == 0 || !any_solo || ins.solo;
        }

        // Patterns, with swing applied to off-beat 16ths.
        let swing_shift = self.swing * (1.0 / 12.0);
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
                        let start = if on_grid && (sixteenth.round() as i64) % 2 == 1 { n.start + swing_shift } else { n.start };
                        Some(CNote { channel, key: n.pitch.clamp(0, 127) as u8, start, length: n.length.max(1e-4), velocity: n.velocity as f32 })
                    })
                    .collect();
                notes.sort_by(|a, b| a.start.total_cmp(&b.start));
                CPattern { id: p.id.clone(), length: p.length.max(1e-3), notes }
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
                pattern: if c.pattern.is_empty() { None } else { self.patterns.iter().position(|p| p.id == c.pattern) },
                sample_path: c.sample.clone(),
                sample: if c.sample.is_empty() { None } else { self.samples.get(&c.sample) },
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
        self.project = project;
    }

    fn make_instrument(&mut self, dev: &Device, channel: &str) -> Box<dyn Instrument> {
        if dev.kind == "plugin" {
            let Some(host) = &self.plugins else {
                self.device_errors.push(format!("channel {channel}: plugins are not available in this engine"));
                return Box::new(instruments::Silent);
            };
            return match host.load(dev, true, self.ctx.sr, MAX_BLOCK) {
                Ok(p) => {
                    let mut inst = PluginInstrument { proc_: p, tmp_l: vec![0.0; MAX_BLOCK], tmp_r: vec![0.0; MAX_BLOCK] };
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
            if ch.instrument.kind == "sampler" {
                add(ch.instrument.option("sample"));
            }
        }
        for c in &self.project.playlist.clips {
            add(&c.sample);
        }
        out
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
            self.clock = 0.0;
        }
    }

    /// Pause keeps the position.
    pub fn pause(&mut self) {
        self.playing = false;
        self.release_all();
    }

    /// Stop rewinds to the start.
    pub fn stop(&mut self) {
        self.playing = false;
        self.position = 0.0;
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
        }
    }

    pub fn mode(&self) -> &PlayMode {
        &self.mode
    }

    /// Position in beats (within the pattern in pattern mode).
    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn seek(&mut self, beat: f64) {
        self.position = beat.max(0.0);
        self.release_all();
    }

    fn release_all(&mut self) {
        self.pending.clear();
        for ch in &mut self.channels {
            ch.events.push(NoteEvent { offset: 0, kind: NoteKind::AllOff });
        }
    }

    /// Live note input (UI keyboard, piano roll preview, MIDI).
    pub fn note_on(&mut self, channel: &str, key: u8, velocity: f32) {
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel) {
            ch.events.push(NoteEvent { offset: 0, kind: NoteKind::On { key, velocity } });
        }
    }

    pub fn note_off(&mut self, channel: &str, key: u8) {
        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel) {
            ch.events.push(NoteEvent { offset: 0, kind: NoteKind::Off { key } });
        }
    }

    /// Current loop length in beats (0 when there is nothing to play).
    pub fn loop_length(&self) -> f64 {
        match &self.mode {
            PlayMode::Pattern(id) => self.patterns.iter().find(|p| &p.id == id).map(|p| p.length).unwrap_or(0.0),
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
        let mut done = 0;
        while done < n {
            let len = (n - done).min(MAX_BLOCK);
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
        if self.playing {
            self.schedule(n);
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
    }

    /// Queue the sequenced note events for the next `n` frames and mix audio
    /// clips directly into their inserts.
    fn schedule(&mut self, n: usize) {
        let bpf = self.ctx.bpm as f64 / 60.0 / self.ctx.sr as f64;
        let loop_len = self.loop_length();
        if loop_len <= 0.0 {
            self.position += bpf * n as f64;
            self.clock += bpf * n as f64;
            return;
        }
        let mut frame = 0usize;
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

            // Note-offs first, so a retriggered key releases before it restarts.
            let channels = &mut self.channels;
            self.pending.retain(|p| {
                if p.end < c1 {
                    let off = frame + (((p.end - c0).max(0.0) / bpf) as usize).min(seg - 1);
                    if let Some(ch) = channels.iter_mut().find(|c| c.handle == p.handle) {
                        ch.events.push(NoteEvent { offset: off, kind: NoteKind::Off { key: p.key } });
                    }
                    false
                } else {
                    true
                }
            });

            let emit = |note: &CNote, t: f64, max_len: f64, channels: &mut Vec<ChannelRt>, pending: &mut Vec<Pending>| {
                let off = frame + (((t - b0) / bpf) as usize).min(seg - 1);
                let ch = &mut channels[note.channel];
                ch.events.push(NoteEvent { offset: off, kind: NoteKind::On { key: note.key, velocity: note.velocity } });
                pending.push(Pending { handle: ch.handle, key: note.key, end: c0 + (t - b0) + note.length.min(max_len) });
            };

            match &self.mode {
                PlayMode::Pattern(id) => {
                    if let Some(p) = self.patterns.iter().find(|p| &p.id == id) {
                        for note in p.notes.iter().filter(|nn| nn.start >= b0 && nn.start < b1) {
                            emit(note, note.start, f64::MAX, &mut self.channels, &mut self.pending);
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
                                    let t = origin + note.start;
                                    if t >= lo && t < hi && note.start < p.length {
                                        emit(note, t, clip.end - t, &mut self.channels, &mut self.pending);
                                    }
                                }
                            }
                        } else if let Some(sample) = &clip.sample {
                            mix_audio_clip(clip, sample, &mut self.inserts[clip.mixer.index()], frame, seg, b0, bpf, self.ctx);
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

#[allow(clippy::too_many_arguments)]
fn mix_audio_clip(clip: &CClip, sample: &SampleData, ins: &mut InsertRt, frame: usize, seg: usize, b0: f64, bpf: f64, ctx: Ctx) {
    let spb = 60.0 / ctx.bpm as f64;
    let fade = 0.004 / spb;
    let src_per_beat = spb * sample.sample_rate as f64;
    let ch_l = &sample.channels[0];
    let ch_r = sample.channels.get(1).unwrap_or(ch_l);
    for i in 0..seg {
        let beat = b0 + i as f64 * bpf;
        if beat < clip.start || beat >= clip.end {
            continue;
        }
        let pos = (beat - clip.start + clip.offset) * src_per_beat;
        if pos >= ch_l.len() as f64 {
            continue;
        }
        let edge = ((beat - clip.start) / fade).min((clip.end - beat) / fade).min(1.0) as f32;
        let g = clip.gain * edge;
        ins.buf_l[frame + i] += hermite(ch_l, pos) * g;
        ins.buf_r[frame + i] += hermite(ch_r, pos) * g;
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

fn make_effect(dev: &Device, ctx: &Ctx, host: Option<&dyn PluginHost>, errors: &mut Vec<String>, path: &str) -> Box<dyn Effect> {
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
