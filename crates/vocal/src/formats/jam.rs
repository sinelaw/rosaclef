//! Timed words as JSON, `[{"start", "end", "word"}]` in seconds: the lyric
//! input of JAM and similar song generators.

use super::{lead, ms};
use crate::line::Song;
use crate::words;
use serde_json::{json, Value};

pub fn write(song: &Song) -> String {
    let list: Vec<Value> = words::words(lead(song))
        .iter()
        .map(|w| json!({"start": ms(w.start().sec), "end": ms(w.end().sec), "word": w.text()}))
        .collect();
    serde_json::to_string_pretty(&list).expect("JSON values serialize") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn words_with_seconds() {
        let v: Value = serde_json::from_str(&write(&song())).unwrap();
        let list = v.as_array().unwrap();
        assert_eq!(list.len(), 12);
        assert_eq!(list[2], json!({"start": 1.5, "end": 2.0, "word": "friend"}));
        assert_eq!(list[6]["word"], "Bye");
        assert_eq!(list[6]["start"], 4.0);
    }
}
