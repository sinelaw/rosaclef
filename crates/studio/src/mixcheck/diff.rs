//! What changed between two reports: under a what-if patch, or from one
//! project to another (`--compare`).

use super::dsp::r1;
use super::report::Report;
use serde_json::{json, Map, Value};

fn num(v: &Value, path: &[&str]) -> Option<f64> {
    let mut at = v;
    for k in path {
        at = at.get(k)?;
    }
    at.as_f64()
}

fn delta(a: Option<f64>, b: Option<f64>) -> Option<Value> {
    match (a, b) {
        (Some(x), Some(y)) if (x - y).abs() >= 0.05 => {
            Some(json!({"from": x, "to": y, "delta": r1(y - x)}))
        }
        (None, Some(y)) => Some(json!({"from": null, "to": y})),
        (Some(x), None) => Some(json!({"from": x, "to": null})),
        _ => None,
    }
}

pub fn diff(base: &Report, now: &Report) -> Value {
    let a = serde_json::to_value(base).unwrap_or(Value::Null);
    let b = serde_json::to_value(now).unwrap_or(Value::Null);
    let mut master = Map::new();
    let fields: [(&str, &[&str]); 10] = [
        ("integratedLufs", &["master", "integratedLufs"]),
        ("shortTermLufsMax", &["master", "shortTermLufsMax"]),
        ("truePeakDbtp", &["master", "truePeakDbtp"]),
        ("preLimiterPeakDbfs", &["master", "preLimiterPeakDbfs"]),
        (
            "limiterGainReductionMaxDb",
            &["master", "limiterGainReductionDb", "max"],
        ),
        (
            "limiterGainReductionMeanDb",
            &["master", "limiterGainReductionDb", "mean"],
        ),
        (
            "limiterPctTimeAbove3",
            &["master", "limiterGainReductionDb", "pctTimeAbove3"],
        ),
        (
            "compressorGainReductionMeanDb",
            &["master", "compressorGainReductionDb", "mean"],
        ),
        ("plrDb", &["master", "plrDb"]),
        ("lra", &["master", "lra"]),
    ];
    for (name, path) in fields {
        if let Some(d) = delta(num(&a, path), num(&b, path)) {
            master.insert(name.into(), d);
        }
    }
    let mut elements = vec![];
    let els = |v: &Value| {
        v.get("elements")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let (ea, eb) = (els(&a), els(&b));
    let mut ids: Vec<String> = ea
        .iter()
        .chain(&eb)
        .filter_map(|e| e.get("id").and_then(|x| x.as_str()).map(str::to_string))
        .collect();
    ids.dedup();
    let mut seen = vec![];
    for id in ids {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        let find = |v: &[Value]| {
            v.iter()
                .find(|e| e.get("id").and_then(|x| x.as_str()) == Some(&id))
                .cloned()
        };
        let (x, y) = (find(&ea), find(&eb));
        let mut m = Map::new();
        let g = |e: &Option<Value>, p: &[&str]| e.as_ref().and_then(|v| num(v, p));
        if let Some(d) = delta(g(&x, &["relativeToMixDb"]), g(&y, &["relativeToMixDb"])) {
            m.insert("relativeToMixDb".into(), d);
        }
        if let Some(d) = delta(
            g(&x, &["audibility", "audibleFractionPct"]),
            g(&y, &["audibility", "audibleFractionPct"]),
        ) {
            m.insert("audibleFractionPct".into(), d);
        }
        let verdict = |e: &Option<Value>| {
            e.as_ref()
                .and_then(|v| v.get("verdict"))
                .cloned()
                .unwrap_or(json!("silent"))
        };
        if verdict(&x) != verdict(&y) {
            m.insert(
                "verdict".into(),
                json!({"from": verdict(&x), "to": verdict(&y)}),
            );
        }
        if !m.is_empty() {
            let mut o = Map::new();
            o.insert("id".into(), json!(id));
            o.extend(m);
            elements.push(Value::Object(o));
        }
    }
    let keys = |r: &Report| r.findings.iter().map(|f| f.key.clone()).collect::<Vec<_>>();
    let (ka, kb) = (keys(base), keys(now));
    let resolved: Vec<&String> = ka.iter().filter(|k| !kb.contains(k)).collect();
    let new: Vec<&String> = kb.iter().filter(|k| !ka.contains(k)).collect();
    let mut rows = vec![];
    for (ra, rb) in base.per_bar.iter().zip(&now.per_bar) {
        let m = delta(
            ra.lufs_momentary_max.flatten(),
            rb.lufs_momentary_max.flatten(),
        );
        let pk = delta(ra.pre_limiter_peak_dbfs, rb.pre_limiter_peak_dbfs);
        let big = |d: &Option<Value>| {
            d.as_ref()
                .and_then(|v| v.get("delta"))
                .and_then(|x| x.as_f64())
                .is_some_and(|x| x.abs() >= 0.5)
        };
        if big(&m) || big(&pk) {
            rows.push(json!({"bar": rb.bar, "section": rb.section, "pass": rb.pass, "lufsMomentaryMax": m, "preLimiterPeakDbfs": pk}));
        }
    }
    let mut summary = vec![];
    if let Some(d) = master.get("integratedLufs").and_then(|d| d.get("delta")) {
        summary.push(format!("loudness {:+.1} LU", d.as_f64().unwrap_or(0.0)));
    }
    if let Some(d) = master
        .get("preLimiterPeakDbfs")
        .and_then(|d| d.get("delta"))
    {
        summary.push(format!(
            "pre-limiter peak {:+.1} dB",
            d.as_f64().unwrap_or(0.0)
        ));
    }
    // Dynamics, when they move by 1 dB (LU) or more.
    for (name, label, unit) in [
        ("lra", "LRA", "LU"),
        ("plrDb", "PLR", "dB"),
        (
            "compressorGainReductionMeanDb",
            "master compressor mean reduction",
            "dB",
        ),
        ("limiterGainReductionMeanDb", "limiter mean reduction", "dB"),
    ] {
        if let Some(d) = master.get(name) {
            if d.get("delta")
                .and_then(|x| x.as_f64())
                .is_some_and(|x| x.abs() >= 1.0)
            {
                summary.push(format!(
                    "{label} {} → {} {unit}",
                    d["from"]
                        .as_f64()
                        .map(|x| format!("{x:.1}"))
                        .unwrap_or("—".into()),
                    d["to"]
                        .as_f64()
                        .map(|x| format!("{x:.1}"))
                        .unwrap_or("—".into())
                ));
            }
        }
    }
    for e in elements.iter().take(3) {
        if let Some(d) = e.get("audibleFractionPct") {
            summary.push(format!(
                "{} audible {}% → {}%",
                e["id"].as_str().unwrap_or(""),
                d["from"]
                    .as_f64()
                    .map(|x| x.to_string())
                    .unwrap_or("—".into()),
                d["to"]
                    .as_f64()
                    .map(|x| x.to_string())
                    .unwrap_or("—".into())
            ));
        }
    }
    if !resolved.is_empty() {
        summary.push(format!("{} finding(s) resolved", resolved.len()));
    }
    if !new.is_empty() {
        summary.push(format!("{} new finding(s)", new.len()));
    }
    if summary.is_empty() {
        summary.push("no measurable change".into());
    }
    json!({
        "summary": summary.join("; "),
        "master": master,
        "elements": elements,
        "findings": {"resolved": resolved, "new": new},
        "perBar": rows,
    })
}
