//! Engine behaviour tests (offline rendering, no audio device needed).

use rosaclef_core::automation::TempoMap;
use rosaclef_core::{validate, Channel, Clip, Device, InsertIx, Note, Project, TrackIx};
use rosaclef_engine::render::{render, render_note, RenderScope};
use rosaclef_engine::soundfont::SoundFont;
use rosaclef_engine::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

const DEMO: &str = include_str!("../../studio/assets/demo/project.json");

fn demo() -> Project {
    let checked = validate::parse_and_validate(DEMO);
    assert!(checked.is_ok(), "{:?}", checked.issues);
    checked.project.unwrap()
}

fn drum_project() -> Project {
    let mut p = Project::empty("t");
    let mut dev = Device::new("drum");
    dev.options.insert("kind".into(), "kick".into());
    p.channels.push(Channel {
        id: "k".into(),
        name: "Kick".into(),
        color: "#ffffff".into(),
        instrument: dev,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        mixer: InsertIx(1),
        arp: None,
        layer_of: None,
    });
    p.patterns[0].notes.push(Note {
        channel: "k".into(),
        pitch: 60,
        start: 0.0,
        length: 0.25,
        velocity: 1.0,
    });
    p.playlist.clips.push(Clip {
        pattern: "pattern-1".into(),
        sample: String::new(),
        track: TrackIx(0),
        start: 0.0,
        length: 16.0,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx::MASTER,
    });
    p
}

/// Frame index of every onset (sample crossing from near-silence).
fn onsets(left: &[f32], sr: f32) -> Vec<f32> {
    let mut out = vec![];
    let mut quiet = true;
    for (i, x) in left.iter().enumerate() {
        if quiet && x.abs() > 0.05 {
            out.push(i as f32 / sr);
            quiet = false;
        } else if !quiet
            && left[i.saturating_sub(2000)..=i]
                .iter()
                .all(|y| y.abs() < 0.01)
        {
            quiet = true;
        }
    }
    out
}

/// Load the built-in soundfont presets the engine's project plays
/// (web/soundfonts/gm), as the studio does.
fn load_presets(e: &mut Engine) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/soundfonts/gm");
    let sf = SoundFont::parse(&std::fs::read(dir.join("index.sf2")).unwrap()).unwrap();
    for key in e.required_presets() {
        let p = sf.find(key.bank, key.program as u16).unwrap();
        let mut pieces: HashMap<usize, Vec<u8>> = HashMap::new();
        for k in sf.pieces(p) {
            pieces.insert(
                k,
                std::fs::read(dir.join(format!("smpl-{k:03}.bin"))).unwrap(),
            );
        }
        let loaded = sf.load(p, &pieces, &mut HashMap::new()).unwrap();
        e.set_preset(key, Arc::new(loaded));
    }
}

#[test]
fn demo_song_renders_with_healthy_levels() {
    let mut e = Engine::new(48000.0);
    e.set_project(demo());
    assert!(
        !e.required_presets().is_empty(),
        "the demo plays the soundfont"
    );
    load_presets(&mut e);
    let project = demo();
    let end = project
        .playlist
        .clips
        .iter()
        .map(|c| c.start + c.length)
        .fold(0.0, f64::max);
    // The demo automates its tempo (a closing ritardando),
    // so the song's length in seconds follows the tempo map, not the project BPM.
    let song_seconds = TempoMap::new(&project).seconds_at(end);
    assert!(
        song_seconds > end * 60.0 / project.transport.bpm,
        "the demo's tempo automation slows it down"
    );
    let a = render(&mut e, &RenderScope::Song);
    assert!(
        a.duration() > song_seconds && a.duration() < song_seconds + 9.0,
        "duration {}",
        a.duration()
    );
    assert!(a.peak() > 0.3 && a.peak() <= 1.0, "peak {}", a.peak());
    assert!(a.rms() > 0.02, "rms {}", a.rms());
    assert!(a.left.iter().all(|x| x.is_finite()));
}

#[test]
fn pattern_loops_inside_a_clip() {
    let mut e = Engine::new(48000.0);
    e.set_project(drum_project());
    let a = render(&mut e, &RenderScope::Song);
    // A one-bar pattern with a kick on beat 1, looped over 4 bars at 120 BPM.
    let hits = onsets(&a.left, 48000.0);
    assert_eq!(hits.len(), 4, "{hits:?}");
    for (i, t) in hits.iter().enumerate() {
        assert!((t - i as f32 * 2.0).abs() < 0.01, "hit {i} at {t}");
    }
}

#[test]
fn muted_tracks_and_channels_are_silent() {
    let mut p = drum_project();
    p.playlist.tracks[0].mute = true;
    let mut e = Engine::new(48000.0);
    e.set_project(p);
    assert!(render(&mut e, &RenderScope::Song).peak() < 1e-6);

    let mut p = drum_project();
    p.channels[0].mute = true;
    let mut e = Engine::new(48000.0);
    e.set_project(p);
    assert!(render(&mut e, &RenderScope::Song).peak() < 1e-6);
}

#[test]
fn solo_silences_other_inserts() {
    let mut p = drum_project();
    p.mixer.inserts[2].solo = true;
    let mut e = Engine::new(48000.0);
    e.set_project(p);
    assert!(render(&mut e, &RenderScope::Song).peak() < 1e-6);
}

#[test]
fn a_layer_plays_the_notes_of_the_channel_it_layers() {
    // The kick's notes, played again by a sine layer on its own (soloed) insert.
    let with_layer = |layer_of: Option<&str>| {
        let mut p = drum_project();
        let mut sine = Device::new("analog");
        sine.options.insert("wave1".into(), "sine".into());
        sine.options.insert("filter".into(), "lowpass".into());
        for (k, v) in [
            ("osc2Mix", 0.0),
            ("sub", 0.0),
            ("drift", 0.0),
            ("cutoff", 20000.0),
        ] {
            sine.params.insert(k.into(), v);
        }
        p.channels.push(Channel {
            id: "body".into(),
            name: "Body".into(),
            color: "#ffffff".into(),
            instrument: sine,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            mixer: InsertIx(2),
            arp: None,
            layer_of: layer_of.map(String::from),
        });
        p.mixer.inserts[2].solo = true;
        assert!(
            validate::validate(&p).is_empty(),
            "{:?}",
            validate::validate(&p)
        );
        p
    };
    let mut e = Engine::new(48000.0);
    e.set_project(with_layer(None));
    assert!(
        render(&mut e, &RenderScope::Song).peak() < 1e-6,
        "no notes of its own"
    );
    e.set_project(with_layer(Some("k")));
    let a = render(&mut e, &RenderScope::Song);
    assert!(
        a.peak() > 0.05,
        "the layer plays the kick's notes: {}",
        a.peak()
    );

    // Live notes reach the layer too.
    let mut e = Engine::new(48000.0);
    e.set_project(with_layer(Some("k")));
    e.note_on("k", 60, 0.9);
    let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
    e.process(&mut l, &mut r);
    assert!(l.iter().any(|x| x.abs() > 0.05), "live note on the layer");
}

#[test]
fn every_builtin_instrument_makes_sound() {
    for kind in ["analog", "fm", "drum"] {
        let a = render_note(&Device::new(kind), 60, 0.9, 0.5, 48000.0);
        assert!(a.peak() > 0.05, "{kind} peak {}", a.peak());
    }
    for drum in rosaclef_core::catalog::DRUM_KINDS {
        let mut d = Device::new("drum");
        d.options.insert("kind".into(), drum.to_string());
        let a = render_note(&d, 60, 0.9, 1.0, 48000.0);
        assert!(a.peak() > 0.02, "drum {drum} peak {}", a.peak());
    }
}

#[test]
fn project_updates_keep_instruments() {
    // Changing a parameter must not cut a sounding note (instrument reused).
    let mut p = drum_project();
    p.channels[0].instrument = Device::new("analog");
    p.patterns[0].notes[0].length = 4.0;
    let mut e = Engine::new(48000.0);
    e.set_project(p.clone());
    e.set_mode(rosaclef_engine::PlayMode::Song);
    e.play();
    let mut l = vec![0.0; 4800];
    let mut r = vec![0.0; 4800];
    e.process(&mut l, &mut r);
    p.channels[0]
        .instrument
        .params
        .insert("cutoff".into(), 800.0);
    e.set_project(p);
    e.process(&mut l, &mut r);
    assert!(
        l.iter().any(|x| x.abs() > 0.01),
        "note was cut by the update"
    );
}
