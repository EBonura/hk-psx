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
    v.and_then(Value::list).map(|l| l.iter().map(|x| x.int().unwrap_or(0)).collect()).unwrap_or_default()
}

fn compact(value: Value, use_variable: bool, name: String) -> Value {
    let key = |s: &str| -> Arc<str> { s.into() };
    Value::Map(vec![(key("value"), value), (key("useVariable"), Value::Bool(use_variable)), (key("name"), Value::Str(name.into_bytes()))])
}

/// host/focus.py `action_fields`: the compact scalar fields of one action.
pub fn action_fields(data: &Value, index: usize, objects: bool) -> Result<Fields> {
    let get = |k: &str| data.get(k).ok_or_else(|| Error::Format(format!("actionData lacks {k}")));
    let names = get("actionNames")?.list().unwrap_or(&[]).len();
    let starts = ints(Some(get("actionStartIndex")?));
    let param_names = get("paramName")?.list().unwrap_or(&[]).to_vec();
    let kinds = ints(Some(get("paramDataType")?));
    let positions = ints(Some(get("paramDataPos")?));
    let sizes = ints(Some(get("paramByteDataSize")?));
    let bytes: Vec<u8> = ints(Some(get("byteData")?)).into_iter().map(|b| b as u8).collect();
    let start = *starts.get(index).ok_or_else(|| Error::Format("action index out of range".into()))? as usize;
    let end = if index + 1 < names { starts[index + 1] as usize } else { param_names.len() };
    let mut fields = Fields::new();
    for i in start..end {
        let name = param_names.get(i).and_then(Value::str).filter(|s| !s.is_empty()).unwrap_or_else(|| i.to_string());
        let kind = kinds[i];
        let pos = positions[i];
        let size = sizes[i];
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
                let text = std::str::from_utf8(&raw[n + 1..]).map_err(|_| Error::Format("compact FSM name is not UTF-8".into()))?;
                compact(scalar, raw[n] != 0, text.to_string())
            }
            23 => {
                let text = std::str::from_utf8(raw).map_err(|_| Error::Format("FSM string is not UTF-8".into()))?;
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
    let list = data.get(key).and_then(Value::list).ok_or_else(|| Error::Format(format!("actionData lacks {key}")))?;
    // Python indexing: a negative position counts from the end.
    let i = if pos < 0 { list.len() as i64 + pos } else { pos };
    list.get(i as usize).cloned().ok_or_else(|| Error::Format(format!("{key} index out of range")))
}

/// One state's enabled actions as (short name, index), in authored order.
pub fn enabled(state: &Value) -> Vec<(String, usize)> {
    let Some(d) = state.get("actionData") else { return vec![] };
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
