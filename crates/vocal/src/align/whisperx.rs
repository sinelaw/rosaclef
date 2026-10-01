//! WhisperX alignments (JSON): `segments[].words[]` with `word`, `start`
//! and `end` in seconds, and, when it was asked for them, `segments[].chars[]`
//! with `char`, `start` and `end`. Words WhisperX could not align (no
//! times) are left out. It gives no phones.

use super::{Aligned, Interval};
use serde_json::Value;

pub fn parse(json: &str) -> Result<Aligned, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    let segments = v["segments"]
        .as_array()
        .ok_or("no \"segments\" in the WhisperX JSON")?;
    let all = |list: &str, key: &str| -> Vec<Interval> {
        segments
            .iter()
            .filter_map(|s| s[list].as_array())
            .flatten()
            .filter_map(|w| interval(w, key))
            .collect()
    };
    Ok(Aligned {
        words: all("words", "word"),
        phones: vec![],
        letters: all("chars", "char"),
    })
}

fn interval(v: &Value, key: &str) -> Option<Interval> {
    Some(Interval {
        start: v["start"].as_f64()?,
        end: v["end"].as_f64()?,
        text: v[key].as_str()?.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_timed_words_and_letters() {
        let a = parse(
            r#"{"segments": [{"start": 0.1, "end": 1.0, "text": " Hello there",
                "words": [{"word": "Hello", "start": 0.1, "end": 0.5, "score": 0.9},
                          {"word": "there"},
                          {"word": "you,", "start": 0.6, "end": 1.0}],
                "chars": [{"char": "H", "start": 0.1, "end": 0.2}, {"char": " "}]}]}"#,
        )
        .unwrap();
        let words: Vec<&str> = a.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(words, ["Hello", "you,"]);
        assert_eq!(a.letters.len(), 1);
        assert!(a.phones.is_empty());
        assert!(parse("{}").is_err());
    }
}
