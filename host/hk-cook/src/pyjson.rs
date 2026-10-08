//! Python's `json.dumps(value, indent=2)` text for the reports the cookers
//! write (host/source.py `dump`), keys in insertion order, ASCII only.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

fn string(s: &str, out: &mut String) {
    hk_unity::value::json_string(s.as_bytes(), out);
}

fn write(v: &Json, level: usize, out: &mut String) {
    let pad = |n: usize, out: &mut String| out.extend(std::iter::repeat_n(' ', n * 2));
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(f) => out.push_str(&float(*f)),
        Json::Str(s) => string(s, out),
        Json::List(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(items) if items.is_empty() => out.push_str("{}"),
        Json::List(items) => {
            out.push_str("[\n");
            for (i, x) in items.iter().enumerate() {
                pad(level + 1, out);
                write(x, level + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(level, out);
            out.push(']');
        }
        Json::Obj(items) => {
            out.push_str("{\n");
            for (i, (k, x)) in items.iter().enumerate() {
                pad(level + 1, out);
                string(k, out);
                out.push_str(": ");
                write(x, level + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(level, out);
            out.push('}');
        }
    }
}

pub fn dumps(v: &Json) -> String {
    let mut out = String::new();
    write(v, 0, &mut out);
    out
}

/// Python's `json.dumps(value, sort_keys=True)`: one line, `", "` and `": "`
/// separators, keys in code point order (the cache keys the cookers hash).
pub fn dumps_sorted_compact(v: &Json) -> String {
    fn go(v: &Json, out: &mut String) {
        match v {
            Json::List(items) => {
                out.push('[');
                for (i, x) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    go(x, out);
                }
                out.push(']');
            }
            Json::Obj(items) => {
                let mut sorted: Vec<&(String, Json)> = items.iter().collect();
                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                out.push('{');
                for (i, (k, x)) in sorted.into_iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    string(k, out);
                    out.push_str(": ");
                    go(x, out);
                }
                out.push('}');
            }
            other => write(other, 0, out),
        }
    }
    let mut out = String::new();
    go(v, &mut out);
    out
}

/// A `serde_json` value as Python's `json` would hand it back (objects keep
/// serde's key order, which is sorted).
pub fn from_serde(v: &serde_json::Value) -> Json {
    use serde_json::Value as J;
    match v {
        J::Null => Json::Null,
        J::Bool(b) => Json::Bool(*b),
        J::Number(n) => n.as_i64().map(Json::Int).unwrap_or_else(|| Json::Float(n.as_f64().unwrap_or(f64::NAN))),
        J::String(s) => Json::Str(s.clone()),
        J::Array(a) => Json::List(a.iter().map(from_serde).collect()),
        J::Object(o) => Json::Obj(o.iter().map(|(k, x)| (k.clone(), from_serde(x))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_json_dumps_indent_2() {
        let v = Json::Obj(vec![
            ("a".into(), Json::Int(1)),
            ("b".into(), Json::List(vec![Json::List(vec![Json::Str("x".into()), Json::Str("é\"".into())])])),
            ("c".into(), Json::List(vec![])),
            ("d".into(), Json::Obj(vec![])),
            ("e".into(), Json::Null),
        ]);
        let want = "{\n  \"a\": 1,\n  \"b\": [\n    [\n      \"x\",\n      \"\\u00e9\\\"\"\n    ]\n  ],\n  \"c\": [],\n  \"d\": {},\n  \"e\": null\n}";
        assert_eq!(dumps(&v), want);
    }
}

/// Python's `json` text for one float: `float.__repr__`, with its names for
/// the non-finite values.
pub fn float(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else {
        crate::pyfloat::repr(f)
    }
}

/// Python's `json.dumps(value, sort_keys=True)` (default separators, no
/// indent) of a value a type tree read, as UnityPy hands it to Python.
pub fn dumps_sorted(v: &hk_unity::Value, out: &mut String) {
    use hk_unity::Value;
    match v {
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::UInt(i) => out.push_str(&i.to_string()),
        Value::F32(f) => out.push_str(&float(*f as f64)),
        Value::F64(f) => out.push_str(&float(*f)),
        Value::Str(s) => hk_unity::value::json_string(s, out),
        // UnityPy hands raw bytes to Python as `bytes`, which json refuses;
        // nothing hashed here carries any.
        Value::Bytes(_) => panic!("json cannot encode bytes"),
        Value::List(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps_sorted(x, out);
            }
            out.push(']');
        }
        Value::Map(fields) => {
            let mut sorted: Vec<&(std::sync::Arc<str>, Value)> = fields.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (i, (k, x)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                hk_unity::value::json_string(k.as_bytes(), out);
                out.push_str(": ");
                dumps_sorted(x, out);
            }
            out.push('}');
        }
    }
}
