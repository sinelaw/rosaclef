use super::*;
use crate::model::{
    AutomationLane, AutomationPoint, Channel, Clip, Device, Insert, InsertIx, Note, Pattern, Track,
    TrackIx,
};

// ------------------------------------------------------------------ builders

/// An empty project: a bare master, no patterns, tracks or channels, with
/// every check on (the ones that are off by default too).
fn blank() -> Project {
    let mut p = Project::empty("Test");
    p.critic.on = RULES
        .iter()
        .filter(|r| !r.default_on)
        .map(|r| r.id.to_string())
        .collect();
    p.patterns.clear();
    p.playlist.tracks.clear();
    p.mixer.inserts.truncate(1);
    p.mixer.inserts[0].effects.clear();
    p
}

fn device(kind: &str, option: &str) -> Device {
    let mut d = Device::new(kind);
    match kind {
        "soundfont" => {
            d.options.insert("program".into(), option.into());
        }
        "drum" => {
            d.options.insert("kind".into(), option.into());
        }
        _ => {}
    }
    d
}

fn channel(p: &mut Project, id: &str, kind: &str, option: &str) {
    p.channels.push(Channel {
        id: id.into(),
        name: id.into(),
        color: "#d4af37".into(),
        instrument: device(kind, option),
        volume: 0.8,
        pan: 0.0,
        mute: false,
        mixer: InsertIx(0),
        arp: None,
        layer_of: None,
    });
}

/// A pattern of (channel, pitch, start, length, velocity) notes.
fn pattern(p: &mut Project, id: &str, length: f64, notes: &[(&str, i32, f64, f64, f64)]) {
    p.patterns.push(Pattern {
        id: id.into(),
        name: id.into(),
        color: "#d4af37".into(),
        length,
        notes: notes
            .iter()
            .map(|n| Note {
                channel: n.0.into(),
                pitch: n.1,
                start: n.2,
                length: n.3,
                velocity: n.4,
            })
            .collect(),
        drums: None,
    });
}

fn place(p: &mut Project, pat: &str, track: usize, start: f64, length: f64) {
    while p.playlist.tracks.len() <= track {
        let name = format!("T{}", p.playlist.tracks.len() + 1);
        p.playlist.tracks.push(Track { name, mute: false });
    }
    p.playlist.clips.push(Clip {
        pattern: pat.into(),
        sample: String::new(),
        track: TrackIx(track as u32),
        start,
        length,
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx(0),
    });
}

fn insert(name: &str) -> Insert {
    let mut i = Insert::new(name);
    i.volume = 0.8;
    i
}

fn with(kind: &str, params: &[(&str, f64)]) -> Device {
    let mut d = Device::new(kind);
    for (k, v) in params {
        d.params.insert((*k).into(), *v);
    }
    d
}

fn rules(p: &Project) -> Vec<&'static str> {
    critique(p, &[])
        .findings
        .into_iter()
        .map(|f| f.rule)
        .collect()
}

fn has(p: &Project, rule: &str) -> bool {
    rules(p).contains(&rule)
}

/// Every fix (of `only`, or all) makes its own finding go away, and leaves a
/// valid project.
fn fixes_resolve(p: &Project, only: &str) {
    let found: Vec<Finding> = critique(p, &[])
        .findings
        .into_iter()
        .filter(|f| f.fix.is_some() && (only.is_empty() || f.rule == only))
        .collect();
    if !only.is_empty() {
        assert!(!found.is_empty(), "{only} is offered as a fix");
    }
    for f in found {
        let (q, done) = apply_fixes(p, std::slice::from_ref(&f.key), &[])
            .unwrap_or_else(|e| panic!("{}: {e}", f.key));
        assert_eq!(done.len(), 1, "{} applied", f.key);
        let errors: Vec<_> = crate::validate::validate(&q)
            .into_iter()
            .filter(|i| i.severity == crate::validate::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "{} leaves a valid project: {errors:?}",
            f.key
        );
        assert!(
            !critique(&q, &[]).findings.iter().any(|x| x.key == f.key),
            "\"{}\" resolves {}",
            f.fix.as_ref().unwrap().label,
            f.key
        );
    }
}

// ------------------------------------------------------------------ catalog

#[test]
fn every_rule_has_a_category_and_a_reason() {
    for r in RULES {
        assert!(CATEGORIES.contains(&r.category), "{}", r.id);
        assert!(r.why.len() > 10, "{}", r.id);
    }
    for (i, r) in RULES.iter().enumerate() {
        assert_eq!(
            RULES.iter().position(|x| x.id == r.id),
            Some(i),
            "{} is unique",
            r.id
        );
    }
}

#[test]
fn an_empty_project_has_nothing_to_say() {
    let found: Vec<_> = critique(&blank(), &[])
        .findings
        .into_iter()
        .filter(|f| f.rule != "names")
        .collect();
    assert!(found.is_empty(), "{found:?}");
}

// ------------------------------------------------------------------ rhythm

fn hats(vel: impl Fn(usize) -> f64) -> Project {
    let mut p = blank();
    channel(&mut p, "hat", "drum", "hat");
    let ns: Vec<_> = (0..16)
        .map(|i| ("hat", 60, i as f64 * 0.25, 0.25, vel(i)))
        .collect();
    pattern(&mut p, "beat", 4.0, &ns);
    p
}

#[test]
fn flat_velocities() {
    let p = hats(|_| 0.8);
    assert!(has(&p, "flat-velocity"));
    fixes_resolve(&p, "flat-velocity");
    assert!(!critique(&p, &["flat-velocity".into()])
        .findings
        .iter()
        .any(|f| f.rule == "flat-velocity"));
}

#[test]
fn machine_gun_runs() {
    let p = hats(|i| if i < 4 { 0.5 + i as f64 * 0.1 } else { 0.8 });
    assert!(has(&p, "machine-gun"));
    fixes_resolve(&p, "machine-gun");
}

#[test]
fn full_velocities() {
    let mut p = blank();
    channel(&mut p, "keys", "fm", "");
    let ns: Vec<_> = (0..10)
        .map(|i| {
            (
                "keys",
                60 + (i % 5),
                i as f64 * 0.5,
                0.5,
                if i == 3 { 0.9 } else { 1.0 },
            )
        })
        .collect();
    pattern(&mut p, "loud", 8.0, &ns);
    assert!(has(&p, "max-velocity"));
    fixes_resolve(&p, "max-velocity");
}

#[test]
fn midi_hygiene() {
    let mut p = blank();
    channel(&mut p, "lead", "analog", "");
    pattern(
        &mut p,
        "messy",
        4.0,
        &[
            ("lead", 64, 0.0, 1.0, 0.8),
            ("lead", 64, 0.0, 1.0, 0.7),
            ("lead", 67, 1.0, 2.0, 0.8),
            ("lead", 67, 2.0, 1.0, 0.6),
            ("lead", 69, 3.0, 0.01, 0.8),
            ("lead", 71, 3.5, 0.5, 0.0),
            ("lead", 72, 5.0, 1.0, 0.8),
        ],
    );
    for rule in [
        "duplicate-notes",
        "same-pitch-overlap",
        "tiny-notes",
        "silent-notes",
        "past-end",
    ] {
        assert!(has(&p, rule), "{rule}");
    }
    fixes_resolve(&p, "");
}

#[test]
fn timing() {
    let mut p = blank();
    channel(&mut p, "piano", "soundfont", "Acoustic Grand Piano");
    let ns: Vec<_> = (0..16)
        .map(|i| {
            (
                "piano",
                60 + (i % 3) * 2,
                i as f64 * 0.5,
                0.5,
                0.6 + (i % 4) as f64 * 0.1,
            )
        })
        .collect();
    pattern(&mut p, "grid", 8.0, &ns);
    assert!(has(&p, "rigid-timing"));
    fixes_resolve(&p, "rigid-timing");
    p.patterns[0].notes[5].start = 2.52;
    assert!(has(&p, "sloppy-timing"));
    fixes_resolve(&p, "sloppy-timing");
}

// ------------------------------------------------------------------ harmony

#[test]
fn low_interval_limits() {
    let mut p = blank();
    channel(&mut p, "pad", "analog", "");
    pattern(
        &mut p,
        "low",
        8.0,
        &[
            ("pad", 36, 0.0, 4.0, 0.8),
            ("pad", 40, 0.0, 4.0, 0.8),
            ("pad", 43, 0.0, 4.0, 0.8),
            ("pad", 36, 4.0, 4.0, 0.8),
            ("pad", 41, 4.0, 4.0, 0.8),
            ("pad", 45, 4.0, 4.0, 0.8),
        ],
    );
    assert!(has(&p, "low-interval"));
    fixes_resolve(&p, "low-interval");
}

#[test]
fn chords_crowding_the_bass() {
    let mut p = blank();
    channel(&mut p, "bass", "analog", "");
    channel(&mut p, "keys", "analog", "");
    let mut ns = vec![
        ("bass", 36, 0.0, 2.0, 0.8),
        ("bass", 36, 2.0, 2.0, 0.8),
        ("bass", 41, 4.0, 2.0, 0.8),
        ("bass", 43, 6.0, 2.0, 0.8),
    ];
    ns.extend([
        ("keys", 43, 0.0, 4.0, 0.8),
        ("keys", 47, 0.0, 4.0, 0.8),
        ("keys", 50, 0.0, 4.0, 0.8),
        ("keys", 45, 4.0, 4.0, 0.8),
        ("keys", 47, 4.0, 4.0, 0.8),
        ("keys", 52, 4.0, 4.0, 0.8),
    ]);
    pattern(&mut p, "verse", 8.0, &ns);
    assert!(has(&p, "chord-too-low"));
    fixes_resolve(&p, "chord-too-low");
}

#[test]
fn voice_leading() {
    let mut p = blank();
    channel(&mut p, "keys", "fm", "");
    let chords = [
        [60, 64, 67],
        [69, 72, 76],
        [53, 57, 60],
        [67, 71, 74],
        [60, 64, 67],
        [69, 72, 76],
    ];
    let ns: Vec<_> = chords
        .iter()
        .enumerate()
        .flat_map(|(i, c)| {
            c.iter()
                .map(move |&x| ("keys", x, i as f64 * 2.0, 2.0, 0.8))
        })
        .collect();
    pattern(&mut p, "jumpy", 12.0, &ns);
    assert!(has(&p, "voice-leading"));
    fixes_resolve(&p, "voice-leading");
}

#[test]
fn parallel_fifths_in_acoustic_parts_only() {
    let mut p = blank();
    channel(&mut p, "strings", "soundfont", "String Ensemble 1");
    pattern(
        &mut p,
        "fifths",
        8.0,
        &[
            ("strings", 48, 0.0, 2.0, 0.8),
            ("strings", 55, 0.0, 2.0, 0.8),
            ("strings", 64, 0.0, 2.0, 0.8),
            ("strings", 50, 2.0, 2.0, 0.8),
            ("strings", 57, 2.0, 2.0, 0.8),
            ("strings", 65, 2.0, 2.0, 0.8),
        ],
    );
    assert!(has(&p, "parallel-fifths"));
    p.channels[0].instrument = Device::new("analog");
    assert!(!has(&p, "parallel-fifths"));
}

#[test]
fn gaps_in_the_upper_voices() {
    let mut p = blank();
    channel(&mut p, "keys", "analog", "");
    let mut ns = vec![];
    for (i, c) in [[48, 52, 79], [50, 53, 81], [48, 52, 79]]
        .iter()
        .enumerate()
    {
        for &x in c {
            ns.push(("keys", x, i as f64 * 2.0, 2.0, 0.8));
        }
    }
    pattern(&mut p, "gaps", 8.0, &ns);
    assert!(has(&p, "wide-spacing"));
    fixes_resolve(&p, "wide-spacing");
}

#[test]
fn the_key() {
    let mut p = blank();
    channel(&mut p, "lead", "analog", "");
    let scale = [60, 62, 64, 65, 67, 69, 71, 72];
    let mut ns: Vec<_> = (0..32)
        .map(|i| ("lead", scale[i % 8], i as f64 * 0.5, 0.5, 0.8))
        .collect();
    ns.push(("lead", 66, 16.0, 0.5, 0.8));
    for x in [48, 52, 55] {
        ns.push(("lead", x, 0.0, 8.0, 0.8));
    }
    pattern(&mut p, "tune", 17.0, &ns);
    assert_eq!(critique(&p, &[]).key, "C major");
    assert!(has(&p, "out-of-key"));
    fixes_resolve(&p, "out-of-key");
    p.score.key = "Eb".into();
    assert!(has(&p, "key-signature"));
    fixes_resolve(&p, "key-signature");
}

// ------------------------------------------------------------------ melody

#[test]
fn instrument_ranges() {
    let mut p = blank();
    channel(&mut p, "flute", "soundfont", "Flute");
    pattern(
        &mut p,
        "low",
        4.0,
        &[
            ("flute", 50, 0.0, 1.0, 0.8),
            ("flute", 62, 1.0, 1.0, 0.8),
            ("flute", 100, 2.0, 1.0, 0.8),
        ],
    );
    assert!(has(&p, "instrument-range"));
    fixes_resolve(&p, "instrument-range");
}

#[test]
fn sub_below_e1() {
    let mut p = blank();
    channel(&mut p, "sub", "analog", "");
    pattern(
        &mut p,
        "rumble",
        4.0,
        &[("sub", 22, 0.0, 2.0, 0.8), ("sub", 26, 2.0, 2.0, 0.8)],
    );
    assert!(has(&p, "sub-too-low"));
    fixes_resolve(&p, "sub-too-low");
}

#[test]
fn melodies() {
    let mut p = blank();
    channel(&mut p, "lead", "analog", "");
    let ns: Vec<_> = (0..40)
        .map(|i| ("lead", if i % 2 == 0 { 60 } else { 88 }, i as f64, 1.0, 0.8))
        .collect();
    pattern(&mut p, "wild", 40.0, &ns);
    for rule in ["melody-range", "large-leap", "no-rests", "monotone"] {
        assert!(has(&p, rule), "{rule}");
    }
}

// ------------------------------------------------------------------ arrangement

#[test]
fn arrangement() {
    let mut p = blank();
    channel(&mut p, "kick", "drum", "kick");
    let ns: Vec<_> = (0..4)
        .map(|i| ("kick", 60, i as f64, 0.5, 0.7 + i as f64 * 0.08))
        .collect();
    pattern(&mut p, "loop", 4.0, &ns);
    assert!(has(&p, "empty-playlist"));
    fixes_resolve(&p, "empty-playlist");
    place(&mut p, "loop", 0, 0.0, 160.0);
    assert!(has(&p, "loopitis"));
    place(&mut p, "loop", 0, 158.5, 4.0);
    assert!(has(&p, "clip-overlap"));
    fixes_resolve(&p, "clip-overlap");

    let mut q = blank();
    channel(&mut q, "kick", "drum", "kick");
    pattern(&mut q, "loop", 4.0, &ns);
    place(&mut q, "loop", 0, 0.25, 4.0);
    assert!(has(&q, "clip-off-bar"));
    fixes_resolve(&q, "clip-off-bar");
    pattern(&mut q, "copy", 4.0, &ns);
    place(&mut q, "copy", 1, 4.0, 4.0);
    assert!(has(&q, "identical-patterns"));
    fixes_resolve(&q, "identical-patterns");
}

// ------------------------------------------------------------------ mix and master

fn band() -> Project {
    let mut p = blank();
    p.meta.title = "Mix".into();
    for n in ["kick", "bass", "hat", "keys", "pad", "pluck"] {
        let drum = n == "kick" || n == "hat";
        channel(
            &mut p,
            n,
            if drum { "drum" } else { "analog" },
            if drum { n } else { "" },
        );
    }
    let mut ns = vec![];
    for i in 0..8 {
        ns.push(("kick", 60, i as f64, 0.5, 0.9 - (i % 2) as f64 * 0.2));
        ns.push(("hat", 60, i as f64 + 0.5, 0.25, 0.5 + (i % 3) as f64 * 0.1));
    }
    for i in 0..4 {
        ns.push(("bass", 36, i as f64 * 2.0, 2.0, 0.8 - i as f64 * 0.05));
        for c in ["keys", "pad", "pluck"] {
            ns.push((c, 64 + i, i as f64 * 2.0, 2.0, 0.6 + i as f64 * 0.05));
        }
    }
    pattern(&mut p, "all", 8.0, &ns);
    place(&mut p, "all", 0, 0.0, 8.0);
    p
}

#[test]
fn mix_and_master() {
    let mut p = band();
    p.channels[1].pan = 0.5;
    p.channels[3].volume = 1.3;
    p.mixer.inserts[0].volume = 1.4;
    p.mixer.inserts[0].effects.push(Device::new("limiter"));
    p.mixer.inserts[0].effects.push(Device::new("eq"));
    for rule in [
        "lowend-panned",
        "hot-faders",
        "master-hot",
        "limiter-last",
        "limiter-ceiling",
        "not-routed",
    ] {
        assert!(has(&p, rule), "{rule}");
    }
    fixes_resolve(&p, "");
}

#[test]
fn buses_and_effects() {
    let mut q = band();
    for i in 0..3 {
        q.mixer.inserts.push(insert(&format!("Bus {}", i + 1)));
    }
    q.channels[1].mixer = InsertIx(1);
    q.channels[4].mixer = InsertIx(2);
    q.channels[2].mixer = InsertIx(3);
    q.mixer.inserts[1]
        .effects
        .push(with("reverb", &[("mix", 0.4)]));
    q.mixer.inserts[2]
        .effects
        .push(with("delay", &[("time", 0.7)]));
    q.mixer.inserts[2].effects.push(Device::new("compressor"));
    q.mixer.inserts[3]
        .effects
        .push(with("compressor", &[("attack", 0.5)]));
    q.mixer.inserts[3].solo = true;
    for rule in [
        "reverb-on-bass",
        "delay-sync",
        "fx-order",
        "drum-attack",
        "solo",
        "no-highpass",
        "all-center",
    ] {
        assert!(has(&q, rule), "{rule}");
    }
    fixes_resolve(&q, "");
}

#[test]
fn more_checks() {
    let mut p = blank();
    p.meta.title = "More".into();
    p.transport.swing = 0.3;
    channel(&mut p, "kick", "drum", "kick");
    channel(&mut p, "snare", "drum", "snare");
    channel(&mut p, "bass", "wavetable", "");
    channel(&mut p, "sub", "analog", "");
    channel(&mut p, "keys", "analog", "");
    channel(&mut p, "pad", "analog", "");
    channel(&mut p, "idle", "analog", "");
    let mut ns = vec![];
    for i in 0..16 {
        ns.push(("kick", 60, i as f64, 0.25, 0.8 + (i % 2) as f64 * 0.1));
        ns.push((
            "snare",
            60,
            i as f64 * 0.5,
            0.25,
            if i < 8 { 0.7 } else { 0.9 },
        ));
    }
    for i in 0..4 {
        let t = i as f64 * 4.0;
        let v = 0.8 - i as f64 * 0.05;
        ns.extend([
            ("bass", 36, t, 4.0, v),
            ("sub", 31, t, 4.0, v),
            ("keys", 61, t, 4.0, 0.7),
            ("pad", 72, t, 4.0, 0.7),
        ]);
    }
    pattern(&mut p, "a", 16.0, &ns);
    pattern(&mut p, "empty", 4.0, &[]);
    pattern(&mut p, "spare", 4.0, &[("keys", 60, 0.0, 1.0, 0.8)]);
    place(&mut p, "a", 0, 0.0, 160.0);
    place(&mut p, "empty", 1, 0.0, 4.0);
    p.channels[2].mixer = InsertIx(1);
    p.channels[0].mixer = InsertIx(2);
    p.channels[1].mixer = InsertIx(2);
    p.channels[5].pan = -0.9;
    p.channels[4].pan = -0.6;
    p.channels[5].mute = true;
    p.channels[2].instrument.params.insert("width".into(), 0.9);
    p.channels[3]
        .instrument
        .params
        .insert("resonance".into(), 0.95);
    for n in ["Bass", "Drums", "Ghost"] {
        p.mixer.inserts.push(insert(n));
    }
    let mut off = Device::new("chorus");
    off.enabled = false;
    p.mixer.inserts[2].effects = vec![
        with("compressor", &[("ratio", 12.0), ("threshold", -40.0)]),
        with("eq", &[("mid", 12.0)]),
        off,
        Device::new("reverb"),
        Device::new("reverb"),
    ];
    p.mixer.inserts[3].effects = vec![
        with("delay", &[("feedback", 0.9), ("time", 0.5)]),
        with("reverb", &[("mix", 0.8)]),
    ];
    p.mixer.inserts[0].effects = vec![
        Device::new("eq"),
        Device::new("compressor"),
        Device::new("drive"),
        with("reverb", &[("mix", 0.3)]),
        with("limiter", &[("gain", 12.0), ("ceiling", -1.0)]),
    ];
    p.automation.push(AutomationLane {
        id: "flat".into(),
        name: String::new(),
        target: "tempo".into(),
        color: "#ffffff".into(),
        mute: false,
        points: vec![
            AutomationPoint {
                beat: 0.0,
                value: 120.0,
                curve: 0.0,
            },
            AutomationPoint {
                beat: 8.0,
                value: 120.0,
                curve: 0.0,
            },
        ],
    });
    let r = rules(&p);
    for id in [
        "swing-unused",
        "low-crowding",
        "kick-bass",
        "semitone-clash",
        "lowend-wide",
        "resonance",
        "unused-pattern",
        "empty-clips",
        "loopitis",
        "full-intro",
        "abrupt-ending",
        "unused-insert",
        "muted",
        "lopsided",
        "crushing-compressor",
        "eq-boost",
        "disabled",
        "double-reverb",
        "delay-feedback",
        "reverb-wet",
        "limiter-drive",
        "master-chain",
        "unused-channel",
        "flat-automation",
        "no-ghost-notes",
    ] {
        assert!(r.contains(&id), "{id} is reported");
    }
    fixes_resolve(&p, "");
    p.mixer.inserts[0].effects.clear();
    assert!(has(&p, "no-limiter"));
    fixes_resolve(&p, "no-limiter");
}

#[test]
fn project_hygiene() {
    let mut p = blank();
    p.transport.bpm = 127.83;
    channel(&mut p, "c", "analog", "");
    pattern(&mut p, "pattern-2", 6.0, &[("c", 60, 0.0, 1.0, 0.8)]);
    p.patterns[0].name = "Pattern 2".into();
    for rule in ["tempo", "pattern-bars", "names"] {
        assert!(has(&p, rule), "{rule}");
    }
    fixes_resolve(&p, "");
}

// ------------------------------------------------------------------ findings, suppressions and fixes

#[test]
fn findings_point_at_their_notes_and_keep_their_keys() {
    let p = hats(|_| 0.8);
    let fs = critique(&p, &[]).findings;
    for (i, f) in fs.iter().enumerate() {
        assert_eq!(
            fs.iter().position(|g| g.key == f.key),
            Some(i),
            "{} is unique",
            f.key
        );
    }
    let flat = fs.iter().find(|f| f.rule == "flat-velocity").unwrap();
    assert_eq!(flat.at.kind, "pattern");
    assert_eq!(flat.at.channel, "hat");
    assert_eq!(flat.at.notes.len(), 16);
    let mut q = p.clone();
    q.transport.bpm = 100.0;
    assert!(critique(&q, &[]).findings.iter().any(|f| f.key == flat.key));
}

#[test]
fn suppressions_live_in_the_project() {
    let mut p = hats(|_| 0.8);
    let key = critique(&p, &[])
        .findings
        .iter()
        .find(|f| f.rule == "flat-velocity")
        .unwrap()
        .key
        .clone();
    p.critic.suppress.push(key.clone());
    let c = critique(&p, &[]);
    let f = c.findings.iter().find(|f| f.key == key).unwrap();
    assert!(f.suppressed, "a suppressed finding is kept, marked");
    assert!(c.findings.last().unwrap().suppressed, "and sorted last");
    assert!(!report_text(&c, false).contains(&f.title));
    assert!(report_text(&c, true).contains(&f.title));
    // --fix all and --fix RULE leave it alone; its own key still fixes it.
    let (q, _) = apply_fixes(&p, &["all".into()], &[]).unwrap();
    assert!(critique(&q, &[])
        .findings
        .iter()
        .any(|f| f.key == key && f.suppressed));
    assert!(apply_fixes(&p, &["flat-velocity".into()], &[])
        .unwrap()
        .1
        .is_empty());
    assert_eq!(
        apply_fixes(&p, std::slice::from_ref(&key), &[])
            .unwrap()
            .1
            .len(),
        1
    );
    // A check turned off is not reported at all.
    p.critic.off.push("flat-velocity".into());
    assert!(!rules(&p).contains(&"flat-velocity"));
    // The settings round-trip through project.json, and validation knows the rules.
    let text = crate::format::to_string(&p);
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back.critic, p.critic);
    p.critic.off.push("no-such-check".into());
    assert!(crate::validate::validate(&p)
        .iter()
        .any(|i| i.path == "critic.off[1]"));
}

#[test]
fn json_patch() {
    let mut doc = json!({"a": {"b": [1, 2, 3]}});
    let ops = vec![
        analysis::set("/a/b/-".into(), json!(4)),
        analysis::set("/a/b/0".into(), json!(0)),
        analysis::remove("/a/b/1".into()),
        analysis::set("/a/c/d".into(), json!("x")),
    ];
    apply_ops(&mut doc, &ops).unwrap();
    assert_eq!(doc, json!({"a": {"b": [0, 2, 3, 4], "c": {"d": "x"}}}));
    assert!(apply_ops(&mut doc, &[analysis::remove("/a/b/9".into())]).is_err());
}

#[test]
fn fix_all_applies_every_suggestion() {
    let mut p = band();
    p.channels[1].pan = 0.5;
    p.mixer.inserts[0].volume = 1.4;
    let before = critique(&p, &[])
        .findings
        .iter()
        .filter(|f| f.fix.is_some())
        .count();
    let (q, done) = apply_fixes(&p, &["all".into()], &[]).unwrap();
    assert!(done.len() >= before.min(3), "{done:?}");
    assert!(!critique(&q, &[])
        .findings
        .iter()
        .any(|f| f.rule == "lowend-panned" || f.rule == "master-hot"));
}

#[test]
fn the_api() {
    let p = hats(|_| 0.8);
    let req = |fix: Vec<&str>| json!({"project": p, "fix": fix}).to_string();
    let v = api(&req(vec![])).unwrap();
    assert!(v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["rule"] == "flat-velocity" && f["fix"]["ops"].as_array().is_some()));
    assert!(v.get("project").is_none());
    let v = api(&req(vec!["flat-velocity"])).unwrap();
    let fixed: Project = serde_json::from_value(v["project"].clone()).unwrap();
    assert!(!rules(&fixed).contains(&"flat-velocity"));
    assert_eq!(v["applied"].as_array().unwrap().len(), 1);
    assert!(api("{").is_err());
    assert_eq!(catalog()["rules"].as_array().unwrap().len(), RULES.len());
}

#[test]
fn content_this_version_does_not_know() {
    let mut v = serde_json::to_value(hats(|i| 0.5 + (i % 4) as f64 * 0.1)).unwrap();
    v["hologram"] = json!(true);
    v["channels"][0]["instrument"] = json!({"type": "quantum-synth"});
    let req = |fix: Vec<&str>| json!({"project": v, "fix": fix}).to_string();
    let r = api(&req(vec![])).unwrap();
    let unknown: Vec<&Value> = r["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["rule"] == "unknown-content")
        .collect();
    assert_eq!(unknown.len(), 2, "{unknown:?}");
    assert!(unknown
        .iter()
        .any(|f| f["where"]["label"] == "hologram" && f["level"] == "warn"));
    assert!(unknown
        .iter()
        .any(|f| f["title"].as_str().unwrap().contains("quantum-synth")));
    // Fixing would write the song without what this version doesn't know.
    assert!(api(&req(vec!["all"]))
        .unwrap_err()
        .contains("nothing of it is lost"));
}

#[test]
fn theory_checks_are_off_unless_turned_on() {
    let theory = [
        "out-of-key",
        "key-signature",
        "parallel-fifths",
        "voice-leading",
        "wide-spacing",
        "semitone-clash",
        "melody-range",
        "large-leap",
        "leap-recovery",
        "no-rests",
        "monotone",
    ];
    for r in RULES {
        assert_eq!(r.default_on, !theory.contains(&r.id), "{}", r.id);
    }
    // A parallel-fifths part: quiet by default, reported once turned on.
    let mut p = blank();
    p.critic.on.clear();
    channel(&mut p, "strings", "soundfont", "String Ensemble 1");
    pattern(
        &mut p,
        "fifths",
        8.0,
        &[
            ("strings", 48, 0.0, 2.0, 0.8),
            ("strings", 55, 0.0, 2.0, 0.8),
            ("strings", 64, 0.0, 2.0, 0.8),
            ("strings", 50, 2.0, 2.0, 0.8),
            ("strings", 57, 2.0, 2.0, 0.8),
            ("strings", 65, 2.0, 2.0, 0.8),
        ],
    );
    assert!(!has(&p, "parallel-fifths"));
    assert_eq!(
        set_enabled(&mut p, "parallel-fifths", true).unwrap(),
        "turned on: Parallel fifths and octaves (parallel-fifths)"
    );
    assert_eq!(p.critic.on, ["parallel-fifths"]);
    assert!(has(&p, "parallel-fifths"));
    // Turning a check back to its default writes nothing.
    set_enabled(&mut p, "parallel-fifths", false).unwrap();
    assert!(p.critic.is_empty());
    // A check on by default is turned off in `off`; unsuppress turns it back on.
    suppress(&mut p, "low-interval", &[]).unwrap();
    assert_eq!(p.critic.off, ["low-interval"]);
    unsuppress(&mut p, "low-interval").unwrap();
    assert!(p.critic.is_empty());
    unsuppress(&mut p, "voice-leading").unwrap();
    assert_eq!(p.critic.on, ["voice-leading"]);
    assert!(rules_text(&p).contains("(on; off by default)"));
    assert!(set_enabled(&mut p, "no-such-check", true).is_err());
}

// ------------------------------------------------------------------ the demo

#[test]
fn the_demo() {
    let p: Project =
        serde_json::from_str(include_str!("../../../studio/assets/demo/project.json")).unwrap();
    let t0 = std::time::Instant::now();
    let c = critique(&p, &[]);
    eprintln!(
        "the demo: {} findings in {:?}",
        c.findings.len(),
        t0.elapsed()
    );
    assert!(
        !c.findings.is_empty() && c.findings.len() < 40,
        "{}",
        c.findings.len()
    );
    // Nothing the demo does well.
    for rule in [
        "flat-velocity",
        "loopitis",
        "no-contrast",
        "empty-playlist",
        "not-routed",
        "all-center",
        "no-limiter",
        "master-hot",
        "lowend-panned",
        "sub-too-low",
    ] {
        assert!(!c.findings.iter().any(|f| f.rule == rule), "{rule}");
    }
    fixes_resolve(&p, "");
}
