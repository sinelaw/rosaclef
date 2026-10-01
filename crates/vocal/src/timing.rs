//! When a syllable's phonemes start, where nothing fixed it: a simple rule
//! for formats that need phoneme durations (DiffSinger) and for aligned
//! words without phones.
//!
//! - Consonants before the first vowel take [`CONSONANT`] seconds each and
//!   end where the vowel starts, so they are sung ahead of it (as singers
//!   and DiffSinger do); they never take more than their share of the time
//!   since `earliest`.
//! - Consonants after the last vowel take [`CONSONANT`] seconds each and end
//!   with the syllable, using at most half of the vowel's time.
//! - The vowel takes the rest (several vowels share it evenly).
//! - A syllable without a vowel spreads its phonemes evenly from `vowel`.

use rosaclef_phonetics::ipa;

/// Seconds a consonant takes.
pub const CONSONANT: f64 = 0.08;

/// The start (seconds) of each phoneme of a syllable whose vowel starts at
/// `vowel` and which ends at `end`; onset consonants start no earlier than
/// `earliest`.
pub fn rule(phonemes: &[String], earliest: f64, vowel: f64, end: f64) -> Vec<f64> {
    let vowels: Vec<usize> = (0..phonemes.len())
        .filter(|&i| ipa::is_vowel(&phonemes[i]))
        .collect();
    let (Some(&first), Some(&last)) = (vowels.first(), vowels.last()) else {
        return even(phonemes.len(), vowel, end);
    };
    let onset = CONSONANT.min((vowel - earliest).max(0.0) / (first as f64 + 1.0));
    let coda_count = phonemes.len() - 1 - last;
    let coda = if coda_count == 0 {
        0.0
    } else {
        CONSONANT.min((end - vowel).max(0.0) * 0.5 / coda_count as f64)
    };
    let coda_start = end - coda * coda_count as f64;
    let mut out: Vec<f64> = (0..first)
        .map(|i| vowel - onset * (first - i) as f64)
        .collect();
    out.extend(even(last + 1 - first, vowel, coda_start));
    out.extend((0..coda_count).map(|j| coda_start + coda * j as f64));
    out
}

/// The start of each phoneme of a syllable whose first phoneme starts at
/// `start` and which ends at `end` (an aligned word, without its phones):
/// onset consonants of [`CONSONANT`] seconds from `start` (together at
/// most half the syllable), then as [`rule`].
pub fn from_start(phonemes: &[String], start: f64, end: f64) -> Vec<f64> {
    let Some(first) = phonemes.iter().position(|p| ipa::is_vowel(p)) else {
        return even(phonemes.len(), start, end);
    };
    let step = CONSONANT.min((end - start).max(0.0) * 0.5 / first.max(1) as f64);
    let vowel = start + step * first as f64;
    // The earliest start that leaves each onset exactly `step`.
    rule(phonemes, vowel - step * (first + 1) as f64, vowel, end)
}

/// `n` starts spread evenly over `[from, to)`.
fn even(n: usize, from: f64, to: f64) -> Vec<f64> {
    let step = (to - from).max(0.0) / n.max(1) as f64;
    (0..n).map(|i| from + step * i as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ph(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }

    fn rounded(v: Vec<f64>) -> Vec<f64> {
        v.into_iter()
            .map(|x| (x * 1000.0).round() / 1000.0)
            .collect()
    }

    #[test]
    fn consonants_lead_the_vowel_and_close_the_syllable() {
        let t = rounded(rule(&ph("f ɹ ɛ n d"), 0.0, 1.0, 1.5));
        assert_eq!(t, [0.84, 0.92, 1.0, 1.34, 1.42]);
    }

    #[test]
    fn short_spaces_shrink_the_consonants() {
        // Only 0.1 s since the previous phoneme: two onsets share 2/3 of it.
        let t = rounded(rule(&ph("s t a"), 0.9, 1.0, 1.2));
        assert_eq!(t, [0.933, 0.967, 1.0]);
        assert_eq!(rounded(rule(&ph("m m"), 0.0, 1.0, 1.5)), [1.0, 1.25]);
    }

    #[test]
    fn from_a_start() {
        let t = rounded(from_start(&ph("f ɹ ɛ n d"), 1.0, 2.0));
        assert_eq!(t, [1.0, 1.08, 1.16, 1.84, 1.92]);
        assert_eq!(
            rounded(from_start(&ph("s t a"), 1.0, 1.2)),
            [1.0, 1.05, 1.1]
        );
    }
}
