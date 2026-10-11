//! PlayMaker FSM data as the cookers read it (host/focus.py).

use crate::value::Value;
use crate::{Error, Result};
use std::sync::Arc;

/// An ordered dict with Python's assignment semantics (a repeated key keeps its first position).
pub type Fields = Vec<(String, Value)>;

pub fn field<'a>(fields: &'a Fields, key: &str) -> Option<&'a Value> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn put(fields: &mut Fields, key: String, value: Value) {
    match fields.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => fields.push((key, value)),
    }
}

fn ints(v: Option<&Value>) -> Vec<i64> {
    v.and_then(Value::list)
        .map(|l| l.iter().map(|x| x.int().unwrap_or(0)).collect())
        .unwrap_or_default()
}

fn compact(value: Value, use_variable: bool, name: String) -> Value {
    let key = |s: &str| -> Arc<str> { s.into() };
    Value::Map(vec![
        (key("value"), value),
        (key("useVariable"), Value::Bool(use_variable)),
        (key("name"), Value::Str(name.into_bytes())),
    ])
}

/// host/focus.py `action_fields`: the compact scalar fields of one action.
pub fn action_fields(data: &Value, index: usize, objects: bool) -> Result<Fields> {
    decode_fields(data, index, objects, false)
}

/// Include serialized plain strings without changing existing cooker fingerprint pins.
pub fn action_fields_with_plain_strings(data: &Value, index: usize, objects: bool) -> Result<Fields> {
    decode_fields(data, index, objects, true)
}

fn plain_string(bytes: &[u8], pos: i64, size: i64) -> Result<Value> {
    let lo = usize::try_from(pos).map_err(|_| Error::Format("negative FSM string offset".into()))?;
    let size = usize::try_from(size).map_err(|_| Error::Format("negative FSM string size".into()))?;
    let hi = lo.checked_add(size).ok_or_else(|| Error::Format("FSM string range overflow".into()))?;
    let raw = bytes.get(lo..hi).ok_or_else(|| Error::Format("truncated plain FSM string".into()))?;
    std::str::from_utf8(raw).map_err(|_| Error::Format("plain FSM string is not UTF-8".into()))?;
    Ok(Value::Str(raw.to_vec()))
}

fn decode_fields(data: &Value, index: usize, objects: bool, plain_strings: bool) -> Result<Fields> {
    let get = |k: &str| {
        data.get(k)
            .ok_or_else(|| Error::Format(format!("actionData lacks {k}")))
    };
    let names = get("actionNames")?.list().unwrap_or(&[]).len();
    let starts = ints(Some(get("actionStartIndex")?));
    let param_names = get("paramName")?.list().unwrap_or(&[]).to_vec();
    let kinds = ints(Some(get("paramDataType")?));
    let positions = ints(Some(get("paramDataPos")?));
    let sizes = ints(Some(get("paramByteDataSize")?));
    let bytes: Vec<u8> = ints(Some(get("byteData")?))
        .into_iter()
        .map(|b| b as u8)
        .collect();
    let start = *starts
        .get(index)
        .ok_or_else(|| Error::Format("action index out of range".into()))? as usize;
    let end = if index + 1 < names {
        starts[index + 1] as usize
    } else {
        param_names.len()
    };
    let mut fields = Fields::new();
    for i in start..end {
        let name = param_names
            .get(i)
            .and_then(Value::str)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| i.to_string());
        let kind = kinds[i];
        let pos = positions[i];
        let size = sizes[i];
        if kind == 3 && plain_strings {
            put(&mut fields, name, plain_string(&bytes, pos, size)?);
            continue;
        }
        let lo = (pos.max(0) as usize).min(bytes.len());
        let hi = ((pos + size).max(0) as usize).min(bytes.len()).max(lo);
        let raw = &bytes[lo..hi];
        let value = match kind {
            15..=17 if size != 0 => {
                let n = if kind == 17 { 1 } else { 4 };
                if (size as usize) < n + 1 || raw.len() < n + 1 {
                    return Err(Error::Format("truncated compact FSM scalar".into()));
                }
                let scalar = match kind {
                    15 => Value::F32(f32::from_le_bytes(raw[..4].try_into().unwrap())),
                    16 => Value::Int(i32::from_le_bytes(raw[..4].try_into().unwrap()) as i64),
                    _ => Value::Bool(raw[0] != 0),
                };
                let text = std::str::from_utf8(&raw[n + 1..])
                    .map_err(|_| Error::Format("compact FSM name is not UTF-8".into()))?;
                compact(scalar, raw[n] != 0, text.to_string())
            }
            23 => {
                let text = std::str::from_utf8(raw)
                    .map_err(|_| Error::Format("FSM string is not UTF-8".into()))?;
                Value::Str(text.as_bytes().to_vec())
            }
            19 if objects => list_item(data, "fsmGameObjectParams", pos)?,
            18 | 20 | 21 | 31 | 39 => {
                let key = match kind {
                    18 => "fsmStringParams",
                    20 => "fsmOwnerDefaultParams",
                    21 => "functionCallParams",
                    31 => "fsmEventTargetParams",
                    _ => "fsmVarParams",
                };
                list_item(data, key, pos)?
            }
            1 if size == 1 => Value::Bool(raw.first().copied().unwrap_or(0) != 0),
            _ => continue,
        };
        put(&mut fields, name, value);
    }
    Ok(fields)
}

fn list_item(data: &Value, key: &str, pos: i64) -> Result<Value> {
    let list = data
        .get(key)
        .and_then(Value::list)
        .ok_or_else(|| Error::Format(format!("actionData lacks {key}")))?;
    // Python indexing: a negative position counts from the end.
    let i = if pos < 0 {
        list.len() as i64 + pos
    } else {
        pos
    };
    list.get(i as usize)
        .cloned()
        .ok_or_else(|| Error::Format(format!("{key} index out of range")))
}

/// One state's enabled actions as (short name, index), in authored order.
pub fn enabled(state: &Value) -> Vec<(String, usize)> {
    let Some(d) = state.get("actionData") else {
        return vec![];
    };
    let names = d.get("actionNames").and_then(Value::list).unwrap_or(&[]);
    let on = d.get("actionEnabled").and_then(Value::list).unwrap_or(&[]);
    names
        .iter()
        .enumerate()
        .filter(|(i, _)| on.get(*i).is_some_and(Value::truthy))
        .map(|(i, n)| {
            let s = n.str().unwrap_or_default();
            (s.rsplit('.').next().unwrap_or("").to_string(), i)
        })
        .collect()
}

#[cfg(test)]
mod plain_string_tests {
    use super::*;
    #[test]
    fn plain_strings_are_additive_to_legacy_fields() {
        let list = |n:i64| Value::List(vec![Value::Int(n)]);
        let mut data=Value::Map(vec![
            ("actionNames".into(),Value::List(vec![Value::Str(b"CheckAlertRangeByName".to_vec())])),
            ("actionStartIndex".into(),list(0)),
            ("paramName".into(),Value::List(vec![Value::Str(b"childName".to_vec())])),
            ("paramDataType".into(),list(3)),("paramDataPos".into(),list(0)),("paramByteDataSize".into(),list(12)),
            ("byteData".into(),Value::List(b"Attack Range".iter().map(|b|Value::Int(*b as i64)).collect()))]);
        assert!(action_fields(&data,0,false).unwrap().is_empty());
        let checked=action_fields_with_plain_strings(&data,0,false).unwrap();
        assert_eq!(field(&checked,"childName").unwrap().str().as_deref(),Some("Attack Range"));
        if let Value::Map(fields)=&mut data {
            for (key,value) in fields { if &**key=="paramDataPos" || &**key=="paramByteDataSize" { *value=list(i64::MAX); } }
        }
        assert!(action_fields_with_plain_strings(&data,0,false).is_err());
    }
    #[test]
    fn authored_child_name_uses_exact_byte_range() {
        let bytes = b"paddingAttack Rangeunused";
        assert_eq!(plain_string(bytes, 7, 12).unwrap().str().as_deref(), Some("Attack Range"));
        assert_eq!(plain_string(bytes, 7, 0).unwrap().str().as_deref(), Some(""));
    }
    #[test]
    fn invalid_ranges_and_encoding_fail_closed() {
        for (pos,size) in [(-1,1),(0,-1),(2,1),(0,3),(i64::MAX,i64::MAX)] {
            assert!(plain_string(b"ok",pos,size).is_err());
        }
        assert!(plain_string(&[255],0,1).is_err());
    }
}
