//! Automation playback: lanes are compiled against the engine's runtime
//! objects when the project changes, then evaluated at the song position
//! before every block (at most [`crate::AUTOMATION_BLOCK`] frames) while the
//! song plays. Nothing here allocates on the audio path: parameter keys are
//! inserted into the devices' working copies at compile time.

use crate::{Engine, PlayMode};
use rosaclef_core::automation::{value_at, AutomationTarget};
use rosaclef_core::{AutomationPoint, Device};

/// What a compiled lane writes to.
enum Slot {
    Tempo,
    Swing,
    /// Index into `Engine::channels` (same order as `project.channels`).
    ChannelVolume(usize),
    ChannelPan(usize),
    ChannelParam(usize, String),
    InsertVolume(usize),
    InsertPan(usize),
    /// Insert, effect index, parameter key.
    EffectParam(usize, usize, String),
}

pub(crate) struct CLane {
    slot: Slot,
    points: Vec<AutomationPoint>,
    integer: bool,
    min: f64,
    max: f64,
    /// Changes smaller than this are not applied (devices are reconfigured
    /// only when a value really moves).
    eps: f64,
    /// Last applied value (NaN: not applied).
    last: f64,
}

/// Tempo changes smaller than this don't reconfigure tempo-synced devices.
const TEMPO_RESYNC_BPM: f32 = 0.05;

/// The project's value of a device parameter. `None` for a plugin parameter
/// the project does not set (its default is only known to the plugin).
fn base_param(dev: &Device, key: &str) -> Option<f64> {
    if dev.kind == "plugin" {
        return dev.params.get(key).copied();
    }
    Some(dev.param(key))
}

/// Make sure a working copy has `key`, so automation can update it in place.
fn ensure_key(dev: &mut Device, key: &str, fallback: f64) {
    if !dev.params.contains_key(key) {
        let v = base_param(dev, key).unwrap_or(fallback);
        dev.params.insert(key.to_string(), v);
    }
}

impl Engine {
    /// Lanes run in song mode while playing (or hold at the end of a render).
    pub(crate) fn automation_active(&self) -> bool {
        self.playing && self.mode == PlayMode::Song && !self.lanes.is_empty()
    }

    /// Resolve the project's lanes to runtime slots. Invalid, muted or empty
    /// lanes are skipped.
    pub(crate) fn compile_automation(&mut self) {
        self.lanes.clear();
        let project = &self.project;
        for lane in &project.automation {
            if lane.mute || lane.points.is_empty() {
                continue;
            }
            let Ok(target) = lane.target.parse::<AutomationTarget>() else { continue };
            let Ok(info) = target.resolve(project) else { continue };
            let mut points = lane.points.clone();
            points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            let first = points[0].value;
            let slot = match &target {
                AutomationTarget::Tempo => Slot::Tempo,
                AutomationTarget::Swing => Slot::Swing,
                AutomationTarget::ChannelVolume(id) | AutomationTarget::ChannelPan(id) | AutomationTarget::ChannelParam(id, _) => {
                    let Some(i) = project.channels.iter().position(|c| &c.id == id) else { continue };
                    match &target {
                        AutomationTarget::ChannelVolume(_) => Slot::ChannelVolume(i),
                        AutomationTarget::ChannelPan(_) => Slot::ChannelPan(i),
                        AutomationTarget::ChannelParam(_, key) => {
                            ensure_key(&mut self.channels[i].dev, key, first);
                            Slot::ChannelParam(i, key.clone())
                        }
                        _ => unreachable!(),
                    }
                }
                AutomationTarget::InsertVolume(ix) => Slot::InsertVolume(ix.index()),
                AutomationTarget::InsertPan(ix) => Slot::InsertPan(ix.index()),
                AutomationTarget::EffectParam(ix, k, key) => {
                    let Some(fx) = self.inserts.get_mut(ix.index()).and_then(|ins| ins.fx.get_mut(*k)) else { continue };
                    ensure_key(&mut fx.dev, key, first);
                    Slot::EffectParam(ix.index(), *k, key.clone())
                }
            };
            let (min, max) = (info.min, info.max);
            let eps = if (max - min).is_finite() { (max - min) * 1e-4 } else { 1e-6 };
            let eps = if matches!(slot, Slot::Tempo | Slot::Swing | Slot::ChannelVolume(_) | Slot::ChannelPan(_) | Slot::InsertVolume(_) | Slot::InsertPan(_)) {
                // Cheap targets follow the lane exactly.
                0.0
            } else {
                eps
            };
            self.lanes.push(CLane { slot, points, integer: info.integer, min, max, eps, last: f64::NAN });
        }
    }

    /// Evaluate every lane at the song position and apply the values. `frames`
    /// is the length of the coming block: the tempo is taken at its middle,
    /// so the beat clock follows a tempo ramp closely.
    pub(crate) fn apply_automation(&mut self, frames: usize) {
        let beat = self.position;
        let mid = beat + self.ctx.bpm as f64 / 60.0 / self.ctx.sr as f64 * frames as f64 * 0.5;
        let Engine { lanes, channels, inserts, ctx, swing, .. } = self;
        for lane in lanes.iter_mut() {
            let at = if matches!(lane.slot, Slot::Tempo) { mid } else { beat };
            let Some(v) = value_at(&lane.points, at) else { continue };
            let mut v = v.clamp(lane.min, lane.max);
            if lane.integer {
                v = v.round();
            }
            if (v - lane.last).abs() <= lane.eps {
                continue;
            }
            lane.last = v;
            match &lane.slot {
                Slot::Tempo => ctx.bpm = v as f32,
                Slot::Swing => *swing = v,
                Slot::ChannelVolume(i) => {
                    let ch = &mut channels[*i];
                    ch.volume = v as f32;
                    ch.update_gains();
                }
                Slot::ChannelPan(i) => {
                    let ch = &mut channels[*i];
                    ch.pan = v as f32;
                    ch.update_gains();
                }
                Slot::ChannelParam(i, key) => {
                    let ch = &mut channels[*i];
                    if let Some(p) = ch.dev.params.get_mut(key.as_str()) {
                        *p = v;
                    }
                    ch.inst.set_device(&ch.dev, ctx);
                }
                Slot::InsertVolume(i) => {
                    let ins = &mut inserts[*i];
                    ins.volume = v as f32;
                    ins.update_gains();
                }
                Slot::InsertPan(i) => {
                    let ins = &mut inserts[*i];
                    ins.pan = v as f32;
                    ins.update_gains();
                }
                Slot::EffectParam(i, k, key) => {
                    let fx = &mut inserts[*i].fx[*k];
                    if let Some(p) = fx.dev.params.get_mut(key.as_str()) {
                        *p = v;
                    }
                    fx.fx.set_device(&fx.dev, ctx);
                }
            }
        }
        self.auto_applied = true;
        self.resync_tempo(false);
    }

    /// Put every automated target back to its project value.
    pub(crate) fn restore_automation(&mut self) {
        if !self.auto_applied {
            return;
        }
        self.auto_applied = false;
        let Engine { lanes, channels, inserts, ctx, swing, project, .. } = self;
        ctx.bpm = project.transport.bpm as f32;
        *swing = project.transport.swing;
        for lane in lanes.iter_mut() {
            if lane.last.is_nan() {
                continue;
            }
            lane.last = f64::NAN;
            match &lane.slot {
                Slot::Tempo | Slot::Swing => {}
                Slot::ChannelVolume(i) | Slot::ChannelPan(i) => {
                    let (ch, base) = (&mut channels[*i], &project.channels[*i]);
                    ch.volume = base.volume as f32;
                    ch.pan = base.pan as f32;
                    ch.update_gains();
                }
                Slot::ChannelParam(i, key) => {
                    let ch = &mut channels[*i];
                    if let (Some(v), Some(p)) = (base_param(&project.channels[*i].instrument, key), ch.dev.params.get_mut(key.as_str())) {
                        *p = v;
                    }
                    ch.inst.set_device(&ch.dev, ctx);
                }
                Slot::InsertVolume(i) | Slot::InsertPan(i) => {
                    let (ins, base) = (&mut inserts[*i], &project.mixer.inserts[*i]);
                    ins.volume = base.volume as f32;
                    ins.pan = base.pan as f32;
                    ins.update_gains();
                }
                Slot::EffectParam(i, k, key) => {
                    let fx = &mut inserts[*i].fx[*k];
                    if let (Some(v), Some(p)) = (base_param(&project.mixer.inserts[*i].effects[*k], key), fx.dev.params.get_mut(key.as_str())) {
                        *p = v;
                    }
                    fx.fx.set_device(&fx.dev, ctx);
                }
            }
        }
        self.resync_tempo(true);
    }

    /// Reconfigure tempo-synced devices (delays, generative instruments)
    /// when the tempo moved.
    fn resync_tempo(&mut self, force: bool) {
        let moved = (self.ctx.bpm - self.synced_bpm).abs();
        if moved == 0.0 || (!force && moved <= TEMPO_RESYNC_BPM) {
            return;
        }
        self.synced_bpm = self.ctx.bpm;
        let ctx = self.ctx;
        for ch in self.channels.iter_mut().filter(|c| c.tempo_synced) {
            ch.inst.set_device(&ch.dev, &ctx);
        }
        for f in self.inserts.iter_mut().flat_map(|i| i.fx.iter_mut()).filter(|f| f.tempo_synced) {
            f.fx.set_device(&f.dev, &ctx);
        }
    }
}
