//! Reading prefabs and PlayMaker states the way host/false_knight_art.py does
//! (`_state`, `_actions`, `_one`, `_scalar`, `_enum`, `_prefab_parts`, `_prefab_box`).

use crate::common::{err, get, Result};
use crate::cook_audio::u;
use crate::recog::xy;
use hk_unity::playmaker::{action_fields, field, Fields};
use hk_unity::{Obj, Source, Value};

pub(crate) fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(*b as i64 as f64),
        o => o.float(),
    }
}

pub(crate) fn single_state<'a>(fsm: &'a Value, name: &str) -> Result<&'a Value> {
    let found: Vec<&Value> = get(fsm, "states")?.list().unwrap_or(&[]).iter().filter(|s| s.get("name").and_then(Value::str).as_deref() == Some(name)).collect();
    if found.len() != 1 {
        return err(format!("no single state {name}"));
    }
    Ok(found[0])
}

/// `_actions`: enabled actions of one type in a state, as (fields, index).
pub(crate) fn actions_of(state: &Value, kind: &str) -> Result<Vec<(Fields, usize)>> {
    let data = get(state, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    let mut out = Vec::new();
    for (i, nm) in names.iter().enumerate() {
        if nm.str().is_some_and(|s| s.rsplit('.').next() == Some(kind)) && enabled.get(i).is_some_and(Value::truthy) {
            out.push((u(action_fields(data, i, true))?, i));
        }
    }
    Ok(out)
}

pub(crate) fn one_of(state: &Value, kind: &str) -> Result<Fields> {
    let mut found = actions_of(state, kind)?;
    if found.len() != 1 {
        return err(format!("{} has {} {kind} actions, expected one", get(state, "name")?.str().unwrap_or_default(), found.len()));
    }
    Ok(found.remove(0).0)
}

/// `_scalar`: a compact parameter that is a literal.
pub(crate) fn literal(fields: &Fields, key: &str) -> Result<f64> {
    match field(fields, key) {
        Some(v) if v.is_map() && !v.get("useVariable").is_some_and(Value::truthy) => v.get("value").and_then(num).ok_or_else(|| "expected a number".to_string()),
        _ => err(format!("expected a literal for {key}")),
    }
}

/// `_enum`: a PlayMaker enum parameter as its int.
pub(crate) fn enum_param(state: &Value, index: usize, name: &str) -> Result<Option<i64>> {
    let data = get(state, "actionData")?;
    let ints = |k: &str| -> Vec<i64> { data.get(k).and_then(Value::list).unwrap_or(&[]).iter().map(|x| x.int().unwrap_or(0)).collect() };
    let (starts, pos, sizes, bytes) = (ints("actionStartIndex"), ints("paramDataPos"), ints("paramByteDataSize"), ints("byteData"));
    let params = get(data, "paramName")?.list().unwrap_or(&[]);
    let count = get(data, "actionNames")?.list().unwrap_or(&[]).len();
    let start = starts[index] as usize;
    let end = if index + 1 < count { starts[index + 1] as usize } else { params.len() };
    for i in start..end {
        if params[i].str().as_deref() == Some(name) {
            let (p, size) = (pos[i] as usize, sizes[i]);
            if size >= 4 {
                let raw: Vec<u8> = bytes[p..p + 4].iter().map(|b| *b as u8).collect();
                return Ok(Some(i32::from_le_bytes(raw.try_into().unwrap()) as i64));
            }
            return Ok(None);
        }
    }
    err("action lacks the parameter")
}

/// `_prefab_parts`: (typename, object, tree) for every component of a prefab GameObject.
pub(crate) fn prefab_parts(source: &Source, go_o: &Obj) -> Result<Vec<(String, Obj, Value)>> {
    let go = u(source.read(go_o))?;
    let mut out = Vec::new();
    for c in get(&go, "m_Component")?.list().unwrap_or(&[]) {
        let o = u(source.deref(&go_o.file, get(c, "component")?))?;
        out.push((u(source.typename(&o))?, o.clone(), u(source.read(&o))?));
    }
    Ok(out)
}

/// `_prefab_box`: the one trigger BoxCollider2D as (x0, y0, x1, y1).
pub(crate) fn prefab_box(parts: &[(String, Obj, Value)]) -> Result<[f64; 4]> {
    let boxes: Vec<&Value> = parts.iter().filter(|p| p.0 == "BoxCollider2D").map(|p| &p.2).collect();
    if boxes.len() != 1 || !get(boxes[0], "m_IsTrigger")?.truthy() {
        return err("expected one trigger BoxCollider2D");
    }
    let (o, s) = (xy(boxes[0], "m_Offset")?, xy(boxes[0], "m_Size")?);
    Ok([o[0] - s[0] / 2.0, o[1] - s[1] / 2.0, o[0] + s[0] / 2.0, o[1] + s[1] / 2.0])
}

