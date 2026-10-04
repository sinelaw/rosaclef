//! The low end, the mixer, the stereo image, effect chains and the master.

use super::analysis::*;
use crate::model::Device;
use serde_json::json;

fn is_kick(c: &ChInfo) -> bool {
    c.role == "drums" && c.drum == "kick"
}

/// A part that owns the low end: a bass or a kick.
fn low_role(c: &ChInfo) -> bool {
    c.role == "bass" || is_kick(c)
}

/// The channels (with notes) routed to an insert.
fn feeding<'b>(a: &'b Ana, i: usize) -> Vec<&'b ChInfo> {
    a.chans
        .iter()
        .filter(|c| c.insert == i && c.count > 0)
        .collect()
}

fn param_path(ins: usize, k: usize, key: &str) -> String {
    format!("/mixer/inserts/{ins}/effects/{k}/params/{key}")
}

pub fn check_low_end(a: &mut Ana) {
    let p = a.p;
    for part in a.parts.clone() {
        let ch = a.ch(&part).clone();
        if !ch.pitched {
            continue;
        }
        let low: Vec<usize> = part
            .idx
            .iter()
            .copied()
            .filter(|&i| a.sounding(&part, i) < 28)
            .collect();
        if low.is_empty() {
            continue;
        }
        let lowest = low.iter().map(|&i| a.sounding(&part, i)).min().unwrap_or(0);
        let notes = &a.pattern(&part).notes;
        let ops = low
            .iter()
            .map(|&i| set(note_path(part.pat, i, "pitch"), json!(notes[i].pitch + 12)))
            .collect();
        let at = a.at_part(&part, low.clone());
        a.add(
            "sub-too-low",
            if lowest < 24 { "warn" } else { "info" },
            format!("{} below E1", plural(low.len(), "note is", "notes are")),
            format!(
                "{} goes down to {} ({} Hz), lower than most speakers reproduce.",
                ch.name,
                note_name(lowest),
                hz(lowest as f64).round()
            ),
            at,
            fix("Move them up an octave", ops),
        );
    }

    // Kick against sustained bass.
    let kicks: Vec<usize> = a
        .chans
        .iter()
        .filter(|c| is_kick(c))
        .map(|c| c.index)
        .collect();
    let basses: Vec<usize> = a
        .chans
        .iter()
        .filter(|c| c.role == "bass")
        .map(|c| c.index)
        .collect();
    if !basses.is_empty() {
        let ducked = p.automation.iter().any(|l| {
            basses.iter().any(|&b| {
                let c = &a.chans[b];
                l.target == format!("channel/{}/volume", c.id)
                    || l.target == format!("insert/{}/volume", c.insert)
            })
        });
        for notes in a.scopes() {
            let hits: Vec<&SNote> = notes
                .iter()
                .filter(|n| kicks.contains(&n.channel))
                .collect();
            if hits.len() < 8 {
                continue;
            }
            let bass: Vec<&SNote> = notes
                .iter()
                .filter(|n| basses.contains(&n.channel))
                .collect();
            let ringing = hits
                .iter()
                .filter(|h| {
                    bass.iter()
                        .any(|b| b.start < h.start - 0.05 && b.end > h.start + 0.1)
                })
                .count();
            if ringing as f64 >= 0.5 * hits.len() as f64 && !ducked {
                let at = a.at_notes(&notes, hits[0].start);
                a.note(
                    "kick-bass",
                    "info",
                    "The bass rings through the kick".into(),
                    format!(
                        "On {ringing} of {} kicks a bass note is already sounding. Shorten the bass around the kick, or duck it under each hit.",
                        hits.len()
                    ),
                    at,
                );
            }
        }
    }

    for c in a.chans.clone() {
        if !low_role(&c) || c.count == 0 {
            continue;
        }
        if c.pan.abs() > 0.1 {
            let at = a.at_channel(c.index);
            a.add(
                "lowend-panned",
                "warn",
                format!(
                    "{} is panned {}",
                    c.name,
                    if c.pan < 0.0 { "left" } else { "right" }
                ),
                format!(
                    "{} belongs in the center.",
                    if c.role == "bass" { "A bass" } else { "A kick" }
                ),
                at,
                fix(
                    "Center it",
                    vec![set(format!("/channels/{}/pan", c.index), json!(0.0))],
                ),
            );
        }
        if c.role != "bass" {
            continue;
        }
        let inst = &p.channels[c.index].instrument;
        if inst.kind == "wavetable" && inst.param("unison") > 1.0 && inst.param("width") > 0.3 {
            let at = a.at_channel(c.index);
            a.add(
                "lowend-wide",
                "warn",
                format!("{} is a wide unison bass", c.name),
                format!(
                    "Its unison voices spread {}% across the stereo field; the low end smears and thins out in mono.",
                    percent(inst.param("width"))
                ),
                at,
                fix("Narrow the unison to 15%", vec![set(format!("/channels/{}/instrument/params/width", c.index), json!(0.15))]),
            );
        } else if inst.kind == "analog" && inst.param("unison") > 1.0 {
            let at = a.at_channel(c.index);
            a.note(
                "lowend-wide",
                "info",
                format!("{} uses stereo unison", c.name),
                "Detuned unison copies spread across the stereo field and phase in mono; keep the sub mono (a mono sub layer under the unison works).".into(),
                at,
            );
        }
        if let Some(ins) = p.mixer.inserts.get(c.insert).filter(|_| c.insert > 0) {
            if ins
                .effects
                .iter()
                .any(|e| e.enabled && (e.kind == "chorus" || e.kind == "phaser"))
            {
                let at = a.at_insert(c.insert);
                a.note(
                    "lowend-wide",
                    "info",
                    format!("Modulation on {}'s insert", c.name),
                    "A chorus or phaser on a bass widens and smears the low end.".into(),
                    at,
                );
            }
        }
        if inst.kind == "fm" {
            let odd: Vec<String> = (1..=6)
                .filter(|k| {
                    let key = format!("op{k}Ratio");
                    let Some(&r) = inst.params.get(&key) else {
                        return false;
                    };
                    inst.param(&format!("op{k}Level")) > 0.01
                        && (r - r.round()).abs() > 0.02
                        && (r * 2.0 - (r * 2.0).round()).abs() > 0.04
                })
                .map(|k| k.to_string())
                .collect();
            if !odd.is_empty() {
                let one = odd.len() == 1;
                let at = a.at_channel(c.index);
                a.note(
                    "fm-bass",
                    "info",
                    format!("{} has inharmonic FM ratios", c.name),
                    format!(
                        "Operator {} {} non-integer ratio{}: the bass's pitch turns vague. Keep one clean carrier at 1:1 for the sub.",
                        odd.join(", "),
                        if one { "has a" } else { "have" },
                        if one { "" } else { "s" }
                    ),
                    at,
                );
            }
        }
    }

    // Inserts that carry only kick and bass: no reverb.
    for i in 1..p.mixer.inserts.len() {
        let src = feeding(a, i);
        if src.is_empty() || !src.iter().all(|c| low_role(c)) {
            continue;
        }
        let ins = &p.mixer.inserts[i];
        let Some(k) = ins
            .effects
            .iter()
            .position(|e| e.enabled && e.kind == "reverb" && e.param("mix") > 0.1)
        else {
            continue;
        };
        let names: Vec<String> = src.iter().map(|c| c.name.clone()).collect();
        let at = a.at_insert(i);
        a.add(
            "reverb-on-bass",
            "warn",
            format!("Reverb on {}", ins.name),
            format!(
                "{} go through a reverb at {}% wet; the tail muddies the low end.",
                names.join(" and "),
                percent(ins.effects[k].param("mix"))
            ),
            at,
            fix(
                "Bring the reverb down to 5%",
                vec![set(param_path(i, k, "mix"), json!(0.05))],
            ),
        );
    }

    // Inserts of parts that live above the bass: a low cut.
    for i in 1..p.mixer.inserts.len() {
        let src = feeding(a, i);
        if src.is_empty()
            || !src
                .iter()
                .all(|c| c.pitched && c.role != "bass" && c.low >= 55)
        {
            continue;
        }
        let ins = &p.mixer.inserts[i];
        let cut = ins.effects.iter().any(|e| {
            (e.kind == "filter" && e.option("mode") == "highpass")
                || (e.kind == "eq" && e.param("low") <= -6.0)
        });
        if cut {
            continue;
        }
        let lowest = src.iter().map(|c| c.low).min().unwrap_or(0);
        let f = (hz(lowest as f64) / 2.0).clamp(60.0, 250.0).round();
        let mut d = Device::new("filter");
        d.options.insert("mode".into(), "highpass".into());
        d.params.insert("cutoff".into(), f);
        d.params.insert("resonance".into(), 0.0);
        d.params.insert("mix".into(), 1.0);
        let at = a.at_insert(i);
        a.add(
            "no-highpass",
            "info",
            format!("No low cut on {}", ins.name),
            format!(
                "Its parts play nothing under {} ({} Hz), yet whatever they carry below that adds to the low end.",
                note_name(lowest),
                hz(lowest as f64).round()
            ),
            at,
            fix(format!("Add a high-pass at {f} Hz"), vec![set(format!("/mixer/inserts/{i}/effects/0"), device_json(&d))]),
        );
    }
}

pub fn check_mix(a: &mut Ana) {
    let p = a.p;
    let hot_ch: Vec<usize> = (0..p.channels.len())
        .filter(|&i| p.channels[i].volume > 1.001)
        .collect();
    let hot_ins: Vec<usize> = (1..p.mixer.inserts.len())
        .filter(|&i| p.mixer.inserts[i].volume > 1.001)
        .collect();
    if !hot_ch.is_empty() || !hot_ins.is_empty() {
        let names: Vec<String> = hot_ch
            .iter()
            .map(|&i| p.channels[i].name.clone())
            .chain(hot_ins.iter().map(|&i| p.mixer.inserts[i].name.clone()))
            .collect();
        let mut ops = vec![];
        let mc = p.channels.iter().map(|c| c.volume).fold(0.0, f64::max);
        if mc > 1.0 {
            for (i, c) in p.channels.iter().enumerate() {
                ops.push(set(
                    format!("/channels/{i}/volume"),
                    json!(r3(c.volume / mc)),
                ));
            }
        }
        let mi = p
            .mixer
            .inserts
            .iter()
            .skip(1)
            .map(|x| x.volume)
            .fold(0.0, f64::max);
        if mi > 1.0 {
            for (i, x) in p.mixer.inserts.iter().enumerate().skip(1) {
                ops.push(set(
                    format!("/mixer/inserts/{i}/volume"),
                    json!(r3(x.volume / mi)),
                ));
            }
        }
        let at = match hot_ch.first() {
            Some(&c) => a.at_channel(c),
            None => a.at_insert(hot_ins[0]),
        };
        a.add(
            "hot-faders",
            "info",
            format!("{} above unity", plural(names.len(), "fader is", "faders are")),
            format!(
                "{} {} pushed above 0 dB. Turning everything down together keeps the balance and wins headroom.",
                list(&names, 4),
                if names.len() == 1 { "is" } else { "are" }
            ),
            at,
            fix("Pull the faders down together to unity", ops),
        );
    }

    let live: Vec<&ChInfo> = a
        .chans
        .iter()
        .filter(|c| c.count > 0 || !c.layer_of.is_empty())
        .collect();
    if live.len() >= 4
        && live
            .iter()
            .all(|c| (c.volume - live[0].volume).abs() < 1e-6)
    {
        let (n, v) = (live.len(), live[0].volume);
        a.note(
            "unmixed",
            "info",
            format!("All {n} channels at {}", db(v)),
            "Set a balance: start with the most important part and bring the others in around it."
                .into(),
            at_project("Channel rack"),
        );
    }

    let direct: Vec<usize> = a
        .chans
        .iter()
        .filter(|c| c.insert == 0 && c.count > 0 && c.layer_of.is_empty())
        .map(|c| c.index)
        .collect();
    if direct.len() >= 3 {
        let mut ops = vec![];
        let mut next = p.mixer.inserts.len();
        let mut drums: Option<usize> = None;
        for &c in &direct {
            let ch = &p.channels[c];
            let to = if is_drums(ch) {
                match drums {
                    Some(d) => d,
                    None => {
                        ops.push(set("/mixer/inserts/-".into(), json!({"name": "Drums"})));
                        drums = Some(next);
                        next += 1;
                        next - 1
                    }
                }
            } else {
                ops.push(set("/mixer/inserts/-".into(), json!({"name": ch.name})));
                next += 1;
                next - 1
            };
            ops.push(set(format!("/channels/{c}/mixer"), json!(to)));
        }
        let names: Vec<String> = direct.iter().map(|&c| a.chans[c].name.clone()).collect();
        a.add(
            "not-routed",
            "info",
            format!(
                "{} straight into the master",
                plural(direct.len(), "channel plays", "channels play")
            ),
            format!(
                "{} have no insert of their own to EQ, compress or level.",
                list(&names, 5)
            ),
            at_project("Mixer"),
            fix("Give each its own insert (drums share one)", ops),
        );
    }

    for i in 1..p.mixer.inserts.len() {
        let ins = &p.mixer.inserts[i];
        if ins.effects.is_empty()
            || a.chans.iter().any(|c| c.insert == i)
            || p.playlist.clips.iter().any(|c| c.mixer.index() == i)
        {
            continue;
        }
        let at = a.at_insert(i);
        a.note(
            "unused-insert",
            "info",
            format!("Nothing plays into {}", ins.name),
            format!(
                "It has {} but no channel or clip is routed to it.",
                plural(ins.effects.len(), "effect", "effects")
            ),
            at,
        );
    }

    if let Some(i) = p.mixer.inserts.iter().position(|x| x.solo) {
        let ops = (0..p.mixer.inserts.len())
            .filter(|&j| p.mixer.inserts[j].solo)
            .map(|j| set(format!("/mixer/inserts/{j}/solo"), json!(false)))
            .collect();
        let at = a.at_insert(i);
        a.add(
            "solo",
            "warn",
            format!("{} is soloed", p.mixer.inserts[i].name),
            "Every other insert is silent while a solo is on.".into(),
            at,
            fix("Turn the solos off", ops),
        );
    }

    let mut muted: Vec<String> = a
        .chans
        .iter()
        .filter(|c| c.mute && c.count > 0)
        .map(|c| c.name.clone())
        .collect();
    for (t, tr) in p.playlist.tracks.iter().enumerate() {
        if tr.mute && p.playlist.clips.iter().any(|c| c.track.index() == t) {
            muted.push(tr.name.clone());
        }
    }
    muted.extend(
        p.mixer
            .inserts
            .iter()
            .filter(|x| x.mute)
            .map(|x| x.name.clone()),
    );
    if !muted.is_empty() {
        a.note(
            "muted",
            "info",
            plural(muted.len(), "muted part", "muted parts"),
            format!(
                "{} won't be heard in the song or the render. Delete what you no longer need.",
                list(&muted, 6)
            ),
            at_project("Mute"),
        );
    }

    for (i, l) in p.automation.iter().enumerate() {
        if l.points.len() < 2
            || !l
                .points
                .iter()
                .all(|pt| (pt.value - l.points[0].value).abs() < 1e-6)
        {
            continue;
        }
        let at = a.at_lane(i);
        a.add(
            "flat-automation",
            "info",
            format!("\"{}\" never moves", at.label),
            format!("All its points are at {}.", num(l.points[0].value)),
            at,
            fix("Remove the lane", vec![remove(format!("/automation/{i}"))]),
        );
    }
}

pub fn check_stereo(a: &mut Ana) {
    let p = a.p;
    // The lead (the busiest single line) stays in the middle with the kick, snare and bass.
    let lead = a
        .chans
        .iter()
        .filter(|c| c.role == "lead")
        .max_by_key(|c| c.count)
        .map(|c| c.index);
    let spreadable: Vec<usize> = a
        .chans
        .iter()
        .filter(|c| {
            c.count > 0
                && c.layer_of.is_empty()
                && !low_role(c)
                && Some(c.index) != lead
                && !["kick", "snare", "clap"].contains(&c.drum.as_str())
                && c.role != "fx"
        })
        .map(|c| c.index)
        .collect();
    let centered = a
        .chans
        .iter()
        .filter(|c| c.count > 0)
        .all(|c| c.pan.abs() < 0.02)
        && p.mixer.inserts.iter().all(|x| x.pan.abs() < 0.02);
    if spreadable.len() >= 3 && centered {
        let amounts = [0.3, 0.45, 0.2, 0.55];
        let ops = spreadable
            .iter()
            .enumerate()
            .map(|(k, &c)| {
                set(
                    format!("/channels/{c}/pan"),
                    json!(if k % 2 == 0 { -1.0 } else { 1.0 } * amounts[(k / 2) % amounts.len()]),
                )
            })
            .collect();
        let names: Vec<String> = spreadable
            .iter()
            .map(|&c| a.chans[c].name.clone())
            .collect();
        a.add(
            "all-center",
            "info",
            "Everything is panned to the center".into(),
            format!(
                "{} could move out to the sides; kick, snare, bass and lead stay in the middle.",
                list(&names, 5)
            ),
            at_project("Panning"),
            fix("Spread the supporting parts", ops),
        );
    }
    let side: Vec<&ChInfo> = a
        .chans
        .iter()
        .filter(|c| c.count > 0 && !low_role(c))
        .collect();
    if side.len() >= 3 {
        let w: f64 = side.iter().map(|c| c.volume).sum();
        let s: f64 = side.iter().map(|c| c.volume * c.pan).sum();
        let tilt = if w > 0.0 { s / w } else { 0.0 };
        if tilt.abs() > 0.25 {
            let way = if tilt < 0.0 { "left" } else { "right" };
            a.note(
                "lopsided",
                "info",
                format!("The mix leans {way}"),
                format!("Weighted by volume, the parts sit {}% to the {way}; balance them across both sides.", percent(tilt.abs())),
                at_project("Panning"),
            );
        }
    }
}

// Delay times that are note values (beats): 64ths to whole notes, dotted and triplet.
const NOTE_VALUES: [f64; 18] = [
    1.0 / 16.0,
    1.0 / 12.0,
    1.0 / 8.0,
    1.0 / 6.0,
    3.0 / 16.0,
    1.0 / 4.0,
    1.0 / 3.0,
    3.0 / 8.0,
    1.0 / 2.0,
    2.0 / 3.0,
    3.0 / 4.0,
    1.0,
    4.0 / 3.0,
    3.0 / 2.0,
    2.0,
    8.0 / 3.0,
    3.0,
    4.0,
];

fn nearest_value(t: f64) -> f64 {
    NOTE_VALUES
        .iter()
        .copied()
        .min_by(|x, y| (t / x).ln().abs().total_cmp(&(t / y).ln().abs()))
        .unwrap_or(1.0)
}

pub fn check_effects(a: &mut Ana) {
    let p = a.p;
    for (i, ins) in p.mixer.inserts.iter().enumerate() {
        let fx = &ins.effects;
        let src = feeding(a, i);
        let (has_src, drums_only) = (
            !src.is_empty(),
            !src.is_empty() && src.iter().all(|c| c.role == "drums"),
        );
        let is_time = |e: &Device| e.kind == "reverb" || e.kind == "delay";

        // Time-based effects before dynamics.
        let first_time = fx.iter().position(|e| e.enabled && is_time(e));
        let last_dyn = fx
            .iter()
            .rposition(|e| e.enabled && ["compressor", "eq", "drive"].contains(&e.kind.as_str()));
        if let (Some(t), Some(d)) = (first_time, last_dyn) {
            if d > t {
                let order: Vec<&Device> = fx
                    .iter()
                    .filter(|e| !is_time(e) && e.kind != "limiter")
                    .chain(fx.iter().filter(|e| is_time(e)))
                    .chain(fx.iter().filter(|e| !is_time(e) && e.kind == "limiter"))
                    .collect();
                let chain: Vec<_> = order.into_iter().map(device_json).collect();
                let at = a.at_insert(i);
                a.add(
                    "fx-order",
                    "info",
                    format!("{}: {} before {}", ins.name, fx[t].kind, fx[d].kind),
                    "Time effects usually come after EQ, compression and saturation, so their tails are not squashed or colored.".into(),
                    at,
                    fix("Move the delays and reverbs after the dynamics", vec![set(format!("/mixer/inserts/{i}/effects"), json!(chain))]),
                );
            }
        }

        let reverbs = fx
            .iter()
            .filter(|e| e.enabled && e.kind == "reverb")
            .count();
        if reverbs >= 2 {
            let at = a.at_insert(i);
            a.note(
                "double-reverb",
                "info",
                format!("{} has {reverbs} reverbs", ins.name),
                "One reverb with the right size usually does the job of two.".into(),
                at,
            );
        }

        for (k, e) in fx.iter().enumerate() {
            if !e.enabled {
                continue;
            }
            let at = a.at_insert(i);
            let setp = |key: &str, v: f64| vec![set(param_path(i, k, key), json!(v))];
            match e.kind.as_str() {
                "reverb" => {
                    let mix = e.param("mix");
                    if i == 0 && mix > 0.15 {
                        a.add(
                            "reverb-wet",
                            "warn",
                            format!("Reverb on the master at {}%", percent(mix)),
                            "A reverb on the whole mix blurs every part, the low end included."
                                .into(),
                            at,
                            fix("Bring it down to 10%", setp("mix", 0.1)),
                        );
                    } else if i > 0 && mix >= 0.5 && has_src {
                        a.add(
                            "reverb-wet",
                            "info",
                            format!("{}'s reverb is {}% wet", ins.name, percent(mix)),
                            "More reverb than dry signal pushes the part far back.".into(),
                            at,
                            fix("Bring it down to 30%", setp("mix", 0.3)),
                        );
                    }
                }
                "delay" => {
                    let t = e.param("time");
                    let v = nearest_value(t);
                    if (t - v).abs() / v > 0.03 {
                        a.add(
                            "delay-sync",
                            "info",
                            format!("{}'s delay is {} beats", ins.name, num(t)),
                            format!("That is not a note value; the nearest is {} beats.", num(v)),
                            at.clone(),
                            fix(format!("Set it to {} beats", num(v)), setp("time", v)),
                        );
                    }
                    let fb = e.param("feedback");
                    if fb >= 0.85 {
                        a.add(
                            "delay-feedback",
                            "info",
                            format!("{}'s delay feedback is {}%", ins.name, percent(fb)),
                            "The repeats barely fade and pile up.".into(),
                            at,
                            fix("Lower it to 60%", setp("feedback", 0.6)),
                        );
                    }
                }
                "compressor" => {
                    let (ratio, thr) = (e.param("ratio"), e.param("threshold"));
                    if ratio >= 8.0 && thr <= -30.0 {
                        a.note(
                            "crushing-compressor",
                            "info",
                            format!("{}'s compressor crushes", ins.name),
                            format!(
                                "A {}:1 ratio from {} dB flattens the part's punch; parallel compression keeps it.",
                                num(ratio),
                                num(thr)
                            ),
                            at.clone(),
                        );
                    }
                    let att = e.param("attack");
                    if att < 3.0 && drums_only {
                        a.add(
                            "drum-attack",
                            "info",
                            format!("{}'s compressor attack is {} ms", ins.name, num(att)),
                            "So fast an attack clamps the drums' transients; a slower one lets the crack through.".into(),
                            at,
                            fix("Set the attack to 10 ms", setp("attack", 10.0)),
                        );
                    }
                }
                "eq" => {
                    let boosts: Vec<&str> = ["low", "mid", "high"]
                        .into_iter()
                        .filter(|key| e.param(key) >= 9.0)
                        .collect();
                    if !boosts.is_empty() {
                        let top = boosts.iter().map(|key| e.param(key)).fold(0.0, f64::max);
                        let ops = boosts
                            .iter()
                            .map(|key| set(param_path(i, k, key), json!(r3(e.param(key) / 2.0))))
                            .collect();
                        a.add(
                            "eq-boost",
                            "info",
                            format!("{}: EQ boost of {} dB", ins.name, num(top)),
                            format!(
                                "The {} band{} boosted 9 dB or more. Cutting what masks the part usually sounds cleaner.",
                                boosts.join(" and "),
                                if boosts.len() > 1 { "s are" } else { " is" }
                            ),
                            at,
                            fix("Halve the big boosts", ops),
                        );
                    }
                }
                _ => {}
            }
        }
        let off = fx.iter().filter(|e| !e.enabled).count();
        if off > 0 {
            let at = a.at_insert(i);
            a.note(
                "disabled",
                "info",
                format!(
                    "{} has {}",
                    ins.name,
                    plural(off, "disabled effect", "disabled effects")
                ),
                "Remove what you no longer use.".into(),
                at,
            );
        }
    }

    for c in a.chans.clone() {
        let inst = &p.channels[c.index].instrument;
        if c.count == 0 || (inst.kind != "analog" && inst.kind != "wavetable") {
            continue;
        }
        let r = inst.param("resonance");
        if r >= 0.9 {
            let at = a.at_channel(c.index);
            a.note(
                "resonance",
                "info",
                format!("{}'s filter resonance is {}%", c.name, percent(r)),
                "Near the top it whistles and self-oscillates; fine for acid, harsh elsewhere."
                    .into(),
                at,
            );
        }
    }
}

pub fn check_master(a: &mut Ana) {
    let p = a.p;
    let Some(m) = p.mixer.inserts.first() else {
        return;
    };
    let at = a.at_insert(0);
    if m.volume > 1.001 {
        a.add(
            "master-hot",
            "warn",
            format!("The master fader is at {}", db(m.volume)),
            "Lower the parts rather than raising the master.".into(),
            at.clone(),
            fix(
                "Set it to 0 dB",
                vec![set("/mixer/inserts/0/volume".into(), json!(1.0))],
            ),
        );
    }
    let on = m.effects.iter().filter(|e| e.enabled).count();
    let lims: Vec<usize> = (0..m.effects.len())
        .filter(|&k| m.effects[k].enabled && m.effects[k].kind == "limiter")
        .collect();
    if !a.parts.is_empty() && lims.is_empty() {
        let mut d = Device::new("limiter");
        d.params.insert("gain".into(), 0.0);
        d.params.insert("ceiling".into(), -1.0);
        a.add(
            "no-limiter",
            "info",
            "No limiter on the master".into(),
            "A limiter with a −1 dB ceiling keeps the render from clipping (and does little else when the mix has headroom).".into(),
            at.clone(),
            fix("Add a limiter (−1 dB ceiling)", vec![set("/mixer/inserts/0/effects/-".into(), device_json(&d))]),
        );
    }
    if let Some(&k) = lims.last() {
        let last_on = m.effects.iter().rposition(|e| e.enabled).unwrap_or(k);
        if k != last_on {
            let chain: Vec<_> = m
                .effects
                .iter()
                .filter(|e| e.kind != "limiter")
                .chain(m.effects.iter().filter(|e| e.kind == "limiter"))
                .map(device_json)
                .collect();
            a.add(
                "limiter-last",
                "warn",
                "The master limiter is not last".into(),
                format!(
                    "{} comes after it and can push the signal past the ceiling.",
                    m.effects[last_on].kind
                ),
                at.clone(),
                fix(
                    "Move the limiter to the end",
                    vec![set("/mixer/inserts/0/effects".into(), json!(chain))],
                ),
            );
        }
        let lim = &m.effects[k];
        let ceil = lim.param("ceiling");
        if ceil > -1.0 {
            a.add(
                "limiter-ceiling",
                "info",
                format!("Limiter ceiling at {} dB", num(ceil)),
                "Streaming services ask for −1 dB true peak: lossy encoding and inter-sample peaks overshoot a higher ceiling.".into(),
                at.clone(),
                fix("Set the ceiling to −1 dB", vec![set(param_path(0, k, "ceiling"), json!(-1.0))]),
            );
        }
        let gain = lim.param("gain");
        if gain > 6.0 {
            a.add(
                "limiter-drive",
                "warn",
                format!("The limiter is driven {} dB", num(gain)),
                "Streaming turns loud masters down to about −14 LUFS: the extra loudness is lost, the crushed transients are not.".into(),
                at.clone(),
                fix("Drive it 3 dB", vec![set(param_path(0, k, "gain"), json!(3.0))]),
            );
        }
    }
    if on > 4 || lims.len() > 1 {
        a.note(
            "master-chain",
            "info",
            format!("{} on the master", plural(on, "device", "devices")),
            if lims.len() > 1 {
                "Several limiters in a row fight each other."
            } else {
                "A long master chain often fixes in mastering what belongs in the mix."
            }
            .into(),
            at,
        );
    }
}
