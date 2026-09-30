//! Importers that turn other music formats into Rosaclef projects.
//!
//! - [`lmms`]: LMMS projects (`.mmp` XML and compressed `.mmpz`).
//! - [`midi`]: Standard MIDI Files (format 0 and 1).
//!
//! Both produce an [`Imported`]: a project that passes
//! [`rosaclef_core::validate`], the audio files to copy into the new project
//! folder, and human-readable warnings for everything that was approximated
//! or skipped. Neither importer touches the file system except to look for
//! referenced samples; the caller creates the folder and copies the files.

pub mod lmms;
pub mod midi;

use anyhow::{bail, Result};
use rosaclef_core::catalog::{self, Category};
use rosaclef_core::validate::{self, Severity};
use rosaclef_core::{Device, InsertIx, Project, TrackIx};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

/// An audio file to copy into the imported project.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleCopy {
    /// Source file on disk.
    pub from: PathBuf,
    /// Project-relative destination (`samples/kick.wav`).
    pub to: String,
}

/// The result of an import.
#[derive(Clone, Debug)]
pub struct Imported {
    pub project: Project,
    pub samples: Vec<SampleCopy>,
    pub warnings: Vec<String>,
}

/// Colors used for imported channels and patterns (the studio palette).
pub const PALETTE: [&str; 10] = ["#d4af37", "#c97b84", "#e8d5b0", "#8e3b46", "#3f8f7a", "#4a6fa5", "#8a6bb0", "#b08d57", "#d98c5f", "#6fa3a0"];

pub(crate) fn color(i: usize) -> String {
    PALETTE[i % PALETTE.len()].to_string()
}

/// Rosaclef's limit on mixer inserts (see `validate`).
pub const MAX_INSERTS: usize = 128;

/// Allocates ids matching `[A-Za-z0-9_.-]{1,64}`, unique within one namespace.
#[derive(Default)]
pub(crate) struct Ids {
    taken: HashSet<String>,
}

impl Ids {
    pub fn with(existing: impl IntoIterator<Item = String>) -> Ids {
        Ids { taken: existing.into_iter().collect() }
    }

    /// Keep `id` if it is free, otherwise derive a fresh one from it.
    pub fn keep(&mut self, id: &str) -> String {
        if self.taken.insert(id.to_string()) {
            id.to_string()
        } else {
            self.make(id)
        }
    }

    /// A fresh id derived from `name` (lower-case slug), suffixed when taken.
    pub fn make(&mut self, name: &str) -> String {
        let mut base: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        if base.is_empty() {
            base = "item".into();
        }
        base.truncate(56);
        let mut id = base.clone();
        let mut n = 2;
        while self.taken.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.taken.insert(id.clone());
        id
    }
}

/// Collects warnings, merging repeats of the same message.
#[derive(Default)]
pub(crate) struct Warnings {
    list: Vec<String>,
    counts: HashMap<String, usize>,
}

impl Warnings {
    pub fn add(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        let n = self.counts.entry(msg.clone()).or_insert(0);
        *n += 1;
        if *n == 1 {
            self.list.push(msg);
        }
    }

    pub fn finish(self) -> Vec<String> {
        self.list
            .into_iter()
            .map(|m| match self.counts.get(&m) {
                Some(&n) if n > 1 => format!("{m} (×{n})"),
                _ => m,
            })
            .collect()
    }
}

/// Set a numeric parameter, clamped into the catalog range (rounded for
/// integer parameters). Unknown keys are ignored.
pub(crate) fn set_param(dev: &mut Device, key: &str, value: f64) {
    let Some(spec) = catalog::device(&dev.kind).and_then(|d| d.param(key)) else { return };
    let mut v = if value.is_finite() { value } else { spec.default };
    v = v.clamp(spec.min, spec.max);
    if spec.integer {
        v = v.round();
    }
    dev.params.insert(key.to_string(), v);
}

/// Set a textual option if it is legal for the device.
pub(crate) fn set_option(dev: &mut Device, key: &str, value: &str) {
    let Some(spec) = catalog::device(&dev.kind).and_then(|d| d.option(key)) else { return };
    if spec.choices.is_empty() || spec.choices.contains(&value) {
        dev.options.insert(key.to_string(), value.to_string());
    }
}

/// Whether the catalog has an instrument of this type (engines on other
/// branches may not exist yet).
pub(crate) fn has_instrument(kind: &str) -> bool {
    catalog::device_in(kind, Category::Instrument).is_some()
}

/// Clamp a value into a range, reporting whether it had to change.
pub(crate) fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        lo
    }
}

/// Round beats to a sane precision (ticks are integers, so this is exact for
/// common resolutions and keeps the JSON short).
pub(crate) fn beats(ticks: f64, per_beat: f64) -> f64 {
    ((ticks / per_beat) * 1_000_000.0).round() / 1_000_000.0
}

/// Pad the playlist with empty tracks so the arrangement has room to grow.
pub(crate) fn pad_tracks(p: &mut Project, min: usize) {
    while p.playlist.tracks.len() < min {
        let n = p.playlist.tracks.len() + 1;
        p.playlist.tracks.push(rosaclef_core::Track { name: format!("Track {n}"), mute: false });
    }
}

/// Final check: the importers must always produce a valid project.
pub(crate) fn ensure_valid(p: &Project) -> Result<()> {
    let errors: Vec<String> = validate::validate(p).into_iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect();
    if !errors.is_empty() {
        bail!("internal error: the imported project is invalid:\n{}", errors.join("\n"));
    }
    Ok(())
}

/// Merge `add` into `base` (e.g. a MIDI file imported into the open song):
/// channels, patterns, playlist tracks and mixer inserts are appended, ids
/// are renamed on collision and indices shifted. Returns warnings.
pub fn merge_into(base: &mut Project, add: Project) -> Vec<String> {
    let mut warnings = Warnings::default();
    if (base.transport.bpm - add.transport.bpm).abs() > 0.01 {
        warnings.add(format!(
            "the imported tempo ({} BPM) differs from the project tempo ({} BPM); the notes keep their positions in beats",
            add.transport.bpm, base.transport.bpm
        ));
    }
    let mut channel_ids = Ids::with(base.channels.iter().map(|c| c.id.clone()));
    let mut pattern_ids = Ids::with(base.patterns.iter().map(|p| p.id.clone()));
    let mut channel_map: HashMap<String, String> = HashMap::new();
    let mut pattern_map: HashMap<String, String> = HashMap::new();

    // Inserts: the imported master is dropped; the others are appended.
    let mut insert_map: HashMap<u32, InsertIx> = HashMap::new();
    insert_map.insert(0, InsertIx::MASTER);
    for (i, ins) in add.mixer.inserts.into_iter().enumerate().skip(1) {
        if base.mixer.inserts.len() >= MAX_INSERTS {
            warnings.add(format!("no free mixer insert for \"{}\"; routed to the master", ins.name));
            insert_map.insert(i as u32, InsertIx::MASTER);
            continue;
        }
        insert_map.insert(i as u32, InsertIx(base.mixer.inserts.len() as u32));
        base.mixer.inserts.push(ins);
    }
    let remap_insert = |ix: InsertIx| insert_map.get(&ix.0).copied().unwrap_or(InsertIx::MASTER);

    for mut ch in add.channels {
        let id = channel_ids.keep(&ch.id);
        channel_map.insert(ch.id.clone(), id.clone());
        ch.id = id;
        ch.mixer = remap_insert(ch.mixer);
        base.channels.push(ch);
    }
    for mut pat in add.patterns {
        let id = pattern_ids.keep(&pat.id);
        pattern_map.insert(pat.id.clone(), id.clone());
        pat.id = id;
        for n in &mut pat.notes {
            if let Some(c) = channel_map.get(&n.channel) {
                n.channel = c.clone();
            }
        }
        base.patterns.push(pat);
    }
    let track_base = base.playlist.tracks.len() as u32;
    // Imported projects are padded with spare tracks; don't append those.
    let used: BTreeSet<u32> = add.playlist.clips.iter().map(|c| c.track.0).collect();
    let mut track_map: HashMap<u32, u32> = HashMap::new();
    for (i, t) in add.playlist.tracks.into_iter().enumerate() {
        if used.contains(&(i as u32)) {
            track_map.insert(i as u32, track_base + track_map.len() as u32);
            base.playlist.tracks.push(t);
        }
    }
    for mut c in add.playlist.clips {
        if !c.pattern.is_empty() {
            if let Some(p) = pattern_map.get(&c.pattern) {
                c.pattern = p.clone();
            }
        } else {
            c.mixer = remap_insert(c.mixer);
        }
        c.track = TrackIx(track_map.get(&c.track.0).copied().unwrap_or(track_base));
        base.playlist.clips.push(c);
    }
    warnings.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_valid_and_unique() {
        let mut ids = Ids::default();
        assert_eq!(ids.make("Bass Line!"), "bass-line");
        assert_eq!(ids.make("bass line"), "bass-line-2");
        assert_eq!(ids.make("§§"), "item");
        let long = ids.make(&"x".repeat(200));
        assert!(long.len() <= 64);
    }

    #[test]
    fn clamps_into_catalog() {
        let mut d = Device::new("synth");
        set_param(&mut d, "cutoff", 1e9);
        set_param(&mut d, "unison", 3.4);
        set_param(&mut d, "nope", 1.0);
        set_option(&mut d, "wave1", "sine");
        set_option(&mut d, "wave2", "banana");
        assert_eq!(d.params["cutoff"], 20000.0);
        assert_eq!(d.params["unison"], 3.0);
        assert!(!d.params.contains_key("nope"));
        assert_eq!(d.options.get("wave1").map(|s| s.as_str()), Some("sine"));
        assert!(!d.options.contains_key("wave2"));
    }
}
