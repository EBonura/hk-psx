//! Recognize the Greenpath Fat Fly for guest admission. Ported from host/fat_fly.py.
//!
//! The controller lives in shared/hk-sim/src/fat_fly.rs; this module admits only a
//! placement whose serialized shape matches the one that was read to write it.
//!
//! A Fat Fly is a Gruzzer's bounce with an attack bolted on: `fat fly bounce` is
//! `Bouncer Control` woken by the Knight coming within 25 units, and `Fatty Fly Attack`
//! slows it to a halt, plays `Attack` and flings four `Spitter Shot R` out on the
//! diagonals. Both FSMs are pinned by structural digest, and the numbers the controller
//! holds are proven against the source.

use crate::common::{component_records, err, get, Result};
use crate::cook_audio::{jobj, js, u};
use crate::fsm_pins::{
    check_action, check_transitions, enabled_actions, state, variable, variables, Pin, PinRow,
};
use crate::pyjson::Json;
use crate::recog::{
    check_assemblies, clip_is, clips_by_name, eq_field, flag, identity_basis, near, only,
};
use crate::runner::{axis_aligned_bounds, fsm_fingerprint, ASSEMBLIES};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const BOUNCE_FSM: &str = "fat fly bounce";
const ATTACK_FSM: &str = "Fatty Fly Attack";
const BOUNCE_SHA256: &str = "dc13fb09efd63d26cd2da8e309eb2beea04febe322bac69fa6bca2b2c73011fa";
const ATTACK_SHA256: &str = "f3f18d310f8661b2997a7c3aa8c85276be8f721cccf822a953d3723115df3279";
const COMPONENTS: [&str; 25] = [
    "AudioSource",
    "BoxCollider2D",
    "DamageHero",
    "EnemyDeathEffects",
    "EnemyDreamnailReaction",
    "ExtraDamageable",
    "FSMActivator",
    "HealthManager",
    "InfectedEnemyEffects",
    "MeshFilter",
    "MeshRenderer",
    "PersonalObjectPool",
    "PlayMakerCollisionEnter2D",
    "PlayMakerCollisionStay2D",
    "PlayMakerFSM",
    "PlayMakerFSM",
    "PlayMakerFixedUpdate",
    "PlayMakerLateUpdate",
    "Recoil",
    "Rigidbody2D",
    "SetZ",
    "SpriteFlash",
    "Transform",
    "tk2dSprite",
    "tk2dSpriteAnimator",
];
const BODY_SIZE: [f64; 2] = [0.890625, 0.84375];
const BODY_OFFSET: [f64; 2] = [0.0390625, -0.03125];
/// name: (frames, fps, wrapMode). `Attack` fires from its sixth frame.
const CLIPS: [(&str, usize, f64, i64); 4] = [
    ("Fly", 8, 12.0, 0),
    ("Attack", 8, 12.0, 2),
    ("Death Air", 2, 12.0, 2),
    ("Death Land", 1, 30.0, 0),
];
const ATTACK_TRIGGER_FRAME: usize = 5;
const SHOT_CLIPS: [(&str, usize, f64, i64); 2] = [("Idle", 4, 20.0, 0), ("Impact", 6, 20.0, 2)];

use Pin::{Var, F, S};
/// The authored numbers shared/hk-sim/src/fat_fly.rs holds, read back rather than trusted.
#[rustfmt::skip]
const BOUNCE_ACTIONS: &[PinRow] = &[
    ("Initialise", "FloatCompare", &[("float2", F(25.0)), ("lessThan", S("FINISHED"))]),
    ("Fly 2", "SetVelocityAsAngle", &[("speed", Var("Speed"))]),
];
#[rustfmt::skip]
const ATTACK_ACTIONS: &[PinRow] = &[
    ("Wait", "WaitRandom", &[("timeMin", F(2.0)), ("timeMax", F(3.0))]),
    ("Attack Antic", "Wait", &[("time", F(0.35))]),
    ("Attack Antic", "Decelerate", &[("deceleration", F(0.1))]),
    ("Attack Antic 2", "Decelerate", &[("deceleration", F(0.1))]),
    ("Attack Antic 2", "Tk2dPlayAnimationWithEvents", &[("clipName", S("Attack")), ("animationTriggerEvent", S("FINISHED")), ("animationCompleteEvent", S("FINISHED"))]),
    ("Attack", "Wait", &[("time", F(0.5))]),
    ("CD", "Wait", &[("time", F(0.5))]),
    ("CD", "Tk2dPlayAnimation", &[("clipName", S("Fly"))]),
];
/// `Attack`'s four FlingObjectsFromGlobalPool, in authored order.
const SHOT_ANGLES: [f64; 4] = [45.0, 135.0, 225.0, 315.0];
const SHOT_SPEED: f64 = 12.0;
type Transitions<'a> = &'a [(&'a str, &'a [(&'a str, &'a str)])];
const BOUNCE_TRANSITIONS: Transitions = &[
    ("Initialise", &[("FINISHED", "Aim")]),
    ("Aim", &[("FINISHED", "Left or Right?")]),
    ("Stopped", &[("WAKE", "Left or Right?")]),
    ("Fly 2", &[("COLLISION STAY 2D", "Collision Check")]),
];
const ATTACK_TRANSITIONS: Transitions = &[
    ("Sleep", &[("START", "Wait")]),
    ("Wait", &[("FINISHED", "Attack Antic")]),
    ("Attack Antic", &[("FINISHED", "Attack Antic 2")]),
    ("Attack Antic 2", &[("FINISHED", "Attack")]),
    ("Attack", &[("FINISHED", "CD")]),
    ("CD", &[("FINISHED", "Wait")]),
];
const BOUNCE_GLOBALS: &[(&str, &str)] = &[("STOP", "Stopped"), ("GO UP", "Go Up")];
const ATTACK_GLOBALS: &[(&str, &str)] = &[("TAKE DAMAGE", "Attack Antic")];

fn n(x: f64) -> Value {
    Value::F64(x)
}

/// `_fsms`: the bounce and the attack FSM, each pinned by digest and transitions.
fn fsms<'a>(records: &[(i64, &str, &'a Value)]) -> Result<(&'a Value, &'a Value)> {
    let mut found: Vec<(String, Vec<&Value>)> = Vec::new();
    for r in records.iter().filter(|r| r.1 == "PlayMakerFSM") {
        let name = get(get(r.2, "fsm")?, "name")?.str().unwrap_or_default();
        match found.iter_mut().find(|f| f.0 == name) {
            Some(slot) => slot.1.push(r.2),
            None => found.push((name, vec![r.2])),
        }
    }
    let mut names: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
    names.sort();
    if names != [ATTACK_FSM, BOUNCE_FSM] || found.iter().any(|f| f.1.len() != 1) {
        return err("no single Fat Fly bounce and attack FSM pair");
    }
    let mut picked = Vec::new();
    for (name, digest, start, transitions, globals) in [
        (
            BOUNCE_FSM,
            BOUNCE_SHA256,
            "Initialise",
            BOUNCE_TRANSITIONS,
            BOUNCE_GLOBALS,
        ),
        (
            ATTACK_FSM,
            ATTACK_SHA256,
            "Sleep",
            ATTACK_TRANSITIONS,
            ATTACK_GLOBALS,
        ),
    ] {
        // Serialized disabled: `FSMActivator` enables both when the enemy is activated.
        let component = found.iter().find(|f| f.0 == name).unwrap().1[0];
        let fsm = get(component, "fsm")?;
        if get(fsm, "startState")?.str().as_deref() != Some(start)
            || fsm_fingerprint(fsm)? != digest
        {
            return err(format!("unverified Fat Fly FSM variant: {name}"));
        }
        check_transitions("Fat Fly", fsm, transitions, globals)?;
        picked.push(fsm);
    }
    Ok((picked[0], picked[1]))
}

fn check_rows(fsm: &Value, rows: &[PinRow]) -> Result<()> {
    for &(st, action, expected) in rows {
        check_action("Fat Fly", state(fsm, st)?, action, expected)?;
    }
    Ok(())
}

/// `_actions`.
fn actions(bounce: &Value, attack: &Value) -> Result<()> {
    check_rows(bounce, BOUNCE_ACTIONS)?;
    check_rows(attack, ATTACK_ACTIONS)?;
    let vars = variables(bounce);
    if !near(variable(&vars, "Speed"), 4.0)
        || !variable(&vars, "Starts Inactive").is_some_and(|v| v.py_eq(&Value::Int(0)))
    {
        return err("unsupported Fat Fly bounce variables");
    }
    Ok(())
}

/// `_volley`: `Attack`'s four flings, the pooled shot, the angle and the speed of each.
fn volley(attack: &Value) -> Result<Value> {
    let flings = enabled_actions(state(attack, "Attack")?, "FlingObjectsFromGlobalPool")?;
    if flings.len() != 4 {
        return err("unsupported Fat Fly volley size");
    }
    let mut prefab: Option<Value> = None;
    for (fields, angle) in flings.iter().zip(SHOT_ANGLES) {
        let field = |key: &str| -> Result<&Value> {
            hk_unity::playmaker::field(fields, key).ok_or_else(|| format!("missing field {key}"))
        };
        // speedMin/speedMax are bound to `Shot Speed`; the variable is 12.
        for key in ["angleMin", "angleMax"] {
            if !near(field(key)?.get("value"), angle) {
                return err("unsupported Fat Fly shot angle");
            }
        }
        for key in ["speedMin", "speedMax"] {
            let f = field(key)?;
            if get(f, "name")?.str().as_deref() != Some("Shot Speed")
                || !get(f, "useVariable")?.truthy()
            {
                return err("unsupported Fat Fly shot speed");
            }
        }
        let zero =
            |key: &str| -> Result<bool> { Ok(get(field(key)?, "value")?.py_eq(&Value::Int(0))) };
        let one =
            |key: &str| -> Result<bool> { Ok(get(field(key)?, "value")?.py_eq(&Value::Int(1))) };
        if !one("spawnMin")?
            || !one("spawnMax")?
            || !zero("originVariationX")?
            || !zero("originVariationY")?
        {
            return err("unsupported Fat Fly shot spread");
        }
        let reference = get(field("gameObject")?, "value")?;
        if prefab.as_ref().is_some_and(|p| !reference.py_eq(p)) {
            return err("Fat Fly volley mixes shot prefabs");
        }
        prefab = Some(reference.clone());
    }
    let shot_speed = variables(attack);
    if !near(variable(&shot_speed, "Shot Speed"), SHOT_SPEED) {
        return err("unsupported Fat Fly shot speed variable");
    }
    prefab.ok_or_else(|| "unsupported Fat Fly volley size".to_string())
}

/// `_shot`: the pooled `Spitter Shot R`, exactly as the Aspid validates it.
fn shot(sc: &Scene, source: &Source, prefab: &Value) -> Result<Json> {
    let obj = u(sc.deref(prefab))?;
    let go = u(source.read(&obj))?;
    if get(&go, "m_Name")?.str().as_deref() != Some("Spitter Shot R") {
        return err("unsupported Fat Fly shot prefab");
    }
    let mut parts: Vec<(String, Value)> = Vec::new();
    for r in get(&go, "m_Component")?.list().unwrap_or(&[]) {
        let component = u(source.deref(&obj.file, get(r, "component")?))?;
        let kind = u(source.typename(&component))?;
        let tree = u(source.read(&component))?;
        match parts.iter_mut().find(|p| p.0 == kind) {
            Some(slot) => slot.1 = tree,
            None => parts.push((kind, tree)),
        }
    }
    let part = |k: &str| -> Result<&Value> {
        parts
            .iter()
            .find(|p| p.0 == k)
            .map(|p| &p.1)
            .ok_or_else(|| "unsupported Fat Fly shot component set".to_string())
    };
    for kind in [
        "Rigidbody2D",
        "BoxCollider2D",
        "DamageHero",
        "EnemyBullet",
        "tk2dSpriteAnimator",
        "tk2dSprite",
        "Transform",
    ] {
        part(kind)?;
    }
    if !near(part("Rigidbody2D")?.get("m_GravityScale"), 0.05)
        || !eq_field(part("DamageHero")?, "damageDealt", &Value::Int(1))?
    {
        return err("unsupported Fat Fly shot body");
    }
    let bx = part("BoxCollider2D")?;
    let (size, offset) = (get(bx, "m_Size")?, get(bx, "m_Offset")?);
    if !near(size.get("x"), 0.640625)
        || !near(size.get("y"), 0.5625)
        || !near(offset.get("x"), 0.0078125)
        || !near(offset.get("y"), 0.0)
    {
        return err("unsupported Fat Fly shot box");
    }
    let library_o = u(source.deref(&obj.file, get(part("tk2dSpriteAnimator")?, "library")?))?;
    let library = u(source.read(&library_o))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in SHOT_CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        if !clip_is(clip, frames, fps, wrap) {
            return err(format!("unsupported Fat Fly shot animation: {name}"));
        }
    }
    let scale = get(get(part("Transform")?, "m_LocalScale")?, "x")?
        .float()
        .ok_or("m_LocalScale")?
        * get(part("EnemyBullet")?, "scaleMin")?
            .float()
            .ok_or("scaleMin")?;
    Ok(jobj(vec![
        ("source", Json::Str(obj.sid())),
        ("library", Json::Str(library_o.sid())),
        // The cook holds the library object itself; the dump names its type.
        ("library_object", js("<ObjectReader>")),
        ("scale", Json::Float(scale)),
    ]))
}

/// Admit the placement at `gid`, or refuse with the first failed check.
pub fn recognize(
    sc: &Scene,
    source: &Source,
    gid: i64,
    position: [f64; 3],
    health: &Value,
) -> Result<Json> {
    check_assemblies(source, "Fat Fly")?;
    let records = component_records(sc, gid);
    let (bounce, attack) = fsms(&records)?;
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    if kinds != COMPONENTS {
        return err("unsupported Fat Fly component set");
    }
    let layer = get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?
        .int()
        .unwrap_or(-1);
    if layer != 11 {
        return err(format!("Fat Fly outside the enemy layer: layer {layer}"));
    }
    let matrix = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if !identity_basis(&matrix) {
        return err("unsupported Fat Fly initial rotation or scale");
    }
    if position[2].abs() > 0.01 {
        return err("Fat Fly depth differs from guest source plane");
    }
    actions(bounce, attack)?;
    let mut bad_health = !eq_field(health, "hp", &Value::Int(10))?
        || !eq_field(health, "smallGeoDrops", &Value::Int(4))?;
    for key in [
        "invincible",
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
        return err("unsupported Fat Fly HealthManager variant");
    }
    let (_, body) = only(&records, "BoxCollider2D", "Fat Fly")?;
    let size = crate::recog::xy(body, "m_Size")?;
    let offset = crate::recog::xy(body, "m_Offset")?;
    if !flag(body, "m_Enabled")?
        || flag(body, "m_IsTrigger")?
        || !eq_field(body, "m_EdgeRadius", &Value::Int(0))?
        || (0..2).any(|k| {
            !near(Some(&n(size[k])), BODY_SIZE[k]) || !near(Some(&n(offset[k])), BODY_OFFSET[k])
        })
    {
        return err("unsupported Fat Fly body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Fat Fly")?;
    if !eq_field(rigid, "m_BodyType", &Value::Int(0))?
        || !flag(rigid, "m_Simulated")?
        || flag(rigid, "m_UseAutoMass")?
        || !eq_field(rigid, "m_Mass", &Value::Int(1))?
        || !eq_field(rigid, "m_GravityScale", &Value::Int(0))?
        || !eq_field(rigid, "m_LinearDamping", &Value::Int(0))?
        || !eq_field(rigid, "m_Constraints", &Value::Int(4))?
    {
        return err("unsupported Fat Fly rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Fat Fly")?;
    if flag(recoil, "freezeInPlace")?
        || !eq_field(recoil, "recoilSpeedBase", &Value::Int(15))?
        || !near(recoil.get("recoilDuration"), 0.15)
        || flag(recoil, "preventRecoilUp")?
    {
        return err("unsupported Fat Fly recoil variant");
    }
    let (_, damage) = only(&records, "DamageHero", "Fat Fly")?;
    if !eq_field(damage, "damageDealt", &Value::Int(1))?
        || !eq_field(damage, "hazardType", &Value::Int(1))?
        || !flag(damage, "m_Enabled")?
    {
        return err("unsupported Fat Fly contact damage");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Fat Fly")?;
    if !flag(animator, "m_Enabled")? || flag(animator, "isRealtime")? {
        return err("Fat Fly requires enabled scaled-time animation");
    }
    let library_o = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_o))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        if !clip_is(clip, frames, fps, wrap)
            || clip
                .and_then(|c| c.get("loopStart"))
                .is_some_and(|v| !v.py_eq(&Value::Int(0)))
        {
            return err(format!("unsupported Fat Fly animation: {name}"));
        }
        let triggers: Vec<usize> = get(clip.unwrap(), "frames")?
            .list()
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .filter(|(_, f)| f.get("triggerEvent").is_some_and(Value::truthy))
            .map(|(i, _)| i)
            .collect();
        let want: Vec<usize> = if name == "Attack" {
            vec![ATTACK_TRIGGER_FRAME]
        } else {
            Vec::new()
        };
        if triggers != want {
            return err(format!("unsupported Fat Fly frame event: {name}"));
        }
    }
    let prefab = volley(attack)?;
    let shot = shot(sc, source, &prefab)?;
    let (_, pool_owner) = only(&records, "PersonalObjectPool", "Fat Fly")?;
    let pool = get(pool_owner, "startupPool")?.list().unwrap_or(&[]);
    if pool.len() != 1
        || !eq_field(&pool[0], "size", &Value::Int(4))?
        || !get(&pool[0], "prefab")?.py_eq(&prefab)
    {
        return err("unsupported Fat Fly shot pool");
    }
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bounds = axis_aligned_bounds(&identity, BODY_OFFSET, BODY_SIZE)?;
    Ok(jobj(vec![
        ("kind", js("FatFly")),
        ("guest_enabled", Json::Bool(true)),
        ("body_bounds_local", Json::List(bounds.iter().map(|&f| Json::Float(f)).collect())),
        (
            "fsm_sha256",
            jobj(vec![(BOUNCE_FSM, js(BOUNCE_SHA256)), (ATTACK_FSM, js(ATTACK_SHA256))]),
        ),
        ("assemblies_sha256", Json::Obj(ASSEMBLIES.iter().map(|(k, h)| (k.to_string(), js(h))).collect())),
        ("library_source", Json::Str(library_o.sid())),
        ("shot", shot),
        ("art_bindings", jobj(vec![("walk", js("Fly")), ("turn", js("Fly")), ("attack", js("Attack"))])),
        (
            "limitations",
            Json::List(
                [
                    "Gravity-free dynamic body on the bounded terrain solver; bonk sides come from the blocked solver axis instead of the contact normal, and the 50 Hz fixed steps run from a 60 Hz accumulator.",
                    "Shots fly straight under gravity .05 without rotation or stretch; their audio and the wing buzz loop are not presented, and `damages_enemy` never meets another enemy.",
                    "The corpse is a permanent prop that bounces to rest; Corpse Flame, Steam and the spore clouds are not presented.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
