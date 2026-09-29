//! Decoded audio used by samplers and audio clips.
//!
//! The engine never touches files: hosts decode audio (natively with
//! symphonia, in the browser with `decodeAudioData`) and hand the engine
//! de-interleaved `f32` channels.

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

#[derive(Default, Clone)]
pub struct SampleBank {
    map: HashMap<String, SampleRef>,
}

impl SampleBank {
    pub fn get(&self, path: &str) -> Option<SampleRef> {
        self.map.get(path).cloned()
    }
    pub fn insert(&mut self, path: &str, data: SampleData) {
        self.map.insert(path.to_string(), Arc::new(data));
    }
    pub fn remove(&mut self, path: &str) {
        self.map.remove(path);
    }
    pub fn contains(&self, path: &str) -> bool {
        self.map.contains_key(path)
    }
}
