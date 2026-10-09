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


/// `_value(field)`: `_scalar` of a compact parameter, or the plain number itself.
pub(crate) fn value_of(v: &Value) -> Result<f64> {
    if v.is_map() {
        if v.get("useVariable").is_some_and(Value::truthy) {
            return err("expected a literal, got a variable");
        }
        return v.get("value").and_then(num).ok_or_else(|| "expected a number".to_string());
    }
    num(v).ok_or_else(|| "expected a number".to_string())
}

/// `fk._wait_seconds(state)`: the one enabled Wait (one value) or WaitRandom (two) time in a state.
pub(crate) fn wait_seconds(state: &Value) -> Result<Vec<f64>> {
    let data = get(state, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    let mut found: Vec<Vec<f64>> = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let short = n.str().map(|s| s.rsplit('.').next().unwrap_or("").to_string()).unwrap_or_default();
        if (short != "Wait" && short != "WaitRandom") || !enabled.get(i).is_some_and(Value::truthy) {
            continue;
        }
        let fields = u(action_fields(data, i, false))?;
        let scalar = |k: &str| -> Result<f64> {
            let v = field(&fields, k).ok_or_else(|| format!("missing {k}"))?;
            if v.is_map() && v.get("useVariable").is_some_and(Value::truthy) {
                return err("a wait time is a variable");
            }
            let v = if v.is_map() { v.get("value").cloned().ok_or("no value")? } else { v.clone() };
            num(&v).ok_or_else(|| "wait time is not a number".to_string())
        };
        found.push(if short == "Wait" { vec![scalar("time")?] } else { vec![scalar("timeMin")?, scalar("timeMax")?] });
    }
    if found.len() != 1 {
        return err(format!("expected one Wait in state {:?}, found {}", get(state, "name")?.str().unwrap_or_default(), found.len()));
    }
    Ok(found.remove(0))
}

/// `_vector3(state, action_index, name)`: a literal FsmVector3 parameter of one action.
pub(crate) fn vector3_param(state: &Value, action_index: usize, name: &str) -> Result<[f64; 3]> {
    let data = get(state, "actionData")?;
    let ints = |k: &str| -> Vec<i64> { data.get(k).and_then(Value::list).unwrap_or(&[]).iter().map(|x| x.int().unwrap_or(0)).collect() };
    let (starts, kinds, pos) = (ints("actionStartIndex"), ints("paramDataType"), ints("paramDataPos"));
    let bytes = ints("byteData");
    let names = get(data, "paramName")?.list().unwrap_or(&[]);
    let count = get(data, "actionNames")?.list().unwrap_or(&[]).len();
    let end = if action_index + 1 < count { starts[action_index + 1] as usize } else { names.len() };
    for i in starts[action_index] as usize..end {
        if names[i].str().as_deref() == Some(name) && kinds[i] == 28 {
            let p = pos[i] as usize;
            let raw: Vec<u8> = bytes[p..p + 13].iter().map(|b| *b as u8).collect();
            if raw[12] != 0 {
                return err(format!("{} {name} is a variable", get(state, "name")?.str().unwrap_or_default()));
            }
            let f = |k: usize| f32::from_le_bytes(raw[4 * k..4 * k + 4].try_into().unwrap()) as f64;
            return Ok([f(0), f(1), f(2)]);
        }
    }
    err(format!("{} lacks a literal {name}", get(state, "name")?.str().unwrap_or_default()))
}

/// `_prefab(s, ...)`: a prefab root's components by type name, every instance kept.
pub struct MultiPrefab {
    pub obj: Obj,
    pub go: Value,
    pub parts: Vec<(String, Vec<(Obj, Value)>)>,
}

impl MultiPrefab {
    pub fn read(source: &Source, obj: Obj) -> Result<MultiPrefab> {
        let go = u(source.read(&obj))?;
        let mut parts: Vec<(String, Vec<(Obj, Value)>)> = Vec::new();
        for c in get(&go, "m_Component")?.list().unwrap_or(&[]) {
            let o = u(source.deref(&obj.file, get(c, "component")?))?;
            let kind = u(source.typename(&o))?;
            let tree = u(source.read(&o))?;
            match parts.iter_mut().find(|p| p.0 == kind) {
                Some(slot) => slot.1.push((o, tree)),
                None => parts.push((kind, vec![(o, tree)])),
            }
        }
        Ok(MultiPrefab { obj, go, parts })
    }

    pub fn first(&self, kind: &str) -> Result<&(Obj, Value)> {
        self.parts.iter().find(|p| p.0 == kind).and_then(|p| p.1.first()).ok_or_else(|| format!("prefab lacks {kind}"))
    }

    pub fn tree(&self, kind: &str) -> Result<&Value> {
        Ok(&self.first(kind)?.1)
    }

    pub fn name(&self) -> String {
        self.go.get("m_Name").and_then(Value::str).unwrap_or_default()
    }
}

/// `focus.action_parameters(data, index)`: an action's fields in declaration order, with their real names.
pub(crate) fn action_parameters(data: &Value, index: usize) -> Result<Vec<(Option<String>, Value)>> {
    let fields = u(action_fields(data, index, false))?;
    let ints = |k: &str| -> Vec<i64> { data.get(k).and_then(Value::list).unwrap_or(&[]).iter().map(|x| x.int().unwrap_or(0)).collect() };
    let starts = ints("actionStartIndex");
    let names = get(data, "paramName")?.list().unwrap_or(&[]);
    let count = get(data, "actionNames")?.list().unwrap_or(&[]).len();
    let end = if index + 1 < count { starts[index + 1] as usize } else { names.len() };
    let mut out = Vec::new();
    for (i, param) in names.iter().enumerate().take(end).skip(starts[index] as usize) {
        let name = param.str().unwrap_or_default();
        let key = if name.is_empty() { i.to_string() } else { name.clone() };
        if let Some(v) = field(&fields, &key) {
            out.push((if name.is_empty() { None } else { Some(name) }, v.clone()));
        }
    }
    Ok(out)
}
