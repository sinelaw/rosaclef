//! Phonemes spelled in the alphabets singing engines use.
//!
//! - **ARPAbet** (lowercase): Synthesizer V and OpenUtau's English
//!   phonemizers, DiffSinger English voicebanks.
//! - **X-SAMPA**: VOCALOID and other engines that want ASCII IPA.
//!
//! A phoneme without an equivalent is left in IPA.

/// IPA, ARPAbet, X-SAMPA.
const TABLE: &[(&str, &str, &str)] = &[
    ("ɑ", "aa", "A"),
    ("æ", "ae", "{"),
    ("ʌ", "ah", "V"),
    ("ə", "ax", "@"),
    ("ɔ", "ao", "O"),
    ("aʊ", "aw", "aU"),
    ("aɪ", "ay", "aI"),
    ("ɛ", "eh", "E"),
    ("ɝ", "er", "3`"),
    ("ɚ", "er", "@`"),
    ("eɪ", "ey", "eI"),
    ("ɪ", "ih", "I"),
    ("i", "iy", "i"),
    ("oʊ", "ow", "oU"),
    ("ɔɪ", "oy", "OI"),
    ("ʊ", "uh", "U"),
    ("u", "uw", "u"),
    ("b", "b", "b"),
    ("tʃ", "ch", "tS"),
    ("d", "d", "d"),
    ("ð", "dh", "D"),
    ("f", "f", "f"),
    ("g", "g", "g"),
    ("h", "hh", "h"),
    ("dʒ", "jh", "dZ"),
    ("k", "k", "k"),
    ("l", "l", "l"),
    ("m", "m", "m"),
    ("n", "n", "n"),
    ("ŋ", "ng", "N"),
    ("p", "p", "p"),
    ("ɹ", "r", "r\\"),
    ("s", "s", "s"),
    ("ʃ", "sh", "S"),
    ("t", "t", "t"),
    ("θ", "th", "T"),
    ("v", "v", "v"),
    ("w", "w", "w"),
    ("j", "y", "j"),
    ("z", "z", "z"),
    ("ʒ", "zh", "Z"),
    ("a", "aa", "a"),
    ("e", "ey", "e"),
    ("o", "ow", "o"),
    ("ɯ", "uw", "M"),
    ("ɾ", "dx", "4"),
    ("r", "r", "r"),
    ("x", "hh", "x"),
    ("ɲ", "ny", "J"),
    ("ʝ", "y", "j\\"),
    ("ɕ", "sh", "s\\"),
    ("tɕ", "ch", "ts\\"),
    ("dʑ", "jh", "dz\\"),
    ("ts", "ts", "ts"),
    ("ɸ", "f", "p\\"),
    ("ç", "hh", "C"),
    ("ɴ", "n", "N\\"),
    ("ʔ", "q", "?"),
];

/// A phoneme in ARPAbet (lowercase), or as given when it has none.
pub fn arpabet(ipa: &str) -> String {
    TABLE
        .iter()
        .find(|(i, _, _)| *i == ipa)
        .map_or_else(|| ipa.to_string(), |(_, a, _)| a.to_string())
}

/// A phoneme in X-SAMPA, or as given when it has none.
pub fn xsampa(ipa: &str) -> String {
    TABLE
        .iter()
        .find(|(i, _, _)| *i == ipa)
        .map_or_else(|| ipa.to_string(), |(_, _, x)| x.to_string())
}

/// An ARPAbet phoneme (any case, stress digits ignored) in IPA.
pub fn from_arpabet(a: &str) -> Option<&'static str> {
    let a = a.trim_end_matches(['0', '1', '2']).to_ascii_lowercase();
    TABLE.iter().find(|(_, x, _)| *x == a).map(|(i, _, _)| *i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings() {
        assert_eq!(arpabet("oʊ"), "ow");
        assert_eq!(arpabet("ɚ"), "er");
        assert_eq!(xsampa("ɹ"), "r\\");
        assert_eq!(xsampa("ɑ"), "A");
        assert_eq!(arpabet("ʁ"), "ʁ");
        assert_eq!(from_arpabet("HH"), Some("h"));
        assert_eq!(from_arpabet("ER1"), Some("ɝ"));
    }
}
