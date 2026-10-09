//! Recognize the verified Windows Climber (Tiktik) and Moss Walker variants for
//! guest admission. Ported from host/climber.py.
//!
//! The controllers live in shared/hk-sim/src/climber.rs and moss_walker.rs; this
//! module admits only the placed instances whose serialized fields match the
//! audited contracts. Anything else stays an unsupported record.

use crate::common::{component_records, err, get, Result};
use crate::cook_audio::{jobj, js, u};
use crate::false_knight::fsm_digest;
use crate::pyjson::Json;
use crate::recog::{body_box, check_assemblies, clip_is, clips_by_name, near, only, variables_strict, xy};
use crate::runner::axis_aligned_bounds;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const CLIPS: [(&str, usize, f64, i64); 4] = [("Walk", 4, 10.0, 0), ("Stun", 7, 12.0, 1), ("Death Air", 8, 30.0, 0), ("Death Land", 3, 15.0, 2)];
const BODY_SIZE: [f64; 2] = [1.09375, 0.921875];
const BODY_OFFSET: [f64; 2] = [0.015625, 0.4765625];
/// Authored world rotations sit a fraction of a degree off the quarter turn
/// (level39:1069 is 179.999988), so the basis is compared against the exact
/// quarter it names rather than against the serialized angle.
const BASIS_TOLERANCE: f64 = 1e-5;

fn n(x: f64) -> Value {
    Value::F64(x)
}

/// `initial_quarter`: the quarter turn `Climber.Start` reads off the transform.
fn initial_quarter(sc: &Scene, gid: i64) -> Result<i64> {
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let scale = get(sc.transform(tid).ok_or("transform missing")?, "m_LocalScale")?;
    let matrix = u(sc.world(tid))?;
    let quarter = (crate::common::py_round(matrix[1][0].atan2(matrix[0][0]).to_degrees() / 90.0)).rem_euclid(4);
    let (cos, sin) = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][quarter as usize];
    let basis = [[cos, -sin], [sin, cos]];
    let scaled = ["x", "y", "z"].iter().all(|a| near(scale.get(a), 1.0));
    if !scaled || (0..2).any(|i| (0..2).any(|j| (matrix[i][j] - basis[i][j]).abs() > BASIS_TOLERANCE)) {
        return err("unsupported Climber initial rotation or scale");
    }
    Ok(quarter)
}

fn identity() -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// Admit the Climber placement at `gid`, or refuse with the first failed check.
pub fn recognize(sc: &Scene, source: &Source, gid: i64) -> Result<Json> {
    let records = component_records(sc, gid);
    let (_, climber) = only(&records, "Climber", "Climber")?;
    for (name, value) in [("speed", 2.0), ("spinTime", 0.25), ("wallRayPadding", 0.1), ("minTurnDistance", 0.25)] {
        if !climber.get("m_Enabled").is_some_and(Value::truthy) || !near(climber.get(name), value) {
            return err(format!("unsupported Climber field: {name}"));
        }
    }
    check_assemblies(source, "Climber")?;
    let quarter = initial_quarter(sc, gid)?;
    // Several placements carry the same box twice; the duplicate is field for
    // field the original, so the contact shape is the one box.
    let body = body_box(&records, "Climber")?;
    let (size, offset) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    if !get(body, "m_Enabled")?.truthy() || get(body, "m_IsTrigger")?.truthy() || get(body, "m_EdgeRadius")?.float() != Some(0.0) || (0..2).any(|k| !near(Some(&n(size[k])), BODY_SIZE[k]) || !near(Some(&n(offset[k])), BODY_OFFSET[k])) {
        return err("unsupported Climber body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Climber")?;
    if get(rigid, "m_BodyType")?.int() != Some(1) || get(rigid, "m_GravityScale")?.float() != Some(0.0) || get(rigid, "m_LinearDamping")?.float() != Some(0.0) || get(rigid, "m_Constraints")?.int() != Some(4) {
        return err("unsupported Climber rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Climber")?;
    if !get(recoil, "freezeInPlace")?.truthy() || get(recoil, "recoilSpeedBase")?.float() != Some(0.0) {
        return err("unsupported Climber recoil variant");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Climber")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        if !clip_is(by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v), frames, fps, wrap) {
            return err(format!("unsupported Climber animation: {name}"));
        }
    }
    let (_, audio) = only(&records, "AudioSource", "Climber")?;
    if !get(audio, "Loop")?.truthy() || !get(audio, "m_PlayOnAwake")?.truthy() {
        return err("unsupported Climber audio source");
    }
    let bounds = axis_aligned_bounds(&identity(), BODY_OFFSET, BODY_SIZE)?;
    let loop_clip = u(sc.deref(get(audio, "m_Resource")?))?;
    Ok(jobj(vec![
        ("kind", js("Climber")),
        ("guest_enabled", Json::Bool(true)),
        ("start_right", Json::Bool(get(climber, "startRight")?.truthy())),
        ("speed", Json::Float(2.0)),
        ("spin_time", Json::Float(0.25)),
        ("wall_ray_padding", Json::Float(0.1)),
        ("min_turn_distance", Json::Float(0.25)),
        ("body_bounds_local", Json::List(bounds.iter().map(|&f| Json::Float(f)).collect())),
        ("rotation_q16", Json::Int(quarter * 90 * 65536)),
        ("audio_sources", jobj(vec![("loop", Json::Str(loop_clip.sid())), ("volume", crate::music::value_json(get(audio, "m_Volume")?))])),
        (
            "limitations",
            Json::List(
                [
                    "Kinematic surface follower on the bounded terrain solver; Unity coroutine ordering and float rounding are not reproduced.",
                    "The live climb loop audio is not presented yet.",
                    "Stun freeze uses the seven-frame clip duration; Recoil.recoilDuration is not the source gate.",
                    "The authored rotation is snapped to its quarter turn; the placements are cardinal and the sub-degree serialized error is not carried.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}

// --- Moss Walker (Mosscreep, Greenpath) --------------------------------------
//
// The controller is shared/hk-sim/src/moss_walker.rs. All 13 placements carry
// one `Moss Walker` FSM (pinned by digest, its `Roams` bool aside) and one
// `Wake Range` FSM. Only floor placements are admitted: the three wall
// placements read, from the IL, as walkers whose rays point away from their
// wall and never turn, which has to be watched on the original before it is
// reproduced.
const MOSS_WALKER_FSM_SHA256: &str = "9f9a1cd537e40cf2e0e8e3a0a0012a456ceb87ff51760ca16871dca16049d2b4";
const MOSS_WAKE_FSM_SHA256: &str = "155198314430555fbfb03504d4a5075efc0578491226ff08583e2c7d6c285e29";
const MOSS_WALKER_CLIPS: [(&str, usize, f64, i64); 6] = [("Walk", 4, 12.0, 0), ("Turn", 3, 12.0, 2), ("Rest", 1, 30.0, 6), ("Shake", 3, 12.0, 0), ("Appear", 5, 10.0, 2), ("Bury", 5, 12.0, 2)];
const MOSS_WALKER_SLOTS: [(&str, &str); 4] = [("rest", "Rest"), ("shake", "Shake"), ("appear", "Appear"), ("bury", "Bury")];
const MOSS_WALKER_BODY: [[f64; 2]; 2] = [[1.261925458908081, 1.250787377357483], [-0.056549072265625, -0.2771453857421875]];
/// Child point rays and the wake circle, as shared/hk-sim/src/moss_walker.rs
/// carries them (EDGE/WALL/GROUND_ORIGIN, WAKE_RADIUS).
const MOSS_WALKER_CHILDREN: [(&str, f64, f64); 3] = [("Edge Range", -0.91, -0.65), ("Wall Range", -0.4, -0.38), ("Ground Range", 0.0, -0.31)];
const MOSS_WAKE_RADIUS: f64 = 8.27;

/// Admit a placed floor Moss Walker, or refuse with the reason. `health` is the
/// actor's HealthManager tree.
pub fn recognize_moss_walker(sc: &Scene, source: &Source, gid: i64, health: &Value) -> Result<Json> {
    let records = component_records(sc, gid);
    check_assemblies(source, "Moss Walker")?;
    let fsms: Vec<&Value> = records.iter().filter(|r| r.1 == "PlayMakerFSM").map(|r| r.2).collect();
    if fsms.len() != 1 || fsms[0].get("fsm").and_then(|f| f.get("name")).and_then(Value::str).as_deref() != Some("Moss Walker") {
        return err("no single Moss Walker FSM");
    }
    let fsm = get(fsms[0], "fsm")?;
    let digest = fsm_digest(fsm, &["Roams"])?;
    if digest != MOSS_WALKER_FSM_SHA256 {
        return err(format!("Moss Walker FSM changed: {digest}"));
    }
    let vars = variables_strict(fsm)?;
    let roams = vars.iter().find(|(k, _)| k == "Roams").and_then(|(_, v)| *v);
    if !roams.is_some_and(|v| v.int().is_some_and(|i| i == 0 || i == 1)) {
        return err("unsupported Moss Walker Roams");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let matrix = u(sc.world(tid))?;
    if (0..2).any(|i| (0..2).any(|j| (matrix[i][j] - if i == j { 1.0 } else { 0.0 }).abs() > 1e-6)) {
        return err("wall or roof Moss Walker: its IL ray directions await verification on the original");
    }
    let hp_ok = get(health, "hp")?.int() == Some(10);
    if !hp_ok
        || !get(health, "invincible")?.truthy()
        || get(health, "invincibleFromDirection")?.truthy()
        || !get(health, "preventInvincibleEffect")?.truthy()
        || get(health, "hasSpecialDeath")?.truthy()
        || get(health, "hasAlternateHitAnimation")?.truthy()
        || get(health, "damageOverride")?.truthy()
    {
        return err("unsupported Moss Walker HealthManager");
    }
    let body = body_box(&records, "Moss Walker")?;
    let (size, offset) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    if !get(body, "m_Enabled")?.truthy() || get(body, "m_IsTrigger")?.truthy() || get(body, "m_EdgeRadius")?.float() != Some(0.0) || (0..2).any(|k| !near(Some(&n(size[k])), MOSS_WALKER_BODY[0][k]) || !near(Some(&n(offset[k])), MOSS_WALKER_BODY[1][k])) {
        return err("unsupported Moss Walker body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Moss Walker")?;
    if get(rigid, "m_BodyType")?.int() != Some(1) || get(rigid, "m_Constraints")?.int() != Some(4) {
        return err("unsupported Moss Walker rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Moss Walker")?;
    if get(recoil, "freezeInPlace")?.truthy() || get(recoil, "recoilSpeedBase")?.float() != Some(15.0) || !near(recoil.get("recoilDuration"), 0.15) {
        return err("unsupported Moss Walker recoil");
    }
    let (_, damage) = only(&records, "DamageHero", "Moss Walker")?;
    let (_, bouncer) = only(&records, "NonBouncer", "Moss Walker")?;
    if get(damage, "damageDealt")?.int() != Some(0) || !get(bouncer, "active")?.truthy() {
        return err("Moss Walker does not start buried");
    }
    // The children keyed by name, in object order (the last of a name wins).
    let mut children: Vec<(String, &Value)> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let go = get(get(&o.tree, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        let name = get(sc.go(go).ok_or("child without a GameObject")?, "m_Name")?.str().unwrap_or_default();
        match children.iter_mut().find(|c| c.0 == name) {
            Some(slot) => slot.1 = &o.tree,
            None => children.push((name, &o.tree)),
        }
    }
    let child = |name: &str| children.iter().find(|c| c.0 == name).map(|c| c.1);
    for (name, x, y) in MOSS_WALKER_CHILDREN {
        let ok = child(name).and_then(|t| xy(t, "m_LocalPosition").ok()).is_some_and(|p| near(Some(&n(p[0])), x) && near(Some(&n(p[1])), y));
        if !ok {
            return err(format!("unsupported Moss Walker ray child: {name}"));
        }
    }
    let zero3 = Value::Map(vec![("x".into(), n(0.0)), ("y".into(), n(0.0)), ("z".into(), n(0.0))]);
    let zero2 = Value::Map(vec![("x".into(), n(0.0)), ("y".into(), n(0.0))]);
    let wake = child("Wake Range").filter(|w| w.get("m_LocalPosition").is_some_and(|p| p.py_eq(&zero3)) && w.get("m_LocalScale").and_then(|s| s.get("x")).and_then(Value::float) == Some(1.0));
    let Some(wake) = wake else { return err("unsupported Moss Walker Wake Range") };
    let wake_go = get(get(wake, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
    let wake_records = component_records(sc, wake_go);
    let (_, circle) = only(&wake_records, "CircleCollider2D", "Moss Walker")?;
    let wake_fsm: Vec<&Value> = wake_records.iter().filter(|r| r.1 == "PlayMakerFSM").filter_map(|r| r.2.get("fsm")).collect();
    if !get(circle, "m_IsTrigger")?.truthy() || !get(circle, "m_Offset")?.py_eq(&zero2) || !near(circle.get("m_Radius"), MOSS_WAKE_RADIUS) || wake_fsm.len() != 1 || fsm_digest(wake_fsm[0], &[])? != MOSS_WAKE_FSM_SHA256 {
        return err("unsupported Moss Walker Wake Range");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Moss Walker")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in MOSS_WALKER_CLIPS {
        if !clip_is(by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v), frames, fps, wrap) {
            return err(format!("unsupported Moss Walker animation: {name}"));
        }
    }
    let mut limitations = vec![
        "Floor placements only; the wall placements are refused until their rays are watched on the original.".to_string(),
        "The footstep loop, the emerge and look sounds and the grass particles are not presented.".to_string(),
    ];
    if !get(fsms[0], "m_Enabled")?.truthy() {
        limitations.push("Authored with its FSM disabled for FSMActivator; seated active like the others.".into());
    }
    let mut art = vec![("walk".to_string(), js("Walk")), ("turn".to_string(), js("Turn"))];
    art.extend(MOSS_WALKER_SLOTS.iter().map(|(k, v)| (k.to_string(), js(v))));
    Ok(jobj(vec![
        ("kind", js("MossWalker")),
        ("guest_enabled", Json::Bool(true)),
        ("roams", Json::Bool(roams.is_some_and(Value::truthy))),
        // Hidden and harmless until it wakes; the controller switches both.
        ("invincible", Json::Bool(false)),
        ("contact_damage", Json::Int(1)),
        ("art_bindings", Json::Obj(art)),
        ("limitations", Json::List(limitations.into_iter().map(Json::Str).collect())),
    ]))
}
