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
        any_file: true,
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
    // The same numbers when rendered again.
    let mut again = check(
        &dir,
        &p,
        json!({"range": "2:3", "history": true, "target": "spotify"}),
    );
    again.render.ms = 0;
    assert_eq!(serde_json::to_string_pretty(&again).unwrap() + "\n", want);
    // The report validates against its schema.
    let schema: Value = serde_json::from_str(mixcheck::SCHEMA).unwrap();
    let v = serde_json::to_value(&r).unwrap();
    if let Err(e) = conforms(&v, &schema, "") {
        panic!("the report does not follow mixcheck.schema.json: {e}");
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
        any_file: true,
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
    mixcheck::cache::forget();
    let mut c = mixcheck::run(&env, &p, &o).unwrap();
    assert!(c.render.cached, "read back from the disk");
    c.render = Default::default();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&c).unwrap()
    );
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
    // 32 dB under the sub, out of reach of its fader (+6 dB at most): no
    // level fix is offered, and the finding says what to do instead.
    assert!(f.fix.is_empty(), "{f:?}");
    assert!(f.detail.contains("give it room"), "{}", f.detail);

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
        any_file: true,
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
        any_file: true,
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

/// A small JSON Schema checker for what mixcheck.schema.json uses: type,
/// properties, required, additionalProperties: false, items, enum, const.
fn conforms(v: &Value, s: &Value, at: &str) -> Result<(), String> {
    if let Some(t) = s.get("type") {
        let names: Vec<&str> = match t {
            Value::String(x) => vec![x.as_str()],
            Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect(),
            _ => vec![],
        };
        let ok = names.iter().any(|n| match *n {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "number" => v.is_number(),
            "integer" => v.as_f64().is_some_and(|x| x.fract() == 0.0),
            "boolean" => v.is_boolean(),
            "null" => v.is_null(),
            _ => true,
        });
        if !ok {
            return Err(format!("{at}: {v} is not {names:?}"));
        }
    }
    if let Some(e) = s.get("enum").and_then(|e| e.as_array()) {
        if !e.contains(v) {
            return Err(format!("{at}: {v} is not one of {e:?}"));
        }
    }
    if let Some(c) = s.get("const") {
        let same = match (c.as_f64(), v.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => c == v,
        };
        if !same {
            return Err(format!("{at}: {v} is not {c}"));
        }
    }
    if let Some(o) = v.as_object() {
        for r in s
            .get("required")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
        {
            if !o.contains_key(r.as_str().unwrap_or("")) {
                return Err(format!("{at}: missing {r}"));
            }
        }
        if let Some(props) = s.get("properties").and_then(|p| p.as_object()) {
            for (k, x) in o {
                match props.get(k) {
                    Some(ps) => conforms(x, ps, &format!("{at}/{k}"))?,
                    None if s.get("additionalProperties") == Some(&Value::Bool(false)) => {
                        return Err(format!("{at}: unexpected {k:?}"))
                    }
                    None => {}
                }
            }
        }
    }
    if let (Some(a), Some(items)) = (v.as_array(), s.get("items")) {
        for (i, x) in a.iter().enumerate() {
            conforms(x, items, &format!("{at}/{i}"))?;
        }
    }
    Ok(())
}

// ------------------------------------------------- ranges and their render

#[test]
fn a_range_measures_like_the_same_bars_of_the_whole_song() {
    // Pre-roll, seeking and held notes: bars 3–4 measured alone read like
    // bars 3–4 of the whole song (reverb tails and the pad's held chord
    // included) — and so does the second pass of a repeat.
    let dir = scratch("ranges");
    let p: Project = serde_json::from_str(include_str!("mixcheck/fixture.json")).unwrap();
    let whole = check(&dir, &p, json!({}));
    let part = check(&dir, &p, json!({"range": "3:4"}));
    let row = |r: &Report, bar: u32, pass: u32| {
        r.per_bar
            .iter()
            .find(|x| x.bar == Some(bar) && x.pass.unwrap_or(1) == pass)
            .cloned()
            .unwrap_or_else(|| panic!("no bar {bar}"))
    };
    let close = |a: Option<f64>, b: Option<f64>| (a.unwrap() - b.unwrap()).abs() <= 0.3;
    for bar in [3, 4] {
        let (a, b) = (row(&whole, bar, 1), row(&part, bar, 1));
        assert!(
            close(
                a.lufs_momentary_max.flatten(),
                b.lufs_momentary_max.flatten()
            ) && close(a.pre_limiter_peak_dbfs, b.pre_limiter_peak_dbfs),
            "bar {bar}: {a:?} vs {b:?}"
        );
    }
    let p = meter_song();
    let whole = check(&dir, &p, json!({}));
    let part = check(&dir, &p, json!({"range": "6:6"}));
    let (a, b) = (row(&whole, 6, 2), row(&part, 6, 2));
    assert!(
        close(
            a.lufs_momentary_max.flatten(),
            b.lufs_momentary_max.flatten()
        ),
        "bar 6, pass 2: {a:?} vs {b:?}"
    );
}

#[test]
fn bad_requests_are_refused_by_name() {
    let p = meter_song();
    let t = timeline::Timeline::new(&p);
    for (bars, beats) in [
        (Some("1:4294967296"), None),
        (Some("1:99999999999999999999"), None),
        (Some("NaN:3"), None),
        (Some("0:3"), None),
        (Some("40:41"), None),
        (None, Some("NaN:5")),
        (None, Some("1:inf")),
        (None, Some("5:2")),
    ] {
        let r = timeline::resolve(&p, &t, bars, beats, None);
        match (bars, &r) {
            // A range past the end ends with the song.
            (Some("1:4294967296" | "1:99999999999999999999"), Ok(x)) => assert_eq!(x.to_bar, 8),
            (_, Err(e)) => assert!(e.starts_with("range:") || e.starts_with("beats:"), "{e}"),
            _ => panic!("{bars:?} {beats:?} gave {r:?}"),
        }
    }

    // A reference outside the project folder, over HTTP.
    let dir = scratch("refuse");
    let folder = Folder::on_disk(&dir);
    let fonts = fonts();
    let env = Env {
        folder: &folder,
        fonts: &fonts,
        setup: &|_| {},
        progress: &|_| {},
        disk_cache: false,
        any_file: false,
    };
    let e = mixcheck::api(&env, &p, r#"{"reference": "/etc/hostname"}"#).unwrap_err();
    assert!(e.0.contains("in the project folder"), "{e}");
    let e = mixcheck::api(&env, &p, r#"{"reference": "../x.wav"}"#).unwrap_err();
    assert!(e.0.contains("in the project folder"), "{e}");

    // A fix is not applied to a song this version reads only in part…
    let mut doc = serde_json::to_value(&p).unwrap();
    doc["fromTheFuture"] = json!(true);
    let body = json!({"project": doc, "apply": [{"op": "replace", "path": "/transport/bpm", "value": 100}]});
    let e = mixcheck::api(&env, &p, &body.to_string()).unwrap_err();
    assert!(e.0.contains("doesn't know"), "{e}");
    // …and is to one it reads whole.
    let body =
        json!({"project": p, "apply": [{"op": "replace", "path": "/transport/bpm", "value": 100}]});
    let v = mixcheck::api(&env, &p, &body.to_string()).unwrap();
    assert_eq!(v["project"]["transport"]["bpm"], json!(100.0));
}

// ------------------------------------------------- a song with known faults

/// "Trouble" (tests/mixcheck/trouble.json): a song made with faults on
/// purpose — a limiter driven 9 dB with a 5 ms release under hot faders,
/// a lead under a loud pad in its register, a quiet rhythm bass under the
/// sub, keys and pad, a counter-melody's C#5 over the pad's C4, warm keys
/// thickening the low mids, a "widener" whose channels cancel, and four
/// sections at one loudness — with a repeated chorus and a 3/4 outro.
fn trouble(dir: &PathBuf) -> Project {
    use std::f64::consts::TAU;
    wav(dir, "widener.wav", 8.0, |_| 0.0);
    // The widener: the right channel is the left upside down.
    let sr = 48000.0;
    let left: Vec<f32> = (0..(8.0 * sr) as usize)
        .map(|i| {
            let t = i as f64 / sr;
            (0.25 * (TAU * 660.0 * t).sin() * (0.6 + 0.4 * (TAU * 0.5 * t).sin())) as f32
        })
        .collect();
    let right = left.iter().map(|x| -x).collect();
    let audio = Audio {
        sample_rate: sr as f32,
        left,
        right,
    };
    std::fs::write(dir.join("samples/widener.wav"), encode_wav(&audio, 32)).unwrap();
    serde_json::from_str(include_str!("mixcheck/trouble.json")).unwrap()
}

#[test]
fn every_planted_fault_is_found_and_the_fixes_converge() {
    let dir = scratch("trouble");
    let p = trouble(&dir);
    // Bars 8–13: the verse's end (with the widener), the chorus twice and
    // the first outro bar — every fault, in a debug build's time.
    let req = |what_if: Value| json!({"range": "8:13", "sampleRate": 24000, "maxFindings": 20, "whatIf": what_if});
    let r = check(&dir, &p, req(json!([])));
    let has = |rule: &str, el: Option<&str>| {
        r.findings
            .iter()
            .any(|f| f.rule == rule && el.is_none_or(|e| f.element.as_deref() == Some(e)))
    };
    for (rule, el) in [
        ("master-overload", None),
        ("limiter-pumping", None),
        ("masked-lead", Some("channel:lead")),
        ("inaudible-part", Some("channel:rbass")),
        ("low-end-buildup", None),
        ("phase-correlation", Some("insert:7/Widener")),
        ("section-loudness-flat", None),
    ] {
        assert!(
            has(rule, el),
            "{rule} {el:?} not found in {:#?}",
            r.findings
        );
    }
    // One lead (the part named so), one finding per problem.
    assert_eq!(
        r.findings
            .iter()
            .filter(|f| f.rule == "masked-lead")
            .count(),
        1
    );
    assert_eq!(
        r.findings
            .iter()
            .filter(|f| f.rule == "master-overload")
            .count(),
        1
    );
    // The planted clash, down to the note.
    let c = r
        .clashes
        .as_ref()
        .unwrap()
        .iter()
        .find(|c| {
            c.a.channel == "counter"
                && c.a.pitch == "C#5"
                && c.b.channel == "pad"
                && c.b.pitch == "C4"
        })
        .expect("the counter's C#5 over the pad's C4");
    assert_eq!((c.interval.as_str(), c.a.note_index), ("m9", 0));

    // Applying the fixes, as an agent would, round after round: the limiter
    // stops fighting and the lead comes through.
    let mut ops: Vec<Value> = vec![];
    let mut last = r;
    for _ in 0..3 {
        for f in &last.findings {
            if matches!(
                f.rule,
                "master-overload"
                    | "limiter-pumping"
                    | "masked-lead"
                    | "inaudible-part"
                    | "low-end-buildup"
            ) {
                ops.extend(f.fix.iter().cloned());
            }
        }
        last = check(&dir, &p, req(json!(ops.clone())));
    }
    let gr = last.master.limiter_gain_reduction_db.as_ref().unwrap();
    assert!(gr.mean < 1.0 && gr.pct_time_above3 < 5.0, "{gr:?}");
    assert!(
        last.master.plr_db.flatten().unwrap() > 7.0,
        "{:?}",
        last.master.plr_db
    );
    for rule in ["master-overload", "limiter-pumping", "masked-lead"] {
        assert!(
            !last.findings.iter().any(|f| f.rule == rule),
            "{rule} left: {:#?}",
            last.findings
        );
    }
}
