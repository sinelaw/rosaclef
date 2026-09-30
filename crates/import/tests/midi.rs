//! MIDI import tests with files written byte by byte.

use rosaclef_core::validate;
use rosaclef_import::midi::{self, Options};
use rosaclef_import::Imported;

fn vlq(mut v: u32) -> Vec<u8> {
    let mut out = vec![(v & 0x7f) as u8];
    v >>= 7;
    while v > 0 {
        out.insert(0, 0x80 | (v & 0x7f) as u8);
        v >>= 7;
    }
    out
}

/// Builds one track chunk from (delta, raw event bytes) pairs.
struct Trk(Vec<u8>);

impl Trk {
    fn new() -> Trk {
        Trk(vec![])
    }
    fn ev(mut self, delta: u32, bytes: &[u8]) -> Trk {
        self.0.extend(vlq(delta));
        self.0.extend_from_slice(bytes);
        self
    }
    fn meta(self, delta: u32, kind: u8, data: &[u8]) -> Trk {
        let mut b = vec![0xff, kind];
        b.extend(vlq(data.len() as u32));
        b.extend_from_slice(data);
        self.ev(delta, &b)
    }
    fn name(self, n: &str) -> Trk {
        self.meta(0, 0x03, n.as_bytes())
    }
    fn tempo(self, delta: u32, bpm: f64) -> Trk {
        let us = (60_000_000.0 / bpm).round() as u32;
        self.meta(delta, 0x51, &[(us >> 16) as u8, (us >> 8) as u8, us as u8])
    }
    fn end(self) -> Vec<u8> {
        let t = self.meta(0, 0x2f, &[]);
        let mut out = b"MTrk".to_vec();
        out.extend((t.0.len() as u32).to_be_bytes());
        out.extend(t.0);
        out
    }
}

fn smf(format: u16, ppq: u16, tracks: Vec<Vec<u8>>) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(format.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend(ppq.to_be_bytes());
    for t in tracks {
        out.extend(t);
    }
    out
}

fn check_valid(im: &Imported) {
    let issues = validate::validate(&im.project);
    assert!(
        issues
            .iter()
            .all(|i| i.severity != validate::Severity::Error),
        "{issues:?}"
    );
}

fn warned(im: &Imported, needle: &str) -> bool {
    im.warnings.iter().any(|w| w.contains(needle))
}

/// Format 0, 96 PPQ: a bass (program 33) with running status and
/// overlapping same-key notes, and drums on channel 10.
fn format0() -> Vec<u8> {
    let t = Trk::new()
        .name("Groove")
        .tempo(0, 100.0)
        .meta(0, 0x58, &[4, 2, 24, 8])
        .ev(0, &[0xC0, 33])
        .ev(0, &[0xB0, 7, 100])
        // Bass: note on 40 (vel 100), running status for the rest.
        .ev(0, &[0x90, 40, 100])
        .ev(48, &[43, 64]) // running status: note on 43 at tick 48
        .ev(48, &[40, 0]) //   note on vel 0 = note off 40 at tick 96
        .ev(0, &[43, 0]) //    off 43 at 96
        // Overlapping notes on key 60: on@96, on@120, off@144, off@192 (FIFO).
        .ev(0, &[0x90, 60, 90])
        .ev(24, &[60, 70])
        .ev(24, &[0x80, 60, 0])
        .ev(48, &[0x80, 60, 0])
        // Drums (channel 10, from tick 192 = beat 2): kick, closed hat, crash; snare at beat 3.
        .ev(0, &[0x99, 36, 127])
        .ev(0, &[42, 80])
        .ev(0, &[49, 100])
        .ev(10, &[36, 0])
        .ev(0, &[42, 0])
        .ev(0, &[49, 0])
        .ev(86, &[0x99, 38, 110])
        .ev(10, &[0x89, 38, 0])
        // An unmapped drum key.
        .ev(0, &[0x99, 20, 100])
        .ev(10, &[0x89, 20, 0])
        .end();
    smf(0, 96, vec![t])
}

#[test]
fn imports_format0() {
    let im = midi::import(&format0(), &Options::new("Groove")).unwrap();
    check_valid(&im);
    let p = &im.project;
    assert_eq!(p.transport.bpm, 100.0);
    assert_eq!(p.transport.beats_per_bar, 4);
    let names: Vec<&str> = p.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Groove", "Kick", "Hi-Hat", "Crash", "Snare"]);

    // Bass: GM program 33 → a bass instrument.
    let bass = &p.channels[0];
    assert!(
        ["synth", "cuivre"].contains(&bass.instrument.kind.as_str()),
        "{}",
        bass.instrument.kind
    );
    assert!((bass.volume - 0.8).abs() < 1e-9);
    let notes: Vec<(i32, f64, f64, f64)> = p
        .patterns
        .iter()
        .flat_map(|x| &x.notes)
        .filter(|n| n.channel == bass.id)
        .map(|n| (n.pitch, n.start, n.length, n.velocity))
        .collect();
    let v = |x: u8| x as f64 / 127.0;
    assert_eq!(
        notes,
        [
            (40, 0.0, 1.0, v(100)),
            (43, 0.5, 0.5, v(64)),
            (60, 1.0, 0.5, v(90)),
            (60, 1.25, 0.75, v(70))
        ]
    );

    // Drums: one channel per group, pitch 60, own inserts.
    for (i, kind) in [(1, "kick"), (2, "hat"), (3, "openhat"), (4, "snare")] {
        let ch = &p.channels[i];
        assert_eq!(ch.instrument.kind, "drum");
        assert_eq!(ch.instrument.option("kind"), kind);
        assert_eq!(ch.mixer.0 as usize, i + 1);
        assert_eq!(p.mixer.inserts[i + 1].name, ch.name);
        let ns: Vec<(i32, f64)> = p
            .patterns
            .iter()
            .flat_map(|x| &x.notes)
            .filter(|n| n.channel == ch.id)
            .map(|n| (n.pitch, n.start))
            .collect();
        let at = if kind == "snare" { 3.0 } else { 2.0 };
        assert_eq!(ns, [(60, at)]);
    }
    assert!(
        p.channels[3].instrument.param("decay") > 2.0,
        "cymbals ring longer"
    );
    assert!(warned(&im, "Crash"));
    assert!(warned(&im, "outside the General MIDI drum map"));

    // One track per channel, one clip each (a single 4-bar block).
    assert_eq!(&p.playlist.tracks[0].name, "Groove");
    assert_eq!(p.playlist.clips.len(), 5);
    assert!(p
        .playlist
        .clips
        .iter()
        .all(|c| c.start == 0.0 && c.length == 16.0));
    assert!(p.patterns.iter().all(|x| x.length == 16.0));
}

/// Format 1: a conductor track with a tempo change, a piano whose first
/// 8 bars repeat a 4-bar phrase, then a different phrase; and a drum track.
fn format1() -> Vec<u8> {
    let ppq = 480u32;
    let conductor = Trk::new()
        .name("My Song")
        .tempo(0, 120.0)
        .meta(0, 0x58, &[4, 2, 24, 8])
        .tempo(ppq * 16, 140.0)
        .end();
    let mut piano = Trk::new().name("Piano").ev(0, &[0xC0, 0]);
    // 12 bars: bars 1-4 and 5-8 identical (C E G per bar), bars 9-12 one long note.
    let mut last = 0u32;
    for bar in 0..8u32 {
        for (i, key) in [60u8, 64, 67].iter().enumerate() {
            let at = bar * 4 * ppq + i as u32 * ppq;
            piano = piano
                .ev(at - last, &[0x90, *key, 96])
                .ev(ppq / 2, &[0x80, *key, 64]);
            last = at + ppq / 2;
        }
    }
    let at = 8 * 4 * ppq;
    piano = piano
        .ev(at - last, &[0x90, 48, 80])
        .ev(ppq * 8, &[0x80, 48, 0]);
    let drums = Trk::new()
        .name("Drums")
        .ev(0, &[0x99, 36, 120])
        .ev(ppq / 4, &[0x89, 36, 0])
        .ev(ppq * 4 - ppq / 4, &[0x99, 45, 100])
        .ev(ppq / 4, &[0x89, 45, 0])
        .end();
    smf(1, ppq as u16, vec![conductor, piano.end(), drums])
}

#[test]
fn imports_format1_with_dedupe() {
    let im = midi::import(&format1(), &Options::new("song-file")).unwrap();
    assert_eq!(
        im.project.meta.title, "My Song",
        "the conductor track names the song"
    );
    check_valid(&im);
    let p = &im.project;
    assert_eq!(p.transport.bpm, 120.0);
    assert!(warned(&im, "tempo change"), "{:?}", im.warnings);
    let names: Vec<&str> = p.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Piano", "Kick", "Toms"]);

    let piano = &p.channels[0];
    let pats: Vec<&rosaclef_core::Pattern> = p
        .patterns
        .iter()
        .filter(|x| x.notes.iter().any(|n| n.channel == piano.id))
        .collect();
    let pnames: Vec<&str> = pats.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(
        pnames,
        ["Piano A", "Piano B"],
        "bars 1-4 and 5-8 share one pattern"
    );
    assert_eq!(pats[0].length, 16.0);
    assert_eq!(pats[0].notes.len(), 12);
    assert_eq!(
        (
            pats[0].notes[0].pitch,
            pats[0].notes[0].start,
            pats[0].notes[0].length
        ),
        (60, 0.0, 0.5)
    );
    assert_eq!((pats[0].notes[4].pitch, pats[0].notes[4].start), (64, 5.0));
    assert_eq!((pats[1].notes[0].pitch, pats[1].notes[0].length), (48, 8.0));
    let clips: Vec<(f64, f64, &str)> = p
        .playlist
        .clips
        .iter()
        .filter(|c| c.track.0 == 0)
        .map(|c| (c.start, c.length, c.pattern.as_str()))
        .collect();
    assert_eq!(
        clips,
        [
            (0.0, 32.0, pats[0].id.as_str()),
            (32.0, 16.0, pats[1].id.as_str())
        ],
        "repeated blocks become one looping clip"
    );

    // Toms keep their relative tuning (low-mid tom 45 → 60).
    let toms = &p.channels[2];
    assert_eq!(toms.instrument.option("kind"), "tom");
    let n = p
        .patterns
        .iter()
        .flat_map(|x| &x.notes)
        .find(|n| n.channel == toms.id)
        .unwrap();
    assert_eq!((n.pitch, n.start), (60, 4.0));
}

#[test]
fn merges_into_an_existing_project() {
    let im = midi::import(&format0(), &Options::new("Groove")).unwrap();
    let mut base = rosaclef_core::Project::empty("Song");
    base.channels.push(rosaclef_core::Channel {
        id: "kick".into(),
        name: "Kick".into(),
        color: "#ffffff".into(),
        instrument: rosaclef_core::Device::new("drum"),
        volume: 0.8,
        pan: 0.0,
        mute: false,
        mixer: rosaclef_core::InsertIx(1),
    });
    let tracks0 = base.playlist.tracks.len();
    let inserts0 = base.mixer.inserts.len();
    let warnings = rosaclef_import::merge_into(&mut base, im.project);
    assert!(warnings.iter().any(|w| w.contains("tempo")));
    let issues = validate::validate(&base);
    assert!(
        issues
            .iter()
            .all(|i| i.severity != validate::Severity::Error),
        "{issues:?}"
    );
    assert_eq!(base.channels.len(), 6);
    assert_eq!(base.channels[2].id, "kick-2", "colliding ids are renamed");
    assert_eq!(
        base.playlist.tracks.len(),
        tracks0 + 5,
        "only tracks with clips are appended"
    );
    assert_eq!(base.mixer.inserts.len(), inserts0 + 5);
    assert_eq!(base.channels[1].mixer.0 as usize, inserts0);
    assert!(base
        .playlist
        .clips
        .iter()
        .all(|c| c.track.0 as usize >= tracks0));
}

#[test]
fn rejects_garbage_and_reports_truncation() {
    assert!(midi::import(b"not a midi file", &Options::new("x")).is_err());
    let mut bytes = format0();
    bytes.truncate(bytes.len() - 20);
    let im = midi::import(&bytes, &Options::new("x")).unwrap();
    check_valid(&im);
    assert!(warned(&im, "truncated"), "{:?}", im.warnings);
}
