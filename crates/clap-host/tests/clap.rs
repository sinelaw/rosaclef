//! End-to-end tests against the `rosaclef-clap-testplug` plugin bundle.

use rosaclef_clap::{params, scan, ClapHost};
use rosaclef_core::{Channel, Clip, Device, Note, Project};
use rosaclef_engine::instruments::{NoteEvent, NoteKind};
use rosaclef_engine::render::{render, RenderScope};
use rosaclef_engine::{Engine, PluginHost};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

const SINE: &str = "dev.rosaclef.test-sine";
const GAIN: &str = "dev.rosaclef.test-gain";

/// Build the test plugin once and copy it to `<tmp>/rosaclef-test.clap`.
fn test_plugin() -> &'static Path {
    static PLUGIN: OnceLock<PathBuf> = OnceLock::new();
    PLUGIN.get_or_init(|| {
        let out = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "rosaclef-clap-testplug",
                "--message-format=json",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("run cargo build");
        assert!(
            out.status.success(),
            "building the test plugin failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Find the cdylib among the build artifacts.
        let mut lib = None;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if msg["reason"] != "compiler-artifact"
                || msg["target"]["name"] != "rosaclef_clap_testplug"
            {
                continue;
            }
            for f in msg["filenames"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str())
            {
                if [".so", ".dylib", ".dll"].iter().any(|ext| f.ends_with(ext)) {
                    lib = Some(PathBuf::from(f));
                }
            }
        }
        let lib = lib.expect("test plugin library not found in cargo output");
        let dir = std::env::temp_dir().join(format!("rosaclef-clap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join("rosaclef-test.clap");
        std::fs::copy(&lib, &dst).unwrap();
        dst
    })
}

fn plugin_device(id: &str, params: &[(&str, f64)]) -> Device {
    let mut d = Device::new("plugin");
    d.options.insert("format".into(), "clap".into());
    d.options
        .insert("path".into(), test_plugin().to_string_lossy().into_owned());
    d.options.insert("id".into(), id.into());
    for (k, v) in params {
        d.params.insert((*k).into(), *v);
    }
    d
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn scan_finds_both_plugins() {
    let dir = test_plugin().parent().unwrap().to_path_buf();
    let found = scan(&[dir]);
    let sine = found.iter().find(|d| d.id == SINE).expect("sine plugin");
    let gain = found.iter().find(|d| d.id == GAIN).expect("gain plugin");
    assert!(sine.instrument && !sine.effect);
    assert!(gain.effect && !gain.instrument);
    assert_eq!(sine.format, "clap");
    assert_eq!(sine.path, test_plugin().to_string_lossy());
    assert!(sine.features.contains(&"synthesizer".to_string()));
    assert_eq!(sine.name, "Rosaclef Test Sine");
    let json = serde_json::to_value(sine).unwrap();
    assert_eq!(json["instrument"], true);
    assert!(json.get("description").is_some());
}

#[test]
fn scan_skips_broken_files() {
    let dir = std::env::temp_dir().join(format!("rosaclef-clap-broken-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken.clap"), b"not a plugin").unwrap();
    assert!(scan(&[dir.clone(), dir.join("missing")]).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lists_params() {
    let path = test_plugin().to_string_lossy().into_owned();
    let p = params(&path, SINE).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(
        (
            p[0].id.as_str(),
            p[0].name.as_str(),
            p[0].min,
            p[0].max,
            p[0].default
        ),
        ("0", "Gain", 0.0, 1.0, 0.5)
    );
    let p = params(&path, GAIN).unwrap();
    assert_eq!((p[0].min, p[0].max, p[0].default), (0.0, 2.0, 1.0));
    // Empty id picks the first plugin.
    assert_eq!(params(&path, "").unwrap()[0].max, 1.0);
    assert!(params(&path, "nope").is_err());
    assert!(params("/nonexistent/x.clap", "").is_err());
}

#[test]
fn instrument_plays_notes() {
    let host = ClapHost::new();
    let mut p = host
        .load(&plugin_device(SINE, &[]), true, 48000.0, 128)
        .unwrap();
    let mut l = vec![0f32; 128];
    let mut r = vec![0f32; 128];
    p.process(
        &[NoteEvent {
            offset: 10,
            kind: NoteKind::On {
                key: 69,
                velocity: 1.0,
            },
            lyric: None,
        }],
        &mut l,
        &mut r,
    );
    assert_eq!(peak(&l[..10]), 0.0, "silent before the note");
    assert_eq!(peak(&r[..10]), 0.0);
    assert!(peak(&l[10..]) > 0.0, "sound after the note");
    assert!(peak(&r[10..]) > 0.0);
    for _ in 0..4 {
        l.fill(0.0);
        r.fill(0.0);
        p.process(&[], &mut l, &mut r);
        assert!(peak(&l) > 0.05);
    }
    // Shorter blocks work too.
    p.process(&[], &mut l[..37], &mut r[..37]);
    assert!(peak(&l[..37]) > 0.05);

    p.set_param("0", 0.0);
    for _ in 0..3 {
        l.fill(0.0);
        r.fill(0.0);
        p.process(&[], &mut l, &mut r);
        assert_eq!(peak(&l), 0.0, "gain 0 silences the synth");
        assert_eq!(peak(&r), 0.0);
    }
    // Back up, then AllOff releases the note.
    p.set_param("0", 1.0);
    p.process(
        &[NoteEvent {
            offset: 0,
            kind: NoteKind::AllOff,
            lyric: None,
        }],
        &mut l,
        &mut r,
    );
    for _ in 0..40 {
        p.process(&[], &mut l, &mut r);
    }
    assert_eq!(peak(&l), 0.0, "released after AllOff");
}

#[test]
fn load_params_are_applied() {
    let host = ClapHost::new();
    let mut p = host
        .load(&plugin_device(SINE, &[("0", 0.0)]), true, 48000.0, 128)
        .unwrap();
    let mut l = vec![0f32; 128];
    let mut r = vec![0f32; 128];
    p.process(
        &[NoteEvent {
            offset: 0,
            kind: NoteKind::On {
                key: 60,
                velocity: 1.0,
            },
            lyric: None,
        }],
        &mut l,
        &mut r,
    );
    p.process(&[], &mut l, &mut r);
    assert_eq!(peak(&l), 0.0);
}

#[test]
fn gain_effect_doubles_input() {
    let host = ClapHost::new();
    let mut p = host
        .load(&plugin_device(GAIN, &[("0", 2.0)]), false, 48000.0, 128)
        .unwrap();
    let mut l: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin() * 0.25).collect();
    let mut r: Vec<f32> = l.iter().map(|x| -x).collect();
    let (l0, r0) = (l.clone(), r.clone());
    p.process(&[], &mut l, &mut r);
    for i in 0..128 {
        assert!((l[i] - 2.0 * l0[i]).abs() < 1e-6);
        assert!((r[i] - 2.0 * r0[i]).abs() < 1e-6);
    }
}

#[test]
fn load_errors() {
    let host = ClapHost::new();
    assert!(host
        .load(
            &plugin_device("dev.rosaclef.missing", &[]),
            true,
            48000.0,
            128
        )
        .is_err());
    let mut d = plugin_device(SINE, &[]);
    d.options
        .insert("path".into(), "/nonexistent/x.clap".into());
    assert!(host.load(&d, true, 48000.0, 128).is_err());
    d.options.insert("format".into(), "vst3".into());
    assert!(host.load(&d, true, 48000.0, 128).is_err());
}

#[test]
fn engine_renders_plugin_instrument() {
    let mut project = Project::empty("t");
    project.channels.push(Channel {
        id: "sine".into(),
        name: "Sine".into(),
        color: "#ffffff".into(),
        instrument: plugin_device(SINE, &[("0", 0.8)]),
        volume: 0.8,
        pan: 0.0,
        mute: false,
        mixer: rosaclef_core::InsertIx(1),
        arp: None,
    });
    project.patterns[0].notes.push(Note {
        channel: "sine".into(),
        pitch: 60,
        start: 0.0,
        length: 1.0,
        velocity: 0.9,
    });
    project.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: rosaclef_core::TrackIx(0),
        start: 0.0,
        length: 4.0,
        offset: 0.0,
        gain: 1.0,
        mixer: rosaclef_core::InsertIx::MASTER,
        verse: None,
    });
    let mut engine = Engine::new(48000.0);
    engine.set_plugin_host(Arc::new(ClapHost::new()));
    engine.set_project(project);
    assert!(
        engine.device_errors.is_empty(),
        "{:?}",
        engine.device_errors
    );
    let audio = render(
        &mut engine,
        &RenderScope::Pattern {
            id: "pattern-1".into(),
            loops: 1,
        },
    );
    assert!(
        engine.device_errors.is_empty(),
        "{:?}",
        engine.device_errors
    );
    assert!(audio.peak() > 0.01, "peak {}", audio.peak());
    assert!(audio.rms() > 0.001);
}
