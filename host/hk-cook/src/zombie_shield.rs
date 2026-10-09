//! Recognize the two Crossroads_15 Zombie Shields for guest admission.
//! Ported from host/zombie_shield.py.
//!
//! The controller lives in shared/hk-sim/src/zombie_shield.rs; this module admits
//! only a placement whose serialized shape matches the one that was read to
//! write it. The Zombie Shield is the second family to carry the source
//! `Walker`, so the Runner's body contract, collider reading, trigger bounds and
//! structural hash are reused. What differs is `pauses = 0`, which the Runner
//! refuses, and the FSM on top: `ZombieShieldControl` raises a shield and
//! counters, where `Zombie Swipe` lunges.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::music::value_json;
use crate::pyjson::Json;
use crate::recog::{body_box, check_assemblies, near, xy};
use crate::runner::{axis_aligned_bounds, body_contract, fsm_fingerprint, ticks, ASSEMBLIES};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// Structural fingerprint of `ZombieShieldControl`.
const FSM_SHA256: &str = "8cd470841556cbf24648cfa41f616ea4bc7d9e10884d7f8e70f4250c019cd386";
const FSM_NAME: &str = "ZombieShieldControl";
/// name: (frames, fps, wrapMode).
const CLIPS: [(&str, usize, f64, i64); 22] = [
    ("Walk", 7, 10.0, 0), ("Turn", 2, 10.0, 2), ("Idle", 6, 12.0, 0), ("Shield Front", 3, 15.0, 2), ("Shield Top", 3, 15.0, 2), ("Shield Front Bump", 2, 10.0, 2), ("Shield Top Bump", 2, 10.0, 2), ("Unshield Front", 2, 15.0, 2), ("Unshield Top", 2, 15.0, 2),
    ("Attack1 A", 5, 10.0, 2), ("Attack1 L", 1, 12.0, 2), ("Attack1 S", 1, 12.0, 2), ("Attack1 CD", 6, 10.0, 2), ("Attack3 A1", 5, 12.0, 2), ("Attack3 L1", 1, 10.0, 2), ("Attack3 S1", 1, 12.0, 2), ("Attack3 CD1", 2, 10.0, 2), ("Attack3 L2", 1, 12.0, 2),
    ("Attack3 CD2", 3, 10.0, 2), ("Attack3 L3", 1, 10.0, 2), ("Attack3 S3", 1, 12.0, 2), ("Attack3 CD3", 4, 10.0, 2),
];
/// The `ActorController::ZombieShield::clips` array, in `Clip::slot()` order,
/// with the source clip each reads.
const SLOT_CLIPS: [(&str, &str); 20] = [
    ("idle", "Idle"), ("shield_front", "Shield Front"), ("shield_top", "Shield Top"), ("bump_front", "Shield Front Bump"), ("bump_top", "Shield Top Bump"), ("unshield_front", "Unshield Front"), ("unshield_top", "Unshield Top"), ("a1_antic", "Attack1 A"), ("a1_lunge", "Attack1 L"),
    ("a1_slash", "Attack1 S"), ("a1_cooldown", "Attack1 CD"), ("a3_antic", "Attack3 A1"), ("a3_lunge1", "Attack3 L1"), ("a3_slash1", "Attack3 S1"), ("a3_cooldown1", "Attack3 CD1"), ("a3_lunge2", "Attack3 L2"), ("a3_cooldown2", "Attack3 CD2"), ("a3_lunge3", "Attack3 L3"),
    ("a3_slash3", "Attack3 S3"), ("a3_cooldown3", "Attack3 CD3"),
];
const ATTACK_RANGE: &str = "Attack Range";
/// The two `DamageHero` sword colliders the attack states switch on; authored
/// inactive and never switched on by the guest.
const SLASH_CHILDREN: [&str; 2] = ["Slash", "Slash 2"];

/// `Walker` fields this controller reproduces: (name, float?, value as the audit recorded it).
fn walker_field_ok(walker: &Value, name: &str) -> bool {
    let floats = [("rightScale", -1.0), ("edgeXAdjuster", 0.0), ("turnPause", 1.0), ("walkSpeedR", 2.0), ("walkSpeedL", -2.0)];
    let ints = [("turnAfterIdlePercentage", 0), ("pauses", 0), ("ambush", 0), ("startInactive", 0), ("waitForHeroX", 0), ("preventTurn", 0), ("ignoreHoles", 0), ("preventTurningToFaceHero", 0), ("preventScaleChange", 0), ("m_Enabled", 1)];
    let actual = walker.get(name);
    if let Some((_, v)) = floats.iter().find(|f| f.0 == name) {
        return actual.is_some_and(|a| matches!(a, Value::F32(_) | Value::F64(_))) && near(actual, *v);
    }
    if let Some((_, v)) = ints.iter().find(|f| f.0 == name) {
        return actual.is_some_and(|a| a.py_eq(&Value::Int(*v)));
    }
    let strings = [("idleClip", "Idle"), ("walkClip", "Walk"), ("turnClip", "Turn")];
    strings.iter().find(|s| s.0 == name).is_some_and(|(_, v)| actual.and_then(Value::str).as_deref() == Some(*v))
}

pub fn recognize(sc: &Scene, source: &Source, gid: i64, position: [f64; 3]) -> Result<Json> {
    check_assemblies(source, "Zombie Shield")?;
    let records = component_records(sc, gid);
    let walkers: Vec<&Value> = records.iter().filter(|r| r.1 == "Walker").map(|r| r.2).collect();
    if walkers.len() != 1 {
        return err("Zombie Shield requires exactly one Walker");
    }
    let walker = walkers[0];
    for name in ["rightScale", "edgeXAdjuster", "turnPause", "turnAfterIdlePercentage", "pauses", "idleClip", "walkClip", "turnClip", "ambush", "startInactive", "waitForHeroX", "preventTurn", "ignoreHoles", "preventTurningToFaceHero", "preventScaleChange", "m_Enabled", "walkSpeedR", "walkSpeedL"] {
        if !walker_field_ok(walker, name) {
            return err(format!("unsupported Zombie Shield Walker field: {name}"));
        }
    }
    let fsms: Vec<&Value> = records.iter().filter(|r| r.1 == "PlayMakerFSM").map(|r| r.2).collect();
    if fsms.len() != 1 || get(get(fsms[0], "fsm")?, "name")?.str().as_deref() != Some(FSM_NAME) {
        let mut present: Vec<String> = fsms.iter().filter_map(|d| d.get("fsm").and_then(|f| f.get("name")).and_then(Value::str)).collect();
        present.sort();
        return err(format!("unsupported Zombie Shield FSM set: {}", if present.is_empty() { "none".to_string() } else { present.join(", ") }));
    }
    let fsm = get(fsms[0], "fsm")?;
    let fingerprint = fsm_fingerprint(fsm)?;
    if !get(fsms[0], "m_Enabled")?.truthy() || fingerprint != FSM_SHA256 {
        return err("unverified Zombie Shield FSM variant");
    }
    if get(fsm, "startState")?.str().as_deref() != Some("Initialise") {
        return err("Zombie Shield begins in an unsupported state");
    }
    if fsm.get("globalTransitions").is_some_and(Value::truthy) {
        return err("unsupported Zombie Shield global transitions");
    }
    if position[2].abs() > 0.01 {
        return err("Zombie Shield depth differs from guest source plane");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let matrix = u(sc.world(tid))?;
    if get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(11) || (matrix[0][0].abs() - 1.0).abs() > 1e-6 || (matrix[1][1] - 1.0).abs() > 1e-6 || matrix[0][1].abs() > 1e-6 || matrix[1][0].abs() > 1e-6 {
        return err("unsupported Zombie Shield layer or initial scale");
    }
    // `rightScale` is -1, so a mirrored transform is the one facing right.
    let mirror = if matrix[0][0] < 0.0 { -1 } else { 1 };
    let box_ = body_box(&records, "Zombie Shield")?;
    if !get(box_, "m_Enabled")?.truthy() || get(box_, "m_IsTrigger")?.truthy() || get(box_, "m_EdgeRadius")?.float() != Some(0.0) {
        return err("unsupported Zombie Shield body collider");
    }
    let bodies: Vec<&Value> = records.iter().filter(|r| r.1 == "Rigidbody2D").map(|r| r.2).collect();
    if bodies.len() != 1 {
        return err("Zombie Shield requires exactly one Rigidbody2D");
    }
    body_contract(bodies[0])?;
    if get(bodies[0], "m_GravityScale")?.float() != Some(1.0) {
        return err("unsupported Zombie Shield gravity scale");
    }
    let recoils: Vec<&Value> = records.iter().filter(|r| r.1 == "Recoil").map(|r| r.2).collect();
    if recoils.len() != 1 || get(recoils[0], "freezeInPlace")?.truthy() || get(recoils[0], "preventRecoilUp")?.truthy() || get(recoils[0], "recoilSpeedBase")?.float() != Some(10.0) || !near(recoils[0].get("recoilDuration"), 0.15) {
        return err("unsupported Zombie Shield recoil variant");
    }
    let damage: Vec<&Value> = records.iter().filter(|r| r.1 == "DamageHero").map(|r| r.2).collect();
    if damage.len() != 1 || !get(damage[0], "m_Enabled")?.truthy() || get(damage[0], "damageDealt")?.int() != Some(1) {
        return err("unsupported Zombie Shield contact damage");
    }
    let detectors: Vec<i64> = records.iter().filter(|r| r.1 == "LineOfSightDetector").map(|r| r.0).collect();
    let reference = get(walker, "lineOfSightDetector")?;
    if detectors.len() != 1 || reference.pptr() != Some((0, detectors[0])) {
        return err("Zombie Shield sensing references differ");
    }
    // The children by name from the transform's own child list (the last of a name wins).
    let mut children: Vec<(String, i64)> = Vec::new();
    for child in get(sc.transform(tid).ok_or("transform missing")?, "m_Children")?.list().unwrap_or(&[]) {
        let ct = sc.transform(get(child, "m_PathID")?.int().unwrap_or(0)).ok_or("child transform missing")?;
        let kid = get(get(ct, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        let name = get(sc.go(kid).ok_or("child without a GameObject")?, "m_Name")?.str().unwrap_or_default();
        match children.iter_mut().find(|c| c.0 == name) {
            Some(slot) => slot.1 = kid,
            None => children.push((name, kid)),
        }
    }
    let child = |n: &str| children.iter().find(|c| c.0 == n).map(|c| c.1);
    // The two sword hitboxes are authored inactive and nothing in this port
    // switches them on.
    for name in SLASH_CHILDREN {
        let Some(k) = child(name) else { return err(format!("Zombie Shield is missing its {name} hitbox")) };
        if sc.active(k) {
            return err(format!("unsupported active Zombie Shield {name} hitbox"));
        }
    }
    // `Attack Range`, the one AlertRange both the Walker and the FSM read.
    let attack_gid = match child(ATTACK_RANGE) {
        Some(k) if sc.active(k) => k,
        _ => return err("Zombie Shield has no active Attack Range child"),
    };
    let range_records = component_records(sc, attack_gid);
    let alerts: Vec<(i64, &Value)> = range_records.iter().filter(|r| r.1 == "AlertRange").map(|r| (r.0, r.2)).collect();
    if alerts.len() != 1 || !get(alerts[0].1, "m_Enabled")?.truthy() {
        return err("unsupported Zombie Shield Attack Range component");
    }
    if get(walker, "alertRange")?.pptr() != Some((0, alerts[0].0)) {
        return err("Zombie Shield Walker reads a different alert range");
    }
    let boxes: Vec<&Value> = range_records.iter().filter(|r| r.1 == "BoxCollider2D").map(|r| r.2).collect();
    if boxes.len() != 1 || !get(boxes[0], "m_Enabled")?.truthy() || !get(boxes[0], "m_IsTrigger")?.truthy() || get(boxes[0], "m_EdgeRadius")?.float() != Some(0.0) {
        return err("unsupported Zombie Shield Attack Range trigger");
    }
    let attack_world = axis_aligned_bounds(&u(sc.world(*sc.go_transform.get(&attack_gid).ok_or("range has no transform")?))?, xy(boxes[0], "m_Offset")?, xy(boxes[0], "m_Size")?)?;
    // The clips.
    let animator = records.iter().rev().find(|r| r.1 == "tk2dSpriteAnimator").map(|r| r.2).ok_or("no tk2dSpriteAnimator")?;
    if !get(animator, "m_Enabled")?.truthy() || get(animator, "isRealtime")?.truthy() {
        return err("Zombie Shield requires enabled scaled-time animation");
    }
    let library_object = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_object))?;
    let by_name = crate::recog::clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        if !crate::recog::clip_is(clip, frames, fps, wrap) {
            return err(format!("unsupported Zombie Shield animation: {name}"));
        }
        let frame_list = clip.unwrap().get("frames").and_then(Value::list).unwrap_or(&[]);
        // The guest's clip clock is the cooked frame count; a source trigger
        // frame would mean the FSM acts partway through one, which none does.
        if frame_list.iter().any(|f| f.get("triggerEvent").is_some_and(Value::truthy)) {
            return err(format!("unsupported Zombie Shield frame event: {name}"));
        }
        let loop_start = clip.unwrap().get("loopStart").map_or(0, |l| l.int().unwrap_or(-1));
        if !(0 <= loop_start && (loop_start as usize) < frame_list.len()) {
            return err(format!("invalid Zombie Shield loop start: {name}"));
        }
    }
    let body_bounds = axis_aligned_bounds(&matrix, xy(box_, "m_Offset")?, xy(box_, "m_Size")?)?;
    let relative = |b: [f64; 4]| -> Result<[i64; 4]> {
        let o = [position[0], position[1]];
        let r = [0, 1, 2, 3].map(|i| py_round((b[i] - o[i % 2]) * 65536.0));
        if r.iter().any(|v| v.abs() > 16 * 65536) {
            return err("Zombie Shield local bounds exceed Q16 contract");
        }
        Ok(r)
    };
    let (body_q16, attack_q16) = (relative(body_bounds)?, relative(attack_world)?);
    // `runner_senses::Shape` mirrors the body with the facing and leaves the
    // trigger alone, which is only right for a trigger centred on the actor.
    if attack_q16[0] != -attack_q16[2] || body_q16[0] >= body_q16[2] || body_q16[1] >= body_q16[3] || attack_q16[1] >= attack_q16[3] {
        return err("Zombie Shield sensing shape is not a mirror-stable box");
    }
    let ints = |a: [i64; 4]| Json::List(a.iter().map(|&v| Json::Int(v)).collect());
    let mut art = vec![("walk".to_string(), js("Walk")), ("turn".to_string(), js("Turn"))];
    art.extend(SLOT_CLIPS.iter().map(|(k, v)| (k.to_string(), js(v))));
    let turn = CLIPS[1];
    Ok(jobj(vec![
        ("kind", js("ZombieShield")),
        ("guest_enabled", Json::Bool(true)),
        ("fsm_sha256", Json::Str(fingerprint)),
        ("assemblies_sha256", Json::Obj(ASSEMBLIES.iter().map(|(n, h)| (n.to_string(), js(h))).collect())),
        ("library_source", Json::Str(library_object.sid())),
        ("initial_direction", Json::Int(-mirror)),
        ("walk_speed", value_json(get(walker, "walkSpeedR")?)),
        ("turn_cooldown_ticks", Json::Int(60)),
        ("turn_ticks", Json::Int(ticks(turn.1 as f64 / turn.2))),
        ("body_bounds_q16", ints(body_q16)),
        ("attack_bounds_q16", ints(attack_q16)),
        ("art_bindings", Json::Obj(art)),
        (
            "limitations",
            Json::List(
                [
                    "The shield is SetInvincible: HealthManager::IsBlockingByDirection is run as the source wrote it, so the overhead shield blocks every direction and the front one blocks the hero's side and an up slash but not a pogo.",
                    "Unshield Front and Unshield Top carry no SetInvincible, so a Shield that loses the hero walks away still guarded until its next lunge clears it. That is the source, not a defect of this controller.",
                    "The Slash and Slash 2 sword hitboxes are not presented: the lunge damages by the body, the way the Runner already does, so an attack reaches the body box rather than about a unit and a half further.",
                    "Nothing of the corpse is presented. The source corpse prefab is a Zombie corpse that effects.py's Runner contract would likely admit, and wiring it is the one piece of this family left outside the guest.",
                    "Shield Counter is a per-actor deterministic sample of the source RandomInt(60, 100), not Unity RNG parity; one decrement a tick, including a tick that re-aims the shield.",
                    "Before the Walker starts, the guest holds Idle. The source animator has playAutomatically on with a default clip of Attack3 S3, a single frame it shows until something plays over it; the Walker's camera gate opens 60 units away, so nothing is ever on screen for that.",
                    "The Dust Kick emitter, the audio one shots and the SetZ depth are not presented.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
