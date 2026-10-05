//! Harmonic clashes: notes of two parts sounding together a minor second
//! (or minor ninth, …), a major seventh or a tritone apart — read from the
//! notes as they play in the range, and weighed by how long they overlap
//! and how loud the quieter part really is at that moment (its measured
//! share of the mix).

use super::timeline::Timeline;
use rosaclef_core::Project;

/// A note as it plays (performance beats), and where it is written.
#[derive(Clone, Debug)]
pub struct Played {
    pub channel: usize,
    pub pattern: usize,
    pub note: usize,
    /// Sounding pitch.
    pub pitch: i32,
    pub from: f64,
    pub to: f64,
    pub span: usize,
    /// Written beat of `from`.
    pub beat: f64,
    pub length: f64,
}

#[derive(Clone, Debug)]
pub struct Clash {
    pub a: Played,
    pub b: Played,
    /// Semitones apart.
    pub semis: i32,
    pub from: f64,
    pub overlap: f64,
    /// The quieter part's level relative to the mix during the overlap (dB).
    pub quiet_db: f64,
    pub a_db: f64,
    pub b_db: f64,
    pub weight: f64,
    pub severity: &'static str,
    /// Other places the same two notes clash: (bar, pass).
    pub also: Vec<(u32, u32)>,
}

pub const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn note_name(p: i32) -> String {
    format!(
        "{}{}",
        NAMES[p.rem_euclid(12) as usize],
        p.div_euclid(12) - 1
    )
}

pub fn interval_name(semis: i32) -> String {
    let s = semis.abs();
    let base = match s % 12 {
        1 => ["m2", "m9", "m16"],
        6 => ["TT", "aug11", "aug18"],
        11 => ["M7", "M14", "M21"],
        _ => ["?", "?", "?"],
    };
    let oct = (s / 12) as usize;
    if oct < 3 {
        base[oct].to_string()
    } else {
        format!("{}+{}oct", base[0], oct)
    }
}

fn pitched(p: &Project, c: usize) -> bool {
    let ch = &p.channels[c];
    !rosaclef_core::drums::is_drum_channel(ch)
        && !matches!(ch.instrument.kind.as_str(), "transition" | "generative")
}

/// The notes sounding in the range, in performance order (layers'
/// inherited notes and drums left out).
pub fn played(p: &Project, t: &Timeline, inside: &[(f64, f64)]) -> Vec<Played> {
    let mut out = vec![];
    let muted_track = |i: usize| p.playlist.tracks.get(i).map(|x| x.mute).unwrap_or(false);
    let ch_index = |id: &str| p.channels.iter().position(|c| c.id == id);
    for (si, span) in t.spans.iter().enumerate() {
        for clip in &p.playlist.clips {
            if clip.pattern.is_empty() || muted_track(clip.track.index()) {
                continue;
            }
            let (c0, c1) = (clip.start, clip.start + clip.length);
            if c1 <= span.start || c0 >= span.end {
                continue;
            }
            let Some(pi) = p.patterns.iter().position(|x| x.id == clip.pattern) else {
                continue;
            };
            let pat = &p.patterns[pi];
            if pat.length <= 0.0 {
                continue;
            }
            let base = c0 - clip.offset;
            let k0 = (((span.start.max(c0) - base) / pat.length).floor() as i64 - 64).max(0);
            let k1 = ((span.end.min(c1) - base) / pat.length).floor() as i64;
            for k in k0..=k1 {
                let origin = base + k as f64 * pat.length;
                for (ni, n) in pat.notes.iter().enumerate() {
                    if n.start >= pat.length {
                        continue;
                    }
                    let w0 = origin + n.start;
                    if w0 < c0 || w0 >= c1 {
                        continue;
                    }
                    let w1 = w0 + n.length.min(c1 - w0);
                    // The part of it inside this span and the range.
                    let (a, b) = (w0.max(span.start), w1.min(span.end));
                    if b <= a {
                        continue;
                    }
                    if !inside.iter().any(|(x, y)| a < *y && b > *x) {
                        continue;
                    }
                    let Some(c) = ch_index(&n.channel) else {
                        continue;
                    };
                    let ch = &p.channels[c];
                    if ch.mute || !pitched(p, c) {
                        continue;
                    }
                    let shift = p.transport.transpose
                        + if ch.instrument.kind == "soundfont" {
                            ch.instrument.param("transpose").round() as i32
                        } else {
                            0
                        };
                    out.push(Played {
                        channel: c,
                        pattern: pi,
                        note: ni,
                        pitch: n.pitch + shift,
                        from: t.perf_of(si, a),
                        to: t.perf_of(si, b),
                        span: si,
                        beat: a,
                        length: n.length,
                    });
                }
            }
        }
    }
    out.sort_by(|x, y| x.from.total_cmp(&y.from).then(x.pitch.cmp(&y.pitch)));
    out
}

/// The clashes among `notes`, with the level of each part from the mix.
pub fn find(
    notes: &[Played],
    t: &Timeline,
    level_db: &dyn Fn(usize, f64, f64) -> Option<f64>,
    strict: bool,
    loose: bool,
) -> Vec<Clash> {
    let mut out: Vec<Clash> = vec![];
    let mut open: Vec<usize> = vec![];
    for (j, n) in notes.iter().enumerate() {
        open.retain(|i| notes[*i].to > n.from + 1e-9);
        for &i in &open {
            let m = &notes[i];
            if m.channel == n.channel {
                continue;
            }
            let semis = (m.pitch - n.pitch).abs();
            // Further apart than two octaves, the notes no longer beat.
            if semis > 24 || !matches!(semis % 12, 1 | 6 | 11) {
                continue;
            }
            let from = m.from.max(n.from);
            let overlap = m.to.min(n.to) - from;
            if overlap <= 1e-6 {
                continue;
            }
            // Passing tones: a short note brushing past another.
            let short = m.length.min(n.length);
            let pass_limit = if loose { 0.5 } else { 0.25 };
            if !strict && short <= pass_limit + 1e-9 && overlap <= pass_limit + 1e-9 {
                continue;
            }
            let (Some(da), Some(db)) = (
                level_db(m.channel, from, from + overlap),
                level_db(n.channel, from, from + overlap),
            ) else {
                continue;
            };
            let quiet = da.min(db);
            let weight = overlap * 10f64.powf(quiet / 20.0);
            // Minor seconds and ninths grind; a major seventh over a chord's
            // root is a maj7 colour as often as a clash, and the tritone a
            // dominant's: those rank one step lower.
            let semitone = semis % 12 == 1;
            let shift = if strict {
                6.0
            } else if loose {
                -4.0
            } else {
                0.0
            };
            let mut sev = if quiet >= -18.0 - shift && overlap >= 1.0 - 1e-9 {
                2
            } else if quiet >= -26.0 - shift && overlap >= 0.5 - 1e-9 {
                1
            } else {
                0
            };
            if !semitone && sev > 0 {
                sev -= 1;
            }
            let (hi, lo, hd, ld) = if m.pitch >= n.pitch {
                (m.clone(), n.clone(), da, db)
            } else {
                (n.clone(), m.clone(), db, da)
            };
            out.push(Clash {
                a: hi,
                b: lo,
                semis,
                from,
                overlap,
                quiet_db: quiet,
                a_db: hd,
                b_db: ld,
                weight,
                severity: ["low", "medium", "high"][sev],
                also: vec![],
            });
        }
        open.push(j);
    }
    // The same two notes clashing again (a looping pattern) are one clash.
    let id = |c: &Clash| (c.a.pattern, c.a.note, c.b.pattern, c.b.note);
    let mut merged: Vec<Clash> = vec![];
    out.sort_by(|x, y| {
        y.weight
            .total_cmp(&x.weight)
            .then(x.from.total_cmp(&y.from))
    });
    for c in out {
        let place = (t.bar_of(c.a.beat.max(c.b.beat)), 0);
        match merged.iter_mut().find(|m| id(m) == id(&c)) {
            Some(m) => {
                let (bar, pass) = (place.0, t.pass_of(c.a.span, c.a.beat));
                if !m.also.contains(&(bar, pass)) {
                    m.also.push((bar, pass));
                }
                m.weight += c.weight;
            }
            None => merged.push(c),
        }
    }
    merged.sort_by(|x, y| {
        let rank = |s: &str| match s {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        };
        rank(x.severity)
            .cmp(&rank(y.severity))
            .then(y.weight.total_cmp(&x.weight))
            .then(x.from.total_cmp(&y.from))
    });
    merged
}
