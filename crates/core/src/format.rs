//! A compact, diff- and agent-friendly JSON printer.
//!
//! Small objects and arrays are kept on one line (so every note is one line)
//! while larger structures are indented. Whole-number floats print without a
//! trailing `.0`, and floats are rounded to 6 decimals to avoid noise such as
//! `0.30000000000000004`.

use serde::Serialize;
use serde_json::Value;

const WIDTH: usize = 110;

pub fn to_string<T: Serialize>(value: &T) -> String {
    let v = serde_json::to_value(value).expect("serializable");
    let mut out = String::new();
    write(&v, 0, &mut out);
    out.push('\n');
    out
}

fn write(v: &Value, indent: usize, out: &mut String) {
    match v {
        Value::Array(items) if !items.is_empty() => {
            let inline = inline(v);
            if indent + inline.len() <= WIDTH && !has_nested_container_list(v) {
                out.push_str(&inline);
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                pad(indent + 2, out);
                write(item, indent + 2, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(indent, out);
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            let inline = inline(v);
            if indent + inline.len() <= WIDTH && !has_nested_container_list(v) {
                out.push_str(&inline);
                return;
            }
            out.push_str("{\n");
            let n = map.len();
            for (i, (k, item)) in map.iter().enumerate() {
                pad(indent + 2, out);
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(": ");
                write(item, indent + 2, out);
                if i + 1 < n {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(indent, out);
            out.push('}');
        }
        _ => out.push_str(&inline(v)),
    }
}

/// Lists of objects (notes, clips, channels...) always get one item per line.
fn has_nested_container_list(v: &Value) -> bool {
    match v {
        Value::Array(items) => {
            items.len() > 1 && items.iter().any(|i| i.is_object() || i.is_array())
        }
        Value::Object(map) => map.values().any(has_nested_container_list),
        _ => false,
    }
}

fn pad(n: usize, out: &mut String) {
    for _ in 0..n {
        out.push(' ');
    }
}

fn inline(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.is_f64() {
                format_f64(n.as_f64().unwrap())
            } else {
                n.to_string()
            }
        }
        Value::String(s) => serde_json::to_string(s).unwrap(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(inline).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            if map.is_empty() {
                return "{}".into();
            }
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", serde_json::to_string(k).unwrap(), inline(v)))
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}

pub fn format_f64(x: f64) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let r = (x * 1e6).round() / 1e6;
    if r.fract() == 0.0 && r.abs() < 1e15 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// A file-name friendly form of a title (`My Song!` → `my-song`).
pub fn slug(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "untitled".into()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_tidy() {
        assert_eq!(format_f64(4.0), "4");
        assert_eq!(format_f64(0.1 + 0.2), "0.3");
        assert_eq!(format_f64(-1.5), "-1.5");
    }

    #[test]
    fn lists_of_objects_are_one_per_line() {
        let v = serde_json::json!({"notes": [{"a": 1}, {"a": 2}]});
        let s = to_string(&v);
        assert_eq!(
            s,
            "{\n  \"notes\": [\n    { \"a\": 1 },\n    { \"a\": 2 }\n  ]\n}\n"
        );
    }
}
