//! The English pronouncing dictionary (`data/en.dict`, made by
//! `tools/gen_dict.py`): `word<TAB>phonemes` lines sorted by word, searched
//! in place.

const DICT: &str = include_str!("../data/en.dict");

/// The IPA phonemes of a lowercase English word, when the dictionary has it.
pub fn lookup(word: &str) -> Option<Vec<String>> {
    let bytes = DICT.as_bytes();
    let (mut lo, mut hi) = (0usize, bytes.len());
    while lo < hi {
        let mid = line_start(bytes, (lo + hi) / 2);
        let end = mid
            + bytes[mid..]
                .iter()
                .position(|b| *b == b'\n')
                .unwrap_or(bytes.len() - mid);
        let line = &DICT[mid..end];
        let (w, phonemes) = line.split_once('\t')?;
        match w.as_bytes().cmp(word.as_bytes()) {
            std::cmp::Ordering::Equal => {
                return Some(phonemes.split(' ').map(str::to_string).collect());
            }
            std::cmp::Ordering::Less => lo = end + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// The start of the line holding byte `i`.
fn line_start(bytes: &[u8], i: usize) -> usize {
    bytes[..i]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |p| p + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_words_first_last_and_between() {
        assert_eq!(lookup("hello").unwrap().join(" "), "h ə l oʊ");
        let first = DICT.lines().next().unwrap().split('\t').next().unwrap();
        let last = DICT.lines().last().unwrap().split('\t').next().unwrap();
        assert!(lookup(first).is_some() && lookup(last).is_some());
        assert!(lookup("zzzzqx").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn the_file_is_sorted() {
        let words: Vec<&str> = DICT
            .lines()
            .map(|l| l.split('\t').next().unwrap())
            .collect();
        assert!(words.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
    }
}
