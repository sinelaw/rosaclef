//! The Critic's audio rules (category "Mix check"): a mix check's findings
//! as Critic findings, for `rosaclef critic --audio` — listed, fixed and
//! suppressed like the others.

use super::report::{FindingOut, Report};
use rosaclef_core::critic::{rule, Finding, Fix, Op, Where};
use serde_json::Value;

fn op_name(op: &str) -> Option<&'static str> {
    match op {
        "add" => Some("add"),
        "replace" => Some("replace"),
        "remove" => Some("remove"),
        _ => None,
    }
}

/// A finding's JSON Patch as the Critic's operations (`None` if it uses
/// operations the Critic does not apply).
pub fn ops(fix: &[Value]) -> Option<Vec<Op>> {
    fix.iter()
        .map(|o| {
            Some(Op {
                op: op_name(o.get("op")?.as_str()?)?,
                path: o.get("path")?.as_str()?.to_string(),
                value: o.get("value").cloned(),
            })
        })
        .collect()
}

pub fn finding(f: &FindingOut, p: &rosaclef_core::Project) -> Option<Finding> {
    let r = rule(f.rule)?;
    let beat = f
        .from_bar
        .map(|b| p.transport.bar_start(b.max(1) - 1))
        .unwrap_or(-1.0);
    let fix = ops(&f.fix).filter(|o| !o.is_empty()).map(|ops| Fix {
        label: if f.fix_label.is_empty() {
            "apply the mix check's fix".into()
        } else {
            f.fix_label.clone()
        },
        ops,
    });
    let first = f
        .detail
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    Some(Finding {
        key: f.key.clone(),
        rule: r.id,
        category: "Mix check",
        level: f.severity,
        title: r.name.to_string(),
        detail: first
            + f.detail
                .get(f.detail.chars().next().map(|c| c.len_utf8()).unwrap_or(0)..)
                .unwrap_or(""),
        at: Where {
            kind: if beat >= 0.0 { "song" } else { "project" },
            id: f.element.clone().unwrap_or_default(),
            channel: f
                .element
                .as_deref()
                .and_then(|e| e.strip_prefix("channel:"))
                .unwrap_or("")
                .to_string(),
            index: -1,
            beat: beat.max(0.0),
            notes: vec![],
            label: f.at.clone(),
        },
        fix,
        suppressed: p.critic.suppress.contains(&f.key),
    })
}

/// The findings of a report, as the Critic's.
pub fn findings(r: &Report, p: &rosaclef_core::Project) -> Vec<Finding> {
    r.findings.iter().filter_map(|f| finding(f, p)).collect()
}
