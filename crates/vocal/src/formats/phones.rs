//! Languages and phonemes as singing engines name them.

use rosaclef_phonetics::alphabet;

/// The language subtag of a BCP 47 tag, lowercased ("en-US" → "en").
pub fn primary(lang: &str) -> String {
    lang.split('-').next().unwrap_or("").to_ascii_lowercase()
}

/// Whether the language is typed a word per note, the notes of its later
/// syllables marked as going on with the word (`+` in OpenUtau and
/// Synthesizer V, whose English phonemizers split words themselves), rather
/// than a syllable per note (Japanese kana, Spanish).
pub fn by_word(lang: &str) -> bool {
    primary(lang) == "en"
}

/// Japanese phones (as DiffSinger and OpenUtau's Japanese banks spell
/// them) for the IPA the phonetics crate writes; the rest are the same.
const ROMAJI: &[(&str, &str)] = &[
    ("ɯ", "u"),
    ("ɾ", "r"),
    ("ɕ", "sh"),
    ("tɕ", "ch"),
    ("dʑ", "j"),
    ("ʑ", "j"),
    ("ɸ", "f"),
    ("ç", "h"),
    ("ɲ", "ny"),
    ("ɴ", "N"),
    ("ʔ", "cl"),
    ("j", "y"),
];

/// A phoneme in the alphabet engines use for the language: ARPAbet for
/// English, romaji phones for Japanese, IPA otherwise.
pub fn for_engine(lang: &str, ipa: &str) -> String {
    match primary(lang).as_str() {
        "en" => alphabet::arpabet(ipa),
        "ja" => ROMAJI
            .iter()
            .find(|(i, _)| *i == ipa)
            .map_or_else(|| ipa.to_string(), |(_, r)| r.to_string()),
        _ => ipa.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_by_language() {
        assert_eq!(for_engine("en-US", "ɹ"), "r");
        assert_eq!(for_engine("ja", "ɯ"), "u");
        assert_eq!(for_engine("ja", "k"), "k");
        assert_eq!(for_engine("es", "ɾ"), "ɾ");
        assert!(by_word("EN-gb") && !by_word("ja"));
    }
}
