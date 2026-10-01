//! Lyrics: the words a pattern sings, verse by verse.
//!
//! A [`Lyrics`] line belongs to a pattern and a vocal channel. Its verses are
//! plain text in a small notation ([`notation`]): words split into syllables
//! with `-`, `_` to hold a syllable over another note, `/` and `//` for line
//! and paragraph ends, `word[ph ph]` for a pronunciation in IPA. The
//! syllables fall on the channel's notes in time order (see
//! [`crate::expand`]), the way verses are written under a melody.
//!
//! Everything a singer, a karaoke file or a score needs is derived from this
//! one copy; only what a person decided is stored.

pub mod notation;

pub use notation::{parse, Break, ParseError, Sung, Token, WordPos};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The words sung on one channel of a pattern.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lyrics {
    /// The vocal channel whose notes carry the words.
    pub channel: String,
    /// BCP 47 language tag (`en`, `en-US`, `ja`, `es`...). Empty: English.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lang: String,
    /// How the words are voiced.
    #[serde(default, skip_serializing_if = "LyricMode::is_sing")]
    pub mode: LyricMode,
    /// Verse number (from 1) → text in the lyric notation.
    pub verses: BTreeMap<u32, String>,
    /// Phoneme timing fixed by hand or by aligning a recording, per syllable.
    /// Everything else is left to the singer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timing: Vec<SyllableTiming>,
}

/// How a lyric line is voiced.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LyricMode {
    /// Sung on the notes' pitches.
    #[default]
    Sing,
    /// Rhythmic speech on the notes' timing; pitches are ignored.
    Rap,
    /// Spoken: each note starts a word, or the whole line on a single note.
    Speak,
}

impl LyricMode {
    fn is_sing(&self) -> bool {
        *self == LyricMode::Sing
    }
}

/// The fixed timing of one syllable's phonemes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SyllableTiming {
    pub verse: u32,
    /// The beat (in the pattern) of the note the syllable is sung on.
    pub at: f64,
    pub phonemes: Vec<TimedPhoneme>,
}

/// A phoneme (IPA) and when it starts.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimedPhoneme {
    pub p: String,
    /// Seconds from the note's start; negative before it (a consonant sung
    /// ahead of the beat). Seconds, because consonants do not stretch with
    /// the tempo.
    pub offset: f64,
}

impl Lyrics {
    /// The text a verse sings: its own, or the lowest-numbered verse when it
    /// has none (a chorus written once is sung every time).
    pub fn verse_text(&self, verse: u32) -> Option<(u32, &str)> {
        self.verses
            .get_key_value(&verse)
            .or_else(|| self.verses.iter().next())
            .map(|(k, v)| (*k, v.as_str()))
    }

    /// The language tag, English when unset.
    pub fn language(&self) -> &str {
        if self.lang.is_empty() {
            "en"
        } else {
            &self.lang
        }
    }

    /// The fixed phoneme timing of the syllable on the note at `at`.
    pub fn timing_at(&self, verse: u32, at: f64) -> Option<&SyllableTiming> {
        self.timing
            .iter()
            .find(|t| t.verse == verse && (t.at - at).abs() < 1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(verses: &[(u32, &str)]) -> Lyrics {
        Lyrics {
            channel: "lead".into(),
            lang: String::new(),
            mode: LyricMode::Sing,
            verses: verses.iter().map(|(k, v)| (*k, v.to_string())).collect(),
            timing: vec![],
        }
    }

    #[test]
    fn a_missing_verse_sings_the_first() {
        let l = line(&[(1, "one"), (2, "two")]);
        assert_eq!(l.verse_text(2), Some((2, "two")));
        assert_eq!(l.verse_text(3), Some((1, "one")));
        assert_eq!(line(&[]).verse_text(1), None);
    }

    #[test]
    fn verses_are_numbered_keys_in_json() {
        let l: Lyrics =
            serde_json::from_str(r#"{"channel":"lead","verses":{"1":"Hel-lo","2":"Good-bye"}}"#)
                .unwrap();
        assert_eq!(l.verse_text(2), Some((2, "Good-bye")));
        assert_eq!(l.language(), "en");
        let back = serde_json::to_string(&l).unwrap();
        assert_eq!(
            back,
            r#"{"channel":"lead","verses":{"1":"Hel-lo","2":"Good-bye"}}"#
        );
    }
}
