//! Strict compiled subset of authored inverse reveal-mask FSMs (host/reveal_masks.py):
//! which scene masks the port can run, the shapes it admits and why it refuses the rest.
//!
//! The recognised shapes are an ordinary reversible reveal, an inverse one, the one-way
//! secret mask, the two-state HIT/UNCOVER mask and Crossroads_09's floor fade.

use crate::breakables::{
    descendants, file_of, jb, jf, jget, ji, jl, jset, jstr, k, kf, ki, kl, ks, local_id, pystr,
    var, Comp,
};
use crate::common::{collider_polygons, components, err, Result};
use crate::cook_audio::{jobj, js, u};
use crate::false_knight::fsm_digest;
use crate::music::value_json;
use crate::pyjson::Json;
use crate::secret_breaks::secret_drivers;
use hk_unity::playmaker::{action_fields, Fields};
use hk_unity::scene::Scene;
use hk_unity::Value;
use std::collections::{BTreeSet, HashMap};

pub const MAX_CONTROLLERS: usize = 16;
pub const SECRET_CANONICAL_NAME: &str = "secret mask";
pub const SECRET_PLACEMENT_VARIABLES: [&str; 1] = ["Play Sound"];
pub const SECRET_SHA256: &str = "12e1a80b34d99f72c9552401c98ae76ced219aa7151e865f834d1d2f3103aa90";
/// `Activate` is the already-revealed path: a 0.1s tween straight to clear.
pub const SECRET_ACTIVATED_SECONDS: f64 = 0.1;
const NO_COLLIDER: &str =
    "no trigger collider on the owner, so the authored Trigger2dEvent can never fire";
pub const TWO_STATE_EVENTS: [&str; 2] = ["HIT", "UNCOVER"];

/// The (event, destination) transitions of every state, in authored order.
type Transitions = Vec<(String, Vec<(String, String)>)>;
/// Each state's enabled actions: (short action name, decoded fields).
type States = Vec<(String, Vec<(String, Fields)>)>;
type Vars = Vec<(String, Value)>;

/// Whether `transitions` is exactly `expected` (dict equality, key order ignored).
fn transitions_eq(t: &Transitions, expected: &[(&str, &[(&str, &str)])]) -> bool {
    t.len() == expected.len()
        && expected.iter().all(|(name, list)| {
            t.iter().find(|e| e.0 == *name).is_some_and(|e| {
                e.1.len() == list.len()
                    && e.1
                        .iter()
                        .zip(list.iter())
                        .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
            })
        })
}

fn states_get<'a>(states: &'a States, name: &str) -> Result<&'a Vec<(String, Fields)>> {
    states
        .iter()
        .find(|e| e.0 == name)
        .map(|e| &e.1)
        .ok_or_else(|| format!("'{name}'"))
}

fn names_of(actions: &[(String, Fields)]) -> Vec<&str> {
    actions.iter().map(|a| a.0.as_str()).collect()
}

fn fk<'a>(fields: &'a Fields, name: &str) -> Result<&'a Value> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("'{name}'"))
}

/// `scalar(fields, name, variables)`.
fn scalar(fields: &Fields, name: &str, variables: Option<&Vars>) -> Result<Value> {
    let value = fk(fields, name)?;
    if k(value, "useVariable")?.truthy() {
        let vn = ks(value, "name")?;
        return match variables.and_then(|v| var(v, &vn)) {
            Some(v) => Ok(v.clone()),
            None => err(format!("unknown FSM variable {name}")),
        };
    }
    Ok(k(value, "value")?.clone())
}

fn is_num(v: &Value, x: f64) -> bool {
    v.py_eq(&Value::F64(x))
}

fn is_str(v: &Value, s: &str) -> bool {
    v.py_eq(&Value::Str(s.as_bytes().to_vec()))
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(_) | Value::Int(_) | Value::UInt(_) | Value::F32(_) | Value::F64(_) => {
            v.float().or(v.int().map(|i| i as f64))
        }
        _ => None,
    }
}

/// `isinstance(x, (int, float)) and finite and 0 < x <= 10`.
fn fade_seconds(v: Option<&Value>) -> Option<f64> {
    let d = num(v?)?;
    (d.is_finite() && d > 0.0 && d <= 10.0).then_some(d)
}

fn put(fields: &mut Fields, key: String, value: Value) {
    match fields.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => fields.push((key, value)),
    }
}

/// `actions(data)`: a state's enabled actions with their decoded fields.
pub fn actions(data: &Value) -> Result<Vec<(String, Fields)>> {
    let mut result = Vec::new();
    let names = kl(data, "actionNames")?;
    let enabled = kl(data, "actionEnabled")?;
    let int_list = |key: &str| -> Result<Vec<i64>> {
        Ok(kl(data, key)?
            .iter()
            .map(|v| v.int().unwrap_or(0))
            .collect())
    };
    let starts = int_list("actionStartIndex")?;
    let param_names = kl(data, "paramName")?;
    let kinds = int_list("paramDataType")?;
    let positions = int_list("paramDataPos")?;
    let sizes = int_list("paramByteDataSize")?;
    let bytes: Vec<u8> = int_list("byteData")?.into_iter().map(|b| b as u8).collect();
    for (i, name) in names.iter().enumerate() {
        if !enabled.get(i).is_some_and(Value::truthy) {
            continue;
        }
        let mut fields = u(action_fields(data, i, false))?;
        let start = starts[i] as usize;
        let end = if i + 1 < names.len() {
            starts[i + 1] as usize
        } else {
            param_names.len()
        };
        for kk in start..end {
            let (kind, pos, size) = (kinds[kk], positions[kk], sizes[kk]);
            let pname = param_names[kk].str().unwrap_or_default();
            if kind == 7 {
                if size != 4 {
                    return err("invalid enum width");
                }
                let lo = (pos.max(0) as usize).min(bytes.len());
                let raw = &bytes[lo..(lo + 4).min(bytes.len())];
                if raw.len() != 4 {
                    return err("unpack requires a buffer of 4 bytes");
                }
                put(
                    &mut fields,
                    pname,
                    Value::Int(i32::from_le_bytes(raw.try_into().unwrap()) as i64),
                );
            } else if kind == 19 {
                put(
                    &mut fields,
                    pname,
                    list_item(data, "fsmGameObjectParams", pos)?,
                );
            } else if kind == 24 {
                put(&mut fields, pname, list_item(data, "fsmObjectParams", pos)?);
            } else if ![1, 15, 16, 17, 18, 20, 23].contains(&kind) {
                return err("unsupported action parameter encoding");
            }
        }
        let short = name.str().unwrap_or_default();
        result.push((short.rsplit('.').next().unwrap_or("").to_string(), fields));
    }
    Ok(result)
}

fn list_item(data: &Value, key: &str, pos: i64) -> Result<Value> {
    let list = kl(data, key)?;
    let i = if pos < 0 {
        list.len() as i64 + pos
    } else {
        pos
    };
    list.get(i as usize)
        .cloned()
        .ok_or_else(|| "list index out of range".to_string())
}

/// `validate_fade(f, time, variables, loop_finish)`.
fn validate_fade(f: &Fields, time: f64, variables: &Vars, loop_finish: bool) -> Result<()> {
    let t = scalar(f, "time", Some(variables))?;
    let bad = (num(&t).map_or(true, |t| (t - time).abs() > 1e-5))
        || !is_num(&scalar(f, "delay", None)?, 0.0)
        || !scalar(f, "includeChildren", None)?.truthy()
        || !is_str(&scalar(f, "namedValueColor", None)?, "_Color")
        || !is_num(fk(f, "easeType")?, 21.0)
        || !is_num(fk(f, "loopType")?, 0.0)
        || scalar(f, "realTime", None)?.truthy()
        || !scalar(f, "stopOnExit", None)?.truthy()
        || !scalar(f, "loopDontFinish", None)?.py_eq(&Value::Bool(loop_finish))
        || fk(f, "startEvent")?.truthy()
        || fk(f, "finishEvent")?.truthy();
    if bad {
        return err("unsupported tween semantics");
    }
    Ok(())
}

fn ticks(duration: f64) -> i64 {
    (duration * 60.0 - 1e-5).ceil() as i64
}

fn fades_of<'a>(states: &'a States, state: &str) -> Result<Vec<&'a Fields>> {
    Ok(states_get(states, state)?
        .iter()
        .filter(|(n, _)| n == "iTweenFadeTo")
        .map(|(_, f)| f)
        .collect())
}

fn first_of<'a>(states: &'a States, state: &str, action: &str) -> Result<&'a Fields> {
    states_get(states, state)?
        .iter()
        .find(|(n, _)| n == action)
        .map(|(_, f)| f)
        .ok_or_else(String::new)
}

fn at<'a>(states: &'a States, state: &str, index: usize) -> Result<&'a Fields> {
    states_get(states, state)?
        .get(index)
        .map(|(_, f)| f)
        .ok_or_else(|| "list index out of range".to_string())
}

fn check_sequences(
    states: &States,
    expected: &[(&str, &[&str])],
    what: &str,
    set_msg: &str,
) -> Result<()> {
    let names: BTreeSet<&str> = states.iter().map(|e| e.0.as_str()).collect();
    let want: BTreeSet<&str> = expected.iter().map(|e| e.0).collect();
    if names != want {
        return err(set_msg);
    }
    for (state, list) in expected {
        if names_of(states_get(states, state)?) != *list {
            return err(format!("{what} {state}"));
        }
    }
    Ok(())
}

fn fade_time(variables: &Vars) -> Result<f64> {
    match fade_seconds(var(variables, "Fade Time")) {
        Some(d) => Ok(d),
        None => err("invalid Fade Time"),
    }
}

fn owner_option(f: &Fields) -> Result<i64> {
    ki(fk(f, "gameObject")?, "ownerOption")
}

fn trigger_ok(trigger: &Fields, kind: i64, event: &str) -> Result<bool> {
    Ok(is_num(fk(trigger, "trigger")?, kind as f64)
        && is_str(fk(trigger, "sendEvent")?, event)
        && is_str(&scalar(trigger, "collideTag", None)?, "Player")
        && is_str(&scalar(trigger, "collideLayer", None)?, ""))
}

fn bool_latch_ok(b: &Fields, name: &str) -> Result<bool> {
    let v = fk(b, "boolVariable")?;
    Ok(k(v, "useVariable")?.truthy()
        && ks(v, "name")? == name
        && scalar(b, "boolValue", None)?.py_eq(&Value::Bool(true))
        && !fk(b, "everyFrame")?.truthy())
}

/// `verify_states(states, variables)`: the inverse reveal.
pub fn verify_states(states: &States, variables: &Vars) -> Result<i64> {
    check_sequences(
        states,
        &[
            ("Idle", &["Trigger2dEvent", "iTweenFadeTo", "iTweenFadeTo"]),
            (
                "Fade Out",
                &[
                    "iTweenFadeTo",
                    "iTweenFadeTo",
                    "SetBoolValue",
                    "Trigger2dEvent",
                ],
            ),
            (
                "Fade In",
                &[
                    "iTweenFadeTo",
                    "iTweenFadeTo",
                    "SetBoolValue",
                    "Trigger2dEvent",
                ],
            ),
            (
                "Pause",
                &["FindChild", "iTweenFadeTo", "WaitForHeroInPosition", "Wait"],
            ),
            ("Hero Leave", &[]),
        ],
        "unsupported enabled action sequence",
        "unsupported reveal states",
    )?;
    let duration = fade_time(variables)?;
    for (state, alpha) in [("Idle", 0.0), ("Fade Out", 1.0), ("Fade In", 0.0)] {
        for (index, f) in fades_of(states, state)?.into_iter().enumerate() {
            let owner = fk(f, "gameObject")?;
            let target = k(owner, "gameObject")?;
            if ki(owner, "ownerOption")? != index as i64
                || (index == 1
                    && (!k(target, "useVariable")?.truthy()
                        || ks(target, "name")? != "Inverse Mask"))
            {
                return err("unsupported fade target");
            }
            let want = if index == 0 { alpha } else { 1.0 - alpha };
            if !is_num(&scalar(f, "alpha", None)?, want) {
                return err("unexpected fade alpha");
            }
            validate_fade(
                f,
                if state == "Idle" { 0.01 } else { duration },
                variables,
                true,
            )?;
        }
        let trigger = first_of(states, state, "Trigger2dEvent")?;
        let fade_out = state == "Fade Out";
        if !trigger_ok(
            trigger,
            if fade_out { 2 } else { 1 },
            if fade_out { "COVER" } else { "UNCOVER" },
        )? {
            return err("unsupported trigger semantics");
        }
        if state != "Idle" {
            let b = first_of(states, state, "SetBoolValue")?;
            if !bool_latch_ok(b, "Activated")? {
                return err("unsupported persistent bookkeeping");
            }
        }
    }
    let find = at(states, "Pause", 0)?;
    let fade = at(states, "Pause", 1)?;
    let wait_hero = at(states, "Pause", 2)?;
    let wait = at(states, "Pause", 3)?;
    let store = fk(find, "storeResult")?;
    if owner_option(find)? != 0
        || !is_str(&scalar(find, "childName", None)?, "Inverse Mask")
        || !k(store, "useVariable")?.truthy()
        || ks(store, "name")? != "Inverse Mask"
    {
        return err("unsupported inverse lookup");
    }
    let fgo = fk(fade, "gameObject")?;
    let inner = k(fgo, "gameObject")?;
    if ki(fgo, "ownerOption")? != 1
        || ks(inner, "name")? != "Inverse Mask"
        || !k(inner, "useVariable")?.truthy()
        || !is_num(&scalar(fade, "alpha", None)?, 0.0)
    {
        return err("unsupported pause target");
    }
    validate_fade(fade, 0.0, variables, false)?;
    let pause_bad = !is_str(fk(wait_hero, "sendEvent")?, "FINISHED")
        || scalar(wait_hero, "skipIfAlreadyPositioned", None)?.truthy()
        || !is_str(fk(wait, "finishEvent")?, "FINISHED")
        || num(&scalar(wait, "time", None)?).map_or(true, |t| (t - 2.0).abs() > 1e-5)
        || fk(wait, "realTime")?.truthy();
    if pause_bad {
        return err("unsupported pause wait");
    }
    Ok(ticks(duration))
}

/// `verify_ordinary_states(states, variables)`: the reversible reveal.
pub fn verify_ordinary_states(states: &States, variables: &Vars) -> Result<i64> {
    check_sequences(
        states,
        &[
            ("Idle", &["Trigger2dEvent"]),
            (
                "Fade Out",
                &["iTweenFadeTo", "SetBoolValue", "Trigger2dEvent"],
            ),
            (
                "Fade In",
                &["iTweenFadeTo", "SetBoolValue", "Trigger2dEvent"],
            ),
            ("Pause", &["WaitForHeroInPosition", "Wait"]),
            ("Hero Leave", &[]),
        ],
        "unsupported ordinary action sequence",
        "unsupported ordinary reveal states",
    )?;
    let duration = fade_time(variables)?;
    for state in ["Idle", "Fade Out", "Fade In"] {
        let trigger = first_of(states, state, "Trigger2dEvent")?;
        let fade_out = state == "Fade Out";
        if !trigger_ok(
            trigger,
            if fade_out { 2 } else { 1 },
            if fade_out { "COVER" } else { "UNCOVER" },
        )? {
            return err("unsupported ordinary trigger semantics");
        }
        if state != "Idle" {
            let fade = at(states, state, 0)?;
            if owner_option(fade)? != 0
                || !is_num(
                    &scalar(fade, "alpha", None)?,
                    if fade_out { 0.0 } else { 1.0 },
                )
            {
                return err("unsupported ordinary fade target");
            }
            validate_fade(fade, duration, variables, true)?;
            let b = at(states, state, 1)?;
            if !bool_latch_ok(b, "Activated")? {
                return err("unsupported persistent bookkeeping");
            }
        }
    }
    let hero = at(states, "Pause", 0)?;
    let wait = at(states, "Pause", 1)?;
    if !is_str(fk(hero, "sendEvent")?, "FINISHED")
        || scalar(hero, "skipIfAlreadyPositioned", None)?.truthy()
        || !is_str(fk(wait, "finishEvent")?, "FINISHED")
        || num(&scalar(wait, "time", None)?).map_or(true, |t| (t - 1.0).abs() > 1e-5)
        || fk(wait, "realTime")?.truthy()
    {
        return err("unsupported ordinary pause wait");
    }
    Ok(ticks(duration))
}

/// `verify_secret_states(states, variables)`: the one-way secret reveal.
pub fn verify_secret_states(states: &States, variables: &Vars) -> Result<(i64, bool)> {
    check_sequences(
        states,
        &[
            ("Pause", &["WaitForHeroInPosition"]),
            ("Idle", &["BoolTest"]),
            ("Idle Stay", &["Trigger2dEvent"]),
            ("Fade", &["iTweenFadeTo", "SetBoolValue", "BoolTest"]),
            ("Activate", &["iTweenFadeTo"]),
            ("Sound", &["AudioPlayerOneShotSingle"]),
        ],
        "unsupported secret action sequence",
        "unsupported secret reveal states",
    )?;
    let hero = at(states, "Pause", 0)?;
    if !is_str(fk(hero, "sendEvent")?, "FINISHED")
        || scalar(hero, "skipIfAlreadyPositioned", None)?.truthy()
    {
        return err("unsupported secret pause wait");
    }
    let gate = at(states, "Idle", 0)?;
    let bv = fk(gate, "boolVariable")?;
    if !k(bv, "useVariable")?.truthy()
        || ks(bv, "name")? != "Activated"
        || !is_str(fk(gate, "isTrue")?, "ACTIVATE")
        || fk(gate, "isFalse")?.truthy()
        || fk(gate, "everyFrame")?.truthy()
    {
        return err("unsupported secret activation test");
    }
    if !crate::breakables::is_int_in(var(variables, "Activated"), &[0]) {
        return err("secret mask is serialized already revealed");
    }
    let trigger = at(states, "Idle Stay", 0)?;
    if !trigger_ok(trigger, 1, "UNCOVER")? {
        return err("unsupported secret trigger semantics");
    }
    let fade = at(states, "Fade", 0)?;
    if owner_option(fade)? != 0 || !is_num(&scalar(fade, "alpha", None)?, 0.0) {
        return err("unsupported secret fade target");
    }
    let seconds = scalar(fade, "time", Some(variables))?;
    let Some(duration) = fade_seconds(Some(&seconds)) else {
        return err("invalid secret fade time");
    };
    validate_fade(fade, duration, variables, true)?;
    let latch = at(states, "Fade", 1)?;
    if !bool_latch_ok(latch, "Activated")
        .map_err(|_| "unsupported secret persistent bookkeeping".to_string())?
    {
        return err("unsupported secret persistent bookkeeping");
    }
    let chime = at(states, "Fade", 2)?;
    let cv = fk(chime, "boolVariable")?;
    if !k(cv, "useVariable")?.truthy()
        || ks(cv, "name")? != "Play Sound"
        || !is_str(fk(chime, "isTrue")?, "SOUND")
        || fk(chime, "isFalse")?.truthy()
        || fk(chime, "everyFrame")?.truthy()
    {
        return err("unsupported secret sound test");
    }
    let revealed = at(states, "Activate", 0)?;
    if owner_option(revealed)? != 0 || !is_num(&scalar(revealed, "alpha", None)?, 0.0) {
        return err("unsupported secret activate target");
    }
    validate_fade(revealed, SECRET_ACTIVATED_SECONDS, variables, true)?;
    let sound = var(variables, "Play Sound");
    if !crate::breakables::is_int_in(sound, &[0, 1]) {
        return err("secret Play Sound is not a serialized boolean");
    }
    Ok((ticks(duration), sound.is_some_and(Value::truthy)))
}

/// `no_collider_reason(sc, gid)`: why an owner with no Collider2D is refused.
pub fn no_collider_reason(sc: &Scene, gid: i64) -> Result<String> {
    let drivers = crate::breakables::uncover_drivers(sc)?;
    let Some((_, driver)) = drivers.iter().find(|d| d.0 == gid) else {
        return Ok(NO_COLLIDER.to_string());
    };
    let g = |key: &str| jget(driver, key).map(jstr).unwrap_or_default();
    Ok(format!(
        "{NO_COLLIDER}; uncovered instead by the hidden wall {} ({}, definition {}), which has to be admitted first",
        g("name"),
        g("source"),
        g("definition")
    ))
}

/// `_fade_action(fields, variables)`: one iTweenFadeTo of the owner and its children to 0.
fn fade_action(fields: &Fields, variables: Option<&Vars>) -> Result<f64> {
    if owner_option(fields)? != 0 || !is_num(&scalar(fields, "alpha", None)?, 0.0) {
        return err("unsupported fade target");
    }
    let seconds = scalar(fields, "time", variables)?;
    let Some(seconds) = fade_seconds(Some(&seconds)) else {
        return err("invalid fade time");
    };
    if !is_num(&scalar(fields, "delay", None)?, 0.0)
        || !scalar(fields, "includeChildren", None)?.truthy()
        || !is_num(fk(fields, "easeType")?, 21.0)
        || !is_num(fk(fields, "loopType")?, 0.0)
        || scalar(fields, "realTime", None)?.truthy()
        || !is_str(&scalar(fields, "namedValueColor", None)?, "_Color")
    {
        return err("unsupported tween semantics");
    }
    Ok(seconds)
}

/// `two_state(fsm, states, transitions)`: (event, seconds, own trigger) of a two-state unmasker.
fn two_state(
    fsm: &Value,
    states: &States,
    transitions: &Transitions,
) -> Result<Option<(String, f64, bool)>> {
    let names: BTreeSet<&str> = states.iter().map(|e| e.0.as_str()).collect();
    let fade_has_transitions = transitions
        .iter()
        .find(|e| e.0 == "Fade")
        .is_some_and(|e| !e.1.is_empty());
    if ks(fsm, "startState")? != "Idle"
        || names != BTreeSet::from(["Idle", "Fade"])
        || fade_has_transitions
    {
        return Ok(None);
    }
    let idle: &[(String, String)] = transitions
        .iter()
        .find(|e| e.0 == "Idle")
        .map_or(&[], |e| &e.1);
    if idle.len() != 1 || idle[0].1 != "Fade" || !TWO_STATE_EVENTS.contains(&idle[0].0.as_str()) {
        return Ok(None);
    }
    let event = idle[0].0.clone();
    if names_of(states_get(states, "Fade")?) != ["iTweenFadeTo"] {
        return err("two-state fade is not one iTweenFadeTo");
    }
    let seconds = fade_action(at(states, "Fade", 0)?, None)?;
    let actions = names_of(states_get(states, "Idle")?);
    if actions.is_empty() {
        return Ok(Some((event, seconds, false)));
    }
    if actions != ["Trigger2dEvent"] {
        return err("two-state Idle is not one Trigger2dEvent");
    }
    let trigger = at(states, "Idle", 0)?;
    if !(is_num(fk(trigger, "trigger")?, 0.0)
        && is_str(fk(trigger, "sendEvent")?, &event)
        && is_str(&scalar(trigger, "collideTag", None)?, "Player")
        && is_str(&scalar(trigger, "collideLayer", None)?, ""))
    {
        return err("unsupported two-state trigger semantics");
    }
    Ok(Some((event, seconds, true)))
}

/// `floor_fade(fsm, states, transitions)`: the `fade` FSM's seconds, or None.
fn floor_fade(fsm: &Value, states: &States, transitions: &Transitions) -> Result<Option<f64>> {
    let names: BTreeSet<&str> = states.iter().map(|e| e.0.as_str()).collect();
    if ks(fsm, "startState")? != "Pause" || names != BTreeSet::from(["Pause", "Idle", "Fade"]) {
        return Ok(None);
    }
    if !transitions_eq(
        transitions,
        &[
            ("Pause", &[("FINISHED", "Idle")]),
            ("Idle", &[("HIT", "Fade")]),
            ("Fade", &[]),
        ],
    ) {
        return Ok(None);
    }
    if names_of(states_get(states, "Pause")?) != ["NextFrameEvent"]
        || names_of(states_get(states, "Idle")?) != ["BoolTest"]
        || names_of(states_get(states, "Fade")?) != ["iTweenFadeTo", "SetBoolValue"]
    {
        return err("unsupported floor mask fade actions");
    }
    let gate = at(states, "Idle", 0)?;
    let bv = fk(gate, "boolVariable")?;
    if ks(bv, "name")? != "Activated"
        || !is_str(fk(gate, "isTrue")?, "HIT")
        || fk(gate, "isFalse")?.truthy()
        || fk(gate, "everyFrame")?.truthy()
    {
        return err("unsupported floor mask activation test");
    }
    Ok(Some(fade_action(at(states, "Fade", 0)?, None)?))
}

/// `_breakable_receivers(sc)`: GameObjects a C# Breakable forwards HIT to.
fn breakable_receivers(sc: &Scene) -> BTreeSet<i64> {
    sc.objects
        .iter()
        .filter(|o| o.typename == "Breakable")
        .filter_map(|o| {
            o.tree
                .get("hitEventReciever")
                .and_then(|r| r.get("m_PathID"))
                .and_then(Value::int)
        })
        .filter(|id| *id != 0)
        .collect()
}

/// The admitted and refused masks of one scene.
pub struct RevealMasks {
    pub controllers: Vec<Json>,
    pub unsupported: Vec<Json>,
}

fn variables_all(fsm: &Value) -> Result<Vars> {
    let mut out: Vars = Vec::new();
    if let Value::Map(groups) = k(fsm, "variables")? {
        for (_, group) in groups {
            let Some(items) = group.list() else { continue };
            for v in items {
                if !matches!(v, Value::Map(_)) {
                    continue;
                }
                let (Some(name), Some(value)) = (v.get("name"), v.get("value")) else {
                    continue;
                };
                let name = name.str().unwrap_or_default();
                match out.iter_mut().find(|e| e.0 == name) {
                    Some(slot) => slot.1 = value.clone(),
                    None => out.push((name, value.clone())),
                }
            }
        }
    }
    Ok(out)
}

fn with_name(fsm: &Value, name: &str) -> Value {
    match fsm {
        Value::Map(f) => {
            let mut f = f.clone();
            match f.iter_mut().find(|(n, _)| &**n == "name") {
                Some(slot) => slot.1 = Value::Str(name.as_bytes().to_vec()),
                None => f.push(("name".into(), Value::Str(name.as_bytes().to_vec()))),
            }
            Value::Map(f)
        }
        other => other.clone(),
    }
}

fn states_json(states: &States) -> Json {
    Json::Obj(
        states
            .iter()
            .map(|(name, actions)| {
                (
                    name.clone(),
                    jl(actions
                        .iter()
                        .map(|(n, fields)| {
                            jl(vec![
                                js(n),
                                Json::Obj(
                                    fields
                                        .iter()
                                        .map(|(fname, v)| (fname.clone(), value_json(v)))
                                        .collect(),
                                ),
                            ])
                        })
                        .collect()),
                )
            })
            .collect(),
    )
}

/// `reveal_mask_sources(sc)`.
pub fn reveal_mask_sources(sc: &Scene) -> Result<RevealMasks> {
    let file = file_of(sc);
    let mut records: Vec<Json> = Vec::new();
    let mut unsupported: Vec<Json> = Vec::new();
    let drivers = secret_drivers(sc)?;
    let receivers = breakable_receivers(sc);
    let mut objects: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().collect();
    objects.sort_by_key(|o| o.id);
    for o in objects {
        if o.typename != "PlayMakerFSM" {
            continue;
        }
        let sid = o.id;
        let tree = &o.tree;
        let fsm = k(tree, "fsm")?;
        let fsm_states = kl(fsm, "states")?;
        let mut names: Vec<String> = Vec::new();
        for st in fsm_states {
            for a in kl(k(st, "actionData")?, "actionNames")? {
                names.push(a.str().unwrap_or_default());
            }
        }
        if !names.iter().any(|n| n.ends_with("iTweenFadeTo")) {
            continue;
        }
        let mut events: BTreeSet<String> = BTreeSet::new();
        for st in fsm_states {
            for t in kl(st, "transitions")? {
                events.insert(ks(k(t, "fsmEvent")?, "name")?);
            }
        }
        if !names.iter().any(|n| n.ends_with("Trigger2dEvent"))
            && !TWO_STATE_EVENTS.iter().any(|e| events.contains(*e))
        {
            continue;
        }
        let gid = ki(k(tree, "m_GameObject")?, "m_PathID")?;
        if !k(tree, "m_Enabled")?.truthy() || !sc.active(gid) {
            continue;
        }
        let one = (|| -> Result<Json> {
            let mut transitions: Transitions = Vec::new();
            for st in fsm_states {
                let mut list = Vec::new();
                for t in kl(st, "transitions")? {
                    list.push((ks(k(t, "fsmEvent")?, "name")?, ks(t, "toState")?));
                }
                transitions.push((ks(st, "name")?, list));
            }
            let mut globals: Vec<(String, String)> = Vec::new();
            for t in kl(fsm, "globalTransitions")? {
                globals.push((ks(k(t, "fsmEvent")?, "name")?, ks(t, "toState")?));
            }
            let variables = variables_all(fsm)?;
            let mut states: States = Vec::new();
            for st in fsm_states {
                let a = actions(k(st, "actionData")?)?;
                let name = ks(st, "name")?;
                match states.iter_mut().find(|e| e.0 == name) {
                    Some(slot) => slot.1 = a,
                    None => states.push((name, a)),
                }
            }
            let driver = drivers.get(&gid).copied();
            let (mut own_trigger, mut replay, mut one_way, mut plays_sound) =
                (true, false, false, false);
            let shape = if globals.is_empty() {
                two_state(fsm, &states, &transitions)?
            } else {
                None
            };
            let fade = if shape.is_some() || !globals.is_empty() {
                None
            } else {
                floor_fade(fsm, &states, &transitions)?
            };
            let (ticks_v, ordinary, kind): (i64, bool, &str);
            if let Some((event, seconds, own)) = shape {
                own_trigger = own;
                if receivers.contains(&gid) {
                    return err("a Breakable forwards HIT to this mask: it is that Breakable's own mask fade");
                }
                if !own_trigger && driver.is_none() {
                    return err(format!("nothing in this port sends {event} to this mask"));
                }
                ticks_v = ticks(seconds);
                ordinary = true;
                one_way = true;
                replay = driver.is_some();
                kind = "two_state";
            } else if let Some(seconds) = fade {
                if driver.is_none() {
                    return err("nothing in this port sends HIT to this floor mask");
                }
                ticks_v = ticks(seconds);
                ordinary = true;
                one_way = true;
                own_trigger = false;
                kind = "floor_fade";
            } else {
                if ks(fsm, "startState")? != "Pause" {
                    return err("unsupported initial state");
                }
                one_way = transitions_eq(
                    &transitions,
                    &[
                        (
                            "Idle",
                            &[
                                ("UNCOVER", "Fade"),
                                ("STAY", "Idle Stay"),
                                ("FINISHED", "Idle Stay"),
                                ("ACTIVATE", "Activate"),
                            ],
                        ),
                        ("Fade", &[("SOUND", "Sound")]),
                        ("Pause", &[("FINISHED", "Idle")]),
                        ("Idle Stay", &[("UNCOVER", "Fade")]),
                        ("Sound", &[]),
                        ("Activate", &[]),
                    ],
                );
                if one_way {
                    if !globals.is_empty() {
                        return err("unsupported global transitions");
                    }
                    let digest = fsm_digest(
                        &with_name(fsm, SECRET_CANONICAL_NAME),
                        &SECRET_PLACEMENT_VARIABLES,
                    )?;
                    if digest != SECRET_SHA256 {
                        return err(format!("unverified secret mask variant: {digest}"));
                    }
                    let (t, s) = verify_secret_states(&states, &variables)?;
                    ticks_v = t;
                    plays_sound = s;
                    ordinary = true;
                    kind = "secret";
                } else {
                    if !transitions_eq(
                        &transitions,
                        &[
                            ("Idle", &[("UNCOVER", "Fade Out")]),
                            ("Fade Out", &[("COVER", "Fade In")]),
                            ("Pause", &[("FINISHED", "Idle")]),
                            ("Fade In", &[("UNCOVER", "Fade Out")]),
                            ("Hero Leave", &[]),
                        ],
                    ) {
                        return err("unsupported transitions");
                    }
                    if globals != [("HERO LEAVE".to_string(), "Hero Leave".to_string())] {
                        return err("unsupported global transitions");
                    }
                    ordinary = states
                        .iter()
                        .find(|e| e.0 == "Idle")
                        .map_or(vec![], |e| names_of(&e.1))
                        == ["Trigger2dEvent"];
                    ticks_v = if ordinary {
                        verify_ordinary_states(&states, &variables)?
                    } else {
                        verify_states(&states, &variables)?
                    };
                    let tid = *sc.go_transform.get(&gid).ok_or("'gid'")?;
                    let children = kl(sc.transform(tid).ok_or("'tid'")?, "m_Children")?;
                    if !ordinary {
                        for c in children {
                            let ct = sc.transform(ki(c, "m_PathID")?).ok_or("child")?;
                            let cg = ki(k(ct, "m_GameObject")?, "m_PathID")?;
                            if ks(sc.go(cg).ok_or("child")?, "m_Name")? == "Inverse Mask" {
                                return err("non-null inverse target needs separate bindings");
                            }
                        }
                    }
                    kind = if ordinary { "ordinary" } else { "inverse" };
                }
            }
            let comps = components(sc, gid)?;
            let mut colliders: Vec<Comp> = Vec::new();
            for c in &comps {
                if c.1.ends_with("Collider2D") && k(c.2, "m_Enabled")?.truthy() {
                    colliders.push(*c);
                }
            }
            let mut polygons: Vec<Vec<(f64, f64)>> = Vec::new();
            if own_trigger {
                if colliders.is_empty() && driver.is_none() {
                    return err(no_collider_reason(sc, gid)?);
                }
                for (_, ctype, body) in &colliders {
                    if !["BoxCollider2D", "PolygonCollider2D"].contains(ctype)
                        || !k(body, "m_IsTrigger")?.truthy()
                    {
                        return err("unsupported trigger geometry");
                    }
                    polygons.extend(collider_polygons(sc, gid, ctype, body)?);
                }
                if polygons.len() > 8 {
                    return err("more than eight trigger paths");
                }
            }
            if driver.is_some() && !["secret", "two_state", "floor_fade"].contains(&kind) {
                return err("a secret uncovers a reversible mask");
            }
            if driver.is_some() {
                polygons.clear();
            }
            let mut renderers: Vec<Json> = Vec::new();
            let mut alphas: Vec<Json> = Vec::new();
            let mut kids: Vec<i64> = descendants(sc, gid)?;
            kids.sort_unstable();
            for child in kids {
                for (ri, rt, rd) in components(sc, child)? {
                    if rt == "SpriteRenderer"
                        && k(rd, "m_Enabled")?.truthy()
                        && sc.active(child)
                        && ki(k(rd, "m_Sprite")?, "m_PathID")? != 0
                    {
                        renderers.push(Json::Str(format!("{file}:{ri}")));
                        alphas.push(value_json(k(k(rd, "m_Color")?, "a")?));
                    }
                }
            }
            if renderers.is_empty() && !one_way {
                return err("no active renderer");
            }
            if renderers.is_empty() && polygons.is_empty() && driver.is_none() {
                return err("no active renderer and nothing fires it");
            }
            let mut persistence = Vec::new();
            for (i, t, d) in &comps {
                if *t == "PersistentBoolItem" {
                    persistence.push(jobj(vec![
                        ("source", Json::Str(format!("{file}:{i}"))),
                        ("dont_save", jb(k(d, "dontSave")?.truthy())),
                        ("semi_persistent", jb(k(d, "semiPersistent")?.truthy())),
                        ("authored", value_json(k(d, "persistentBoolData")?)),
                    ]));
                }
            }
            let saved = one_way
                && driver.is_none()
                && persistence.iter().any(|p| {
                    jget(p, "dont_save") == Some(&Json::Bool(false))
                        && jget(p, "semi_persistent") == Some(&Json::Bool(false))
                });
            let poly_json =
                |p: &Vec<(f64, f64)>| jl(p.iter().map(|q| jl(vec![jf(q.0), jf(q.1)])).collect());
            Ok(jobj(vec![
                ("source", Json::Str(format!("{file}:{sid}"))),
                ("source_id", ji(sid)),
                ("game_object", Json::Str(format!("{file}:{gid}"))),
                ("name", value_json(k(sc.go(gid).ok_or("'gid'")?, "m_Name")?)),
                (
                    "trigger_sources",
                    if polygons.is_empty() {
                        jl(vec![])
                    } else {
                        jl(colliders
                            .iter()
                            .map(|c| Json::Str(format!("{file}:{}", c.0)))
                            .collect())
                    },
                ),
                ("trigger", polygons.first().map_or(Json::Null, poly_json)),
                ("triggers", jl(polygons.iter().map(poly_json).collect())),
                ("fade_ticks", ji(ticks_v)),
                ("renderer_sources", jl(renderers)),
                ("renderer_alphas", jl(alphas)),
                ("persistence", jl(persistence)),
                ("saved", jb(saved)),
                ("initial_opacity", ji(if ordinary { 128 } else { 0 })),
                ("kind", js(kind)),
                ("one_way", jb(one_way)),
                ("plays_sound", jb(plays_sound)),
                ("driver_state", driver.map_or(Json::Null, ji)),
                ("replay_on_load", jb(replay)),
                ("states", states_json(&states)),
            ]))
        })();
        match one {
            Ok(r) => records.push(r),
            Err(e) => unsupported.push(jobj(vec![
                ("source", Json::Str(format!("{file}:{sid}"))),
                ("name", value_json(k(sc.go(gid).ok_or("'gid'")?, "m_Name")?)),
                ("error", Json::Str(e)),
            ])),
        }
    }
    if records.len() > MAX_CONTROLLERS {
        return err("reveal controller scene pool exceeds16");
    }
    for (i, r) in records.iter_mut().enumerate() {
        jset(r, "controller", ji(i as i64));
    }
    let mut saved: Vec<usize> = (0..records.len())
        .filter(|&i| jget(&records[i], "saved") == Some(&Json::Bool(true)))
        .collect();
    saved.sort_by_key(|&i| crate::breakables::jint(jget(&records[i], "source_id").unwrap()));
    for (slot, i) in saved.into_iter().enumerate() {
        jset(&mut records[i], "persist_slot", ji(slot as i64));
    }
    Ok(RevealMasks {
        controllers: records,
        unsupported,
    })
}

/// `validate_palette(raw, draw)`: renderer opacity uses a subtractive CLUT only for this exact black mask.
pub fn validate_palette(raw: &[u8], draw: usize) -> Result<()> {
    let layout = crate::region_delta::layout(raw)?;
    let [_, textures, draws, ..] = layout.counts.map(|c| c as usize);
    if draw >= draws {
        return err("reveal draw outside room");
    }
    let word = |at: usize| u16::from_le_bytes([raw[at], raw[at + 1]]) as usize;
    let texture = word(40 + textures * 16 + draw * 44);
    if texture >= textures {
        return err("reveal texture outside room");
    }
    let page = word(40 + texture * 16);
    let palette = word(40 + texture * 16 + 10);
    if page == 65535 || palette >= textures {
        return err("reveal mask must use static valid palette");
    }
    let at = layout.prefix + palette * 32;
    let words: Vec<usize> = (0..16).map(|i| word(at + i * 2)).collect();
    let mut want = vec![0usize, 1];
    want.extend([0x8000; 14]);
    if words != want {
        return err("reveal mask requires exact binary black palette");
    }
    Ok(())
}

/// `refuse_partial_one_way(by_scene, scene_hits)`: a one-way reveal takes its whole authored fade
/// with it, or it does not run. Returns scene id to {old index: new index}.
pub fn refuse_partial_one_way(
    by_scene: &mut [(i64, Json)],
    scene_hits: &HashMap<i64, Vec<(usize, String, Option<String>)>>,
) -> HashMap<i64, HashMap<usize, usize>> {
    let mut renumbered = HashMap::new();
    for (scene, entry) in by_scene.iter_mut() {
        let hits: &[(usize, String, Option<String>)] =
            scene_hits.get(scene).map_or(&[], |v| v.as_slice());
        let controllers = match jget(entry, "controllers") {
            Some(Json::List(l)) => l.clone(),
            _ => vec![],
        };
        let mut kept: Vec<Json> = Vec::new();
        let mut moved: HashMap<usize, usize> = HashMap::new();
        let mut refused: Vec<Json> = Vec::new();
        for (index, record) in controllers.into_iter().enumerate() {
            if jget(&record, "one_way") == Some(&Json::Bool(true)) {
                let drawn: BTreeSet<&str> = hits
                    .iter()
                    .filter(|h| h.0 == index)
                    .map(|h| h.1.as_str())
                    .collect();
                let sources: Vec<String> = match jget(&record, "renderer_sources") {
                    Some(Json::List(l)) => l.iter().map(jstr).collect(),
                    _ => vec![],
                };
                let absent = sources
                    .iter()
                    .filter(|s| !drawn.contains(s.as_str()))
                    .count();
                if absent > 0 {
                    refused.push(jobj(vec![
                        ("source", jget(&record, "source").cloned().unwrap_or(Json::Null)),
                        ("name", jget(&record, "name").cloned().unwrap_or(Json::Null)),
                        (
                            "error",
                            Json::Str(format!(
                                "one-way reveal cannot fade its whole authored group: {absent} of {} renderers are not cooked into any draw",
                                sources.len()
                            )),
                        ),
                    ]));
                    continue;
                }
            }
            moved.insert(index, kept.len());
            kept.push(record);
        }
        if !refused.is_empty() {
            let mut list = match jget(entry, "unsupported") {
                Some(Json::List(l)) => l.clone(),
                _ => vec![],
            };
            list.extend(refused);
            jset(entry, "unsupported", jl(list));
        }
        for (index, record) in kept.iter_mut().enumerate() {
            jset(record, "controller", ji(index as i64));
        }
        jset(entry, "controllers", jl(kept));
        renumbered.insert(*scene, moved);
    }
    renumbered
}

/// A hit found while binding: (controller, draw, renderer source, palette error).
pub type Hit = (usize, usize, String, Option<String>);

/// The Python scene-file name for error messages that quote a path.
#[allow(dead_code)]
fn quoted(s: &str) -> String {
    pystr(s)
}

/// Keeps shared helpers in use for the binding step that follows.
#[allow(dead_code)]
fn _unused() {
    let _ = (local_id, kf);
}
