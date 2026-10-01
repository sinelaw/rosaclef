//! Checks for lyric lines (`patterns[].lyrics`): the text reads, the verses
//! fit the notes, one voice sings one note at a time.

use super::V;
use crate::expand;
use crate::lyrics::{self, Lyrics};
use crate::model::Project;

pub(super) fn check(v: &mut V, p: &Project) {
    for (i, pat) in p.patterns.iter().enumerate() {
        for (k, line) in pat.lyrics.iter().enumerate() {
            let path = format!("patterns[{i}].lyrics[{k}]");
            if p.channel(&line.channel).is_none() {
                v.err(
                    format!("{path}.channel"),
                    format!("unknown channel {:?}", line.channel),
                );
                continue;
            }
            if !line.lang.is_empty() && !valid_lang(&line.lang) {
                v.err(
                    format!("{path}.lang"),
                    format!(
                        "{:?} is not a language tag such as en, en-US or ja",
                        line.lang
                    ),
                );
            }
            check_verses(v, p, &path, i, k);
            check_timing(v, &path, line);
            check_one_voice(v, p, &path, i, line);
        }
    }
}

fn check_verses(v: &mut V, p: &Project, path: &str, pattern: usize, index: usize) {
    let line = &p.patterns[pattern].lyrics[index];
    if line.verses.is_empty() {
        v.err(
            format!("{path}.verses"),
            "a lyric line needs at least one verse",
        );
    }
    for (&n, text) in &line.verses {
        let vp = format!("{path}.verses.{n}");
        if n == 0 {
            v.err(&vp, "verses are numbered from 1");
            continue;
        }
        if let Err(e) = lyrics::parse(text) {
            v.err(&vp, e.to_string());
            continue;
        }
        let (tokens, notes) = expand::fit(p, pattern, index, n);
        if tokens > notes {
            v.warn(
                &vp,
                format!(
                    "{tokens} syllables for {notes} notes on {:?}: the last {} are not sung",
                    line.channel,
                    tokens - notes
                ),
            );
        } else if tokens < notes && tokens > 0 {
            v.warn(
                &vp,
                format!(
                    "{tokens} syllables for {notes} notes on {:?}: {} notes have no words (use _ to hold a syllable)",
                    line.channel,
                    notes - tokens
                ),
            );
        }
    }
}

fn check_timing(v: &mut V, path: &str, line: &Lyrics) {
    for (j, t) in line.timing.iter().enumerate() {
        let tp = format!("{path}.timing[{j}]");
        if !line.verses.contains_key(&t.verse) {
            v.err(
                format!("{tp}.verse"),
                format!("there is no verse {}", t.verse),
            );
        }
        if t.phonemes.is_empty() {
            v.err(format!("{tp}.phonemes"), "list the syllable's phonemes");
        }
        for (q, ph) in t.phonemes.iter().enumerate() {
            v.range(&format!("{tp}.phonemes[{q}].offset"), ph.offset, -2.0, 30.0);
        }
    }
}

/// A sung channel holds one note at a time (no singing format has chords).
fn check_one_voice(v: &mut V, p: &Project, path: &str, pattern: usize, line: &Lyrics) {
    let notes = expand::pattern(p, pattern, 1);
    let mut end = f64::NEG_INFINITY;
    for n in notes.iter().filter(|n| n.channel == line.channel) {
        if n.start < end - 1e-6 {
            v.warn(
                format!("{path}.channel"),
                format!(
                    "notes overlap on {:?} at beat {}: a voice sings one note at a time",
                    line.channel, n.start
                ),
            );
            return;
        }
        end = n.start + n.length;
    }
}

/// A BCP 47 tag, loosely: a 2–3 letter language and optional subtags.
fn valid_lang(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let lang = parts.next().unwrap_or("");
    (2..=3).contains(&lang.len())
        && lang.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|s| (1..=8).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric()))
}
