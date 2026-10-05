use super::*;
use crate::model::Project;

fn sec(name: &str, bars: u32, play: &str, fill: &str, crash: bool) -> DrumSection {
    DrumSection {
        name: name.into(),
        bars,
        play: play.into(),
        fill: fill.into(),
        crash,
        groove: String::new(),
    }
}

fn song(groove: &str, kit: &str) -> Project {
    let mut p = Project::empty("Drums");
    let mut part = DrumPart::new(groove);
    part.kit = kit.into();
    part.sections = vec![
        sec("Intro", 1, "count", "none", false),
        sec("Verse", 8, "a", "beat", false),
        sec("Chorus", 8, "b", "bar", true),
    ];
    p.drums = Some(part);
    p
}

#[test]
fn library_rows_are_well_formed() {
    let mut ids = std::collections::HashSet::new();
    for g in GROOVES {
        assert!(ids.insert(g.id), "duplicate groove id {}", g.id);
        assert_eq!(g.steps % g.bar_beats, 0, "{}: steps per beat", g.id);
        assert!(valid_kit(g.kit), "{}: kit {}", g.id, g.kit);
        assert!(g.tempo.0 < g.tempo.1, "{}: tempo", g.id);
        for (part, rows) in [("a", g.a), ("b", g.b)] {
            assert!(!rows.is_empty(), "{} {part}: empty", g.id);
            for (role, row) in rows {
                assert!(ROLES.contains(role), "{} {part}: unknown role {role}", g.id);
                let s = steps(row);
                assert!(
                    !s.is_empty() && s.len() % g.steps as usize == 0,
                    "{} {part} {role}: {} steps is not whole bars of {}",
                    g.id,
                    s.len(),
                    g.steps
                );
                for c in s {
                    assert!(
                        c == '.' || level(c).is_some(),
                        "{} {part} {role}: {c:?}",
                        g.id
                    );
                }
            }
        }
    }
    for (i, f) in FILLS.iter().enumerate() {
        for (role, row) in f.rows {
            assert!(ROLES.contains(role), "fill {i}: unknown role {role}");
            let s = steps(row);
            assert_eq!(
                s.len() as u32,
                f.beats * f.steps_per_beat,
                "fill {i} {role}"
            );
            for c in s {
                assert!(c == '.' || level(c).is_some(), "fill {i} {role}: {c:?}");
            }
        }
    }
}

#[test]
fn grooves_and_fills_are_playable() {
    for g in GROOVES.iter().filter(|g| !g.machine()) {
        for (part, rows) in [("a", g.a), ("b", g.b)] {
            let problems = playability(rows, g.steps_per_beat(), g.tempo.1);
            assert!(problems.is_empty(), "{} {part}: {problems:?}", g.id);
        }
    }
    for (i, f) in FILLS.iter().enumerate() {
        let problems = playability(f.rows, f.steps_per_beat, 180);
        assert!(problems.is_empty(), "fill {i}: {problems:?}");
    }
}

#[test]
fn playability_catches_three_hands() {
    let rows = [("hat", "x..."), ("snare", "x..."), ("tom1", "x...")];
    assert!(!playability(&rows, 4, 100).is_empty());
}

#[test]
fn every_meter_has_fills_of_every_size() {
    for g in GROOVES {
        for beats in [1, 2, g.bar_beats] {
            assert!(
                pick_fill(g.steps_per_beat(), beats, 1, "k", None).is_some(),
                "{}: no {beats}-beat fill",
                g.id
            );
        }
    }
}

#[test]
fn writes_a_song() {
    let mut p = song("rock-8ths", "");
    let r = write(&mut p).unwrap();
    // Count-in, groove A with a turnaround on bar 4 and a fill on bar 8,
    // B with a crash, a turnaround and a fill, the ending.
    let clips: Vec<&Clip> = p
        .playlist
        .clips
        .iter()
        .filter(|c| c.track.index() == r.track)
        .collect();
    let starts: Vec<f64> = clips.iter().map(|c| c.start).collect();
    assert_eq!(
        starts,
        vec![0.0, 4.0, 16.0, 20.0, 32.0, 36.0, 40.0, 48.0, 52.0, 64.0, 68.0],
        "{clips:#?}"
    );
    let names: Vec<&str> = clips
        .iter()
        .map(|c| p.pattern(&c.pattern).unwrap().name.as_str())
        .collect();
    assert_eq!(names[0], "Drums · Count-in");
    assert_eq!(names[1], "Drums · Straight 8ths A");
    assert!(
        names[2].starts_with("Drums · Straight 8ths A + turnaround"),
        "{names:?}"
    );
    assert!(
        names[4].starts_with("Drums · Straight 8ths A + 1-beat fill"),
        "{names:?}"
    );
    assert!(
        names[5].starts_with("Drums · Straight 8ths B + crash"),
        "{names:?}"
    );
    assert!(
        names[9].starts_with("Drums · Straight 8ths B + 1-bar fill"),
        "{names:?}"
    );
    assert_eq!(names[10], "Drums · Ending");
    // The plain groove is one pattern used twice.
    assert_eq!(clips[1].pattern, clips[3].pattern);
    assert_eq!(clips[1].length, 12.0);
    // One GM kit channel.
    let kits: Vec<&Channel> = p
        .channels
        .iter()
        .filter(|c| c.instrument.kind == "soundfont")
        .collect();
    assert_eq!(kits.len(), 1);
    assert_eq!(kits[0].instrument.option("program"), "Standard Kit");
    assert_eq!(p.playlist.tracks[r.track].name, "Drums");
    assert!(crate::validate::validate(&p)
        .iter()
        .all(|i| i.severity != crate::validate::Severity::Error));
}

#[test]
fn writing_again_is_deterministic() {
    let mut p = song("funk-16ths", "");
    write(&mut p).unwrap();
    let first = p.clone();
    write(&mut p).unwrap();
    assert_eq!(p, first);
    assert!(edited(&p).is_empty());
}

/// The written pattern playing `slot`.
fn written_id(p: &Project, slot: &str) -> String {
    p.drums
        .as_ref()
        .unwrap()
        .written
        .iter()
        .find(|(_, w)| w.slot == slot)
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| panic!("no pattern for {slot}"))
}

fn pattern_mut<'a>(p: &'a mut Project, id: &str) -> &'a mut Pattern {
    p.patterns.iter_mut().find(|x| x.id == id).unwrap()
}

#[test]
fn hand_edits_to_the_groove_are_kept_and_spread() {
    let mut p = song("rock-8ths", "");
    write(&mut p).unwrap();
    // The producer adds a cowbell on every beat of groove A, in the piano roll.
    let id = written_id(&p, "rock-8ths/a");
    let ch = p.pattern(&id).unwrap().notes[0].channel.clone();
    for beat in 0..4 {
        pattern_mut(&mut p, &id).notes.push(Note {
            channel: ch.clone(),
            pitch: 56,
            start: beat as f64,
            length: 0.25,
            velocity: 0.7,
        });
    }
    assert_eq!(edited(&p), vec![id.clone()]);
    let edited_notes = p.pattern(&id).unwrap().notes.clone();
    // Writing again (say with another feel) keeps it note for note…
    p.drums.as_mut().unwrap().feel = "tight".into();
    let r = write(&mut p).unwrap();
    assert!(r.kept >= 1);
    let id = written_id(&p, "rock-8ths/a");
    let mut now = p.pattern(&id).unwrap().notes.clone();
    let mut was = edited_notes;
    let key = |n: &Note| (n.start.to_bits(), n.pitch);
    now.sort_by_key(key);
    was.sort_by_key(key);
    assert_eq!(now, was);
    // …and the groove's other bars (crash, turnaround, fill) have the cowbell too.
    let derived: Vec<(String, String)> = p
        .drums
        .as_ref()
        .unwrap()
        .written
        .iter()
        .filter(|(_, w)| w.slot.starts_with("rock-8ths/a+"))
        .map(|(id, w)| (id.clone(), w.slot.clone()))
        .collect();
    assert!(derived.len() >= 2, "{derived:?}");
    for (id, slot) in derived {
        let bells = p
            .pattern(&id)
            .unwrap()
            .notes
            .iter()
            .filter(|n| n.pitch == 56)
            .count();
        assert!(bells >= 3, "{slot}: {bells} cowbells");
    }
    let edit = &p.drums.as_ref().unwrap().grooves["rock-8ths"];
    assert!(edit.a.iter().any(|r| r[0] == "cowbell"), "{edit:?}");
    // Nothing is edited any more until the producer edits again.
    assert!(edited(&p).is_empty());
}

#[test]
fn a_kept_fill_follows_a_kit_change() {
    let mut p = song("rock-8ths", "");
    write(&mut p).unwrap();
    let slot = p
        .drums
        .as_ref()
        .unwrap()
        .written
        .values()
        .find(|w| w.slot.contains("fill"))
        .unwrap()
        .slot
        .clone();
    let id = written_id(&p, &slot);
    for n in &mut pattern_mut(&mut p, &id).notes {
        n.velocity = 0.5;
    }
    write(&mut p).unwrap();
    assert!(p.drums.as_ref().unwrap().kept.contains_key(&slot));
    // On the drum machine the kept fill plays on its channels, velocities kept.
    p.drums.as_mut().unwrap().kit = "Ebony".into();
    write(&mut p).unwrap();
    let id = written_id(&p, &slot);
    let pat = p.pattern(&id).unwrap();
    assert!(pat.notes.iter().all(|n| n.velocity == 0.5));
    assert!(pat
        .notes
        .iter()
        .all(|n| p.channel(&n.channel).unwrap().instrument.kind == "drum"));
    // Reset: forgetting the kept pattern gives it back to the drummer.
    p.drums.as_mut().unwrap().kept.remove(&slot);
    write(&mut p).unwrap();
    let id = written_id(&p, &slot);
    assert!(p
        .pattern(&id)
        .unwrap()
        .notes
        .iter()
        .any(|n| n.velocity != 0.5));
}

#[test]
fn changing_the_part_rewrites_it() {
    let mut p = song("rock-8ths", "");
    write(&mut p).unwrap();
    let n = p.patterns.len();
    p.drums.as_mut().unwrap().sections[1].play = "b".into();
    write(&mut p).unwrap();
    assert!(p.patterns.len() <= n);
    assert!(p
        .patterns
        .iter()
        .all(|x| !x.name.contains(" A") || !x.id.starts_with("drums")));
}

#[test]
fn ebony_kit_makes_and_reuses_channels() {
    let mut p = song("house", "");
    write(&mut p).unwrap();
    let kinds: Vec<&str> = p
        .channels
        .iter()
        .map(|c| c.instrument.option("kind"))
        .collect();
    assert!(
        kinds.contains(&"kick") && kinds.contains(&"clap"),
        "{kinds:?}"
    );
    let crash = p.channels.iter().find(|c| c.name == "Crash").unwrap();
    assert_eq!(crash.instrument.params.get("decay"), Some(&3.0));
    let n = p.channels.len();
    write(&mut p).unwrap();
    assert_eq!(p.channels.len(), n);
}

#[test]
fn a_groove_in_another_meter_is_refused() {
    let mut p = song("waltz", "");
    let err = write(&mut p).unwrap_err();
    assert!(err.contains("3/4"), "{err}");
    p.transport.beats_per_bar = 3;
    write(&mut p).unwrap();
}

#[test]
fn fills_do_not_repeat_back_to_back() {
    let mut p = song("rock-8ths", "");
    let part = p.drums.as_mut().unwrap();
    part.sections = (0..6)
        .map(|i| sec(&format!("S{i}"), 2, "a", "beat", false))
        .collect();
    write(&mut p).unwrap();
    let mut fills: Vec<&str> = p
        .playlist
        .clips
        .iter()
        .map(|c| p.pattern(&c.pattern).unwrap().name.as_str())
        .filter(|n| n.contains("fill"))
        .collect();
    assert_eq!(fills.len(), 6);
    for w in fills.windows(2) {
        assert_ne!(w[0], w[1]);
    }
    fills.dedup();
    assert!(fills.len() > 1);
}

#[test]
fn feel_puts_the_backbeat_behind() {
    let mut p = song("rock-8ths", "");
    p.drums.as_mut().unwrap().feel = "loose".into();
    write(&mut p).unwrap();
    let plain = p
        .patterns
        .iter()
        .find(|x| x.name == "Drums · Straight 8ths A")
        .unwrap();
    let snares: Vec<f64> = plain
        .notes
        .iter()
        .filter(|n| n.pitch == 38)
        .map(|n| n.start)
        .collect();
    assert_eq!(snares.len(), 2);
    for (s, beat) in snares.iter().zip([1.0, 3.0]) {
        assert!(*s > beat && *s < beat + 0.05, "{s}");
    }
    p.drums.as_mut().unwrap().feel = "tight".into();
    write(&mut p).unwrap();
    let plain = p
        .patterns
        .iter()
        .find(|x| x.name == "Drums · Straight 8ths A")
        .unwrap();
    assert!(plain.notes.iter().all(|n| (n.start * 4.0).fract() == 0.0));
}

fn clip(p: &mut Project, id: &str, name: &str, track: u32, start: f64, beats: f64) {
    if p.pattern(id).is_none() {
        p.patterns.push(Pattern {
            id: id.into(),
            name: name.into(),
            color: "#ffffff".into(),
            length: 4.0,
            notes: vec![],
            drums: None,
        });
    }
    p.playlist.clips.push(Clip {
        pattern: id.into(),
        sample: String::new(),
        track: TrackIx(track),
        start,
        length: beats,
        offset: 0.0,
        gain: 1.0,
        mixer: Default::default(),
    });
}

#[test]
fn guesses_sections_from_the_playlist() {
    let mut p = Project::empty("Song");
    // Bar 2: an 8-bar verse (bass), then an 8-bar chorus (bass and keys).
    clip(&mut p, "bv", "Bass · Verse", 0, 4.0, 32.0);
    clip(&mut p, "bc", "Bass · Chorus", 0, 36.0, 32.0);
    clip(&mut p, "kc", "Keys · Chorus", 1, 36.0, 32.0);
    let (start, s) = guess_sections(&p);
    assert_eq!(start, 2);
    assert_eq!(s.len(), 2);
    assert_eq!(
        (
            s[0].name.as_str(),
            s[0].bars,
            s[0].play.as_str(),
            s[0].fill.as_str()
        ),
        ("Verse", 8, "a", "beat")
    );
    assert_eq!(
        (
            s[1].name.as_str(),
            s[1].bars,
            s[1].play.as_str(),
            s[1].crash
        ),
        ("Chorus", 8, "b", true)
    );
}

#[test]
fn same_named_neighbours_join() {
    let mut p = Project::empty("Song");
    // The verse's keys come in halfway: still the verse.
    clip(&mut p, "bv", "Bass · Verse", 0, 0.0, 32.0);
    clip(&mut p, "kv", "Keys · Verse", 1, 16.0, 16.0);
    let (_, s) = guess_sections(&p);
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].name.as_str(), s[0].bars), ("Verse", 8));
}

#[test]
fn a_kit_change_removes_the_channels_left_unused() {
    let mut p = song("rock-8ths", "Ebony");
    write(&mut p).unwrap();
    let ebony = p
        .channels
        .iter()
        .filter(|c| c.instrument.kind == "drum")
        .count();
    assert!(ebony >= 4);
    p.drums.as_mut().unwrap().kit = "Standard Kit".into();
    write(&mut p).unwrap();
    assert_eq!(
        p.channels
            .iter()
            .filter(|c| c.instrument.kind == "drum")
            .count(),
        0
    );
    assert_eq!(p.channels.len(), 1);
    // A drum channel that something else plays is kept.
    let mut p = song("rock-8ths", "Ebony");
    write(&mut p).unwrap();
    let kick = p
        .channels
        .iter()
        .find(|c| c.name == "Kick")
        .unwrap()
        .id
        .clone();
    p.patterns.push(Pattern {
        id: "mine".into(),
        name: "Mine".into(),
        color: "#ffffff".into(),
        length: 4.0,
        notes: vec![Note {
            channel: kick.clone(),
            pitch: 60,
            start: 0.0,
            length: 0.25,
            velocity: 0.8,
        }],
        drums: None,
    });
    p.drums.as_mut().unwrap().kit = "Standard Kit".into();
    write(&mut p).unwrap();
    assert!(p.channel(&kick).is_some());
}

#[test]
fn round_trips_through_json() {
    let mut p = song("jazz-swing", "Jazz Kit");
    write(&mut p).unwrap();
    let text = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p);
}

#[test]
fn validation_names_bad_values() {
    let mut p = song("rock-8ths", "");
    {
        let d = p.drums.as_mut().unwrap();
        d.groove = "polka".into();
        d.kit = "Bongos".into();
        d.sections[1].play = "c".into();
        d.sections[2].bars = 0;
    }
    let paths: Vec<String> = crate::validate::validate(&p)
        .into_iter()
        .filter(|i| i.severity == crate::validate::Severity::Error)
        .map(|i| i.path)
        .collect();
    for want in [
        "drums.groove",
        "drums.kit",
        "drums.sections[1].play",
        "drums.sections[2].bars",
    ] {
        assert!(paths.iter().any(|p| p == want), "{want} not in {paths:?}");
    }
}

#[test]
fn written_ids_are_valid() {
    let mut p = song("shuffle-half", "");
    write(&mut p).unwrap();
    for pat in &p.patterns {
        assert!(
            pat.id.len() <= 64
                && pat
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')),
            "{}",
            pat.id
        );
    }
}

#[test]
fn short_changes_join_a_section() {
    let mut p = Project::empty("Song");
    // 8 bars, 1 bar, 7 bars, 2 bars: each short one joins the one before.
    for (i, (id, start, bars)) in [
        ("a", 0.0, 8.0),
        ("b", 32.0, 1.0),
        ("c", 36.0, 7.0),
        ("d", 64.0, 2.0),
    ]
    .iter()
    .enumerate()
    {
        p.patterns.push(Pattern {
            id: (*id).into(),
            name: (*id).into(),
            color: "#fff".into(),
            length: 4.0,
            notes: vec![],
            drums: None,
        });
        p.playlist.clips.push(Clip {
            pattern: (*id).into(),
            sample: String::new(),
            track: TrackIx(i as u32 % 2),
            start: *start,
            length: bars * 4.0,
            offset: 0.0,
            gain: 1.0,
            mixer: Default::default(),
        });
    }
    let (_, s) = guess_sections(&p);
    let bars: Vec<u32> = s.iter().map(|x| x.bars).collect();
    assert_eq!(bars, vec![9, 9]);
}

#[test]
fn reset_edits_replaces_the_written_drums() {
    let mut p = song("rock-8ths", "");
    write(&mut p).unwrap();
    let patterns = p.patterns.len();
    let clips = p.playlist.clips.len();
    let id = written_id(&p, "rock-8ths/a");
    let ch = p.pattern(&id).unwrap().notes[0].channel.clone();
    pattern_mut(&mut p, &id).notes.push(Note {
        channel: ch,
        pitch: 56,
        start: 0.0,
        length: 0.25,
        velocity: 0.7,
    });
    write(&mut p).unwrap();
    assert!(!p.drums.as_ref().unwrap().kept.is_empty());
    // Forgetting the edits writes the drummer's patterns in place of the old
    // ones: none kept, none twice.
    reset_edits(&mut p);
    let r = write(&mut p).unwrap();
    assert_eq!(r.kept, 0);
    assert!(p.drums.as_ref().unwrap().kept.is_empty());
    assert_eq!(
        (p.patterns.len(), p.playlist.clips.len()),
        (patterns, clips)
    );
    let id = written_id(&p, "rock-8ths/a");
    assert!(p.pattern(&id).unwrap().notes.iter().all(|n| n.pitch != 56));
}

#[test]
fn the_songs_own_kit_keeps_its_sound() {
    // The producer's own drums, on a Brush Kit.
    let mut p = song("rock-8ths", "Standard Kit");
    let mut dev = crate::model::Device::new("soundfont");
    dev.options.insert("program".into(), "Brush Kit".into());
    let mine = push_channel(&mut p, "My kit", dev);
    write(&mut p).unwrap();
    let program = |p: &Project, id: &str| {
        p.channel(id)
            .unwrap()
            .instrument
            .option("program")
            .to_string()
    };
    assert_eq!(program(&p, &mine), "Brush Kit");
    // The drummer plays a channel of its own on the Standard Kit…
    let ours = p.pattern(&written_id(&p, "rock-8ths/a")).unwrap().notes[0]
        .channel
        .clone();
    assert_ne!(ours, mine);
    assert_eq!(program(&p, &ours), "Standard Kit");
    // …and a kit change switches that one over, not the producer's.
    p.drums.as_mut().unwrap().kit = "Jazz Kit".into();
    write(&mut p).unwrap();
    assert_eq!(program(&p, &ours), "Jazz Kit");
    assert_eq!(program(&p, &mine), "Brush Kit");
}

#[test]
fn a_kit_change_keeps_channels_the_score_names() {
    let mut p = song("rock-8ths", "Standard Kit");
    write(&mut p).unwrap();
    let ours = p.pattern(&written_id(&p, "rock-8ths/a")).unwrap().notes[0]
        .channel
        .clone();
    p.score.marks.push(crate::model::ScoreMark {
        start: 0.0,
        end: 4.0,
        color: "#d4af37".into(),
        label: String::new(),
        pattern: String::new(),
        channels: vec![ours.clone()],
    });
    p.drums.as_mut().unwrap().kit = "Ebony".into();
    write(&mut p).unwrap();
    assert!(p.channel(&ours).is_some());
    assert!(crate::validate::validate(&p)
        .iter()
        .all(|i| i.severity != crate::validate::Severity::Error));
}

fn recipe(groove: &str, kit: &str) -> PatternDrums {
    PatternDrums {
        groove: groove.into(),
        play: "a".into(),
        fill: "none".into(),
        crash: false,
        turnaround: false,
        kit: kit.into(),
        feel: "tight".into(),
        swing: 0.0,
        seed: 1,
        edited: false,
    }
}

/// A song with one empty drum pattern of `beats` beats made from `r`.
fn drum_pattern(r: PatternDrums, beats: f64) -> Project {
    let mut p = Project::empty("Drums");
    p.patterns.push(Pattern {
        id: "drums-verse".into(),
        name: "Drums · Verse".into(),
        color: "#8e3b46".into(),
        length: beats,
        notes: vec![],
        drums: Some(r),
    });
    p
}

#[test]
fn a_pattern_is_made_from_its_recipe() {
    let mut p = drum_pattern(recipe("rock-8ths", "Standard Kit"), 16.0);
    render_pattern(&mut p, "drums-verse").unwrap();
    let pat = p.pattern("drums-verse").unwrap();
    assert_eq!(pat.length, 16.0);
    assert!(!pat.notes.is_empty());
    // Four bars of the groove: each bar has a kick on its downbeat.
    for bar in 0..4 {
        let t = bar as f64 * 4.0;
        assert!(
            pat.notes
                .iter()
                .any(|n| n.pitch == 36 && (n.start - t).abs() < 1e-6),
            "no kick on bar {}",
            bar + 1
        );
    }
    let kit = p
        .channels
        .iter()
        .find(|c| c.instrument.option("program") == "Standard Kit");
    assert!(kit.is_some(), "the kit channel is made");
    assert!(pat.notes.iter().all(|n| n.channel == kit.unwrap().id));
    // The same recipe makes the same notes.
    let mut q = p.clone();
    render_pattern(&mut q, "drums-verse").unwrap();
    assert_eq!(p, q);
}

#[test]
fn a_pattern_rounds_to_whole_bars_and_takes_a_crash_and_fill() {
    let mut r = recipe("rock-8ths", "Standard Kit");
    r.crash = true;
    r.fill = "bar".into();
    r.edited = true;
    let mut p = drum_pattern(r, 7.0);
    render_pattern(&mut p, "drums-verse").unwrap();
    let pat = p.pattern("drums-verse").unwrap();
    assert_eq!(pat.length, 8.0, "7 beats of 4/4 round to 2 bars");
    assert!(
        pat.notes.iter().any(|n| n.pitch == 49 && n.start < 1e-6),
        "the crash"
    );
    assert!(
        !pat.drums.as_ref().unwrap().edited,
        "made again: no hand edits"
    );
    let plain = {
        let mut q = drum_pattern(recipe("rock-8ths", "Standard Kit"), 8.0);
        render_pattern(&mut q, "drums-verse").unwrap();
        q.pattern("drums-verse").unwrap().notes.clone()
    };
    let last_bar = |notes: &[Note]| -> Vec<(i32, i64)> {
        notes
            .iter()
            .filter(|n| n.start >= 4.0)
            .map(|n| (n.pitch, (n.start * 1000.0).round() as i64))
            .collect()
    };
    assert_ne!(
        last_bar(&pat.notes),
        last_bar(&plain),
        "the last bar is a fill"
    );
}

#[test]
fn a_pattern_kit_change_drops_the_old_channels() {
    let mut p = drum_pattern(recipe("rock-8ths", EBONY), 4.0);
    render_pattern(&mut p, "drums-verse").unwrap();
    assert!(
        p.channels
            .iter()
            .filter(|c| c.instrument.kind == "drum")
            .count()
            >= 3
    );
    pattern_mut(&mut p, "drums-verse")
        .drums
        .as_mut()
        .unwrap()
        .kit = "Jazz Kit".into();
    render_pattern(&mut p, "drums-verse").unwrap();
    assert_eq!(p.channels.len(), 1, "only the Jazz Kit channel is left");
    assert_eq!(p.channels[0].instrument.option("program"), "Jazz Kit");
}

#[test]
fn a_pattern_api_validates_and_refuses_what_it_cannot_make() {
    let p = drum_pattern(recipe("rock-8ths", ""), 4.0);
    let v = api_pattern(&serde_json::to_string(&p).unwrap(), "drums-verse").unwrap();
    let out: Project = serde_json::from_value(v["project"].clone()).unwrap();
    assert!(!out.pattern("drums-verse").unwrap().notes.is_empty());
    assert!(api_pattern(&serde_json::to_string(&p).unwrap(), "nope").is_err());
    let mut bad = p.clone();
    pattern_mut(&mut bad, "drums-verse")
        .drums
        .as_mut()
        .unwrap()
        .groove = "no-such-groove".into();
    let e = api_pattern(&serde_json::to_string(&bad).unwrap(), "drums-verse").unwrap_err();
    assert!(e.contains("patterns[1].drums.groove"), "{e}");
    let mut plain = p.clone();
    pattern_mut(&mut plain, "drums-verse").drums = None;
    assert!(api_pattern(&serde_json::to_string(&plain).unwrap(), "drums-verse").is_err());
}
