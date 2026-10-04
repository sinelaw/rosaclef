//! Loading a project to play it, forward-compatibly.
//!
//! Editing is strict: validation names every unknown key, so an agent's typo
//! is caught. Playing is lenient, so an engine older than the project still
//! plays it — a newer studio may have added a section, a field, an
//! instrument, an effect or a parameter:
//!
//! - sections and fields the schema does not define are ignored;
//! - an unknown instrument plays on a stand-in ([`STAND_IN`], a plain synth);
//! - an unknown effect is bypassed (left out of its chain);
//! - unknown parameters and option values fall back to their defaults.
//!
//! Each fallback is reported as a note (the Critic lists them too). What
//! is left must still be a valid project.

use crate::catalog::{self, Category};
use crate::model::Project;
use crate::validate::{self, Checked, Severity};
use serde_json::{Map, Value};

/// The instrument an unknown one plays on.
pub const STAND_IN: &str = "analog";

/// A project ready to play, and what was left out or replaced to play it.
#[derive(Debug)]
pub struct Playable {
    pub project: Project,
    /// One line per fallback: "channels[2].instrument: unknown instrument …".
    pub fallbacks: Vec<String>,
}

/// Parse a project for playback. Errors (bad JSON, or problems no fallback
/// covers) come back as the strict validation's result.
pub fn for_playback(text: &str) -> Result<Playable, Checked> {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Err(validate::parse_and_validate(text));
    };
    value_for_playback(v)
}

/// [`for_playback`] on a parsed document.
pub fn value_for_playback(mut v: Value) -> Result<Playable, Checked> {
    let mut fallbacks = vec![];
    let schema = crate::schema::schema();
    strip(&mut v, &schema, &schema, "", &mut fallbacks);
    devices(&mut v, &mut fallbacks);
    let checked = validate::value_and_validate(v);
    if !checked.is_ok() {
        return Err(checked);
    }
    Ok(Playable {
        project: checked.project.expect("valid"),
        fallbacks,
    })
}

/// Follow a `$ref` to its definition.
fn resolve<'a>(node: &'a Value, root: &'a Value) -> &'a Value {
    match node
        .get("$ref")
        .and_then(|r| r.as_str())
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        Some(name) => root.get("$defs").and_then(|d| d.get(name)).unwrap_or(node),
        None => node,
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Drop the members the schema does not define (where it allows no others).
fn strip(v: &mut Value, node: &Value, root: &Value, path: &str, out: &mut Vec<String>) {
    let node = resolve(node, root);
    match v {
        Value::Object(m) => {
            let Some(props) = node.get("properties").and_then(|p| p.as_object()) else {
                return;
            };
            if node.get("additionalProperties") == Some(&Value::Bool(false)) {
                let unknown: Vec<String> = m
                    .keys()
                    .filter(|k| !props.contains_key(*k))
                    .cloned()
                    .collect();
                for k in unknown {
                    m.remove(&k);
                    out.push(format!(
                        "{}: not known to this version; ignored",
                        join(path, &k)
                    ));
                }
            }
            for (k, child) in m.iter_mut() {
                if let Some(p) = props.get(k) {
                    strip(child, p, root, &join(path, k), out);
                }
            }
        }
        Value::Array(items) => {
            if let Some(item) = node.get("items").filter(|i| i.is_object()) {
                for (i, child) in items.iter_mut().enumerate() {
                    strip(child, item, root, &format!("{path}[{i}]"), out);
                }
            }
        }
        _ => {}
    }
}

/// Stand-ins for unknown instruments, bypass for unknown effects, defaults
/// for unknown parameters and option values.
fn devices(v: &mut Value, out: &mut Vec<String>) {
    if let Some(chans) = v.get_mut("channels").and_then(|c| c.as_array_mut()) {
        for (i, ch) in chans.iter_mut().enumerate() {
            let Some(inst) = ch.get_mut("instrument") else {
                continue;
            };
            let path = format!("channels[{i}].instrument");
            let kind = inst
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            if catalog::device_in(&kind, Category::Instrument).is_none() {
                *inst = serde_json::json!({"type": STAND_IN});
                out.push(format!(
                    "{path}: unknown instrument {kind:?}; it plays on a stand-in ({STAND_IN})"
                ));
            } else {
                settings(inst, &kind, &path, out);
            }
        }
    }
    if let Some(inserts) = v
        .pointer_mut("/mixer/inserts")
        .and_then(|c| c.as_array_mut())
    {
        for (i, ins) in inserts.iter_mut().enumerate() {
            let Some(fx) = ins.get_mut("effects").and_then(|e| e.as_array_mut()) else {
                continue;
            };
            let mut k = 0;
            fx.retain_mut(|e| {
                let path = format!("mixer.inserts[{i}].effects[{k}]");
                k += 1;
                let kind = e
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                if catalog::device_in(&kind, Category::Effect).is_none() {
                    out.push(format!("{path}: unknown effect {kind:?}; bypassed"));
                    return false;
                }
                settings(e, &kind, &path, out);
                true
            });
        }
    }
}

/// Unknown parameters and option values fall back to their defaults.
fn settings(dev: &mut Value, kind: &str, path: &str, out: &mut Vec<String>) {
    let Some(spec) = catalog::device(kind) else {
        return;
    };
    if !spec.open_params {
        if let Some(Value::Object(params)) = dev.get_mut("params") {
            drop_where(
                params,
                |k, _| spec.param(k).is_none(),
                |k| format!("{path}.params.{k}: unknown parameter of {kind}; its default plays"),
                out,
            );
        }
    }
    if let Some(Value::Object(options)) = dev.get_mut("options") {
        drop_where(
            options,
            |k, v| match spec.option(k) {
                None => true,
                Some(o) => {
                    !o.choices.is_empty()
                        && !v.as_str().map(|s| o.choices.contains(&s)).unwrap_or(false)
                }
            },
            |k| format!("{path}.options.{k}: not known to this version; its default plays"),
            out,
        );
    }
}

fn drop_where(
    m: &mut Map<String, Value>,
    unknown: impl Fn(&str, &Value) -> bool,
    say: impl Fn(&str) -> String,
    out: &mut Vec<String>,
) {
    let keys: Vec<String> = m
        .iter()
        .filter(|(k, v)| unknown(k, v))
        .map(|(k, _)| k.clone())
        .collect();
    for k in keys {
        m.remove(&k);
        out.push(say(&k));
    }
}

/// Did loading for playback leave anything out? (Then writing the project
/// back would lose it.)
pub fn lossy(p: &Playable) -> bool {
    !p.fallbacks.is_empty()
}

/// The fallbacks as warnings, in validation's form.
pub fn as_issues(p: &Playable) -> Vec<validate::Issue> {
    p.fallbacks
        .iter()
        .map(|f| {
            let (path, message) = f.split_once(": ").unwrap_or(("", f));
            validate::Issue {
                severity: Severity::Warning,
                path: path.into(),
                message: message.into(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn newer() -> Value {
        let mut v = serde_json::to_value(Project::empty("Future")).unwrap();
        v["hologram"] = json!({"angle": 3});
        v["meta"]["mood"] = json!("bright");
        v["channels"] = json!([
            {"id": "a", "name": "A", "instrument": {"type": "quantum-synth", "params": {"warp": 9}}},
            {"id": "b", "name": "B", "instrument": {"type": "analog", "params": {"cutoff": 800, "flux": 1}, "options": {"filter": "time-crystal"}}, "sparkle": true}
        ]);
        v["mixer"]["inserts"][0]["effects"] =
            json!([{"type": "eq"}, {"type": "tesseract-reverb"}, {"type": "limiter"}]);
        v
    }

    #[test]
    fn a_newer_project_still_plays() {
        let text = newer().to_string();
        assert!(
            !validate::parse_and_validate(&text).is_ok(),
            "editing stays strict"
        );
        let p = for_playback(&text).unwrap();
        assert_eq!(p.project.channels[0].instrument.kind, STAND_IN);
        assert_eq!(
            p.project.channels[1].instrument.params.get("cutoff"),
            Some(&800.0)
        );
        assert!(!p.project.channels[1].instrument.params.contains_key("flux"));
        assert!(!p.project.channels[1]
            .instrument
            .options
            .contains_key("filter"));
        let fx: Vec<&str> = p.project.mixer.inserts[0]
            .effects
            .iter()
            .map(|e| e.kind.as_str())
            .collect();
        assert_eq!(fx, ["eq", "limiter"]);
        let all = p.fallbacks.join("\n");
        for what in [
            "hologram",
            "meta.mood",
            "channels[1].sparkle",
            "quantum-synth",
            "params.flux",
            "options.filter",
            "tesseract-reverb",
        ] {
            assert!(all.contains(what), "{what} is reported:\n{all}");
        }
        assert!(lossy(&p));
        assert_eq!(as_issues(&p).len(), p.fallbacks.len());
    }

    #[test]
    fn a_current_project_needs_no_fallback() {
        let demo = include_str!("../../studio/assets/demo/project.json");
        let p = for_playback(demo).unwrap();
        assert!(p.fallbacks.is_empty(), "{:?}", p.fallbacks);
        assert_eq!(
            p.project,
            validate::parse_and_validate(demo).project.unwrap()
        );
    }

    #[test]
    fn real_errors_still_fail() {
        let mut v = newer();
        v["transport"]["bpm"] = json!(-5);
        assert!(value_for_playback(v).is_err());
        assert!(for_playback("{").is_err());
    }
}
