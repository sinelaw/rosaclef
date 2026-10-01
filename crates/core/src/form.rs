//! The form of the song: the order in which the arrangement plays once its
//! repeats are taken, the way a musician reads repeat signs and endings.
//!
//! The playlist is the written score. A [`Repeat`] sends playing back to its
//! start at its end, `times` times in all; on each pass, an ending (volta)
//! that does not list that pass is skipped. The last pass goes on past the
//! end repeat sign — through the ending for that pass, which is usually
//! written after it — and the song continues.
//!
//! [`performance`] unrolls this into spans of written time (beats), played
//! one after the other; without repeats it is the whole song in one span.

use crate::automation::TempoMap;
use crate::model::{Ending, Project, Repeat};

/// A stretch of written time `[start, end)` (beats), played straight through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

const EPS: f64 = 1e-9;

/// The end of the written arrangement: the last clip, or a repeat or ending past it.
pub fn written_end(p: &Project) -> f64 {
    let mut end = p.song_length();
    for r in &p.repeats {
        end = end.max(r.end);
        for e in &r.endings {
            end = end.max(e.end);
        }
    }
    end
}

/// Whether a repeat is well formed enough to play (validation reports the rest).
fn playable(r: &Repeat) -> bool {
    r.start.is_finite() && r.end.is_finite() && r.start >= 0.0 && r.end > r.start + EPS
}

fn push(out: &mut Vec<Span>, start: f64, end: f64) {
    if end <= start + EPS {
        return;
    }
    // Contiguous spans join: no jump between them.
    if let Some(last) = out.last_mut() {
        if (last.end - start).abs() < EPS {
            last.end = end;
            return;
        }
    }
    out.push(Span { start, end });
}

/// Play `[a, b)` but skip the given endings (sorted by start).
fn play_except(out: &mut Vec<Span>, a: f64, b: f64, skip: &[&Ending]) {
    let mut t = a;
    for e in skip {
        if e.end <= t + EPS || e.start >= b - EPS {
            continue;
        }
        push(out, t, e.start.max(t));
        t = t.max(e.end);
    }
    push(out, t, b);
}

/// The order the song plays in: spans of written time, one after another.
pub fn performance(p: &Project) -> Vec<Span> {
    let song_end = written_end(p);
    let mut repeats: Vec<&Repeat> = p.repeats.iter().filter(|r| playable(r)).collect();
    repeats.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out = vec![];
    let mut cursor = 0.0f64;
    for r in repeats {
        if r.end <= cursor + EPS {
            continue; // overlaps an earlier repeat (validation reports it)
        }
        push(&mut out, cursor, r.start.max(cursor));
        let start = r.start.max(cursor);
        let times = r.times.clamp(1, 99);
        let mut endings: Vec<&Ending> =
            r.endings.iter().filter(|e| e.end > e.start + EPS).collect();
        endings.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut after = r.end;
        for pass in 1..=times {
            let skip: Vec<&Ending> = endings
                .iter()
                .copied()
                .filter(|e| !e.passes.contains(&pass))
                .collect();
            play_except(&mut out, start, r.end, &skip);
            if pass == times {
                // Past the end sign, the endings of other passes are skipped.
                for e in &endings {
                    if e.start >= after - EPS && e.start <= after + EPS && !e.passes.contains(&pass)
                    {
                        after = e.end;
                    }
                }
            }
        }
        cursor = after;
    }
    push(&mut out, cursor, song_end);
    out
}

/// Beats the song plays for, repeats taken.
pub fn performance_beats(p: &Project) -> f64 {
    performance(p).iter().map(|s| s.end - s.start).sum()
}

/// Seconds the song plays for, repeats taken (following tempo automation,
/// which is written time like the notes).
pub fn performance_seconds(p: &Project, tempo: &TempoMap) -> f64 {
    performance(p)
        .iter()
        .map(|s| tempo.seconds_at(s.end) - tempo.seconds_at(s.start))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Clip, TrackIx};

    fn song(beats: f64) -> Project {
        let mut p = Project::empty("t");
        p.playlist.clips.push(Clip {
            pattern: "pattern-1".into(),
            sample: String::new(),
            track: TrackIx(0),
            start: 0.0,
            length: beats,
            offset: 0.0,
            gain: 1.0,
            mixer: Default::default(),
        });
        p
    }

    fn spans(p: &Project) -> Vec<(f64, f64)> {
        performance(p).iter().map(|s| (s.start, s.end)).collect()
    }

    #[test]
    fn without_repeats_the_song_plays_through() {
        assert_eq!(spans(&song(32.0)), vec![(0.0, 32.0)]);
    }

    #[test]
    fn a_repeat_plays_its_passage_again() {
        let mut p = song(32.0);
        p.repeats.push(Repeat {
            start: 8.0,
            end: 16.0,
            times: 2,
            endings: vec![],
        });
        assert_eq!(spans(&p), vec![(0.0, 16.0), (8.0, 32.0)]);
        assert_eq!(performance_beats(&p), 40.0);
        p.repeats[0].times = 3;
        assert_eq!(spans(&p), vec![(0.0, 16.0), (8.0, 16.0), (8.0, 32.0)]);
    }

    #[test]
    fn first_and_second_endings() {
        // |: A A A [1. B :| [2. C | D
        let mut p = song(24.0);
        p.repeats.push(Repeat {
            start: 0.0,
            end: 16.0,
            times: 2,
            endings: vec![
                Ending {
                    start: 12.0,
                    end: 16.0,
                    passes: vec![1],
                },
                Ending {
                    start: 16.0,
                    end: 20.0,
                    passes: vec![2],
                },
            ],
        });
        assert_eq!(spans(&p), vec![(0.0, 16.0), (0.0, 12.0), (16.0, 24.0)]);
    }

    #[test]
    fn an_ending_for_two_passes_then_a_third() {
        // |: A [1.–2. B :| [3. C — three times.
        let mut p = song(16.0);
        p.repeats.push(Repeat {
            start: 0.0,
            end: 8.0,
            times: 3,
            endings: vec![
                Ending {
                    start: 4.0,
                    end: 8.0,
                    passes: vec![1, 2],
                },
                Ending {
                    start: 8.0,
                    end: 12.0,
                    passes: vec![3],
                },
            ],
        });
        assert_eq!(
            spans(&p),
            vec![(0.0, 8.0), (0.0, 8.0), (0.0, 4.0), (8.0, 16.0)]
        );
    }

    #[test]
    fn performance_length_follows_the_tempo() {
        let mut p = song(16.0);
        p.transport.bpm = 120.0;
        p.repeats.push(Repeat {
            start: 0.0,
            end: 8.0,
            times: 2,
            endings: vec![],
        });
        let secs = performance_seconds(&p, &TempoMap::new(&p));
        assert!((secs - 12.0).abs() < 1e-9, "{secs}");
    }
}
