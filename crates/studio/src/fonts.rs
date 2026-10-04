//! Soundfonts for `soundfont` instruments: reading the split soundfont files
//! (`web/soundfonts/<font>/index.sf2` and `smpl-NNN.bin`, see
//! `tools/split_soundfont.py`) and preparing presets for the engine.
//!
//! Loading a preset parses the index (once per soundfont), reads the pieces
//! holding the preset's samples and decodes them. Decoded samples are kept,
//! so presets sharing samples share memory and a second load is instant.

use anyhow::{anyhow, Result};
use rosaclef_engine::samples::PresetKey;
use rosaclef_engine::soundfont::{FontSample, LoadedPreset, SoundFont};
use rosaclef_engine::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Where soundfont files come from.
pub trait FontFiles: Send + Sync {
    /// Read a file of the soundfonts directory, e.g. `gm/index.sf2`.
    fn read(&self, name: &str) -> Result<Vec<u8>>;
}

/// Soundfont files in a directory (natively: `web/soundfonts`).
pub struct DirFonts(pub PathBuf);

impl FontFiles for DirFonts {
    fn read(&self, name: &str) -> Result<Vec<u8>> {
        if name.split('/').any(|p| p.is_empty() || p == "..") {
            return Err(anyhow!("invalid soundfont file {name:?}"));
        }
        let path = self.0.join(name);
        std::fs::read(&path).map_err(|e| anyhow!("{}: {e}", path.display()))
    }
}

#[derive(Default)]
struct Cache {
    fonts: HashMap<String, Arc<SoundFont>>,
    samples: HashMap<(String, u32), Arc<FontSample>>,
    presets: HashMap<PresetKey, Arc<LoadedPreset>>,
}

/// Loads soundfont presets, caching what it loaded.
pub struct Fonts {
    files: Arc<dyn FontFiles>,
    cache: Mutex<Cache>,
}

/// File name of a piece of a split soundfont.
pub fn piece_name(font: &str, k: usize) -> String {
    format!("{font}/smpl-{k:03}.bin")
}

impl Fonts {
    pub fn new(files: Arc<dyn FontFiles>) -> Fonts {
        Fonts {
            files,
            cache: Mutex::new(Cache::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The parsed index of a soundfont.
    pub fn font(&self, font: &str) -> Result<Arc<SoundFont>> {
        if let Some(f) = self.lock().fonts.get(font) {
            return Ok(f.clone());
        }
        let bytes = self.files.read(&format!("{font}/index.sf2"))?;
        let f = Arc::new(SoundFont::parse(&bytes).map_err(|e| anyhow!("soundfont {font}: {e}"))?);
        self.lock().fonts.insert(font.to_string(), f.clone());
        Ok(f)
    }

    /// Load (or return the cached) preset.
    pub fn preset(&self, key: &PresetKey) -> Result<Arc<LoadedPreset>> {
        if let Some(p) = self.lock().presets.get(key) {
            return Ok(p.clone());
        }
        let sf = self.font(&key.font)?;
        let idx = sf
            .find(key.bank, key.program as u16)
            .ok_or_else(|| anyhow!("soundfont {} has no presets", key.font))?;
        // Read every piece first, so a host that loads files on demand
        // learns all of them at once.
        let mut pieces: HashMap<usize, Vec<u8>> = HashMap::new();
        let mut missing = None;
        if sf.is_split() {
            let cached: Vec<u32> = {
                let c = self.lock();
                sf.regions(idx)
                    .iter()
                    .map(|r| r.sample)
                    .filter(|s| c.samples.contains_key(&(key.font.clone(), *s)))
                    .collect()
            };
            for k in sf.pieces_except(idx, &cached) {
                match self.files.read(&piece_name(&key.font, k)) {
                    Ok(b) => {
                        pieces.insert(k, b);
                    }
                    Err(e) => missing = missing.or(Some(e)),
                }
            }
        }
        if let Some(e) = missing {
            return Err(e);
        }
        let mut local: HashMap<u32, Arc<FontSample>> = {
            let c = self.lock();
            c.samples
                .iter()
                .filter(|((f, _), _)| *f == key.font)
                .map(|((_, i), s)| (*i, s.clone()))
                .collect()
        };
        let preset = sf.load(idx, &pieces, &mut local).map_err(|e| {
            anyhow!(
                "soundfont {} preset {}: {e}",
                key.font,
                sf.presets[idx].name
            )
        })?;
        let preset = Arc::new(preset);
        let mut c = self.lock();
        for (i, s) in local {
            c.samples.insert((key.font.clone(), i), s);
        }
        c.presets.insert(key.clone(), preset.clone());
        Ok(preset)
    }

    /// Give the engine every preset its project plays; returns the problems.
    pub fn provide(&self, engine: &mut Engine) -> Vec<String> {
        self.provide_with(engine, |_, _| {})
    }

    /// [`Fonts::provide`], calling `progress(done, total)` as presets load.
    pub fn provide_with(
        &self,
        engine: &mut Engine,
        mut progress: impl FnMut(usize, usize),
    ) -> Vec<String> {
        let mut warnings = vec![];
        let keys: Vec<PresetKey> = engine
            .required_presets()
            .into_iter()
            .filter(|k| !engine.has_preset(k))
            .collect();
        let total = keys.len();
        for (i, key) in keys.into_iter().enumerate() {
            progress(i, total);
            match self.preset(&key) {
                Ok(p) => engine.set_preset(key, p),
                Err(e) => warnings.push(format!("{e:#}")),
            }
        }
        warnings
    }

    /// Load presets ahead of use; fails with the first error.
    pub fn ensure(&self, keys: &[PresetKey]) -> Result<()> {
        let mut first = None;
        for k in keys {
            if let Err(e) = self.preset(k) {
                first = first.or(Some(e));
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// Forget loaded presets and decoded samples (the indexes stay).
    pub fn clear(&self) {
        let mut c = self.lock();
        c.presets.clear();
        c.samples.clear();
    }
}
