//! JSON Patch (RFC 6902) on a project document, in memory: `add`, `remove`,
//! `replace`, `move`, `copy` and `test`. Errors name the operation and the
//! JSON pointer (`whatIf[2] replace /channels/9/volume: no element 9`).

use serde_json::Value;

fn unescape(s: &str) -> String {
    s.replace("~1", "/").replace("~0", "~")
}

fn parts(path: &str) -> Result<Vec<String>, String> {
    if path.is_empty() {
        return Ok(vec![]);
    }
    if !path.starts_with('/') {
        return Err("a JSON pointer starts with /".into());
    }
    Ok(path.split('/').skip(1).map(unescape).collect())
}

fn index(seg: &str, len: usize, append: bool) -> Result<usize, String> {
    if append && seg == "-" {
        return Ok(len);
    }
    let i: usize = seg
        .parse()
        .map_err(|_| format!("{seg:?} is not an array index"))?;
    if i > len || (!append && i == len) {
        return Err(format!("no element {i}"));
    }
    Ok(i)
}

fn get<'a>(doc: &'a Value, path: &str) -> Result<&'a Value, String> {
    let mut at = doc;
    for seg in parts(path)? {
        at = match at {
            Value::Object(m) => m.get(&seg).ok_or_else(|| format!("no member {seg:?}"))?,
            Value::Array(a) => &a[index(&seg, a.len(), false)?],
            _ => return Err(format!("{seg:?}: not inside an object or array")),
        };
    }
    Ok(at)
}

fn parent<'a>(doc: &'a mut Value, path: &str) -> Result<(&'a mut Value, String), String> {
    let mut p = parts(path)?;
    let last = p.pop().ok_or("the whole document cannot be the target")?;
    let mut at = doc;
    for seg in p {
        at = match at {
            Value::Object(m) => m
                .get_mut(&seg)
                .ok_or_else(|| format!("no member {seg:?}"))?,
            Value::Array(a) => {
                let i = index(&seg, a.len(), false)?;
                &mut a[i]
            }
            _ => return Err(format!("{seg:?}: not inside an object or array")),
        };
    }
    Ok((at, last))
}

fn add(doc: &mut Value, path: &str, v: Value) -> Result<(), String> {
    let (at, last) = parent(doc, path)?;
    match at {
        Value::Object(m) => {
            m.insert(last, v);
        }
        Value::Array(a) => {
            let i = index(&last, a.len(), true)?;
            a.insert(i, v);
        }
        _ => return Err("the parent is not an object or array".into()),
    }
    Ok(())
}

fn remove(doc: &mut Value, path: &str) -> Result<Value, String> {
    let (at, last) = parent(doc, path)?;
    match at {
        Value::Object(m) => m.remove(&last).ok_or_else(|| format!("no member {last:?}")),
        Value::Array(a) => {
            let i = index(&last, a.len(), false)?;
            Ok(a.remove(i))
        }
        _ => Err("the parent is not an object or array".into()),
    }
}

/// JSON equality as RFC 6902 `test` means it: numbers by value (120 = 120.0).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

/// Apply operations in order; on an error the document is left as it was.
/// `what` names the list in messages (`whatIf`, `suggestion`).
pub fn apply(doc: &mut Value, ops: &[Value], what: &str) -> Result<(), String> {
    let mut work = doc.clone();
    for (i, op) in ops.iter().enumerate() {
        let field = |k: &str| op.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let name = field("op").unwrap_or_default();
        let path = field("path").unwrap_or_default();
        let fail = |e: String| format!("{what}[{i}] {name} {path}: {e}");
        if op.get("path").and_then(|p| p.as_str()).is_none() {
            return Err(fail("missing \"path\"".into()));
        }
        let value = || {
            op.get("value")
                .cloned()
                .ok_or_else(|| fail("missing \"value\"".into()))
        };
        let from = || field("from").ok_or_else(|| fail("missing \"from\"".into()));
        match name.as_str() {
            "add" => add(&mut work, &path, value()?).map_err(fail)?,
            "remove" => {
                remove(&mut work, &path).map_err(fail)?;
            }
            "replace" => {
                get(&work, &path).map_err(fail)?;
                let v = value()?;
                if path.is_empty() {
                    work = v;
                } else {
                    remove(&mut work, &path).map_err(fail)?;
                    add(&mut work, &path, v).map_err(fail)?;
                }
            }
            "move" => {
                let f = from()?;
                let v = remove(&mut work, &f).map_err(fail)?;
                add(&mut work, &path, v).map_err(fail)?;
            }
            "copy" => {
                let f = from()?;
                let v = get(&work, &f).map_err(fail)?.clone();
                add(&mut work, &path, v).map_err(fail)?;
            }
            "test" => {
                if !same(get(&work, &path).map_err(fail)?, &value()?) {
                    return Err(fail("the value differs".into()));
                }
            }
            "" => return Err(fail("missing \"op\"".into())),
            o => return Err(fail(format!("unknown operation {o:?}"))),
        }
    }
    *doc = work;
    Ok(())
}

/// Parse `--what-if`: a JSON array of operations (or one operation), or
/// `@file.json`, read by `read`.
pub fn parse_ops(
    text: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<Vec<Value>, String> {
    let body = match text.trim().strip_prefix('@') {
        Some(file) => read(file)?,
        None => text.to_string(),
    };
    let v: Value = serde_json::from_str(&body).map_err(|e| format!("what-if: not JSON ({e})"))?;
    match v {
        Value::Array(a) => Ok(a),
        Value::Object(_) => Ok(vec![v]),
        _ => Err("what-if: expected a JSON Patch array".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn operations() {
        let mut d = json!({"a": [1, 2, 3], "b": {"c": 1}});
        apply(
            &mut d,
            &[
                json!({"op": "replace", "path": "/a/1", "value": 9}),
                json!({"op": "add", "path": "/a/-", "value": 4}),
                json!({"op": "remove", "path": "/a/0"}),
                json!({"op": "copy", "from": "/b/c", "path": "/b/d"}),
                json!({"op": "move", "from": "/b/c", "path": "/e"}),
                json!({"op": "test", "path": "/e", "value": 1}),
            ],
            "whatIf",
        )
        .unwrap();
        assert_eq!(d, json!({"a": [9, 3, 4], "b": {"d": 1}, "e": 1}));
        // `test` compares numbers by value.
        let mut f = json!({"bpm": 120.0});
        apply(
            &mut f,
            &[json!({"op": "test", "path": "/bpm", "value": 120})],
            "whatIf",
        )
        .unwrap();
        let e = apply(
            &mut d,
            &[json!({"op": "replace", "path": "/a/7", "value": 0})],
            "whatIf",
        );
        assert_eq!(e.unwrap_err(), "whatIf[0] replace /a/7: no element 7");
        let e = apply(
            &mut d,
            &[json!({"op": "replace", "path": "/zz", "value": 0})],
            "whatIf",
        );
        assert_eq!(e.unwrap_err(), "whatIf[0] replace /zz: no member \"zz\"");
    }
}
