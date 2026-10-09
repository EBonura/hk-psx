//! Recognize the verified Vengefly (Buzzer) `chaser` variant, the Acid Flyer
//! (Duranda) and the Mosquito (Squit) for guest admission. Ported from
//! host/vengefly.py.
//!
//! The controllers live in shared/hk-sim/src/{vengefly,acid_flyer,mosquito}.rs;
//! this module admits only placed instances whose serialized FSM parameters and
//! bodies match the audited contracts. Anything else stays unsupported.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::false_knight::fsm_digest;
use crate::pyjson::Json;
use crate::recog::Want::{B, F, I, S};
use crate::recog::{body_box, check_actions, check_assemblies, clip_is, clips_by_name, near, only, state, states, transitions, variables, variables_strict, xy};
use crate::runner::axis_aligned_bounds;
use hk_unity::playmaker::{action_fields, Fields};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

fn n(x: f64) -> Value {
    Value::F64(x)
}
fn identity() -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}
fn floats(v: &[f64]) -> Json {
    Json::List(v.iter().map(|&f| Json::Float(f)).collect())
}
fn strings(v: &[&str]) -> Json {
    Json::List(v.iter().map(|s| js(s)).collect())
}
fn unit_rotation(m: &[[f64; 4]; 4]) -> bool {
    (0..2).all(|i| (0..2).all(|j| (m[i][j] - if i == j { 1.0 } else { 0.0 }).abs() <= 1e-6))
}
fn zero3() -> Value {
    Value::Map(vec![("x".into(), n(0.0)), ("y".into(), n(0.0)), ("z".into(), n(0.0))])
}
fn zero2() -> Value {
    Value::Map(vec![("x".into(), n(0.0)), ("y".into(), n(0.0))])
}
fn body_ok(body: &Value, size: [f64; 2], offset: [f64; 2]) -> Result<bool> {
    let (s, o) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    Ok(get(body, "m_Enabled")?.truthy() && !get(body, "m_IsTrigger")?.truthy() && get(body, "m_EdgeRadius")?.float() == Some(0.0) && (0..2).all(|k| near(Some(&n(s[k])), size[k]) && near(Some(&n(o[k])), offset[k])))
}
fn clips_ok(library: &Value, table: &[(&str, usize, f64, i64, Option<i64>)]) -> Result<Option<String>> {
    let by_name = clips_by_name(library)?;
    for &(name, frames, fps, wrap, loop_start) in table {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let ok = clip_is(clip, frames, fps, wrap) && loop_start.is_none_or(|l| clip.and_then(|c| c.get("loopStart")).map_or(0, |v| v.int().unwrap_or(-1)) == l);
        if !ok {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}
fn health_is(h: &Value, hp: i64) -> bool {
    h.get("hp").and_then(Value::int) == Some(hp)
}

// --- Vengefly ------------------------------------------------------------------

const CLIPS: [(&str, usize, f64, i64, Option<i64>); 5] = [("Idle", 5, 12.0, 0, None), ("TurnToIdle", 7, 12.0, 1, None), ("Startle", 4, 12.0, 2, None), ("Chase", 4, 12.0, 0, None), ("TurnToFly", 6, 12.0, 1, None)];
const BODY_SIZE: [f64; 2] = [1.25, 0.625];
const BODY_OFFSET: [f64; 2] = [0.0, -0.1875];
const ALERT_RADIUS: f64 = 0.5 * 15.608528137207031;
#[rustfmt::skip]
const ACTIONS: &[(&str, &str, &[(&str, crate::recog::Want)])] = &[
    ("Idle", "IdleBuzz", &[("waitMin", F(0.75)), ("waitMax", F(1.0)), ("speedMax", F(1.75)), ("accelerationMax", F(15.0)), ("roamingRange", F(1.0))]),
    ("Idle", "FaceDirection", &[("spriteFacesRight", B(false)), ("playNewAnimation", B(true)), ("newAnimationClip", S("TurnToIdle")), ("everyFrame", B(true)), ("pauseBetweenTurns", B(true)), ("pauseTime", F(0.5))]),
    ("Idle", "Tk2dPlayAnimation", &[("clipName", S("Idle"))]),
    ("Startle", "Tk2dPlayAnimation", &[("clipName", S("Startle"))]),
    ("Chase Start", "Tk2dPlayAnimation", &[("clipName", S("Chase"))]),
    ("Chase Start", "Tk2dPlayFrame", &[("frame", I(3))]),
    ("Chase Start", "Wait", &[("time", F(0.0))]),
    ("Chase - In Sight", "ChaseObject", &[("speedMax", F(5.0)), ("acceleration", F(0.045)), ("targetSpread", F(0.0))]),
    ("Chase - In Sight", "FaceDirection", &[("newAnimationClip", S("TurnToFly")), ("pauseTime", F(0.5)), ("pauseBetweenTurns", B(true))]),
    ("Chase - Out of Sight", "ChaseObject", &[("speedMax", F(5.0)), ("acceleration", F(0.045)), ("targetSpread", F(0.0))]),
    ("Chase - Out of Sight", "Wait", &[("time", S("Attention Span"))]),
    ("Stop", "Decelerate", &[("deceleration", F(0.12))]),
    ("Stop", "Wait", &[("time", F(1.0))]),
    ("Stop", "Tk2dPlayAnimation", &[("clipName", S("Idle"))]),
    ("Stop", "Tk2dPlayFrame", &[("frame", I(3))]),
];
const TRANSITIONS: [(&str, &[(&str, &str)]); 8] = [
    ("Initiate", &[("FINISHED", "Idle"), ("ALERT", "Chase Start")]),
    ("Idle", &[("ALERT", "Startles?"), ("TOOK DAMAGE", "Startles?")]),
    ("Startles?", &[("TRUE", "Startle"), ("FALSE", "Chase Start")]),
    ("Startle", &[("ANIM END", "Chase Start")]),
    ("Chase Start", &[("WAIT", "Chase - In Sight")]),
    ("Chase - In Sight", &[("WAIT", "Chase - Out of Sight")]),
    ("Chase - Out of Sight", &[("ALERT", "Chase - In Sight"), ("WAIT", "Stop")]),
    ("Stop", &[("WAIT", "Idle")]),
];

fn check_transitions(sts: &[(String, &Value)], table: &[(&str, &[(&str, &str)])], who: &str) -> Result<()> {
    for (name, expected) in table {
        let ok = state(sts, name).map(transitions).transpose()?.is_some_and(|t| t.len() == expected.len() && t.iter().zip(expected.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1));
        if !ok {
            return err(format!("unsupported {who} transitions: {name}"));
        }
    }
    Ok(())
}

pub fn recognize(sc: &Scene, source: &Source, gid: i64) -> Result<Json> {
    let records = component_records(sc, gid);
    check_assemblies(source, "Vengefly")?;
    let fsms: Vec<&Value> = records.iter().filter(|r| r.1 == "PlayMakerFSM").filter_map(|r| r.2.get("fsm")).filter(|f| f.get("name").and_then(Value::str).as_deref() == Some("chaser")).collect();
    if fsms.len() != 1 {
        return err("no single Vengefly chaser FSM");
    }
    let fsm = fsms[0];
    if get(fsm, "startState")?.str().as_deref() != Some("Initiate") {
        return err("Vengefly chaser begins in unsupported state");
    }
    let vars = variables(fsm);
    let var = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
    let eq = |v: Option<&Value>, x: f64| v.is_some_and(|v| !matches!(v, Value::Str(_)) && v.float() == Some(x));
    if !eq(var("Attention Span"), 10.0) || !eq(var("Startles"), 1.0) || !eq(var("Start Alert"), 0.0) {
        return err("unsupported Vengefly chaser variables");
    }
    let sts = states(fsm)?;
    check_transitions(&sts, &TRANSITIONS, "Vengefly chaser")?;
    check_actions(&sts, ACTIONS, "Vengefly")?;
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if !unit_rotation(&matrix) {
        return err("unsupported Vengefly initial rotation or scale");
    }
    let body = body_box(&records, "Vengefly")?;
    if !body_ok(body, BODY_SIZE, BODY_OFFSET)? {
        return err("unsupported Vengefly body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Vengefly")?;
    if get(rigid, "m_BodyType")?.int() != Some(0) || get(rigid, "m_GravityScale")?.float() != Some(0.0) || get(rigid, "m_LinearDamping")?.float() != Some(0.0) || get(rigid, "m_Constraints")?.int() != Some(4) {
        return err("unsupported Vengefly rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Vengefly")?;
    if get(recoil, "freezeInPlace")?.truthy() || get(recoil, "recoilSpeedBase")?.float() != Some(15.0) || get(recoil, "recoilDuration")?.float() != Some(0.25) || get(recoil, "preventRecoilUp")?.truthy() {
        return err("unsupported Vengefly recoil variant");
    }
    let (_, sight) = only(&records, "LineOfSightDetector", "Vengefly")?;
    let ranges = get(sight, "alertRanges")?.list().unwrap_or(&[]);
    if !get(sight, "m_Enabled")?.truthy() || ranges.len() != 1 {
        return err("unsupported Vengefly line of sight detector");
    }
    let alert_id = get(&ranges[0], "m_PathID")?.int().unwrap_or(0);
    let alert_gid = get(get(&sc.object(alert_id).ok_or("alert range is not in the scene")?.tree, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
    let alert_records = component_records(sc, alert_gid);
    let (_, circle) = only(&alert_records, "CircleCollider2D", "Vengefly")?;
    let alert_t = sc.transform(*sc.go_transform.get(&alert_gid).ok_or("alert object has no transform")?).ok_or("alert transform missing")?;
    let alert_scale = xy(alert_t, "m_LocalScale")?;
    let radius = get(circle, "m_Radius")?.float().ok_or("m_Radius")? * alert_scale[0];
    if !sc.active(alert_gid) || !get(circle, "m_IsTrigger")?.truthy() || !get(circle, "m_Offset")?.py_eq(&zero2()) || !near(Some(&n(radius)), ALERT_RADIUS) || !near(Some(&n(alert_scale[0])), alert_scale[1]) || !get(alert_t, "m_LocalPosition")?.py_eq(&zero3()) {
        return err("unsupported Vengefly alert range");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Vengefly")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    if let Some(name) = clips_ok(&library, &CLIPS)? {
        return err(format!("unsupported Vengefly animation: {name}"));
    }
    let bounds = axis_aligned_bounds(&identity(), BODY_OFFSET, BODY_SIZE)?;
    Ok(jobj(vec![
        ("kind", js("Vengefly")),
        ("guest_enabled", Json::Bool(true)),
        ("alert_radius", Json::Float(ALERT_RADIUS)),
        ("body_bounds_local", floats(&bounds)),
        (
            "limitations",
            strings(&[
                "Gravity-free dynamic body on the bounded terrain solver; Unity fixed-step ordering and Random.Range are not reproduced.",
                "Startle one-shot and the live buzz loop audio are not presented.",
                "The breaker corpse is removed on landing; its break pieces are not presented.",
            ]),
        ),
    ]))
}

// --- Acid Flyer (Duranda, Fungus1_09) -------------------------------------------
//
// Not a Vengefly, but the nearest flyer: no AI, a pogo platform over the acid.
// The controller is shared/hk-sim/src/acid_flyer.rs. Every number below is read
// back out of the placement and checked, so a placement whose FSMs or boxes
// moved is refused rather than seated wrong (see the notes in host/vengefly.py
// on `Tween`, `Acid Flyer`, the HealthManager and the `Shell` child).

const ACID_FLYER_CLIPS: [(&str, usize, f64, i64, Option<i64>); 2] = [("Fly", 6, 12.0, 0, Some(0)), ("TurnToFly", 9, 12.0, 1, Some(3))];
const ACID_FLYER_BODY: [[f64; 2]; 2] = [[0.8472197651863098, 1.14892578125], [-0.51702880859375, -0.44126415252685547]];
const ACID_FLYER_SHELL: [[f64; 2]; 2] = [[1.7023437023162842, 1.7226080894470215], [-0.0997161865234375, -0.13818359375]];
const ACID_FLYER_TWEEN: [(&str, &[(&str, &str)]); 4] = [("Init", &[("FINISHED", "Tween Up")]), ("Tween Up", &[("FINISHED", "Tween Down")]), ("Tween Down", &[("FINISHED", "Reset Pos")]), ("Reset Pos", &[("FINISHED", "Tween Up")])];
const ACID_FLYER_TWEEN_ACTIONS: [(&str, &[&str]); 4] = [("Init", &["SetVector3Value", "Vector3Multiply", "GetPosition"]), ("Tween Up", &["iTweenMoveBy"]), ("Tween Down", &["iTweenMoveBy"]), ("Reset Pos", &["SetPosition"])];
const ACID_FLYER_CONTROL: [(&str, &[(&str, &str)]); 3] = [("Init", &[("FINISHED", "Idle")]), ("Idle", &[("BLOCKED DOWN", "Bounce Anim")]), ("Bounce Anim", &[("FINISHED", "Idle")])];
const EASE_IN_OUT_SINE: i64 = 14;
/// `Spell Vulnerable`: user tag 66 in TagManager, serialized as 20000 + 66.
const SPELL_VULNERABLE_TAG: i64 = 20066;

/// `_enabled_actions`: (index, short name) of a state's enabled actions.
fn enabled_actions(st: &Value) -> Result<Vec<(usize, String)>> {
    let data = get(st, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    Ok(names.iter().enumerate().filter(|(i, _)| enabled.get(*i).is_some_and(Value::truthy)).map(|(i, n)| (i, n.str().unwrap_or_default().rsplit('.').next().unwrap_or("").to_string())).collect())
}

/// `_raw_int`: a serialized 4-byte action parameter of a state's action.
fn raw_int(st: &Value, index: usize, name: &str) -> Result<i64> {
    let data = get(st, "actionData")?;
    let starts: Vec<i64> = get(data, "actionStartIndex")?.list().unwrap_or(&[]).iter().map(|x| x.int().unwrap_or(0)).collect();
    let names = get(data, "actionNames")?.list().unwrap_or(&[]).len();
    let param_names = get(data, "paramName")?.list().unwrap_or(&[]);
    let start = starts[index] as usize;
    let end = if index + 1 < names { starts[index + 1] as usize } else { param_names.len() };
    let bytes: Vec<u8> = match get(data, "byteData")? {
        Value::Bytes(b) => b.clone(),
        other => other.list().unwrap_or(&[]).iter().map(|x| x.int().unwrap_or(0) as u8).collect(),
    };
    for i in start..end {
        if param_names.get(i).and_then(Value::str).as_deref() == Some(name) {
            let pos = get(data, "paramDataPos")?.list().and_then(|l| l.get(i)).and_then(Value::int).unwrap_or(0) as usize;
            let slice = &bytes[pos.min(bytes.len())..(pos + 4).min(bytes.len())];
            let mut word = [0u8; 4];
            word[..slice.len()].copy_from_slice(slice);
            let mut v = i32::from_le_bytes(word) as i64;
            if slice.len() < 4 {
                // int.from_bytes of a short slice is unsigned in its own width
                v = slice.iter().enumerate().fold(0i64, |a, (k, b)| a | (*b as i64) << (8 * k));
                if slice.last().is_some_and(|b| b & 0x80 != 0) {
                    v -= 1 << (8 * slice.len());
                }
            }
            return Ok(v);
        }
    }
    err(format!("Acid Flyer {} action lacks {name}", get(st, "name")?.str().unwrap_or_default()))
}

/// `_literal`: a compact scalar's literal; a variable slot left as PlayMaker
/// None (useVariable with no name) keeps the literal too.
fn literal(v: &Value) -> Result<Value> {
    if v.is_map() {
        if v.get("useVariable").is_some_and(Value::truthy) && v.get("name").is_some_and(Value::truthy) {
            return err(format!("Acid Flyer action reads a variable where a literal was audited: {}", v.get("name").and_then(Value::str).unwrap_or_default()));
        }
        return Ok(get(v, "value")?.clone());
    }
    Ok(v.clone())
}
fn field<'a>(f: &'a Fields, k: &str) -> Result<&'a Value> {
    hk_unity::playmaker::field(f, k).ok_or_else(|| format!("missing field {k}"))
}
fn num_is(v: &Value, x: f64) -> bool {
    !matches!(v, Value::Str(_) | Value::List(_) | Value::Map(_) | Value::Bytes(_)) && (v.float() == Some(x) || v.int().map(|i| i as f64) == Some(x))
}

/// `_acid_flyer_tween`: (Move Vector y, Speed, waits) of one `Tween` FSM, or a refusal.
fn acid_flyer_tween(fsm: &Value) -> Result<(f64, f64, bool)> {
    if get(fsm, "startState")?.str().as_deref() != Some("Init") {
        return err("Acid Flyer Tween starts elsewhere");
    }
    let sts = states(fsm)?;
    let mut have: Vec<&str> = sts.iter().map(|s| s.0.as_str()).collect();
    let mut want: Vec<&str> = ACID_FLYER_TWEEN.iter().map(|s| s.0).collect();
    have.sort();
    want.sort();
    if have != want {
        return err("unsupported Acid Flyer Tween states");
    }
    for (name, expected) in ACID_FLYER_TWEEN {
        let t = transitions(state(&sts, name).unwrap())?;
        if !(t.len() == expected.len() && t.iter().zip(expected.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1)) {
            return err(format!("unsupported Acid Flyer Tween transitions: {name}"));
        }
    }
    let init_state = state(&sts, "Init").unwrap();
    let init: Vec<String> = enabled_actions(init_state)?.into_iter().map(|a| a.1).collect();
    let base: Vec<String> = ACID_FLYER_TWEEN_ACTIONS[0].1.iter().map(|s| s.to_string()).collect();
    let waits = init == [base.clone(), vec!["Wait".to_string()]].concat();
    if !waits && init != base {
        return err("unsupported Acid Flyer Tween Init");
    }
    let init_data = get(init_state, "actionData")?;
    if waits {
        let f = crate::cook_audio::u(action_fields(init_data, 3, false))?;
        if !near(Some(&literal(field(&f, "time")?)?), 0.5) {
            return err("unsupported Acid Flyer Tween wait");
        }
    }
    let multiply = crate::cook_audio::u(action_fields(init_data, 1, false))?;
    if !near(Some(&literal(field(&multiply, "multiplyBy")?)?), -1.0) {
        return err("unsupported Acid Flyer inverse vector");
    }
    for (name, expected) in &ACID_FLYER_TWEEN_ACTIONS[1..] {
        let got: Vec<String> = enabled_actions(state(&sts, name).unwrap())?.into_iter().map(|a| a.1).collect();
        if got != expected.iter().map(|s| s.to_string()).collect::<Vec<_>>() {
            return err(format!("unsupported Acid Flyer Tween actions: {name}"));
        }
    }
    for name in ["Tween Up", "Tween Down"] {
        let st = state(&sts, name).unwrap();
        let fields = crate::cook_audio::u(action_fields(get(st, "actionData")?, 0, false))?;
        let speed = field(&fields, "speed")?;
        let ok = speed.get("useVariable").is_some_and(Value::truthy)
            && speed.get("name").and_then(Value::str).as_deref() == Some("Speed")
            && num_is(&literal(field(&fields, "time")?)?, 0.0)
            && num_is(&literal(field(&fields, "delay")?)?, 0.0)
            && field(&fields, "finishEvent")?.str().as_deref() == Some("FINISHED")
            && literal(field(&fields, "stopOnExit")?)?.truthy()
            && literal(field(&fields, "loopDontFinish")?)?.truthy()
            && !literal(field(&fields, "orientToPath")?)?.truthy()
            && (raw_int(st, 0, "easeType")?, raw_int(st, 0, "loopType")?, raw_int(st, 0, "space")?) == (EASE_IN_OUT_SINE, 0, 0);
        if !ok {
            return err(format!("unsupported Acid Flyer iTweenMoveBy: {name}"));
        }
    }
    let vars = variables_strict(fsm)?;
    let var = |k: &str| vars.iter().find(|(n, _)| n == k).and_then(|(_, v)| *v);
    let (vector, speed) = (var("Move Vector"), var("Speed"));
    let vy = vector.filter(|v| v.is_map()).and_then(|v| {
        let ok = v.get("x").is_some_and(|x| num_is(x, 0.0)) && v.get("z").is_some_and(|z| num_is(z, 0.0)) && v.get("y").is_some_and(Value::truthy);
        ok.then(|| v.get("y").and_then(Value::float)).flatten()
    });
    let speed = speed.filter(|s| matches!(s, Value::F32(_) | Value::F64(_))).and_then(Value::float).filter(|s| *s > 0.0);
    match (vy, speed) {
        (Some(y), Some(s)) => Ok((y, s, waits)),
        _ => err("unsupported Acid Flyer Move Vector or Speed"),
    }
}

/// `recognize_acid_flyer`: admit a placed Acid Flyer, or refuse with the reason.
pub fn recognize_acid_flyer(sc: &Scene, source: &Source, gid: i64, health: &Value) -> Result<Json> {
    let records = component_records(sc, gid);
    check_assemblies(source, "Acid Flyer")?;
    let go = sc.go(gid).ok_or("no such GameObject")?;
    if get(go, "m_Layer")?.int() != Some(11) || go.get("m_Tag").and_then(Value::int) != Some(SPELL_VULNERABLE_TAG) {
        return err("Acid Flyer is not a Spell Vulnerable enemy");
    }
    let fsms: Vec<&Value> = records.iter().filter(|r| r.1 == "PlayMakerFSM" && r.2.get("m_Enabled").is_some_and(Value::truthy)).filter_map(|r| r.2.get("fsm")).collect();
    let named = |n: &str| -> Vec<&Value> { fsms.iter().copied().filter(|f| f.get("name").and_then(Value::str).as_deref() == Some(n)).collect() };
    let (control, tweens) = (named("Acid Flyer"), named("Tween"));
    if control.len() != 1 || fsms.len() != 1 + tweens.len() || !(1..=2).contains(&tweens.len()) {
        return err("unsupported Acid Flyer FSM set");
    }
    let sts = states(control[0])?;
    let mut have: Vec<&str> = sts.iter().map(|s| s.0.as_str()).collect();
    let mut want: Vec<&str> = ACID_FLYER_CONTROL.iter().map(|s| s.0).collect();
    have.sort();
    want.sort();
    if get(control[0], "startState")?.str().as_deref() != Some("Init") || have != want {
        return err("unsupported Acid Flyer control states");
    }
    for (name, expected) in ACID_FLYER_CONTROL {
        let t = transitions(state(&sts, name).unwrap())?;
        if !(t.len() == expected.len() && t.iter().zip(expected.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1)) {
            return err(format!("unsupported Acid Flyer control transitions: {name}"));
        }
    }
    let idle = state(&sts, "Idle").unwrap();
    if enabled_actions(idle)?.into_iter().map(|a| a.1).collect::<Vec<_>>() != ["FaceObject", "Tk2dPlayAnimation"] {
        return err("unsupported Acid Flyer Idle actions");
    }
    let idle_data = get(idle, "actionData")?;
    let face = crate::cook_audio::u(action_fields(idle_data, 0, false))?;
    let play = crate::cook_audio::u(action_fields(idle_data, 1, false))?;
    let lit = |f: &Fields, k: &str| -> Result<Value> { literal(field(f, k)?) };
    if lit(&face, "spriteFacesRight")?.truthy()
        || !lit(&face, "playNewAnimation")?.truthy()
        || lit(&face, "newAnimationClip")?.str().as_deref() != Some("TurnToFly")
        || !lit(&face, "resetFrame")?.truthy()
        || !lit(&face, "everyFrame")?.truthy()
        || lit(&play, "clipName")?.str().as_deref() != Some("Fly")
    {
        return err("unsupported Acid Flyer facing");
    }
    let parsed: Vec<(f64, f64, bool)> = tweens.iter().map(|t| acid_flyer_tween(t)).collect::<Result<_>>()?;
    let leads: Vec<&(f64, f64, bool)> = parsed.iter().filter(|t| !t.2).collect();
    let mains: Vec<&(f64, f64, bool)> = parsed.iter().filter(|t| t.2).collect();
    if mains.len() != 1 || leads.len() > 1 {
        return err("Acid Flyer needs one waiting Tween and at most one lead");
    }
    let (amount, speed, _) = *mains[0];
    let q = |v: f64| py_round(v * 65536.0);
    let lead = leads.first().map_or([0, 0], |l| [q(l.0), q(l.1)]);
    if !health_is(health, 30) || !get(health, "invincible")?.truthy() || get(health, "invincibleFromDirection")?.int() != Some(7) || !get(health, "preventInvincibleEffect")?.truthy() || get(health, "hasSpecialDeath")?.truthy() || get(health, "hasAlternateHitAnimation")?.truthy() || get(health, "damageOverride")?.truthy() || !near(health.get("invulnerableTime"), 0.25) {
        return err("unsupported Acid Flyer HealthManager");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    if !unit_rotation(&u(sc.world(tid))?) {
        return err("unsupported Acid Flyer initial rotation or scale");
    }
    let body = body_box(&records, "Acid Flyer")?;
    if !body_ok(body, ACID_FLYER_BODY[0], ACID_FLYER_BODY[1])? {
        return err("unsupported Acid Flyer body collider");
    }
    if !records.iter().any(|r| r.1 == "BigBouncer" && r.2.get("m_Enabled").is_some_and(Value::truthy)) {
        return err("Acid Flyer without BigBouncer");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Acid Flyer")?;
    if get(rigid, "m_BodyType")?.int() != Some(0) || get(rigid, "m_GravityScale")?.float() != Some(0.0) || get(rigid, "m_Constraints")?.int() != Some(4) {
        return err("unsupported Acid Flyer rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Acid Flyer")?;
    if get(recoil, "recoilSpeedBase")?.float() != Some(0.0) || get(recoil, "freezeInPlace")?.truthy() {
        return err("unsupported Acid Flyer recoil");
    }
    let mut shells: Vec<i64> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let g = get(get(&o.tree, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        if sc.go(g).and_then(|x| x.get("m_Name")).and_then(Value::str).as_deref() == Some("Shell") {
            shells.push(g);
        }
    }
    if shells.len() != 1 || !sc.active(shells[0]) {
        return err("Acid Flyer without one active Shell");
    }
    let shell_records = component_records(sc, shells[0]);
    let shell_t = sc.transform(*sc.go_transform.get(&shells[0]).ok_or("shell has no transform")?).ok_or("shell transform missing")?;
    let mut shell_fsms: Vec<String> = shell_records.iter().filter(|r| r.1 == "PlayMakerFSM" && r.2.get("m_Enabled").is_some_and(Value::truthy)).filter_map(|r| r.2.get("fsm").and_then(|f| f.get("name")).and_then(Value::str)).collect();
    shell_fsms.sort();
    let (_, shell_box) = only(&shell_records, "BoxCollider2D", "Acid Flyer")?;
    let (_, shell_damage) = only(&shell_records, "DamageHero", "Acid Flyer")?;
    let (_, tink) = only(&shell_records, "TinkEffect", "Acid Flyer")?;
    let one3 = Value::Map(vec![("x".into(), n(1.0)), ("y".into(), n(1.0)), ("z".into(), n(1.0))]);
    let (ssize, soffset) = (xy(shell_box, "m_Size")?, xy(shell_box, "m_Offset")?);
    let shell_ok = shell_fsms == ["Block Bounce", "Destroy if parent null", "FSM"]
        && get(sc.go(shells[0]).ok_or("no shell object")?, "m_Layer")?.int() == Some(11)
        && get(shell_t, "m_LocalPosition")?.py_eq(&zero3())
        && get(shell_t, "m_LocalScale")?.py_eq(&one3)
        && get(shell_box, "m_Enabled")?.truthy()
        && !get(shell_box, "m_IsTrigger")?.truthy()
        && (0..2).all(|k| near(Some(&n(ssize[k])), ACID_FLYER_SHELL[0][k]) && near(Some(&n(soffset[k])), ACID_FLYER_SHELL[1][k]))
        && get(shell_damage, "damageDealt")?.int() == Some(1)
        && !get(tink, "useNailPosition")?.truthy()
        && !get(tink, "sendFSMEvent")?.truthy()
        && !shell_records.iter().any(|r| matches!(r.1, "HealthManager" | "BigBouncer" | "NonBouncer"));
    if !shell_ok {
        return err("unsupported Acid Flyer Shell");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Acid Flyer")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    if let Some(name) = clips_ok(&library, &ACID_FLYER_CLIPS)? {
        return err(format!("unsupported Acid Flyer animation: {name}"));
    }
    let shell = axis_aligned_bounds(&identity(), ACID_FLYER_SHELL[1], ACID_FLYER_SHELL[0])?;
    Ok(jobj(vec![
        ("kind", js("AcidFlyer")),
        ("guest_enabled", Json::Bool(true)),
        ("amount", Json::Int(q(amount))),
        ("speed", Json::Int(q(speed))),
        ("lead", Json::List(lead.iter().map(|&v| Json::Int(v)).collect())),
        ("shell", Json::List(shell.iter().map(|&v| Json::Int(q(v))).collect())),
        // The live direction test replaces the serialized flag; the generator
        // would otherwise refuse invincibleFromDirection.
        ("invincible", Json::Bool(false)),
        ("invincible_from_direction", Json::Int(7)),
        ("art_bindings", jobj(vec![("walk", js("Fly")), ("turn", js("TurnToFly"))])),
        (
            "limitations",
            strings(&[
                "The tween is a fixed 60 Hz sample of iTween easeInOutSine; the disposed lead tween is modelled from its curve, not from a running second tween.",
                "The fly loop audio, the Shell's Block Hit v2 effect and corpse steam are not presented.",
                "BounceHigh is taken when a down-slash reaches the body box; the source takes whichever of the body and the Shell its trigger callbacks report first.",
            ]),
        ),
    ]))
}

// --- Mosquito (Squit, Greenpath) -------------------------------------------------
//
// The controller is shared/hk-sim/src/mosquito.rs. Its numbers were read out of
// `Mozzie` and its custom actions' IL; both FSMs are identical on all 14
// placements, so they are pinned by digest and any change refuses the
// placement instead of seating stale numbers.
const MOSQUITO_FSM_SHA256: [(&str, &str); 2] = [
    ("Mozzie", "d5c5dbe9844f54dd6897e07f2f3bf9c4cee04520c0f72aae93a47bd887cf6a0f"),
    // `GO UP/LEFT/RIGHT/DOWN` mover; nothing in the five scenes sends to it.
    ("FSM", "6118e236bc3a678dd700800def0c2554cf0d1e501f7ea825b5198c5ff15f75ae"),
];
const MOSQUITO_CLIPS: [(&str, usize, f64, i64, Option<i64>); 6] = [("Idle", 8, 10.0, 0, Some(0)), ("TurnToIdle", 10, 12.0, 1, Some(2)), ("Startle", 4, 12.0, 2, Some(0)), ("Attack Antic", 6, 10.0, 2, Some(0)), ("Attack", 3, 12.0, 0, Some(0)), ("Death Air", 3, 12.0, 2, Some(0))];
/// Slot order of `ActorController::Mosquito::clips`, after walk (Idle) and turn (TurnToIdle).
const MOSQUITO_SLOTS: [(&str, &str); 4] = [("startle", "Startle"), ("antic", "Attack Antic"), ("attack", "Attack"), ("pull_out", "Death Air")];
const MOSQUITO_BODY: [[f64; 2]; 2] = [[1.40625, 0.265625], [-0.453125, -0.0703125]];
const MOSQUITO_ALERT_RADIUS: f64 = 0.41109946370124817 * 21.115947723388672;
/// `TileDetector`: a second solid box (terrain and nail), until `Attack Antic`.
const MOSQUITO_TILE: [[f64; 2]; 4] = [[0.6755398511886597, 0.965610146522522], [0.0, 0.01719517633318901], [-0.15, -0.16], [1.33, 1.47628915309906]];

/// `recognize_mosquito`: admit a placed Mosquito, or refuse with the reason.
pub fn recognize_mosquito(sc: &Scene, source: &Source, gid: i64, health: &Value) -> Result<Json> {
    let records = component_records(sc, gid);
    check_assemblies(source, "Mosquito")?;
    let mut fsms: Vec<(String, &Value)> = Vec::new();
    for r in records.iter().filter(|r| r.1 == "PlayMakerFSM" && r.2.get("m_Enabled").is_some_and(Value::truthy)) {
        let f = get(r.2, "fsm")?;
        let name = get(f, "name")?.str().unwrap_or_default();
        match fsms.iter_mut().find(|x| x.0 == name) {
            Some(slot) => slot.1 = f,
            None => fsms.push((name, f)),
        }
    }
    let mut names: Vec<&str> = fsms.iter().map(|f| f.0.as_str()).collect();
    names.sort();
    if names != ["FSM", "Mozzie"] {
        return err(format!("unsupported Mosquito FSM set: {}", names.join(", ")));
    }
    for (name, digest) in MOSQUITO_FSM_SHA256 {
        let got = fsm_digest(fsms.iter().find(|f| f.0 == name).unwrap().1, &[])?;
        if got != digest {
            return err(format!("Mosquito FSM {name} changed: {got}"));
        }
    }
    if !health_is(health, 10) || get(health, "invincible")?.truthy() || get(health, "invincibleFromDirection")?.truthy() || get(health, "hasSpecialDeath")?.truthy() || get(health, "hasAlternateHitAnimation")?.truthy() || get(health, "damageOverride")?.truthy() {
        return err("unsupported Mosquito HealthManager");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    if !unit_rotation(&u(sc.world(tid))?) {
        return err("unsupported Mosquito initial rotation or scale");
    }
    let body = body_box(&records, "Mosquito")?;
    if !body_ok(body, MOSQUITO_BODY[0], MOSQUITO_BODY[1])? {
        return err("unsupported Mosquito body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Mosquito")?;
    if get(rigid, "m_BodyType")?.int() != Some(0) || get(rigid, "m_GravityScale")?.float() != Some(0.0) || get(rigid, "m_LinearDamping")?.float() != Some(0.0) || get(rigid, "m_Constraints")?.int() != Some(4) {
        return err("unsupported Mosquito rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Mosquito")?;
    if get(recoil, "freezeInPlace")?.truthy() || get(recoil, "recoilSpeedBase")?.float() != Some(20.0) || !near(recoil.get("recoilDuration"), 0.15) || get(recoil, "preventRecoilUp")?.truthy() {
        return err("unsupported Mosquito recoil");
    }
    let (_, sight) = only(&records, "LineOfSightDetector", "Mosquito")?;
    let ranges = get(sight, "alertRanges")?.list().unwrap_or(&[]);
    if !get(sight, "m_Enabled")?.truthy() || ranges.len() != 1 {
        return err("unsupported Mosquito line of sight detector");
    }
    let mut children: Vec<(String, i64)> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let g = get(get(&o.tree, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        let name = get(sc.go(g).ok_or("child without a GameObject")?, "m_Name")?.str().unwrap_or_default();
        match children.iter_mut().find(|c| c.0 == name) {
            Some(slot) => slot.1 = g,
            None => children.push((name, g)),
        }
    }
    let mut child_names: Vec<&str> = children.iter().map(|c| c.0.as_str()).collect();
    child_names.sort();
    if child_names != ["Alert Range New", "Thunk Effect", "TileDetector"] {
        return err("unsupported Mosquito children");
    }
    let child = |n: &str| children.iter().find(|c| c.0 == n).map(|c| c.1).unwrap();
    let alert_id = get(&ranges[0], "m_PathID")?.int().unwrap_or(0);
    let alert_gid = get(get(&sc.object(alert_id).ok_or("alert range is not in the scene")?.tree, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
    if alert_gid != child("Alert Range New") {
        return err("Mosquito sight reads another alert range");
    }
    let alert_records = component_records(sc, alert_gid);
    let (_, circle) = only(&alert_records, "CircleCollider2D", "Mosquito")?;
    let alert = sc.transform(*sc.go_transform.get(&alert_gid).ok_or("alert object has no transform")?).ok_or("alert transform missing")?;
    let scale = xy(alert, "m_LocalScale")?;
    let reach = get(circle, "m_Radius")?.float().ok_or("m_Radius")? * scale[0].max(scale[1]);
    if !get(circle, "m_IsTrigger")?.truthy() || !get(circle, "m_Offset")?.py_eq(&zero2()) || !get(alert, "m_LocalPosition")?.py_eq(&zero3()) || !near(Some(&n(reach)), MOSQUITO_ALERT_RADIUS) {
        return err("unsupported Mosquito alert range");
    }
    let tile_gid = child("TileDetector");
    let tile_records = component_records(sc, tile_gid);
    let (_, tile_box) = only(&tile_records, "BoxCollider2D", "Mosquito")?;
    let tile = sc.transform(*sc.go_transform.get(&tile_gid).ok_or("tile has no transform")?).ok_or("tile transform missing")?;
    let [tsize, toffset, tpos, tscale] = MOSQUITO_TILE;
    let (bsize, boffset, lpos, lscale) = (xy(tile_box, "m_Size")?, xy(tile_box, "m_Offset")?, xy(tile, "m_LocalPosition")?, xy(tile, "m_LocalScale")?);
    let tile_ok = sc.active(tile_gid)
        && !get(tile_box, "m_IsTrigger")?.truthy()
        && get(sc.go(tile_gid).ok_or("no tile object")?, "m_Layer")?.int() == Some(11)
        && (0..2).all(|k| near(Some(&n(bsize[k])), tsize[k]) && (boffset[k] - toffset[k]).abs() < 1e-5 && near(Some(&n(lpos[k])), tpos[k]) && near(Some(&n(lscale[k])), tscale[k]));
    if !tile_ok {
        return err("unsupported Mosquito TileDetector");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Mosquito")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    if let Some(name) = clips_ok(&library, &MOSQUITO_CLIPS)? {
        return err(format!("unsupported Mosquito animation: {name}"));
    }
    let tile_bounds = axis_aligned_bounds(&identity(), [tpos[0] + toffset[0] * tscale[0], tpos[1] + toffset[1] * tscale[1]], [tsize[0] * tscale[0], tsize[1] * tscale[1]])?;
    let mut art = vec![("walk".to_string(), js("Idle")), ("turn".to_string(), js("TurnToIdle"))];
    art.extend(MOSQUITO_SLOTS.iter().map(|(k, v)| (k.to_string(), js(v))));
    Ok(jobj(vec![
        ("kind", js("Mosquito")),
        ("guest_enabled", Json::Bool(true)),
        ("alert_radius", Json::Float(MOSQUITO_ALERT_RADIUS)),
        ("tile", Json::List(tile_bounds.iter().map(|&v| Json::Int(py_round(v * 65536.0))).collect())),
        ("art_bindings", Json::Obj(art)),
        (
            "limitations",
            strings(&[
                "The body box stays axis aligned through the lunge; the source rotates it with the sprite.",
                "The fly loop, Impact Lines and Thunk Effect are not presented.",
                "The inert `FSM` mover (GO UP/LEFT/RIGHT/DOWN, no sender in any of its scenes) is not run.",
            ]),
        ),
    ]))
}
