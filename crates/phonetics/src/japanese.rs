//! Japanese, sung mora by mora: kana (hiragana or katakana) or romaji.

/// Hiragana and their romaji. Katakana are read as hiragana.
const KANA: &[(char, &str)] = &[
    ('あ', "a"),
    ('い', "i"),
    ('う', "u"),
    ('え', "e"),
    ('お', "o"),
    ('か', "ka"),
    ('き', "ki"),
    ('く', "ku"),
    ('け', "ke"),
    ('こ', "ko"),
    ('が', "ga"),
    ('ぎ', "gi"),
    ('ぐ', "gu"),
    ('げ', "ge"),
    ('ご', "go"),
    ('さ', "sa"),
    ('し', "shi"),
    ('す', "su"),
    ('せ', "se"),
    ('そ', "so"),
    ('ざ', "za"),
    ('じ', "ji"),
    ('ず', "zu"),
    ('ぜ', "ze"),
    ('ぞ', "zo"),
    ('た', "ta"),
    ('ち', "chi"),
    ('つ', "tsu"),
    ('て', "te"),
    ('と', "to"),
    ('だ', "da"),
    ('ぢ', "ji"),
    ('づ', "zu"),
    ('で', "de"),
    ('ど', "do"),
    ('な', "na"),
    ('に', "ni"),
    ('ぬ', "nu"),
    ('ね', "ne"),
    ('の', "no"),
    ('は', "ha"),
    ('ひ', "hi"),
    ('ふ', "fu"),
    ('へ', "he"),
    ('ほ', "ho"),
    ('ば', "ba"),
    ('び', "bi"),
    ('ぶ', "bu"),
    ('べ', "be"),
    ('ぼ', "bo"),
    ('ぱ', "pa"),
    ('ぴ', "pi"),
    ('ぷ', "pu"),
    ('ぺ', "pe"),
    ('ぽ', "po"),
    ('ま', "ma"),
    ('み', "mi"),
    ('む', "mu"),
    ('め', "me"),
    ('も', "mo"),
    ('や', "ya"),
    ('ゆ', "yu"),
    ('よ', "yo"),
    ('ら', "ra"),
    ('り', "ri"),
    ('る', "ru"),
    ('れ', "re"),
    ('ろ', "ro"),
    ('わ', "wa"),
    ('を', "o"),
    ('ん', "n"),
    ('ゔ', "vu"),
    ('っ', "cl"),
];

/// Consonants (romaji) and their IPA, longest first.
const CONSONANTS: &[(&str, &str)] = &[
    ("ky", "kʲ"),
    ("gy", "gʲ"),
    ("sh", "ɕ"),
    ("ch", "tɕ"),
    ("ts", "ts"),
    ("ny", "ɲ"),
    ("hy", "ç"),
    ("by", "bʲ"),
    ("py", "pʲ"),
    ("my", "mʲ"),
    ("ry", "ɾʲ"),
    ("j", "dʑ"),
    ("k", "k"),
    ("g", "g"),
    ("s", "s"),
    ("z", "z"),
    ("t", "t"),
    ("d", "d"),
    ("n", "n"),
    ("h", "h"),
    ("f", "ɸ"),
    ("b", "b"),
    ("p", "p"),
    ("m", "m"),
    ("y", "j"),
    ("r", "ɾ"),
    ("w", "w"),
    ("v", "v"),
];

/// The IPA phonemes of a sung syllable (one or more morae).
pub fn syllable(text: &str) -> Vec<String> {
    let romaji = to_romaji(text);
    let mut out: Vec<String> = vec![];
    let mut rest = romaji.as_str();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("cl") {
            out.push("ʔ".into());
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix('-') {
            if let Some(v) = out.iter().rev().find(|p| crate::ipa::is_vowel(p)).cloned() {
                out.push(v);
            }
            rest = r;
            continue;
        }
        rest = mora(rest, &mut out);
    }
    out
}

/// Read one mora of romaji into `out`; returns the rest.
fn mora<'a>(rest: &'a str, out: &mut Vec<String>) -> &'a str {
    let (cons, after) = CONSONANTS
        .iter()
        .find(|(r, _)| rest.starts_with(r))
        .map(|(r, ipa)| (Some(*ipa), &rest[r.len()..]))
        .unwrap_or((None, rest));
    let vowel = after.chars().next().filter(|c| "aiueo".contains(*c));
    match (cons, vowel) {
        (Some("n"), None) => out.push("ɴ".into()),
        (Some(c), Some(v)) => {
            // h before i is ç (hi); the others keep their sound.
            out.push(if c == "h" && v == 'i' {
                "ç".into()
            } else {
                c.into()
            });
            out.push(vowel_ipa(v).into());
        }
        (None, Some(v)) => out.push(vowel_ipa(v).into()),
        (Some(c), None) => out.push(c.into()),
        (None, None) => {
            let skip = rest.chars().next().map_or(1, char::len_utf8);
            return &rest[skip..];
        }
    }
    &after[vowel.map_or(0, |_| 1)..]
}

fn vowel_ipa(v: char) -> &'static str {
    match v {
        'u' => "ɯ",
        'a' => "a",
        'i' => "i",
        'e' => "e",
        _ => "o",
    }
}

/// Kana to romaji (romaji passes through, lowercased); `ー` becomes `-`.
fn to_romaji(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().map(hiragana) {
        match c {
            'ゃ' | 'ゅ' | 'ょ' => {
                let v = match c {
                    'ゃ' => "a",
                    'ゅ' => "u",
                    _ => "o",
                };
                // き + ゃ = kya, し + ゃ = sha.
                if out.ends_with('i') {
                    out.pop();
                    if !(out.ends_with("sh") || out.ends_with("ch") || out.ends_with('j')) {
                        out.push('y');
                    }
                }
                out.push_str(v);
            }
            'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' => {
                // ふ + ぁ = fa: the small vowel replaces the mora's own.
                if out.ends_with(['a', 'i', 'u', 'e', 'o']) {
                    out.pop();
                }
                out.push_str(KANA[(c as u32 - 'ぁ' as u32) as usize / 2].1);
            }
            'ー' => out.push('-'),
            c => match KANA.iter().find(|(k, _)| *k == c) {
                Some((_, r)) => out.push_str(r),
                None => out.extend(c.to_lowercase()),
            },
        }
    }
    out
}

/// Katakana to hiragana (the two blocks are parallel).
fn hiragana(c: char) -> char {
    match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> String {
        syllable(s).join(" ")
    }

    #[test]
    fn kana_and_romaji() {
        assert_eq!(j("さ"), "s a");
        assert_eq!(j("し"), "ɕ i");
        assert_eq!(j("きょ"), "kʲ o");
        assert_eq!(j("しゃ"), "ɕ a");
        assert_eq!(j("ふぁ"), "ɸ a");
        assert_eq!(j("ん"), "ɴ");
        assert_eq!(j("カー"), "k a a");
        assert_eq!(j("っ"), "ʔ");
        assert_eq!(j("hi"), "ç i");
        assert_eq!(j("tsu"), "ts ɯ");
        assert_eq!(j("san"), "s a ɴ");
    }
}
