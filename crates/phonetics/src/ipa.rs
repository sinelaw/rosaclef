//! IPA phonemes: what kind of sound each is.

/// The manner of a phoneme, which is what a voice needs to know to make it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Vowel,
    Stop,
    Affricate,
    Fricative,
    Nasal,
    Liquid,
    Glide,
    /// A glottal stop or anything unknown: a short silence.
    Other,
}

const VOWEL_CHARS: &str = "aeiouyɑæʌəɔɛɝɚɪʊɯøœɒɐɨʉɤ";

/// The class of an IPA phoneme ("ɑ", "tʃ", "aɪ", "ŋ"...).
pub fn class(p: &str) -> Class {
    let p = p.trim_end_matches(['ː', 'ʰ', 'ʲ', 'ʷ']);
    let first = p.chars().next().unwrap_or(' ');
    if VOWEL_CHARS.contains(first) {
        return Class::Vowel;
    }
    match p {
        "p" | "b" | "t" | "d" | "k" | "g" | "ɡ" | "c" | "ɟ" | "q" => Class::Stop,
        "tʃ" | "dʒ" | "ts" | "dz" | "tɕ" | "dʑ" => Class::Affricate,
        "f" | "v" | "θ" | "ð" | "s" | "z" | "ʃ" | "ʒ" | "h" | "x" | "ɣ" | "ç" | "ɸ" | "β" | "ɕ"
        | "ʑ" | "ʝ" | "χ" => Class::Fricative,
        "m" | "n" | "ŋ" | "ɲ" | "ɴ" | "ɱ" => Class::Nasal,
        "l" | "ɹ" | "r" | "ɾ" | "ʎ" | "ɫ" | "ʁ" => Class::Liquid,
        "w" | "j" | "ɰ" => Class::Glide,
        _ => Class::Other,
    }
}

pub fn is_vowel(p: &str) -> bool {
    class(p) == Class::Vowel
}

/// Whether the vocal folds sound through it.
pub fn voiced(p: &str) -> bool {
    match class(p) {
        Class::Vowel | Class::Nasal | Class::Liquid | Class::Glide => true,
        Class::Stop | Class::Affricate | Class::Fricative => {
            matches!(
                p,
                "b" | "d"
                    | "g"
                    | "ɡ"
                    | "ɟ"
                    | "dʒ"
                    | "dz"
                    | "dʑ"
                    | "v"
                    | "ð"
                    | "z"
                    | "ʒ"
                    | "ɣ"
                    | "β"
                    | "ʑ"
                    | "ʝ"
            )
        }
        Class::Other => false,
    }
}

/// One spelling for phonemes typed in more than one way (`ɡ` → `g`).
pub fn normalize(p: &str) -> String {
    p.replace('ɡ', "g")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(class("aɪ"), Class::Vowel);
        assert_eq!(class("ɚ"), Class::Vowel);
        assert_eq!(class("tʃ"), Class::Affricate);
        assert_eq!(class("ŋ"), Class::Nasal);
        assert_eq!(class("ɹ"), Class::Liquid);
        assert_eq!(class("kʲ"), Class::Stop);
        assert!(voiced("z") && !voiced("s") && voiced("m"));
        assert_eq!(normalize("ɡ"), "g");
    }
}
