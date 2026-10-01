//! Matching what an aligner heard to the lyric line: words paired in order,
//! and a word's phones shared out over its syllables.

use rosaclef_phonetics::ipa;

/// A word for comparing: lowercase letters and digits only.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Leaving a word out costs this much...
const GAP: usize = 2;
/// ... and pairing two different words this much: less than leaving both
/// out, so a misheard word still pairs with the word in its place.
const MISHEARD: usize = 3;

/// Pairs (line word, heard word), in order, from an edit-distance alignment
/// of the two lists: equal words pair, a different word in the same place
/// pairs too (the aligner misheard it), and extra words on either side are
/// left out.
pub fn pair_words(line: &[String], heard: &[String]) -> Vec<(usize, usize)> {
    let cost = costs(line, heard);
    let mut out = vec![];
    let (mut i, mut j) = (line.len(), heard.len());
    while i > 0 && j > 0 {
        let c = cost[i][j];
        let matched = line[i - 1] == heard[j - 1] && c == cost[i - 1][j - 1];
        if !matched && c == cost[i - 1][j] + GAP {
            i -= 1;
        } else if !matched && c == cost[i][j - 1] + GAP {
            j -= 1;
        } else {
            out.push((i - 1, j - 1));
            (i, j) = (i - 1, j - 1);
        }
    }
    out.reverse();
    out
}

/// `cost[i][j]`: the cheapest alignment of the first `i` line words with
/// the first `j` heard words.
fn costs(line: &[String], heard: &[String]) -> Vec<Vec<usize>> {
    let (n, m) = (line.len(), heard.len());
    let mut cost = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in cost.iter_mut().enumerate() {
        row[0] = i * GAP;
    }
    cost[0] = (0..=m).map(|j| j * GAP).collect();
    for i in 1..=n {
        for j in 1..=m {
            let differ = if line[i - 1] == heard[j - 1] {
                0
            } else {
                MISHEARD
            };
            let pair = cost[i - 1][j - 1] + differ;
            cost[i][j] = pair.min(cost[i - 1][j] + GAP).min(cost[i][j - 1] + GAP);
        }
    }
    cost
}

/// Where each syllable of a word starts among its phones (IPA), given the
/// syllables' phonemes by the dictionary (`expected`): at its vowel, with
/// as many of the consonants before it as the dictionary starts it with.
/// When the phones do not have a vowel per syllable, each syllable takes as
/// many phones as the dictionary gives it. A syllable left without phones
/// starts at the end.
pub fn split(phones: &[String], expected: &[Vec<String>]) -> Vec<usize> {
    let vowels: Vec<usize> = (0..phones.len())
        .filter(|&i| ipa::is_vowel(&phones[i]))
        .collect();
    if vowels.len() != expected.len() {
        let mut at = 0;
        return expected
            .iter()
            .map(|e| {
                let start = at.min(phones.len());
                at += e.len();
                start
            })
            .collect();
    }
    let mut out = vec![0];
    for k in 1..vowels.len() {
        let cluster = vowels[k] - vowels[k - 1] - 1;
        let onset = expected[k].iter().take_while(|p| !ipa::is_vowel(p)).count();
        out.push(vowels[k] - onset.min(cluster));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn pairs_words_in_order_across_mistakes() {
        let line = words("hello dark ness my old friend");
        let heard = words("hello darkness my oh old friend");
        assert_eq!(
            pair_words(&line, &heard),
            [(0, 0), (1, 1), (3, 2), (4, 4), (5, 5)]
        );
        assert_eq!(normalize("Friend,"), "friend");
    }

    #[test]
    fn splits_at_vowels_as_the_dictionary_does() {
        let hello = [words("h ə"), words("l oʊ")];
        assert_eq!(split(&words("h ʌ l oʊ"), &hello), [0, 2]);
        let darkness = [words("d ɑ ɹ k"), words("n ə s")];
        assert_eq!(split(&words("d ɑ ɹ k n ɪ s"), &darkness), [0, 4]);
        let monster = [words("m ɑ n"), words("s t ɚ")];
        assert_eq!(split(&words("m ɑ n s t ɚ"), &monster), [0, 3]);
        // No vowel per syllable: by count.
        assert_eq!(split(&words("h m m"), &hello), [0, 2]);
        let three = [words("a"), words("b c"), words("d")];
        assert_eq!(split(&words("h m m"), &three), [0, 1, 3]);
    }
}
