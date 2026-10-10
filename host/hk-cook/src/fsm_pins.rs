//! Pin the authored numbers of a PlayMaker FSM, for the recognizers that hold them as constants.
//! Ported from host/fsm_pins.py.
//!
//! A recognizer admits a placement by structural digest (`runner::fsm_fingerprint`); this reads
//! the same FSM back and proves the handful of values the controller actually carries (waits,
//! speeds, ranges, clip names) against it, so the digest says nothing changed and these say what
//! the constants are.

use crate::common::{err, get, Result};
use crate::runner::{fingerprint_with, py_value_repr};
use hk_unity::playmaker::{action_fields, enabled, Fields};
use hk_unity::Value;

/// What a pinned action field must be: a literal, or (`Var`) bound to the named FSM variable.
/// A float literal is compared within 1e-6, as `_same` does for a Python float.
pub enum Pin {
    F(f64),
    I(i64),
    S(&'static str),
    B(bool),
    Var(&'static str),
}

/// One pinned `(state, action, fields)` row.
pub type PinRow<'a> = (&'a str, &'a str, &'a [(&'a str, Pin)]);

/// `state(fsm, name)`: the first state of that name.
pub fn state<'a>(fsm: &'a Value, name: &str) -> Result<&'a Value> {
    get(fsm, "states")?
        .list()
        .unwrap_or(&[])
        .iter()
        .find(|st| st.get("name").and_then(Value::str).as_deref() == Some(name))
        .ok_or_else(|| format!("no state {name}"))
}

/// `variables(fsm)`: the FSM's plain variables by name (vectors and object references left out).
pub fn variables(fsm: &Value) -> Vec<(String, &Value)> {
    let mut out: Vec<(String, &Value)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, group) in groups {
            for v in group.list().unwrap_or(&[]) {
                if !v.is_map() {
                    continue;
                }
                let (Some(name), Some(value)) = (v.get("name"), v.get("value")) else {
                    continue;
                };
                if value.is_map() {
                    continue;
                }
                let name = name.str().unwrap_or_default();
                match out.iter_mut().find(|(k, _)| *k == name) {
                    Some(slot) => slot.1 = value,
                    None => out.push((name, value)),
                }
            }
        }
    }
    out
}

/// One plain variable by name.
pub fn variable<'a>(vars: &[(String, &'a Value)], name: &str) -> Option<&'a Value> {
    vars.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
}

/// The indices of a state's enabled actions of one kind, in authored order.
pub fn enabled_indices(st: &Value, action: &str) -> Vec<usize> {
    enabled(st)
        .into_iter()
        .filter(|(name, _)| name == action)
        .map(|(_, i)| i)
        .collect()
}

/// `enabled_actions`: every enabled instance of one action in a state, as decoded fields.
pub fn enabled_actions(st: &Value, action: &str) -> Result<Vec<Fields>> {
    let data = get(st, "actionData")?;
    enabled_indices(st, action)
        .into_iter()
        .map(|i| action_fields(data, i, true).map_err(|e| e.to_string()))
        .collect()
}

/// `one_action`: the single enabled instance of an action.
pub fn one_action(who: &str, st: &Value, action: &str) -> Result<Fields> {
    let mut found = enabled_actions(st, action)?;
    if found.len() != 1 {
        return err(format!(
            "{who} {} no longer carries one enabled {action}",
            name_of(st)
        ));
    }
    Ok(found.remove(0))
}

fn name_of(st: &Value) -> String {
    st.get("name").and_then(Value::str).unwrap_or_default()
}

/// `_same(value, want)`.
fn same(value: Option<&Value>, want: &Pin) -> bool {
    match want {
        Pin::F(w) => value.is_some_and(|v| match v {
            Value::Int(_) | Value::UInt(_) | Value::F32(_) | Value::F64(_) => v
                .float()
                .or(v.int().map(|i| i as f64))
                .is_some_and(|x| (x - w).abs() <= 1e-6),
            _ => false,
        }),
        Pin::I(w) => value.is_some_and(|v| v.py_eq(&Value::Int(*w))),
        Pin::B(w) => value.is_some_and(|v| v.py_eq(&Value::Bool(*w))),
        Pin::S(w) => value.is_some_and(|v| v.py_eq(&Value::Str(w.as_bytes().to_vec()))),
        Pin::Var(_) => false,
    }
}

/// `check_action`: `expected` maps a field to its literal, or to a variable binding.
pub fn check_action(
    who: &str,
    st: &Value,
    action: &str,
    expected: &[(&str, Pin)],
) -> Result<Fields> {
    let fields = one_action(who, st, action)?;
    for (key, want) in expected {
        let value = hk_unity::playmaker::field(&fields, key);
        let ok = match want {
            Pin::Var(name) => value.is_some_and(|v| {
                v.is_map()
                    && v.get("useVariable").is_some_and(Value::truthy)
                    && v.get("name")
                        .is_some_and(|n| n.py_eq(&Value::Str(name.as_bytes().to_vec())))
            }),
            _ => match value {
                Some(v) if v.is_map() => {
                    !v.get("useVariable").is_some_and(Value::truthy) && same(v.get("value"), want)
                }
                other => same(other, want),
            },
        };
        if !ok {
            return err(format!(
                "unsupported {who} parameter: {}/{action}.{key}",
                name_of(st)
            ));
        }
    }
    Ok(fields)
}

/// `check_transitions`: each named state's transitions, and the FSM's global ones.
pub fn check_transitions(
    who: &str,
    fsm: &Value,
    expected: &[(&str, &[(&str, &str)])],
    globals: &[(&str, &str)],
) -> Result<()> {
    let pairs = |list: &[Value]| -> Result<Vec<(String, String)>> {
        list.iter()
            .map(|t| {
                Ok((
                    get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(),
                    get(t, "toState")?.str().unwrap_or_default(),
                ))
            })
            .collect()
    };
    let want = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    };
    let name = get(fsm, "name")?.str().unwrap_or_default();
    let actual = pairs(
        fsm.get("globalTransitions")
            .and_then(Value::list)
            .unwrap_or(&[]),
    )?;
    if actual != want(globals) {
        return err(format!("unsupported {who} global transitions: {name}"));
    }
    for (state_name, transitions) in expected {
        let st = state(fsm, state_name)?;
        let actual = pairs(get(st, "transitions")?.list().unwrap_or(&[]))?;
        if actual != want(transitions) {
            return err(format!(
                "unsupported {who} transitions: {name}/{state_name}"
            ));
        }
    }
    Ok(())
}

fn ints(v: &Value) -> Vec<i64> {
    v.list()
        .map(|l| l.iter().map(|x| x.int().unwrap_or(0)).collect())
        .unwrap_or_default()
}

/// A parameter's (kind, bytes).
pub type Raw = (i64, Vec<u8>);
/// An action's raw parameters by name.
pub type RawParams = Vec<(String, Raw)>;

/// `raw_params`: an action's parameters as (kind, bytes) by name, for the kinds
/// `focus.action_fields` leaves alone (vectors).
pub fn raw_params(data: &Value, index: usize) -> Result<RawParams> {
    let starts = ints(get(data, "actionStartIndex")?);
    let names = get(data, "paramName")?.list().unwrap_or(&[]).to_vec();
    let kinds = ints(get(data, "paramDataType")?);
    let positions = ints(get(data, "paramDataPos")?);
    let sizes = ints(get(data, "paramByteDataSize")?);
    let bytes: Vec<u8> = ints(get(data, "byteData")?)
        .into_iter()
        .map(|b| b as u8)
        .collect();
    let count = get(data, "actionNames")?.list().unwrap_or(&[]).len();
    let start = *starts.get(index).ok_or("action index out of range")? as usize;
    let end = if index + 1 < count {
        *starts.get(index + 1).ok_or("action index out of range")? as usize
    } else {
        names.len()
    };
    let mut out: RawParams = Vec::new();
    for i in start..end {
        let (Some(&pos), Some(&size), Some(&kind)) = (positions.get(i), sizes.get(i), kinds.get(i))
        else {
            return err("parameter index out of range");
        };
        let lo = (pos.max(0) as usize).min(bytes.len());
        let hi = ((pos + size).max(0) as usize).min(bytes.len()).max(lo);
        let name = names
            .get(i)
            .and_then(Value::str)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| i.to_string());
        let value = (kind, bytes[lo..hi].to_vec());
        match out.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => out.push((name, value)),
        }
    }
    Ok(out)
}

fn raw<'a>(params: &'a [(String, Raw)], field: &str) -> Result<&'a Raw> {
    params
        .iter()
        .find(|(k, _)| k == field)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("missing parameter {field}"))
}

/// `vector2`: a serialized FsmVector2 parameter, x, y, useVariable, then the variable name.
pub fn vector2(raw: &Raw) -> Result<(f64, f64, bool, String)> {
    let (kind, payload) = raw;
    if *kind != 37 || payload.len() < 9 {
        return err("not a compact Vector2 parameter");
    }
    let x = f32::from_le_bytes(payload[0..4].try_into().unwrap()) as f64;
    let y = f32::from_le_bytes(payload[4..8].try_into().unwrap()) as f64;
    let name = std::str::from_utf8(&payload[9..]).map_err(|_| "Vector2 name is not UTF-8")?;
    Ok((x, y, payload[8] != 0, name.to_string()))
}

/// What a pinned Vector2 field must be.
pub enum VecPin {
    Lit(f64, f64),
    Var(&'static str),
}

/// The `nth` enabled instance of an action, as its raw parameters.
fn nth_params(who: &str, st: &Value, action: &str, nth: usize) -> Result<RawParams> {
    let found = enabled_indices(st, action);
    let Some(&index) = found.get(nth) else {
        return err(format!(
            "{who} {} no longer carries {action} #{nth}",
            name_of(st)
        ));
    };
    raw_params(get(st, "actionData")?, index)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

/// `check_vectors`: the `nth` enabled instance of an action must carry these Vector2 literals
/// or variable bindings.
pub fn check_vectors(
    who: &str,
    st: &Value,
    action: &str,
    nth: usize,
    expected: &[(&str, VecPin)],
) -> Result<()> {
    let params = nth_params(who, st, action, nth)?;
    for (field, want) in expected {
        let (x, y, used, name) = vector2(raw(&params, field)?)?;
        let ok = match want {
            VecPin::Var(n) => used && name == *n,
            VecPin::Lit(wx, wy) => !used && close(x, *wx) && close(y, *wy),
        };
        if !ok {
            return err(format!(
                "unsupported {who} parameter: {}/{action}.{field}",
                name_of(st)
            ));
        }
    }
    Ok(())
}

/// `check_enums`: the `nth` enabled instance of an action must carry these enum values.
pub fn check_enums(
    who: &str,
    st: &Value,
    action: &str,
    nth: usize,
    expected: &[(&str, i32)],
) -> Result<()> {
    let params = nth_params(who, st, action, nth)?;
    for (field, want) in expected {
        let (kind, payload) = raw(&params, field)?;
        let ok = *kind == 7
            && payload.len() >= 4
            && i32::from_le_bytes(payload[0..4].try_into().unwrap()) == *want;
        if !ok {
            return err(format!(
                "unsupported {who} parameter: {}/{action}.{field}",
                name_of(st)
            ));
        }
    }
    Ok(())
}

/// `fingerprint`: `runner::fsm_fingerprint`, tolerant of the parameter kinds that name their
/// variable differently (function calls, variable references). Equal to it wherever that one succeeds.
pub fn fingerprint(fsm: &Value) -> Result<String> {
    fingerprint_with(fsm, |v| {
        let name = v.get("name").or_else(|| v.get("variableName"));
        format!(
            "VAR:{}",
            match name {
                None => String::new(),
                Some(Value::Str(s)) => String::from_utf8_lossy(s).into_owned(),
                Some(other) => py_value_repr(other),
            }
        )
    })
}
