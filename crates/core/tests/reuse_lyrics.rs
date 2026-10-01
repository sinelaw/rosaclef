//! Validation of patterns used by reference and of lyrics, from JSON as an
//! agent would write it.

use rosaclef_core::validate::{parse_and_validate, Severity};
use serde_json::{json, Value};

fn song(patterns: Value, clips: Value) -> String {
    json!({
        "format": "rosaclef/1",
        "meta": {"title": "t"},
        "transport": {"bpm": 120},
        "channels": [
            {"id": "lead", "name": "Lead", "instrument": {"type": "synth"}},
            {"id": "bass", "name": "Bass", "instrument": {"type": "synth"}}
        ],
        "patterns": patterns,
        "playlist": {"tracks": [{"name": "T"}], "clips": clips},
        "mixer": {"inserts": [{"name": "Master"}]}
    })
    .to_string()
}

fn notes(n: usize) -> Value {
    (0..n)
        .map(|i| json!({"channel": "lead", "pitch": 60, "start": i, "length": 1}))
        .collect()
}

/// (severity, path) of every issue.
fn issues(text: &str) -> Vec<(Severity, String)> {
    parse_and_validate(text)
        .issues
        .into_iter()
        .map(|i| (i.severity, i.path))
        .collect()
}

#[test]
fn a_well_formed_song_with_uses_and_verses_is_clean() {
    let text = song(
        json!([
            {"id": "hook", "name": "Hook", "length": 2, "notes": notes(2),
             "lyrics": [{"channel": "lead", "verses": {"1": "oh yeah"}}]},
            {"id": "verse", "name": "Verse", "length": 8, "notes": notes(2),
             "uses": [{"pattern": "hook", "start": 4, "transpose": 2, "channel": "bass"}],
             "lyrics": [{"channel": "lead", "lang": "en-US",
                         "verses": {"1": "Hel-lo", "2": "Good-bye"}}]}
        ]),
        json!([{"pattern": "verse", "track": 0, "start": 0, "length": 8, "verse": 2}]),
    );
    assert_eq!(issues(&text), vec![]);
}

#[test]
fn uses_are_checked() {
    let text = song(
        json!([
            {"id": "a", "name": "A", "length": 4, "uses": [
                {"pattern": "b", "start": 0},
                {"pattern": "nope", "start": 0},
                {"pattern": "b", "start": 0, "from": 2, "to": 9, "channel": "x", "verse": 0}
            ]},
            {"id": "b", "name": "B", "length": 4, "uses": [{"pattern": "a", "start": 0}]}
        ]),
        json!([]),
    );
    let paths: Vec<String> = issues(&text).into_iter().map(|(_, p)| p).collect();
    for want in [
        "patterns[0].uses[0].pattern",
        "patterns[0].uses[1].pattern",
        "patterns[0].uses[2].to",
        "patterns[0].uses[2].channel",
        "patterns[0].uses[2].verse",
        "patterns[1].uses[0].pattern",
    ] {
        assert!(paths.iter().any(|p| p == want), "{want} in {paths:?}");
    }
}

#[test]
fn lyrics_are_checked() {
    let text = song(
        json!([
            {"id": "v", "name": "V", "length": 4, "notes": notes(3), "lyrics": [
                {"channel": "lead", "lang": "english!", "verses": {
                    "1": "one two three four", "2": "one", "3": "bad [", "0": "x"}},
                {"channel": "nobody", "verses": {"1": "x"}}
            ]}
        ]),
        json!([]),
    );
    let got = issues(&text);
    let has = |sev: Severity, path: &str| got.iter().any(|(s, p)| *s == sev && p == path);
    assert!(
        has(Severity::Error, "patterns[0].lyrics[0].lang"),
        "{got:?}"
    );
    assert!(
        has(Severity::Warning, "patterns[0].lyrics[0].verses.1"),
        "{got:?}"
    );
    assert!(
        has(Severity::Warning, "patterns[0].lyrics[0].verses.2"),
        "{got:?}"
    );
    assert!(
        has(Severity::Error, "patterns[0].lyrics[0].verses.3"),
        "{got:?}"
    );
    assert!(
        has(Severity::Error, "patterns[0].lyrics[0].verses.0"),
        "{got:?}"
    );
    assert!(
        has(Severity::Error, "patterns[0].lyrics[1].channel"),
        "{got:?}"
    );
}

#[test]
fn a_sung_channel_holds_one_note_at_a_time() {
    let text = song(
        json!([{"id": "v", "name": "V", "length": 4,
            "notes": [
                {"channel": "lead", "pitch": 60, "start": 0, "length": 2},
                {"channel": "lead", "pitch": 64, "start": 1, "length": 1}
            ],
            "lyrics": [{"channel": "lead", "verses": {"1": "a b"}}]}]),
        json!([]),
    );
    assert_eq!(
        issues(&text),
        vec![(
            Severity::Warning,
            "patterns[0].lyrics[0].channel".to_string()
        )]
    );
}
