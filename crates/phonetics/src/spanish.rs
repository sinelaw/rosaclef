//! Spanish, by rule (Latin American: c/z before e/i are s).

/// The IPA phonemes of a Spanish word.
pub fn word(text: &str) -> Vec<String> {
    let chars: Vec<char> = text
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .map(plain)
        .collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let front = matches!(next, Some('e' | 'i'));
        let (sound, used) = match (c, next) {
            ('c', Some('h')) => ("tʃ", 2),
            ('l', Some('l')) => ("ʝ", 2),
            ('r', Some('r')) => ("r", 2),
            ('q', Some('u')) => ("k", 2),
            ('g', Some('u')) if matches!(chars.get(i + 2), Some('e' | 'i')) => ("g", 2),
            ('c', _) if front => ("s", 1),
            ('g', _) if front => ("x", 1),
            ('r', _) if i == 0 => ("r", 1),
            ('y', None) => ("i", 1),
            _ => (single(c), 1),
        };
        if !sound.is_empty() {
            out.extend(sound.split(' ').map(str::to_string));
        }
        i += used;
    }
    out
}

fn single(c: char) -> &'static str {
    match c {
        'a' => "a",
        'e' => "e",
        'i' => "i",
        'o' => "o",
        'u' | 'ü' => "u",
        'b' | 'v' => "b",
        'c' | 'k' => "k",
        'd' => "d",
        'f' => "f",
        'g' => "g",
        'h' => "",
        'j' => "x",
        'l' => "l",
        'm' => "m",
        'n' => "n",
        'ñ' => "ɲ",
        'p' => "p",
        'r' => "ɾ",
        's' | 'z' => "s",
        't' => "t",
        'w' => "w",
        'x' => "k s",
        'y' => "ʝ",
        _ => "",
    }
}

/// A vowel without its accent mark (ü keeps its diaeresis).
fn plain(c: char) -> char {
    match c {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ú' => 'u',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_words() {
        assert_eq!(word("corazón").join(" "), "k o ɾ a s o n");
        assert_eq!(word("guitarra").join(" "), "g i t a r a");
        assert_eq!(word("llueve").join(" "), "ʝ u e b e");
        assert_eq!(word("hoy").join(" "), "o i");
    }
}
