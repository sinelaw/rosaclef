//! Offline rendering of a project folder's song.

use crate::folder::Folder;
use crate::fonts::Fonts;
use anyhow::anyhow;
use rosaclef_core::Project;
use rosaclef_engine::render::{self, Audio, RenderScope};
use rosaclef_engine::samples::PresetKey;
use rosaclef_engine::Engine;

/// Render a project offline, loading its samples from the folder and its
/// soundfont presets from `fonts`. `setup` prepares the engine first (the
/// native server gives it a plugin host). Returns the audio and warnings
/// (devices, samples or presets that failed to load).
pub fn render_project(
    folder: &Folder,
    project: Project,
    scope: &RenderScope,
    sample_rate: f32,
    fonts: &Fonts,
    setup: impl FnOnce(&mut Engine),
) -> (Audio, Vec<String>) {
    let mut engine = Engine::new(sample_rate);
    setup(&mut engine);
    engine.set_project(project);
    let mut warnings = engine.device_errors.clone();
    for path in engine.required_samples() {
        match folder
            .resolve(&path)
            .ok_or_else(|| anyhow!("invalid path"))
            .and_then(|p| crate::decode::decode_file(folder.fs.as_ref(), &p))
        {
            Ok(data) => engine.set_sample(&path, data),
            Err(e) => warnings.push(format!("sample {path}: {e}")),
        }
    }
    warnings.extend(fonts.provide(&mut engine));
    (render::render(&mut engine, scope), warnings)
}

/// Project-relative paths of the samples a render of `project` reads.
pub fn required_samples(project: &Project) -> Vec<String> {
    let mut engine = Engine::new(48000.0);
    engine.set_project(project.clone());
    engine.required_samples()
}

/// Soundfont presets a render of `project` plays.
pub fn required_presets(project: &Project) -> Vec<PresetKey> {
    let mut engine = Engine::new(48000.0);
    engine.set_project(project.clone());
    engine.required_presets()
}

/// Peak and RMS of rendered audio, in dBFS.
pub fn levels_db(audio: &Audio) -> (f32, f32) {
    (
        20.0 * audio.peak().max(1e-9).log10(),
        20.0 * audio.rms().max(1e-9).log10(),
    )
}
