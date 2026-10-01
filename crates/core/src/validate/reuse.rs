//! Checks for patterns used by reference (`patterns[].uses`) and the verse a
//! clip sings.

use super::V;
use crate::model::Project;

pub(super) fn check(v: &mut V, p: &Project) {
    for (i, pat) in p.patterns.iter().enumerate() {
        for (j, u) in pat.uses.iter().enumerate() {
            let path = format!("patterns[{i}].uses[{j}]");
            let Some(k) = p.patterns.iter().position(|q| q.id == u.pattern) else {
                v.err(
                    format!("{path}.pattern"),
                    format!("unknown pattern {:?}", u.pattern),
                );
                continue;
            };
            if reaches(p, k, i) {
                v.err(
                    format!("{path}.pattern"),
                    format!(
                        "{:?} uses {:?}, which plays {:?} again: a pattern cannot contain itself",
                        pat.id, u.pattern, pat.id
                    ),
                );
            }
            check_range(v, &path, u, p.patterns[k].length);
            if !u.channel.is_empty() && p.channel(&u.channel).is_none() {
                v.err(
                    format!("{path}.channel"),
                    format!("unknown channel {:?}", u.channel),
                );
            }
            if !(-48..=48).contains(&u.transpose) {
                v.err(format!("{path}.transpose"), "must be between -48 and 48");
            }
            v.range(&format!("{path}.velocity"), u.velocity, 0.0, 2.0);
            check_verse(v, &path, u.verse);
        }
    }
    for (i, c) in p.playlist.clips.iter().enumerate() {
        let path = format!("playlist.clips[{i}]");
        check_verse(v, &path, c.verse);
        if c.verse.is_some() && c.pattern.is_empty() {
            v.warn(format!("{path}.verse"), "only pattern clips sing verses");
        }
    }
}

fn check_range(v: &mut V, path: &str, u: &crate::model::Use, used_length: f64) {
    if !(u.start >= 0.0 && u.start.is_finite()) {
        v.err(format!("{path}.start"), "start must be >= 0");
    }
    if !(u.from >= 0.0 && u.from.is_finite()) {
        v.err(format!("{path}.from"), "from must be >= 0");
    }
    let to = u.to.unwrap_or(used_length);
    if !(to > u.from && to.is_finite()) {
        v.err(format!("{path}.to"), "to must come after from");
    } else if to > used_length + 1e-9 {
        v.err(
            format!("{path}.to"),
            format!(
                "{to} is past the end of {:?} ({used_length} beats)",
                u.pattern
            ),
        );
    }
}

fn check_verse(v: &mut V, path: &str, verse: Option<u32>) {
    if verse == Some(0) {
        v.err(format!("{path}.verse"), "verses are numbered from 1");
    }
}

/// Whether playing `patterns[from]` plays `patterns[target]` somewhere inside.
fn reaches(p: &Project, from: usize, target: usize) -> bool {
    let mut seen = vec![false; p.patterns.len()];
    let mut todo = vec![from];
    while let Some(i) = todo.pop() {
        if i == target {
            return true;
        }
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        for u in &p.patterns[i].uses {
            if let Some(k) = p.patterns.iter().position(|q| q.id == u.pattern) {
                todo.push(k);
            }
        }
    }
    false
}
