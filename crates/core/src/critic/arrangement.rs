//! The song's form: the playlist, clips, repetition, density over time, and
//! project hygiene (tempo, pattern lengths, unused channels, names).

use super::analysis::*;
use serde_json::json;

/// For each bar: the channel groups that sound in it (drums count as one,
/// transitions not at all).
fn bar_elements(a: &Ana) -> Vec<Vec<Option<usize>>> {
    let mut out: Vec<Vec<Option<usize>>> = vec![vec![]; a.bars.len()];
    for n in &a.song {
        let c = &a.chans[n.channel];
        if c.role == "fx" {
            continue;
        }
        // `None` stands for the drums.
        let g = if c.role == "drums" {
            None
        } else {
            Some(a.group_of(n.channel))
        };
        let (b0, b1) = (a.bar_of(n.start), a.bar_of(n.start.max(n.end - 1e-6)));
        for bar in out.iter_mut().take(b1 + 1).skip(b0) {
            if !bar.contains(&g) {
                bar.push(g);
            }
        }
    }
    out
}

/// A fingerprint of what plays in each bar (relative to the bar).
fn bar_prints(a: &Ana) -> Vec<String> {
    let mut parts: Vec<Vec<String>> = vec![vec![]; a.bars.len()];
    for n in &a.song {
        let b = a.bar_of(n.start);
        parts[b].push(format!(
            "{}/{}/{}/{}",
            n.channel,
            n.pitch,
            r3(n.start - a.bars[b].start),
            r3(n.end - n.start)
        ));
    }
    parts
        .into_iter()
        .map(|mut x| {
            x.sort();
            x.join(";")
        })
        .collect()
}

pub fn check_arrangement(a: &mut Ana) {
    let p = a.p;
    let used: Vec<usize> = (0..p.patterns.len())
        .filter(|&i| !p.patterns[i].notes.is_empty())
        .collect();
    if p.playlist.clips.is_empty() {
        if !used.is_empty() {
            let mut ops = vec![];
            if p.playlist.tracks.is_empty() {
                ops.push(set("/playlist/tracks/-".into(), json!({"name": "Track 1"})));
            }
            let mut at = 0.0;
            for &i in &used {
                let pat = &p.patterns[i];
                ops.push(set(
                    "/playlist/clips/-".into(),
                    json!({"pattern": pat.id, "track": 0, "start": at, "length": pat.length}),
                ));
                at += pat.length;
            }
            a.add(
                "empty-playlist",
                "warn",
                "The playlist is empty".into(),
                format!(
                    "{} notes, but nothing is placed in the song, so the song plays silence.",
                    plural(used.len(), "pattern has", "patterns have")
                ),
                at_project("Playlist"),
                fix("Lay the patterns out one after another", ops),
            );
        }
        return;
    }

    // Patterns and clips.
    for &i in &used {
        let pat = &p.patterns[i];
        if !p.playlist.clips.iter().any(|c| c.pattern == pat.id) {
            let at = a.at_pattern(i);
            a.note(
                "unused-pattern",
                "info",
                format!("\"{}\" is never placed", pat.name),
                "Its notes are not part of the song.".into(),
                at,
            );
        }
    }
    let empty: Vec<usize> = (0..p.playlist.clips.len())
        .filter(|&i| {
            let c = &p.playlist.clips[i];
            !c.pattern.is_empty()
                && p.pattern(&c.pattern)
                    .map(|x| x.notes.is_empty())
                    .unwrap_or(false)
        })
        .collect();
    if let Some(&first) = empty.first() {
        let at = a.at_song(p.playlist.clips[first].start, first as i64);
        a.add(
            "empty-clips",
            "info",
            format!(
                "{} an empty pattern",
                plural(empty.len(), "clip plays", "clips play")
            ),
            "They play nothing.".into(),
            at,
            fix("Remove them", remove_all("/playlist/clips", empty.clone())),
        );
    }

    // Identical patterns.
    let prints: Vec<String> = p
        .patterns
        .iter()
        .map(|pat| {
            if pat.notes.is_empty() {
                return String::new();
            }
            let mut ns: Vec<String> = pat
                .notes
                .iter()
                .map(|n| {
                    format!(
                        "{}/{}/{}/{}/{}",
                        n.channel,
                        n.pitch,
                        r3(n.start),
                        r3(n.length),
                        r3(n.velocity)
                    )
                })
                .collect();
            ns.sort();
            format!("{}|{}", pat.length, ns.join(";"))
        })
        .collect();
    for j in 0..p.patterns.len() {
        if prints[j].is_empty() {
            continue;
        }
        let i = prints.iter().position(|x| *x == prints[j]).unwrap_or(j);
        if i == j {
            continue;
        }
        let (keep, dup) = (&p.patterns[i], &p.patterns[j]);
        let mut ops: Vec<_> = (0..p.playlist.clips.len())
            .filter(|&c| p.playlist.clips[c].pattern == dup.id)
            .map(|c| set(format!("/playlist/clips/{c}/pattern"), json!(keep.id)))
            .collect();
        // Delete the copy unless something else names it.
        let quoted = format!("\"{}\"", dup.id);
        let named = p.score.marks.iter().any(|m| m.pattern == dup.id)
            || serde_json::to_string(&p.drums)
                .unwrap_or_default()
                .contains(&quoted);
        if !named {
            ops.push(remove(format!("/patterns/{j}")));
        }
        let at = a.at_pattern(j);
        a.add(
            "identical-patterns",
            "info",
            format!("\"{}\" is a copy of \"{}\"", dup.name, keep.name),
            "Two patterns with the same notes have to be edited twice.".into(),
            at,
            fix(format!("Use \"{}\" in its place", keep.name), ops),
        );
    }

    // Overlapping clips on one track.
    let mut clips: Vec<usize> = (0..p.playlist.clips.len())
        .filter(|&i| !p.playlist.clips[i].pattern.is_empty())
        .collect();
    clips.sort_by(|&x, &y| {
        let (m, n) = (&p.playlist.clips[x], &p.playlist.clips[y]);
        m.track
            .index()
            .cmp(&n.track.index())
            .then(m.start.total_cmp(&n.start))
    });
    for w in clips.windows(2) {
        let (m, n) = (&p.playlist.clips[w[0]], &p.playlist.clips[w[1]]);
        if m.track != n.track || n.start >= m.start + m.length - 0.01 {
            continue;
        }
        let len = n.start - m.start;
        if len <= 0.0 {
            continue;
        }
        let at = a.at_song(n.start, w[1] as i64);
        a.add(
            "clip-overlap",
            "info",
            "Two clips overlap on one track".into(),
            format!(
                "On track {}, a clip of \"{}\" still plays when one of \"{}\" starts (for {} beats).",
                m.track.index() + 1,
                m.pattern,
                n.pattern,
                num(m.start + m.length - n.start)
            ),
            at,
            fix("Trim the first one", vec![set(format!("/playlist/clips/{}/length", w[0]), json!(r3(len)))]),
        );
    }
    // Clips a little off the bar.
    if !a.bars.is_empty() {
        for (i, c) in p.playlist.clips.iter().enumerate() {
            let b = a.bars[a.bar_of(c.start)];
            let next = b.start + b.length;
            let target = if c.start - b.start <= next - c.start {
                b.start
            } else {
                next
            };
            let d = (c.start - target).abs();
            if d > 0.01 && d <= 0.5 {
                let at = a.at_song(c.start, i as i64);
                a.add(
                    "clip-off-bar",
                    "info",
                    format!("A clip starts {} beats off the bar", num(d)),
                    format!(
                        "The clip of \"{}\" on track {} is just off bar {}.",
                        if c.pattern.is_empty() {
                            &c.sample
                        } else {
                            &c.pattern
                        },
                        c.track.index() + 1,
                        a.bar_of(target) + 1
                    ),
                    at,
                    fix(
                        "Snap it to the bar",
                        vec![set(format!("/playlist/clips/{i}/start"), json!(target))],
                    ),
                );
            }
        }
    }

    if a.bars.is_empty() || a.song.is_empty() {
        return;
    }
    let nb = a.bars.len();
    let els = bar_elements(a);
    let counts: Vec<usize> = els.iter().map(|e| e.len()).collect();
    let max = counts.iter().copied().max().unwrap_or(0);
    let bar_at = |a: &Ana, b: usize| a.at_song(a.bars[b].start, -1);
    if nb <= 16 && max > 0 {
        let at = a.at_song(0.0, -1);
        a.note(
            "short-song",
            "info",
            format!("The song is only {} long", plural(nb, "bar", "bars")),
            "It is a loop so far: build an intro, sections that contrast and an ending.".into(),
            at,
        );
    }

    // Loopitis: the same bars repeating with a period of 1, 2, 4 or 8 bars.
    let prints = bar_prints(a);
    let (mut best_run, mut best_at, mut best_period) = (0, 0, 1);
    for k in [1, 2, 4, 8] {
        let mut s = k;
        for i in k..=nb {
            let same = i < nb && !prints[i].is_empty() && prints[i] == prints[i - k];
            if same {
                continue;
            }
            let run = i + k - s;
            if i > s && run > best_run {
                best_run = run;
                best_at = s - k;
                best_period = k;
            }
            s = i + 1;
        }
    }
    if best_run >= 24 {
        let at = bar_at(a, best_at);
        a.note(
            "loopitis",
            if best_run >= 32 { "warn" } else { "info" },
            format!("The same {} for {best_run} bars", plural(best_period, "bar repeats", "bars repeat")),
            format!(
                "From bar {}, {} unchanged for {best_run} bars. Add or take away a part every 4–8 bars, or vary a fill.",
                best_at + 1,
                if best_period == 1 { "one bar plays".to_string() } else { format!("a {best_period}-bar loop plays") }
            ),
            at,
        );
    }

    // Contrast between 8-bar blocks.
    if nb >= 24 && max >= 3 {
        let blocks: Vec<usize> = (0..nb)
            .step_by(8)
            .map(|b| {
                let mut set: Vec<Option<usize>> = vec![];
                for e in &els[b..nb.min(b + 8)] {
                    for g in e {
                        if !set.contains(g) {
                            set.push(*g);
                        }
                    }
                }
                set.len()
            })
            .filter(|&n| n > 0)
            .collect();
        let (lo, hi) = (
            blocks.iter().copied().min().unwrap_or(0),
            blocks.iter().copied().max().unwrap_or(0),
        );
        if blocks.len() >= 3 && hi - lo < 2 {
            let at = a.at_song(0.0, -1);
            a.note(
                "no-contrast",
                "info",
                "The density never changes".into(),
                format!("Every 8-bar section has {lo}–{hi} parts playing. Drop parts out for a breakdown so the full sections hit harder."),
                at,
            );
        }
    }

    // Too many parts at once.
    let crowded: Vec<usize> = (0..nb).filter(|&i| counts[i] > 6).collect();
    if let Some(&b) = crowded.first() {
        let names: Vec<String> = els[b]
            .iter()
            .map(|g| {
                g.map(|c| a.chans[c].name.clone())
                    .unwrap_or_else(|| "drums".into())
            })
            .collect();
        let at = bar_at(a, b);
        a.note(
            "too-many-elements",
            "info",
            format!("Up to {max} parts play at once"),
            format!(
                "{} more than six parts (drums counted as one), first bar {}: {}. Keep the focus on rhythm, a melody and one wildcard.",
                plural(crowded.len(), "bar has", "bars have"),
                b + 1,
                names.join(", ")
            ),
            at,
        );
    }

    // Entrances and endings.
    if nb >= 16 && max >= 4 {
        let full = (max as f64 * 0.8).ceil() as usize;
        if let Some(first) = counts.iter().position(|&c| c > 0) {
            if counts[first] >= full {
                let at = bar_at(a, first);
                a.note(
                    "full-intro",
                    "info",
                    "Everything enters at once".into(),
                    format!(
                        "Bar {} already has {} of the song's {max} parts; an intro that builds gives the song somewhere to go.",
                        first + 1,
                        counts[first]
                    ),
                    at,
                );
            }
        }
        let mut last = nb - 1;
        while last > 0 && counts[last] == 0 {
            last -= 1;
        }
        let tail = a.bars[last.saturating_sub(3)].start;
        let automated_end = p
            .automation
            .iter()
            .any(|l| l.points.iter().any(|pt| pt.beat >= tail));
        if counts[last] >= full && !automated_end {
            let at = bar_at(a, last);
            a.note(
                "abrupt-ending",
                "info",
                "The song stops at full density".into(),
                format!(
                    "The last bar still has {} parts playing. Thin it out, add an outro or fade it with volume automation.",
                    counts[last]
                ),
                at,
            );
        }
    }
    if nb >= 32 && p.automation.is_empty() {
        let at = a.at_song(0.0, -1);
        a.note(
            "no-movement",
            "info",
            "No automation".into(),
            "Filter sweeps and volume rides into new sections carry the energy of a song; nothing moves here.".into(),
            at,
        );
    }
}

// ------------------------------------------------------------------ project

/// "Pattern 3", "Channel", "insert #2", "Untitled": a name nobody chose.
fn default_name(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    ["pattern", "channel", "insert", "track", "untitled"]
        .iter()
        .any(|w| {
            n.strip_prefix(w)
                .map(|rest| {
                    let rest = rest.trim_start();
                    let rest = rest.strip_prefix('#').unwrap_or(rest);
                    rest.chars().all(|c| c.is_ascii_digit())
                })
                .unwrap_or(false)
        })
}

pub fn check_project(a: &mut Ana) {
    let p = a.p;
    let t = &p.transport;
    if (t.bpm - t.bpm.round()).abs() > 0.01 {
        let bpm = t.bpm.round();
        a.add(
            "tempo",
            "info",
            format!("The tempo is {} BPM", t.bpm),
            "A fractional tempo is usually an accident.".into(),
            at_project("Transport"),
            fix(
                format!("Round it to {bpm} BPM"),
                vec![set("/transport/bpm".into(), json!(bpm))],
            ),
        );
    } else if t.bpm < 60.0 || t.bpm > 200.0 {
        a.note(
            "tempo",
            "info",
            format!("{} BPM", t.bpm),
            if t.bpm < 60.0 {
                "Very slow: is this half time of a faster groove?"
            } else {
                "Very fast: is this double time of a slower groove?"
            }
            .into(),
            at_project("Transport"),
        );
    }
    let bar = a.bar_beats();
    for (i, pat) in p.patterns.iter().enumerate() {
        if pat.notes.is_empty() || pat.length <= 0.0 {
            continue;
        }
        let bars = pat.length / bar;
        if (bars - bars.round()).abs() > 1e-6 {
            let whole = (bars - 1e-6).ceil();
            let at = a.at_pattern(i);
            a.add(
                "pattern-bars",
                "info",
                format!("\"{}\" is {} beats", pat.name, num(pat.length)),
                format!("That is {} bars; the next loop starts mid-bar.", num(bars)),
                at,
                fix(
                    format!("Make it {}", plural(whole as usize, "bar", "bars")),
                    vec![set(format!("/patterns/{i}/length"), json!(whole * bar))],
                ),
            );
        } else if [3.0, 5.0, 7.0].contains(&bars.round()) {
            let at = a.at_pattern(i);
            a.note(
                "pattern-bars",
                "info",
                format!("\"{}\" is {} bars", pat.name, bars.round()),
                "Odd phrase lengths can be deliberate; usually phrases come in 2, 4 or 8 bars."
                    .into(),
                at,
            );
        }
    }
    let layered: Vec<&str> = p
        .channels
        .iter()
        .filter_map(|c| c.layer_of.as_deref())
        .collect();
    for c in a.chans.clone() {
        if c.count == 0
            && c.layer_of.is_empty()
            && !layered.contains(&c.id.as_str())
            && c.kind != "sampler"
            && c.kind != "plugin"
        {
            let at = a.at_channel(c.index);
            a.note(
                "unused-channel",
                "info",
                format!("{} has no notes", c.name),
                "It plays nothing in any pattern.".into(),
                at,
            );
        }
    }
    let mut names: Vec<String> = vec![];
    if p.meta.title.trim().is_empty() || p.meta.title == "Untitled" {
        names.push("the song".into());
    }
    names.extend(
        p.patterns
            .iter()
            .filter(|x| default_name(&x.name))
            .map(|x| x.name.clone()),
    );
    names.extend(
        p.channels
            .iter()
            .filter(|x| default_name(&x.name))
            .map(|x| x.name.clone()),
    );
    if !names.is_empty() {
        a.note(
            "names",
            "info",
            plural(names.len(), "default name", "default names"),
            format!(
                "{}: names that describe the part help when you come back.",
                list(&names, 5)
            ),
            at_project("Names"),
        );
    }
}
