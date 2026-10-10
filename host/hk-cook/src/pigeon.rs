//! Recognize the Greenpath Pigeon for guest admission. Ported from host/pigeon.py.
//!
//! The controller lives in shared/hk-sim/src/pigeon.rs; this module admits only a
//! placement whose serialized shape matches the one that was read to write it.
//! The Pigeon is a critter, not an enemy: one hit point, no `DamageHero`, no
//! `Recoil`, no Geo, and a single trigger BoxCollider2D on layer 19, so it cannot
//! hurt the hero, cannot be stood on and never touches terrain. Its range circles
//! are constants in `pigeon.rs`; this module recomputes the placement's own world
//! circles and proves them.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::pyfloat;
use crate::pyjson::Json;
use crate::recog::{body_box, check_assemblies, near, xy};
use crate::runner::{fsm_fingerprint, ASSEMBLIES};
use hk_unity::playmaker::{action_fields, field};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const FSM_NAME: &str = "Pigeon";
/// Structural fingerprints of the three FSMs a Pigeon carries.
const FSM_SHA256: &str = "e55f8f77ea3171e357ce1170c79029898cff3afb25f6ee3b373e4a13a5c6b095";
const HERO_RANGE_SHA256: &str = "1049dc3ecf7356b8320a5b3ef07d476ffb84f0ce1ba9ebd00906b586d60f7ebb";
const ENEMY_RANGE_SHA256: &str = "31f41df6835d30fde2436c6e601e9800205bb58c995162a619f62b26ee03f21b";
const WAKER_SHA256: &str = "e8e8ffc84583e9c97ffe988a9eb606531456935602c500d9af0fb81d1730684e";
const COMPONENTS: [&str; 16] = [
    "AudioSource",
    "BoxCollider2D",
    "EnemyDeathEffectsNoEffect",
    "ExtraDamageable",
    "HealthManager",
    "MeshFilter",
    "MeshRenderer",
    "PlayMakerCollisionEnter2D",
    "PlayMakerCollisionStay2D",
    "PlayMakerFSM",
    "PlayMakerFixedUpdate",
    "Rigidbody2D",
    "SetZ",
    "Transform",
    "tk2dSprite",
    "tk2dSpriteAnimator",
];
/// The authored uniform scale every placement stands at.
const SCALE: f64 = 0.800000011920929;
const LAYER: i64 = 19; // Interactive Object
const BODY_SIZE: [f64; 2] = [0.6744292974472046, 0.9062597751617432];
const BODY_OFFSET: [f64; 2] = [0.116851806640625, 0.6600000262260437];
/// name: (frames, fps, wrapMode).
const CLIPS: [(&str, usize, f64, i64); 4] = [
    ("Fly", 4, 12.0, 0),
    ("Idle 01", 67, 12.0, 0),
    ("Idle 02", 41, 12.0, 0),
    ("Idle 03", 61, 12.0, 0),
];
/// `pigeon::Clip::slot()` order; `Idle 01` and `Fly` are the shared `ActorSpec` slots.
pub(crate) const CLIP_SLOTS: [(&str, &str); 2] = [("idle2", "Idle 02"), ("idle3", "Idle 03")];
const CHILDREN: [&str; 3] = ["Hero Range", "Enemy Range", "Waker"];
/// The range circles as (centre x, centre y, radius) relative to the actor origin in Q16.
const HERO_RANGE_Q16: [i64; 3] = [0, 39426, 332399];
const ENEMY_RANGE_Q16: [i64; 3] = [0, 42992, 185074];
const WAKER_Q16: [i64; 3] = [0, 42992, 185074];
const CHILD_LAYERS: [(&str, i64); 3] = [("Hero Range", 13), ("Enemy Range", 15), ("Waker", 11)];
const TAKEOFF_RISE_Q16: i64 = 32768;
const RISE_FORCE_Q16: [i64; 2] = [655360, 2293760];
const SIDE_FORCE_Q16: [i64; 2] = [2293760, 4915200];
const LIFE_TICKS: i64 = 300;
const START_FRAMES: [i64; 2] = [0, 41];

fn n(x: f64) -> Value {
    Value::F64(x)
}

/// `_trigger_circle`: one range child, as a Q16 circle around the actor origin.
fn trigger_circle(
    sc: &Scene,
    children: &[(String, (i64, i64))],
    name: &str,
    origin: [f64; 2],
    expected: [i64; 3],
    fingerprint: &str,
    enabled: bool,
) -> Result<[i64; 3]> {
    let Some(&(_, (gid, tid))) = children.iter().find(|c| c.0 == name) else {
        return err(format!("Pigeon is missing its {name} child"));
    };
    let layer = CHILD_LAYERS.iter().find(|c| c.0 == name).unwrap().1;
    if !sc.active(gid)
        || get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(layer)
    {
        return err(format!("inactive or relayered Pigeon {name} child"));
    }
    let records = component_records(sc, gid);
    let circles: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "CircleCollider2D")
        .map(|r| r.2)
        .collect();
    if circles.len() != 1
        || get(circles[0], "m_Enabled")?.truthy() != enabled
        || !get(circles[0], "m_IsTrigger")?.truthy()
    {
        return err(format!("unsupported Pigeon {name} trigger"));
    }
    let fsms: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .collect();
    if fsms.len() != 1 || fsm_fingerprint(fsms[0])? != fingerprint {
        return err(format!("unverified Pigeon {name} trigger FSM"));
    }
    let circle = circles[0];
    let m = u(sc.world(tid))?;
    // A mirrored placement mirrors its children too, and a circle centred on x = 0
    // is unmoved by that; a rotation or a non-uniform scale would not be.
    if m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 || !near(Some(&n(m[0][0].abs())), m[1][1].abs())
    {
        return err(format!("rotated or non-uniform Pigeon {name} child"));
    }
    let off = xy(circle, "m_Offset")?;
    let radius = get(circle, "m_Radius")?.float().ok_or("m_Radius")?;
    let actual = [
        py_round((m[0][3] + m[0][0] * off[0] - origin[0]) * 65536.0),
        py_round((m[1][3] + m[1][1] * off[1] - origin[1]) * 65536.0),
        py_round(radius * m[0][0].abs() * 65536.0),
    ];
    if actual != expected {
        return err(format!(
            "Pigeon {name} circle {actual:?} is not the admitted {expected:?}"
        ));
    }
    Ok(actual)
}

/// `_containment`: prove the smaller circle lies inside the one the guest carries.
fn containment(inner: [i64; 3], outer: [i64; 3]) -> Result<()> {
    let span = crate::pyfloat::hypot((inner[0] - outer[0]) as f64, (inner[1] - outer[1]) as f64);
    if span + inner[2] as f64 > outer[2] as f64 {
        return err(format!(
            "Pigeon {inner:?} is not inside the admitted {outer:?}"
        ));
    }
    Ok(())
}

/// `_depth`: `SetZ`, which is why the authored transform z is not checked.
fn depth(records: &[(i64, &str, &Value)]) -> Result<(f64, f64, bool)> {
    let sets: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "SetZ")
        .map(|r| r.2)
        .collect();
    if sets.len() != 1 || !get(sets[0], "m_Enabled")?.truthy() {
        return err("Pigeon needs exactly one enabled SetZ");
    }
    let d = sets[0];
    if get(d, "randomizeFromStartingValue")?.truthy() {
        return err("a Pigeon that keeps its authored depth is not the admitted variant");
    }
    let (z, delay) = (
        get(d, "z")?.float().ok_or("z")?,
        get(d, "delayBeforeRandomizing")?.float().ok_or("delay")?,
    );
    if !(0.0..=0.01).contains(&z) || !(0.0..=1.0).contains(&delay) {
        return err(format!(
            "Pigeon settles at depth {}, off the guest source plane",
            pyfloat::repr(z)
        ));
    }
    Ok((z, delay, !get(d, "dontRandomize")?.truthy()))
}

/// `_literals`: one named action's compact scalars, refusing any bound to a variable.
fn literals(st: &Value, action: &str, keys: &[&str]) -> Result<Vec<Value>> {
    let sname = get(st, "name")?.str().unwrap_or_default();
    let data = get(st, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    let found: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(i, nm)| {
            nm.str()
                .is_some_and(|s| s.rsplit('.').next() == Some(action))
                && enabled.get(*i).is_some_and(Value::truthy)
        })
        .map(|(i, _)| i)
        .collect();
    if found.len() != 1 {
        return err(format!(
            "Pigeon {sname} no longer carries one enabled {action}"
        ));
    }
    let fields = u(action_fields(data, found[0], false))?;
    let mut out = Vec::new();
    for key in keys {
        let mut value = field(&fields, key).cloned();
        if let Some(v) = &value {
            if v.is_map() {
                if v.get("useVariable").is_some_and(Value::truthy) {
                    return err(format!(
                        "Pigeon {sname}/{action}/{key} is written from a variable"
                    ));
                }
                value = v.get("value").cloned();
            }
        }
        match value {
            Some(v) => out.push(v),
            None => return err(format!("Pigeon {sname}/{action} no longer carries {key}")),
        }
    }
    Ok(out)
}

fn state_named<'a>(fsm: &'a Value, name: &str) -> Result<&'a Value> {
    get(fsm, "states")?
        .list()
        .unwrap_or(&[])
        .iter()
        .find(|s| s.get("name").and_then(Value::str).as_deref() == Some(name))
        .ok_or_else(|| format!("no state {name}"))
}

fn q(v: &Value) -> i64 {
    py_round(v.float().unwrap_or(f64::NAN) * 65536.0)
}

/// `_flight`: the authored numbers `pigeon.rs` holds, read rather than trusted.
fn flight(fsm: &Value) -> Result<()> {
    let fly = state_named(fsm, "Fly")?;
    let rise = &literals(fly, "Translate", &["y"])?[0];
    if q(rise) != TAKEOFF_RISE_Q16 {
        return err(format!(
            "Pigeon takeoff lift {} is not the admitted {}",
            pyfloat::repr(rise.float().unwrap_or(0.0)),
            pyfloat::repr(TAKEOFF_RISE_Q16 as f64 / 65536.0)
        ));
    }
    let span = literals(fly, "RandomFloat", &["min", "max"])?;
    if [q(&span[0]), q(&span[1])] != RISE_FORCE_Q16 {
        return err("Pigeon rise force is not the admitted one");
    }
    // `CheckTargetDirection` names the event per side of the hero, and the state
    // transitions name where each event goes. Both halves are read, because it is
    // the pair that says the bird flies away rather than towards.
    let events = literals(fly, "CheckTargetDirection", &["rightEvent", "leftEvent"])?;
    let mut to: Vec<(String, String)> = Vec::new();
    for t in get(fly, "transitions")?.list().unwrap_or(&[]) {
        let (e, s) = (
            get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(),
            get(t, "toState")?.str().unwrap_or_default(),
        );
        match to.iter_mut().find(|x| x.0 == e) {
            Some(slot) => slot.1 = s,
            None => to.push((e, s)),
        }
    }
    let dest = |e: &Value| {
        e.str()
            .and_then(|e| to.iter().find(|x| x.0 == e).map(|x| x.1.clone()))
    };
    if dest(&events[0]).as_deref() != Some("Left") || dest(&events[1]).as_deref() != Some("Right") {
        return err("a Pigeon that flies towards the hero is not the admitted variant");
    }
    for (state, expected) in [
        ("Right", SIDE_FORCE_Q16),
        ("Left", [-SIDE_FORCE_Q16[1], -SIDE_FORCE_Q16[0]]),
    ] {
        let st = state_named(fsm, state)?;
        let span = literals(st, "RandomFloat", &["min", "max"])?;
        if [q(&span[0]), q(&span[1])] != expected {
            return err(format!("Pigeon {state} side force is not the admitted one"));
        }
        let wait = &literals(st, "Wait", &["time"])?[0];
        if py_round(wait.float().unwrap_or(f64::NAN) * 60.0) != LIFE_TICKS {
            return err(format!(
                "Pigeon {state} flight lasts {}s, not the admitted {}s",
                pyfloat::repr(wait.float().unwrap_or(0.0)),
                pyfloat::repr(LIFE_TICKS as f64 / 60.0)
            ));
        }
    }
    let frame = literals(
        state_named(fsm, "Set Frame")?,
        "RandomInt",
        &["min", "max", "inclusiveMax"],
    )?;
    if [frame[0].int(), frame[1].int()] != [Some(START_FRAMES[0]), Some(START_FRAMES[1])]
        || !frame[2].truthy()
    {
        return err("Pigeon start frame is not the admitted one");
    }
    Ok(())
}

/// A perched bird that lifts off away from the hero and does not come back.
pub fn recognize(
    sc: &Scene,
    source: &Source,
    gid: i64,
    position: [f64; 3],
    health: &Value,
) -> Result<Json> {
    check_assemblies(source, "Pigeon")?;
    let records = component_records(sc, gid);
    // The FSM set first, because it is the difference that carries a reason.
    let fsm_records: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .map(|r| r.2)
        .collect();
    if fsm_records.len() != 1
        || get(get(fsm_records[0], "fsm")?, "name")?.str().as_deref() != Some(FSM_NAME)
    {
        let mut present: Vec<String> = fsm_records
            .iter()
            .filter_map(|d| {
                d.get("fsm")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::str)
            })
            .collect();
        present.sort();
        return err(format!(
            "unsupported Pigeon FSM set: {}",
            if present.is_empty() {
                "none".to_string()
            } else {
                present.join(", ")
            }
        ));
    }
    let fsm = get(fsm_records[0], "fsm")?;
    if !get(fsm_records[0], "m_Enabled")?.truthy() || fsm_fingerprint(fsm)? != FSM_SHA256 {
        return err("unverified Pigeon FSM variant");
    }
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    let mut want = COMPONENTS.to_vec();
    want.sort();
    if kinds != want {
        return err("unsupported Pigeon component set");
    }
    let layer = get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?
        .int()
        .unwrap_or(-1);
    if layer != LAYER {
        return err(format!(
            "Pigeon outside the Interactive Object layer: layer {layer}"
        ));
    }
    let (set_z, delay, randomized) = depth(&records)?;
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    // The constants are measured at the authored 0.8; a mirror is fine.
    if m[0][1].abs() > 1e-6
        || m[1][0].abs() > 1e-6
        || !near(Some(&n(m[0][0].abs())), SCALE)
        || !near(Some(&n(m[1][1])), SCALE)
    {
        return err("unsupported Pigeon rotation or scale");
    }
    if get(fsm, "startState")?.str().as_deref() != Some("Set Size") {
        return err("Pigeon begins in an unsupported state");
    }
    if fsm
        .get("globalTransitions")
        .and_then(Value::list)
        .is_some_and(|l| !l.is_empty())
    {
        return err("unsupported Pigeon global transitions");
    }
    // `_body`: the one trigger box, which is the whole of the Pigeon's hurt surface.
    let boxes: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    if boxes.len() != 1 {
        return err("unsupported Pigeon body colliders");
    }
    let (bs, bo) = (xy(boxes[0], "m_Size")?, xy(boxes[0], "m_Offset")?);
    if !get(boxes[0], "m_Enabled")?.truthy()
        || !get(boxes[0], "m_IsTrigger")?.truthy()
        || get(boxes[0], "m_EdgeRadius")?.float() != Some(0.0)
        || (0..2)
            .any(|k| !near(Some(&n(bs[k])), BODY_SIZE[k]) || !near(Some(&n(bo[k])), BODY_OFFSET[k]))
    {
        return err("unsupported Pigeon body collider");
    }
    let _ = body_box;
    // `_rigid_body`: gravityScale 0, unit mass, no drag.
    let bodies: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "Rigidbody2D")
        .map(|r| r.2)
        .collect();
    if bodies.len() != 1 {
        return err("unsupported Pigeon rigid body count");
    }
    let b = bodies[0];
    if get(b, "m_BodyType")?.int() != Some(0)
        || !get(b, "m_Simulated")?.truthy()
        || get(b, "m_UseAutoMass")?.truthy()
        || !near(b.get("m_Mass"), 1.0)
        || !near(b.get("m_GravityScale"), 0.0)
        || !near(b.get("m_LinearDamping"), 0.0)
    {
        return err("unsupported Pigeon rigid body");
    }
    if get(health, "hp")?.int() != Some(1)
        || [
            "invincible",
            "invincibleFromDirection",
            "hasSpecialDeath",
            "hasAlternateHitAnimation",
            "damageOverride",
            "megaFlingGeo",
            "smallGeoDrops",
            "mediumGeoDrops",
            "largeGeoDrops",
        ]
        .iter()
        .any(|k| get(health, k).map(Value::truthy).unwrap_or(true))
    {
        return err("unsupported Pigeon HealthManager variant");
    }
    let deaths: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "EnemyDeathEffectsNoEffect")
        .map(|r| r.2)
        .collect();
    if deaths.len() != 1 || !get(deaths[0], "m_Enabled")?.truthy() {
        return err("Pigeon needs exactly one enabled EnemyDeathEffectsNoEffect");
    }
    let sprite = records
        .iter()
        .rev()
        .find(|r| r.1 == "tk2dSprite")
        .map(|r| r.2)
        .ok_or("no sprite")?;
    let one4 = Value::Map(
        ["r", "g", "b", "a"]
            .iter()
            .map(|k| ((*k).into(), n(1.0)))
            .collect(),
    );
    let one3 = Value::Map(
        ["x", "y", "z"]
            .iter()
            .map(|k| ((*k).into(), n(1.0)))
            .collect(),
    );
    if !get(sprite, "_color")?.py_eq(&one4) || !get(sprite, "_scale")?.py_eq(&one3) {
        return err("unsupported Pigeon sprite scale/color");
    }
    let children = crate::recog::child_map(sc, tid)?;
    let mut names: Vec<&str> = children.iter().map(|c| c.0.as_str()).collect();
    names.sort();
    let mut want = CHILDREN.to_vec();
    want.sort();
    if names != want {
        return err(format!("unsupported Pigeon children: {}", names.join(", ")));
    }
    let origin = [position[0], position[1]];
    let hero_range = trigger_circle(
        sc,
        &children,
        "Hero Range",
        origin,
        HERO_RANGE_Q16,
        HERO_RANGE_SHA256,
        true,
    )?;
    let enemy_range = trigger_circle(
        sc,
        &children,
        "Enemy Range",
        origin,
        ENEMY_RANGE_Q16,
        ENEMY_RANGE_SHA256,
        true,
    )?;
    // The `Waker` starts disabled: `Activate` only switches it on a quarter second after takeoff.
    let waker = trigger_circle(
        sc,
        &children,
        "Waker",
        origin,
        WAKER_Q16,
        WAKER_SHA256,
        false,
    )?;
    containment(enemy_range, hero_range)?;
    containment(waker, hero_range)?;
    flight(fsm)?;
    // `_clips`.
    let animator = records
        .iter()
        .rev()
        .find(|r| r.1 == "tk2dSpriteAnimator")
        .map(|r| r.2)
        .ok_or("no tk2dSpriteAnimator")?;
    if !get(animator, "m_Enabled")?.truthy()
        || get(animator, "isRealtime")?.truthy()
        || !get(animator, "playAutomatically")?.truthy()
    {
        return err("Pigeon requires enabled scaled-time animation");
    }
    let library_object = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_object))?;
    let all = get(&library, "clips")?.list().unwrap_or(&[]);
    let default = get(animator, "defaultClipId")?.int().unwrap_or(-1);
    if all
        .get(default as usize)
        .and_then(|c| c.get("name"))
        .and_then(Value::str)
        .as_deref()
        != Some("Idle 01")
    {
        return err("Pigeon does not start on Idle 01");
    }
    let by_name = crate::recog::clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        let clip = by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        if !crate::recog::clip_is(clip, frames, fps, wrap) {
            return err(format!("unsupported Pigeon animation: {name}"));
        }
        let c = clip.unwrap();
        // `Set Frame` seeks by frame index and the guest's clip clock is the cooked frame count.
        if c.get("frames")
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .any(|f| f.get("triggerEvent").is_some_and(Value::truthy))
        {
            return err(format!("unsupported Pigeon frame event: {name}"));
        }
        if c.get("loopStart").map_or(0, |l| l.int().unwrap_or(-1)) != 0 {
            return err(format!("unsupported Pigeon loop start: {name}"));
        }
    }
    let triple = |a: [i64; 3]| Json::List(a.iter().map(|&v| Json::Int(v)).collect());
    let mut art = vec![
        ("walk".to_string(), js("Idle 01")),
        ("turn".to_string(), js("Fly")),
    ];
    art.extend(CLIP_SLOTS.iter().map(|(k, v)| (k.to_string(), js(v))));
    let (zs, ds) = (pyfloat::repr(set_z), pyfloat::repr(delay));
    let limitations: Vec<String> = vec![
        "`Set Size` is not presented. The source draws `RandomFloat(0.8, 1.0)` on the first frame and rescales the whole object, which also rescales its trigger circles; the guest draws every bird at the authored 0.8 and reads the circle measured there, because a cooked frame cannot be resized and the circle is a linked constant.".into(),
        "The `Waker` flock cascade is not presented. In the source a bird that lifts off unparents an `Enemies`-layer trigger a quarter second later, which trips its neighbours' `Enemy Range` and lifts the whole flock, at any distance with a clear line to the hero. The guest reads only `Hero Range`, so each bird answers the hero on its own. In Fungus1_01 the two flocks stand inside each other's `Hero Range` circles, so they still leave together there.".into(),
        "`HERO CAST SPELL` is not presented. The source sends it to every Pigeon in the scene and `Check` lifts any within sixty units of the hero, which is the whole room; in the guest a spell startles nothing.".into(),
        "`FaceAngle` is not presented: the source rotates a flying bird to its velocity every frame, and actor draws carry a rotation only for the Climber. It keeps its upright pose and only the horizontal mirror follows the flight direction.".into(),
        "The flight is integrated at 60 Hz against the source's 50 Hz fixed step. The force is an acceleration, so the speed matches; the positions differ by the Euler integration residue rather than by a constant.".into(),
        "A bird that leaves the actor neighbourhood freezes where it is instead of finishing its five seconds, and the guest removes one that reaches the edge of the validated coordinate range. Both happen far outside the room and neither is ever drawn.".into(),
        format!("The `SetZ` depth is not presented. The source holds the authored transform z for {ds}s and then replaces it with `Random.Range(z, z + 0.001)` off the component field, which is {zs} for every placement; the guest draws every actor on its one source plane throughout, so neither depth reaches the screen."),
        "The takeoff `AudioPlayRandom` and the looping `AudioSource` are not presented, and neither is the recycled `PlayMakerCollisionEnter2D`/`Stay2D` pair, which this FSM has no action for.".into(),
    ];
    Ok(jobj(vec![
        ("kind", js("Pigeon")),
        ("guest_enabled", Json::Bool(true)),
        ("no_corpse", Json::Bool(true)),
        // Its one collider is a trigger, so there is no solid body to measure the spec bounds from.
        ("trigger_body", Json::Bool(true)),
        ("fsm_sha256", js(FSM_SHA256)),
        (
            "assemblies_sha256",
            Json::Obj(
                ASSEMBLIES
                    .iter()
                    .map(|(k, v)| (k.to_string(), js(v)))
                    .collect(),
            ),
        ),
        ("library_source", Json::Str(library_object.sid())),
        ("hero_range_q16", triple(hero_range)),
        ("enemy_range_q16", triple(enemy_range)),
        ("waker_q16", triple(waker)),
        (
            "set_z",
            jobj(vec![
                ("z", Json::Float(set_z)),
                ("delay_seconds", Json::Float(delay)),
                ("randomized", Json::Bool(randomized)),
            ]),
        ),
        // This is the authored pose, which the guest writes as -1.
        (
            "initial_direction",
            Json::Int(if m[0][0] < 0.0 { 1 } else { -1 }),
        ),
        ("art_bindings", Json::Obj(art)),
        (
            "limitations",
            Json::List(limitations.into_iter().map(Json::Str).collect()),
        ),
    ]))
}
