//! `rosaclef mixcheck`: a golden report for a small project, the masking
//! model, gain reduction against signals of known level, and bar numbers
//! through a meter change and a repeat.

use rosaclef_core::Project;
use rosaclef_engine::render::{encode_wav, Audio};
use rosaclef_studio::fonts::{DirFonts, Fonts};
use rosaclef_studio::mixcheck::{self, timeline, Env, Options, Report};
use rosaclef_studio::Folder;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rosaclef-mixcheck-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("samples")).unwrap();
    d
}

fn fonts() -> Fonts {
    Fonts::new(Arc::new(DirFonts(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../web/soundfonts"
    )))))
}

fn check(dir: &std::path::Path, p: &Project, req: Value) -> Report {
    let folder = Folder::on_disk(dir);
    let fonts = fonts();
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &|_| {},
        progress: &|_| {},
        disk_cache: true,
    };
    let mut req = req;
    req["cache"] = json!(false);
    let o = Options::from_json(&req).unwrap();
    mixcheck::run(&env, p, &o).unwrap()
}

fn project(v: Value) -> Project {
    serde_json::from_value(v).unwrap()
}

/// A stereo WAV of `f(t)` for `seconds`.
fn wav(dir: &std::path::Path, name: &str, seconds: f64, f: impl Fn(f64) -> f32) {
    let sr = 48000.0;
    let n = (seconds * sr) as usize;
    let left: Vec<f32> = (0..n).map(|i| f(i as f64 / sr)).collect();
    let audio = Audio {
        sample_rate: sr as f32,
        right: left.clone(),
        left,
    };
    std::fs::write(dir.join("samples").join(name), encode_wav(&audio, 32)).unwrap();
}

/// A song of audio clips, one insert each (`(sample, insert name, effects)`).
fn clips_song(bpm: f64, beats: f64, parts: &[(&str, &str, Value)], master: Value) -> Project {
    let mut inserts = vec![json!({"name": "Master", "effects": master})];
    let mut clips = vec![];
    for (i, (sample, name, fx)) in parts.iter().enumerate() {
        inserts.push(json!({"name": name, "effects": fx}));
        clips.push(json!({"sample": format!("samples/{sample}"), "track": 0, "start": 0, "length": beats, "mixer": i + 1}));
    }
    project(json!({
        "format": "rosaclef/1", "meta": {"title": "t"},
        "transport": {"bpm": bpm, "beatsPerBar": 4},
        "playlist": {"tracks": [{"name": "A"}], "clips": clips},
        "mixer": {"inserts": inserts}
    }))
}

fn element<'a>(r: &'a Report, id: &str) -> &'a mixcheck::report::ElementOut {
    r.elements.iter().find(|e| e.id == id).unwrap_or_else(|| {
        panic!(
            "no element {id} in {:?}",
            r.elements.iter().map(|e| &e.id).collect::<Vec<_>>()
        )
    })
}

// ------------------------------------------------------------- golden

#[test]
fn golden_report_of_the_fixture() {
    let dir = scratch("golden");
    let text = include_str!("mixcheck/fixture.json");
    let p: Project = serde_json::from_str(text).unwrap();
    let mut r = check(
        &dir,
        &p,
        json!({"range": "2:3", "history": true, "target": "spotify"}),
    );
    r.render.ms = 0;
    let got = serde_json::to_string_pretty(&r).unwrap() + "\n";
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/mixcheck/golden.json"
    ));
    if std::env::var("UPDATE_GOLDEN").is_ok() || !path.exists() {
        std::fs::write(&path, &got).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert!(
        got == want,
        "the report changed (UPDATE_GOLDEN=1 cargo test -p rosaclef-studio --test mixcheck to accept it):\n{got}"
    );
    // The same numbers again (and from the cache, quantized the same way).
    let mut again = check(
        &dir,
        &p,
        json!({"range": "2:3", "history": true, "target": "spotify"}),
    );
    again.render.ms = 0;
    assert_eq!(serde_json::to_string_pretty(&again).unwrap() + "\n", want);
    // The report validates against its schema's top-level shape.
    let schema: Value = serde_json::from_str(mixcheck::SCHEMA).unwrap();
    let v = serde_json::to_value(&r).unwrap();
    for k in schema["required"].as_array().unwrap() {
        assert!(v.get(k.as_str().unwrap()).is_some(), "missing {k}");
    }
}

#[test]
fn a_cached_report_is_the_same() {
    let dir = scratch("cache");
    let p: Project = serde_json::from_str(include_str!("mixcheck/fixture.json")).unwrap();
    let folder = Folder::on_disk(&dir);
    let fonts = fonts();
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &|_| {},
        progress: &|_| {},
        disk_cache: true,
    };
    let o = Options::from_json(&json!({"range": "1:2"})).unwrap();
    let mut a = mixcheck::run(&env, &p, &o).unwrap();
    let mut b = mixcheck::run(&env, &p, &o).unwrap();
    assert!(!a.render.cached && b.render.cached);
    assert_eq!(b.render.renders, 0);
    a.render = Default::default();
    b.render = Default::default();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    // And from the disk, as a new process would find it.
    let key_files = std::fs::read_dir(dir.join(".rosaclef/mixcheck"))
        .unwrap()
        .count();
    assert_eq!(key_files, 1);
}

// ------------------------------------------------------------ masking

#[test]
fn a_quiet_pluck_under_a_loud_sub_is_masked() {
    let dir = scratch("masking");
    use std::f64::consts::TAU;
    // A loud 100 Hz sub, and a quiet 200 Hz pluck every half second.
    wav(&dir, "sub.wav", 8.0, |t| {
        (0.5 * (TAU * 100.0 * t).sin()) as f32
    });
    wav(&dir, "pluck.wav", 8.0, |t| {
        let local = t % 0.5;
        (0.012 * (-local * 8.0).exp() * (TAU * 200.0 * t).sin()) as f32
    });
    let p = clips_song(
        120.0,
        16.0,
        &[
            ("sub.wav", "Sub", json!([])),
            ("pluck.wav", "Pluck", json!([])),
        ],
        json!([]),
    );
    let r = check(&dir, &p, json!({}));
    let pluck = element(&r, "insert:2/Pluck");
    let a = pluck.audibility.as_ref().unwrap();
    assert!(
        matches!(pluck.verdict, "inaudible" | "buried"),
        "verdict {} ({}% audible)",
        pluck.verdict,
        a.audible_fraction_pct
    );
    assert!(a.audible_fraction_pct < 25.0, "{}", a.audible_fraction_pct);
    let by = &a.masked_by.as_ref().unwrap()[0];
    assert_eq!(by.id, "insert:1/Sub");
    assert!(
        by.band_hz[0] <= 200.0 && by.band_hz[1] >= 200.0,
        "{:?}",
        by.band_hz
    );
    // The sub itself is plainly audible (it is nearly all of the mix).
    assert_eq!(element(&r, "insert:1/Sub").verdict, "dominant");
    // A finding names it, with a fix.
    let f = r
        .findings
        .iter()
        .find(|f| f.rule == "inaudible-part")
        .unwrap_or_else(|| panic!("{} {:?}", pluck.verdict, r.findings));
    assert!(!f.fix.is_empty());

    // Alone, the same pluck is heard.
    let mut alone = p.clone();
    alone.mixer.inserts[1].mute = true;
    let r = check(&dir, &alone, json!({}));
    let pluck = element(&r, "insert:2/Pluck");
    assert_eq!(pluck.verdict, "dominant"); // it is the whole mix
    assert!(pluck.audibility.as_ref().unwrap().audible_fraction_pct > 90.0);

    // Loud enough, its attacks come through the sub (its decays still sink under it).
    let mut loud = p.clone();
    loud.mixer.inserts[2].volume = 2.0;
    loud.playlist.clips[1].gain = 15.0;
    let r = check(&dir, &loud, json!({"focus": "insert:2"}));
    let pluck = element(&r, "insert:2/Pluck");
    assert!(
        pluck.audibility.as_ref().unwrap().audible_fraction_pct >= 40.0,
        "{:?}",
        pluck.audibility
    );
}

// ---------------------------------------------------- gain reduction

#[test]
fn gain_reduction_of_signals_of_known_level() {
    let dir = scratch("gr");
    // A square wave at -6 dBFS: its level is constant, so the static curve
    // tells the reduction exactly.
    wav(&dir, "square.wav", 6.0, |t| {
        if (t * 220.0).fract() < 0.5 {
            0.5
        } else {
            -0.5
        }
    });
    let p = clips_song(
        120.0,
        12.0,
        &[
            // Compressor: (-6 - -18) × (1 - 1/4) = 9 dB.
            (
                "square.wav",
                "Comp",
                json!([{"type": "compressor", "params": {"threshold": -18, "ratio": 4, "attack": 1, "release": 50, "makeup": 0}}]),
            ),
            // Limiter: -6 dBFS + 12 dB into a -1 dB ceiling = 7 dB, then
            // the fader brings it down to stay clear of the master.
            (
                "square.wav",
                "Lim",
                json!([{"type": "limiter", "params": {"gain": 12, "ceiling": -1, "release": 50}}]),
            ),
        ],
        json!([]),
    );
    let mut p = p;
    p.mixer.inserts[2].volume = 0.1;
    let r = check(&dir, &p, json!({"checks": "gainreduction,levels"}));
    let gr = |id: &str| {
        r.gain_reduction
            .iter()
            .find(|g| g.id == id)
            .unwrap_or_else(|| panic!("no GR for {id}: {:?}", r.gain_reduction))
    };
    let c = gr("insert:1/Comp");
    assert_eq!(c.kind, "compressor");
    assert!((c.max - 9.0).abs() <= 0.3, "compressor max {}", c.max);
    assert!((c.mean - 9.0).abs() <= 0.5, "compressor mean {}", c.mean);
    assert!(c.pct_time_above3 >= 95.0, "{}", c.pct_time_above3);
    let l = gr("insert:2/Lim");
    assert!((l.max - 7.0).abs() <= 0.3, "limiter max {}", l.max);
    assert!((l.mean - 7.0).abs() <= 0.5, "limiter mean {}", l.mean);
    // Per bar, too.
    let row = r.per_bar.iter().find(|x| x.bar == Some(2)).unwrap();
    let map = row.gain_reduction_db.as_ref().unwrap();
    assert!(
        (map["insert:1/Comp#0"].as_f64().unwrap() - 9.0).abs() <= 0.3,
        "{map:?}"
    );
}

// ------------------------------------------------- meters and repeats

fn meter_song() -> Project {
    // 4 bars of 3/4 (beats 0–12), then 4/4 from bar 5; bars 5–6 (beats
    // 12–20) play twice.
    let notes: Vec<Value> = (0..28)
        .map(|b| json!({"channel": "k", "pitch": 60, "start": b, "length": 0.25, "velocity": 0.9}))
        .collect();
    project(json!({
        "format": "rosaclef/1", "meta": {"title": "meters"},
        "transport": {"bpm": 120, "beatsPerBar": 3, "meters": [{"bar": 5, "numerator": 4, "denominator": 4}]},
        "channels": [{"id": "k", "name": "Kick", "instrument": {"type": "drum", "options": {"kind": "kick"}}, "mixer": 1}],
        "patterns": [{"id": "p", "name": "P", "length": 28, "notes": notes}],
        "playlist": {"tracks": [{"name": "A"}], "clips": [{"pattern": "p", "track": 0, "start": 0, "length": 28}]},
        "mixer": {"inserts": [{"name": "Master"}, {"name": "Kick"}]},
        "repeats": [{"start": 12, "end": 20, "times": 2}]
    }))
}

#[test]
fn bars_follow_the_meters_and_each_pass_of_a_repeat() {
    let p = meter_song();
    let t = timeline::Timeline::new(&p);
    assert_eq!(t.bar_of(0.0), 1);
    assert_eq!(t.bar_of(11.9), 4);
    assert_eq!(t.bar_of(12.0), 5);
    assert_eq!(t.bar_start(6), 16.0);
    assert_eq!(t.bar_start(7), 20.0);
    let res = timeline::resolve(&p, &t, Some("4:6"), None, None).unwrap();
    assert_eq!((res.from_beat, res.to_beat), (9.0, 20.0));
    // Bars 5–6 play twice, back to back: one stretch of the performance.
    let seg = t.segments(&[(12.0, 20.0)]);
    assert_eq!(seg.len(), 1);
    assert_eq!((seg[0].from, seg[0].to), (12.0, 28.0));
    // Bar 5 alone: its two passes are apart.
    let seg = t.segments(&[(12.0, 16.0)]);
    assert_eq!(seg.len(), 2);
    assert_eq!(
        (seg[0].from, seg[0].to, seg[1].from, seg[1].to),
        (12.0, 16.0, 20.0, 24.0)
    );
    assert_eq!(t.segments(&res.ranges).len(), 1);

    let dir = scratch("meters");
    let r = check(&dir, &p, json!({"range": "4:7"}));
    let rows: Vec<(u32, Option<u32>, f64, f64)> = r
        .per_bar
        .iter()
        .map(|x| (x.bar.unwrap(), x.pass, x.from_beat, x.to_beat))
        .collect();
    assert_eq!(
        rows,
        vec![
            (4, Some(1), 9.0, 12.0),
            (5, Some(1), 12.0, 16.0),
            (6, Some(1), 16.0, 20.0),
            (5, Some(2), 12.0, 16.0),
            (6, Some(2), 16.0, 20.0),
            (7, Some(1), 20.0, 24.0),
        ]
    );
    assert!(r.range.repeats);
    // 3 + 4 + 4 + 4 + 4 + 4 beats at 120 BPM.
    assert!((r.range.seconds - 11.5).abs() < 0.15, "{}", r.range.seconds);
    // A section-less song has no --section.
    let folder = Folder::on_disk(&dir);
    let fonts = fonts();
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &|_| {},
        progress: &|_| {},
        disk_cache: true,
    };
    let o = Options::from_json(&json!({"section": "Chorus"})).unwrap();
    assert!(mixcheck::run(&env, &p, &o)
        .unwrap_err()
        .0
        .contains("no section"));
}

#[test]
fn what_if_never_touches_the_project_and_reports_the_change() {
    let dir = scratch("whatif");
    let p: Project = serde_json::from_str(include_str!("mixcheck/fixture.json")).unwrap();
    let before = p.clone();
    let r = check(
        &dir,
        &p,
        json!({"range": "2:3", "whatIf": [{"op": "replace", "path": "/mixer/inserts/0/effects/1/params/gain", "value": 0}]}),
    );
    assert_eq!(p, before);
    let w = r.what_if.as_ref().unwrap();
    assert!(
        w["master"]["integratedLufs"]["delta"].as_f64().unwrap() < 0.0,
        "{w}"
    );
    assert_eq!(r.render.renders, 2);
    // A bad path is named.
    let folder = Folder::on_disk(&dir);
    let fonts = fonts();
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &|_| {},
        progress: &|_| {},
        disk_cache: true,
    };
    let o = Options::from_json(
        &json!({"whatIf": [{"op": "replace", "path": "/channels/9/volume", "value": 1}]}),
    )
    .unwrap();
    assert_eq!(
        mixcheck::run(&env, &p, &o).unwrap_err().0,
        "whatIf[0] replace /channels/9/volume: no element 9"
    );
}

#[test]
fn a_clash_names_the_notes_and_its_fix_resolves_it() {
    let dir = scratch("clash");
    let p: Project = serde_json::from_str(include_str!("mixcheck/fixture.json")).unwrap();
    // Lead C#5 (pattern tune, note 1) over the pad's C4 in bar 2.
    let r = check(&dir, &p, json!({"range": "2:4", "threshold": "strict"}));
    let cl = r.clashes.as_ref().unwrap();
    let c = cl
        .iter()
        .find(|c| c.a.pattern == "tune" && c.a.note_index == 1)
        .unwrap_or_else(|| panic!("{cl:?}"));
    assert_eq!(c.bar, 2);
    assert_eq!(c.a.pitch, "C#5");
    assert_eq!(c.interval, "m9");
    let fix = c.fix.clone().unwrap();
    let r2 = check(
        &dir,
        &p,
        json!({"range": "2:4", "threshold": "strict", "whatIf": fix}),
    );
    assert!(!r2
        .clashes
        .unwrap()
        .iter()
        .any(|x| x.a.pattern == c.a.pattern
            && x.a.note_index == c.a.note_index
            && x.b.note_index == c.b.note_index
            && x.interval == "m9"));
}
