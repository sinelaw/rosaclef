//! Sharing a word's phonemes out over its written syllables.

use crate::ipa;

/// Consonant clusters a syllable may start with (English and Spanish), so
/// "dark-ness" splits as "d ɑ ɹ k | n ə s" and "a-gain" as "ə | g ɛ n".
const ONSETS: &[&str] = &[
    "p l", "p ɹ", "t ɹ", "k ɹ", "k l", "b l", "b ɹ", "d ɹ", "g ɹ", "g l", "f l", "f ɹ", "θ ɹ",
    "ʃ ɹ", "s p", "s t", "s k", "s m", "s n", "s l", "s w", "t w", "k w", "d w", "p j", "b j",
    "k j", "m j", "f j", "s p ɹ", "s t ɹ", "s k ɹ", "s p l", "s k w", "p ɾ", "t ɾ", "k ɾ", "b ɾ",
    "d ɾ", "g ɾ", "f ɾ",
];

/// Split phonemes into `n` syllables, one vowel each, consonants between two
/// vowels starting the second syllable as far as a syllable can start that
/// way. `None` when the word does not have `n` vowels.
pub fn split(phonemes: &[String], n: usize) -> Option<Vec<Vec<String>>> {
    let vowels: Vec<usize> = (0..phonemes.len())
        .filter(|&i| ipa::is_vowel(&phonemes[i]))
        .collect();
    if vowels.len() != n || n == 0 {
        return None;
    }
    let mut cuts = vec![0];
    for pair in vowels.windows(2) {
        let (a, b) = (pair[0] + 1, pair[1]);
        cuts.push(b - onset_len(&phonemes[a..b]));
    }
    cuts.push(phonemes.len());
    Some(
        cuts.windows(2)
            .map(|w| phonemes[w[0]..w[1]].to_vec())
            .collect(),
    )
}

/// How many of these consonants (from the end) start the next syllable.
fn onset_len(consonants: &[String]) -> usize {
    let n = consonants.len();
    for k in (2..=n.min(3)).rev() {
        if ONSETS.contains(&consonants[n - k..].join(" ").as_str()) {
            return k;
        }
    }
    // A single consonant starts the next syllable, except ŋ, which ends one.
    match consonants.last() {
        Some(c) if c == "ŋ" => 0,
        Some(_) => 1,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str, n: usize) -> Option<Vec<String>> {
        let ph: Vec<String> = text.split(' ').map(str::to_string).collect();
        split(&ph, n).map(|v| v.iter().map(|x| x.join(" ")).collect())
    }

    #[test]
    fn splits_between_vowels() {
        assert_eq!(s("d ɑ ɹ k n ə s", 2).unwrap(), ["d ɑ ɹ k", "n ə s"]);
        assert_eq!(s("ə g ɛ n", 2).unwrap(), ["ə", "g ɛ n"]);
        assert_eq!(s("ɪ k s t ɹ ə", 2).unwrap(), ["ɪ k", "s t ɹ ə"]);
        assert_eq!(s("s ɪ ŋ ɪ ŋ", 2).unwrap(), ["s ɪ ŋ", "ɪ ŋ"]);
        assert_eq!(s("b j u t ɪ f ə l", 3).unwrap(), ["b j u", "t ɪ", "f ə l"]);
        assert!(s("h ə l oʊ", 3).is_none());
    }
}
