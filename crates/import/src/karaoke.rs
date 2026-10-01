//! Lyrics in MIDI files: `FF 05` lyric events (RP-017), or, in karaoke
//! (`.kar`) files without them, `FF 01` text events.
//!
//! - Encoding: UTF-8 when every lyric is valid UTF-8 (a byte order mark is
//!   dropped), else Windows-1252 (Latin-1 with the 0x80–0x9F extras).
//!   Shift-JIS is not recognized.
//! - Karaoke headers (`@KMIDI…`, `@T`, `@L`…) are skipped.
//! - Breaks: CR ends a line and LF a paragraph (RP-017); a leading `/` starts
//!   a line and `\` a paragraph (KAR).
//! - Words: a leading space starts a word, a trailing space ends one; a
//!   syllable without either goes on with the next one, and a trailing
//!   hyphen (an old syllable mark) is dropped and means the same.
//!
//! Each lyric is then attached to the note it sings ([`attach`]), and the
//! notes of a pattern become a verse in the lyric notation ([`notation`]):
//! `-` between the syllables of a word, `_` for a note without a lyric (it
//! holds the syllable before, as in karaoke files), `/` and `//` for breaks.

use crate::midi::Event;
use rosaclef_core::lyrics::Break;

/// A lyric event, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Lyric {
    pub text: String,
    pub starts_word: bool,
    pub ends_word: bool,
    /// A trailing hyphen: the word goes on.
    pub joins: bool,
    pub before: Break,
    pub after: Break,
}

/// A note's syllable once the lyrics around it are known.
#[derive(Clone, Debug, PartialEq)]
pub struct Syllable {
    pub text: String,
    /// The next syllable is part of the same word.
    pub joins: bool,
    /// A line or paragraph ends after it (and the notes holding it).
    pub brk: Break,
}

/// The file's lyrics: (track, tick, lyric), in each track's order.
pub fn lyrics(tracks: &[Vec<(u64, Event)>]) -> Vec<(usize, u64, Lyric)> {
    let mut raw = collect(tracks, |e| match e {
        Event::Lyric(b) => Some(b),
        _ => None,
    });
    if raw.is_empty() && is_kar(tracks) {
        raw = collect(tracks, |e| match e {
            Event::Text(b) if !b.starts_with(b"@") => Some(b),
            _ => None,
        });
    }
    let texts = decode(&raw.iter().map(|r| r.2).collect::<Vec<_>>());
    let read: Vec<(usize, u64, Lyric)> = raw
        .iter()
        .zip(texts)
        .map(|(&(track, tick, _), text)| (track, tick, lyric(&text)))
        .collect();
    merge_empty(read)
}

fn collect<'a>(
    tracks: &'a [Vec<(u64, Event)>],
    pick: impl Fn(&'a Event) -> Option<&'a Vec<u8>>,
) -> Vec<(usize, u64, &'a [u8])> {
    let mut out = vec![];
    for (t, events) in tracks.iter().enumerate() {
        for (tick, e) in events {
            if let Some(b) = pick(e) {
                out.push((t, *tick, b.as_slice()));
            }
        }
    }
    out
}

/// A karaoke file announces itself with `@` text events.
fn is_kar(tracks: &[Vec<(u64, Event)>]) -> bool {
    tracks
        .iter()
        .flatten()
        .any(|(_, e)| matches!(e, Event::Text(b) if b.starts_with(b"@")))
}

/// The texts in one encoding for the whole file.
pub fn decode(raw: &[&[u8]]) -> Vec<String> {
    let utf8 = raw.iter().all(|b| std::str::from_utf8(b).is_ok());
    raw.iter()
        .map(|b| {
            let s = if utf8 {
                String::from_utf8_lossy(b).into_owned()
            } else {
                b.iter().map(|&c| cp1252(c)).collect()
            };
            s.trim_start_matches('\u{feff}').to_string()
        })
        .collect()
}

/// A Windows-1252 byte as a character.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// One lyric event's text, read.
pub fn lyric(text: &str) -> Lyric {
    let mut s = text;
    let mut before = Break::None;
    let mut after = Break::None;
    while let Some(c) = s.chars().next().filter(|c| "\r\n/\\".contains(*c)) {
        before = stronger(before, mark(c));
        s = &s[c.len_utf8()..];
    }
    while let Some(c) = s.chars().last().filter(|c| "\r\n".contains(*c)) {
        after = stronger(after, mark(c));
        s = &s[..s.len() - c.len_utf8()];
    }
    let starts_word = s.starts_with(char::is_whitespace) || before != Break::None;
    let ends_word = s.ends_with(char::is_whitespace) || after != Break::None;
    let trimmed = s.trim();
    let joins = trimmed.ends_with('-') && trimmed.len() > 1;
    let text = if joins {
        trimmed.trim_end_matches('-').trim_end()
    } else {
        trimmed
    };
    Lyric {
        text: text.to_string(),
        starts_word,
        ends_word,
        joins,
        before,
        after,
    }
}

fn mark(c: char) -> Break {
    match c {
        '\n' | '\\' => Break::Paragraph,
        _ => Break::Line,
    }
}

fn stronger(a: Break, b: Break) -> Break {
    match (a, b) {
        (Break::Paragraph, _) | (_, Break::Paragraph) => Break::Paragraph,
        (Break::Line, _) | (_, Break::Line) => Break::Line,
        _ => Break::None,
    }
}

/// Lyrics without text (a lone CR, a space) give their breaks and word
/// ends to the lyric before them in the track (or after, at its start).
fn merge_empty(read: Vec<(usize, u64, Lyric)>) -> Vec<(usize, u64, Lyric)> {
    let mut out: Vec<(usize, u64, Lyric)> = vec![];
    let mut carry: Option<(usize, Lyric)> = None;
    for (track, tick, mut l) in read {
        if l.text.is_empty() {
            match out.last_mut().filter(|p| p.0 == track) {
                Some(prev) => {
                    prev.2.after = stronger(prev.2.after, stronger(l.before, l.after));
                    prev.2.ends_word = true;
                }
                None => carry = Some((track, l)),
            }
            continue;
        }
        if let Some((_, c)) = carry.take().filter(|c| c.0 == track) {
            l.before = stronger(l.before, stronger(c.before, c.after));
            l.starts_word = true;
        }
        out.push((track, tick, l));
    }
    out
}

/// The lyric each note sings: each lyric goes to the note starting nearest
/// its tick, at most `tolerance` ticks away (lyrics on one note are joined).
/// `starts` are the notes' start ticks, sorted; `lyrics` are in tick order.
/// Returns the lyrics per note and how many found no note.
pub fn attach(
    starts: &[u64],
    lyrics: &[(u64, Lyric)],
    tolerance: u64,
) -> (Vec<Option<Lyric>>, usize) {
    let mut out: Vec<Option<Lyric>> = vec![None; starts.len()];
    let mut missed = 0;
    for (tick, l) in lyrics {
        let first = starts.partition_point(|s| *s + tolerance < *tick);
        let near = (first..starts.len())
            .take_while(|&k| starts[k] <= tick + tolerance)
            .min_by_key(|&k| starts[k].abs_diff(*tick));
        match near {
            Some(k) => join(&mut out[k], l.clone()),
            None => missed += 1,
        }
    }
    (out, missed)
}

fn join(slot: &mut Option<Lyric>, l: Lyric) {
    match slot {
        Some(prev) => {
            prev.text.push_str(&l.text);
            prev.ends_word = l.ends_word;
            prev.joins = l.joins;
            prev.after = l.after;
        }
        None => *slot = Some(l),
    }
}

/// Syllables from a channel's lyrics, note by note: whether each goes on
/// into the next syllable's word, and the break after it.
pub fn resolve(lyrics: &[Option<Lyric>]) -> Vec<Option<Syllable>> {
    let found: Vec<usize> = (0..lyrics.len()).filter(|&i| lyrics[i].is_some()).collect();
    let mut out = vec![None; lyrics.len()];
    for (k, &i) in found.iter().enumerate() {
        let l = lyrics[i].as_ref().expect("found");
        let next = found.get(k + 1).and_then(|&j| lyrics[j].as_ref());
        let joins = next.is_some_and(|n| l.joins || !l.ends_word && !n.starts_word);
        let brk = stronger(l.after, next.map_or(Break::None, |n| n.before));
        out[i] = Some(Syllable {
            text: l.text.clone(),
            joins,
            brk,
        });
    }
    out
}

/// A pattern's notes (in time order) as a verse in the lyric notation;
/// `None` when none of them sings a syllable.
pub fn notation(notes: &[Option<&Syllable>]) -> Option<String> {
    if notes.iter().all(Option::is_none) {
        return None;
    }
    let mut out = String::new();
    let mut pending = Break::None;
    for n in notes {
        if !out.is_empty() {
            out.push_str(match n.map(|_| pending) {
                Some(Break::Line) => " / ",
                Some(Break::Paragraph) => " // ",
                _ => " ",
            });
        }
        match n {
            Some(s) => {
                out.push_str(&escape(&s.text));
                if s.joins {
                    out.push('-');
                }
                pending = s.brk;
            }
            None => out.push('_'),
        }
    }
    Some(out)
}

/// A syllable's text with the notation's own characters escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "-_/[]()\\".contains(c) || c.is_whitespace() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosaclef_core::lyrics::{parse, Sung, WordPos};

    fn l(text: &str) -> Lyric {
        lyric(text)
    }

    #[test]
    fn reads_breaks_spaces_and_hyphens() {
        let a = l("Hel-");
        assert_eq!((a.text.as_str(), a.joins), ("Hel", true));
        let b = l("lo \r");
        assert!(b.ends_word && b.after == Break::Line);
        let c = l("/Dark");
        assert!(c.starts_word && c.before == Break::Line);
        assert_eq!(l("\\Then").before, Break::Paragraph);
        assert_eq!(l(" -").text, "-", "a lone hyphen is a syllable");
    }

    #[test]
    fn decodes_utf8_or_windows_1252() {
        assert_eq!(decode(&["\u{feff}café".as_bytes()]), ["café"]);
        assert_eq!(
            decode(&[&b"caf\xe9"[..], &b"\x93hi\x94"[..]]),
            ["café", "“hi”"]
        );
    }

    #[test]
    fn attaches_to_the_nearest_note() {
        let lyrics = [(0, l("Hel")), (2, l("lo ")), (480, l("there"))];
        let (out, missed) = attach(&[0, 240, 960], &lyrics, 30);
        assert_eq!(missed, 1, "nothing near tick 480");
        assert_eq!(out[0].as_ref().unwrap().text, "Hello", "joined on one note");
        assert!(out[1].is_none());
    }

    #[test]
    fn notes_become_a_verse() {
        let lyrics: Vec<Option<Lyric>> = ["Hel", "", "lo\r", "dark-", "ness ", "", "/my"]
            .iter()
            .map(|t| (!t.is_empty()).then(|| l(t)))
            .collect();
        let syl = resolve(&lyrics);
        let refs: Vec<Option<&Syllable>> = syl.iter().map(Option::as_ref).collect();
        let text = notation(&refs).unwrap();
        assert_eq!(text, "Hel- _ lo / dark- ness _ / my");
        let tokens = parse(&text).unwrap();
        assert_eq!(tokens.len(), 7);
        assert!(matches!(
            &tokens[0].sung,
            Sung::Syllable {
                pos: WordPos::Begin,
                ..
            }
        ));
        assert!(matches!(
            &tokens[2].sung,
            Sung::Syllable {
                pos: WordPos::End,
                ..
            }
        ));
        assert_eq!(tokens[2].brk, Break::Line);
        assert_eq!(notation(&[None, None]), None);
        assert_eq!(escape("a-b c"), "a\\-b\\ c");
    }

    #[test]
    fn lone_breaks_join_their_neighbours() {
        let read = vec![(1, 0, l("Hi")), (1, 5, l("\r")), (1, 10, l("there"))];
        let merged = merge_empty(read);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].2.after, Break::Line);
        assert!(merged[0].2.ends_word);
    }
}
