//! TTML lyrics with word timing, as music apps show them (Apple's
//! `itunes:timing="Word"`): a `<div>` per paragraph, a `<p>` per lyric line
//! and a `<span>` per word, each with its `begin` and `end`.

use super::paragraphs;
use super::xml::{escape, tag, Xml};
use crate::line::Song;
use crate::words::Phrase;

pub fn write(song: &Song) -> String {
    let paras = paragraphs(song);
    let lang = paras
        .first()
        .map_or("en", |p| p[0].words[0].first().lang.as_str())
        .to_string();
    let end = paras
        .last()
        .and_then(|p| p.last())
        .map_or(0.0, |ph| ph.end().sec);
    let mut x = Xml::new("");
    x.open(
        "tt",
        &[
            ("xmlns", "http://www.w3.org/ns/ttml"),
            ("xmlns:ttm", "http://www.w3.org/ns/ttml#metadata"),
            ("xmlns:itunes", "http://music.apple.com/lyric-ttml-internal"),
            ("itunes:timing", "Word"),
            ("xml:lang", &lang),
        ],
    );
    x.open("head", &[]);
    x.open("metadata", &[]);
    x.leaf("ttm:title", &[], &song.title);
    x.close();
    x.close();
    x.open("body", &[("dur", &clock(end))]);
    for para in &paras {
        let (from, to) = (para[0].start().sec, para[para.len() - 1].end().sec);
        x.open("div", &[("begin", &clock(from)), ("end", &clock(to))]);
        for phrase in para {
            x.raw(&line(phrase));
        }
        x.close();
    }
    x.finish()
}

fn line(phrase: &Phrase) -> String {
    let spans: Vec<String> = phrase
        .words
        .iter()
        .map(|w| {
            let (b, e) = (clock(w.start().sec), clock(w.end().sec));
            tag("span", &[("begin", &b), ("end", &e)], &escape(&w.text()))
        })
        .collect();
    let (b, e) = (clock(phrase.start().sec), clock(phrase.end().sec));
    tag("p", &[("begin", &b), ("end", &e)], &spans.join(" "))
}

/// `hh:mm:ss.mmm`
fn clock(sec: f64) -> String {
    let ms = (sec.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn word_spans_in_lines_in_paragraphs() {
        let text = write(&song());
        let doc = roxmltree::Document::parse(&text).unwrap();
        let count = |name: &str| doc.descendants().filter(|n| n.has_tag_name(name)).count();
        assert_eq!((count("div"), count("p"), count("span")), (4, 6, 12));
        assert!(text.contains(r#"<body dur="00:00:08.000">"#));
        assert!(text.contains(
            r#"<p begin="00:00:02.500" end="00:00:03.000"><span begin="00:00:02.500" end="00:00:03.000">Hi</span></p>"#
        ));
        assert_eq!(clock(3725.5), "01:02:05.500");
    }
}
