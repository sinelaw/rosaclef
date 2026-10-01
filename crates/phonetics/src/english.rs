//! English: the dictionary first, spelling rules for other words.

use crate::dict;

/// The IPA phonemes of an English word.
pub fn word(text: &str) -> Vec<String> {
    let w: String = text
        .chars()
        .filter(|c| c.is_alphabetic() || *c == '\'')
        .flat_map(char::to_lowercase)
        .collect();
    let w = w.trim_matches('\'');
    if w.is_empty() {
        return vec![];
    }
    dict::lookup(w).unwrap_or_else(|| by_rule(w))
}

/// Letter groups and their sounds, longest first where they overlap. `^`
/// matches only at the start of the word, `$` only at its end.
const RULES: &[(&str, &str)] = &[
    ("^kn", "n"),
    ("^wr", "ɹ"),
    ("^gn", "n"),
    ("^ps", "s"),
    ("tion", "ʃ ə n"),
    ("sion", "ʒ ə n"),
    ("ture", "tʃ ɚ"),
    ("igh", "aɪ"),
    ("ough", "oʊ"),
    ("augh", "ɔ"),
    ("eigh", "eɪ"),
    ("tch", "tʃ"),
    ("dge", "dʒ"),
    ("le$", "ə l"),
    ("ed$", "d"),
    ("es$", "z"),
    ("ies$", "i z"),
    ("y$", "i"),
    ("ch", "tʃ"),
    ("sh", "ʃ"),
    ("th", "θ"),
    ("ph", "f"),
    ("wh", "w"),
    ("ck", "k"),
    ("ng", "ŋ"),
    ("qu", "k w"),
    ("gh", ""),
    ("ee", "i"),
    ("ea", "i"),
    ("oo", "u"),
    ("ou", "aʊ"),
    ("ow$", "oʊ"),
    ("ow", "aʊ"),
    ("ai", "eɪ"),
    ("ay", "eɪ"),
    ("oi", "ɔɪ"),
    ("oy", "ɔɪ"),
    ("au", "ɔ"),
    ("aw", "ɔ"),
    ("ie", "i"),
    ("ei", "eɪ"),
    ("ey", "eɪ"),
    ("ue", "u"),
    ("ew", "u"),
    ("oa", "oʊ"),
    ("ar", "ɑ ɹ"),
    ("er", "ɚ"),
    ("ir", "ɝ"),
    ("ur", "ɝ"),
    ("or", "ɔ ɹ"),
    ("x", "k s"),
    ("j", "dʒ"),
    ("y", "j"),
    ("w", "w"),
    ("v", "v"),
    ("z", "z"),
    ("r", "ɹ"),
    ("b", "b"),
    ("d", "d"),
    ("f", "f"),
    ("h", "h"),
    ("k", "k"),
    ("l", "l"),
    ("m", "m"),
    ("n", "n"),
    ("p", "p"),
    ("s", "s"),
    ("t", "t"),
    ("'", ""),
];

/// A long vowel before consonant + silent e ("made", "time", "home").
const LONG: &[(char, &str)] = &[
    ('a', "eɪ"),
    ('e', "i"),
    ('i', "aɪ"),
    ('o', "oʊ"),
    ('u', "u"),
];
const SHORT: &[(char, &str)] = &[('a', "æ"), ('e', "ɛ"), ('i', "ɪ"), ('o', "ɑ"), ('u', "ʌ")];

/// Read a word by spelling rules (names, made-up words, words the
/// dictionary lacks).
pub fn by_rule(w: &str) -> Vec<String> {
    let chars: Vec<char> = w.chars().collect();
    let silent_e = silent_final_e(&chars);
    let end = if silent_e {
        chars.len() - 1
    } else {
        chars.len()
    };
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i < end {
        // A doubled consonant is said once ("cell", "sunny").
        if i + 1 < end && chars[i + 1] == chars[i] && !"aeiou".contains(chars[i]) {
            i += 1;
            continue;
        }
        if let Some((len, sound)) = rule_at(&chars[..end], i, end == chars.len()) {
            out.extend(sound.split_whitespace().map(str::to_string));
            i += len;
            continue;
        }
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if let Some(sound) = vowel(c, i, end, silent_e) {
            out.push(sound.to_string());
        } else if c == 'c' || c == 'g' {
            let soft = matches!(next, Some('e' | 'i' | 'y'));
            out.push(
                match (c, soft) {
                    ('c', true) => "s",
                    ('c', false) => "k",
                    (_, true) => "dʒ",
                    _ => "g",
                }
                .to_string(),
            );
        }
        i += 1;
    }
    out
}

fn silent_final_e(chars: &[char]) -> bool {
    let n = chars.len();
    n > 2
        && chars[n - 1] == 'e'
        && !"aeiou".contains(chars[n - 2])
        && "aeiou".contains(chars[n - 3])
}

/// The rule matching at `i`, if any: (letters used, sound).
fn rule_at(chars: &[char], i: usize, at_real_end: bool) -> Option<(usize, &'static str)> {
    for (pat, sound) in RULES {
        let start = pat.starts_with('^');
        let end = pat.ends_with('$');
        let body: Vec<char> = pat
            .trim_start_matches('^')
            .trim_end_matches('$')
            .chars()
            .collect();
        if start && i != 0 {
            continue;
        }
        if chars.len() < i + body.len() || chars[i..i + body.len()] != body[..] {
            continue;
        }
        if end && (i + body.len() != chars.len() || !at_real_end || chars.len() <= body.len()) {
            continue;
        }
        return Some((body.len(), sound));
    }
    None
}

/// A vowel letter's sound: long before a silent e, else short.
fn vowel(c: char, i: usize, end: usize, silent_e: bool) -> Option<&'static str> {
    let long = silent_e && i + 2 == end;
    let table = if long { LONG } else { SHORT };
    table.iter().find(|(v, _)| *v == c).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(w: &str) -> String {
        by_rule(w).join(" ")
    }

    #[test]
    fn spelling_rules_read_unknown_words() {
        assert_eq!(r("brick"), "b ɹ ɪ k");
        assert_eq!(r("time"), "t aɪ m");
        assert_eq!(r("nation"), "n æ ʃ ə n");
        assert_eq!(r("knight"), "n aɪ t");
        assert_eq!(r("cell"), "s ɛ l");
        assert_eq!(r("sunny"), "s ʌ n i");
        assert_eq!(r("zorbak"), "z ɔ ɹ b æ k");
    }

    #[test]
    fn words_come_from_the_dictionary_first() {
        assert_eq!(word("Hello,").join(" "), "h ə l oʊ");
        assert_eq!(word("don't").join(" "), "d oʊ n t");
        assert!(word("—").is_empty());
    }
}
