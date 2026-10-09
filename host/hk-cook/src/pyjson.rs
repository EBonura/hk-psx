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

/// Python's `json.loads` into an order-preserving tree: object keys stay in
/// file order (a repeated key keeps its first position and last value), a
/// number with a point or exponent is a float, any other an integer.
pub fn parse(text: &str) -> Result<Json, String> {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
                self.i += 1;
            }
        }
        fn lit(&mut self, word: &str) -> bool {
            if self.s[self.i..].starts_with(word.as_bytes()) {
                self.i += word.len();
                true
            } else {
                false
            }
        }
        fn hex4(&mut self) -> Result<u32, String> {
            let h = self.s.get(self.i..self.i + 4).ok_or("truncated \\u escape")?;
            self.i += 4;
            u32::from_str_radix(std::str::from_utf8(h).map_err(|e| e.to_string())?, 16).map_err(|e| e.to_string())
        }
        fn string(&mut self) -> Result<String, String> {
            self.i += 1;
            let mut out: Vec<u8> = Vec::new();
            loop {
                let c = *self.s.get(self.i).ok_or("unterminated string")?;
                self.i += 1;
                match c {
                    b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                    b'\\' => {
                        let e = *self.s.get(self.i).ok_or("bad escape")?;
                        self.i += 1;
                        let ch = match e {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => {
                                let mut cp = self.hex4()?;
                                if (0xd800..0xdc00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    cp = 0x10000 + ((cp - 0xd800) << 10) + (lo.wrapping_sub(0xdc00) & 0x3ff);
                                }
                                char::from_u32(cp).unwrap_or('\u{fffd}')
                            }
                            _ => return Err("bad escape".into()),
                        };
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    c => out.push(c),
                }
            }
        }
        fn value(&mut self) -> Result<Json, String> {
            self.ws();
            match self.s.get(self.i).copied().ok_or("unexpected end")? {
                b'{' => {
                    self.i += 1;
                    let mut fields: Vec<(String, Json)> = Vec::new();
                    self.ws();
                    if self.s[self.i] == b'}' {
                        self.i += 1;
                        return Ok(Json::Obj(fields));
                    }
                    loop {
                        self.ws();
                        let k = self.string()?;
                        self.ws();
                        if self.s.get(self.i) != Some(&b':') {
                            return Err("expected ':'".into());
                        }
                        self.i += 1;
                        let v = self.value()?;
                        match fields.iter_mut().find(|f| f.0 == k) {
                            Some(slot) => slot.1 = v,
                            None => fields.push((k, v)),
                        }
                        self.ws();
                        match self.s.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b'}') => {
                                self.i += 1;
                                return Ok(Json::Obj(fields));
                            }
                            _ => return Err("expected ',' or '}'".into()),
                        }
                    }
                }
                b'[' => {
                    self.i += 1;
                    let mut items = Vec::new();
                    self.ws();
                    if self.s[self.i] == b']' {
                        self.i += 1;
                        return Ok(Json::List(items));
                    }
                    loop {
                        items.push(self.value()?);
                        self.ws();
                        match self.s.get(self.i) {
                            Some(b',') => self.i += 1,
                            Some(b']') => {
                                self.i += 1;
                                return Ok(Json::List(items));
                            }
                            _ => return Err("expected ',' or ']'".into()),
                        }
                    }
                }
                b'"' => Ok(Json::Str(self.string()?)),
                _ if self.lit("true") => Ok(Json::Bool(true)),
                _ if self.lit("false") => Ok(Json::Bool(false)),
                _ if self.lit("null") => Ok(Json::Null),
                _ if self.lit("NaN") => Ok(Json::Float(f64::NAN)),
                _ if self.lit("Infinity") => Ok(Json::Float(f64::INFINITY)),
                _ if self.lit("-Infinity") => Ok(Json::Float(f64::NEG_INFINITY)),
                _ => {
                    let start = self.i;
                    while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                        self.i += 1;
                    }
                    let n = std::str::from_utf8(&self.s[start..self.i]).map_err(|e| e.to_string())?;
                    if n.contains(['.', 'e', 'E']) {
                        n.parse::<f64>().map(Json::Float).map_err(|e| format!("{n}: {e}"))
                    } else {
                        n.parse::<i64>().map(Json::Int).map_err(|e| format!("{n}: {e}"))
                    }
                }
            }
        }
    }
    let mut p = P { s: text.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err("trailing data".into());
    }
    Ok(v)
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

    #[test]
    fn parse_reads_back_what_dumps_wrote_in_the_same_order() {
        let v = Json::Obj(vec![
            ("z".into(), Json::Int(-3)),
            ("a".into(), Json::List(vec![Json::Float(0.1), Json::Float(1e22), Json::Null, Json::Bool(true), Json::Float(2.0)])),
            ("s".into(), Json::Str("é\"\\\n\u{1f600}".into())),
            ("e".into(), Json::Obj(vec![])),
        ]);
        assert_eq!(parse(&dumps(&v)).unwrap(), v);
        assert!(parse("{\"a\": 1,}").is_err());
        assert_eq!(parse("{\"k\": 1, \"j\": 2, \"k\": 3}").unwrap(), Json::Obj(vec![("k".into(), Json::Int(3)), ("j".into(), Json::Int(2))]));
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
