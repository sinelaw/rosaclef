//! A sung line read as text: words, lyric lines and paragraphs, which is
//! what the lyric formats (LRC, TTML, SSML, tagged lyrics, timed words)
//! write.
//!
//! A [`Word`] is the run of notes that sings one word: its syllables and the
//! holds after them. Breaths and notes without words fall between words. A
//! [`Phrase`] is one lyric line: the words up to a `/` or `//` in the text,
//! or, where the text has none, up to a rest of a bar or more. A paragraph
//! ends at `//` or where another section (pattern or verse) starts.

use crate::line::{Performed, Syllable, Time};
use rosaclef_core::lyrics::{Break, Sung, WordPos};

/// The notes that sing one word.
#[derive(Clone, Copy, Debug)]
pub struct Word<'a> {
    pub notes: &'a [Performed],
}

impl<'a> Word<'a> {
    /// The notes with a syllable (not the holds), with its text.
    pub fn syllables(&self) -> impl Iterator<Item = (&'a Performed, &'a str)> {
        self.notes
            .iter()
            .filter_map(|n| match &syllable(n)?.token.sung {
                Sung::Syllable { text, .. } => Some((n, text.as_str())),
                _ => None,
            })
    }

    pub fn text(&self) -> String {
        self.syllables().map(|(_, t)| t).collect()
    }

    pub fn start(&self) -> Time {
        self.notes[0].start
    }

    pub fn end(&self) -> Time {
        self.notes[self.notes.len() - 1].end
    }

    /// The first syllable: the word's language, mode and section.
    pub fn first(&self) -> &'a Syllable {
        syllable(&self.notes[0]).expect("a word starts with a syllable")
    }

    /// The strongest line or paragraph end written in it.
    pub fn brk(&self) -> Break {
        self.notes
            .iter()
            .filter_map(syllable)
            .map(|s| s.token.brk)
            .max_by_key(|b| strength(*b))
            .unwrap_or_default()
    }

    /// Whether the lyrics spell out how it is pronounced (`word[ph]`).
    pub fn pronounced(&self) -> bool {
        self.notes.iter().filter_map(syllable).any(
            |s| matches!(&s.token.sung, Sung::Syllable { phonemes, .. } if !phonemes.is_empty()),
        )
    }

    /// Its phonemes (IPA), syllable by syllable.
    pub fn phonemes(&self) -> Vec<&'a [String]> {
        self.syllables()
            .filter_map(|(n, _)| syllable(n))
            .map(|s| s.phonemes.as_slice())
            .collect()
    }
}

fn syllable(n: &Performed) -> Option<&Syllable> {
    n.syllable.as_ref()
}

fn strength(b: Break) -> u8 {
    match b {
        Break::None => 0,
        Break::Line => 1,
        Break::Paragraph => 2,
    }
}

/// The words of a line's notes, in order.
pub fn words(notes: &[Performed]) -> Vec<Word<'_>> {
    let mut out = vec![];
    let mut open: Option<usize> = None;
    let mut close = |open: &mut Option<usize>, end: usize| {
        if let Some(start) = open.take() {
            out.push(Word {
                notes: &notes[start..end],
            });
        }
    };
    for (i, n) in notes.iter().enumerate() {
        match syllable(n).map(|s| &s.token.sung) {
            Some(Sung::Syllable { pos, .. }) => {
                if matches!(pos, WordPos::Single | WordPos::Begin) || open.is_none() {
                    close(&mut open, i);
                    open = Some(i);
                }
            }
            Some(Sung::Hold) if open.is_some() => {}
            _ => close(&mut open, i),
        }
    }
    close(&mut open, notes.len());
    out
}

/// One lyric line.
#[derive(Clone, Debug)]
pub struct Phrase<'a> {
    pub words: Vec<Word<'a>>,
}

impl<'a> Phrase<'a> {
    pub fn start(&self) -> Time {
        self.words[0].start()
    }

    pub fn end(&self) -> Time {
        self.words[self.words.len() - 1].end()
    }

    /// The words, separated by spaces.
    pub fn text(&self) -> String {
        let words: Vec<String> = self.words.iter().map(Word::text).collect();
        words.join(" ")
    }

    /// The section and verse it is sung in.
    pub fn section(&self) -> (&'a str, u32) {
        let s = self.words[0].first();
        (&s.section, s.verse)
    }
}

/// Words cut into lyric lines: after a word that ends a line or a
/// paragraph, before a word of another section, and before a word that
/// comes `bar` beats or more after the one before.
pub fn phrases<'a>(words: &[Word<'a>], bar: f64) -> Vec<Phrase<'a>> {
    let mut out: Vec<Phrase<'a>> = vec![];
    let mut current: Vec<Word<'a>> = vec![];
    for w in words {
        if let Some(prev) = current.last() {
            let rest = w.start().beat - prev.end().beat;
            let (a, b) = (prev.first(), w.first());
            let moved = (&a.section, a.verse) != (&b.section, b.verse);
            if prev.brk() != Break::None || moved || rest >= bar - 1e-9 {
                out.push(Phrase {
                    words: std::mem::take(&mut current),
                });
            }
        }
        current.push(*w);
    }
    if !current.is_empty() {
        out.push(Phrase { words: current });
    }
    out
}

/// Lyric lines grouped into paragraphs: after `//`, and where the section
/// or the verse changes.
pub fn paragraphs(phrases: Vec<Phrase<'_>>) -> Vec<Vec<Phrase<'_>>> {
    let mut out: Vec<Vec<Phrase>> = vec![];
    for ph in phrases {
        let starts = match out.last().and_then(|p| p.last()) {
            None => true,
            Some(prev) => {
                prev.words[prev.words.len() - 1].brk() == Break::Paragraph
                    || prev.section() != ph.section()
            }
        };
        if starts {
            out.push(vec![ph]);
        } else if let Some(p) = out.last_mut() {
            p.push(ph);
        }
    }
    out
}

/// The paragraphs of a line's notes (`bar`: beats of rest that end a line).
pub fn read(notes: &[Performed], bar: f64) -> Vec<Vec<Phrase<'_>>> {
    paragraphs(phrases(&words(notes), bar))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line::Song;
    use crate::testing::song;

    fn lead(song: &Song) -> &[Performed] {
        &song.lines[0].notes
    }

    #[test]
    fn words_take_their_holds_and_skip_breaths() {
        let s = song();
        let w = words(lead(&s));
        let texts: Vec<String> = w.iter().map(Word::text).collect();
        assert_eq!(texts[..4], ["Hello", "dark", "friend", "Hi"]);
        assert_eq!(w[2].notes.len(), 2, "friend _");
        assert!(w[2].pronounced());
        assert_eq!(w[2].phonemes(), [["f", "ɹ", "ɛ", "n", "d"]]);
        assert_eq!(w[2].brk(), Break::Line);
    }

    #[test]
    fn lines_and_paragraphs() {
        let s = song();
        let paras = read(lead(&s), 4.0);
        let shown: Vec<Vec<String>> = paras
            .iter()
            .map(|p| p.iter().map(Phrase::text).collect())
            .collect();
        assert_eq!(
            shown,
            [
                vec!["Hello dark friend", "Hi"],
                vec!["La la"],
                vec!["Bye now friend", "Hi"],
                vec!["La la"]
            ]
        );
        assert_eq!(paras[1][0].section(), ("Hook", 1));
        assert_eq!(paras[2][0].section(), ("Verse", 2));
    }
}
