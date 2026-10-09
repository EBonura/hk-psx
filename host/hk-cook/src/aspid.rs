//! Recognize the verified Aspid Hunter (Spitter) `spitter` FSM for guest
//! admission. Ported from host/aspid.py.
//!
//! The controller lives in shared/hk-sim/src/aspid.rs; this module admits only
//! placed instances matching the audited contract.

use crate::common::{component_records, err, get, Result};
use crate::cook_audio::{jobj, js, u};
use crate::pyjson::Json;
use crate::recog::{
    check_actions, check_assemblies, clip_is, clips_by_name, near, only, state, states,
    transitions, variables, xy,
};
use crate::runner::axis_aligned_bounds;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// (name, frames, fps, wrap mode).
const CLIPS: [(&str, usize, f64, i64); 3] = [
    ("Fly", 8, 12.0, 0),
    ("TurnToFly", 10, 12.0, 1),
    ("Fire Long", 12, 12.0, 2),
];
const SHOT_CLIPS: [(&str, usize, f64, i64); 2] = [("Idle", 4, 20.0, 0), ("Impact", 6, 20.0, 2)];
const BODY_SIZE: [f64; 2] = [1.09375, 1.234375];
const BODY_OFFSET: [f64; 2] = [-0.0625, -0.0390625];
const ALERT_RADIUS: f64 = 0.5 * 15.608528137207031;
const UNALERT_RADIUS: f64 = 12.100000381469727;

use crate::recog::Want::{B, F, S};
#[rustfmt::skip]
const ACTIONS: &[(&str, &str, &[(&str, crate::recog::Want)])] = &[
    ("Idle", "IdleBuzz", &[("waitMin", F(0.75)), ("waitMax", F(1.0)), ("speedMax", F(1.75)), ("accelerationMax", F(15.0)), ("roamingRange", F(1.0))]),
    ("Idle", "FaceDirection", &[("newAnimationClip", S("TurnToFly")), ("pauseTime", F(0.5)), ("pauseBetweenTurns", B(true)), ("spriteFacesRight", B(false))]),
    ("Distance Fly", "DistanceFly", &[("distance", F(7.0)), ("speedMax", F(4.0)), ("acceleration", F(0.1)), ("targetsHeight", B(false))]),
    ("Distance Fly", "WaitRandom", &[("timeMin", F(1.5)), ("timeMax", F(2.25))]),
    ("Distance Fly", "FloatCompare", &[("float2", F(8.0)), ("greaterThan", S("UNALERT"))]),
    ("Raycast", "FloatCompare", &[("float2", F(14.0)), ("greaterThan", S("FALSE"))]),
    ("Fly Back", "Wait", &[("time", F(0.5))]),
    ("Fly Back", "DistanceFly", &[("distance", F(8.25)), ("speedMax", F(4.0)), ("acceleration", F(0.1))]),
    ("Fire Anticipate", "DistanceFly", &[("distance", F(9.0)), ("speedMax", F(2.0)), ("acceleration", F(0.1))]),
    ("Fire Anticipate", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Fire Long")), ("animationTriggerEvent", S("WAIT"))]),
    ("Fire", "FireAtTarget", &[("speed", F(15.0)), ("spread", F(0.0))]),
];
const TRANSITIONS: [(&str, &[(&str, &str)]); 9] = [
    ("Idle", &[("ALERT", "Alert")]),
    ("Alert", &[("FINISHED", "Distance Fly")]),
    (
        "Distance Fly",
        &[("WAIT", "Raycast"), ("UNALERT", "Unalert Frame")],
    ),
    (
        "Raycast",
        &[("WAIT", "Raycast Check"), ("FALSE", "Distance Fly")],
    ),
    (
        "Raycast Check",
        &[("TRUE", "Distance Fly"), ("FALSE", "Fly Back")],
    ),
    ("Fly Back", &[("FINISHED", "Fire Anticipate")]),
    ("Fire Anticipate", &[("WAIT", "Fire")]),
    ("Fire", &[("WAIT", "Fire Dribble")]),
    ("Fire Dribble", &[("WAIT", "Distance Fly")]),
];

fn f64v(x: f64) -> Value {
    Value::F64(x)
}

/// Admit the placement at `gid`, or refuse with the first failed check.
pub fn recognize(sc: &Scene, source: &Source, gid: i64) -> Result<Json> {
    let records = component_records(sc, gid);
    check_assemblies(source, "Aspid")?;
    let fsms: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .filter(|f| f.get("name").and_then(Value::str).as_deref() == Some("spitter"))
        .collect();
    if fsms.len() != 1 || get(fsms[0], "startState")?.str().as_deref() != Some("Idle") {
        return err("no single Aspid spitter FSM");
    }
    let fsm = fsms[0];
    let vars = variables(fsm);
    let start_alert = vars
        .iter()
        .find(|(k, _)| k == "startAlert")
        .map(|(_, v)| *v);
    if !start_alert.is_some_and(|v| matches!(v, Value::Bool(_) | Value::Int(0) | Value::Int(1)))
        || !start_alert.is_some_and(|v| v.int().is_some_and(|i| i == 0 || i == 1))
    {
        return err("unsupported Aspid start alert");
    }
    let sts = states(fsm)?;
    for (name, expected) in TRANSITIONS {
        let ok = state(&sts, name)
            .map(transitions)
            .transpose()?
            .is_some_and(|t| {
                t.len() == expected.len()
                    && t.iter()
                        .zip(expected.iter())
                        .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
            });
        if !ok {
            return err(format!("unsupported Aspid transitions: {name}"));
        }
    }
    check_actions(&sts, ACTIONS, "Aspid")?;
    let fire = get(state(&sts, "Fire").ok_or("missing state")?, "actionData")?;
    let shot_ref = get(fire, "fsmGameObjectParams")?
        .list()
        .unwrap_or(&[])
        .iter()
        .find(|p| {
            !p.get("useVariable").is_some_and(Value::truthy)
                && p.get("value")
                    .and_then(|v| v.get("m_PathID"))
                    .and_then(Value::int)
                    .unwrap_or(0)
                    != 0
        })
        .and_then(|p| p.get("value"))
        .ok_or("Aspid shot prefab reference missing")?;
    let shot_obj = u(sc.deref(shot_ref))?;
    let shot_go = u(source.read(&shot_obj))?;
    if get(&shot_go, "m_Name")?.str().as_deref() != Some("Spitter Shot R") {
        return err("unsupported Aspid shot prefab");
    }
    let mut parts: Vec<(String, Value)> = Vec::new();
    for r in get(&shot_go, "m_Component")?.list().unwrap_or(&[]) {
        let component = u(source.deref(&shot_obj.file, get(r, "component")?))?;
        let kind = u(source.typename(&component))?;
        let tree = u(source.read(&component))?;
        match parts.iter_mut().find(|p| p.0 == kind) {
            Some(slot) => slot.1 = tree,
            None => parts.push((kind, tree)),
        }
    }
    let part = |k: &str| parts.iter().find(|p| p.0 == k).map(|p| &p.1);
    for kind in [
        "Rigidbody2D",
        "BoxCollider2D",
        "DamageHero",
        "EnemyBullet",
        "tk2dSpriteAnimator",
        "tk2dSprite",
        "Transform",
    ] {
        if part(kind).is_none() {
            return err("unsupported Aspid shot component set");
        }
    }
    if !near(part("Rigidbody2D").unwrap().get("m_GravityScale"), 0.05)
        || part("DamageHero")
            .unwrap()
            .get("damageDealt")
            .and_then(Value::float)
            != Some(1.0)
    {
        return err("unsupported Aspid shot body");
    }
    let shot_library_o = u(source.deref(
        &shot_obj.file,
        get(part("tk2dSpriteAnimator").unwrap(), "library")?,
    ))?;
    let shot_library = u(source.read(&shot_library_o))?;
    let shot_by_name = clips_by_name(&shot_library)?;
    for (name, frames, fps, wrap) in SHOT_CLIPS {
        if !clip_is(
            shot_by_name
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| *v),
            frames,
            fps,
            wrap,
        ) {
            return err(format!("unsupported Aspid shot animation: {name}"));
        }
    }
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if (0..2).any(|i| (0..2).any(|j| (matrix[i][j] - if i == j { 1.0 } else { 0.0 }).abs() > 1e-6))
    {
        return err("unsupported Aspid initial rotation or scale");
    }
    let (_, body) = only(&records, "BoxCollider2D", "Aspid")?;
    let (size, offset) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    if !get(body, "m_Enabled")?.truthy()
        || get(body, "m_IsTrigger")?.truthy()
        || get(body, "m_EdgeRadius")?.float() != Some(0.0)
        || (0..2).any(|k| {
            !near(Some(&f64v(size[k])), BODY_SIZE[k])
                || !near(Some(&f64v(offset[k])), BODY_OFFSET[k])
        })
    {
        return err("unsupported Aspid body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Aspid")?;
    if get(rigid, "m_BodyType")?.int() != Some(0)
        || get(rigid, "m_GravityScale")?.float() != Some(0.0)
        || get(rigid, "m_LinearDamping")?.float() != Some(0.0)
        || get(rigid, "m_Constraints")?.int() != Some(4)
    {
        return err("unsupported Aspid rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Aspid")?;
    if get(recoil, "freezeInPlace")?.truthy()
        || get(recoil, "recoilSpeedBase")?.float() != Some(15.0)
        || !near(recoil.get("recoilDuration"), 0.15)
        || get(recoil, "preventRecoilUp")?.truthy()
    {
        return err("unsupported Aspid recoil variant");
    }
    let mut circles: Vec<(String, f64)> = Vec::new();
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    for child in get(sc.transform(tid).ok_or("transform missing")?, "m_Children")?
        .list()
        .unwrap_or(&[])
    {
        let child_id = get(child, "m_PathID")?.int().unwrap_or(0);
        let child_t = sc.transform(child_id).ok_or("child transform missing")?;
        let child_gid = get(get(child_t, "m_GameObject")?, "m_PathID")?
            .int()
            .unwrap_or(0);
        for (_, kind, data) in component_records(sc, child_gid) {
            if kind == "CircleCollider2D" {
                let name = get(sc.go(child_gid).ok_or("no such GameObject")?, "m_Name")?
                    .str()
                    .unwrap_or_default();
                let radius = get(data, "m_Radius")?.float().ok_or("m_Radius")?
                    * xy(child_t, "m_LocalScale")?[0];
                match circles.iter_mut().find(|c| c.0 == name) {
                    Some(slot) => slot.1 = radius,
                    None => circles.push((name, radius)),
                }
            }
        }
    }
    let circle = |n: &str| circles.iter().find(|c| c.0 == n).map_or(0.0, |c| c.1);
    if !near(Some(&f64v(circle("Alert Range New"))), ALERT_RADIUS)
        || !near(Some(&f64v(circle("Unalert Range"))), UNALERT_RADIUS)
    {
        return err("unsupported Aspid alert ranges");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Aspid")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        if !clip_is(
            by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v),
            frames,
            fps,
            wrap,
        ) {
            return err(format!("unsupported Aspid animation: {name}"));
        }
    }
    let fire_long = by_name
        .iter()
        .find(|(k, _)| k == "Fire Long")
        .map(|(_, v)| *v)
        .ok_or("no Fire Long clip")?;
    let triggers: Vec<usize> = get(fire_long, "frames")?
        .list()
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .filter(|(_, f)| f.get("triggerEvent").is_some_and(Value::truthy))
        .map(|(i, _)| i)
        .collect();
    if triggers != [9] {
        return err("unsupported Aspid Fire Long trigger frame");
    }
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bounds = axis_aligned_bounds(&identity, BODY_OFFSET, BODY_SIZE)?;
    let shot_scale = xy(part("Transform").unwrap(), "m_LocalScale")?[0]
        * get(part("EnemyBullet").unwrap(), "scaleMin")?
            .float()
            .ok_or("scaleMin")?;
    Ok(jobj(vec![
        ("kind", js("Aspid")),
        ("guest_enabled", Json::Bool(true)),
        ("body_bounds_local", Json::List(bounds.iter().map(|&f| Json::Float(f)).collect())),
        ("start_alert", Json::Bool(start_alert.is_some_and(Value::truthy))),
        (
            "shot",
            jobj(vec![
                ("source", Json::Str(shot_obj.sid())),
                ("library", Json::Str(shot_library_o.sid())),
                // The cook holds the library object itself; the dump names its type.
                ("library_object", js("<ObjectReader>")),
                ("scale", Json::Float(shot_scale)),
            ]),
        ),
        (
            "limitations",
            Json::List(
                [
                    "Gravity-free dynamic body on the bounded terrain solver; 50 Hz fixed steps from a 60 Hz accumulator.",
                    "Shots fly straight under gravity .05 without rotation or stretch; dribble spatter, shot audio and shockwave are not presented.",
                    "The collider-less corpse is removed on its first landing instead of falling out of the scene.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
