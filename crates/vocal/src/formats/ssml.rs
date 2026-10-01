//! SSML for text to speech: a `<p>` per paragraph and an `<s>` per lyric
//! line; before each word a `<mark name="wN"/>` (N counts words from 0, so
//! the word times an engine reports map back to the words) and, after a
//! silence of a quarter second or more, a `<break>` of that length. Words
//! whose pronunciation the lyrics give are wrapped in `<phoneme
//! alphabet="ipa">` (syllables separated by `.`).

use super::paragraphs;
use super::xml::{escape, tag, Xml};
use crate::line::Song;
use crate::words::{Phrase, Word};

/// Silences shorter than this are not written.
const MIN_BREAK: f64 = 0.25;

pub fn write(song: &Song) -> String {
    let paras = paragraphs(song);
    let lang = paras
        .first()
        .map_or("en", |p| p[0].words[0].first().lang.as_str())
        .to_string();
    let mut x = Xml::new("");
    x.open(
        "speak",
        &[
            ("version", "1.1"),
            ("xmlns", "http://www.w3.org/2001/10/synthesis"),
            ("xml:lang", &lang),
        ],
    );
    let mut cursor = Cursor { word: 0, time: 0.0 };
    for para in &paras {
        x.open("p", &[]);
        for phrase in para {
            x.raw(&sentence(phrase, &mut cursor));
        }
        x.close();
    }
    x.finish()
}

/// Where writing is: the next word's number and the end of the last word.
struct Cursor {
    word: usize,
    time: f64,
}

fn sentence(phrase: &Phrase, at: &mut Cursor) -> String {
    let mut parts = vec![];
    for w in &phrase.words {
        let gap = w.start().sec - at.time;
        let mut s = String::new();
        if gap >= MIN_BREAK {
            let time = format!("{}ms", (gap * 1000.0).round());
            s += &tag("break", &[("time", &time)], "");
        }
        s += &tag("mark", &[("name", &format!("w{}", at.word))], "");
        s += &spoken(w);
        parts.push(s);
        at.word += 1;
        at.time = w.end().sec;
    }
    tag("s", &[], &parts.join(" "))
}

/// The word, with its pronunciation when the lyrics give one.
fn spoken(w: &Word) -> String {
    let text = escape(&w.text());
    if !w.pronounced() {
        return text;
    }
    let syllables: Vec<String> = w.phonemes().iter().map(|p| p.concat()).collect();
    tag(
        "phoneme",
        &[("alphabet", "ipa"), ("ph", &syllables.join("."))],
        &text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn sentences_with_marks_breaks_and_phonemes() {
        let text = write(&song());
        let doc = roxmltree::Document::parse(&text).unwrap();
        let root = doc.root_element();
        assert_eq!(root.tag_name().name(), "speak");
        let count = |name: &str| doc.descendants().filter(|n| n.has_tag_name(name)).count();
        assert_eq!(count("p"), 4);
        assert_eq!(count("s"), 6);
        assert_eq!(count("mark"), 13);
        assert!(text.contains(
            r#"<s><mark name="w0"/>Hello <mark name="w1"/>dark <mark name="w2"/><phoneme alphabet="ipa" ph="fɹɛnd">friend</phoneme></s>"#
        ));
        assert!(text.contains(r#"<s><break time="500ms"/><mark name="w3"/>Hi</s>"#));
    }
}
