//! Values read from a type tree, in field order.

use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    UInt(u64),
    F32(f32),
    F64(f64),
    /// Raw string bytes; Unity strings are UTF-8 but not guaranteed valid.
    Str(Vec<u8>),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(Vec<(Arc<str>, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(fields) => fields.iter().find(|(k, _)| &**k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match *self {
            Value::Int(v) => Some(v),
            Value::UInt(v) => i64::try_from(v).ok(),
            Value::Bool(b) => Some(b as i64),
            _ => None,
        }
    }
    pub fn float(&self) -> Option<f64> {
        match *self {
            Value::F32(v) => Some(v as f64),
            Value::F64(v) => Some(v),
            Value::Int(v) => Some(v as f64),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(String::from_utf8_lossy(s).into_owned()),
            _ => None,
        }
    }
    pub fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v),
            _ => None,
        }
    }
    /// Python truthiness of the value the Python tools held.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(v) => *v != 0,
            Value::UInt(v) => *v != 0,
            Value::F32(v) => *v != 0.0,
            Value::F64(v) => *v != 0.0,
            Value::Str(s) | Value::Bytes(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }
    pub fn is_map(&self) -> bool {
        matches!(self, Value::Map(_))
    }
    fn number(&self) -> Option<f64> {
        match *self {
            Value::Bool(b) => Some(b as i64 as f64),
            Value::Int(v) => Some(v as f64),
            Value::UInt(v) => Some(v as f64),
            Value::F32(v) => Some(v as f64),
            Value::F64(v) => Some(v),
            _ => None,
        }
    }
    /// Python `==` between the values the Python tools held (numbers compare
    /// across bool, int and float; dicts ignore key order).
    pub fn py_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Str(a), Value::Str(b)) | (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
            }
            (Value::Map(a), Value::Map(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v.py_eq(v2)))
            }
            _ => match (self.number(), other.number()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }

    /// (m_FileID, m_PathID) of a PPtr.
    pub fn pptr(&self) -> Option<(i32, i64)> {
        Some((
            self.get("m_FileID")?.int()? as i32,
            self.get("m_PathID")?.int()?,
        ))
    }

    /// The canonical text the parity oracle hashes: JSON-like, keys in field
    /// order, floats as the bits of the f64 Python held, bytes as `#hex#`,
    /// strings as Python's `json.dumps` writes them (ASCII, surrogateescape).
    pub fn canonical(&self, out: &mut String) {
        use std::fmt::Write;
        match self {
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(v) => write!(out, "{v}").unwrap(),
            Value::UInt(v) => write!(out, "{v}").unwrap(),
            Value::F32(v) => write!(out, "f{:016x}", (*v as f64).to_bits()).unwrap(),
            Value::F64(v) => write!(out, "f{:016x}", v.to_bits()).unwrap(),
            Value::Str(s) => json_string(s, out),
            Value::Bytes(b) => {
                out.push('#');
                for x in b {
                    write!(out, "{x:02x}").unwrap();
                }
                out.push('#');
            }
            Value::List(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.canonical(out);
                }
                out.push(']');
            }
            Value::Map(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    json_string(k.as_bytes(), out);
                    out.push(':');
                    v.canonical(out);
                }
                out.push('}');
            }
        }
    }
}

/// Python `json.dumps(bytes.decode('utf8', 'surrogateescape'))`.
pub fn json_string(bytes: &[u8], out: &mut String) {
    use std::fmt::Write;
    out.push('"');
    let mut rest = bytes;
    loop {
        let (valid, bad) = match std::str::from_utf8(rest) {
            Ok(s) => (s, &[][..]),
            Err(e) => {
                let (good, after) = rest.split_at(e.valid_up_to());
                let n = e.error_len().unwrap_or(after.len());
                (std::str::from_utf8(good).unwrap(), &after[..n])
            }
        };
        for ch in valid.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                    let mut buf = [0u16; 2];
                    for unit in c.encode_utf16(&mut buf) {
                        write!(out, "\\u{unit:04x}").unwrap();
                    }
                }
                c => out.push(c),
            }
        }
        for b in bad {
            write!(out, "\\u{:04x}", 0xdc00 + *b as u32).unwrap();
        }
        let consumed = valid.len() + bad.len();
        if consumed >= rest.len() {
            break;
        }
        rest = &rest[consumed..];
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(v: &Value) -> String {
        let mut s = String::new();
        v.canonical(&mut s);
        s
    }

    #[test]
    fn strings_escape_like_python_json() {
        assert_eq!(
            canon(&Value::Str(b"a\"b\\\n\x7f".to_vec())),
            "\"a\\\"b\\\\\\n\\u007f\""
        );
        assert_eq!(
            canon(&Value::Str("é😀".as_bytes().to_vec())),
            "\"\\u00e9\\ud83d\\ude00\""
        );
        // Invalid UTF-8 bytes come through surrogateescape.
        assert_eq!(canon(&Value::Str(vec![b'x', 0xff])), "\"x\\udcff\"");
    }

    #[test]
    fn python_equality_and_truth() {
        assert!(Value::Int(1).py_eq(&Value::F32(1.0)));
        assert!(Value::Bool(true).py_eq(&Value::Int(1)));
        assert!(!Value::Str(b"1".to_vec()).py_eq(&Value::Int(1)));
        assert!(!Value::List(vec![]).truthy());
        assert!(Value::Map(vec![("k".into(), Value::Int(0))]).truthy());
    }
}
