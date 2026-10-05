//! Findings and suggestions: what the numbers mean, ranked, each with a
//! concrete JSON Patch against `project.json`. The rules are the Critic's
//! audio rules (`rosaclef critic --audio`); a project's `critic.off` and
//! `critic.suppress` leave them out here too.

use super::clashes::{note_name, Clash, Played};
use super::dsp::{self, r1, BARKS};
use super::model::{self, Audibility, Element, Mix};
use super::options::{Check, Options, Threshold};
use super::report::{band_span, pct, Context, ElementOut, FindingOut, Report, Suggestion};
use super::timeline;
use rosaclef_core::Project;
use serde_json::{json, Value};

/// The rules, in the order findings rank.
pub const RULES: [&str; 8] = [
    "master-overload",
    "limiter-pumping",
    "masked-lead",
    "inaudible-part",
    "harmonic-clash",
    "low-end-buildup",
    "phase-correlation",
    "section-loudness-flat",
];

fn set(path: String, value: Value) -> Value {
    json!({"op": "add", "path": path, "value": value})
}

fn replace(path: String, value: Value) -> Value {
    json!({"op": "replace", "path": path, "value": value})
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Ops raising (or lowering) an element's level by `db`: its channel
/// volume first (up to 1.5), then its insert's fader (up to 2).
pub fn level_ops(p: &Project, channel: Option<usize>, insert: usize, db: f64) -> Vec<Value> {
    let mut ops = vec![];
    let mut left = db;
    if let Some(c) = channel {
        let v = p.channels[c].volume.max(1e-4);
        let want = v * gain(left);
        let to = want.clamp(0.0, 1.5);
        ops.push(set(format!("/channels/{c}/volume"), json!(round3(to))));
        left -= dsp::amp_db(to / v);
    }
    if left.abs() > 0.05 && insert > 0 && (channel.is_none() || left > 0.0) {
        let v = p.mixer.inserts[insert].volume.max(1e-4);
        let to = (v * gain(left)).clamp(0.0, 2.0);
        ops.push(set(
            format!("/mixer/inserts/{insert}/volume"),
            json!(round3(to)),
        ));
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

/// Where a new effect goes on an insert: before a limiter that ends it.
fn effect_slot(p: &Project, insert: usize) -> String {
    let fx = &p.mixer.inserts[insert].effects;
    match fx.iter().rposition(|d| d.kind == "limiter") {
        Some(i) if i + 1 == fx.len() => format!("/mixer/inserts/{insert}/effects/{i}"),
        _ => format!("/mixer/inserts/{insert}/effects/-"),
    }
}

/// An EQ move on an insert: an unused band of an EQ already there, or a new EQ.
fn eq_ops(p: &Project, insert: usize, band: &str, params: &[(&str, f64)]) -> Vec<Value> {
    let fx = &p.mixer.inserts[insert].effects;
    if let Some(j) = fx
        .iter()
        .position(|d| d.kind == "eq" && d.enabled && d.param(band).abs() < 1e-9)
    {
        return params
            .iter()
            .map(|(k, v)| {
                set(
                    format!("/mixer/inserts/{insert}/effects/{j}/params/{k}"),
                    json!(r1(*v)),
                )
            })
            .collect();
    }
    let mut ps = serde_json::Map::new();
    for (k, v) in params {
        ps.insert(k.to_string(), json!(r1(*v)));
    }
    vec![
        json!({"op": "add", "path": effect_slot(p, insert), "value": {"type": "eq", "params": ps}}),
    ]
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

/// Concrete changes for a buried or inaudible element, with what the model
/// predicts each does (`fractions`: under each gain, then the masker cut).
#[allow(clippy::too_many_arguments)]
pub fn suggestions(
    p: &Project,
    mix: &Mix,
    e: &Element,
    out: &ElementOut,
    gains: &[f64],
    fractions: &[f64],
    cut: Option<&(usize, [f64; BARKS], f64, f64)>,
    ok_at: f64,
) -> Vec<Suggestion> {
    let mut s = vec![];
    let rel = out.relative_to_mix_db.flatten();
    let now = out
        .audibility
        .as_ref()
        .map(|a| a.audible_fraction_pct / 100.0)
        .unwrap_or(0.0);
    // The masker cut first when it is enough: it keeps the balance.
    if let Some((u, _, f0, q)) = cut {
        let f = fractions.get(gains.len()).copied().unwrap_or(0.0);
        let unit = &mix.units[*u];
        let insert = unit.insert;
        if f >= now + 0.05 && insert > 0 {
            s.push(Suggestion {
                why: format!(
                    "{} covers it around {:.0} Hz: a 6 dB cut there on {} clears it",
                    unit.id,
                    f0,
                    model::insert_id(p, insert)
                ),
                patch: eq_ops(
                    p,
                    insert,
                    "mid",
                    &[("mid", -6.0), ("midFreq", *f0), ("midQ", *q)],
                ),
                expected_relative_to_mix_db: rel,
                expected_audible_fraction_pct: Some(pct(f)),
                verified: None,
            });
        }
    }
    // Only as much gain as the faders can give (channel ≤ 1.5, insert ≤ 2).
    let most = max_gain_db(p, e.channel, e.insert);
    let reachable = |g: &&f64| **g <= most + 0.05;
    let pick = gains
        .iter()
        .zip(fractions)
        .filter(|(g, _)| reachable(g))
        .find(|(_, f)| **f >= ok_at)
        .or_else(|| gains.iter().zip(fractions).rfind(|(g, _)| reachable(g)));
    if let Some((g, f)) = pick {
        s.push(Suggestion {
            why: format!("{:.1} dB louder it comes through the parts covering it", g),
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

/// Move the quieter note of a clash to the nearest pitch that clashes with
/// nothing sounding with it.
pub fn clash_fix(p: &Project, c: &Clash, notes: &[Played]) -> Option<(Vec<Value>, String)> {
    let (q, o) = if c.a_db <= c.b_db {
        (&c.a, &c.b)
    } else {
        (&c.b, &c.a)
    };
    let others: Vec<i32> = notes
        .iter()
        .filter(|n| n.from < q.to && n.to > q.from && n.channel != q.channel)
        .map(|n| n.pitch)
        .collect();
    let bad = |np: i32, x: i32| matches!((np - x).abs() % 12, 1 | 6 | 11);
    for d in [-1, 1, -2, 2] {
        let np = q.pitch + d;
        let iv = (np - o.pitch).abs() % 12;
        if !matches!(iv, 0 | 3 | 4 | 5 | 7 | 8 | 9) || others.iter().any(|x| bad(np, *x)) {
            continue;
        }
        let written = p.patterns[q.pattern].notes[q.note].pitch + d;
        if !(0..=127).contains(&written) {
            continue;
        }
        return Some((
            vec![replace(
                format!("/patterns/{}/notes/{}/pitch", q.pattern, q.note),
                json!(written),
            )],
            format!(
                "move {} {} → {} (pattern {}, note {}; everywhere the pattern plays)",
                p.channels[q.channel].id,
                note_name(q.pitch),
                note_name(np),
                p.patterns[q.pattern].id,
                q.note
            ),
        ));
    }
    None
}

// ------------------------------------------------------------------ rules

fn bars_label(from: u32, to: u32, pass: Option<u32>) -> String {
    let b = if from == to {
        format!("bar {from}")
    } else {
        format!("bars {from}–{to}")
    };
    match pass {
        Some(n) if n > 1 => format!("{b} (pass {n})"),
        _ => b,
    }
}

fn bars_key(from: u32, to: u32, pass: Option<u32>) -> String {
    match pass {
        Some(n) if n > 1 => format!("bars:{from}-{to}:pass{n}"),
        _ => format!("bars:{from}-{to}"),
    }
}

/// Runs of consecutive rows (in playing order) where `hit` holds. A run
/// breaks where the rows do not follow each other in the song (another
/// passage of a section, another pass of a repeat).
fn runs(report: &Report, hit: &dyn Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let rows = &report.per_bar;
    let follows = |i: usize| {
        i > 0
            && (rows[i].from_beat - rows[i - 1].to_beat).abs() < 1e-6
            && rows[i].pass == rows[i - 1].pass
    };
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

fn row_bars(report: &Report, a: usize, b: usize) -> (u32, u32, Option<u32>) {
    let ra = &report.per_bar[a];
    let rb = &report.per_bar[b];
    (
        ra.bar.unwrap_or(0),
        rb.to_bar.or(rb.bar).unwrap_or(0),
        ra.pass,
    )
}

/// Units ranked by their energy over hops `hs`, with their share.
fn contributors(mix: &Mix, hs: &[usize]) -> Vec<(usize, f64)> {
    let mut e: Vec<(usize, f64)> = (0..mix.units.len())
        .map(|u| (u, hs.iter().map(|h| mix.unit_ms(u, *h)).sum::<f64>()))
        .collect();
    let total: f64 = e.iter().map(|x| x.1).sum();
    e.sort_by(|x, y| y.1.total_cmp(&x.1));
    e.into_iter()
        .map(|(u, x)| (u, if total > 0.0 { x / total } else { 0.0 }))
        .collect()
}

fn unit_name(p: &Project, mix: &Mix, u: usize) -> String {
    let unit = &mix.units[u];
    let ins = &p.mixer.inserts[unit.insert];
    let name = match unit.channel {
        Some(c) => p.channels[c].name.clone(),
        None => ins.name.clone(),
    };
    format!(
        "{name} (insert {}, fader {:+.1} dB)",
        unit.insert,
        dsp::amp_db(ins.volume)
    )
}

/// A unit's name, as the producer knows it.
fn short_name(p: &Project, mix: &Mix, u: usize) -> String {
    let unit = &mix.units[u];
    match unit.channel {
        Some(c) => p.channels[c].name.clone(),
        None => p.mixer.inserts[unit.insert].name.clone(),
    }
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
fn overload_fix(p: &Project, mix: &Mix, top: &[(usize, f64)], cut: f64) -> (Vec<Value>, String) {
    let heavy: Vec<usize> = top
        .iter()
        .take(2)
        .filter(|x| x.1 >= 0.25)
        .map(|x| x.0)
        .collect();
    if !heavy.is_empty() {
        let ops = heavy
            .iter()
            .flat_map(|u| lower_unit(p, mix, *u, cut))
            .collect();
        let names: Vec<String> = heavy.iter().map(|u| short_name(p, mix, *u)).collect();
        return (ops, format!("{} down {cut:.1} dB", names.join(" and ")));
    }
    let mut ops = vec![];
    let mut left = cut;
    let master = &p.mixer.inserts[0].effects;
    if let Some(j) = master
        .iter()
        .rposition(|d| d.enabled && d.kind == "limiter")
    {
        let g = master[j].param("gain");
        if g > 0.0 {
            let to = (g - left).max(0.0);
            ops.push(set(
                format!("/mixer/inserts/0/effects/{j}/params/gain"),
                json!(r1(to)),
            ));
            left -= g - to;
        }
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
    }
    let label = if left > 0.05 && left < cut - 0.05 {
        format!("less limiter drive, and every fader down {left:.1} dB")
    } else if left > 0.05 {
        format!("every fader down {cut:.1} dB (no part dominates)")
    } else {
        format!("the limiter's input gain down {cut:.1} dB")
    };
    (ops, label)
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
        let gr_thr = pick(3.0, 2.0, 6.0);
        let hit = |i: usize| {
            let r = &report.per_bar[i];
            let pre = r.pre_limiter_peak_dbfs.unwrap_or(-99.0);
            if has_limiter {
                pre > thr && r.limiter_gr_max_db.unwrap_or(0.0) >= gr_thr
            } else {
                r.peak_dbfs.unwrap_or(-99.0) > 0.0
            }
        };
        for (a, b) in runs(report, &hit) {
            let hs: Vec<usize> = (a..=b).flat_map(|i| rows_hops[i].iter().copied()).collect();
            let pre = (a..=b)
                .filter_map(|i| report.per_bar[i].pre_limiter_peak_dbfs)
                .fold(-99.0, f64::max);
            let lims: Vec<&super::analyze::GrSeries> = mix
                .a
                .gr
                .iter()
                .filter(|g| g.insert == 0 && g.kind == "limiter")
                .collect();
            let (mut grmax, mut above) = (0f64, 0usize);
            for h in &hs {
                let m = lims.iter().map(|g| g.max[*h] as f64).fold(0.0, f64::max);
                grmax = grmax.max(m);
                if lims.iter().map(|g| g.mean[*h] as f64).sum::<f64>() >= 3.0 {
                    above += 1;
                }
            }
            let top = contributors(mix, &hs);
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
            let (fb, tb, pass) = row_bars(report, a, b);
            let detail = if has_limiter {
                format!(
                    "pre-limiter peaks {:+.1} dBFS; the limiter reduces up to {:.1} dB, above 3 dB {}% of the time. Top contributors: {}.",
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
                    severity: "warn",
                    rule: "master-overload",
                    key: format!("master-overload|{}", bars_key(fb, tb, pass)),
                    at: bars_label(fb, tb, pass),
                    detail,
                    element: None,
                    from_bar: Some(fb),
                    to_bar: Some(tb),
                    from_beat: None,
                    fix,
                    fix_label,
                },
                pre + grmax,
            ));
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
            if median < pick(4.0, 3.0, 6.0) || above < pick(0.2, 0.1, 0.35) {
                continue;
            }
            let dev = &p.mixer.inserts[0].effects[g.fx];
            let spec = rosaclef_core::catalog::device(&dev.kind).and_then(|d| d.param("release"));
            let rel = dev.param("release");
            let mut fix = vec![];
            let base = format!("/mixer/inserts/0/effects/{}/params", g.fx);
            if let Some(s) = spec {
                let to = (rel * 2.5).min(s.max).round();
                if to > rel {
                    fix.push(set(format!("{base}/release"), json!(to)));
                }
            }
            if g.kind == "limiter" && dev.param("gain") > 0.0 {
                let to = (dev.param("gain") - mean / 2.0).max(0.0);
                fix.push(set(format!("{base}/gain"), json!(r1(to))));
            } else if g.kind == "compressor" {
                fix.push(set(
                    format!("{base}/threshold"),
                    json!(r1((dev.param("threshold") + mean / 2.0).min(0.0))),
                ));
            }
            out.push((
                FindingOut {
                    severity: "warn",
                    rule: "limiter-pumping",
                    key: format!("limiter-pumping|insert:0#{}", g.fx),
                    at: format!("master {} (effect {})", g.kind, g.fx),
                    detail: format!(
                        "its gain reduction swings {:.1} dB within a beat (median), above 3 dB {}% of the time (mean {:.1} dB): the mix breathes with the beat.",
                        median, pct(above), mean
                    ),
                    element: Some(model::insert_id(p, 0)),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    from_beat: None,
                    fix,
                    fix_label: "a slower release and less drive".into(),
                },
                median,
            ));
        }
    }

    // masked-lead, inaudible-part, phase (elements)
    for e in &report.elements {
        let lead = e.role == Some("lead");
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
        let rel = e.relative_to_mix_db.flatten();
        let first_fix = e.suggestions.first();
        let level = rel
            .map(|r| format!(" ({r:+.1} dB against the mix)"))
            .unwrap_or_default();
        if lead && matches!(e.verdict, "inaudible" | "buried") {
            out.push((
                FindingOut {
                    severity: "warn",
                    rule: "masked-lead",
                    key: format!("masked-lead|{}", e.id),
                    at: format!(
                        "{} in bars {}–{}",
                        e.name, report.range.from_bar, report.range.to_bar
                    ),
                    detail: format!(
                        "the lead is audible only {frac:.0}% of the time it plays{level}{}",
                        if maskers.is_empty() {
                            ".".to_string()
                        } else {
                            format!("; masked by {maskers}.")
                        }
                    ),
                    element: Some(e.id.clone()),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    from_beat: None,
                    fix: first_fix.map(|s| s.patch.clone()).unwrap_or_default(),
                    fix_label: first_fix.map(|s| s.why.clone()).unwrap_or_default(),
                },
                100.0 - frac,
            ));
        } else if e.verdict == "inaudible" || (strict && e.verdict == "buried") {
            let gain_fix = e
                .suggestions
                .iter()
                .find(|s| s.why.contains("louder"))
                .or(first_fix);
            out.push((
                FindingOut {
                    severity: if e.verdict == "inaudible" {
                        "warn"
                    } else {
                        "info"
                    },
                    rule: "inaudible-part",
                    key: format!("inaudible-part|{}", e.id),
                    at: format!(
                        "{} in bars {}–{}",
                        e.name, report.range.from_bar, report.range.to_bar
                    ),
                    detail: format!(
                        "{} is {}: audible {frac:.0}% of the time it plays{level}{}",
                        e.name,
                        e.verdict,
                        if maskers.is_empty() {
                            ".".to_string()
                        } else {
                            format!("; masked by {maskers}.")
                        }
                    ),
                    element: Some(e.id.clone()),
                    from_bar: Some(report.range.from_bar),
                    to_bar: Some(report.range.to_bar),
                    from_beat: None,
                    fix: gain_fix.map(|s| s.patch.clone()).unwrap_or_default(),
                    fix_label: gain_fix.map(|s| s.why.clone()).unwrap_or_default(),
                },
                100.0 - frac,
            ));
        }
        if let Some(Some(c)) = e.correlation {
            if c < pick(-0.3, -0.1, -0.5) && e.share_of_energy_pct >= 3.0 {
                out.push((
                    FindingOut {
                        severity: "warn",
                        rule: "phase-correlation",
                        key: format!("phase-correlation|{}", e.id),
                        at: e.name.clone(),
                        detail: format!(
                            "{} has a stereo correlation of {c:+.2}: its sides cancel when the mix is played in mono (phone, club).",
                            e.name
                        ),
                        element: Some(e.id.clone()),
                        from_bar: None,
                        to_bar: None,
                        from_beat: None,
                        fix: vec![],
                        fix_label: String::new(),
                    },
                    -c * 10.0,
                ));
            }
        }
    }

    // harmonic-clash
    if let Some(cl) = &report.clashes {
        // The channels in focus: those reported, and those on inserts reported.
        let focus: Vec<String> = if o.focus.is_empty() {
            vec![]
        } else {
            let inserts: Vec<usize> = report
                .elements
                .iter()
                .filter(|e| e.kind == "insert")
                .map(|e| e.insert)
                .collect();
            p.channels
                .iter()
                .filter(|c| {
                    report
                        .elements
                        .iter()
                        .any(|e| e.id == format!("channel:{}", c.id))
                        || inserts.contains(&c.mixer.index())
                })
                .map(|c| c.id.clone())
                .collect()
        };
        for c in cl {
            let wanted = c.severity == "high" || (strict && c.severity == "medium");
            if !wanted {
                continue;
            }
            if !o.focus.is_empty() && !focus.contains(&c.a.channel) && !focus.contains(&c.b.channel)
            {
                continue;
            }
            let also = if c.also_in_bars.is_empty() {
                String::new()
            } else {
                format!(
                    " (also in bars {})",
                    c.also_in_bars
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            out.push((
                FindingOut {
                    severity: "warn",
                    rule: "harmonic-clash",
                    key: format!(
                        "harmonic-clash|{}:{}|{}:{}",
                        c.a.pattern, c.a.note_index, c.b.pattern, c.b.note_index
                    ),
                    at: bars_label(c.bar, c.bar, c.pass),
                    detail: format!(
                        "{} {} (pattern {}, note {}) against {} {} (pattern {}, note {}): a {} held {:.2} beats, the quieter at {:+.1} dB against the mix{also}.",
                        c.a.channel, c.a.pitch, c.a.pattern, c.a.note_index,
                        c.b.channel, c.b.pitch, c.b.pattern, c.b.note_index,
                        c.interval, c.overlap_beats, c.a.level_db.min(c.b.level_db)
                    ),
                    element: Some(format!("channel:{}", if c.a.level_db <= c.b.level_db { &c.a.channel } else { &c.b.channel })),
                    from_bar: Some(c.bar),
                    to_bar: Some(c.bar),
                    from_beat: None,
                    fix: c.fix.clone().unwrap_or_default(),
                    fix_label: c.fix_label.clone().unwrap_or_default(),
                },
                c.overlap_beats,
            ));
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
        let hit = |i: usize| {
            report.per_bar[i].spectrum_db.as_ref().is_some_and(|sp| {
                let (low, m) = excess(sp);
                low > thr || m > mud
            })
        };
        for (a, b) in runs(report, &hit) {
            if b == a && report.per_bar.len() > 1 {
                continue; // one bar: a hit, not a build-up
            }
            let hs: Vec<usize> = (a..=b).flat_map(|i| rows_hops[i].iter().copied()).collect();
            let mut low_e: Vec<(usize, f64)> = (0..mix.units.len())
                .map(|u| {
                    (
                        u,
                        hs.iter()
                            .map(|h| {
                                let s = mix.unit_six(u, *h);
                                s[0] + s[1] + s[2]
                            })
                            .sum::<f64>(),
                    )
                })
                .collect();
            let total: f64 = low_e.iter().map(|x| x.1).sum();
            low_e.sort_by(|x, y| y.1.total_cmp(&x.1));
            let role = |u: usize| {
                mix.units[u]
                    .channel
                    .and_then(|c| ctx.roles.get(c).copied())
                    .unwrap_or("")
            };
            let culprit = low_e.iter().find(|(u, e)| {
                !matches!(role(*u), "bass" | "drums")
                    && *e > total * 0.1
                    && mix.units[*u].insert > 0
            });
            let sp_avg: Vec<f64> = (0..6)
                .map(|k| {
                    let n = (b - a + 1) as f64;
                    dsp::db(
                        (a..=b)
                            .map(|i| {
                                10f64.powf(
                                    report.per_bar[i]
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
            let (fix, label) = match culprit {
                Some((u, _)) => (
                    eq_ops(
                        p,
                        mix.units[*u].insert,
                        "low",
                        &[("low", -4.0), ("lowFreq", 150.0)],
                    ),
                    format!(
                        "a 4 dB low shelf at 150 Hz on {}",
                        model::insert_id(p, mix.units[*u].insert)
                    ),
                ),
                None => match low_e.first() {
                    Some((u, _)) => (
                        lower_unit(p, mix, *u, 2.0),
                        format!("{} down 2 dB", short_name(p, mix, *u)),
                    ),
                    None => (vec![], String::new()),
                },
            };
            let (fb, tb, pass) = row_bars(report, a, b);
            let names: Vec<String> = low_e
                .iter()
                .take(3)
                .filter(|x| total > 0.0 && x.1 / total >= 0.1)
                .map(|(u, e)| format!("{} {:.0}%", short_name(p, mix, *u), e / total * 100.0))
                .collect();
            out.push((
                FindingOut {
                    severity: "warn",
                    rule: "low-end-buildup",
                    key: format!("low-end-buildup|{}", bars_key(fb, tb, pass)),
                    at: bars_label(fb, tb, pass),
                    detail: format!(
                        "below 250 Hz is {:+.1} dB against 500 Hz–6 kHz and 250–500 Hz {:+.1} dB over a balanced tilt: the low end builds up. Under 500 Hz: {}.",
                        low, m, names.join(", ")
                    ),
                    element: None,
                    from_bar: Some(fb),
                    to_bar: Some(tb),
                    from_beat: None,
                    fix,
                    fix_label: label,
                },
                low.max(m),
            ));
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
                        severity: "warn",
                        rule: "phase-correlation",
                        key: "phase-correlation|master".into(),
                        at: "master".into(),
                        detail: format!(
                            "the mix's correlation is {:+.2} and it loses {:.1} dB in mono: parts cancel on mono playback.",
                            mean.unwrap_or(0.0), -loss.unwrap_or(0.0)
                        ),
                        element: Some(model::insert_id(p, 0)),
                        from_bar: None,
                        to_bar: None,
                        from_beat: None,
                        fix: vec![],
                        fix_label: String::new(),
                    },
                    -loss.unwrap_or(0.0),
                ));
            }
        }
    }

    // section-loudness-flat
    if ctx.include_master && o.has(Check::Levels) {
        if let Some(f) = flat_sections(report, ctx, pick(1.5, 2.5, 1.0)) {
            out.push(f);
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
    v.into_iter().take(o.max_findings).map(|x| x.0).collect()
}

/// Sections whose loudness hardly changes: no build.
fn flat_sections(report: &Report, ctx: &Context, spread_at: f64) -> Option<(FindingOut, f64)> {
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
    let louds: Vec<((usize, u32), f64)> = groups
        .iter()
        .filter_map(|(k, w)| model::integrated(w).map(|l| (*k, l)))
        .collect();
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
    Some((
        FindingOut {
            severity: "warn",
            rule: "section-loudness-flat",
            key: format!(
                "section-loudness-flat|bars:{}-{}",
                report.range.from_bar, report.range.to_bar
            ),
            at: format!("bars {}–{}", report.range.from_bar, report.range.to_bar),
            detail: format!(
                "the sections are all within {:.1} LU of each other (LUFS: {}): nothing builds.",
                max - min,
                names.join(", ")
            ),
            element: None,
            from_bar: Some(report.range.from_bar),
            to_bar: Some(report.range.to_bar),
            from_beat: None,
            fix,
            fix_label: if lane_exists {
                String::new()
            } else {
                "a master volume lane: the sparse sections 1.5–3 dB down, the busiest at full level"
                    .into()
            },
        },
        spread_at - (max - min),
    ))
}
