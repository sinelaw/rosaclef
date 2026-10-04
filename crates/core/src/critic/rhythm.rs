//! Rhythm and MIDI hygiene: velocities, timing, overlapping, duplicate,
//! silent and stray notes, and swing.

use super::analysis::*;
use serde_json::json;

/// Velocity with an accent for where the note falls: downbeats strongest,
/// off-beat 8ths softer, 16ths softest, plus a little variation.
fn accented(base: f64, start: f64, bar: f64, i: usize) -> f64 {
    let in_beat = start - (start + 1e-6).floor();
    let on_bar = (start / bar - (start / bar).round()).abs() < 1e-4;
    let w = if !(0.01..=0.99).contains(&in_beat) {
        if on_bar {
            1.0
        } else {
            0.92
        }
    } else if (in_beat - 0.5).abs() < 0.01 {
        0.82
    } else {
        0.72
    };
    r3((base * w + jitter(i, 3) * 0.035).clamp(0.05, 1.0))
}

fn on_grid(t: f64) -> bool {
    (t * 4.0 - (t * 4.0).round()).abs() < 1e-4
}

pub fn check_rhythm(a: &mut Ana) {
    let bar = a.bar_beats();
    for part in a.parts.clone() {
        let ch = a.ch(&part).clone();
        if ch.role == "fx" || ch.role == "gen" {
            continue;
        }
        let pat = a.pattern(&part);
        let notes = &pat.notes;
        let pi = part.pat;
        let idx = &part.idx;
        let vels: Vec<f64> = idx.iter().map(|&i| notes[i].velocity).collect();
        let vmin = vels.iter().copied().fold(f64::INFINITY, f64::min);
        let vmax = vels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // Held chords and pads may keep one velocity; struck and rhythmic parts should not.
        let mut lens: Vec<f64> = idx.iter().map(|&i| notes[i].length).collect();
        lens.sort_by(f64::total_cmp);
        let rhythmic = ch.role == "drums" || lens[lens.len() / 2] <= 1.0;
        let flat = idx.len() >= 8 && vmax - vmin < 0.02 && rhythmic;
        let velocity_ops = |which: &[usize], f: &dyn Fn(usize) -> f64| {
            which
                .iter()
                .map(|&i| set(note_path(pi, i, "velocity"), json!(f(i))))
                .collect::<Vec<_>>()
        };

        if flat {
            let base = if vmax >= 0.95 { 0.85 } else { vmax };
            let ops = velocity_ops(idx, &|i| accented(base, notes[i].start, bar, i));
            let at = a.at_part(&part, idx.clone());
            a.add(
                "flat-velocity",
                "warn",
                format!("All {} notes at velocity {}", idx.len(), (vmax * 127.0).round()),
                if ch.role == "drums" {
                    format!(
                        "{} has no dynamics: accent the downbeats and soften the off-beats (a kick on the one about 100–110, ghost notes 40–60).",
                        ch.name
                    )
                } else {
                    format!("{} has no dynamics: a player leans into the strong beats and plays the passing notes softer.", ch.name)
                },
                at,
                fix("Add dynamics (accents and a little variation)", ops),
            );
        } else if idx.len() >= 8
            && vels.iter().filter(|&&v| v >= 0.97).count() as f64 >= 0.9 * idx.len() as f64
        {
            let ops = velocity_ops(idx, &|i| r3(notes[i].velocity * 0.8));
            let at = a.at_part(&part, idx.clone());
            a.add(
                "max-velocity",
                "warn",
                "Nearly every note at full velocity".into(),
                format!("{} leaves no room for accents. Bring the part down and let the strong notes stand out.", ch.name),
                at,
                fix("Scale the velocities to 80%", ops),
            );
        }

        if ch.role == "drums" && !flat {
            // Runs of 8+ fast hits of one drum at one velocity.
            let mut runs = vec![];
            let mut s = 0;
            for k in 1..=idx.len() {
                let same = k < idx.len()
                    && (notes[idx[k]].velocity - notes[idx[s]].velocity).abs() < 0.01
                    && notes[idx[k]].start - notes[idx[k - 1]].start <= 0.26
                    && notes[idx[k]].pitch == notes[idx[s]].pitch;
                if same {
                    continue;
                }
                if k - s >= 8 {
                    runs.extend_from_slice(&idx[s..k]);
                }
                s = k;
            }
            if !runs.is_empty() {
                let ops = velocity_ops(&runs, &|i| {
                    accented(notes[i].velocity.min(0.9), notes[i].start, bar, i)
                });
                let at = a.at_part(&part, runs.clone());
                a.add(
                    "machine-gun",
                    "info",
                    format!("{} fast hits at one velocity", runs.len()),
                    format!(
                        "{} has runs of eight or more 16ths that all hit equally hard.",
                        ch.name
                    ),
                    at,
                    fix("Alternate strong and weak hits", ops),
                );
            }
            let snare = ch.drum == "snare"
                || ch.drum == "hat"
                || (ch.kind == "soundfont"
                    && idx.iter().any(|&i| [38, 40, 42].contains(&notes[i].pitch)));
            if snare && idx.len() >= 8 && vmin >= 0.6 {
                let at = a.at_part(&part, vec![]);
                a.note(
                    "no-ghost-notes",
                    "info",
                    format!("No ghost notes in {}", ch.name),
                    format!(
                        "Every hit is at velocity {} or more; soft in-between hits (velocity 40–60) add groove.",
                        (vmin * 127.0).round()
                    ),
                    at,
                );
            }
        }

        // Timing.
        let gridded = idx.iter().filter(|&&i| on_grid(notes[i].start)).count();
        let live = crate::gm::range(&ch.program).is_some();
        if live && idx.len() >= 16 && gridded == idx.len() {
            let beats_per_ms = a.p.transport.bpm / 60000.0;
            let mut ops = vec![];
            for &i in idx {
                let n = &notes[i];
                let d = jitter(i, 7) * 8.0 * beats_per_ms;
                let s = r3((n.start + d).max(0.0));
                ops.push(set(note_path(pi, i, "start"), json!(s)));
                ops.push(set(
                    note_path(pi, i, "length"),
                    json!((n.length - (s - n.start)).max(0.01)),
                ));
            }
            let at = a.at_part(&part, vec![]);
            a.add(
                "rigid-timing",
                "info",
                format!("{} is exactly on the grid", ch.name),
                format!(
                    "A {} played by a person drifts by a few milliseconds around the beat. Humanizing nudges each note by up to ±8 ms.",
                    ch.program.to_lowercase()
                ),
                at,
                fix("Humanize the timing (±8 ms)", ops),
            );
        }
        if idx.len() >= 8 && gridded as f64 >= 0.8 * idx.len() as f64 && gridded < idx.len() {
            let slips: Vec<usize> = idx
                .iter()
                .copied()
                .filter(|&i| {
                    let t = notes[i].start;
                    let d = (t * 4.0 - (t * 4.0).round()).abs() / 4.0;
                    d > 1e-4 && d <= 0.03
                })
                .collect();
            if !slips.is_empty() && slips.len() + gridded == idx.len() {
                let ops = slips
                    .iter()
                    .map(|&i| {
                        set(
                            note_path(pi, i, "start"),
                            json!((notes[i].start * 4.0).round() / 4.0),
                        )
                    })
                    .collect();
                let at = a.at_part(&part, slips.clone());
                a.add(
                    "sloppy-timing",
                    "info",
                    format!("{} a hair off the grid", plural(slips.len(), "note is", "notes are")),
                    format!("{} is otherwise quantized to 16ths; these land up to 3% of a beat away from it.", ch.name),
                    at,
                    fix("Snap them to the grid", ops),
                );
            }
        }

        // Hygiene: overlaps, duplicates, silent, tiny and unreachable notes.
        let mut by_pitch = idx.clone();
        by_pitch.sort_by(|&x, &y| {
            let (m, n) = (&notes[x], &notes[y]);
            m.pitch
                .cmp(&n.pitch)
                .then(m.start.total_cmp(&n.start))
                .then(n.velocity.total_cmp(&m.velocity))
        });
        let mut dupes = vec![];
        let mut trims = vec![];
        for k in 1..by_pitch.len() {
            let (m, n) = (&notes[by_pitch[k - 1]], &notes[by_pitch[k]]);
            if m.pitch != n.pitch {
                continue;
            }
            if (n.start - m.start).abs() < 0.005 {
                dupes.push(by_pitch[k]);
            } else if ch.role != "drums" && n.start < m.start + m.length - 1e-6 {
                trims.push((by_pitch[k - 1], n.start - m.start));
            }
        }
        if !dupes.is_empty() {
            let at = a.at_part(&part, dupes.clone());
            a.add(
                "duplicate-notes",
                "warn",
                plural(dupes.len(), "duplicate note", "duplicate notes"),
                format!("{} has notes stacked on top of identical ones.", ch.name),
                at,
                fix(
                    "Remove the duplicates",
                    remove_all(&format!("/patterns/{pi}/notes"), dupes.clone()),
                ),
            );
        }
        if !trims.is_empty() {
            let ops = trims
                .iter()
                .map(|&(i, len)| set(note_path(pi, i, "length"), json!(len)))
                .collect();
            let at = a.at_part(&part, trims.iter().map(|t| t.0).collect());
            a.add(
                "same-pitch-overlap",
                "warn",
                format!(
                    "{} the next note of the same pitch",
                    plural(trims.len(), "note overlaps", "notes overlap")
                ),
                format!(
                    "In {} a note is still sounding when the same pitch starts again.",
                    ch.name
                ),
                at,
                fix("Trim each to where the next begins", ops),
            );
        }
        let silent: Vec<usize> = idx
            .iter()
            .copied()
            .filter(|&i| notes[i].velocity <= 0.01)
            .collect();
        if !silent.is_empty() {
            let at = a.at_part(&part, silent.clone());
            a.add(
                "silent-notes",
                "warn",
                plural(silent.len(), "silent note", "silent notes"),
                format!("{} has notes at velocity 0.", ch.name),
                at,
                fix(
                    "Remove them",
                    remove_all(&format!("/patterns/{pi}/notes"), silent.clone()),
                ),
            );
        }
        if ch.pitched {
            let tiny: Vec<usize> = idx
                .iter()
                .copied()
                .filter(|&i| notes[i].length < 1.0 / 16.0)
                .collect();
            if !tiny.is_empty() {
                let ops = tiny
                    .iter()
                    .map(|&i| set(note_path(pi, i, "length"), json!(0.25)))
                    .collect();
                let at = a.at_part(&part, tiny.clone());
                a.add(
                    "tiny-notes",
                    "info",
                    plural(tiny.len(), "very short note", "very short notes"),
                    format!(
                        "{} has notes shorter than a 64th — usually accidental clicks.",
                        ch.name
                    ),
                    at,
                    fix("Lengthen them to a 16th", ops),
                );
            }
        }
        let late: Vec<usize> = idx
            .iter()
            .copied()
            .filter(|&i| notes[i].start >= pat.length - 1e-9)
            .collect();
        if !late.is_empty() {
            let at = a.at_part(&part, late.clone());
            a.add(
                "past-end",
                "warn",
                format!(
                    "{} after the pattern ends",
                    plural(late.len(), "note starts", "notes start")
                ),
                format!(
                    "{} is {} beats long; these notes of {} never play.",
                    pat.name,
                    num(pat.length),
                    ch.name
                ),
                at,
                fix(
                    "Remove them",
                    remove_all(&format!("/patterns/{pi}/notes"), late.clone()),
                ),
            );
        }
    }

    // Swing with nothing on the off-beat 16ths.
    let sw = a.p.transport.swing;
    if sw > 0.02 && !a.parts.is_empty() {
        let odd = a.parts.iter().any(|part| {
            part.idx.iter().any(|&i| {
                let f = a.pattern(part).notes[i].start * 4.0;
                (f - f.round()).abs() < 0.05 && (f.round() as i64).rem_euclid(2) == 1
            })
        });
        if !odd {
            a.note(
                "swing-unused",
                "info",
                "Swing has nothing to swing".into(),
                format!("Swing is at {}%, but no note falls on an off-beat 16th, which is what it delays.", percent(sw)),
                at_project("Transport"),
            );
        }
    }
}
