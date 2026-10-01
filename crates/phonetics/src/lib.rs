//! Pronunciation for lyrics: the phonemes (IPA) each sung syllable is made
//! of, for the singing voice and for exports that carry phonemes.
//!
//! - [`word`] turns a word's syllables into phonemes per syllable: English
//!   from a dictionary (a CMU Pronouncing Dictionary subset) and spelling
//!   rules for the rest, Spanish (Latin American) and Japanese (kana or
//!   romaji) by rule; other languages are read as English.
//! - [`pronounce`] does it for a run of lyric tokens, keeping the
//!   pronunciations written in the lyrics (`word[w ɜ d]`).
//! - [`ipa`] classifies phonemes; [`alphabet`] spells them in ARPAbet and
//!   X-SAMPA for engines that want those.
//!
//! Everything is pure and allocation-light; it runs natively and in
//! WebAssembly.

pub mod alphabet;
mod dict;
mod english;
pub mod ipa;
mod japanese;
mod spanish;
mod syllables;

use rosaclef_core::expand::{LineRef, Sounding};
use rosaclef_core::lyrics::{Sung, Token, WordPos};
use rosaclef_core::Project;

/// The phonemes of each syllable of a word in `lang` (a BCP 47 tag).
pub fn word(lang: &str, syllables: &[&str]) -> Vec<Vec<String>> {
    match primary(lang) {
        "ja" => syllables.iter().map(|s| japanese::syllable(s)).collect(),
        "es" => spread(syllables, spanish::word),
        _ => spread(syllables, english::word),
    }
}

/// The phonemes of each token (empty for holds and breaths), words taken
/// whole so that "Hel-lo" is pronounced as "hello". A pronunciation written
/// in the lyrics wins.
pub fn pronounce(lang: &str, tokens: &[&Token]) -> Vec<Vec<String>> {
    let mut out = vec![vec![]; tokens.len()];
    let mut word_at: Vec<usize> = vec![];
    for (i, t) in tokens.iter().enumerate() {
        let Sung::Syllable { pos, .. } = &t.sung else {
            continue;
        };
        if matches!(pos, WordPos::Single | WordPos::Begin) {
            word_at.clear();
        }
        word_at.push(i);
        if pos.ends_word() {
            fill(lang, tokens, &word_at, &mut out);
            word_at.clear();
        }
    }
    if !word_at.is_empty() {
        fill(lang, tokens, &word_at, &mut out);
    }
    out
}

/// Pronounce the word made of `tokens[at]`, into `out`.
fn fill(lang: &str, tokens: &[&Token], at: &[usize], out: &mut [Vec<String>]) {
    let texts: Vec<&str> = at.iter().map(|&i| syllable_text(tokens[i])).collect();
    let spoken = word(lang, &texts);
    for (k, &i) in at.iter().enumerate() {
        out[i] = match &tokens[i].sung {
            Sung::Syllable { phonemes, .. } if !phonemes.is_empty() => {
                phonemes.iter().map(|p| ipa::normalize(p)).collect()
            }
            _ => spoken[k].clone(),
        };
    }
}

fn syllable_text(t: &Token) -> &str {
    match &t.sung {
        Sung::Syllable { text, .. } => text,
        _ => "",
    }
}

/// The language subtag of a BCP 47 tag, lowercased ("en-US" → "en").
fn primary(lang: &str) -> &str {
    let p = lang.split('-').next().unwrap_or("");
    if p.eq_ignore_ascii_case("ja") {
        "ja"
    } else if p.eq_ignore_ascii_case("es") {
        "es"
    } else {
        "en"
    }
}

/// A word read whole and its phonemes shared out over its syllables; when
/// they do not match (more syllables than vowels), each syllable is read on
/// its own.
fn spread(syllables: &[&str], read: fn(&str) -> Vec<String>) -> Vec<Vec<String>> {
    let whole = read(&syllables.concat());
    if syllables.len() <= 1 {
        return vec![whole];
    }
    syllables::split(&whole, syllables.len())
        .unwrap_or_else(|| syllables.iter().map(|s| read(s)).collect())
}

/// The phonemes of each note's syllable (empty for notes without one, holds
/// and breaths). `notes` are one channel's sounding notes in order; each
/// lyric line's run is pronounced in its language, words taken whole.
pub fn pronounce_notes(project: &Project, notes: &[&Sounding]) -> Vec<Vec<String>> {
    let mut out = vec![vec![]; notes.len()];
    let mut i = 0;
    while i < notes.len() {
        let Some(first) = notes[i].lyric.as_ref() else {
            i += 1;
            continue;
        };
        let line = first.line;
        let run = notes[i..]
            .iter()
            .take_while(|n| n.lyric.as_ref().is_some_and(|l| same_line(l.line, line)))
            .count();
        let tokens: Vec<&Token> = notes[i..i + run]
            .iter()
            .filter_map(|n| n.lyric.as_ref().map(|l| &l.token))
            .collect();
        let lang = line.lyrics(project).language();
        for (k, ph) in pronounce(lang, &tokens).into_iter().enumerate() {
            out[i + k] = ph;
        }
        i += run;
    }
    out
}

fn same_line(a: LineRef, b: LineRef) -> bool {
    a.pattern == b.pattern && a.index == b.index && a.verse == b.verse
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosaclef_core::lyrics::parse;

    fn joined(v: &[Vec<String>]) -> Vec<String> {
        v.iter().map(|p| p.join(" ")).collect()
    }

    #[test]
    fn a_word_is_pronounced_whole_and_shared_out() {
        assert_eq!(joined(&word("en", &["Hel", "lo"])), ["h ə", "l oʊ"]);
        assert_eq!(
            joined(&word("en-US", &["dark", "ness"])),
            ["d ɑ ɹ k", "n ə s"]
        );
        assert_eq!(joined(&word("en", &["friend"])), ["f ɹ ɛ n d"]);
    }

    #[test]
    fn tokens_keep_written_pronunciations_and_skip_holds() {
        let tokens = parse("Hel-lo _ friend[f ɹ ɛ n] (br) a- _ gain").unwrap();
        let refs: Vec<&Token> = tokens.iter().collect();
        assert_eq!(
            joined(&pronounce("en", &refs)),
            ["h ə", "l oʊ", "", "f ɹ ɛ n", "", "ə", "", "g ɛ n"]
        );
    }

    #[test]
    fn other_languages() {
        assert_eq!(
            joined(&word("ja", &["さ", "く", "ら"])),
            ["s a", "k ɯ", "ɾ a"]
        );
        assert_eq!(
            joined(&word("es", &["co", "ra", "zón"])),
            ["k o", "ɾ a", "s o n"]
        );
    }
}
