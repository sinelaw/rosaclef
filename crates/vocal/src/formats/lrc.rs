//! LRC with enhanced word timing: a `[mm:ss.xx]` line per lyric line, a
//! `<mm:ss.xx>` before each word and one after the last word (karaoke
//! players, DiffRhythm).

use super::paragraphs;
use crate::line::Song;
use crate::words::Phrase;

pub fn write(song: &Song) -> String {
    let mut out = String::new();
    for (tag, value) in [("ti", &song.title), ("ar", &song.author)] {
        if !value.is_empty() {
            out += &format!("[{tag}:{value}]\n");
        }
    }
    for phrase in paragraphs(song).iter().flatten() {
        out += &line(phrase);
    }
    out
}

/// `[01:02.50]<01:02.50>Hello <01:03.00>world<01:04.00>`
fn line(phrase: &Phrase) -> String {
    let words: Vec<String> = phrase
        .words
        .iter()
        .map(|w| format!("<{}>{}", clock(w.start().sec), w.text()))
        .collect();
    format!(
        "[{}]{}<{}>\n",
        clock(phrase.start().sec),
        words.join(" "),
        clock(phrase.end().sec)
    )
}

/// `mm:ss.xx`
fn clock(sec: f64) -> String {
    let cs = (sec.max(0.0) * 100.0).round() as u64;
    format!("{:02}:{:02}.{:02}", cs / 6000, cs % 6000 / 100, cs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::song;

    #[test]
    fn lines_with_word_times() {
        let text = write(&song());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "[ti:Song]");
        assert_eq!(lines[1], "[ar:Ann]");
        assert_eq!(
            lines[2],
            "[00:00.00]<00:00.00>Hello <00:01.00>dark <00:01.50>friend<00:02.00>"
        );
        assert_eq!(lines[3], "[00:02.50]<00:02.50>Hi<00:03.00>");
        assert_eq!(lines.len(), 8);
        assert_eq!(clock(61.237), "01:01.24");
    }
}
