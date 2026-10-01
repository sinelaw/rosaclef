use super::*;
use crate::line::Song;
use crate::testing::project;

/// The verse pattern of the test song (verse 1: "Hel-lo dark friend _ /
/// (br) Hi") recorded from second 10: a beat is half a second.
fn target() -> Target {
    Target {
        pattern: "verse".into(),
        channel: "lead".into(),
        verse: 1,
        at: 10.0,
    }
}

/// A long-format TextGrid with a words and a phones tier.
fn grid(words: &[(f64, f64, &str)], phones: &[(f64, f64, &str)]) -> String {
    let tier = |name: &str, list: &[(f64, f64, &str)]| {
        let mut s = format!(
            "    item [1]:\n        class = \"IntervalTier\"\n        name = \"{name}\"\n        \
             intervals: size = {}\n",
            list.len()
        );
        for (k, (a, b, t)) in list.iter().enumerate() {
            s += &format!(
                "        intervals [{}]:\n            xmin = {a}\n            xmax = {b}\n            \
                 text = \"{t}\"\n",
                k + 1
            );
        }
        s
    };
    format!(
        "File type = \"ooTextFile\"\nObject class = \"TextGrid\"\nitem []:\n{}{}",
        tier("words", words),
        tier("phones", phones)
    )
}

fn sung_take() -> String {
    grid(
        &[
            (9.95, 10.9, "hello"),
            (10.95, 11.45, "dark"),
            (11.45, 12.0, "friend"),
            (12.0, 12.3, "oh"),
            (12.45, 12.9, "hi"),
        ],
        &[
            (9.95, 10.02, "HH"),
            (10.02, 10.42, "AH0"),
            (10.42, 10.5, "L"),
            (10.5, 10.9, "OW1"),
            (10.95, 11.0, "D"),
            (11.0, 11.2, "AA1"),
            (11.2, 11.35, "R"),
            (11.35, 11.45, "K"),
            (11.45, 11.48, "F"),
            (11.48, 11.5, "R"),
            (11.5, 11.8, "EH1"),
            (11.8, 11.9, "N"),
            (11.9, 12.0, "D"),
            (12.45, 12.5, "HH"),
            (12.5, 12.9, "AY1"),
        ],
    )
}

/// (beat, "phoneme offset ...") of each timed syllable.
fn shown(timing: &[SyllableTiming]) -> Vec<(f64, String)> {
    timing
        .iter()
        .map(|t| {
            let ph: Vec<String> = t
                .phonemes
                .iter()
                .map(|p| format!("{} {}", p.p, p.offset))
                .collect();
            (t.at, ph.join(", "))
        })
        .collect()
}

#[test]
fn phones_become_offsets_from_each_note() {
    let aligned = read("take.TextGrid", &sung_take()).unwrap();
    let t = timing(&project(), &target(), &aligned).unwrap();
    assert_eq!(
        shown(&t),
        [
            (0.0, "h -0.05, ʌ 0.02".to_string()),
            (1.0, "l -0.08, oʊ 0".to_string()),
            (2.0, "d -0.05, ɑ 0, ɹ 0.2, k 0.35".to_string()),
            (3.0, "f -0.05, ɹ -0.02, ɛ 0, n 0.3, d 0.4".to_string()),
            (5.0, "h -0.05, aɪ 0".to_string()),
        ],
        "\"oh\" is not in the lyrics; the hook's words are another line"
    );
    assert!(t.iter().all(|s| s.verse == 1));
}

#[test]
fn apply_replaces_the_line_timing() {
    let aligned = read("take.textgrid", &sung_take()).unwrap();
    let (once, n) = apply(&project(), &target(), &aligned).unwrap();
    assert_eq!(n, 5);
    let (twice, _) = apply(&once, &target(), &aligned).unwrap();
    assert_eq!(twice.patterns[0].lyrics[0].timing.len(), 5);
    let song = Song::of_project(&twice);
    let hel = song.lines[0].notes[0].syllable.as_ref().unwrap();
    assert_eq!(hel.timing.as_ref().unwrap().phonemes[0].offset, -0.05);
    let wrong = Target {
        channel: "kit".into(),
        ..target()
    };
    assert!(apply(&project(), &wrong, &aligned).is_err());
}

#[test]
fn words_and_letters_without_phones() {
    let json = r#"{"segments": [{"words": [
            {"word": "Hello", "start": 9.95, "end": 10.9},
            {"word": "dark", "start": 10.95, "end": 11.45}],
        "chars": [{"char": "H", "start": 9.95, "end": 10.0}, {"char": "e", "start": 10.0, "end": 10.3},
                  {"char": "l", "start": 10.3, "end": 10.45}, {"char": "l", "start": 10.45, "end": 10.5},
                  {"char": "o", "start": 10.5, "end": 10.9}]}]}"#;
    let aligned = read("take.json", json).unwrap();
    let t = timing(&project(), &target(), &aligned).unwrap();
    assert_eq!(
        shown(&t),
        [
            (0.0, "h -0.05, ə 0.03".to_string()),
            (1.0, "l -0.05, oʊ 0.03".to_string()),
            (2.0, "d -0.05, ɑ 0.03, ɹ 0.29, k 0.37".to_string()),
        ]
    );
    assert!(read("take.wav", "").is_err());
}
