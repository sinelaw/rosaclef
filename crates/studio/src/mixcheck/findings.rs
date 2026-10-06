//! Findings and suggestions: what the numbers mean, ranked, each with a
//! concrete JSON Patch against `project.json`. The rules are the Critic's
//! audio rules (`rosaclef critic --audio`); a project's `critic.off` and
//! `critic.suppress` leave them out here too.

use super::dsp::{self, r1, BARKS};
use super::model::{self, Audibility, Change, Element, Mix};
use super::options::{Check, Options, Threshold};
use super::report::{band_span, pct, Context, ElementOut, FindingOut, Report, Suggestion};
use super::timeline;
use rosaclef_core::Project;
use serde_json::{json, Value};

/// The rules, in the order findings rank.
/// A mix engineer's order: the balance and what covers what, then tone,
/// then the bus dynamics and the limiter's drive and release, the
/// arrangement's shape, and the ceiling last (it depends on all the rest).
pub const RULES: [&str; 22] = [
    "masked-lead",
    "inaudible-part",
    "part-dropout",
    "dominant-part",
    "low-end-buildup",
    "boxy-lowmids",
    "harsh-presence",
    "bright-highs",
    "reverb-wash",
    "low-end-off-centre",
    "lr-balance",
    "phase-correlation",
    "part-over-compression",
    "over-compression",
    "master-overload",
    "limiter-pumping",
    "fast-limiter-release",
    "section-lift",
    "section-loudness-flat",
    "loud-master",
    "master-fader",
    "true-peak",
];

fn set(path: String, value: Value) -> Value {
    json!({"op": "add", "path": path, "value": value})
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Ops raising (or lowering) an element's level by `db`. Raising first
/// undoes a cut of its insert's fader (when the insert carries it alone),
/// then its channel volume (up to 1.5), then its insert's fader (up to 2);
/// lowering takes its channel volume first.
pub fn level_ops(p: &Project, channel: Option<usize>, insert: usize, db: f64) -> Vec<Value> {
    let ops = fader_ops(p, channel, insert, db);
    // A fader an automation lane drives does nothing while it plays: the
    // lane's points move by as much instead.
    let mut out = vec![];
    for op in ops {
        let path = op["path"].as_str().unwrap_or("").to_string();
        let to = op["value"].as_f64().unwrap_or(0.0);
        let (target, from, max) = if path == format!("/mixer/inserts/{insert}/volume") {
            (
                format!("insert/{insert}/volume"),
                p.mixer.inserts[insert].volume,
                2.0,
            )
        } else if let Some(c) = channel.filter(|c| path == format!("/channels/{c}/volume")) {
            (
                format!("channel/{}/volume", p.channels[c].id),
                p.channels[c].volume,
                1.5,
            )
        } else {
            out.push(op);
            continue;
        };
        match p
            .automation
            .iter()
            .position(|l| !l.mute && !l.points.is_empty() && l.target == target)
        {
            Some(i) => {
                let k = to / from.max(1e-4);
                for (j, q) in p.automation[i].points.iter().enumerate() {
                    out.push(set(
                        format!("/automation/{i}/points/{j}/value"),
                        json!(round3((q.value * k).clamp(0.0, max))),
                    ));
                }
            }
            None => out.push(op),
        }
    }
    out
}

fn fader_ops(p: &Project, channel: Option<usize>, insert: usize, db: f64) -> Vec<Value> {
    let mut ops = vec![];
    let mut left = db;
    let alone = channel.is_none()
        || p.channels
            .iter()
            .filter(|c| c.mixer.index() == insert)
            .count()
            == 1;
    let fader = p.mixer.inserts[insert].volume;
    if left > 0.05 && insert > 0 && alone && fader < 1.0 {
        let to = (fader.max(1e-4) * gain(left)).min(1.0);
        ops.push(set(
            format!("/mixer/inserts/{insert}/volume"),
            json!(round3(to)),
        ));
        left -= dsp::amp_db(to / fader.max(1e-4));
        if left <= 0.05 {
            return ops;
        }
    }
    let fader = ops
        .last()
        .and_then(|o| o["value"].as_f64())
        .unwrap_or(fader);
    if let Some(c) = channel {
        let v = p.channels[c].volume.max(1e-4);
        let want = v * gain(left);
        let to = want.clamp(0.0, 1.5);
        ops.push(set(format!("/channels/{c}/volume"), json!(round3(to))));
        left -= dsp::amp_db(to / v);
    }
    if left.abs() > 0.05 && insert > 0 && (channel.is_none() || left > 0.0) {
        let v = fader.max(1e-4);
        let to = (v * gain(left)).clamp(0.0, 2.0);
        // One op per path: the last one written wins.
        let path = format!("/mixer/inserts/{insert}/volume");
        ops.retain(|o| o["path"] != json!(path));
        ops.push(set(path, json!(round3(to))));
    }
    ops
}

/// The most an element's level can rise: its channel volume to 1.5, then its
/// insert's fader to 2 (none for the master's).
pub fn max_gain_db(p: &Project, channel: Option<usize>, insert: usize) -> f64 {
    let ch = channel
        .map(|c| dsp::amp_db(1.5 / p.channels[c].volume.max(1e-4)))
        .unwrap_or(0.0);
    let ins = if insert > 0 {
        dsp::amp_db(2.0 / p.mixer.inserts[insert].volume.max(1e-4))
    } else {
        0.0
    };
    (ch.max(0.0) + ins.max(0.0)).max(0.0)
}

/// Where a new EQ goes on an insert: before the time effects (delay,
/// reverb, chorus, phaser) and a limiter that ends the chain.
fn effect_slot(p: &Project, insert: usize) -> String {
    let fx = &p.mixer.inserts[insert].effects;
    let at = fx
        .iter()
        .position(|d| matches!(d.kind.as_str(), "delay" | "reverb" | "chorus" | "phaser"))
        .or_else(|| {
            fx.iter()
                .rposition(|d| d.kind == "limiter")
                .filter(|i| i + 1 == fx.len())
        });
    match at {
        Some(i) => format!("/mixer/inserts/{insert}/effects/{i}"),
        None => format!("/mixer/inserts/{insert}/effects/-"),
    }
}

/// An EQ move on an insert: `band` by `db` (a cut when negative) at
/// `freq_key` = `freq`, with `extra` settings (a Q). An EQ already there
/// takes it: its unused band, or the band already in use when it works near
/// that frequency (deepened, its own frequency kept). Otherwise a new EQ.
fn eq_ops(
    p: &Project,
    insert: usize,
    band: &str,
    db: f64,
    (freq_key, freq): (&str, f64),
    extra: &[(&str, f64)],
) -> (Vec<Value>, String) {
    let fx = &p.mixer.inserts[insert].effects;
    let path = |j: usize, k: &str| format!("/mixer/inserts/{insert}/effects/{j}/params/{k}");
    let on = model::insert_id(p, insert);
    let kind = match band {
        "low" => "low-shelf",
        "high" => "high-shelf",
        _ => "bell",
    };
    let move_ = |x: f64| {
        if x < 0.0 {
            format!("{:.1} dB {kind} cut", -x)
        } else {
            format!("{x:.1} dB {kind} boost")
        }
    };
    let eqs = || {
        fx.iter()
            .enumerate()
            .filter(|(_, d)| d.kind == "eq" && d.enabled)
    };
    if let Some((j, _)) = eqs().find(|(_, d)| d.param(band).abs() < 1e-9) {
        let mut ops = vec![
            set(path(j, band), json!(r1(db))),
            set(path(j, freq_key), json!(r1(freq))),
        ];
        ops.extend(extra.iter().map(|(k, v)| set(path(j, k), json!(r1(*v)))));
        return (
            ops,
            format!("a {} at {freq:.0} Hz with the EQ on {on}", move_(db)),
        );
    }
    // Within half an octave: the same band moved, at its own frequency.
    if let Some((j, d)) =
        eqs().find(|(_, d)| (d.param(freq_key).max(1.0) / freq).log2().abs() <= 0.5)
    {
        let (now, to) = (d.param(band), r1(d.param(band) + db));
        return (
            vec![set(path(j, band), json!(to))],
            format!(
                "the {band} band of the EQ on {on} ({:.0} Hz) from {now:+.1} to {to:+.1} dB",
                d.param(freq_key)
            ),
        );
    }
    let mut ps = serde_json::Map::new();
    ps.insert(band.to_string(), json!(r1(db)));
    ps.insert(freq_key.to_string(), json!(r1(freq)));
    for (k, v) in extra {
        ps.insert(k.to_string(), json!(r1(*v)));
    }
    (
        vec![
            json!({"op": "add", "path": effect_slot(p, insert), "value": {"type": "eq", "params": ps}}),
        ],
        format!("a new EQ on {on}: a {} at {freq:.0} Hz", move_(db)),
    )
}

/// The masker to cut for an element: its strongest masker on an insert of
/// its own, the bands to cut (power factors) and the bell's frequency and Q.
pub fn masker_cut(
    mix: &Mix,
    au: &Audibility,
    e: &Element,
) -> Option<(usize, [f64; BARKS], f64, f64)> {
    let m = au.maskers.first()?;
    let unit = &mix.units[m.unit];
    let k = unit.insert;
    // The EQ goes on the masker's insert: only when that insert carries the
    // masker alone (not the masked part, nor other parts of a bus).
    let p = mix.p;
    let channels = p.channels.iter().filter(|c| c.mixer.index() == k).count();
    let clips = p
        .playlist
        .clips
        .iter()
        .any(|c| !c.sample.is_empty() && c.mixer.index() == k);
    let alone = match unit.channel {
        Some(_) => channels == 1 && !clips,
        None => channels == 0,
    };
    if k == 0 || k == e.insert || !alone {
        return None;
    }
    let (lo, hi) = band_span(&m.bands, 0.15);
    let mut cut = [1.0; BARKS];
    for (z, c) in cut.iter_mut().enumerate() {
        if z >= lo && z <= hi {
            *c = 10f64.powf(-0.6);
        } else if z + 1 == lo || z == hi + 1 {
            *c = 10f64.powf(-0.3);
        }
    }
    let [f_lo, f_hi] = model::band_hz(lo, hi);
    let f0 = (f_lo * f_hi).sqrt().clamp(150.0, 10000.0);
    let q = (f0 / (f_hi - f_lo).max(1.0)).clamp(0.5, 4.0);
    Some((m.unit, cut, f0, q))
}

/// Where a lead sits against the mix once its balance is fixed (LU).
pub const LEAD_TARGET_DB: f64 = -5.0;

/// A lead's balance: boosted faders of the parts over it back to unity, and
/// the lead up as far as it needs (and its faders allow).
pub struct Balance {
    pub ops: Vec<Value>,
    pub why: String,
    /// The same, for the masking model.
    pub change: Change,
    /// Its level against the mix afterwards (LU), from the parts' energies.
    pub expected_rel: f64,
}

/// How far a unit's faders sit above unity (dB): its insert's (when the
/// insert carries it alone) and its channel's.
fn fader_boost(p: &Project, mix: &Mix, u: usize) -> (f64, f64) {
    let unit = &mix.units[u];
    let alone = match unit.channel {
        Some(_) => {
            p.channels
                .iter()
                .filter(|c| c.mixer.index() == unit.insert)
                .count()
                == 1
        }
        None => true,
    };
    let ins = if unit.insert > 0 && alone {
        dsp::amp_db(p.mixer.inserts[unit.insert].volume).max(0.0)
    } else {
        0.0
    };
    let ch = unit
        .channel
        .map(|c| dsp::amp_db(p.channels[c].volume).max(0.0))
        .unwrap_or(0.0);
    (ins, ch)
}

pub fn balance(
    p: &Project,
    mix: &Mix,
    blocks: &[usize],
    stretches: &[(f64, f64)],
    e: &Element,
    au: &Audibility,
    rel: f64,
) -> Option<Balance> {
    let energy: Vec<f64> = (0..mix.units.len())
        .map(|u| blocks.iter().map(|b| mix.unit_kms(u, *b)).sum())
        .collect();
    let total: f64 = energy.iter().sum();
    if total <= 0.0 {
        return None;
    }
    // The parts covering it and the parts carrying the mix, when a fader of
    // theirs is pushed above unity (at most two).
    let mut cands: Vec<usize> = au.maskers.iter().take(3).map(|m| m.unit).collect();
    for (u, x) in energy.iter().enumerate() {
        if x / total >= 0.15 && !cands.contains(&u) {
            cands.push(u);
        }
    }
    let rest: Vec<usize> = cands.into_iter().filter(|u| !e.units.contains(u)).collect();
    // Only the parts covering it come down further, and a little: a lead
    // is brought out by carving what masks it, not by gutting the mix.
    let maskers: Vec<usize> = au
        .maskers
        .iter()
        .take(3)
        .map(|m| m.unit)
        .filter(|u| !e.units.contains(u))
        .collect();
    let mut downs: Vec<(usize, f64, f64)> = rest
        .iter()
        .map(|&u| {
            let (ins, ch) = fader_boost(p, mix, u);
            (u, ins, ch)
        })
        .filter(|x| x.1 + x.2 >= 1.0)
        .collect();
    downs.sort_by(|a, b| (b.1 + b.2).total_cmp(&(a.1 + a.2)));
    downs.truncate(2);
    // A masker's EQ boosting the range where it covers the lead is the
    // first cause: that band back to +2 dB. Its effect on the masker's
    // energy: the share of it in the boosted band, cut by as much.
    // Only when it is masked, and near where the lead itself is loudest.
    let (lo, hi) = band_span(&au.spectrum, 0.25);
    let [own_lo, own_hi] = model::band_hz(lo, hi);
    let near_lead = |f: f64| f >= own_lo / 2.0 && f <= own_hi * 2.0;
    let eq = au
        .maskers
        .iter()
        .take(3)
        .filter(|_| au.fraction() < 0.95)
        .filter(|m| !e.units.contains(&m.unit))
        .filter_map(|m| eq_boost_over(p, mix, m.unit, &m.bands).map(|b| (m.unit, b)))
        .filter(|(_, b)| near_lead(b.freq))
        .max_by(|a, b| a.1.boost.total_cmp(&b.1.boost));
    let eq_factor = |u: usize| match &eq {
        Some((v, b)) if *v == u => {
            let share = band_share(mix, u, b.freq);
            1.0 - share + share * 10f64.powf(-(b.boost - EQ_KEEP) / 10.0)
        }
        _ => 1.0,
    };
    // How far each other part comes down: its boost back to unity, and
    // `under` more for the parts over the lead.
    let cut = |u: usize, under: f64| {
        let boost = downs
            .iter()
            .find(|d| d.0 == u)
            .map(|d| d.1 + d.2)
            .unwrap_or(0.0);
        boost + if maskers.contains(&u) { under } else { 0.0 }
    };
    let predicted = |lift: f64, under: f64| {
        let after: f64 = energy
            .iter()
            .enumerate()
            .map(|(u, x)| {
                let g = if e.units.contains(&u) {
                    lift
                } else {
                    -cut(u, under)
                };
                x * eq_factor(u) * 10f64.powf(g / 10.0)
            })
            .sum();
        rel + lift - dsp::db(after / total)
    };
    // An automation lane holding the lead down in these stretches, and up
    // elsewhere, is the cause: its points there back to its level elsewhere.
    let ride = lane_dip(p, e, stretches);
    let auto = ride.as_ref().map(|r| r.lift).unwrap_or(0.0);
    // The lead up 6 dB at most (more would push the master); the rest by
    // bringing the parts that cover it down (4 dB at most), in 0.5 dB steps.
    let most = max_gain_db(p, e.channel, e.insert).min(6.0);
    let predicted = |lift: f64, under: f64| predicted(auto + lift, under);
    let mut lift = 0.0;
    while predicted(lift, 0.0) < LEAD_TARGET_DB && lift + 0.5 <= most + 1e-9 {
        lift += 0.5;
    }
    let mut under = 0.0;
    while predicted(lift, under) < LEAD_TARGET_DB
        && under + 0.5 <= 4.0 + 1e-9
        && !maskers.is_empty()
    {
        under += 0.5;
    }
    let expected = predicted(lift, under);
    if expected < rel + 1.5 {
        return None;
    }
    let mut ops = vec![];
    let mut said = vec![];
    if let Some(r) = &ride {
        ops.extend(r.ops.iter().cloned());
        said.push(r.said.clone());
    }
    if let Some((u, b)) = &eq {
        ops.push(set(
            format!(
                "/mixer/inserts/{}/effects/{}/params/{}",
                mix.units[*u].insert, b.effect, b.band
            ),
            json!(EQ_KEEP),
        ));
        said.push(format!(
            "{}'s EQ boost of {:+.1} dB at {:.0} Hz down to {EQ_KEEP:+.1} dB",
            short_name(p, mix, *u),
            b.boost,
            b.freq
        ));
    }
    for &(u, ins, ch) in &downs {
        let unit = &mix.units[u];
        // Unity, and lower still with the others when they come down.
        let to = round3(gain(-under));
        if ins > 0.0 {
            ops.push(set(
                format!("/mixer/inserts/{}/volume", unit.insert),
                json!(to),
            ));
        }
        if let (Some(c), true) = (unit.channel, ch > 0.0) {
            ops.push(set(
                format!("/channels/{c}/volume"),
                json!(if ins > 0.0 { 1.0 } else { to }),
            ));
        }
        said.push(format!(
            "{}'s faders back to unity ({:.1} dB down)",
            short_name(p, mix, u),
            ins + ch
        ));
    }
    if lift > 0.0 {
        // On top of the lane put back up, when one drives its fader.
        let after = ride
            .as_ref()
            .and_then(|r| super::patched(p, &r.ops, "fix").ok());
        ops.extend(level_ops(
            after.as_ref().unwrap_or(p),
            e.channel,
            e.insert,
            lift,
        ));
        said.push(format!("{} up {lift:.1} dB", e.name));
    }
    if under > 0.0 {
        // On top of what the ops so far did to their faders.
        let after = super::patched(p, &ops, "fix").ok();
        let q = after.as_ref().unwrap_or(p);
        for &u in &maskers {
            ops.extend(lower_unit(q, mix, u, under));
        }
        let names: Vec<String> = maskers.iter().map(|u| short_name(p, mix, *u)).collect();
        said.push(format!(
            "{} down {under:.1} dB{}",
            names.join(" and "),
            if downs.is_empty() { "" } else { " more" }
        ));
    }
    let short = if expected < LEAD_TARGET_DB - 0.5 {
        " (as far as its faders go)"
    } else {
        ""
    };
    let masker = rest
        .iter()
        .chain(maskers.iter())
        .map(|&u| (u, cut(u, under)))
        .filter(|x| x.1 > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1));
    Some(Balance {
        ops,
        why: format!(
            "{} sits {:.1} dB under the mix: {} brings it to {:+.1} dB{short}",
            e.name,
            -rel,
            said.join(", and "),
            expected
        ),
        change: Change {
            gain_db: auto + lift,
            // The EQ's masker when there is one (the cause), with its fader
            // cut too; else the part brought down most.
            masker: match &eq {
                Some((u, b)) => {
                    let d = cut(*u, under);
                    Some((
                        *u,
                        std::array::from_fn(|z| {
                            let f = dsp::bark_centre(z) as f64;
                            let near = (f / b.freq).log2().abs() <= 1.0;
                            10f64.powf(-(d + if near { b.boost - EQ_KEEP } else { 0.0 }) / 10.0)
                        }),
                    ))
                }
                None => masker.map(|(u, d)| (u, [10f64.powf(-d / 10.0); BARKS])),
            },
        },
        expected_rel: expected,
    })
}

/// An automation lane on a part's volume holding it down in `stretches`.
struct Dip {
    ops: Vec<Value>,
    said: String,
    /// How much its level rises when the lane is put back (dB).
    lift: f64,
    /// The lane (index in `automation`).
    lane: usize,
}

/// A lane's value at `beat` (linear between its points; a step where two
/// points share a beat takes the later).
fn lane_at(points: &[rosaclef_core::AutomationPoint], beat: f64) -> Option<f64> {
    let after = points.iter().position(|q| q.beat > beat)?;
    if after == 0 {
        return Some(points[0].value);
    }
    let (a, b) = (&points[after - 1], &points[after]);
    let f = ((beat - a.beat) / (b.beat - a.beat).max(1e-9)).clamp(0.0, 1.0);
    Some(a.value + (b.value - a.value) * f)
}

fn lane_dip(p: &Project, e: &Element, stretches: &[(f64, f64)]) -> Option<Dip> {
    volume_dip(p, e.insert, e.channel, &e.name, stretches)
}

/// A lane on a part's volume (its insert's, or its channel's) holding it
/// 6 dB or more down in `stretches`.
fn volume_dip(
    p: &Project,
    insert: usize,
    channel: Option<usize>,
    name: &str,
    stretches: &[(f64, f64)],
) -> Option<Dip> {
    let targets: Vec<String> = std::iter::once(format!("insert/{insert}/volume"))
        .chain(channel.map(|c| format!("channel/{}/volume", p.channels[c].id)))
        .collect();
    lane_dip_on(p, &targets, stretches, name, 6.0)
}

/// A lane driving one of `targets` that holds `who` `min_lift` dB or more down in
/// `stretches` (written beats) and up elsewhere.
fn lane_dip_on(
    p: &Project,
    targets: &[String],
    stretches: &[(f64, f64)],
    who: &str,
    min_lift: f64,
) -> Option<Dip> {
    if stretches.is_empty() {
        return None;
    }
    let inside = |b: f64| {
        stretches
            .iter()
            .any(|(s0, s1)| b >= *s0 - 1e-6 && b <= *s1 + 1e-6)
    };
    p.automation.iter().enumerate().find_map(|(i, l)| {
        if l.mute || !targets.contains(&l.target) || l.points.is_empty() {
            return None;
        }
        let mut pts = l.points.clone();
        pts.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        // Its level in the stretches (their middles) and elsewhere (the
        // most it reaches outside them).
        let low = stretches
            .iter()
            .filter_map(|(s0, s1)| lane_at(&pts, (s0 + s1) / 2.0))
            .fold(f64::INFINITY, f64::min);
        let high = pts
            .iter()
            .filter(|q| !inside(q.beat))
            .map(|q| q.value)
            .fold(f64::NEG_INFINITY, f64::max);
        if !(low.is_finite() && high.is_finite() && low > 0.0) {
            return None;
        }
        let lift = dsp::amp_db(high / low);
        if lift < min_lift {
            return None;
        }
        let ops: Vec<Value> = l
            .points
            .iter()
            .enumerate()
            .filter(|(_, q)| inside(q.beat) && q.value < high)
            .map(|(j, _)| set(format!("/automation/{i}/points/{j}/value"), json!(round3(high))))
            .collect();
        (!ops.is_empty()).then(|| Dip {
            lane: i,
            ops,
            said: format!(
                "the automation lane \"{}\" back up to {high:.2} there (it holds {who} {lift:.1} dB down)",
                if l.name.is_empty() { &l.id } else { &l.name },
            ),
            lift,
        })
    })
}

/// What an EQ boost the balance takes back keeps (dB): a touch of presence.
const EQ_KEEP: f64 = 2.0;

/// An EQ band boosted 6 dB or more on a unit's insert, within an octave of
/// the bands where it covers the lead.
struct EqBoost {
    effect: usize,
    band: &'static str,
    freq: f64,
    boost: f64,
}

fn eq_boost_over(p: &Project, mix: &Mix, u: usize, bands: &[f64; BARKS]) -> Option<EqBoost> {
    let insert = mix.units[u].insert;
    if insert == 0 {
        return None;
    }
    let (lo, hi) = band_span(bands, 0.15);
    let [f_lo, f_hi] = model::band_hz(lo, hi);
    p.mixer.inserts[insert]
        .effects
        .iter()
        .enumerate()
        .filter(|(_, d)| d.enabled && d.kind == "eq")
        .flat_map(|(j, d)| {
            [("low", "lowFreq"), ("mid", "midFreq"), ("high", "highFreq")]
                .into_iter()
                .map(move |(band, fk)| EqBoost {
                    effect: j,
                    band,
                    freq: d.param(fk),
                    boost: d.param(band),
                })
        })
        .filter(|b| b.boost >= 6.0 && b.freq >= f_lo / 2.0 && b.freq <= f_hi * 2.0)
        .max_by(|a, b| a.boost.total_cmp(&b.boost))
}

/// The share of a unit's energy (over the range) in the six-band region
/// holding `freq`.
fn band_share(mix: &Mix, u: usize, freq: f64) -> f64 {
    let edges = [60.0, 250.0, 500.0, 2000.0, 6000.0];
    let k = edges.iter().filter(|e| freq >= **e).count();
    let mut six = [0.0; dsp::BANDS];
    for &h in &mix.inside {
        for (a, b) in six.iter_mut().zip(mix.unit_six(u, h)) {
            *a += b;
        }
    }
    let total: f64 = six.iter().sum();
    if total > 0.0 {
        six[k] / total
    } else {
        0.0
    }
}

/// Concrete changes for a buried or inaudible element, with what the model
/// predicts each does (`fractions`: under each gain, then the masker cut,
/// then the balance).
#[allow(clippy::too_many_arguments)]
pub fn suggestions(
    p: &Project,
    mix: &Mix,
    e: &Element,
    out: &ElementOut,
    gains: &[f64],
    fractions: &[f64],
    cut: Option<&(usize, [f64; BARKS], f64, f64)>,
    balance: Option<&Balance>,
    ok_at: f64,
) -> Vec<Suggestion> {
    let mut s = vec![];
    let rel = out.relative_to_mix_db.flatten();
    let now = out
        .audibility
        .as_ref()
        .map(|a| a.audible_fraction_pct / 100.0)
        .unwrap_or(0.0);
    // A lead under the mix: its balance first.
    if let Some(b) = balance {
        let at = gains.len() + usize::from(cut.is_some());
        s.push(Suggestion {
            why: b.why.clone(),
            patch: b.ops.clone(),
            expected_relative_to_mix_db: Some(r1(b.expected_rel)),
            expected_audible_fraction_pct: fractions.get(at).map(|f| pct(*f)),
            verified: None,
        });
    }
    // The masker cut first when it is enough: it keeps the balance.
    if let Some((u, _, f0, q)) = cut {
        let f = fractions.get(gains.len()).copied().unwrap_or(0.0);
        let unit = &mix.units[*u];
        let insert = unit.insert;
        if f >= now + 0.05 && insert > 0 {
            let (patch, what) = eq_ops(p, insert, "mid", -6.0, ("midFreq", *f0), &[("midQ", *q)]);
            s.push(Suggestion {
                why: format!(
                    "{} covers it around {:.0} Hz: {what} {}",
                    unit.id,
                    f0,
                    if f >= ok_at {
                        "clears it".to_string()
                    } else {
                        format!(
                            "helps (audible {:.0}% → {:.0}%, still under {:.0}%)",
                            now * 100.0,
                            f * 100.0,
                            ok_at * 100.0
                        )
                    }
                ),
                patch,
                expected_relative_to_mix_db: rel,
                expected_audible_fraction_pct: Some(pct(f)),
                verified: None,
            });
        }
    }
    // A low part covered by what another (not the bass or the drums)
    // carries under its own notes: a high-pass there.
    if let (Some((u, _, _, _)), Some(a)) = (cut, &out.audibility) {
        let unit = &mix.units[*u];
        let roles = rosaclef_core::critic::channel_roles(p);
        let role = unit
            .channel
            .and_then(|c| roles.get(c).copied())
            .unwrap_or("");
        let low_part = a.dominant_band_hz.is_some_and(|b| b[1] <= 300.0);
        // Not a bass by its sound (most of it under 250 Hz).
        let lows = |u: usize| {
            let six: [f64; dsp::BANDS] = mix.inside.iter().fold([0.0; dsp::BANDS], |mut acc, h| {
                for (a, b) in acc.iter_mut().zip(mix.unit_six(u, *h)) {
                    *a += b;
                }
                acc
            });
            let total: f64 = six.iter().sum();
            total > 0.0 && (six[0] + six[1]) / total >= 0.5
        };
        let has_hp = p.mixer.inserts[unit.insert]
            .effects
            .iter()
            .any(|d| d.enabled && d.kind == "filter" && d.option("mode") == "highpass");
        if low_part && unit.insert > 0 && !has_hp && !matches!(role, "bass" | "drums") && !lows(*u)
        {
            s.push(Suggestion {
                why: format!(
                    "{} carries low end under it: a high-pass at 150 Hz on {} leaves the lows to it",
                    unit.id,
                    model::insert_id(p, unit.insert)
                ),
                patch: vec![json!({"op": "add", "path": format!("/mixer/inserts/{}/effects/0", unit.insert),
                    "value": {"type": "filter", "params": {"cutoff": 150.0, "resonance": 0.0, "mix": 1.0}, "options": {"mode": "highpass"}}})],
                expected_relative_to_mix_db: None,
                expected_audible_fraction_pct: None,
                verified: None,
            });
        }
    }
    // Only as much gain as the faders can give (channel ≤ 1.5, insert ≤ 2),
    // and 12 dB at most unless its own fader sits that far under the
    // others' (then the fader is the cause; else what covers it is).
    let gap = e.channel.map(|c| channel_gap(p, c).0).unwrap_or(0.0);
    // More than 6 dB pushes everything else (and the master): beyond that
    // it is an arrangement conflict — unless its own fader is the cause.
    let most =
        max_gain_db(p, e.channel, e.insert).min(if gap >= 12.0 { gap.min(36.0) } else { 6.0 });
    let reachable = |g: &&f64| **g <= most + 0.05;
    let pick = gains
        .iter()
        .zip(fractions)
        .filter(|(g, _)| reachable(g))
        .find(|(_, f)| **f >= ok_at)
        .or_else(|| gains.iter().zip(fractions).rfind(|(g, _)| reachable(g)))
        // Only a change the model says helps.
        .filter(|(_, f)| **f >= now + 0.05);
    if let Some((g, f)) = pick {
        s.push(Suggestion {
            why: if *f >= ok_at {
                format!("{:.1} dB louder it comes through the parts covering it", g)
            } else {
                format!(
                    "{:.1} dB louder (as far as its faders go) it is audible {:.0}% of the time, still under {:.0}%",
                    g,
                    f * 100.0,
                    ok_at * 100.0
                )
            },
            patch: level_ops(p, e.channel, e.insert, *g),
            expected_relative_to_mix_db: rel.map(|r| r1(r + g)),
            expected_audible_fraction_pct: Some(pct(*f)),
            verified: None,
        });
    }
    // A filtered-down sound under a low masker: open the filter.
    if let (Some(c), Some(a)) = (e.channel, &out.audibility) {
        let dev = &p.channels[c].instrument;
        let spec = rosaclef_core::catalog::device(&dev.kind).and_then(|d| d.param("cutoff"));
        let dom_hi = a.dominant_band_hz.map(|b| b[1]).unwrap_or(0.0);
        if let Some(spec) = spec {
            let cutoff = dev.param("cutoff");
            if dom_hi > 0.0 && dom_hi < 1500.0 && cutoff < 3000.0 {
                let to = (cutoff * 2.0).max(1400.0).min(spec.max);
                if to > cutoff * 1.2 {
                    s.push(Suggestion {
                        why: format!(
                            "its fundamental sits under the maskers; opening the filter ({:.0} → {:.0} Hz) lets its harmonics through above them",
                            cutoff, to
                        ),
                        patch: vec![set(format!("/channels/{c}/instrument/params/cutoff"), json!(to.round()))],
                        expected_relative_to_mix_db: None,
                        expected_audible_fraction_pct: None,
                        verified: None,
                    });
                }
            }
        }
    }
    s
}

// ------------------------------------------------------------------ rules

/// Runs of consecutive rows (in playing order) where `hit` holds. A run
/// follows the performance (through a repeat) and breaks where the music
/// does not go on (another passage of a section).
fn runs(report: &Report, ctx: &Context, hit: &dyn Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let rows = &report.per_bar;
    let hops = &ctx.rows_hops;
    let seg = |i: usize, last: bool| {
        let h = if last {
            hops[i].last()
        } else {
            hops[i].first()
        };
        h.map(|h| ctx.mix.a.hops[*h].seg)
    };
    let follows = |i: usize| i > 0 && seg(i, false).is_some() && seg(i, false) == seg(i - 1, true);
    let mut out = vec![];
    let mut start: Option<usize> = None;
    for i in 0..=rows.len() {
        let on = i < rows.len() && hit(i);
        if let Some(s) = start {
            if !on || !follows(i) {
                out.push((s, i - 1));
                start = None;
            }
        }
        if on && start.is_none() {
            start = Some(i);
        }
    }
    out
}

/// Where runs of rows are, for people ("bars 1–7, 9–12 (pass 2), 13–18";
/// many stretches read "bars 2–140 (112 bars, in 20 stretches)"), and the
/// first and last bars.
fn place_of(report: &Report, runs: &[(usize, usize)]) -> (String, u32, u32) {
    let parts: Vec<(u32, u32, Option<u32>)> =
        runs.iter().map(|(a, b)| row_bars(report, *a, *b)).collect();
    let from = parts.iter().map(|x| x.0).min().unwrap_or(0);
    let to = parts.iter().map(|x| x.1).max().unwrap_or(0);
    let label = if parts.len() > 4 {
        let bars: u32 = parts.iter().map(|(f, t, _)| t - f + 1).sum();
        format!(
            "bars {from}–{to} ({bars} bars, in {} stretches)",
            parts.len()
        )
    } else {
        bars_label(&parts)
    };
    (label, from, to)
}

/// Bars for people: "bar 3", "bars 1–7, 9–12 (pass 2)"; a pass of `Some(0)`
/// runs through a repeat.
fn bars_label(parts: &[(u32, u32, Option<u32>)]) -> String {
    let list: Vec<String> = parts
        .iter()
        .map(|(f, t, pass)| {
            let span = if f == t {
                format!("{f}")
            } else {
                format!("{f}–{t}")
            };
            match pass {
                Some(0) => format!("{span} (with the repeat)"),
                Some(n) if *n > 1 => format!("{span} (pass {n})"),
                _ => span,
            }
        })
        .collect();
    let single = parts.len() == 1 && parts[0].0 == parts[0].1;
    format!(
        "{} {}",
        if single { "bar" } else { "bars" },
        list.join(", ")
    )
}

fn row_bars(report: &Report, a: usize, b: usize) -> (u32, u32, Option<u32>) {
    let rows = &report.per_bar[a..=b];
    let from = rows.iter().filter_map(|r| r.bar).min().unwrap_or(0);
    let to = rows
        .iter()
        .filter_map(|r| r.to_bar.or(r.bar))
        .max()
        .unwrap_or(0);
    let first = rows[0].pass;
    let pass = if rows.iter().all(|r| r.pass == first) {
        first
    } else {
        Some(0)
    };
    (from, to, pass)
}

/// A unit's name, as the producer knows it.
fn short_name(p: &Project, mix: &Mix, u: usize) -> String {
    let unit = &mix.units[u];
    match unit.channel {
        Some(c) => p.channels[c].name.clone(),
        None => p.mixer.inserts[unit.insert].name.clone(),
    }
}

/// A unit's name with where it sits in the mixer.
fn unit_name(p: &Project, mix: &Mix, u: usize) -> String {
    let insert = mix.units[u].insert;
    format!(
        "{} (insert {insert}, fader {:+.1} dB)",
        short_name(p, mix, u),
        dsp::amp_db(p.mixer.inserts[insert].volume)
    )
}

/// The energy under 500 Hz of a six-band spectrum.
fn low3(s: [f64; dsp::BANDS]) -> f64 {
    s[0] + s[1] + s[2]
}

/// An EQ on an insert boosting its low shelf by 3 dB or more: (effect
/// index, frequency, boost).
fn low_boost(p: &Project, insert: usize) -> Option<(usize, f64, f64)> {
    if insert == 0 {
        return None;
    }
    p.mixer.inserts[insert]
        .effects
        .iter()
        .enumerate()
        .filter(|(_, d)| d.enabled && d.kind == "eq" && d.param("low") >= 3.0)
        .map(|(j, d)| (j, d.param("lowFreq"), d.param("low")))
        .max_by(|a, b| a.2.total_cmp(&b.2))
}

/// An EQ on an insert boosting its highs (a high shelf, or a bell at 3 kHz
/// or more) by 6 dB or more: (effect index, band, frequency, boost).
/// An EQ boost in the low mids on an insert: a bell at 200-600 Hz, or a
/// low shelf reaching 250 Hz or more: (effect, band, frequency, boost).
fn lowmid_boost(p: &Project, insert: usize) -> Option<(usize, &'static str, f64, f64)> {
    if insert == 0 {
        return None;
    }
    p.mixer.inserts[insert]
        .effects
        .iter()
        .enumerate()
        .filter(|(_, d)| d.enabled && d.kind == "eq")
        .flat_map(|(j, d)| {
            [("mid", "midFreq"), ("low", "lowFreq")]
                .into_iter()
                .map(move |(band, fk)| (j, band, d.param(fk), d.param(band)))
        })
        .filter(|(_, band, f, g)| {
            *g > 0.0
                && match *band {
                    "mid" => (200.0..=600.0).contains(f),
                    _ => *f >= 250.0,
                }
        })
        .max_by(|a, b| a.3.total_cmp(&b.3))
}

/// An EQ boost in the presence range on an insert: a bell at 2-6 kHz, or a
/// high shelf from 4 kHz down: (effect, band, frequency, boost).
fn presence_boost(p: &Project, insert: usize) -> Option<(usize, &'static str, f64, f64)> {
    if insert == 0 {
        return None;
    }
    p.mixer.inserts[insert]
        .effects
        .iter()
        .enumerate()
        .filter(|(_, d)| d.enabled && d.kind == "eq")
        .flat_map(|(j, d)| {
            [("mid", "midFreq"), ("high", "highFreq")]
                .into_iter()
                .map(move |(band, fk)| (j, band, d.param(fk), d.param(band)))
        })
        .filter(|(_, band, f, g)| {
            *g > 0.0
                && match *band {
                    "mid" => (2000.0..=6000.0).contains(f),
                    _ => *f <= 4000.0,
                }
        })
        .max_by(|a, b| a.3.total_cmp(&b.3))
}

fn high_boost(p: &Project, insert: usize) -> Option<(usize, &'static str, f64, f64)> {
    if insert == 0 {
        return None;
    }
    p.mixer.inserts[insert]
        .effects
        .iter()
        .enumerate()
        .filter(|(_, d)| d.enabled && d.kind == "eq")
        .flat_map(|(j, d)| {
            [("high", "highFreq"), ("mid", "midFreq")]
                .into_iter()
                .map(move |(band, fk)| (j, band, d.param(fk), d.param(band)))
        })
        .filter(|(_, band, f, g)| *g >= 6.0 && (*band == "high" || *f >= 3000.0))
        .max_by(|a, b| a.3.total_cmp(&b.3))
}

/// How far channel `c`'s volume sits under the other channels' median
/// (dB), and that median.
fn channel_gap(p: &Project, c: usize) -> (f64, f64) {
    let mut others: Vec<f64> = p
        .channels
        .iter()
        .enumerate()
        .filter(|(i, ch)| *i != c && !ch.mute)
        .map(|(_, ch)| ch.volume)
        .collect();
    if others.is_empty() {
        return (0.0, 0.0);
    }
    others.sort_by(|a, b| a.total_cmp(b));
    let median = others[others.len() / 2];
    (
        dsp::amp_db(median.max(1e-4) / p.channels[c].volume.max(1e-4)),
        median,
    )
}

/// " Its channel volume is 0.05, 24.1 dB under the other channels' (0.80)."
/// when a part's channel volume sits 12 dB or more under the others' median.
fn fader_far_under(p: &Project, e: &ElementOut) -> String {
    let Some(c) =
        e.id.strip_prefix("channel:")
            .and_then(|id| p.channels.iter().position(|c| c.id == id))
    else {
        return String::new();
    };
    let (under, median) = channel_gap(p, c);
    if under < 12.0 {
        return String::new();
    }
    format!(
        " Its channel volume is {:.2}, {under:.1} dB under the other channels' ({median:.2}).",
        p.channels[c].volume
    )
}

/// The lowest value earlier findings' fixes set at `path` (infinity when
/// none does): two fixes of one setting agree on the deeper move, so
/// applying both leaves it there whatever their order.
fn proposed(out: &[(FindingOut, f64)], path: &str) -> f64 {
    out.iter()
        .flat_map(|(f, _)| &f.fix)
        .filter(|o| o["path"] == json!(path))
        .filter_map(|o| o["value"].as_f64())
        .fold(f64::INFINITY, f64::min)
}

/// A finding with no fix yet, keyed `rule|key`.
fn finding(rule: &'static str, key: &str, at: String, detail: String) -> FindingOut {
    FindingOut {
        severity: "warn",
        rule,
        key: format!("{rule}|{key}"),
        at,
        detail,
        ..Default::default()
    }
}

/// A compressor brought to about 3 dB of gain reduction from `mean`, the
/// loudness kept: a ratio of `ratio_max` at most, an attack that lets the
/// transients through (10 ms), a release that does not chase the beat
/// (150 ms), the threshold where it takes about 3 dB and the makeup down by
/// as much as it no longer takes.
fn glue(
    dev: &rosaclef_core::Device,
    base: &str,
    mean: f64,
    ratio_max: f64,
) -> (Vec<Value>, Vec<String>) {
    let (thr, ratio, makeup) = (
        dev.param("threshold"),
        dev.param("ratio").max(1.01),
        dev.param("makeup"),
    );
    let (attack, release) = (dev.param("attack"), dev.param("release"));
    let less = mean - 3.0;
    let ratio_to = ratio.min(ratio_max);
    let over = mean / (1.0 - 1.0 / ratio);
    let to = r1((thr + over - 3.0 / (1.0 - 1.0 / ratio_to)).clamp(-60.0, 0.0));
    let up = r1((makeup - less).max(0.0));
    let mut ops = vec![
        set(format!("{base}/threshold"), json!(to)),
        set(format!("{base}/makeup"), json!(up)),
    ];
    let mut said = vec![
        format!("threshold {thr:.1} → {to:.1} dB"),
        format!("makeup {makeup:.1} → {up:.1} dB"),
    ];
    if ratio_to < ratio - 0.05 {
        ops.push(set(format!("{base}/ratio"), json!(r1(ratio_to))));
        said.push(format!("ratio {ratio:.1}:1 → {ratio_to:.1}:1"));
    }
    if attack < 10.0 {
        ops.push(set(format!("{base}/attack"), json!(10.0)));
        said.push(format!("attack {attack:.1} → 10 ms"));
    }
    if release < 100.0 {
        ops.push(set(format!("{base}/release"), json!(150.0)));
        said.push(format!("release {release:.0} → 150 ms"));
    }
    (ops, said)
}

/// A master limiter's or compressor's release that holds steady instead
/// of breathing with the beat: a compressor about a beat (150-600 ms), a
/// limiter about half (100-300 ms).
fn steady_release_ms(p: &Project, kind: &str) -> f64 {
    let beat = 60000.0 / p.transport.bpm.max(1.0);
    if kind == "limiter" {
        (beat / 2.0).clamp(100.0, 300.0).round()
    } else {
        beat.clamp(150.0, 600.0).round()
    }
}

/// A channel's notes inside `ranges` (written beats) against its notes
/// elsewhere in the song: " Its notes there (pattern keys-b2) average
/// velocity 0.06 against 0.52 elsewhere." when they are much softer, or
/// much sparser.
fn soft_notes(p: &Project, channel: usize, ranges: &[(f64, f64)]) -> Option<String> {
    let ch = &p.channels[channel].id;
    let inside = |b: f64| ranges.iter().any(|(a, z)| b >= *a - 1e-6 && b < *z - 1e-6);
    let span: f64 = ranges.iter().map(|(a, z)| z - a).sum();
    let (mut vin, mut vout) = (vec![], vec![]);
    let mut pats: Vec<&str> = vec![];
    let mut song_end: f64 = 0.0;
    for c in &p.playlist.clips {
        let Some(pat) = p.pattern(&c.pattern) else {
            continue;
        };
        song_end = song_end.max(c.start + c.length);
        if pat.length <= 0.0 {
            continue;
        }
        let mut k = 0.0;
        while c.start - c.offset + k * pat.length < c.start + c.length {
            let origin = c.start - c.offset + k * pat.length;
            for n in pat.notes.iter().filter(|n| &n.channel == ch) {
                let at = origin + n.start;
                if at < c.start || at >= c.start + c.length {
                    continue;
                }
                if inside(at) {
                    vin.push(n.velocity);
                    if !pats.contains(&pat.id.as_str()) {
                        pats.push(pat.id.as_str());
                    }
                } else {
                    vout.push(n.velocity);
                }
            }
            k += 1.0;
        }
    }
    if vout.is_empty() {
        return None;
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let (a, b) = (mean(&vin), mean(&vout));
    let pats = if pats.is_empty() {
        String::new()
    } else {
        format!(" (pattern {})", pats.join(", "))
    };
    if !vin.is_empty() && a < 0.5 * b {
        return Some(format!(
            " Its notes there{pats} average velocity {a:.2} against {b:.2} elsewhere: they are played much softer."
        ));
    }
    let rest = (song_end - span).max(1.0);
    let (din, dout) = (vin.len() as f64 / span.max(1.0), vout.len() as f64 / rest);
    (din < 0.3 * dout).then(|| {
        format!(
            " It has {:.1} notes a beat there{pats} against {:.1} elsewhere: it plays much less.",
            din, dout
        )
    })
}

/// What makes a part cancel in mono: its stereo effects, else the audio
/// it plays (one channel of the file out of polarity).
fn phase_source(p: &Project, e: &ElementOut) -> String {
    let fx: Vec<&str> = p.mixer.inserts[e.insert]
        .effects
        .iter()
        .filter(|d| {
            d.enabled && matches!(d.kind.as_str(), "chorus" | "phaser" | "reverb" | "delay")
        })
        .map(|d| d.kind.as_str())
        .collect();
    if !fx.is_empty() {
        return format!(
            " Its insert's {} widen it: try less width or mix on them.",
            fx.join(", ")
        );
    }
    if e.kind == "insert" {
        let mut files: Vec<&str> = p
            .playlist
            .clips
            .iter()
            .filter(|c| !c.sample.is_empty() && c.mixer.index() == e.insert)
            .map(|c| c.sample.as_str())
            .collect();
        files.sort();
        files.dedup();
        if !files.is_empty() {
            return format!(
                " Nothing on its insert widens it: the audio ({}) has one channel out of polarity. The mixer has no polarity switch: flip one channel of the file (or use one channel), then point the clips at it.",
                files.join(", ")
            );
        }
    }
    String::new()
}

/// A part (not the lead) that drops out in stretches: its level there
/// against its loudest stretch, and the lane that holds it down.
fn dropout(p: &Project, t: &timeline::Timeline, e: &ElementOut) -> Option<(FindingOut, f64)> {
    // Where it usually sits: the median of its other stretches.
    let mut rest: Vec<&super::report::UnderMix> = e
        .by_section
        .iter()
        .filter(|u| {
            !e.buried_in
                .iter()
                .any(|b| u.from_bar >= b.from_bar && u.to_bar <= b.to_bar && u.pass == b.pass)
        })
        .collect();
    rest.sort_by(|a, b| a.relative_to_mix_db.total_cmp(&b.relative_to_mix_db));
    let top = *rest.get(rest.len() / 2)?;
    let worst = e
        .buried_in
        .iter()
        .map(|u| u.relative_to_mix_db)
        .fold(f64::INFINITY, f64::min);
    let bars: Vec<(u32, u32, Option<u32>)> = e
        .buried_in
        .iter()
        .map(|u| (u.from_bar, u.to_bar, u.pass))
        .collect();
    let stretches: Vec<(f64, f64)> = e
        .buried_in
        .iter()
        .map(|u| (t.bar_start(u.from_bar), t.bar_start(u.to_bar + 1)))
        .collect();
    let channel =
        e.id.strip_prefix("channel:")
            .and_then(|id| p.channels.iter().position(|c| c.id == id));
    let dip = volume_dip(p, e.insert, channel, &e.name, &stretches);
    let label = |u: &super::report::UnderMix| -> String {
        let name = match &u.section {
            Some(s) => s.clone(),
            None if u.from_bar == u.to_bar => format!("bar {}", u.from_bar),
            None => format!("bars {}–{}", u.from_bar, u.to_bar),
        };
        match u.pass {
            Some(n) => format!("{name} (pass {n})"),
            None => name,
        }
    };
    let low: Vec<String> = e
        .by_section
        .iter()
        .filter(|u| {
            e.buried_in
                .iter()
                .any(|b| u.from_bar >= b.from_bar && u.to_bar <= b.to_bar && u.pass == b.pass)
        })
        .map(|u| format!("{} {:.1}", label(u), u.relative_to_mix_db))
        .collect();
    let cause = match &dip {
        Some(d) => {
            // "the automation lane "X" back up …": the lane.
            let lane = d.said.split(" back up").next().unwrap_or("");
            format!(" The{} holds it {:.1} dB down there.", lane.trim_start_matches("the"), d.lift)
        }
        None => match channel.and_then(|c| soft_notes(p, c, &stretches)) {
            Some(why) => format!(" Nothing in the mixer holds it down there.{why}"),
            None => " Nothing in the mixer holds it down there: it plays fewer or softer notes (the arrangement).".into(),
        },
    };
    let detail = format!(
        "{} drops out in {}: {:.1} dB under the mix there ({}), against {:.1} dB elsewhere ({}{}).{cause}",
        e.name,
        bars_label(&bars),
        -worst,
        low.join(", "),
        top.relative_to_mix_db,
        if rest.len() > 2 { "median: " } else { "" },
        label(top),
    );
    // A ride down in a breakdown is a choice; a part gone (25 dB under the
    // mix) is not.
    let gone = worst < -25.0;
    let (fix, fix_label, severity) = match dip {
        Some(d) => (d.ops, d.said, if gone { "warn" } else { "info" }),
        None => (vec![], String::new(), "info"),
    };
    Some((
        FindingOut {
            severity,
            element: Some(e.id.clone()),
            from_bar: bars.first().map(|b| b.0),
            to_bar: bars.last().map(|b| b.1),
            fix,
            fix_label,
            ..finding(
                "part-dropout",
                &e.id,
                format!("{} in {}", e.name, bars_label(&bars)),
                detail,
            )
        },
        top.relative_to_mix_db - worst,
    ))
}

/// The gain ops lowering a contributor by `db`.
fn lower_unit(p: &Project, mix: &Mix, u: usize, db: f64) -> Vec<Value> {
    let unit = &mix.units[u];
    // Audio clips straight to the master: the master fader comes after the
    // limiter, so the clips themselves come down.
    if unit.insert == 0 && unit.channel.is_none() {
        return p
            .playlist
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.sample.is_empty() && c.mixer.index() == 0)
            .map(|(i, c)| {
                set(
                    format!("/playlist/clips/{i}/gain"),
                    json!(round3(c.gain * gain(-db))),
                )
            })
            .collect();
    }
    let alone = unit.insert > 0
        && p.channels
            .iter()
            .filter(|c| c.mixer.index() == unit.insert)
            .count()
            <= 1;
    if unit.channel.is_none() || alone {
        let v = p.mixer.inserts[unit.insert].volume;
        return vec![set(
            format!("/mixer/inserts/{}/volume", unit.insert),
            json!(round3(v * gain(-db))),
        )];
    }
    level_ops(p, unit.channel, unit.insert, -db)
}

/// Bring the master's input down by `cut` dB: the parts that dominate it,
/// or — when no part does — the limiter's drive, then every fader.
/// A master limiter's input gain (dB) above which the drive itself is the
/// first suspect of an overload.
const DRIVE_OK: f64 = 3.0;

/// The master's last limiter and its input gain.
fn master_drive(p: &Project) -> Option<(usize, f64)> {
    let fx = &p.mixer.inserts[0].effects;
    fx.iter()
        .rposition(|d| d.enabled && d.kind == "limiter")
        .map(|j| (j, fx[j].param("gain")))
}

/// Bring the master's input down by `cut` dB: a hard-driven limiter's
/// drive first, then the parts that dominate, or — when none does — the
/// remaining drive and every fader.
fn overload_fix(p: &Project, mix: &Mix, top: &[(usize, f64)], cut: f64) -> (Vec<Value>, String) {
    let mut ops = vec![];
    let mut said = vec![];
    let mut left = cut;
    let mut drive = master_drive(p);
    let mut ease = |left: &mut f64, ops: &mut Vec<Value>, said: &mut Vec<String>| {
        if let Some((j, g)) = drive.take() {
            if g > 0.05 && *left > 0.05 {
                let to = (g - *left).max(0.0);
                ops.push(set(
                    format!("/mixer/inserts/0/effects/{j}/params/gain"),
                    json!(r1(to)),
                ));
                said.push(format!("the limiter's input gain {g:+.1} → {to:+.1} dB"));
                *left -= g - to;
            }
        }
    };
    if master_drive(p).is_some_and(|d| d.1 > DRIVE_OK) {
        ease(&mut left, &mut ops, &mut said);
    }
    let heavy: Vec<usize> = top
        .iter()
        .take(3)
        .filter(|x| x.1 >= 0.15)
        .map(|x| x.0)
        .collect();
    // Less than half a dB more is within the measurement's reach: done.
    if left < 0.5 {
        left = 0.0;
    }
    if left > 0.05 && !heavy.is_empty() {
        ops.extend(heavy.iter().flat_map(|u| lower_unit(p, mix, *u, left)));
        let names: Vec<String> = heavy.iter().map(|u| short_name(p, mix, *u)).collect();
        said.push(format!("{} down {left:.1} dB", names.join(" and ")));
        left = 0.0;
    }
    if left > 0.05 {
        ease(&mut left, &mut ops, &mut said);
    }
    if left > 0.05 {
        let f = gain(-left);
        for (i, ins) in p.mixer.inserts.iter().enumerate().skip(1) {
            ops.push(set(
                format!("/mixer/inserts/{i}/volume"),
                json!(round3(ins.volume * f)),
            ));
        }
        for (c, ch) in p.channels.iter().enumerate() {
            if ch.mixer.index() == 0 {
                ops.push(set(
                    format!("/channels/{c}/volume"),
                    json!(round3(ch.volume * f)),
                ));
            }
        }
        said.push(format!("every fader down {left:.1} dB (no part dominates)"));
    }
    (ops, said.join(", and "))
}

pub fn derive(report: &Report, ctx: &Context, o: &Options) -> Vec<FindingOut> {
    let rows_hops = &ctx.rows_hops;
    let p = ctx.project;
    let mix = &ctx.mix;
    let mut out: Vec<(FindingOut, f64)> = vec![];
    let strict = o.threshold == Threshold::Strict;
    let loose = o.threshold == Threshold::Loose;
    let pick = |n: f64, s: f64, l: f64| {
        if strict {
            s
        } else if loose {
            l
        } else {
            n
        }
    };

    // master-overload
    if ctx.include_master && o.has(Check::Levels) {
        let has_limiter = p.mixer.inserts[0]
            .effects
            .iter()
            .any(|d| d.enabled && d.kind == "limiter");
        let thr = pick(1.0, 0.0, 3.0);
        // A limiter catching the odd peak is mastering; one taking 6 dB is not.
        // Loud genres run a limiter 6 dB deep on their peaks: overload is
        // more than that.
        let gr_thr = pick(8.0, 5.0, 11.0);
        let hit = |i: usize| {
            let r = &report.per_bar[i];
            let pre = r.pre_limiter_peak_dbfs.unwrap_or(-99.0);
            if has_limiter {
                pre > thr && r.limiter_gr_max_db.unwrap_or(0.0) >= gr_thr
            } else {
                r.peak_dbfs.unwrap_or(-99.0) > 0.0
            }
        };
        // One finding for every bar it happens in: one problem, one fix.
        let found = runs(report, ctx, &hit);
        if !found.is_empty() {
            let rows: Vec<usize> = found.iter().flat_map(|(a, b)| *a..=*b).collect();
            let hs: Vec<usize> = rows
                .iter()
                .flat_map(|i| rows_hops[*i].iter().copied())
                .collect();
            let pre = rows
                .iter()
                .filter_map(|i| report.per_bar[*i].pre_limiter_peak_dbfs)
                .fold(-99.0, f64::max);
            let lims = mix.a.master_gr("limiter");
            let (mut grmax, mut above) = (0f64, 0usize);
            for h in &hs {
                let m = lims.iter().map(|g| g.max[*h] as f64).fold(0.0, f64::max);
                grmax = grmax.max(m);
                if lims.iter().map(|g| g.mean[*h] as f64).sum::<f64>() >= 3.0 {
                    above += 1;
                }
            }
            let top = mix.ranked(&hs, &|u, h| mix.unit_ms(u, h));
            let named: Vec<String> = top
                .iter()
                .take(3)
                .filter(|x| x.1 >= 0.02)
                .map(|(u, s)| format!("{} {:.0}%", unit_name(p, mix, *u), s * 100.0))
                .collect();
            let cut = if has_limiter {
                (grmax - 2.0).clamp(1.5, 9.0)
            } else {
                (pre + 1.0).clamp(1.0, 9.0)
            };
            let (fix, fix_label) = overload_fix(p, mix, &top, cut);
            let (at, fb, tb) = place_of(report, &found);
            let drive = match master_drive(p) {
                Some((_, g)) if g > DRIVE_OK => {
                    format!(" (its input gain alone adds {g:+.1} dB)")
                }
                _ => String::new(),
            };
            let detail = if has_limiter {
                format!(
                    "pre-limiter peaks {:+.1} dBFS{drive}; the limiter reduces up to {:.1} dB, above 3 dB {}% of the time. Top contributors: {}.",
                    pre, grmax, pct(above as f64 / hs.len().max(1) as f64), named.join(", ")
                )
            } else {
                format!(
                    "the output clips at {:+.1} dBFS (no limiter on the master). Top contributors: {}.",
                    pre, named.join(", ")
                )
            };
            out.push((
                FindingOut {
                    from_bar: Some(fb),
                    to_bar: Some(tb),
                    fix,
                    fix_label,
                    ..finding("master-overload", "master", at, detail)
                },
                pre + grmax,
            ));
        }
    }

    // true-peak: over the delivery target's limit (`--target`), else
    // streaming's -1 dBTP (strict -2, loose 0).
    let target = o.target.as_deref().and_then(super::targets::find);
    let limit = target
        .map(|t| t.true_peak_for(report.master.integrated_lufs.flatten()))
        .unwrap_or_else(|| pick(-1.0, -2.0, 0.0));
    if ctx.include_master && o.has(Check::Levels) {
        if let Some(tp) = report.master.true_peak_dbtp.filter(|tp| *tp > limit) {
            // Inter-sample peaks follow a sample-peak ceiling loosely: half a
            // dB of margin under the limit.
            let down = tp - limit + 0.5;
            let (fix, label, how) = match master_drive(p) {
                Some((j, _)) => {
                    let c = p.mixer.inserts[0].effects[j].param("ceiling");
                    let to = r1((c - down).max(-12.0));
                    (
                        vec![set(
                            format!("/mixer/inserts/0/effects/{j}/params/ceiling"),
                            json!(to),
                        )],
                        format!("the limiter's ceiling {c:+.1} → {to:+.1} dB"),
                        format!(" The limiter holds sample peaks to {c:+.1} dB; the peaks between samples go over it."),
                    )
                }
                None => {
                    let v = p.mixer.inserts[0].volume;
                    (
                        vec![set(
                            "/mixer/inserts/0/volume".into(),
                            json!(round3(v * gain(-down))),
                        )],
                        format!("the master fader down {down:.1} dB"),
                        " There is no limiter on the master.".to_string(),
                    )
                }
            };
            let loudest = report
                .per_bar
                .iter()
                .filter(|r| r.true_peak_dbtp.is_some())
                .max_by(|a, b| {
                    a.true_peak_dbtp
                        .unwrap_or(-99.0)
                        .total_cmp(&b.true_peak_dbtp.unwrap_or(-99.0))
                });
            let at = match loudest.and_then(|r| r.bar) {
                Some(b) => format!("master (highest in bar {b})"),
                None => "master".to_string(),
            };
            out.push((
                FindingOut {
                    element: Some(model::insert_id(p, 0)),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    fix,
                    fix_label: label,
                    ..finding(
                        "true-peak",
                        "master",
                        at,
                        format!(
                            "the output's true peak reaches {tp:+.1} dBTP, over {}'s {limit:+.1} dBTP: peaks between the samples clip when the song is converted or encoded.{how}",
                            target.map(|t| t.name).unwrap_or("streaming")
                        ),
                    )
                },
                tp,
            ));
        }
    }

    // over-compression: a master compressor or limiter holding heavy gain
    // reduction all the time squashes the mix flat (pumping or not).
    if ctx.include_master && o.has(Check::GainReduction) {
        let hs = &mix.inside;
        for g in mix.a.gr.iter().filter(|g| g.insert == 0) {
            if hs.is_empty() {
                continue;
            }
            let mean = hs.iter().map(|h| g.mean[*h] as f64).sum::<f64>() / hs.len() as f64;
            let above = hs.iter().filter(|h| g.mean[**h] >= 3.0).count() as f64 / hs.len() as f64;
            if mean < pick(6.0, 4.0, 9.0) || above < 0.5 {
                continue;
            }
            let dev = &p.mixer.inserts[0].effects[g.fx];
            let base = format!("/mixer/inserts/0/effects/{}/params", g.fx);
            // Down to about 3 dB of gain reduction, the loudness kept.
            let less = mean - 3.0;
            let (fix, label) = if g.kind == "compressor" {
                // A bus compressor glues: a gentle ratio.
                let (ops, said) = glue(dev, &base, mean, 2.5);
                (ops, format!("the compressor's {}", said.join(", ")))
            } else {
                let drive = dev.param("gain");
                let to = r1((drive - less).max(0.0)).min(proposed(&out, &format!("{base}/gain")));
                (
                    vec![set(format!("{base}/gain"), json!(to))],
                    format!("the limiter's input gain {drive:+.1} → {to:+.1} dB"),
                )
            };
            let dynamics = match (report.master.lra.flatten(), report.master.plr_db.flatten()) {
                (Some(l), Some(pl)) => format!(" (LRA {l:.1} LU, PLR {pl:.1} dB)"),
                _ => String::new(),
            };
            out.push((
                FindingOut {
                    element: Some(model::insert_id(p, 0)),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    fix,
                    fix_label: label,
                    ..finding(
                        "over-compression",
                        &format!("insert:0#{}", g.fx),
                        format!("master {} (effect {})", g.kind, g.fx),
                        format!(
                            "it takes {mean:.1} dB off on average, over 3 dB {}% of the time: the mix is squashed flat{dynamics}.",
                            pct(above)
                        ),
                    )
                },
                mean,
            ));
        }
    }

    // part-over-compression: a compressor on a part crushing it.
    if o.has(Check::GainReduction) {
        for g in mix
            .a
            .gr
            .iter()
            .filter(|g| g.insert > 0 && g.kind == "compressor")
        {
            // Where the part plays.
            let hs: Vec<usize> = match mix.insert_stream(g.insert) {
                Some(st) => mix
                    .inside
                    .iter()
                    .copied()
                    .filter(|h| mix.ms(st, *h) > 1e-9)
                    .collect(),
                None => mix.inside.clone(),
            };
            if hs.is_empty() {
                continue;
            }
            let mean = hs.iter().map(|h| g.mean[*h] as f64).sum::<f64>() / hs.len() as f64;
            let most = hs.iter().map(|h| g.max[*h] as f64).fold(0.0, f64::max);
            let dev = &p.mixer.inserts[g.insert].effects[g.fx];
            let attack = dev.param("attack");
            let crushed =
                mean >= pick(8.0, 6.0, 12.0) || (most >= pick(15.0, 12.0, 20.0) && attack < 2.0);
            if !crushed {
                continue;
            }
            let base = format!("/mixer/inserts/{}/effects/{}/params", g.insert, g.fx);
            let (fix, said) = glue(dev, &base, mean.max(3.5), 4.0);
            let name = p.mixer.inserts[g.insert].name.clone();
            out.push((
                FindingOut {
                    element: Some(model::insert_id(p, g.insert)),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    fix,
                    fix_label: format!("{name}'s compressor: {}", said.join(", ")),
                    ..finding(
                        "part-over-compression",
                        &format!("{}#{}", model::insert_id(p, g.insert), g.fx),
                        format!("{name} compressor (effect {})", g.fx),
                        format!(
                            "{name}'s compressor takes {mean:.1} dB off on average where it plays and up to {most:.1} dB (ratio {:.0}:1, attack {attack:.1} ms): its transients are gone — flat, lifeless, pumping.",
                            dev.param("ratio")
                        ),
                    )
                },
                mean.max(most / 3.0),
            ));
        }
    }

    // lr-balance: the mix leans to one side.
    if ctx.include_master && o.has(Check::Stereo) {
        if let Some(b) = mix.balance(mix.master_out, &mix.inside) {
            let explained = out.iter().any(|(f, _)| f.rule == "low-end-off-centre");
            if b.abs() >= pick(1.5, 1.0, 3.0) && !explained {
                let side: Vec<String> = report
                    .elements
                    .iter()
                    .filter(|e| {
                        e.balance_db
                            .is_some_and(|x| x.signum() == b.signum() && x.abs() >= 3.0)
                    })
                    .map(|e| format!("{} {:+.1} dB", e.name, e.balance_db.unwrap_or(0.0)))
                    .collect();
                out.push((
                    FindingOut {
                        severity: "info",
                        ..finding(
                            "lr-balance",
                            "master",
                            "master".into(),
                            format!(
                                "the mix is {:.1} dB louder on the {}{}.",
                                b.abs(),
                                if b > 0.0 { "left" } else { "right" },
                                if side.is_empty() {
                                    String::new()
                                } else {
                                    format!(" (to that side: {})", side.join(", "))
                                }
                            ),
                        )
                    },
                    b.abs(),
                ));
            }
        }
    }

    // master-fader: the master fader comes after the limiter.
    if ctx.include_master && o.has(Check::Levels) {
        let v = p.mixer.inserts[0].volume;
        if let Some((j, drive)) = master_drive(p) {
            let db = dsp::amp_db(v.max(1e-4));
            if !(-1.0..=0.05).contains(&db) {
                let to = r1((drive + db).clamp(-24.0, 24.0));
                out.push((
                    FindingOut {
                        severity: if db > 0.05 { "warn" } else { "info" },
                        fix: vec![
                            set("/mixer/inserts/0/volume".into(), json!(1.0)),
                            set(format!("/mixer/inserts/0/effects/{j}/params/gain"), json!(to)),
                        ],
                        fix_label: format!(
                            "the master fader {db:+.1} → 0 dB, and the limiter's input gain {drive:+.1} → {to:+.1} dB (the same loudness)"
                        ),
                        ..finding(
                            "master-fader",
                            "master",
                            "master".into(),
                            if db > 0.05 {
                                format!("the master fader is at {db:+.1} dB after the limiter: it pushes the limited peaks over the ceiling.")
                            } else {
                                format!("the master fader is at {db:+.1} dB after the limiter: it turns the limited mix down, so the ceiling no longer sets the peaks and the limiter works for nothing. Set the level before the limiter (its input gain).")
                            },
                        )
                    },
                    db.abs(),
                ));
            }
        }
    }

    // loud-master: louder than streaming plays it (no --target asked; a
    // target's verdict says it then).
    if ctx.include_master && o.has(Check::Levels) && target.is_none() {
        if let Some(i) = report.master.integrated_lufs.flatten() {
            let at = pick(-10.0, -12.0, -8.0);
            if i > at {
                let down = r1(i - (-14.0));
                let drive = master_drive(p).filter(|(_, d)| *d > 0.0);
                let (fix, fix_label) = match drive {
                    Some((j, d)) => {
                        let to = r1((d - (i - at + 2.0)).max(0.0));
                        (
                            vec![set(
                                format!("/mixer/inserts/0/effects/{j}/params/gain"),
                                json!(to),
                            )],
                            format!("the limiter's input gain {d:+.1} → {to:+.1} dB"),
                        )
                    }
                    None => (vec![], String::new()),
                };
                out.push((
                    FindingOut {
                        severity: "info",
                        fix,
                        fix_label,
                        ..finding(
                            "loud-master",
                            "master",
                            "master".into(),
                            format!(
                                "the master is {i:.1} LUFS (PLR {}): streaming plays it about {down:.1} dB quieter (normalized to -14 LUFS), so the loudness is lost and the squashed peaks stay. --target names a destination.",
                                report
                                    .master
                                    .plr_db
                                    .flatten()
                                    .map(|x| format!("{x:.1} dB"))
                                    .unwrap_or_else(|| "—".into())
                            ),
                        )
                    },
                    i - at,
                ));
            }
        }
    }

    // fast-limiter-release: a limiter releasing faster than the bass's
    // waveform rides it: the low end distorts.
    if ctx.include_master && o.has(Check::GainReduction) {
        let gr = report
            .master
            .limiter_gain_reduction_db
            .as_ref()
            .map(|g| g.max)
            .unwrap_or(0.0);
        if let Some(j) = p.mixer.inserts[0]
            .effects
            .iter()
            .rposition(|d| d.enabled && d.kind == "limiter")
        {
            let rel = p.mixer.inserts[0].effects[j].param("release");
            if rel < 20.0 && gr >= 3.0 {
                let to = steady_release_ms(p, "limiter");
                out.push((
                    FindingOut {
                        fix: vec![set(format!("/mixer/inserts/0/effects/{j}/params/release"), json!(to))],
                        fix_label: format!("the limiter's release {rel:.0} → {to:.0} ms (about half a beat)"),
                        ..finding(
                            "fast-limiter-release",
                            "master",
                            format!("master limiter (effect {j})"),
                            format!(
                                "the master limiter releases in {rel:.0} ms and takes up to {gr:.1} dB: faster than a bass note's cycle (25 ms at 40 Hz), it rides the waveform and distorts the low end."
                            ),
                        )
                    },
                    gr,
                ));
            }
        }
    }

    // limiter-pumping
    if ctx.include_master && o.has(Check::GainReduction) {
        let bpm = p.transport.bpm.max(1.0);
        let beat_hops = ((60.0 / bpm) * mix.a.sr as f64 / super::analyze::HOP as f64)
            .round()
            .max(2.0) as usize;
        for g in mix.a.gr.iter().filter(|g| g.insert == 0) {
            let hs = &mix.inside;
            if hs.len() < beat_hops * 4 {
                continue;
            }
            let mut swings: Vec<f64> = hs
                .chunks(beat_hops)
                .filter(|c| c.len() == beat_hops)
                .map(|c| {
                    let v: Vec<f64> = c.iter().map(|h| g.max[*h] as f64).collect();
                    v.iter().copied().fold(0.0, f64::max)
                        - v.iter().copied().fold(f64::INFINITY, f64::min)
                })
                .collect();
            if swings.is_empty() {
                continue;
            }
            swings.sort_by(|a, b| a.total_cmp(b));
            let median = swings[swings.len() / 2];
            let above = hs.iter().filter(|h| g.mean[**h] >= 3.0).count() as f64 / hs.len() as f64;
            let mean = hs.iter().map(|h| g.mean[*h] as f64).sum::<f64>() / hs.len() as f64;
            let most = hs.iter().map(|h| g.max[*h] as f64).fold(0.0, f64::max);
            if median < pick(4.0, 3.0, 6.0) || above < pick(0.2, 0.1, 0.35) {
                continue;
            }
            let dev = &p.mixer.inserts[0].effects[g.fx];
            let spec = rosaclef_core::catalog::device(&dev.kind).and_then(|d| d.param("release"));
            let rel = dev.param("release");
            let mut fix = vec![];
            let mut said = vec![];
            let base = format!("/mixer/inserts/0/effects/{}/params", g.fx);
            if let Some(s) = spec {
                // A release shorter than a beat recovers between the hits
                // and breathes with them: about a beat holds it steady.
                let to = steady_release_ms(p, &g.kind).min(s.max).max(rel);
                if to > rel {
                    fix.push(set(format!("{base}/release"), json!(to)));
                    said.push(format!("release {rel:.0} → {to:.0} ms"));
                }
            }
            if g.kind == "limiter" && dev.param("gain") > 0.0 {
                // Less drive, until it takes 3 dB at most.
                let drive = dev.param("gain");
                let to = r1((drive - (most - 3.0).max(mean)).max(0.0))
                    .min(proposed(&out, &format!("{base}/gain")));
                fix.push(set(format!("{base}/gain"), json!(to)));
                said.push(format!("input gain {drive:+.1} → {to:+.1} dB"));
            } else if g.kind == "compressor" {
                let thr = dev.param("threshold");
                let to = r1((thr + mean / 2.0).min(0.0));
                fix.push(set(format!("{base}/threshold"), json!(to)));
                said.push(format!("threshold {thr:.1} → {to:.1} dB"));
            }
            out.push((
                FindingOut {
                    element: Some(model::insert_id(p, 0)),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    fix,
                    fix_label: format!("the {}'s {}", g.kind, said.join(", ")),
                    ..finding(
                        "limiter-pumping",
                        &format!("insert:0#{}", g.fx),
                        format!("master {} (effect {})", g.kind, g.fx),
                        format!(
                            "its gain reduction swings {:.1} dB within a beat (median), above 3 dB {}% of the time (mean {:.1} dB): the mix breathes with the beat.",
                            median, pct(above), mean
                        ),
                    )
                },
                median,
            ));
        }
    }

    // masked-lead, inaudible-part, phase (elements)
    for e in &report.elements {
        // A part playing under 5 % of the range (a release tail) is not
        // judged for its audibility.
        if e.active_pct < 5.0 && !matches!(e.verdict, "ok" | "dominant" | "overloading") {
            continue;
        }
        let lead = e.lead;
        let frac = e
            .audibility
            .as_ref()
            .map(|a| a.audible_fraction_pct)
            .unwrap_or(100.0);
        let maskers = e
            .audibility
            .as_ref()
            .and_then(|a| a.masked_by.as_ref())
            .map(|m| {
                m.iter()
                    .map(|x| {
                        format!(
                            "{} {:.0}–{:.0} Hz ({:+.0} dB)",
                            x.id, x.band_hz[0], x.band_hz[1], x.masking_db
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let level = e
            .relative_to_mix_db
            .flatten()
            .map(|r| format!(" ({r:+.1} dB against the mix)"))
            .unwrap_or_default();
        let masked = if maskers.is_empty() {
            ".".to_string()
        } else {
            format!("; masked by {maskers}.")
        };
        let at = format!(
            "{} in bars {}–{}",
            e.name, report.range.from_bar, report.range.to_bar
        );
        // Judged only when its level or audibility was asked for.
        let judged = o.has(Check::Audibility) || o.has(Check::Masking) || o.has(Check::Levels);
        let found = if !judged {
            None
        } else if lead && matches!(e.verdict, "inaudible" | "buried") {
            // Under the mix only in stretches: there, with the whole range's
            // level for comparison.
            let stretches: Vec<(u32, u32, Option<u32>)> = e
                .buried_in
                .iter()
                .map(|u| (u.from_bar, u.to_bar, u.pass))
                .collect();
            let (at, where_) = if stretches.is_empty() {
                (at, String::new())
            } else {
                let worst = e
                    .buried_in
                    .iter()
                    .map(|u| u.relative_to_mix_db)
                    .fold(f64::INFINITY, f64::min);
                (
                    format!("{} in {}", e.name, bars_label(&stretches)),
                    format!(
                        " in {} (down to {:.1} dB under it there)",
                        bars_label(&stretches),
                        -worst
                    ),
                )
            };
            let detail = match e.relative_to_mix_db.flatten() {
                Some(r) if where_.is_empty() => format!(
                    "the lead sits {:.1} dB under the mix (its insert fader at {:+.1} dB) and is audible {frac:.0}% of the time it plays{masked}",
                    -r, e.level.fader_db
                ),
                Some(r) => format!(
                    "the lead drops under the mix{where_}, though it sits {:.1} dB under it over bars {}–{} (its insert fader at {:+.1} dB{}); audible {frac:.0}% of the time it plays{masked}",
                    -r,
                    report.range.from_bar,
                    report.range.to_bar,
                    e.level.fader_db,
                    if e.level.automated.is_empty() {
                        String::new()
                    } else {
                        format!(", its volume automated by {}", e.level.automated.join(", "))
                    }
                ),
                None => format!("the lead is audible only {frac:.0}% of the time it plays{masked}"),
            };
            Some((
                finding("masked-lead", &e.id, at, detail),
                e.suggestions.first(),
            ))
        } else if e.verdict == "inaudible" || (strict && e.verdict == "buried") {
            // A part takes its level fix when it has one, else its best.
            let fix = e
                .suggestions
                .iter()
                .find(|s| s.why.contains("louder"))
                .or(e.suggestions.first());
            let stuck = if fix.is_none() {
                " No level its faders can reach (6 dB at most: more pushes everything else) brings it out — an arrangement conflict: give it room (another register, a high-pass or a cut in what covers it), or mute it: if nothing changes, cut it."
            } else {
                ""
            };
            // Played much softer here than elsewhere in the song: the
            // notes, not the mixer.
            let softer = e
                .id
                .strip_prefix("channel:")
                .and_then(|id| p.channels.iter().position(|c| c.id == id))
                .and_then(|c| soft_notes(p, c, &[(report.range.from_beat, report.range.to_beat)]))
                .map(|s| s.replace("there", "here"))
                .unwrap_or_default();
            let detail = format!(
                "{} is {}: audible {frac:.0}% of the time it plays{level}{masked}{}{softer}{stuck}",
                e.name,
                e.verdict,
                fader_far_under(p, e)
            );
            Some((
                FindingOut {
                    severity: if e.verdict == "inaudible" {
                        "warn"
                    } else {
                        "info"
                    },
                    ..finding("inaudible-part", &e.id, at, detail)
                },
                fix,
            ))
        } else {
            None
        };
        if let Some((f, fix)) = found {
            let from = e
                .buried_in
                .iter()
                .filter(|_| lead)
                .map(|u| u.from_bar)
                .min();
            let to = e.buried_in.iter().filter(|_| lead).map(|u| u.to_bar).max();
            out.push((
                FindingOut {
                    element: Some(e.id.clone()),
                    from_bar: Some(from.unwrap_or(report.range.from_bar)),
                    to_bar: Some(to.unwrap_or(report.range.to_bar)),
                    fix: fix.map(|s| s.patch.clone()).unwrap_or_default(),
                    fix_label: fix.map(|s| s.why.clone()).unwrap_or_default(),
                    ..f
                },
                100.0 - frac,
            ));
        }
        // part-dropout: another part falling away in stretches.
        if !lead && !e.buried_in.is_empty() && o.has(Check::Levels) {
            if let Some(f) = dropout(p, ctx.timeline, e) {
                out.push(f);
            }
        }
        // low-end-off-centre: a part carrying low end, panned to a side.
        if let (Some(b), Some(sp)) = (e.balance_db, e.spectrum_db.as_ref()) {
            let p6 = |i: usize| 10f64.powf(sp[i] / 10.0);
            let total: f64 = (0..6).map(p6).sum();
            let low = if total > 0.0 {
                (p6(0) + p6(1)) / total
            } else {
                0.0
            };
            if low >= 0.5 && b.abs() >= pick(6.0, 4.0, 10.0) {
                let channel =
                    e.id.strip_prefix("channel:")
                        .and_then(|id| p.channels.iter().position(|c| c.id == id));
                let (path, pan) = match channel {
                    Some(c) if p.channels[c].pan.abs() > 0.01 => {
                        (format!("/channels/{c}/pan"), p.channels[c].pan)
                    }
                    _ => (
                        format!("/mixer/inserts/{}/pan", e.insert),
                        p.mixer.inserts[e.insert].pan,
                    ),
                };
                let (fix, fix_label) = if pan.abs() > 0.01 {
                    (
                        vec![set(path, json!(0.0))],
                        format!("{}'s pan {pan:+.2} → centre", e.name),
                    )
                } else {
                    (vec![], String::new())
                };
                out.push((
                    FindingOut {
                        element: Some(e.id.clone()),
                        from_bar: Some(report.range.from_bar),
                        to_bar: Some(report.range.to_bar),
                        fix,
                        fix_label,
                        ..finding(
                            "low-end-off-centre",
                            &e.id,
                            e.name.clone(),
                            format!(
                                "{} is {} and {:.0}% of it is under 250 Hz: the low end leans to one side (lopsided on headphones, lost on one speaker of a club system). Bass and kick sit in the centre.",
                                e.name,
                                if b.abs() >= 20.0 {
                                    format!("only on the {}", if b > 0.0 { "left" } else { "right" })
                                } else {
                                    format!("{:.1} dB louder on the {}", b.abs(), if b > 0.0 { "left" } else { "right" })
                                },
                                low * 100.0
                            ),
                        )
                    },
                    b.abs() * low,
                ));
            }
        }
        // reverb-wash: its insert's reverb or delay makes up most of what
        // is heard of it, or a delay's repeats pile up.
        if let Some(wet) = e.wet_pct {
            let ins = &p.mixer.inserts[e.insert];
            let fx: Vec<(usize, &rosaclef_core::Device)> = ins
                .effects
                .iter()
                .enumerate()
                .filter(|(_, d)| d.enabled)
                .collect();
            let piling = fx
                .iter()
                .find(|(_, d)| d.kind == "delay" && d.param("feedback") >= 0.7);
            let at = pick(50.0, 40.0, 65.0);
            if wet >= at || (piling.is_some() && wet >= 20.0) {
                let mut ops = vec![];
                let mut said = vec![];
                for (j, d) in &fx {
                    let base = format!("/mixer/inserts/{}/effects/{j}/params", e.insert);
                    let m = d.param("mix");
                    match d.kind.as_str() {
                        // Where the wet share falls to about a quarter: the
                        // dry falls with (1 - mix/2), the wet rises with mix.
                        "reverb" => {
                            let share = wet / 100.0;
                            let to = (m * 0.3).max(0.1).min(m);
                            let r = |x: f64| {
                                let w = share / (1.0 - share).max(0.02)
                                    * (1.0 - m / 2.0).powi(2)
                                    * (x / m.max(1e-3)).powi(2);
                                w / (w + (1.0 - x / 2.0).powi(2))
                            };
                            let mut x = m;
                            while x > to && r(x) > 0.25 {
                                x -= 0.05;
                            }
                            let x = (x.max(to) * 100.0).round() / 100.0;
                            if x < m - 0.01 {
                                ops.push(set(format!("{base}/mix"), json!(x)));
                                said.push(format!("the reverb's mix {m:.2} → {x:.2}"));
                            }
                        }
                        "delay" => {
                            let fb = d.param("feedback");
                            if fb > 0.4 {
                                ops.push(set(format!("{base}/feedback"), json!(0.35)));
                                said.push(format!("the delay's feedback {fb:.2} → 0.35"));
                            }
                            if m > 0.3 {
                                ops.push(set(format!("{base}/mix"), json!(0.25)));
                                said.push(format!("its mix {m:.2} → 0.25"));
                            }
                        }
                        _ => {}
                    }
                }
                let what: Vec<&str> = fx.iter().map(|(_, d)| d.kind.as_str()).collect();
                out.push((
                    FindingOut {
                        element: Some(e.id.clone()),
                        from_bar: Some(report.range.from_bar),
                        to_bar: Some(report.range.to_bar),
                        fix: ops,
                        fix_label: format!("on {}: {}", model::insert_id(p, e.insert), said.join(", ")),
                        ..finding(
                            "reverb-wash",
                            &e.id,
                            e.name.clone(),
                            format!(
                                "{wet:.0}% of what is heard of {} is its {}{}: far away and washy, smearing into the parts around it.",
                                e.name,
                                what.join(" and "),
                                match piling {
                                    Some((_, d)) => format!(
                                        " (the delay's feedback at {:.2} piles the repeats up)",
                                        d.param("feedback")
                                    ),
                                    None => String::new(),
                                }
                            ),
                        )
                    },
                    wet,
                ));
            }
        }
        // dominant-part: one part (not the lead) is most of the mix.
        // Its share of the mix's loudness (K-weighted), from its level.
        let loud_share = e
            .relative_to_mix_db
            .flatten()
            .map(|r| 100.0 * 10f64.powf(r / 10.0))
            .unwrap_or(0.0);
        // The lead is meant to lead, not to be the whole mix: 80 % or more
        // of its loudness and the band behind it disappears.
        let too_much = if lead {
            loud_share >= pick(80.0, 70.0, 90.0)
        } else {
            e.verdict == "dominant" && loud_share >= pick(60.0, 50.0, 75.0)
        };
        if too_much {
            let unit = mix
                .elements
                .iter()
                .find(|x| x.id == e.id)
                .and_then(|x| x.units.first());
            let (ins, ch) = unit.map(|u| fader_boost(p, mix, *u)).unwrap_or((0.0, 0.0));
            let pushed = ins + ch >= 3.0;
            let (fix, fix_label, cause) = match unit {
                Some(&u) if pushed => (
                    lower_unit(p, mix, u, ins + ch),
                    format!("{}'s faders back to unity ({:.1} dB down)", e.name, ins + ch),
                    format!(
                        " Its faders are pushed {:+.1} dB over unity (channel {ch:+.1}, insert {ins:+.1}).",
                        ins + ch
                    ),
                ),
                _ => (vec![], String::new(), String::new()),
            };
            out.push((
                FindingOut {
                    severity: if pushed { "warn" } else { "info" },
                    element: Some(e.id.clone()),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    fix,
                    fix_label,
                    ..finding(
                        "dominant-part",
                        &e.id,
                        e.name.clone(),
                        format!(
                            "{}{} is {:.0}% of the mix's loudness ({:+.1} dB against it; {:.0}% of its energy): it covers the other parts and what the sections add.{cause}",
                            e.name,
                            if lead { ", the lead," } else { "" },
                            loud_share,
                            e.relative_to_mix_db.flatten().unwrap_or(0.0),
                            e.share_of_energy_pct
                        ),
                    )
                },
                loud_share,
            ));
        }
        if let Some(Some(c)) = e.correlation {
            // A part that cancels in mono disappears there, however quiet.
            if c < pick(-0.3, -0.1, -0.5)
                && e.relative_to_mix_db.flatten().is_none_or(|r| r >= -40.0)
            {
                out.push((
                    FindingOut {
                        element: Some(e.id.clone()),
                        ..finding(
                            "phase-correlation",
                            &e.id,
                            e.name.clone(),
                            format!(
                                "{} has a stereo correlation of {c:+.2}: its sides cancel when the mix is played in mono (phone, club).{}",
                                e.name,
                                phase_source(p, e)
                            ),
                        )
                    },
                    -c * 10.0,
                ));
            }
        }
    }

    // low-end-buildup
    if ctx.include_master && o.has(Check::Spectrum) {
        let thr = pick(14.0, 11.0, 18.0);
        let mud = pick(6.0, 4.0, 9.0);
        let excess = |sp: &[f64]| -> (f64, f64) {
            let p = |i: usize| 10f64.powf(sp[i] / 10.0);
            (
                dsp::db(p(0) + p(1)) - dsp::db(p(3) + p(4)),
                sp[2] - (sp[3] - 3.0),
            )
        };
        let drums = |u: usize| {
            mix.units[u].channel.and_then(|c| ctx.roles.get(c).copied()) == Some("drums")
        };
        // A drum break has no middle to weigh the lows against.
        let drum_break = |i: usize| {
            let lows: f64 = (0..mix.units.len())
                .map(|u| {
                    rows_hops[i]
                        .iter()
                        .map(|h| low3(mix.unit_six(u, *h)))
                        .sum::<f64>()
                })
                .sum();
            let drummed: f64 = (0..mix.units.len())
                .filter(|u| drums(*u))
                .map(|u| {
                    rows_hops[i]
                        .iter()
                        .map(|h| low3(mix.unit_six(u, *h)))
                        .sum::<f64>()
                })
                .sum();
            lows > 0.0 && drummed >= 0.9 * lows
        };
        // Nor does a part playing alone (a solo intro): its own tone is not
        // a balance.
        let solo = |i: usize| {
            mix.ranked(&rows_hops[i], &|u, h| mix.unit_six(u, h).iter().sum())
                .first()
                .is_some_and(|(_, share)| *share >= 0.85)
        };
        let hit = |i: usize| {
            report.per_bar[i].spectrum_db.as_ref().is_some_and(|sp| {
                let (low, m) = excess(sp);
                low > thr || m > mud
            }) && !drum_break(i)
                && !solo(i)
        };
        // A part whose EQ boosts its lows hard, carrying a good share of the
        // lows of a mix that leans low: the setting is the cause, all along.
        let all_low = mix.ranked(&mix.inside, &|u, h| low3(mix.unit_six(u, h)));
        let boosted = all_low
            .iter()
            .filter(|(_, share)| *share >= 0.25)
            .find_map(|(u, share)| low_boost(p, mix.units[*u].insert).map(|b| (*u, *share, b)))
            .filter(|(_, _, b)| b.2 >= pick(6.0, 4.0, 9.0));
        let tilt = report.master.spectrum.as_ref().map(|sp| excess(&sp.db));
        // The mix itself must lean low too (nearly as far as the rule's
        // own threshold): a boost in a balanced mix is a choice.
        let boosted = boosted
            .zip(tilt)
            .filter(|(_, (low, m))| *low > thr - 4.0 || *m > mud - 2.0);
        // Two bars or more (one is a hit, not a build-up); one finding for
        // all of them, with one fix.
        let found: Vec<(usize, usize)> = runs(report, ctx, &hit)
            .into_iter()
            .filter(|(a, b)| b > a || report.per_bar.len() == 1)
            .collect();
        if let Some(((u, share, (j, freq, boost)), (low, m))) = boosted {
            let insert = mix.units[u].insert;
            let to = 3f64.min(boost - (low.max(m) - 3.0).max(0.0)).max(0.0);
            let detail = format!(
                "{}'s EQ boosts its lows {boost:+.1} dB at {freq:.0} Hz ({}), and it carries {:.0}% of the energy under 500 Hz: below 250 Hz the mix is {low:+.1} dB against 500 Hz–6 kHz, and 250–500 Hz {m:+.1} dB over a balanced tilt.",
                short_name(p, mix, u),
                model::insert_id(p, insert),
                share * 100.0
            );
            out.push((
                FindingOut {
                    element: Some(mix.units[u].id.clone()),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    fix: vec![set(
                        format!("/mixer/inserts/{insert}/effects/{j}/params/low"),
                        json!(r1(to)),
                    )],
                    fix_label: format!(
                        "the low shelf on {} from {boost:+.1} to {to:+.1} dB",
                        model::insert_id(p, insert)
                    ),
                    ..finding(
                        "low-end-buildup",
                        "master",
                        format!("bars {}–{}", report.range.from_bar, report.range.to_bar),
                        detail,
                    )
                },
                low.max(m),
            ));
        } else if !found.is_empty() {
            let rows: Vec<usize> = found.iter().flat_map(|(a, b)| *a..=*b).collect();
            let hs: Vec<usize> = rows
                .iter()
                .flat_map(|i| rows_hops[*i].iter().copied())
                .collect();
            // Who is under 500 Hz, with their shares there.
            let low_e = mix.ranked(&hs, &|u, h| low3(mix.unit_six(u, h)));
            let role = |u: usize| {
                mix.units[u]
                    .channel
                    .and_then(|c| ctx.roles.get(c).copied())
                    .unwrap_or("")
            };
            // A part boosting its lows is cut first; else a part that is
            // not the bass or the drums.
            let culprit = low_e
                .iter()
                .find(|(u, share)| *share > 0.1 && low_boost(p, mix.units[*u].insert).is_some())
                .or_else(|| {
                    low_e.iter().find(|(u, share)| {
                        !matches!(role(*u), "bass" | "drums")
                            && *share > 0.1
                            && mix.units[*u].insert > 0
                    })
                });
            let sp_avg: Vec<f64> = (0..6)
                .map(|k| {
                    let n = rows.len() as f64;
                    dsp::db(
                        rows.iter()
                            .map(|i| {
                                10f64.powf(
                                    report.per_bar[*i]
                                        .spectrum_db
                                        .as_ref()
                                        .map(|s| s[k])
                                        .unwrap_or(-120.0)
                                        / 10.0,
                                )
                            })
                            .sum::<f64>()
                            / n,
                    )
                })
                .collect();
            let (low, m) = excess(&sp_avg);
            // The low mids over, from a part's EQ boost there: that boost.
            let honk = low_e
                .iter()
                .filter(|(_, share)| *share > 0.1)
                .find_map(|(u, _)| {
                    lowmid_boost(p, mix.units[*u].insert)
                        .filter(|b| b.3 >= pick(6.0, 4.0, 9.0))
                        .map(|b| (*u, b))
                })
                .filter(|_| m - mud > low - thr);
            let (fix, label) = match (honk, culprit) {
                (Some((u, (j, band, freq, boost))), _) => {
                    let insert = mix.units[u].insert;
                    (
                        vec![set(
                            format!("/mixer/inserts/{insert}/effects/{j}/params/{band}"),
                            json!(EQ_KEEP),
                        )],
                        format!(
                            "the {band} band of the EQ on {} ({freq:.0} Hz) from {boost:+.1} to {EQ_KEEP:+.1} dB",
                            model::insert_id(p, insert)
                        ),
                    )
                }
                // The low mids (250-500 Hz) over: a bell there; the lows: a
                // shelf.
                (None, Some((u, _))) if m - mud > low - thr => eq_ops(
                    p,
                    mix.units[*u].insert,
                    "mid",
                    -4.0,
                    ("midFreq", 350.0),
                    &[("midQ", 1.0)],
                ),
                (None, Some((u, _))) => eq_ops(
                    p,
                    mix.units[*u].insert,
                    "low",
                    -4.0,
                    ("lowFreq", 150.0),
                    &[],
                ),
                (None, None) => match low_e.first() {
                    Some((u, _)) => (
                        lower_unit(p, mix, *u, 2.0),
                        format!("{} down 2 dB", short_name(p, mix, *u)),
                    ),
                    None => (vec![], String::new()),
                },
            };
            let (at, fb, tb) = place_of(report, &found);
            let names: Vec<String> = low_e
                .iter()
                .take(3)
                .filter(|x| x.1 >= 0.1)
                .map(|(u, share)| format!("{} {:.0}%", short_name(p, mix, *u), share * 100.0))
                .collect();
            let detail = format!(
                "below 250 Hz is {:+.1} dB against 500 Hz–6 kHz and 250–500 Hz {:+.1} dB over a balanced tilt: the low end builds up. Under 500 Hz: {}.",
                low, m, names.join(", ")
            );
            out.push((
                FindingOut {
                    from_bar: Some(fb),
                    to_bar: Some(tb),
                    fix,
                    fix_label: label,
                    ..finding("low-end-buildup", "master", at, detail)
                },
                low.max(m),
            ));
        }
    }

    // boxy-lowmids: 250-500 Hz about as loud as 500 Hz-2 kHz (a balanced
    // mix keeps it a few dB under), an EQ boost there the usual cause.
    if ctx.include_master && o.has(Check::Spectrum) {
        if let Some(sp) = report.master.spectrum.as_ref() {
            let over = sp.db[2] - sp.db[3];
            let box_ = mix.ranked(&mix.inside, &|u, h| mix.unit_six(u, h)[2]);
            let boosted = box_
                .iter()
                .filter(|(_, share)| *share >= 0.25)
                .find_map(|(u, share)| {
                    lowmid_boost(p, mix.units[*u].insert).map(|b| (*u, *share, b))
                })
                .filter(|(_, _, b)| b.3 >= pick(6.0, 4.0, 9.0));
            let built = out.iter().any(|(f, _)| f.rule == "low-end-buildup");
            let names: Vec<String> = box_
                .iter()
                .take(3)
                .filter(|x| x.1 >= 0.1)
                .map(|(u, share)| format!("{} {:.0}%", short_name(p, mix, *u), share * 100.0))
                .collect();
            let role = |u: usize| {
                mix.units[u]
                    .channel
                    .and_then(|c| ctx.roles.get(c).copied())
                    .unwrap_or("")
            };
            let found = match boosted {
                Some((u, share, (j, band, freq, boost))) if over > pick(-3.0, -4.0, -1.0) => {
                    let insert = mix.units[u].insert;
                    Some((
                        vec![set(
                            format!("/mixer/inserts/{insert}/effects/{j}/params/{band}"),
                            json!(EQ_KEEP),
                        )],
                        format!(
                            "the {band} band of the EQ on {} ({freq:.0} Hz) from {boost:+.1} to {EQ_KEEP:+.1} dB",
                            model::insert_id(p, insert)
                        ),
                        format!(
                            " {}'s EQ boosts {boost:+.1} dB at {freq:.0} Hz, and it carries {:.0}% of 250–500 Hz.",
                            short_name(p, mix, u),
                            share * 100.0
                        ),
                    ))
                }
                _ if over > pick(2.0, 1.0, 4.0) && !built => {
                    // The part carrying the most of it, not the bass or drums.
                    match box_.iter().find(|(u, share)| {
                        !matches!(role(*u), "bass" | "drums")
                            && *share > 0.1
                            && mix.units[*u].insert > 0
                    }) {
                        Some((u, _)) => {
                            let (fix, label) = eq_ops(
                                p,
                                mix.units[*u].insert,
                                "mid",
                                -3.0,
                                ("midFreq", 350.0),
                                &[("midQ", 1.0)],
                            );
                            Some((fix, label, String::new()))
                        }
                        None => Some((vec![], String::new(), String::new())),
                    }
                }
                _ => None,
            };
            if let Some((fix, fix_label, cause)) = found {
                out.push((
                    FindingOut {
                        from_bar: Some(report.range.from_bar),
                        to_bar: Some(report.range.to_bar),
                        fix,
                        fix_label,
                        ..finding(
                            "boxy-lowmids",
                            "master",
                            format!("bars {}–{}", report.range.from_bar, report.range.to_bar),
                            format!(
                                "250–500 Hz is {over:+.1} dB against 500 Hz–2 kHz (a balanced mix keeps it 3 dB or more under): boxy, cardboard-like.{cause} At 250–500 Hz: {}.",
                                names.join(", ")
                            ),
                        )
                    },
                    over + 5.0,
                ));
            }
        }
    }

    // harsh-presence: 2-6 kHz about as loud as the mids (500 Hz-2 kHz) — a
    // balanced mix keeps it several dB under them; an EQ boost there is the
    // usual cause.
    if ctx.include_master && o.has(Check::Spectrum) {
        if let Some(sp) = report.master.spectrum.as_ref() {
            let over = sp.db[4] - sp.db[3];
            let pres = mix.ranked(&mix.inside, &|u, h| mix.unit_six(u, h)[4]);
            // A boost there on a part carrying a tenth of it, in a mix that
            // is already nearly as bright as its mids; else a mix brighter
            // than distorted guitars make it (2 dB over its mids).
            let boosted = pres
                .iter()
                .filter(|(_, share)| *share >= 0.1)
                .find_map(|(u, _)| presence_boost(p, mix.units[*u].insert).map(|b| (*u, b)))
                .filter(|(_, b)| b.3 >= pick(6.0, 4.0, 9.0))
                .filter(|_| over > pick(-1.0, -2.0, 1.0));
            if boosted.is_some() || over > pick(2.0, 1.0, 4.0) {
                let names: Vec<String> = pres
                    .iter()
                    .take(3)
                    .filter(|x| x.1 >= 0.1)
                    .map(|(u, share)| format!("{} {:.0}%", short_name(p, mix, *u), share * 100.0))
                    .collect();
                let is_lead = |u: usize| {
                    report
                        .elements
                        .iter()
                        .any(|e| e.lead && e.id == mix.units[u].id)
                };
                let (fix, label, cause) = match boosted {
                    Some((u, (j, band, freq, boost))) => {
                        let insert = mix.units[u].insert;
                        (
                            vec![set(
                                format!("/mixer/inserts/{insert}/effects/{j}/params/{band}"),
                                json!(EQ_KEEP),
                            )],
                            format!(
                                "the {band} band of the EQ on {} ({freq:.0} Hz) from {boost:+.1} to {EQ_KEEP:+.1} dB",
                                model::insert_id(p, insert)
                            ),
                            format!(" {}'s EQ boosts {boost:+.1} dB at {freq:.0} Hz.", short_name(p, mix, u)),
                        )
                    }
                    // A gentle cut on the part carrying the most of it — the
                    // lead keeps its presence when another part can give.
                    None => match pres
                        .iter()
                        .find(|(u, sh)| *sh >= 0.15 && !is_lead(*u) && mix.units[*u].insert > 0)
                        .or_else(|| pres.first().filter(|(u, _)| mix.units[*u].insert > 0))
                    {
                        Some((u, _)) => {
                            let (ops, what) = eq_ops(
                                p,
                                mix.units[*u].insert,
                                "mid",
                                -3.0,
                                ("midFreq", 3500.0),
                                &[("midQ", 1.0)],
                            );
                            (ops, what, String::new())
                        }
                        None => (vec![], String::new(), String::new()),
                    },
                };
                out.push((
                    FindingOut {
                        from_bar: Some(report.range.from_bar),
                        to_bar: Some(report.range.to_bar),
                        fix,
                        fix_label: label,
                        ..finding(
                            "harsh-presence",
                            "master",
                            format!("bars {}–{}", report.range.from_bar, report.range.to_bar),
                            format!(
                                "2–6 kHz is {over:+.1} dB against 500 Hz–2 kHz (a balanced mix keeps it a few dB under): harsh, fatiguing.{cause} At 2–6 kHz: {}.",
                                names.join(", ")
                            ),
                        )
                    },
                    over + 10.0,
                ));
            }
        }
    }

    // bright-highs: the top octaves (6 kHz up) about as loud as the presence
    // range (2-6 kHz) — a balanced mix keeps them 4 dB or more under it.
    // Often a choice (airy pop, EDM): a finding when it is that bright.
    if ctx.include_master && o.has(Check::Spectrum) {
        if let Some(sp) = report.master.spectrum.as_ref() {
            let over = sp.db[5] - sp.db[4];
            if over > pick(-2.0, -3.0, 0.0) {
                let air = mix.ranked(&mix.inside, &|u, h| mix.unit_six(u, h)[5]);
                // The setting at fault: a high EQ boost on a part carrying
                // a quarter of the top band.
                let boosted =
                    air.iter()
                        .filter(|(_, share)| *share >= 0.25)
                        .find_map(|(u, share)| {
                            high_boost(p, mix.units[*u].insert).map(|b| (*u, *share, b))
                        });
                let names: Vec<String> = air
                    .iter()
                    .take(3)
                    .filter(|x| x.1 >= 0.1)
                    .map(|(u, share)| format!("{} {:.0}%", short_name(p, mix, *u), share * 100.0))
                    .collect();
                let (fix, label, cause) = match boosted {
                    Some((u, _, (j, band, freq, boost))) => {
                        let insert = mix.units[u].insert;
                        (
                            vec![set(
                                format!("/mixer/inserts/{insert}/effects/{j}/params/{band}"),
                                json!(EQ_KEEP),
                            )],
                            format!(
                                "the {band} band of the EQ on {} ({freq:.0} Hz) from {boost:+.1} to {EQ_KEEP:+.1} dB",
                                model::insert_id(p, insert)
                            ),
                            format!(
                                " {}'s EQ boosts {boost:+.1} dB at {freq:.0} Hz.",
                                short_name(p, mix, u)
                            ),
                        )
                    }
                    None => match air.first() {
                        Some((u, _)) if mix.units[*u].insert > 0 => {
                            let (ops, what) = eq_ops(
                                p,
                                mix.units[*u].insert,
                                "high",
                                -3.0,
                                ("highFreq", 8000.0),
                                &[],
                            );
                            (ops, what, String::new())
                        }
                        _ => (vec![], String::new(), String::new()),
                    },
                };
                out.push((
                    FindingOut {
                        from_bar: Some(report.range.from_bar),
                        to_bar: Some(report.range.to_bar),
                        fix,
                        fix_label: label,
                        ..finding(
                            "bright-highs",
                            "master",
                            format!("bars {}–{}", report.range.from_bar, report.range.to_bar),
                            format!(
                                "above 6 kHz the mix is {over:+.1} dB against 2–6 kHz (a balanced mix keeps it 4 dB or more under): bright and sizzly.{cause} Above 6 kHz: {}.",
                                names.join(", ")
                            ),
                        )
                    },
                    over + 10.0,
                ));
            }
        }
    }

    // phase (master)
    if ctx.include_master && o.has(Check::Stereo) {
        if let Some(c) = &report.master.correlation {
            let mean = c.get("mean").and_then(|x| x.as_f64());
            let loss = c.get("monoLossDb").and_then(|x| x.as_f64());
            if mean.is_some_and(|m| m < pick(0.0, 0.2, -0.2))
                || loss.is_some_and(|l| l < pick(-6.0, -4.5, -9.0))
            {
                out.push((
                    FindingOut {
                        element: Some(model::insert_id(p, 0)),
                        ..finding(
                            "phase-correlation",
                            "master",
                            "master".into(),
                            format!(
                                "the mix's correlation is {:+.2} and it loses {:.1} dB in mono: parts cancel on mono playback.",
                                mean.unwrap_or(0.0), -loss.unwrap_or(0.0)
                            ),
                        )
                    },
                    -loss.unwrap_or(0.0),
                ));
            }
        }
    }

    // The sections' shape: a heuristic (a chorus often lifts by density
    // and width, not loudness), so for information — and not judged while
    // the limiter clamps everything (its fix comes first).
    let clamped = out
        .iter()
        .any(|(f, _)| matches!(f.rule, "master-overload" | "over-compression"));
    if ctx.include_master && o.has(Check::Levels) && !clamped {
        if let Some((f, x)) = section_lift(report, ctx, pick(1.0, 0.5, 2.0)) {
            out.push((
                FindingOut {
                    severity: "info",
                    ..f
                },
                x,
            ));
        }
        if let Some((f, x)) = flat_sections(report, ctx, pick(1.5, 2.5, 1.0)) {
            out.push((
                FindingOut {
                    severity: "info",
                    ..f
                },
                x,
            ));
        }
    }

    // The project's choices, the focus, ranking, the limit.
    let off = &p.critic.off;
    let sup = &p.critic.suppress;
    let mut v: Vec<(FindingOut, f64)> = out
        .into_iter()
        .filter(|(f, _)| !off.iter().any(|x| x == f.rule) && !sup.contains(&f.key))
        .collect();
    let rank = |f: &FindingOut| {
        (if f.severity == "warn" { 0 } else { 1 }) * 100
            + RULES.iter().position(|r| *r == f.rule).unwrap_or(99)
    };
    v.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then(b.1.total_cmp(&a.1)));
    let mut seen = vec![];
    v.retain(|(f, _)| {
        if seen.contains(&f.key) {
            false
        } else {
            seen.push(f.key.clone());
            true
        }
    });
    // At most a third of the list (and at least 3) for one rule: one
    // problem does not crowd out the others.
    let per_rule = (o.max_findings / 3).max(3);
    let mut counts = [0usize; RULES.len()];
    v.retain(|(f, _)| {
        let i = RULES.iter().position(|r| *r == f.rule).unwrap_or(0);
        counts[i] += 1;
        counts[i] <= per_rule
    });
    v.into_iter()
        .take(o.max_findings)
        .map(|(f, score)| FindingOut { score, ..f })
        .collect()
}

/// Sections whose loudness hardly changes: no build.
/// The sections' loudness at the output, each pass apart: ((section index,
/// pass), LUFS), with the sections.
/// ((section index, pass), LUFS).
type SectionLoud = ((usize, u32), f64);

fn section_louds(ctx: &Context) -> (Vec<timeline::Section>, Vec<SectionLoud>) {
    let p = ctx.project;
    let t = ctx.timeline;
    let a = ctx.mix.a;
    let secs = timeline::sections(p);
    let kms = |b: usize| a.kms_at(ctx.mix.master_out, b) as f64;
    let lo = model::Loudness { a };
    // (section index, pass) → its momentary windows.
    let mut groups: Vec<((usize, u32), Vec<f64>)> = vec![];
    for &b in &ctx.mix.inside_blocks {
        let pos = a.lblocks[b];
        let Some(si) = secs
            .iter()
            .position(|s| pos.beat >= s.start - 1e-6 && pos.beat < s.end - 1e-6)
        else {
            continue;
        };
        let key = (si, t.pass_of(pos.span as usize, pos.beat));
        let w = lo.window(&kms, b, 4);
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(w),
            None => groups.push((key, vec![w])),
        }
    }
    let louds: Vec<SectionLoud> = groups
        .iter()
        .filter_map(|(k, w)| model::integrated(w).map(|l| (*k, l)))
        .collect();
    (secs, louds)
}

/// A section the song builds to (a chorus, a drop, a hook) or one it builds
/// from (a verse), by its name.
fn section_role(name: &str) -> Option<bool> {
    let n = name.to_lowercase();
    if ["chorus", "drop", "hook", "refrain"]
        .iter()
        .any(|w| n.contains(w))
    {
        Some(true)
    } else if n.contains("verse") {
        Some(false)
    } else {
        None
    }
}

/// Choruses no louder than the verses (by `gap` LU): the song does not lift
/// where it should.
fn section_lift(report: &Report, ctx: &Context, gap: f64) -> Option<(FindingOut, f64)> {
    let p = ctx.project;
    let t = ctx.timeline;
    let (secs, louds) = section_louds(ctx);
    let verses: Vec<f64> = louds
        .iter()
        .filter(|((si, _), _)| section_role(&secs[*si].name) == Some(false))
        .map(|x| x.1)
        .collect();
    if verses.is_empty() {
        return None;
    }
    // The verses' energy average.
    let verse = 10.0
        * (verses.iter().map(|l| 10f64.powf(l / 10.0)).sum::<f64>() / verses.len() as f64).log10();
    let low: Vec<&SectionLoud> = louds
        .iter()
        .filter(|((si, _), l)| section_role(&secs[*si].name) == Some(true) && *l < verse + gap)
        .collect();
    if low.is_empty() {
        return None;
    }
    let worst = low.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
    let mut stretches: Vec<(f64, f64)> = low
        .iter()
        .map(|((si, _), _)| (secs[*si].start, secs[*si].end))
        .collect();
    stretches.sort_by(|a, b| a.0.total_cmp(&b.0));
    stretches.dedup();
    // A master lane dipping 1.5 dB or more there is the cause.
    let dip = lane_dip_on(p, &["insert/0/volume".into()], &stretches, "the mix", 1.5);
    let names: Vec<String> = low
        .iter()
        .map(|((si, pass), l)| {
            if *pass > 1 {
                format!("{} (pass {}) at {:.1} LUFS", secs[*si].name, pass, l)
            } else {
                format!("{} at {:.1} LUFS", secs[*si].name, l)
            }
        })
        .collect();
    let bars: Vec<(u32, u32, Option<u32>)> = stretches
        .iter()
        .map(|(s0, s1)| (t.bar_of(*s0), t.bar_of((s1 - 1e-6).max(*s0)), None))
        .collect();
    let (fb, tb) = (bars[0].0, bars[bars.len() - 1].1);
    // Else what in the mixer holds the choruses back: lanes holding parts
    // down there, a limiter taking more there, one part covering the rest.
    let mut others: Vec<String> = vec![];
    let mut other_ops: Vec<Value> = vec![];
    let mut other_said: Vec<String> = vec![];
    if dip.is_none() {
        for l in &p.automation {
            let who = l
                .target
                .strip_prefix("insert/")
                .and_then(|x| x.strip_suffix("/volume"))
                .and_then(|i| i.parse::<usize>().ok())
                .filter(|i| *i > 0 && *i < p.mixer.inserts.len())
                .map(|i| p.mixer.inserts[i].name.clone())
                .or_else(|| {
                    let id = l.target.strip_prefix("channel/")?.strip_suffix("/volume")?;
                    p.channels
                        .iter()
                        .find(|c| c.id == id)
                        .map(|c| c.name.clone())
                });
            let Some(who) = who else { continue };
            if let Some(d) = lane_dip_on(p, std::slice::from_ref(&l.target), &stretches, &who, 3.0)
            {
                others.push(format!("a lane holds {who} {:.1} dB down there", d.lift));
                other_ops.extend(d.ops);
                other_said.push(d.said);
            }
        }
        let in_bars = |bars: &[(u32, u32, Option<u32>)]| -> Option<f64> {
            let gr: Vec<f64> = report
                .per_bar
                .iter()
                .filter(|r| {
                    r.bar
                        .is_some_and(|b| bars.iter().any(|(x, y, _)| b >= *x && b <= *y))
                })
                .filter_map(|r| r.limiter_gr_max_db)
                .collect();
            (!gr.is_empty()).then(|| gr.iter().sum::<f64>() / gr.len() as f64)
        };
        let verse_bars: Vec<(u32, u32, Option<u32>)> = secs
            .iter()
            .filter(|s| section_role(&s.name) == Some(false))
            .map(|s| {
                (
                    t.bar_of(s.start),
                    t.bar_of((s.end - 1e-6).max(s.start)),
                    None,
                )
            })
            .collect();
        if let (Some(c), Some(v)) = (in_bars(&bars), in_bars(&verse_bars)) {
            if c - v >= 1.5 {
                others.push(format!(
                    "the master limiter takes {c:.1} dB there (peak, per bar) against {v:.1} in the verses"
                ));
                if let Some((j, drive)) = master_drive(p).filter(|(_, d)| *d > 0.0) {
                    let to = r1((drive - (c - v)).max(0.0));
                    other_ops.push(set(
                        format!("/mixer/inserts/0/effects/{j}/params/gain"),
                        json!(to),
                    ));
                    other_said.push(format!(
                        "the limiter's input gain {drive:+.1} → {to:+.1} dB"
                    ));
                }
            }
        }
        if let Some((e, r)) = report.elements.iter().find_map(|e| {
            e.relative_to_mix_db
                .flatten()
                .filter(|r| !e.lead && *r >= -2.2)
                .map(|r| (e, r))
        }) {
            others.push(format!(
                "{} is {:.0}% of the mix's loudness in verse and chorus alike, so what the chorus adds hardly counts",
                e.name,
                100.0 * 10f64.powf(r / 10.0)
            ));
        }
    }
    let cause = match &dip {
        Some(d) => format!(
            " The master volume lane holds it down {:.1} dB there.",
            d.lift
        ),
        None if !others.is_empty() => format!(" In the mixer: {}.", others.join("; ")),
        None => " Nothing in the mixer holds it down: the arrangement (parts, voicing, \
                 dynamics) has to lift it."
            .into(),
    };
    let (fix, fix_label) = match dip {
        Some(mut d) => {
            // Back up; and when that is not enough, the rest of the song
            // down on the same lane (3 dB at most) — never the choruses
            // over the lane's level elsewhere: the master's volume comes
            // after its limiter, and would push them over the ceiling.
            let need = verse + gap + 0.5 - worst;
            let more = (need - d.lift).clamp(0.0, 3.0);
            if more > 0.0 {
                // The rest: points outside the choruses, and those on their
                // edges the dip left alone (the verse's side of a step).
                let inside = |b: f64| {
                    stretches
                        .iter()
                        .any(|(s0, s1)| b > *s0 + 0.05 && b < *s1 - 0.05)
                };
                let touched: Vec<String> = d
                    .ops
                    .iter()
                    .filter_map(|o| o["path"].as_str().map(String::from))
                    .collect();
                for (j, q) in p.automation[d.lane].points.iter().enumerate() {
                    let path = format!("/automation/{}/points/{j}/value", d.lane);
                    if !inside(q.beat) && !touched.contains(&path) {
                        d.ops.push(set(path, json!(round3(q.value * gain(-more)))));
                    }
                }
                d.said = format!(
                    "{}, and the rest of the song {more:.1} dB down on it",
                    d.said
                );
            }
            (d.ops, d.said)
        }
        None => (other_ops, other_said.join("; ")),
    };
    Some((
        FindingOut {
            from_bar: Some(fb),
            to_bar: Some(tb),
            fix,
            fix_label,
            ..finding(
                "section-lift",
                "master",
                bars_label(&bars),
                format!(
                    "{}, against the verses' {verse:.1} LUFS: a chorus should be {gap:.1} LU or more louder than the verses.{cause}",
                    names.join(", "),
                ),
            )
        },
        verse + gap - worst,
    ))
}

fn flat_sections(report: &Report, ctx: &Context, spread_at: f64) -> Option<(FindingOut, f64)> {
    let p = ctx.project;
    let (secs, louds) = section_louds(ctx);
    if louds.len() < 3 {
        return None;
    }
    let max = louds.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
    let min = louds.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
    if max - min >= spread_at {
        return None;
    }
    // Plan a build from the sections' note density: the busiest at full level.
    let density = |s: &timeline::Section| -> f64 {
        let mut n = 0usize;
        for c in &p.playlist.clips {
            let Some(pat) = p.pattern(&c.pattern) else {
                continue;
            };
            if pat.length <= 0.0 || c.start >= s.end || c.start + c.length <= s.start {
                continue;
            }
            let mut k = 0.0;
            while c.start - c.offset + k * pat.length < (c.start + c.length).min(s.end) {
                let origin = c.start - c.offset + k * pat.length;
                n += pat
                    .notes
                    .iter()
                    .filter(|x| {
                        let at = origin + x.start;
                        at >= s.start.max(c.start) && at < s.end.min(c.start + c.length)
                    })
                    .count();
                k += 1.0;
            }
        }
        n as f64 / (s.end - s.start).max(1.0)
    };
    let used: Vec<usize> = {
        let mut u: Vec<usize> = louds.iter().map(|x| x.0 .0).collect();
        u.sort();
        u.dedup();
        u
    };
    let mut dens: Vec<(usize, f64)> = used.iter().map(|i| (*i, density(&secs[*i]))).collect();
    dens.sort_by(|x, y| x.1.total_cmp(&y.1));
    let n = dens.len();
    let level = |si: usize| -> f64 {
        let rank = dens.iter().position(|d| d.0 == si).unwrap_or(n - 1);
        if rank + 1 > n.div_ceil(3) * 2 || n < 3 {
            0.0
        } else if rank < n / 3 {
            -3.0
        } else {
            -1.5
        }
    };
    let lane_exists = p
        .automation
        .iter()
        .any(|l| !l.mute && l.target == "insert/0/volume");
    let mut fix = vec![];
    if !lane_exists {
        let base = p.mixer.inserts[0].volume;
        let mut points = vec![];
        for &si in &used {
            let s = &secs[si];
            let v = round3(base * gain(level(si)));
            points.push(json!({"beat": s.start, "value": v}));
            points.push(json!({"beat": s.end, "value": v}));
        }
        // Points on one beat are an instant jump; keep them sorted.
        points.sort_by(|a, b| {
            a["beat"]
                .as_f64()
                .unwrap_or(0.0)
                .total_cmp(&b["beat"].as_f64().unwrap_or(0.0))
        });
        points.dedup();
        let lane = json!({"id": "mixcheck-build", "name": "Build (mix check)", "target": "insert/0/volume", "points": points});
        fix.push(if p.automation.is_empty() {
            json!({"op": "add", "path": "/automation", "value": [lane]})
        } else {
            json!({"op": "add", "path": "/automation/-", "value": lane})
        });
    }
    let names: Vec<String> = louds
        .iter()
        .map(|((si, pass), l)| {
            if *pass > 1 {
                format!("{} ({}) {:.1}", secs[*si].name, pass, l)
            } else {
                format!("{} {:.1}", secs[*si].name, l)
            }
        })
        .collect();
    let (fb, tb) = (report.range.from_bar, report.range.to_bar);
    Some((
        FindingOut {
            from_bar: Some(fb),
            to_bar: Some(tb),
            fix,
            fix_label: if lane_exists {
                String::new()
            } else {
                "a master volume lane: the sparse sections 1.5–3 dB down, the busiest at full level"
                    .into()
            },
            ..finding(
                "section-loudness-flat",
                "master",
                format!("bars {fb}–{tb}"),
                format!(
                    "the sections are all within {:.1} LU of each other (LUFS: {}): nothing builds.",
                    max - min,
                    names.join(", ")
                ),
            )
        },
        spread_at - (max - min),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(edit: impl Fn(&mut Value)) -> Project {
        let mut v: Value =
            serde_json::from_str(include_str!("../../tests/mixcheck/fixture.json")).unwrap();
        edit(&mut v);
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn an_eq_move_takes_the_eq_already_there() {
        // Insert 3 has a reverb only: a new EQ goes before it.
        let p = fixture(|_| {});
        let (ops, _) = eq_ops(&p, 3, "low", -4.0, ("lowFreq", 150.0), &[]);
        assert_eq!(ops[0]["path"], json!("/mixer/inserts/3/effects/0"));
        assert_eq!(ops[0]["value"]["params"]["low"], json!(-4.0));
        // A low shelf already cutting near there is cut deeper, in place.
        let p = fixture(|v| {
            v["mixer"]["inserts"][3]["effects"] = json!([
                {"type": "eq", "params": {"low": -3, "lowFreq": 160, "mid": 1.5, "midFreq": 2200}},
                {"type": "reverb", "params": {"mix": 0.2}}
            ]);
        });
        let (ops, _) = eq_ops(&p, 3, "low", -4.0, ("lowFreq", 150.0), &[]);
        assert_eq!(
            ops,
            vec![set(
                "/mixer/inserts/3/effects/0/params/low".into(),
                json!(-7.0)
            )]
        );
        // A mid band busy far from the frequency: a new EQ, before the reverb.
        let (ops, what) = eq_ops(&p, 3, "mid", -6.0, ("midFreq", 700.0), &[("midQ", 1.0)]);
        assert!(what.starts_with("a new EQ"), "{what}");
        assert_eq!(ops[0]["path"], json!("/mixer/inserts/3/effects/1"));
    }

    #[test]
    fn raising_a_part_restores_its_cut_insert_first() {
        let p = fixture(|v| v["mixer"]["inserts"][3]["volume"] = json!(0.5));
        // Channel lead (index 2) on insert 3, alone there.
        let ops = level_ops(&p, Some(2), 3, 9.0);
        assert_eq!(ops[0], set("/mixer/inserts/3/volume".into(), json!(1.0)));
        assert_eq!(ops[1]["path"], json!("/channels/2/volume"));
    }

    #[test]
    fn raising_a_part_a_lane_drives_moves_the_lane() {
        let p = fixture(|v| {
            v["channels"][2]["volume"] = json!(1.5);
            v["automation"] = json!([{"id": "ride", "target": "insert/3/volume",
                "points": [{"beat": 0, "value": 0.5}, {"beat": 8, "value": 1.0}]}]);
        });
        // The channel is at its top: the rest goes to the insert, which the
        // lane drives — its points rise instead.
        let ops = level_ops(&p, Some(2), 3, 6.0);
        assert!(
            ops.iter()
                .all(|o| o["path"] != json!("/mixer/inserts/3/volume")),
            "{ops:?}"
        );
        let v0 = ops
            .iter()
            .find(|o| o["path"] == json!("/automation/0/points/0/value"))
            .and_then(|o| o["value"].as_f64())
            .unwrap();
        assert!((v0 - 0.5 * gain(6.0)).abs() < 0.01, "{ops:?}");
    }
}
