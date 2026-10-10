//! Recognize the Greenpath Moss Charger for guest admission. Ported from host/moss_charger.py.
//!
//! The controller lives in shared/hk-sim/src/moss_charger.rs; this module admits only a
//! placement whose serialized shape matches the one that was read to write it.
//!
//! Like the Plant Trap it has no collider of its own: tk2d builds one from the sprite
//! definition of the frame showing, so the boxes the controller holds are read off the sprite
//! definitions here and proved against the constants in `moss_charger.rs`. Everything its FSM
//! does with a number is pinned below: the structural digest says nothing changed, and these say
//! what the constants are, including the vectors and enums the digest does not cover.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::fsm_pins::{
    check_action, check_enums, check_transitions, check_vectors, enabled_actions, enabled_indices,
    fingerprint, state, variable, variables, Pin, PinRow, VecPin,
};
use crate::pyjson::Json;
use crate::recog::{
    check_assemblies, child_map, clip_is, clips_by_name, eq_field, flag, identity_basis, near,
    only, plain_sprite,
};
use crate::runner::ASSEMBLIES;
use hk_unity::playmaker::action_fields;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const CONTROL_FSM: &str = "Mossy Control";
/// `GRIMMKIN SPAWN` -> `Deactivate`: the Grimm Troupe clears this one out. One placement carries it;
/// the event is broadcast only when the Troupe arrives, which a fresh save never reaches.
const GRIMM_FSM: &str = "FSM";
const CONTROL_SHA256: &str = "9590bed6fcbb65d6a9f3e041f77a090d6881d5833ba29b786df9777db8dd39d6";
const GRIMM_SHA256: &str = "48907c6957a40c0b74b500a9f6231b243bda8825840723ac8cfd7478e252bda3";
const COMPONENTS: [&str; 21] = [
    "AudioSource",
    "DamageHero",
    "EnemyDeathEffects",
    "EnemyDreamnailReaction",
    "ExtraDamageable",
    "HealthManager",
    "InfectedEnemyEffects",
    "MeshFilter",
    "MeshRenderer",
    "NonBouncer",
    "ObjectBounce",
    "PersistentBoolItem",
    "PlayMakerFSM",
    "PlayMakerFixedUpdate",
    "Recoil",
    "Rigidbody2D",
    "SetZ",
    "SpriteFlash",
    "Transform",
    "tk2dSprite",
    "tk2dSpriteAnimator",
];
/// name: (frames, fps, wrapMode, loopStart)
const CLIPS: [(&str, usize, f64, i64, i64); 11] = [
    ("Appear", 6, 12.0, 2, 0),
    ("Charge", 4, 15.0, 0, 0),
    ("Disappear", 12, 12.0, 2, 0),
    ("Stun", 3, 12.0, 0, 0),
    ("Get Up", 4, 18.0, 2, 0),
    ("TurnRun", 6, 12.0, 1, 2),
    ("Run", 4, 30.0, 0, 0),
    ("Escape", 14, 12.0, 2, 0),
    ("Death Air", 2, 12.0, 2, 0),
    ("Grass Burst", 9, 12.0, 2, 0),
    ("Death Land", 2, 12.0, 2, 0),
];
/// The cooked clips, in `moss_charger::Clip` order.
pub(crate) const CLIP_SLOTS: [(&str, &str); 7] = [
    ("appear", "Appear"),
    ("charge", "Charge"),
    ("disappear", "Disappear"),
    ("stun", "Stun"),
    ("get_up", "Get Up"),
    ("turn_run", "TurnRun"),
    ("escape", "Escape"),
];
/// The frames whose trigger event is set, per clip.
fn triggers(name: &str) -> Vec<usize> {
    match name {
        "Disappear" | "Escape" => vec![5],
        _ => Vec::new(),
    }
}
/// Sprite-definition colliders [x0, y0, x1, y1] in Q16 relative to the body, as moss_charger.rs
/// holds them. Frames not listed define none.
const BIG: [i64; 4] = [-102400, -125952, 123904, 12288];
const BIG_LOW: [i64; 4] = [-102400, -125952, 123904, -51200];
const BIG_MID: [i64; 4] = [-102400, -125952, 123904, 48128];
const BIG_HIGH: [i64; 4] = [-102400, -125952, 123904, 62464];
const STUN: [i64; 4] = [-28672, -43008, 39936, 22528];
const RUN: [i64; 4] = [-28672, -52224, 39936, 22528];

fn collider(name: &str, index: usize) -> Option<[i64; 4]> {
    let frames: &[[i64; 4]] = match name {
        "Appear" => &[BIG; 6],
        "Charge" => &[BIG; 4],
        "Disappear" => &[BIG, BIG_LOW, BIG, BIG_MID, BIG_HIGH],
        "Stun" => &[STUN; 3],
        "Get Up" => &[STUN; 4],
        "TurnRun" => &[STUN, STUN, RUN, RUN, RUN, RUN],
        "Run" => &[RUN; 4],
        "Escape" => &[STUN; 6],
        "Death Air" => &[STUN, STUN],
        "Death Land" => &[STUN],
        _ => &[],
    };
    frames.get(index).copied()
}

use Pin::{F, I, S};
/// Pinned scalars.
#[rustfmt::skip]
const ACTIONS: &[PinRow] = &[
    ("Emerge Pause", "WaitRandom", &[("timeMin", F(0.5)), ("timeMax", F(1.0))]),
    ("Emerge", "FloatMultiply", &[("multiplyBy", F(0.25))]),
    ("Emerge", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Appear")), ("animationCompleteEvent", S("FINISHED"))]),
    ("Charge", "Tk2dPlayAnimation", &[("clipName", S("Charge"))]),
    ("Submerge", "Decelerate", &[("deceleration", F(0.7))]),
    ("Submerge", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Disappear")), ("animationTriggerEvent", S("FINISHED"))]),
    ("Submerge Grass effect", "Decelerate", &[("deceleration", F(0.7))]),
    ("Submerge CD", "Wait", &[("time", F(0.35))]),
    ("Line Loop", "IntCompare", &[("integer2", I(11)), ("greaterThan", S("LOOP COMPLETE"))]),
    ("Fly Left", "SetVelocityAsAngle", &[("angle", F(110.0)), ("speed", F(18.0))]),
    ("Fly Right", "SetVelocityAsAngle", &[("angle", F(70.0)), ("speed", F(18.0))]),
    ("FlyUp", "SetVelocityAsAngle", &[("angle", F(90.0)), ("speed", F(20.0))]),
    ("Fly Down", "SetVelocityAsAngle", &[("angle", F(270.0)), ("speed", F(10.0))]),
    ("Get Up", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Get Up")), ("animationCompleteEvent", S("FINISHED"))]),
    ("Run L", "AccelerateVelocity", &[("xAccel", F(-0.5)), ("xMaxSpeed", F(10.0))]),
    ("Run R", "AccelerateVelocity", &[("xAccel", F(0.5)), ("xMaxSpeed", F(10.0))]),
    ("Run L", "Wait", &[("time", F(1.0))]),
    ("Run R", "Wait", &[("time", F(1.0))]),
    ("Dig Start", "Decelerate", &[("deceleration", F(0.4))]),
    ("Dig Start", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Escape")), ("animationTriggerEvent", S("FINISHED"))]),
    ("Dig", "Tk2dWatchAnimationEvents", &[("animationCompleteEvent", S("FINISHED"))]),
];
/// Vector2 pins: (state, action, nth enabled instance) -> fields.
type VectorRow<'a> = (&'a str, &'a str, usize, &'a [(&'a str, VecPin)]);
#[rustfmt::skip]
const VECTORS: &[VectorRow] = &[
    ("Charge", "RayCast2d", 0, &[("fromPosition", VecPin::Lit(0.0, -0.5)), ("direction", VecPin::Var("RayForward Direction"))]),
    ("Charge", "RayCast2d", 1, &[("fromPosition", VecPin::Var("RayDown X")), ("direction", VecPin::Lit(0.0, -1.0))]),
    ("Run L", "RayCast2d", 0, &[("fromPosition", VecPin::Var("")), ("direction", VecPin::Lit(-1.0, 0.0))]),
    ("Run L", "RayCast2d", 1, &[("fromPosition", VecPin::Lit(-3.0, 0.0)), ("direction", VecPin::Lit(0.0, -1.0))]),
    ("Run R", "RayCast2d", 0, &[("fromPosition", VecPin::Var("")), ("direction", VecPin::Lit(1.0, 0.0))]),
    ("Run R", "RayCast2d", 1, &[("fromPosition", VecPin::Lit(3.0, 0.0)), ("direction", VecPin::Lit(0.0, -1.0))]),
];
/// `SetVector2XY` in order: (state, nth) -> (variable, x, y). The ray offsets are world offsets
/// ahead of the charge, so `Emerge Right` (charging left) and `Emerge Left` mirror each other.
const SETTERS: [(&str, usize, &str, f64, f64); 4] = [
    ("Emerge Right", 0, "RayDown X", -6.5, -0.5),
    ("Emerge Right", 1, "RayForward Direction", -1.0, 0.0),
    ("Emerge Left", 0, "RayForward Direction", 1.0, 0.0),
    ("Emerge Left", 1, "RayDown X", 6.5, -0.5),
];
/// RayCast2d distances (state, nth) -> metres, and the enums: space Self = 1.
const RAY_DISTANCES: [(&str, usize, f64); 6] = [
    ("Charge", 0, 5.5),
    ("Charge", 1, 3.0),
    ("Run L", 0, 2.0),
    ("Run L", 1, 1.3),
    ("Run R", 0, 2.0),
    ("Run R", 1, 1.3),
];
/// Init's FloatOperator enum values in order: X Min -= length, X Max += length, X Min += 2, X Max -= 2.
const INIT_OPERATIONS: [i32; 4] = [1, 0, 0, 1];
#[rustfmt::skip]
const TRANSITIONS: &[(&str, &[(&str, &str)])] = &[
    ("Init", &[("FINISHED", "Hidden")]),
    ("Hidden", &[("IN RANGE", "Emerge Pause")]),
    ("Emerge Pause", &[("FINISHED", "Hero Beyond?")]),
    ("Hero Beyond?", &[("CANCEL", "Hidden"), ("FINISHED", "Left or Right?")]),
    ("Left or Right?", &[("LEFT", "Emerge Left"), ("RIGHT", "Emerge Right")]),
    ("Emerge Right", &[("FINISHED", "Emerge"), ("LEFT", "Pause")]),
    ("Emerge Left", &[("FINISHED", "Emerge"), ("RIGHT", "Pause 2")]),
    ("Emerge", &[("FINISHED", "Charge")]),
    ("Charge", &[("SUBMERGE", "Submerge"), ("TAKE DAMAGE", "Line Loop")]),
    ("Submerge", &[("FINISHED", "Submerge Grass effect")]),
    ("Submerge Grass effect", &[("FINISHED", "Submerge CD")]),
    ("Submerge CD", &[("FINISHED", "Play Range")]),
    ("Play Range", &[("FINISHED", "Hidden")]),
    ("Line Loop", &[("FINISHED", "State 2"), ("LOOP COMPLETE", "Burst")]),
    ("Burst", &[("LEFT", "Fly Left"), ("RIGHT", "Fly Right"), ("UP", "FlyUp"), ("DOWN", "Fly Down")]),
    ("Fly Left", &[("FINISHED", "In Air")]),
    ("Fly Right", &[("FINISHED", "In Air")]),
    ("FlyUp", &[("FINISHED", "In Air")]),
    ("Fly Down", &[("FINISHED", "In Air")]),
    ("In Air", &[("DOWN", "Land")]),
    ("Land", &[("FINISHED", "Get Up")]),
    ("Get Up", &[("FINISHED", "Direction")]),
    ("Direction", &[("LEFT", "Run R"), ("RIGHT", "Run L")]),
    ("Run L", &[("LEFT", "Run R"), ("SUBMERGE", "On Ground?")]),
    ("Run R", &[("RIGHT", "Run L"), ("SUBMERGE", "On Ground?")]),
    ("On Ground?", &[("DOWN", "Dig Start"), ("FINISHED", "Dig Start")]),
    ("Dig Start", &[("FINISHED", "Dig"), ("FALL", "In Air")]),
    ("Dig", &[("FINISHED", "Submerge CD"), ("FALL", "In Air")]),
];
const GLOBALS: &[(&str, &str)] = &[("ZERO HP", "Detach"), ("BLOCKED HIT", "Line Loop")];

fn q16(v: f64) -> i64 {
    py_round(v * 65536.0)
}

/// `_fsms`: the `Mossy Control` FSM, with the Grimm Troupe's `FSM` tolerated beside it.
fn fsms<'a>(records: &[(i64, &str, &'a Value)]) -> Result<&'a Value> {
    let all: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .map(|r| get(r.2, "fsm"))
        .collect::<Result<_>>()?;
    let mut found: Vec<(String, &Value)> = Vec::new();
    for fsm in &all {
        let name = get(fsm, "name")?.str().unwrap_or_default();
        match found.iter_mut().find(|k| k.0 == name) {
            Some(slot) => slot.1 = fsm,
            None => found.push((name, fsm)),
        }
    }
    let extra = found
        .iter()
        .position(|k| k.0 == GRIMM_FSM)
        .map(|i| found.remove(i).1);
    if found.len() != 1
        || found[0].0 != CONTROL_FSM
        || all.len() != 1 + usize::from(extra.is_some())
    {
        return err("no single Moss Charger Mossy Control FSM");
    }
    if let Some(extra) = extra {
        if fingerprint(extra)? != GRIMM_SHA256 {
            return err("unverified Moss Charger extra FSM");
        }
    }
    let control = found[0].1;
    if get(control, "startState")?.str().as_deref() != Some("Init Pause")
        || fingerprint(control)? != CONTROL_SHA256
    {
        return err("unverified Moss Charger FSM variant");
    }
    check_transitions("Moss Charger", control, TRANSITIONS, GLOBALS)?;
    Ok(control)
}

/// `_pins`: every number the FSM carries, read back.
fn pins(control: &Value) -> Result<()> {
    const WHO: &str = "Moss Charger";
    for &(st, action, expected) in ACTIONS {
        check_action(WHO, state(control, st)?, action, expected)?;
    }
    for &(st, action, nth, expected) in VECTORS {
        if !expected.is_empty() {
            check_vectors(WHO, state(control, st)?, action, nth, expected)?;
        }
    }
    for (st, nth, variable_name, x, y) in SETTERS {
        let s = state(control, st)?;
        check_vectors(
            WHO,
            s,
            "SetVector2XY",
            nth,
            &[("vector2Variable", VecPin::Var(variable_name))],
        )?;
        let fields = enabled_actions(s, "SetVector2XY")?;
        let fields = fields.get(nth).ok_or("SetVector2XY")?;
        let value = |k: &str| -> Option<&Value> {
            hk_unity::playmaker::field(fields, k).and_then(|f| f.get("value"))
        };
        if !near(value("x"), x) || !near(value("y"), y) {
            return err(format!(
                "unsupported {WHO} parameter: {st}/SetVector2XY #{nth}"
            ));
        }
    }
    for (st, nth, distance) in RAY_DISTANCES {
        let s = state(control, st)?;
        let found = enabled_indices(s, "RayCast2d");
        let index = *found.get(nth).ok_or("RayCast2d")?;
        let fields =
            action_fields(get(s, "actionData")?, index, false).map_err(|e| e.to_string())?;
        if !near(
            hk_unity::playmaker::field(&fields, "distance").and_then(|f| f.get("value")),
            distance,
        ) {
            return err(format!(
                "unsupported {WHO} parameter: {st}/RayCast2d #{nth}.distance"
            ));
        }
        check_enums(WHO, s, "RayCast2d", nth, &[("space", 1)])?;
    }
    let init = state(control, "Init")?;
    if enabled_indices(init, "FloatOperator").len() != 4 {
        return err("unsupported Moss Charger Init operators");
    }
    for (nth, want) in INIT_OPERATIONS.iter().enumerate() {
        check_enums(WHO, init, "FloatOperator", nth, &[("operation", *want)])?;
    }
    let plain = variables(control);
    if !near(variable(&plain, "Charge Speed"), 15.0) {
        return err("unsupported Moss Charger Charge Speed");
    }
    Ok(())
}

/// `_range_box`: `Attack Range`, the trigger box the Knight must stand in, detached at the tuft.
fn range_box(sc: &Scene, gid: i64, origin: [f64; 2]) -> Result<[i64; 4]> {
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let children = child_map(sc, tid)?;
    let Some(&(_, (kid, ktid))) = children.iter().find(|c| c.0 == "Attack Range") else {
        return err("Moss Charger has no Attack Range");
    };
    let records = component_records(sc, kid);
    let boxes: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    if get(sc.go(kid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(13)
        || boxes.len() != 1
        || !flag(boxes[0], "m_IsTrigger")?
        || !flag(boxes[0], "m_Enabled")?
        || !records.iter().any(|r| r.1 == "AlertRange")
    {
        return err("unsupported Moss Charger Attack Range");
    }
    let m = u(sc.world(ktid))?;
    if m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 || m[0][0] <= 0.0 || m[1][1] <= 0.0 {
        return err("rotated or mirrored Moss Charger Attack Range");
    }
    let (size, offset) = (
        crate::recog::xy(boxes[0], "m_Size")?,
        crate::recog::xy(boxes[0], "m_Offset")?,
    );
    let cx = m[0][3] + m[0][0] * offset[0] - origin[0];
    let cy = m[1][3] + m[1][1] * offset[1] - origin[1];
    let (hx, hy) = (m[0][0] * size[0] / 2.0, m[1][1] * size[1] / 2.0);
    Ok([q16(cx - hx), q16(cy - hy), q16(cx + hx), q16(cy + hy)])
}

/// `_clips`: the library's clip contract and the collider boxes of its frames.
fn clips(sc: &Scene, source: &Source, animator: &Value) -> Result<hk_unity::Obj> {
    if !flag(animator, "m_Enabled")?
        || flag(animator, "isRealtime")?
        || flag(animator, "playAutomatically")?
    {
        return err("Moss Charger requires enabled scaled-time animation it starts itself");
    }
    let library_o = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_o))?;
    let by_name = clips_by_name(&library)?;
    let mut collections: Vec<(i64, Value)> = Vec::new();
    for (name, frames, fps, wrap, loop_start) in CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        if !clip_is(clip, frames, fps, wrap)
            || clip
                .and_then(|c| c.get("loopStart"))
                .map_or(0, |v| v.int().unwrap_or(-1))
                != loop_start
        {
            return err(format!("unsupported Moss Charger animation: {name}"));
        }
        let clip = clip.unwrap();
        let listed = get(clip, "frames")?.list().unwrap_or(&[]);
        let fired: Vec<usize> = listed
            .iter()
            .enumerate()
            .filter(|(_, f)| f.get("triggerEvent").is_some_and(Value::truthy))
            .map(|(i, _)| i)
            .collect();
        if fired != triggers(name) {
            return err(format!("unsupported Moss Charger frame event: {name}"));
        }
        if name == "Grass Burst" {
            continue;
        }
        for (index, frame) in listed.iter().enumerate() {
            let collection_o = u(source.deref(&library_o.file, get(frame, "spriteCollection")?))?;
            if !collections.iter().any(|c| c.0 == collection_o.path_id()) {
                collections.push((collection_o.path_id(), u(source.read(&collection_o))?));
            }
            let collection = &collections
                .iter()
                .find(|c| c.0 == collection_o.path_id())
                .unwrap()
                .1;
            let definition = get(collection, "spriteDefinitions")?
                .list()
                .unwrap_or(&[])
                .get(get(frame, "spriteId")?.int().unwrap_or(-1) as usize)
                .ok_or("sprite definition")?;
            if !eq_field(definition, "physicsEngine", &Value::Int(1))? {
                return err(format!(
                    "Moss Charger sprite is not a 2D physics sprite: {name}"
                ));
            }
            let collider_type = get(definition, "colliderType")?;
            let Some(want) = collider(name, index) else {
                if collider_type.py_eq(&Value::Int(2)) {
                    return err(format!(
                        "unexpected Moss Charger collider: {name} frame {index}"
                    ));
                }
                continue;
            };
            let actual = crate::plant_trap::definition_box(definition)?;
            if !collider_type.py_eq(&Value::Int(2)) || actual != want {
                return err(format!(
                    "Moss Charger collider {name} frame {index} {actual:?} is not the admitted {want:?}"
                ));
            }
        }
    }
    Ok(library_o)
}

/// A tuft that surfaces beside the Knight and charges across the room.
pub fn recognize(
    sc: &Scene,
    source: &Source,
    gid: i64,
    position: [f64; 3],
    health: &Value,
) -> Result<Json> {
    check_assemblies(source, "Moss Charger")?;
    let records = component_records(sc, gid);
    let control = fsms(&records)?;
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    if kinds.iter().filter(|k| **k == "PlayMakerFSM").count() == 2 {
        let i = kinds.iter().position(|k| *k == "PlayMakerFSM").unwrap();
        kinds.remove(i);
    }
    if kinds != COMPONENTS {
        return err("unsupported Moss Charger component set");
    }
    let layer = get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?
        .int()
        .unwrap_or(-1);
    if layer != 11 {
        return err(format!(
            "Moss Charger outside the enemy layer: layer {layer}"
        ));
    }
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if !identity_basis(&matrix) {
        return err("unsupported Moss Charger rotation or scale");
    }
    if position[2].abs() > 0.01 {
        return err("Moss Charger depth differs from guest source plane");
    }
    pins(control)?;
    let mut bad_health = !eq_field(health, "hp", &Value::Int(15))?
        || !eq_field(health, "smallGeoDrops", &Value::Int(8))?
        || !flag(health, "invincible")?
        || !flag(health, "preventInvincibleEffect")?;
    for key in [
        "invincibleFromDirection",
        "hasSpecialDeath",
        "hasAlternateHitAnimation",
        "damageOverride",
        "megaFlingGeo",
        "mediumGeoDrops",
        "largeGeoDrops",
    ] {
        bad_health |= flag(health, key)?;
    }
    if bad_health {
        return err("unsupported Moss Charger HealthManager variant");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Moss Charger")?;
    if !eq_field(rigid, "m_BodyType", &Value::Int(0))?
        || !flag(rigid, "m_Simulated")?
        || flag(rigid, "m_UseAutoMass")?
        || !eq_field(rigid, "m_Mass", &Value::Int(10))?
        || !eq_field(rigid, "m_GravityScale", &Value::Int(0))?
        || !eq_field(rigid, "m_LinearDamping", &Value::Int(0))?
        || !eq_field(rigid, "m_Constraints", &Value::Int(4))?
    {
        return err("unsupported Moss Charger rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Moss Charger")?;
    if flag(recoil, "freezeInPlace")?
        || !eq_field(recoil, "recoilSpeedBase", &Value::Int(15))?
        || !near(recoil.get("recoilDuration"), 0.15)
        || !flag(recoil, "preventRecoilUp")?
        || flag(recoil, "stopVelocityXWhenRecoilingUp")?
    {
        return err("unsupported Moss Charger recoil variant");
    }
    let (_, damage) = only(&records, "DamageHero", "Moss Charger")?;
    if !eq_field(damage, "damageDealt", &Value::Int(1))?
        || !eq_field(damage, "hazardType", &Value::Int(1))?
        || !flag(damage, "m_Enabled")?
    {
        return err("unsupported Moss Charger contact damage");
    }
    // `actor[kind]` is the last component of the kind.
    let last = |kind: &str| -> Result<&Value> {
        records
            .iter()
            .rev()
            .find(|r| r.1 == kind)
            .map(|r| r.2)
            .ok_or_else(|| format!("missing {kind}"))
    };
    if !plain_sprite(last("tk2dSprite")?)? {
        return err("unsupported Moss Charger sprite");
    }
    let library_o = clips(sc, source, last("tk2dSpriteAnimator")?)?;
    let range = range_box(sc, gid, [position[0], position[1]])?;
    let mut bindings = vec![("walk", js("Appear")), ("turn", js("Appear"))];
    bindings.extend(CLIP_SLOTS.iter().map(|(slot, clip)| (*slot, js(clip))));
    Ok(jobj(vec![
        ("kind", js("MossCharger")),
        ("guest_enabled", Json::Bool(true)),
        ("bounds_q16", Json::List(BIG.iter().map(|&v| Json::Int(v)).collect())),
        ("range_q16", Json::List(range.iter().map(|&v| Json::Int(v)).collect())),
        ("fsm_sha256", jobj(vec![(CONTROL_FSM, js(CONTROL_SHA256))])),
        ("assemblies_sha256", Json::Obj(ASSEMBLIES.iter().map(|(k, h)| (k.to_string(), js(h))).collect())),
        ("library_source", Json::Str(library_o.sid())),
        ("art_bindings", jobj(bindings)),
        (
            "limitations",
            Json::List(
                [
                    "The hurt box is the collider tk2d builds for the frame showing, read from the sprite definitions; the charge is a kinematic slide on its ground line and the burst and the run use the bounded gravity body.",
                    "The Dig Check child, grass puffs, hit effects, camera shake and every sound are not presented.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
