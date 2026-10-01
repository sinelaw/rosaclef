//! What each note sings, as singing-engine projects type it (OpenUtau,
//! Synthesizer V): languages typed by word put the whole word on its first
//! note and a marker on the notes of its later syllables (the engine splits
//! the word); other languages put each syllable on its note. Holds and
//! breaths get the format's own markers.

use super::phones;
use crate::line::Performed;
use rosaclef_core::lyrics::{Sung, WordPos};

/// A format's markers.
pub struct Marks {
    /// A later syllable of the word on the note before.
    pub next: &'static str,
    /// A note holding the syllable before (a melisma).
    pub hold: &'static str,
    pub breath: &'static str,
}

/// Each note's lyric; `None` for a note without words.
pub fn lyrics(notes: &[Performed], marks: &Marks) -> Vec<Option<String>> {
    (0..notes.len())
        .map(|i| match sung(&notes[i])? {
            Sung::Hold => Some(marks.hold.to_string()),
            Sung::Breath => Some(marks.breath.to_string()),
            Sung::Syllable { .. } => {
                let unit = unit(notes, i);
                Some(if unit.is_empty() {
                    marks.next.to_string()
                } else {
                    unit.iter().map(|&k| text(&notes[k])).collect()
                })
            }
        })
        .collect()
}

/// The syllable notes whose text the lyric on note `i` spells: the whole
/// word at its first syllable in a language typed by word (none at its
/// later syllables), else the syllable itself.
pub fn unit(notes: &[Performed], i: usize) -> Vec<usize> {
    let Some(Sung::Syllable { pos, .. }) = sung(&notes[i]) else {
        return vec![];
    };
    let by_word = notes[i]
        .syllable
        .as_ref()
        .is_some_and(|s| phones::by_word(&s.lang));
    if !by_word {
        return vec![i];
    }
    if !matches!(pos, WordPos::Single | WordPos::Begin) {
        return vec![];
    }
    let mut out = vec![i];
    let mut k = i + 1;
    while k < notes.len() && !ends_word(&notes[out[out.len() - 1]]) {
        match sung(&notes[k]) {
            Some(Sung::Hold) => {}
            Some(Sung::Syllable { pos, .. })
                if !matches!(pos, WordPos::Single | WordPos::Begin) =>
            {
                out.push(k)
            }
            _ => break,
        }
        k += 1;
    }
    out
}

/// Whether the lyrics spell out the syllable's pronunciation (`word[ph]`).
pub fn written(n: &Performed) -> bool {
    matches!(sung(n), Some(Sung::Syllable { phonemes, .. }) if !phonemes.is_empty())
}

fn sung(n: &Performed) -> Option<&Sung> {
    n.syllable.as_ref().map(|s| &s.token.sung)
}

fn text(n: &Performed) -> &str {
    match sung(n) {
        Some(Sung::Syllable { text, .. }) => text,
        _ => "",
    }
}

fn ends_word(n: &Performed) -> bool {
    matches!(sung(n), Some(Sung::Syllable { pos, .. }) if pos.ends_word())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    const MARKS: Marks = Marks {
        next: "+",
        hold: "-",
        breath: "br",
    };

    #[test]
    fn words_on_their_first_note() {
        let s = song();
        let l: Vec<String> = lyrics(&s.lines[0].notes, &MARKS)
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect();
        assert_eq!(
            l[..9],
            ["Hello", "+", "dark", "friend", "-", "br", "Hi", "La", "la"]
        );
        assert!(written(&s.lines[0].notes[3]) && !written(&s.lines[0].notes[2]));
    }

    #[test]
    fn syllable_languages_spell_each_syllable() {
        let s = song();
        let mut notes = s.lines[0].notes.clone();
        for n in &mut notes {
            if let Some(syl) = &mut n.syllable {
                syl.lang = "ja".into();
            }
        }
        let l = lyrics(&notes, &MARKS);
        assert_eq!(l[0].as_deref(), Some("Hel"));
        assert_eq!(l[1].as_deref(), Some("lo"));
    }
}
