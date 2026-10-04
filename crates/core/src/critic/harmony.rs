//! Harmony and melody: the key, voicings, voice leading, clashes between
//! parts, melodic lines and real instruments' ranges.

use super::analysis::*;
use serde_json::json;

// Krumhansl–Kessler key profiles.
const MAJOR_PROFILE: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// How well a pitch-class histogram fits a key profile rotated to `key` (Pearson).
fn correlate(hist: &[f64; 12], profile: &[f64; 12], key: i32) -> f64 {
    let mh = hist.iter().sum::<f64>() / 12.0;
    let mp = profile.iter().sum::<f64>() / 12.0;
    let (mut num, mut dh, mut dp) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let h = hist[(i + key as usize) % 12] - mh;
        let q = profile[i] - mp;
        num += h * q;
        dh += h * h;
        dp += q * q;
    }
    if dh > 0.0 && dp > 0.0 {
        num / (dh * dp).sqrt()
    } else {
        0.0
    }
}

/// Parse a key name ("F#m", "Bb") into a tonic pitch class and mode.
fn parse_key(name: &str) -> (i32, bool) {
    let mut chars = name.chars();
    let letter = chars.next().unwrap_or('C').to_ascii_uppercase();
    let mut tonic = match letter {
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => 0,
    };
    let mut rest = chars.as_str();
    if let Some(r) = rest.strip_prefix('#') {
        tonic += 1;
        rest = r;
    } else if let Some(r) = rest.strip_prefix('b') {
        tonic += 11;
        rest = r;
    }
    (pc(tonic), rest == "m")
}

/// The pitch classes a key allows (minor keys also take the raised 6th and 7th).
pub fn key_steps(tonic: i32, minor: bool) -> Vec<i32> {
    let steps: &[i32] = if minor {
        &[0, 2, 3, 5, 7, 8, 9, 10, 11]
    } else {
        &[0, 2, 4, 5, 7, 9, 11]
    };
    steps.iter().map(|s| pc(tonic + s)).collect()
}

fn key_text(tonic: i32, minor: bool) -> String {
    format!(
        "{} {}",
        KEY_NAMES[pc(tonic) as usize],
        if minor { "minor" } else { "major" }
    )
}

// Keys as the score names them (F# over Gb, Db over C#).
const MAJOR_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
const MINOR_NAMES: [&str; 12] = [
    "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm",
];

/// Find the key: the score's when it names one (unless the notes clearly
/// disagree), else the best Krumhansl–Schmuckler fit.
pub fn find_key(a: &mut Ana) {
    let mut hist = [0.0; 12];
    let mut total = 0;
    for part in &a.parts {
        let ch = a.ch(part);
        if !ch.pitched {
            continue;
        }
        for &i in &part.idx {
            let n = &a.pattern(part).notes[i];
            hist[pc(n.pitch + ch.shift) as usize] += n.length.clamp(0.05, 4.0);
            total += 1;
        }
    }
    if total == 0 {
        return;
    }
    let mut best = -2.0;
    for k in 0..12 {
        let maj = correlate(&hist, &MAJOR_PROFILE, k);
        if maj > best {
            best = maj;
            a.tonic = k;
            a.minor = false;
        }
        let min = correlate(&hist, &MINOR_PROFILE, k);
        if min > best {
            best = min;
            a.tonic = k;
            a.minor = true;
        }
    }
    a.fit = best;
    a.key_label = key_text(a.tonic, a.minor);
    let named = a.p.score.key.clone();
    if named.is_empty() || named == "auto" {
        return;
    }
    let (tonic, minor) = parse_key(&named);
    let fit = correlate(
        &hist,
        if minor {
            &MINOR_PROFILE
        } else {
            &MAJOR_PROFILE
        },
        tonic,
    );
    if fit < best - 0.15 {
        let (t, m) = (a.tonic, a.minor);
        let name = if m {
            MINOR_NAMES[t as usize]
        } else {
            MAJOR_NAMES[t as usize]
        };
        a.add(
            "key-signature",
            "info",
            format!("The notes read as {}, not {}", key_text(t, m), key_text(tonic, minor)),
            format!(
                "The score is set to {}, but the notes fit {} much better (key-profile correlation {}% against {}%).",
                key_text(tonic, minor),
                key_text(t, m),
                percent(best),
                percent(fit)
            ),
            at_project("Score key"),
            fix(format!("Set the key to {}", key_text(t, m)), vec![set("/score/key".into(), json!(name))]),
        );
    } else {
        a.tonic = tonic;
        a.minor = minor;
        a.fit = fit;
        a.key_label = key_text(tonic, minor);
        a.key_from_score = true;
    }
}

// ------------------------------------------------------------------ voicings

// The lowest pitch where the lower note of each interval (semitones) still sounds clear.
const LOW_LIMITS: [i32; 15] = [0, 52, 51, 48, 46, 46, 47, 34, 43, 41, 41, 41, 0, 40, 39];

/// The first adjacent pair (its lower note's index) under its low interval limit.
fn low_limit_hit(ps: &[i32]) -> Option<usize> {
    (0..ps.len().saturating_sub(1)).find(|&j| {
        let iv = ps[j + 1] - ps[j];
        iv > 0
            && (iv as usize) < LOW_LIMITS.len()
            && LOW_LIMITS[iv as usize] > 0
            && ps[j] < LOW_LIMITS[iv as usize]
    })
}

fn interval_name(iv: i32) -> String {
    const NAMES: [&str; 15] = [
        "unison",
        "minor 2nd",
        "major 2nd",
        "minor 3rd",
        "major 3rd",
        "4th",
        "tritone",
        "5th",
        "minor 6th",
        "major 6th",
        "minor 7th",
        "major 7th",
        "octave",
        "minor 9th",
        "major 9th",
    ];
    NAMES
        .get(iv as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{iv} semitones"))
}

fn mean(xs: &[i32]) -> f64 {
    xs.iter().sum::<i32>() as f64 / xs.len().max(1) as f64
}

/// Choose a voicing of `pitches` near `prev`: an inversion (the k lowest
/// notes up an octave) and an octave shift. Returns the shift of each note
/// (low to high).
fn nearest_voicing(pitches: &[i32], prev: &[i32]) -> Vec<i32> {
    let n = pitches.len();
    let mut best = vec![0; n];
    let mut best_cost = f64::INFINITY;
    for k in 0..n {
        for s in [0, -12, 12, -24, 24] {
            let d: Vec<i32> = (0..n).map(|j| if j < k { 12 } else { 0 } + s).collect();
            let mut v: Vec<i32> = pitches.iter().zip(&d).map(|(x, y)| x + y).collect();
            if v.iter().min().copied().unwrap_or(0) < 36
                || v.iter().max().copied().unwrap_or(0) > 96
            {
                continue;
            }
            v.sort();
            let mut cost = movement(prev, &v);
            if !(k == 0 && s == 0) {
                cost += 0.5;
            }
            if cost < best_cost - 1e-9 {
                best_cost = cost;
                best = d;
            }
        }
    }
    best
}

/// How far the voices move from one chord to the next (semitones).
fn movement(a: &[i32], b: &[i32]) -> f64 {
    if a.len() == b.len() {
        return a.iter().zip(b).map(|(x, y)| (x - y).abs() as f64).sum();
    }
    (mean(a) - mean(b)).abs() * a.len().max(b.len()) as f64
}

/// Is this a part of power chords (only roots, fifths and octaves)?
fn power_chords(chords: &[&Chord]) -> bool {
    chords.iter().all(|c| {
        c.pitches
            .iter()
            .all(|x| [0, 7].contains(&pc(x - c.pitches[0])))
    })
}

fn chord_notes(cs: &[&Chord]) -> Vec<usize> {
    cs.iter().flat_map(|c| c.idx.iter().copied()).collect()
}

pub fn check_harmony(a: &mut Ana) {
    let has_bass = a.chans.iter().any(|c| c.role == "bass");
    for part in a.parts.clone() {
        let ch = a.ch(&part).clone();
        if !ch.pitched {
            continue;
        }
        let chords = a.chords(&part);
        let pi = part.pat;
        let notes = &a.pattern(&part).notes;

        // Chords below C3 while a bass plays: lift the low notes (leave the root to the bass).
        let low_chords: Vec<&Chord> = if has_bass && ch.role != "bass" {
            chords
                .iter()
                .filter(|c| {
                    c.pitches.len() >= 3 && c.pitches.iter().filter(|&&x| x < 48).count() >= 2
                })
                .collect()
        } else {
            vec![]
        };
        if !low_chords.is_empty() {
            let idx = chord_notes(&low_chords);
            let mut ops = vec![];
            for &i in &idx {
                let mut p = notes[i].pitch;
                while p + ch.shift < 48 {
                    p += 12;
                }
                if p != notes[i].pitch {
                    ops.push(set(note_path(pi, i, "pitch"), json!(p)));
                }
            }
            let lowest = low_chords.iter().map(|c| c.pitches[0]).min().unwrap_or(0);
            let at = a.at_part(&part, idx);
            a.add(
                "chord-too-low",
                "warn",
                format!("{} in the bass register", plural(low_chords.len(), "chord sits", "chords sit")),
                format!("{} plays chords with two or more notes under C3 (the lowest is {}), where the bass lives.", ch.name, note_name(lowest)),
                at,
                fix("Raise the notes under C3 an octave", ops),
            );
        }

        // Low interval limits (chords not already lifted above).
        let mut muddy: Vec<&Chord> = vec![];
        let mut worst = String::new();
        for c in &chords {
            if c.pitches.len() < 2 || low_chords.contains(&c) {
                continue;
            }
            let Some(j) = low_limit_hit(&c.pitches) else {
                continue;
            };
            if c.pitches[c.pitches.len() - 1] + 12 > 108 {
                continue;
            }
            muddy.push(c);
            if worst.is_empty() {
                worst = format!(
                    "a {} on {}",
                    interval_name(c.pitches[j + 1] - c.pitches[j]),
                    note_name(c.pitches[j])
                );
            }
        }
        if !muddy.is_empty() {
            let ops = muddy
                .iter()
                .flat_map(|c| {
                    c.idx[1..]
                        .iter()
                        .map(|&i| set(note_path(pi, i, "pitch"), json!(notes[i].pitch + 12)))
                })
                .collect();
            let at = a.at_part(&part, chord_notes(&muddy));
            a.add(
                "low-interval",
                "warn",
                format!("{} voiced below the low interval limits", plural(muddy.len(), "chord is", "chords are")),
                format!(
                    "{} has close intervals too low to sound clear (first: {worst}). Spreading the voicing keeps the bass note and lifts the rest.",
                    ch.name
                ),
                at,
                fix("Open the voicing (upper notes up an octave)", ops),
            );
        }

        if ch.role != "harmony" {
            continue;
        }
        let big: Vec<&Chord> = chords.iter().filter(|c| c.pitches.len() >= 3).collect();

        // Voice leading: jumps between consecutive chords of three or more notes.
        let mut jumps = 0;
        let mut before = 0.0;
        for w in big.windows(2) {
            let (p, n) = (&w[0].pitches, &w[1].pitches);
            if p == n {
                continue;
            }
            before += movement(p, n);
            if (n[n.len() - 1] - p[p.len() - 1]).abs() >= 9 || (mean(n) - mean(p)).abs() >= 7.0 {
                jumps += 1;
            }
        }
        if jumps > 0 {
            // Each chord as near the previous (smoothed) one as it can be.
            let mut prev = big[0].pitches.clone();
            let mut after = 0.0;
            let mut ops = vec![];
            for i in 1..big.len() {
                let d = nearest_voicing(&big[i].pitches, &prev);
                let mut v: Vec<i32> = big[i].pitches.iter().zip(&d).map(|(x, y)| x + y).collect();
                v.sort();
                if big[i].pitches != big[i - 1].pitches {
                    after += movement(&prev, &v);
                }
                for (j, &note_i) in big[i].idx.iter().enumerate() {
                    if d[j] != 0 {
                        ops.push(set(
                            note_path(pi, note_i, "pitch"),
                            json!(notes[note_i].pitch + d[j]),
                        ));
                    }
                }
                prev = v;
            }
            if after < before * 0.75 {
                let at = a.at_part(&part, chord_notes(&big));
                a.add(
                    "voice-leading",
                    "info",
                    format!("{} instead of moving smoothly", plural(jumps, "chord change jumps", "chord changes jump")),
                    format!(
                        "In {} the voices leap between chords ({} semitones of movement in all). Inversions bring it to {}.",
                        ch.name,
                        before.round(),
                        after.round()
                    ),
                    at,
                    fix("Revoice with the nearest inversions", ops),
                );
            }
        }

        // Parallel fifths and octaves: in acoustic parts (synth stacks fuse on purpose), not power chords.
        if ch.kind == "soundfont" && !power_chords(&big) {
            let mut par = 0;
            let mut first = 0;
            for i in 1..big.len() {
                let (p, n) = (&big[i - 1].pitches, &big[i].pitches);
                if p.len() != n.len() || p == n {
                    continue;
                }
                for x in 0..p.len() {
                    for y in x + 1..p.len() {
                        let (i1, i2) = (p[y] - p[x], n[y] - n[x]);
                        let kind = pc(i1);
                        if i1 != i2 || (kind != 7 && !(kind == 0 && i1 > 0)) {
                            continue;
                        }
                        let mx = n[x] - p[x];
                        if mx == 0 || mx.signum() != (n[y] - p[y]).signum() {
                            continue;
                        }
                        par += 1;
                        if first == 0 {
                            first = i;
                        }
                    }
                }
            }
            if par > 0 {
                let mut idx = big[first].idx.clone();
                idx.extend(&big[first - 1].idx);
                let at = a.at_part(&part, idx);
                a.note(
                    "parallel-fifths",
                    "info",
                    format!("{} in {}", plural(par, "parallel fifth or octave", "parallel fifths and octaves"), ch.name),
                    format!(
                        "Voices move in parallel perfect intervals (first at beat {}). Fine for power chords and synth stabs; avoid it when the voices should sound independent.",
                        num(big[first].start)
                    ),
                    at,
                );
            }
        }

        // Upper voices more than an octave apart.
        let gaps: Vec<&Chord> = big
            .iter()
            .copied()
            .filter(|c| {
                (1..c.pitches.len().saturating_sub(1)).any(|j| c.pitches[j + 1] - c.pitches[j] > 12)
            })
            .collect();
        if !gaps.is_empty() {
            let mut ops = vec![];
            let mut idx = vec![];
            for c in &gaps {
                let mut v = c.pitches.clone();
                for j in 1..v.len() - 1 {
                    let mut drop = 0;
                    while v[j + 1] - drop - v[j] > 12 {
                        drop += 12;
                    }
                    for x in v.iter_mut().skip(j + 1) {
                        *x -= drop;
                    }
                }
                for (j, &i) in c.idx.iter().enumerate() {
                    idx.push(i);
                    let d = v[j] - c.pitches[j];
                    if d != 0 {
                        ops.push(set(note_path(pi, i, "pitch"), json!(notes[i].pitch + d)));
                    }
                }
            }
            let at = a.at_part(&part, idx);
            a.add(
                "wide-spacing",
                "info",
                format!("{} a gap over an octave in the upper voices", plural(gaps.len(), "chord has", "chords have")),
                format!("{} leaves more than an octave between adjacent upper voices; close voicings blend better.", ch.name),
                at,
                fix("Close the gaps", ops),
            );
        }
    }
}

/// Notes outside the key, part by part (when the song as a whole is clearly in it).
pub fn check_key(a: &mut Ana) {
    if a.key_label.is_empty() || a.fit < 0.6 {
        return;
    }
    let steps = key_steps(a.tonic, a.minor);
    let mut flagged = vec![];
    let (mut total, mut outside) = (0, 0);
    for part in &a.parts {
        let ch = a.ch(part);
        if !ch.pitched {
            continue;
        }
        let idx: Vec<usize> = part
            .idx
            .iter()
            .copied()
            .filter(|&i| !steps.contains(&pc(a.sounding(part, i))))
            .collect();
        total += part.idx.len();
        outside += idx.len();
        if !idx.is_empty() && (idx.len() <= 2 || idx.len() as f64 / part.idx.len() as f64 <= 0.05) {
            flagged.push((part.clone(), idx));
        }
    }
    // Lots of chromatic notes: a style, not slips.
    if total == 0 || outside as f64 / total as f64 > 0.08 {
        return;
    }
    for (part, idx) in flagged {
        let notes = &a.pattern(&part).notes;
        let shift = a.ch(&part).shift;
        let names: Vec<String> = idx
            .iter()
            .map(|&i| note_name(notes[i].pitch + shift))
            .collect();
        let ops = idx
            .iter()
            .filter_map(|&i| {
                let s = notes[i].pitch + shift;
                // The nearer neighbour in the key, the lower one on a tie.
                let to = if steps.contains(&pc(s - 1)) {
                    notes[i].pitch - 1
                } else if steps.contains(&pc(s + 1)) {
                    notes[i].pitch + 1
                } else {
                    return None;
                };
                Some(set(note_path(part.pat, i, "pitch"), json!(to)))
            })
            .collect();
        let name = a.ch(&part).name.clone();
        let at = a.at_part(&part, idx.clone());
        let key = a.key_label.clone();
        a.add(
            "out-of-key",
            "warn",
            format!(
                "{} outside {key}",
                plural(idx.len(), "note is", "notes are")
            ),
            format!(
                "{name}: {}. {} {key}; the rest of the song stays in it.",
                list(&names, 4),
                if a.key_from_score {
                    "The score is in"
                } else {
                    "The song reads as"
                }
            ),
            at,
            fix("Snap them to the nearest note of the key", ops),
        );
    }
}

// ------------------------------------------------------------------ clashes

/// Overlap between two channel groups, summed (beats), from `at`.
struct Pair {
    a: usize,
    b: usize,
    beats: f64,
    at: f64,
}

/// Sum the overlaps of notes that `pick` keeps, by pair of channel groups,
/// when `clash` says they clash.
fn overlaps(
    a: &Ana,
    notes: &[SNote],
    pick: impl Fn(&SNote) -> bool,
    clash: impl Fn(&SNote, &SNote) -> bool,
) -> Vec<Pair> {
    let mut pairs: Vec<Pair> = vec![];
    let mut active: Vec<&SNote> = vec![];
    for n in notes.iter().filter(|n| pick(n)) {
        active.retain(|m| m.end > n.start + 1e-9);
        let g = a.group_of(n.channel);
        for m in &active {
            let h = a.group_of(m.channel);
            if g == h || !clash(m, n) {
                continue;
            }
            let ov = m.end.min(n.end) - n.start;
            if ov <= 0.0 {
                continue;
            }
            let (x, y) = (g.min(h), g.max(h));
            match pairs.iter_mut().find(|q| q.a == x && q.b == y) {
                Some(pr) => pr.beats += ov,
                None => pairs.push(Pair {
                    a: x,
                    b: y,
                    beats: ov,
                    at: n.start,
                }),
            }
        }
        active.push(n);
    }
    pairs.sort_by(|x, y| y.beats.total_cmp(&x.beats));
    pairs
}

pub fn check_clashes(a: &mut Ana) {
    for notes in a.scopes() {
        let melodic = |n: &SNote| {
            let c = &a.chans[n.channel];
            c.pitched && c.role != "fx"
        };
        let semis = overlaps(a, &notes, melodic, |m, n| {
            [1, 13, 25].contains(&(m.pitch - n.pitch).abs()) && m.end.min(n.end) - n.start >= 1.0
        });
        let subs = overlaps(a, &notes, |n| melodic(n) && n.pitch < 43, |_, _| true);
        for pr in semis.iter().take(3).filter(|p| p.beats >= 2.0) {
            let at = a.at_notes(&notes, pr.at);
            a.note(
                "semitone-clash",
                "info",
                format!("{} and {} hold notes a semitone apart", a.chans[pr.a].name, a.chans[pr.b].name),
                format!(
                    "For {} beats in all they sustain minor 2nds or 9ths against each other (first around bar {}). Move one of them a semitone, or keep the clash short.",
                    num(pr.beats),
                    a.bar_of(pr.at) + 1
                ),
                at,
            );
        }
        for pr in subs.iter().take(2).filter(|p| p.beats >= 4.0) {
            let at = a.at_notes(&notes, pr.at);
            a.note(
                "low-crowding",
                "warn",
                format!("{} and {} share the sub", a.chans[pr.a].name, a.chans[pr.b].name),
                format!(
                    "Both play under G2 at the same time for {} beats (first around bar {}). Give the low end to one and move the other up an octave.",
                    num(pr.beats),
                    a.bar_of(pr.at) + 1
                ),
                at,
            );
        }
    }
}

// ------------------------------------------------------------------ melody

/// A note of a part's top line.
struct Line {
    pitch: i32,
    start: f64,
    end: f64,
    i: usize,
}

/// A part's top line: its highest note at each onset.
fn line_of(a: &Ana, part: &Part) -> Vec<Line> {
    let notes = &a.pattern(part).notes;
    a.chords(part)
        .into_iter()
        .map(|c| {
            let k = c.pitches.len() - 1;
            let n = &notes[c.idx[k]];
            Line {
                pitch: c.pitches[k],
                start: c.start,
                end: n.start + n.length,
                i: c.idx[k],
            }
        })
        .collect()
}

pub fn check_melody(a: &mut Ana) {
    let bar = a.bar_beats();
    for part in a.parts.clone() {
        let ch = a.ch(&part).clone();
        let pat = a.pattern(&part);

        // Real instruments' ranges.
        if let Some((lo, hi)) = crate::gm::range(&ch.program) {
            let idx: Vec<usize> = part
                .idx
                .iter()
                .copied()
                .filter(|&i| {
                    let s = a.sounding(&part, i);
                    s < lo || s > hi
                })
                .collect();
            if !idx.is_empty() {
                let outs: Vec<i32> = idx.iter().map(|&i| a.sounding(&part, i)).collect();
                let far = if outs.iter().any(|&x| x < lo) {
                    *outs.iter().min().unwrap()
                } else {
                    *outs.iter().max().unwrap()
                };
                let ops = idx
                    .iter()
                    .filter_map(|&i| {
                        let s = a.sounding(&part, i);
                        let mut d = 0;
                        while s + d < lo && s + d + 12 <= hi {
                            d += 12;
                        }
                        while s + d > hi && s + d - 12 >= lo {
                            d -= 12;
                        }
                        (d != 0).then(|| {
                            set(
                                note_path(part.pat, i, "pitch"),
                                json!(pat.notes[i].pitch + d),
                            )
                        })
                    })
                    .collect();
                let instrument = ch.program.to_lowercase();
                let at = a.at_part(&part, idx.clone());
                a.add(
                    "instrument-range",
                    "warn",
                    format!(
                        "{} out of the {instrument}'s range",
                        plural(idx.len(), "note is", "notes are")
                    ),
                    format!(
                        "A {instrument} plays {}–{}; {} goes to {} in {}.",
                        note_name(lo),
                        note_name(hi),
                        ch.name,
                        note_name(far),
                        pat.name
                    ),
                    at,
                    fix("Move them into range by octaves", ops),
                );
            }
        }

        if ch.role != "lead" {
            continue;
        }
        let line = line_of(a, &part);
        if line.len() < 4 {
            continue;
        }
        let lo = line.iter().min_by_key(|l| l.pitch).unwrap();
        let hi = line.iter().max_by_key(|l| l.pitch).unwrap();
        if hi.pitch - lo.pitch > 19 {
            let at = a.at_part(&part, vec![lo.i, hi.i]);
            a.note(
                "melody-range",
                "info",
                format!("The melody spans {} semitones", hi.pitch - lo.pitch),
                format!(
                    "{} runs from {} to {} — more than an octave and a half; singable melodies keep to about an octave.",
                    ch.name,
                    note_name(lo.pitch),
                    note_name(hi.pitch)
                ),
                at,
            );
        }
        let (mut big, mut loose) = (vec![], vec![]);
        for k in 1..line.len() {
            if line[k].start - line[k - 1].end > bar {
                continue;
            }
            let iv = line[k].pitch - line[k - 1].pitch;
            if iv.abs() > 12 {
                big.push(line[k].i);
            }
            if iv.abs() >= 8 && k + 1 < line.len() {
                let next = line[k + 1].pitch - line[k].pitch;
                if next.signum() == iv.signum() || next.abs() > 4 {
                    loose.push(line[k].i);
                }
            }
        }
        if !big.is_empty() {
            let at = a.at_part(&part, big.clone());
            a.note(
                "large-leap",
                "info",
                format!(
                    "{} wider than an octave",
                    plural(big.len(), "leap", "leaps")
                ),
                format!(
                    "{} jumps more than an octave between consecutive notes.",
                    ch.name
                ),
                at,
            );
        }
        if loose.len() >= 2 {
            let at = a.at_part(&part, loose.clone());
            a.note(
                "leap-recovery",
                "info",
                format!("{} not balanced by a step back", plural(loose.len(), "big leap is", "big leaps are")),
                format!("In {}, leaps of a minor 6th or more keep going (or leap again); a step back the other way rounds them off.", ch.name),
                at,
            );
        }
        // The longest stretch without a rest.
        let mut run: f64 = 0.0;
        let mut since = line[0].start;
        for k in 1..line.len() {
            if line[k].start - line[k - 1].end > 0.124 {
                since = line[k].start;
            }
            run = run.max(line[k].end - since);
        }
        if run >= 8.0 * bar {
            let at = a.at_part(&part, vec![]);
            a.note(
                "no-rests",
                "info",
                format!("{} plays {} bars without a rest", ch.name, (run / bar).round()),
                "Phrases breathe: leave rests between them (two- and four-bar phrases are the norm).".into(),
                at,
            );
        }
        let span = line[line.len() - 1].end - line[0].start;
        let mut classes: Vec<i32> = line.iter().map(|l| pc(l.pitch)).collect();
        classes.sort();
        classes.dedup();
        if line.len() >= 16 && span >= 4.0 * bar && classes.len() <= 2 {
            let at = a.at_part(&part, vec![]);
            a.note(
                "monotone",
                "info",
                format!(
                    "{} keeps to {}",
                    ch.name,
                    plural(classes.len(), "note", "notes")
                ),
                format!(
                    "{} notes over {} bars use only {}.",
                    line.len(),
                    (span / bar).round(),
                    plural(classes.len(), "pitch class", "pitch classes")
                ),
                at,
            );
        }
    }
}
