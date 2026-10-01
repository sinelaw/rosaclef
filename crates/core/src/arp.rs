//! The channel arpeggiator: how held notes become runs of notes.
//!
//! The notes of a pattern stay as written (the score and the piano roll
//! show them); the engine plays each through its channel's [`Arpeggio`]
//! with [`arpeggiate`]. A held note becomes a note every `rate` beats,
//! cycling through the chord's intervals above it over `octaves` octaves,
//! for as long as it is held; each lasts `gate × rate` (cut at the end of
//! the held note).

use crate::Arpeggio;

/// Chords an arpeggio cycles through: name and semitones above the note.
pub const CHORDS: &[(&str, &[i32])] = &[
    ("octave", &[0]),
    ("major", &[0, 4, 7]),
    ("major-b5", &[0, 4, 6]),
    ("minor", &[0, 3, 7]),
    ("minor-b5", &[0, 3, 6]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("augmented", &[0, 4, 8]),
    ("aug-sus4", &[0, 5, 8]),
    ("diminished", &[0, 3, 6, 9]),
    ("6", &[0, 4, 7, 9]),
    ("6sus4", &[0, 5, 7, 9]),
    ("6add9", &[0, 4, 7, 14]),
    ("m6", &[0, 3, 7, 9]),
    ("m6add9", &[0, 3, 7, 9, 14]),
    ("7", &[0, 4, 7, 10]),
    ("7sus4", &[0, 5, 7, 10]),
    ("7#5", &[0, 4, 8, 10]),
    ("7b5", &[0, 4, 6, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("m7", &[0, 3, 7, 10]),
];

/// Legal `direction` values.
pub const DIRECTIONS: &[&str] = &["up", "down", "updown", "downup", "random"];
/// Legal `mode` values.
pub const MODES: &[&str] = &["free", "sort"];
/// Range of `rate` in beats (a 1/256 note to a whole bar of 4/4).
pub const RATE_MIN: f64 = 1.0 / 64.0;
pub const RATE_MAX: f64 = 4.0;
/// Range of `gate` (fraction of `rate`).
pub const GATE_MIN: f64 = 0.05;
pub const GATE_MAX: f64 = 2.0;
/// Range of `octaves`.
pub const OCTAVES_MAX: u32 = 8;

/// The arpeggiator's choices and ranges, for the studio's catalog.
pub fn catalog() -> serde_json::Value {
    let chords: Vec<&str> = CHORDS.iter().map(|c| c.0).collect();
    serde_json::json!({
        "chords": chords,
        "directions": DIRECTIONS,
        "modes": MODES,
        "rateMin": RATE_MIN,
        "rateMax": RATE_MAX,
        "gateMin": GATE_MIN,
        "gateMax": GATE_MAX,
        "octavesMax": OCTAVES_MAX,
    })
}

/// The intervals of a chord name (`None` when unknown).
pub fn chord(name: &str) -> Option<&'static [i32]> {
    CHORDS.iter().find(|c| c.0 == name).map(|c| c.1)
}

/// The semitone offsets an arpeggio cycles through: the chord over its
/// octaves, lowest first.
pub fn offsets(arp: &Arpeggio) -> Vec<i32> {
    let chord = chord(&arp.chord).unwrap_or(&[0]);
    let octaves = arp.octaves.clamp(1, OCTAVES_MAX) as i32;
    (0..octaves)
        .flat_map(|o| chord.iter().map(move |k| k + 12 * o))
        .collect()
}

/// A held note, as the arpeggiator sees it (beats and semitones).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub start: f64,
    pub length: f64,
    pub pitch: i32,
}

/// A note the arpeggiator plays: `source` is the index of the held note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub source: usize,
    pub start: f64,
    pub length: f64,
    pub pitch: i32,
}

/// Index into the offsets for step `k` of a run of `n`.
fn step_index(direction: &str, k: usize, n: usize) -> usize {
    let up_down = |k: usize| {
        if n < 2 {
            0
        } else {
            let m = k % (2 * n - 2);
            if m >= n {
                2 * n - 2 - m
            } else {
                m
            }
        }
    };
    match direction {
        "down" => n - 1 - k % n,
        "updown" => up_down(k),
        "downup" => n - 1 - up_down(k),
        // A fixed hash, so playback and renders are reproducible.
        "random" => ((k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize % n,
        _ => k % n,
    }
}

/// Play `notes` (one channel's, in one pattern) through an arpeggio. In
/// "sort" mode notes that start together take turns, lowest first. Hits
/// outside the MIDI range are dropped.
pub fn arpeggiate(arp: &Arpeggio, notes: &[Held]) -> Vec<Hit> {
    let offsets = offsets(arp);
    let n = offsets.len().max(1);
    let rate = arp.rate.clamp(RATE_MIN, RATE_MAX);
    let gate = arp.gate.clamp(GATE_MIN, GATE_MAX);
    let sort = arp.mode == "sort";
    let mut out = vec![];
    for (i, note) in notes.iter().enumerate() {
        let (turn, group) = if sort {
            let mut together: Vec<(i32, usize)> = notes
                .iter()
                .enumerate()
                .filter(|(_, o)| (o.start - note.start).abs() < 1e-9)
                .map(|(j, o)| (o.pitch, j))
                .collect();
            together.sort();
            let turn = together.iter().position(|x| x.1 == i).unwrap_or(0);
            (turn, together.len())
        } else {
            (0, 1)
        };
        let end = note.start + note.length;
        let steps = (note.length / rate - 1e-9).ceil().max(1.0) as usize;
        for k in 0..steps {
            if (k % (n * group)) / n != turn {
                continue;
            }
            let start = note.start + k as f64 * rate;
            let length = (rate * gate).min(end - start);
            let pitch = note.pitch + offsets[step_index(&arp.direction, k, n)];
            if length <= 1e-6 || !(0..=127).contains(&pitch) {
                continue;
            }
            out.push(Hit {
                source: i,
                start,
                length,
                pitch,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(start: f64, length: f64, pitch: i32) -> Held {
        Held {
            start,
            length,
            pitch,
        }
    }

    fn run(arp: &Arpeggio, notes: &[Held]) -> Vec<(i32, f64, f64)> {
        arpeggiate(arp, notes)
            .iter()
            .map(|h| (h.pitch, h.start, h.length))
            .collect()
    }

    #[test]
    fn octaves_up_for_as_long_as_the_note_is_held() {
        let arp = Arpeggio {
            octaves: 2,
            rate: 0.5,
            gate: 0.5,
            ..Arpeggio::default()
        };
        assert_eq!(
            run(&arp, &[held(0.0, 2.0, 60)]),
            [
                (60, 0.0, 0.25),
                (72, 0.5, 0.25),
                (60, 1.0, 0.25),
                (72, 1.5, 0.25)
            ]
        );
    }

    #[test]
    fn directions_and_chords() {
        let arp = Arpeggio {
            chord: "major".into(),
            rate: 0.25,
            direction: "updown".into(),
            ..Arpeggio::default()
        };
        let pitches: Vec<i32> = run(&arp, &[held(0.0, 1.5, 60)])
            .iter()
            .map(|h| h.0)
            .collect();
        assert_eq!(pitches, [60, 64, 67, 64, 60, 64]);
        let down = Arpeggio {
            direction: "down".into(),
            ..arp
        };
        let pitches: Vec<i32> = run(&down, &[held(0.0, 0.75, 60)])
            .iter()
            .map(|h| h.0)
            .collect();
        assert_eq!(pitches, [67, 64, 60]);
    }

    #[test]
    fn the_last_note_is_cut_at_the_end_of_the_held_one() {
        let arp = Arpeggio {
            rate: 0.4,
            ..Arpeggio::default()
        };
        let hits = run(&arp, &[held(0.0, 1.0, 60)]);
        assert_eq!(hits.len(), 3);
        assert!((hits[2].2 - 0.2).abs() < 1e-9);
    }

    #[test]
    fn sorted_notes_take_turns() {
        let arp = Arpeggio {
            octaves: 2,
            rate: 0.25,
            mode: "sort".into(),
            ..Arpeggio::default()
        };
        // Two notes struck together: the lower runs its two octaves, then the upper.
        let hits = run(&arp, &[held(0.0, 1.0, 67), held(0.0, 1.0, 60)]);
        let seq: Vec<(i32, f64)> = hits.iter().map(|h| (h.0, h.1)).collect();
        let mut seq = seq;
        seq.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(seq, [(60, 0.0), (72, 0.25), (67, 0.5), (79, 0.75)]);
    }

    #[test]
    fn notes_out_of_range_are_dropped() {
        let arp = Arpeggio {
            octaves: 3,
            rate: 0.25,
            ..Arpeggio::default()
        };
        let hits = run(&arp, &[held(0.0, 0.75, 110)]);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), [110, 122]);
    }
}
