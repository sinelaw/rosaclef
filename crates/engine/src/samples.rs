//! Decoded audio used by samplers, audio clips and soundfont instruments.
//!
//! The engine never touches files: hosts decode audio (natively with
//! symphonia, in the browser with `decodeAudioData`) and hand the engine
//! de-interleaved `f32` channels, and they load soundfont presets
//! ([`crate::soundfont`]) off the audio thread.

use crate::soundfont::LoadedPreset;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct SampleData {
    pub sample_rate: f32,
    /// One or two channels of equal length.
    pub channels: Vec<Vec<f32>>,
}

impl SampleData {
    pub fn len(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn duration(&self) -> f64 {
        self.len() as f64 / self.sample_rate as f64
    }
}

pub type SampleRef = Arc<SampleData>;

/// A soundfont preset: which soundfont, bank and program.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PresetKey {
    /// Soundfont name; `gm` is the built-in General MIDI soundfont.
    pub font: String,
    pub bank: u16,
    pub program: u8,
}

impl PresetKey {
    /// A preset of the built-in General MIDI soundfont.
    pub fn gm(bank: u16, program: u8) -> PresetKey {
        PresetKey {
            font: "gm".into(),
            bank,
            program,
        }
    }
}

#[derive(Default, Clone)]
pub struct SampleBank {
    map: HashMap<String, SampleRef>,
    presets: HashMap<PresetKey, Arc<LoadedPreset>>,
}

impl SampleBank {
    pub fn get(&self, path: &str) -> Option<SampleRef> {
        self.map.get(path).cloned()
    }
    pub fn insert(&mut self, path: &str, data: SampleData) {
        self.map.insert(path.to_string(), Arc::new(data));
    }
    pub fn insert_shared(&mut self, path: &str, data: SampleRef) {
        self.map.insert(path.to_string(), data);
    }
    pub fn remove(&mut self, path: &str) {
        self.map.remove(path);
    }
    pub fn contains(&self, path: &str) -> bool {
        self.map.contains_key(path)
    }
    pub fn preset(&self, key: &PresetKey) -> Option<Arc<LoadedPreset>> {
        self.presets.get(key).cloned()
    }
    pub fn insert_preset(&mut self, key: PresetKey, preset: Arc<LoadedPreset>) {
        self.presets.insert(key, preset);
    }
    pub fn has_preset(&self, key: &PresetKey) -> bool {
        self.presets.contains_key(key)
    }
}
