//! Praat TextGrids (the long text format), as Montreal Forced Aligner and
//! SOFA write them: the `words` and `phones` interval tiers (by name, or
//! the first and second interval tiers).

use super::{Aligned, Interval};

/// An interval tier being read.
#[derive(Default)]
struct Tier {
    name: String,
    points: bool,
    intervals: Vec<Interval>,
}

pub fn parse(text: &str) -> Result<Aligned, String> {
    if !text.contains("ooTextFile") || !text.contains("intervals [") {
        return Err("not a TextGrid in the long text format".into());
    }
    let mut tiers: Vec<Tier> = vec![];
    for line in text.lines().map(str::trim) {
        if line.starts_with("item [") && !line.starts_with("item []") {
            tiers.push(Tier::default());
            continue;
        }
        let Some(tier) = tiers.last_mut() else {
            continue;
        };
        if line.starts_with("intervals [") {
            tier.intervals.push(Interval::default());
        } else if let Some(v) = value(line, "class") {
            tier.points = unquote(v) != "IntervalTier";
        } else if let Some(v) = value(line, "name") {
            tier.name = unquote(v).to_lowercase();
        } else if let Some(last) = tier.intervals.last_mut() {
            read_field(last, line)?;
        }
    }
    let tiers: Vec<Tier> = tiers.into_iter().filter(|t| !t.points).collect();
    let pick = |names: &[&str], fallback: usize| {
        tiers
            .iter()
            .position(|t| names.contains(&t.name.as_str()))
            .or((tiers.len() > fallback).then_some(fallback))
            .map(|i| spoken(&tiers[i].intervals))
            .unwrap_or_default()
    };
    Ok(Aligned {
        words: pick(&["words", "word"], 0),
        phones: pick(&["phones", "phone", "phonemes"], 1),
        letters: vec![],
    })
}

/// `xmin`, `xmax` and `text` of an interval.
fn read_field(i: &mut Interval, line: &str) -> Result<(), String> {
    let number = |v: &str| {
        v.trim()
            .parse::<f64>()
            .map_err(|_| format!("a bad time in the TextGrid: {line:?}"))
    };
    if let Some(v) = value(line, "xmin") {
        i.start = number(v)?;
    } else if let Some(v) = value(line, "xmax") {
        i.end = number(v)?;
    } else if let Some(v) = value(line, "text") {
        i.text = unquote(v);
    }
    Ok(())
}

/// The value of a `key = value` line.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?.trim_start();
    Some(rest.strip_prefix('=')?.trim())
}

/// A quoted TextGrid string (`""` inside is a quote).
fn unquote(v: &str) -> String {
    let v = v.trim();
    let inner = v
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(v);
    inner.replace("\"\"", "\"")
}

/// Labels aligners give silences and breaths.
const SILENT: &[&str] = &["", "sil", "sp", "spn", "SP", "AP", "<eps>", "<sil>"];

fn spoken(intervals: &[Interval]) -> Vec<Interval> {
    intervals
        .iter()
        .filter(|i| !SILENT.contains(&i.text.trim()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: &str = r#"File type = "ooTextFile"
Object class = "TextGrid"

xmin = 0
xmax = 1.2
tiers? <exists>
size = 2
item []:
    item [1]:
        class = "IntervalTier"
        name = "words"
        xmin = 0
        xmax = 1.2
        intervals: size = 3
        intervals [1]:
            xmin = 0
            xmax = 0.2
            text = ""
        intervals [2]:
            xmin = 0.2
            xmax = 0.9
            text = "hello"
        intervals [3]:
            xmin = 0.9
            xmax = 1.2
            text = "say ""hi"""
    item [2]:
        class = "IntervalTier"
        name = "phones"
        xmin = 0
        xmax = 1.2
        intervals: size = 2
        intervals [1]:
            xmin = 0.2
            xmax = 0.3
            text = "HH"
        intervals [2]:
            xmin = 0.3
            xmax = 0.5
            text = "AH0"
"#;

    #[test]
    fn reads_words_and_phones() {
        let a = parse(GRID).unwrap();
        let words: Vec<&str> = a.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(words, ["hello", "say \"hi\""]);
        assert_eq!((a.words[0].start, a.words[0].end), (0.2, 0.9));
        assert_eq!(a.phones.len(), 2);
        assert_eq!(a.phones[1].text, "AH0");
        assert!(parse("hello").is_err());
    }
}
