//! Timing from a recording: what a forced aligner (Montreal Forced Aligner,
//! SOFA, WhisperX) found in a sung take becomes the fixed phoneme timing
//! of a pattern's lyric line.
//!
//! - [`textgrid`] and [`whisperx`] read the aligners' files ([`read`]).
//! - [`timing`] pairs the aligned words with the line's words in order
//!   ([`matching`]), shares each word's phones out over its syllables, and
//!   gives each syllable its phonemes with their offsets from its note's
//!   start, in seconds. Without phones (WhisperX), a syllable starts with
//!   its word (or, with letter times, at its first letter) and its
//!   phonemes follow [`crate::timing::from_start`].
//! - [`apply`] writes them into the project, replacing what those notes had.
//!
//! The pattern is taken to play at the project tempo from [`Target::at`].

mod matching;
pub mod textgrid;
pub mod whisperx;

use crate::timing;
use matching::{normalize, pair_words, split};
use rosaclef_core::expand::{self, Sounding};
use rosaclef_core::lyrics::{Sung, WordPos};
use rosaclef_core::{Project, SyllableTiming, TimedPhoneme};
use rosaclef_phonetics::{alphabet, ipa};

/// A labeled stretch of a recording, in seconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interval {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// What an aligner found, in seconds from the start of the recording:
/// words, and phones or letters when it gives them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aligned {
    pub words: Vec<Interval>,
    pub phones: Vec<Interval>,
    pub letters: Vec<Interval>,
}

/// The lyric line a recording sings, and where in it the pattern starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub pattern: String,
    pub channel: String,
    pub verse: u32,
    /// Seconds into the recording where the pattern's beat 0 falls.
    pub at: f64,
}

/// An aligner's file, read by its name: a `.TextGrid`, or WhisperX `.json`.
pub fn read(name: &str, text: &str) -> Result<Aligned, String> {
    let lower = name.to_lowercase();
    if lower.ends_with(".textgrid") {
        textgrid::parse(text)
    } else if lower.ends_with(".json") {
        whisperx::parse(text)
    } else {
        Err(format!(
            "{name}: expected a .TextGrid or a WhisperX .json file"
        ))
    }
}

/// The fixed timing the alignment gives the line's syllables.
pub fn timing(
    p: &Project,
    target: &Target,
    aligned: &Aligned,
) -> Result<Vec<SyllableTiming>, String> {
    let line = written(p, target)?;
    let words = words(&line);
    let texts: Vec<String> = words
        .iter()
        .map(|w| normalize(&w.iter().map(|&k| line[k].text.as_str()).collect::<String>()))
        .collect();
    let heard: Vec<String> = aligned.words.iter().map(|w| normalize(&w.text)).collect();
    Ok(pair_words(&texts, &heard)
        .into_iter()
        .flat_map(|(i, j)| time_word(&line, &words[i], &aligned.words[j], aligned))
        .collect())
}

/// The project with the alignment's timing in the line, and how many
/// syllables it timed.
pub fn apply(p: &Project, target: &Target, aligned: &Aligned) -> Result<(Project, usize), String> {
    let fixed = timing(p, target, aligned)?;
    let (index, line) = locate(p, target)?;
    let mut out = p.clone();
    let list = &mut out.patterns[index].lyrics[line].timing;
    list.retain(|t| {
        !fixed
            .iter()
            .any(|f| f.verse == t.verse && (f.at - t.at).abs() < 1e-6)
    });
    let count = fixed.len();
    list.extend(fixed);
    list.sort_by(|a, b| a.verse.cmp(&b.verse).then(a.at.total_cmp(&b.at)));
    Ok((out, count))
}

/// The pattern and its lyric line on the channel.
fn locate(p: &Project, target: &Target) -> Result<(usize, usize), String> {
    let index = p
        .patterns
        .iter()
        .position(|x| x.id == target.pattern)
        .ok_or_else(|| format!("no pattern {:?}", target.pattern))?;
    let line = p.patterns[index]
        .lyrics
        .iter()
        .position(|l| l.channel == target.channel)
        .ok_or_else(|| {
            format!(
                "pattern {:?} has no lyrics on {:?}",
                target.pattern, target.channel
            )
        })?;
    Ok((index, line))
}

/// A syllable as the line writes it, and where its note is.
struct Written {
    verse: u32,
    /// The note's beat in the pattern.
    at: f64,
    /// The note's start in the recording, in seconds.
    start: f64,
    text: String,
    pos: WordPos,
    /// IPA, from the dictionary or the lyrics.
    phonemes: Vec<String>,
}

/// The line's syllables, in order (not those its uses bring).
fn written(p: &Project, target: &Target) -> Result<Vec<Written>, String> {
    let (index, line) = locate(p, target)?;
    let notes: Vec<Sounding> = expand::pattern(p, index, target.verse)
        .into_iter()
        .filter(|n| n.channel == target.channel)
        .collect();
    let refs: Vec<&Sounding> = notes.iter().collect();
    let phonemes = rosaclef_phonetics::pronounce_notes(p, &refs);
    let spb = p.seconds_per_beat();
    Ok(notes
        .iter()
        .zip(phonemes)
        .filter_map(|(n, phonemes)| {
            let l = n
                .lyric
                .as_ref()
                .filter(|l| (l.line.pattern, l.line.index) == (index, line))?;
            let Sung::Syllable { text, pos, .. } = &l.token.sung else {
                return None;
            };
            Some(Written {
                verse: l.line.verse,
                at: n.start,
                start: target.at + n.start * spb,
                text: text.clone(),
                pos: *pos,
                phonemes,
            })
        })
        .collect())
}

/// The line's words: indices of their syllables.
fn words(line: &[Written]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = vec![];
    for (k, s) in line.iter().enumerate() {
        match out.last_mut() {
            Some(w) if !matches!(s.pos, WordPos::Single | WordPos::Begin) => w.push(k),
            _ => out.push(vec![k]),
        }
    }
    out
}

/// The timing of one word's syllables from the word heard.
fn time_word(
    line: &[Written],
    word: &[usize],
    heard: &Interval,
    aligned: &Aligned,
) -> Vec<SyllableTiming> {
    let phones = phones_in(&aligned.phones, heard);
    let timed = if phones.is_empty() {
        by_rule(line, word, heard, &aligned.letters)
    } else {
        by_phones(line, word, &phones)
    };
    word.iter()
        .zip(timed)
        .filter(|(_, t)| !t.is_empty())
        .map(|(&k, t)| SyllableTiming {
            verse: line[k].verse,
            at: line[k].at,
            phonemes: t
                .into_iter()
                .map(|(p, start)| TimedPhoneme {
                    p,
                    offset: ((start - line[k].start) * 1000.0).round() / 1000.0,
                })
                .collect(),
        })
        .collect()
}

/// The phones (IPA, start) heard inside a word.
fn phones_in(phones: &[Interval], word: &Interval) -> Vec<(String, f64)> {
    phones
        .iter()
        .filter(|p| (word.start..word.end).contains(&((p.start + p.end) / 2.0)))
        .map(|p| (to_ipa(&p.text), p.start))
        .collect()
}

/// An aligner's phone label in IPA: ARPAbet (MFA's English models) is
/// converted, anything else is taken as IPA.
fn to_ipa(label: &str) -> String {
    let label = label.trim();
    match alphabet::from_arpabet(label).filter(|_| label.is_ascii()) {
        Some(p) => p.to_string(),
        None => ipa::normalize(label),
    }
}

/// The word's phones shared out over its syllables.
fn by_phones(
    line: &[Written],
    word: &[usize],
    phones: &[(String, f64)],
) -> Vec<Vec<(String, f64)>> {
    let names: Vec<String> = phones.iter().map(|p| p.0.clone()).collect();
    let expected: Vec<Vec<String>> = word.iter().map(|&k| line[k].phonemes.clone()).collect();
    let starts = split(&names, &expected);
    (0..word.len())
        .map(|s| {
            let end = starts.get(s + 1).copied().unwrap_or(phones.len());
            phones[starts[s].min(end)..end].to_vec()
        })
        .collect()
}

/// Without phones: each syllable starts with its word, its first letter or
/// its note (kept inside the word), and its phonemes follow the rule.
fn by_rule(
    line: &[Written],
    word: &[usize],
    heard: &Interval,
    letters: &[Interval],
) -> Vec<Vec<(String, f64)>> {
    let starts = syllable_starts(line, word, heard, letters);
    (0..word.len())
        .map(|s| {
            let end = starts.get(s + 1).copied().unwrap_or(heard.end);
            let ph = &line[word[s]].phonemes;
            let at = timing::from_start(ph, starts[s], end);
            ph.iter().cloned().zip(at).collect()
        })
        .collect()
}

fn syllable_starts(
    line: &[Written],
    word: &[usize],
    heard: &Interval,
    letters: &[Interval],
) -> Vec<f64> {
    let inside: Vec<&Interval> = letters
        .iter()
        .filter(|l| !l.text.trim().is_empty() && (heard.start..heard.end).contains(&l.start))
        .collect();
    let lengths: Vec<usize> = word.iter().map(|&k| line[k].text.chars().count()).collect();
    let by_letters = inside.len() >= lengths.iter().sum::<usize>();
    let first = line[word[0]].start;
    let mut offset = 0;
    word.iter()
        .zip(lengths)
        .map(|(&k, len)| {
            let t = if by_letters {
                inside[offset].start
            } else {
                (heard.start + line[k].start - first).clamp(heard.start, heard.end)
            };
            offset += len;
            t
        })
        .collect()
}

#[cfg(test)]
mod tests;
