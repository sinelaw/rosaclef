//! `--text`: the report as at most 40 lines of plain language, the most
//! important first.

use super::report::Report;
use serde_json::Value;

const MAX_LINES: usize = 40;

/// A `verified` object in a line: "resolved — loudness -9.9 LU; …".
fn verified(v: &Value) -> String {
    if let Some(e) = v.get("error").and_then(|x| x.as_str()) {
        return format!("does not apply ({e})");
    }
    let new: Vec<&str> = v
        .get("new")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|k| k.as_str()).collect())
        .unwrap_or_default();
    let summary = format!(
        "{}{}",
        v.get("summary").and_then(|x| x.as_str()).unwrap_or(""),
        if new.is_empty() {
            String::new()
        } else {
            format!(" (new: {})", new.join(", "))
        }
    );
    match v.get("resolved").and_then(|x| x.as_bool()) {
        Some(true) => format!("resolved — {summary}"),
        Some(false) => format!(
            "NOT resolved — {summary}{}",
            v.get("still")
                .and_then(|x| x.as_str())
                .map(|s| format!("; still: {s}"))
                .unwrap_or_default()
        ),
        None => {
            // A suggestion: the part's level and audibility under it.
            let rel = v.get("relativeToMixDb").and_then(|x| x.as_f64());
            let aud = v.get("audibleFractionPct").and_then(|x| x.as_f64());
            let verdict = v.get("verdict").and_then(|x| x.as_str()).unwrap_or("");
            format!(
                "{verdict}{}{}",
                rel.map(|r| format!(", {r:+.1} dB against the mix"))
                    .unwrap_or_default(),
                aud.map(|a| format!(", audible {a:.0}%"))
                    .unwrap_or_default()
            )
        }
    }
}

fn f(x: Option<f64>, unit: &str) -> String {
    match x {
        Some(v) => format!("{v:.1}{unit}"),
        None => "—".into(),
    }
}

pub fn summary(r: &Report) -> String {
    let mut out: Vec<String> = vec![];
    let rg = &r.range;
    let span = if rg.from_bar == rg.to_bar {
        format!("bar {}", rg.from_bar)
    } else {
        format!("bars {}–{}", rg.from_bar, rg.to_bar)
    };
    out.push(format!(
        "mixcheck {span}{} — {:.1} s{}, threshold {}",
        rg.section
            .as_ref()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default(),
        rg.seconds,
        if rg.repeats { ", every pass" } else { "" },
        rg.threshold
    ));
    let m = &r.master;
    let mut line = vec![];
    if let Some(i) = m.integrated_lufs {
        line.push(format!("{} LUFS-I", f(i, "")));
    }
    if let Some(s) = m.short_term_lufs_max {
        line.push(format!("short-term max {}", f(s, "")));
    }
    if let Some(tp) = m.true_peak_dbtp {
        line.push(format!("TP {tp:.1} dBTP"));
    }
    if let Some(p) = m.plr_db {
        line.push(format!("PLR {}", f(p, " dB")));
    }
    if let Some(l) = m.lra {
        line.push(format!("LRA {}", f(l, " LU")));
    }
    if !line.is_empty() {
        out.push(format!("master: {}", line.join(" · ")));
    }
    let mut dyn_line = vec![];
    if let Some(p) = m.pre_limiter_peak_dbfs {
        dyn_line.push(format!("pre-limiter peak {p:+.1} dBFS"));
    }
    if let Some(g) = &m.limiter_gain_reduction_db {
        dyn_line.push(format!(
            "limiter GR max {:.1} / mean {:.1} dB ({:.0}% of the time > 3 dB)",
            g.max, g.mean, g.pct_time_above3
        ));
    }
    if let Some(c) = m
        .correlation
        .as_ref()
        .and_then(|c| c.get("mean"))
        .and_then(|x| x.as_f64())
    {
        dyn_line.push(format!("correlation {c:+.2}"));
    }
    if !dyn_line.is_empty() {
        out.push(format!("        {}", dyn_line.join(" · ")));
    }
    if let Some(w) = &r.what_if {
        out.push(format!(
            "what-if: {}",
            w.get("summary").and_then(|x| x.as_str()).unwrap_or("")
        ));
    }
    if let Some(c) = &r.compare {
        out.push(format!(
            "compare: {}",
            c.get("summary").and_then(|x| x.as_str()).unwrap_or("")
        ));
    }
    if let Some(t) = &r.target {
        out.push(format!(
            "target {}: {}{}",
            t.name,
            t.status,
            if t.notes.is_empty() {
                String::new()
            } else {
                format!(" — {}", t.notes.join("; "))
            }
        ));
    }
    if let Some(d) = r
        .reference
        .as_ref()
        .and_then(|x| x.get("summary"))
        .and_then(|x| x.as_str())
    {
        out.push(format!("reference: {d}"));
    }
    if r.findings.is_empty() {
        out.push("no findings".into());
    } else {
        out.push(format!("findings ({}):", r.findings.len()));
        for (i, x) in r.findings.iter().enumerate() {
            out.push(format!(
                "{:>2}. {} {} — {}: {}",
                i + 1,
                x.severity.to_uppercase(),
                x.rule,
                x.at,
                x.detail
            ));
            if !x.fix_label.is_empty() {
                out.push(format!("    fix: {}  (key {})", x.fix_label, x.key));
            }
            if let Some(v) = &x.verified {
                out.push(format!("    verified: {}", verified(v)));
            }
        }
    }
    let not_ok: Vec<Vec<String>> = r
        .elements
        .iter()
        .filter(|e| e.verdict != "ok")
        .map(|e| {
            let rel = e
                .relative_to_mix_db
                .flatten()
                .map(|v| format!(" {v:+.1} dB"))
                .unwrap_or_default();
            let aud = e
                .audibility
                .as_ref()
                .map(|a| {
                    let by = a
                        .masked_by
                        .as_ref()
                        .and_then(|m| m.first())
                        .map(|m| {
                            format!(
                                ", masked by {} {:.0}–{:.0} Hz",
                                m.id, m.band_hz[0], m.band_hz[1]
                            )
                        })
                        .unwrap_or_default();
                    format!(", audible {:.0}%{by}", a.audible_fraction_pct)
                })
                .unwrap_or_default();
            let mut lines = vec![format!("  {} ({}) {}{rel}{aud}", e.name, e.id, e.verdict)];
            if let Some(s) = e.suggestions.first() {
                let checked = s
                    .verified
                    .as_ref()
                    .map(|v| format!(" — verified: {}", verified(v)))
                    .unwrap_or_default();
                lines.push(format!("    try: {}{checked}", s.why));
            }
            lines
        })
        .collect();
    if !not_ok.is_empty() {
        out.push("elements:".into());
        let n = not_ok.len();
        out.extend(not_ok.into_iter().take(6).flatten());
        if n > 6 {
            out.push(format!("  … {} more not ok", n - 6));
        }
    }
    let rows: Vec<(String, f64)> = r
        .per_bar
        .iter()
        .filter_map(|x| {
            let l = x.lufs_momentary_max.flatten()?;
            let k = x
                .section
                .clone()
                .or(x.bar.map(|b| b.to_string()))
                .unwrap_or_default();
            let pass = x
                .pass
                .filter(|p| *p > 1)
                .map(|p| format!("×{p}"))
                .unwrap_or_default();
            Some((format!("{k}{pass}"), l))
        })
        .collect();
    if rows.len() > 16 {
        let loud = rows
            .iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .cloned()
            .unwrap_or_default();
        let quiet = rows
            .iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .cloned()
            .unwrap_or_default();
        out.push(format!(
            "momentary max LUFS per {}: loudest {} ({:.1}), quietest {} ({:.1}) — --json for every row",
            if rg.by == "bar" { "bar" } else { "row" },
            loud.0,
            loud.1,
            quiet.0,
            quiet.1
        ));
    } else if !rows.is_empty() {
        out.push(format!(
            "momentary max LUFS by {}: {}",
            rg.by,
            rows.iter()
                .map(|(k, l)| format!("{k}:{l:.1}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    out.push(format!(
        "render: {} ms{}, {} render(s), pre-roll {} beats",
        r.render.ms,
        if r.render.cached { " (cached)" } else { "" },
        r.render.renders,
        r.render.preroll_beats
    ));
    if out.len() > MAX_LINES {
        let last = out.pop().unwrap_or_default();
        out.truncate(MAX_LINES - 2);
        out.push("… (--json has everything)".into());
        out.push(last);
    }
    out.join("\n") + "\n"
}
