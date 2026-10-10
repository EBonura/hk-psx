//! Recognize the Hatcher and its cage of Hatcher Babies for guest admission.
//! Ported from host/hatcher.py.
//!
//! The controllers live in shared/hk-sim/src/hatcher.rs; this module admits only
//! placements matching docs/HATCHER.md. The source parks a fixed cage of babies per
//! scene and `Fire` moves one of them to the Hatcher, so the pool is a cook-time
//! reservation: every baby a Hatcher can ever release owns a guest actor slot before
//! the scene loads. Two scene-wide bounds decide whether a family is admitted at all:
//! the 32-slot guest pool and the 20 animation slots a frame may bind. A scene that
//! does not fit has its whole family refused.

use crate::common::{component_records, err, get, Result};
use crate::cook_audio::{jobj, js, u};
use crate::gruzzer::scene_bounds;
use crate::pyjson::Json;
use crate::recog::Want::{B, F, I, S};
use crate::recog::{
    body_box, check_actions, check_assemblies, clips_ok, near, scalar, state, states, transitions,
    variables, xy, ActionRow,
};
use crate::runner::axis_aligned_bounds;
use hk_unity::playmaker::{action_fields, field};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// `Extra Tag`, which is what `Initiate`'s FindGameObject looks the cage up by.
const CAGE_TAG: i64 = 20054;
const POOL_SLOTS: usize = 32;
const FRAME_SLOTS: usize = 20;
/// `enemies.rs::RELEASES`: releases the runtime can carry on one frame.
const MAX_HATCHERS_PER_SCENE: usize = 4;
const BODY_SIZE: [f64; 2] = [1.3125, 1.84375];
const BODY_OFFSET: [f64; 2] = [-0.125, -0.171875];
const BABY_BODY_SIZE: [f64; 2] = [0.359375, 0.375];
const BABY_BODY_OFFSET: [f64; 2] = [-0.0234375, -0.03125];
const ALERT_RADIUS: f64 = 0.5 * 15.608528137207031;
/// name: (frames, fps, wrapMode, loopStart).
const CLIPS: [(&str, usize, f64, i64, Option<i64>); 2] =
    [("Fly", 6, 12.0, 1, Some(2)), ("Fire", 8, 15.0, 2, Some(0))];
const BABY_CLIPS: [(&str, usize, f64, i64, Option<i64>); 2] = [
    ("Fly", 10, 12.0, 1, Some(2)),
    ("Death", 5, 18.0, 2, Some(0)),
];
const FSMS: [&str; 2] = ["Hatcher", "flyer_receive_direction_msg"];
const BABY_FSMS: [&str; 2] = ["Control", "flyer_receive_direction_msg"];
const IGNORED_FSMS: [&str; 1] = ["Remove on battle start"];
const TRANSITIONS: [(&str, &[(&str, &str)]); 6] = [
    ("Initiate", &[("FINISHED", "Idle")]),
    ("Idle", &[("ALERT", "Distance Fly")]),
    ("Distance Fly", &[("WAIT", "Hatched Max Check")]),
    (
        "Hatched Max Check",
        &[("TRUE", "Distance Fly"), ("FALSE", "Fire Anticipate")],
    ),
    ("Fire Anticipate", &[("WAIT", "Fire")]),
    (
        "Fire",
        &[("WAIT", "Distance Fly"), ("CANCEL", "Distance Fly")],
    ),
];
const BABY_TRANSITIONS: [(&str, &[(&str, &str)]); 4] = [
    ("Init", &[("FINISHED", "Inert")]),
    ("Inert", &[("SPAWN", "Chase")]),
    ("Chase", &[("CENTIPEDE DEATH", "Death")]),
    ("Death", &[("FINISHED", "Inert")]),
];
#[rustfmt::skip]
const ACTIONS: &[ActionRow] = &[
    ("Idle", "IdleBuzz", &[("waitMin", F(0.75)), ("waitMax", F(1.0)), ("speedMax", F(1.75)), ("accelerationMax", F(15.0)), ("roamingRange", F(1.0))]),
    ("Idle", "FaceDirection", &[("spriteFacesRight", B(false)), ("playNewAnimation", B(false)), ("everyFrame", B(true)), ("pauseBetweenTurns", B(false)), ("pauseTime", F(0.0))]),
    ("Distance Fly", "DistanceFly", &[("distance", F(6.0)), ("speedMax", F(3.5)), ("acceleration", F(0.1)), ("targetsHeight", B(true)), ("height", F(3.5))]),
    ("Distance Fly", "FaceObject", &[("spriteFacesRight", B(false)), ("playNewAnimation", B(false)), ("everyFrame", B(true))]),
    ("Distance Fly", "WaitRandom", &[("timeMin", F(2.0)), ("timeMax", F(3.0)), ("finishEvent", S("WAIT"))]),
    ("Hatched Max Check", "IntCompare", &[("integer2", I(0)), ("equal", S("TRUE")), ("lessThan", S("TRUE")), ("greaterThan", S("FALSE")), ("everyFrame", B(false))]),
    ("Fire Anticipate", "Tk2dPlayAnimation", &[("clipName", S("Fire"))]),
    ("Fire Anticipate", "Wait", &[("time", F(0.335)), ("finishEvent", S("WAIT"))]),
    ("Fire", "FloatAdd", &[("add", F(-1.0)), ("everyFrame", B(false)), ("perSecond", B(false))]),
    ("Fire", "SendEventByName", &[("sendEvent", S("SPAWN")), ("delay", F(0.0))]),
    ("Fire", "Tk2dWatchAnimationEvents", &[("animationCompleteEvent", S("WAIT"))]),
];
#[rustfmt::skip]
const BABY_ACTIONS: &[ActionRow] = &[
    ("Chase", "FaceDirection", &[("spriteFacesRight", B(false)), ("playNewAnimation", B(false)), ("everyFrame", B(true)), ("pauseBetweenTurns", B(true)), ("pauseTime", F(0.4))]),
    ("Chase", "ChaseObject", &[("speedMax", F(5.0)), ("acceleration", F(0.1)), ("targetSpread", F(1.5)), ("spreadResetTimeMin", F(1.0)), ("spreadResetTimeMax", F(2.0))]),
    ("Death", "SetHP", &[("hp", I(5))]),
    ("Death", "SetDamageHeroAmount", &[("damageDealt", I(1))]),
    ("Death", "SetIsDead", &[("setValue", B(false))]),
    ("Death", "SetPosition", &[("x", F(0.0)), ("y", F(0.0)), ("everyFrame", B(false)), ("lateUpdate", B(false))]),
];

fn n(x: f64) -> Value {
    Value::F64(x)
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(*b as i64 as f64),
        o => o.float(),
    }
}

/// `_fsm`: the named FSM, refusing an object that carries one this port skips.
fn fsm_named<'a>(records: &[(i64, &str, &'a Value)], name: &str, who: &str) -> Result<&'a Value> {
    let mut present: Vec<String> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .filter_map(|f| f.get("name").and_then(Value::str))
        .filter(|nm| !IGNORED_FSMS.contains(&nm.as_str()))
        .collect();
    present.sort();
    let mut expected: Vec<&str> = if name == "Hatcher" {
        FSMS.to_vec()
    } else {
        BABY_FSMS.to_vec()
    };
    expected.sort();
    if present != expected {
        return err(format!("unsupported {who} FSM set: {}", present.join(", ")));
    }
    records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .find(|f| f.get("name").and_then(Value::str).as_deref() == Some(name))
        .ok_or_else(|| "no such FSM".to_string())
}

/// `_check_states`: the transitions and audited action parameters.
fn check_states<'a>(
    fsm: &'a Value,
    trans: &[(&str, &[(&str, &str)])],
    actions: &[ActionRow],
    who: &str,
) -> Result<Vec<(String, &'a Value)>> {
    let sts = states(fsm)?;
    for &(name, expected) in trans {
        let ok = state(&sts, name)
            .map(transitions)
            .transpose()?
            .is_some_and(|t| {
                t.len() == expected.len()
                    && t.iter()
                        .zip(expected)
                        .all(|(a, b)| a.0 == b.0 && a.1 == b.1)
            });
        if !ok {
            return err(format!("unsupported {who} transitions: {name}"));
        }
    }
    check_actions(&sts, actions, who)?;
    Ok(sts)
}

/// `_check_clips`: the animator's library and the clips the family needs.
fn check_clips(
    sc: &Scene,
    source: &Source,
    records: &[(i64, &str, &Value)],
    expected: &[(&str, usize, f64, i64, Option<i64>)],
    who: &str,
) -> Result<hk_unity::Obj> {
    let animator = records
        .iter()
        .rev()
        .find(|r| r.1 == "tk2dSpriteAnimator")
        .map(|r| r.2)
        .ok_or("no tk2dSpriteAnimator")?;
    if !get(animator, "m_Enabled")?.truthy() {
        return err(format!("{who} animator disabled"));
    }
    let library_object = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_object))?;
    if let Some(name) = clips_ok(&library, expected)? {
        return err(format!("unsupported {who} animation: {name}"));
    }
    Ok(library_object)
}

/// `_check_body`: layer, pose, collider, rigid body and contact damage; the local bounds.
fn check_body(
    sc: &Scene,
    gid: i64,
    records: &[(i64, &str, &Value)],
    size: [f64; 2],
    offset: [f64; 2],
    who: &str,
) -> Result<[f64; 4]> {
    if get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(11) {
        return err(format!("{who} outside the enemy layer"));
    }
    let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if (0..2).any(|i| (0..2).any(|j| (m[i][j] - if i == j { 1.0 } else { 0.0 }).abs() > 1e-6)) {
        return err(format!("unsupported {who} rotation or scale"));
    }
    let bx = body_box(records, who)?;
    let (bs, bo) = (xy(bx, "m_Size")?, xy(bx, "m_Offset")?);
    if !get(bx, "m_Enabled")?.truthy()
        || get(bx, "m_IsTrigger")?.truthy()
        || get(bx, "m_EdgeRadius")?.float() != Some(0.0)
        || (0..2).any(|k| !near(Some(&n(bs[k])), size[k]) || !near(Some(&n(bo[k])), offset[k]))
    {
        return err(format!("unsupported {who} body collider"));
    }
    let bodies: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "Rigidbody2D")
        .map(|r| r.2)
        .collect();
    if bodies.len() != 1 {
        return err(format!("{who} requires exactly one Rigidbody2D"));
    }
    let r = bodies[0];
    if get(r, "m_BodyType")?.int() != Some(0)
        || get(r, "m_GravityScale")?.float() != Some(0.0)
        || get(r, "m_LinearDamping")?.float() != Some(0.0)
        || get(r, "m_Constraints")?.int() != Some(4)
    {
        return err(format!("unsupported {who} rigid body"));
    }
    let damage: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "DamageHero")
        .map(|r| r.2)
        .collect();
    if damage.len() != 1
        || !get(damage[0], "m_Enabled")?.truthy()
        || get(damage[0], "damageDealt")?.int() != Some(1)
    {
        return err(format!("unsupported {who} contact damage"));
    }
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    axis_aligned_bounds(&identity, offset, size)
}

/// `cage`: the scene's single `Extra Tag` cage and the active babies parked in it.
fn cage(sc: &Scene) -> Result<(i64, Vec<i64>)> {
    let cages: Vec<i64> = sc
        .gos
        .keys()
        .copied()
        .filter(|g| {
            sc.go(*g)
                .and_then(|go| go.get("m_Tag"))
                .and_then(Value::int)
                == Some(CAGE_TAG)
                && sc.active(*g)
        })
        .collect();
    if cages.len() != 1 {
        return err(format!(
            "Hatcher scene needs exactly one Extra Tag cage, found {}",
            cages.len()
        ));
    }
    let gid = cages[0];
    let tid = *sc.go_transform.get(&gid).ok_or("cage has no transform")?;
    let mut children = Vec::new();
    for child in get(sc.transform(tid).ok_or("transform missing")?, "m_Children")?
        .list()
        .unwrap_or(&[])
    {
        let ctid = get(child, "m_PathID")?.int().unwrap_or(0);
        let kid = get(
            get(
                sc.transform(ctid).ok_or("child transform missing")?,
                "m_GameObject",
            )?,
            "m_PathID",
        )?
        .int()
        .unwrap_or(0);
        if sc.active(kid) {
            children.push(kid);
        }
    }
    Ok((gid, children))
}

/// `placed_hatchers`: live Hatcher bodies standing inside the room.
fn placed_hatchers(sc: &Scene, bounds: [f64; 4]) -> Result<usize> {
    let owners: Vec<i64> = sc
        .objects
        .iter()
        .filter(|o| {
            o.typename == "HealthManager" && o.tree.get("m_Enabled").is_some_and(Value::truthy)
        })
        .filter_map(|o| {
            o.tree
                .get("m_GameObject")
                .and_then(|g| g.get("m_PathID"))
                .and_then(Value::int)
        })
        .collect();
    let mut found = 0;
    for &gid in sc.gos.keys() {
        let name = get(sc.go(gid).ok_or("no such GameObject")?, "m_Name")?
            .str()
            .unwrap_or_default();
        if !name.starts_with("Hatcher")
            || name.starts_with("Hatcher Baby")
            || name.starts_with("Hatcher Cage")
        {
            continue;
        }
        if !owners.contains(&gid) || !sc.active(gid) {
            continue;
        }
        let p = u(sc.point(gid, 0.0, 0.0, 0.0))?;
        if bounds[0] <= p[0] && p[0] <= bounds[2] && bounds[1] <= p[1] && p[1] <= bounds[3] {
            found += 1;
        }
    }
    Ok(found)
}

/// `family_budget`: refuse the scene's whole family unless the cage fits both guest bounds.
fn family_budget(
    sc: &Scene,
    bounds: [f64; 4],
    others: &dyn Fn() -> Result<usize>,
) -> Result<usize> {
    let (_, children) = cage(sc)?;
    let hatchers = placed_hatchers(sc, bounds)?;
    if hatchers > MAX_HATCHERS_PER_SCENE {
        return err(format!("{hatchers} Hatchers in one scene exceeds the {MAX_HATCHERS_PER_SCENE} releases the guest carries on a frame"));
    }
    let total = others()? + hatchers + children.len();
    if total > POOL_SLOTS {
        return err(format!(
            "Hatcher cage of {} needs {total} of the {POOL_SLOTS} guest actor slots this scene has",
            children.len()
        ));
    }
    if total > FRAME_SLOTS {
        return err(format!(
            "Hatcher cage of {} needs {total} of the {FRAME_SLOTS} animation slots a frame binds",
            children.len()
        ));
    }
    Ok(children.len())
}

fn float_list(a: &[f64]) -> Json {
    Json::List(a.iter().map(|&f| Json::Float(f)).collect())
}

/// The Hatcher itself: a gravity-free flyer that releases its cage.
pub fn recognize(
    sc: &Scene,
    source: &Source,
    gid: i64,
    position: [f64; 3],
    catalogue: &crate::actors::Catalogue,
    others: &dyn Fn() -> Result<usize>,
) -> Result<Json> {
    check_assemblies(source, "Hatcher")?;
    let records = component_records(sc, gid);
    let bounds = scene_bounds(sc, catalogue)?;
    let (x, y) = (position[0], position[1]);
    if !(bounds[0] <= x && x <= bounds[2] && bounds[1] <= y && y <= bounds[3]) {
        return err("Hatcher parked outside the room is arena content, not a placement");
    }
    let fsm = fsm_named(&records, "Hatcher", "Hatcher")?;
    if get(fsm, "startState")?.str().as_deref() != Some("Initiate") {
        return err("Hatcher begins in an unsupported state");
    }
    if fsm
        .get("globalTransitions")
        .and_then(Value::list)
        .is_some_and(|l| !l.is_empty())
    {
        return err("unsupported Hatcher global transitions");
    }
    let vars = variables(fsm);
    let start_alert = vars
        .iter()
        .find(|(k, _)| k == "startAlert")
        .map(|(_, v)| *v);
    if !start_alert
        .and_then(num)
        .is_some_and(|v| v == 0.0 || v == 1.0)
    {
        return err("unsupported Hatcher start alert");
    }
    let sts = check_states(fsm, &TRANSITIONS, ACTIONS, "Hatcher")?;
    let count_of = |st: &str, suffix: &str| -> Result<Vec<usize>> {
        let data = get(state(&sts, st).ok_or("missing state")?, "actionData")?;
        let names = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        Ok(names
            .iter()
            .enumerate()
            .filter(|(i, nm)| {
                nm.str().is_some_and(|s| s.ends_with(suffix))
                    && enabled.get(*i).is_some_and(Value::truthy)
            })
            .map(|(i, _)| i)
            .collect())
    };
    // `Hatched Max` is the disabled cap; the live gate is the cage child count.
    if count_of("Hatched Max Check", "GetChildCount")?.len() != 1 {
        return err("Hatcher no longer counts its cage before firing");
    }
    if count_of("Fire", "GetRandomChild")?.len() != 1 {
        return err("Hatcher no longer draws its shot from the cage");
    }
    let velocity_at = *count_of("Fire", "SetVelocity2d")?
        .first()
        .ok_or("no SetVelocity2d")?;
    let fire_data = get(state(&sts, "Fire").ok_or("missing state")?, "actionData")?;
    let velocity = u(action_fields(fire_data, velocity_at, false))?;
    let vy = field(&velocity, "y").map(scalar);
    if !near(vy.as_ref(), -5.0) {
        return err("unsupported Hatcher release velocity");
    }
    let body = check_body(sc, gid, &records, BODY_SIZE, BODY_OFFSET, "Hatcher")?;
    let recoils: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "Recoil")
        .map(|r| r.2)
        .collect();
    if recoils.len() != 1
        || get(recoils[0], "freezeInPlace")?.truthy()
        || get(recoils[0], "recoilSpeedBase")?.float() != Some(20.0)
        || !near(recoils[0].get("recoilDuration"), 0.15)
        || get(recoils[0], "preventRecoilUp")?.truthy()
    {
        return err("unsupported Hatcher recoil variant");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let mut circles: Vec<(String, f64)> = Vec::new();
    for (name, (kid, ctid)) in child_map_all(sc, tid)? {
        for (_, kind, data) in component_records(sc, kid) {
            if kind == "CircleCollider2D" {
                let radius = get(data, "m_Radius")?.float().ok_or("m_Radius")?
                    * xy(
                        sc.transform(ctid).ok_or("child transform missing")?,
                        "m_LocalScale",
                    )?[0];
                match circles.iter_mut().find(|c| c.0 == name) {
                    Some(slot) => slot.1 = radius,
                    None => circles.push((name.clone(), radius)),
                }
            }
        }
    }
    let alert = circles
        .iter()
        .find(|c| c.0 == "Alert Range New")
        .map_or(0.0, |c| c.1);
    if !near(Some(&n(alert)), ALERT_RADIUS) {
        return err("unsupported Hatcher alert range");
    }
    let library_object = check_clips(sc, source, &records, &CLIPS, "Hatcher")?;
    let reserved = family_budget(sc, bounds, others)?;
    let limitations: Vec<String> = vec![
        format!("The cage is reserved at cook time: {reserved} guest actor slots this scene always holds, parked, so a release can never fail for want of one."),
        "The cage is shared by every Hatcher in the scene, as the source shares it.".into(),
        "GetRandomChild picks uniformly; the guest releases the first parked member.".into(),
        "Gravity-free dynamic body on the bounded terrain solver; 50 Hz fixed steps from a 60 Hz accumulator.".into(),
        "The corpse is Corpse Hatcher v2, a CorpseHatcher rather than the plain Corpse the cook recognizes, so the body is removed on death and the Burst clip is not presented.".into(),
        "FSMActivator.activateStaggered, PersistentBoolItem, the SetZ depth, the flyer_receive_direction_msg push, the global-pool flings, the audio pitch ramp and every one shot are not presented.".into(),
    ];
    Ok(jobj(vec![
        ("kind", js("Hatcher")),
        ("guest_enabled", Json::Bool(true)),
        ("body_bounds_local", float_list(&body)),
        ("no_corpse", Json::Bool(true)),
        (
            "start_alert",
            Json::Bool(start_alert.is_some_and(Value::truthy)),
        ),
        ("cage_reserved", Json::Int(reserved as i64)),
        ("library_source", Json::Str(library_object.sid())),
        // Neither FaceDirection nor FaceObject plays a clip, so the turn slot holds Fly.
        (
            "art_bindings",
            jobj(vec![
                ("walk", js("Fly")),
                ("turn", js("Fly")),
                ("fire", js("Fire")),
            ]),
        ),
        (
            "limitations",
            Json::List(limitations.into_iter().map(Json::Str).collect()),
        ),
    ]))
}

/// One member of a Hatcher's cage: parked until a release, recycled on death.
pub fn recognize_baby(
    sc: &Scene,
    source: &Source,
    gid: i64,
    catalogue: &crate::actors::Catalogue,
    others: &dyn Fn() -> Result<usize>,
) -> Result<Json> {
    check_assemblies(source, "Hatcher Baby")?;
    let records = component_records(sc, gid);
    let (cage_gid, children) = cage(sc)?;
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let parent = get(
        get(sc.transform(tid).ok_or("transform missing")?, "m_Father")?,
        "m_PathID",
    )?
    .int()
    .unwrap_or(0);
    if parent == 0
        || get(
            get(
                sc.transform(parent).ok_or("parent transform missing")?,
                "m_GameObject",
            )?,
            "m_PathID",
        )?
        .int()
            != Some(cage_gid)
    {
        return err("Hatcher Baby outside the scene cage is not a reserved pool member");
    }
    let fsm = fsm_named(&records, "Control", "Hatcher Baby")?;
    if get(fsm, "startState")?.str().as_deref() != Some("Init") {
        return err("Hatcher Baby begins in an unsupported state");
    }
    if fsm
        .get("globalTransitions")
        .and_then(Value::list)
        .is_some_and(|l| !l.is_empty())
    {
        return err("unsupported Hatcher Baby global transitions");
    }
    check_states(fsm, &BABY_TRANSITIONS, BABY_ACTIONS, "Hatcher Baby")?;
    let body = check_body(
        sc,
        gid,
        &records,
        BABY_BODY_SIZE,
        BABY_BODY_OFFSET,
        "Hatcher Baby",
    )?;
    if records.iter().any(|r| r.1 == "Recoil") {
        return err("Hatcher Baby with a Recoil component is a different placement");
    }
    let bounce: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "ObjectBounce")
        .map(|r| r.2)
        .collect();
    if bounce.len() != 1
        || !near(bounce[0].get("bounceFactor"), 0.3)
        || !near(bounce[0].get("speedThreshold"), 1.0)
    {
        return err("unsupported Hatcher Baby ObjectBounce variant");
    }
    let death: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "EnemyDeathEffects")
        .map(|r| r.2)
        .collect();
    if death.len() != 1 || get(get(death[0], "corpsePrefab")?, "m_PathID")?.truthy() {
        return err("Hatcher Baby with a corpse is a different placement");
    }
    let library_object = check_clips(sc, source, &records, &BABY_CLIPS, "Hatcher Baby")?;
    let bounds = scene_bounds(sc, catalogue)?;
    let reserved = family_budget(sc, bounds, others)?;
    if children.len() != reserved {
        return err("Hatcher cage membership changed between reads");
    }
    let limitations = [
        "Reserved, not spawned: this actor is seated with the scene and parked in the cage until a Hatcher releases it, which is what the source does with it too.",
        "Death recycles rather than removing: hp back to 5, the dead flag cleared and the body returned to the cage, one frame after the kill is counted.",
        "The Death clip and the global-pool flings are not presented; the body simply leaves.",
        "No Recoil component, so the nail does not displace it; the shared recoil pass is skipped.",
        "ObjectBounce (factor .3, threshold 1) is not the terrain solver slide the body uses.",
        "EnemyDreamnailReaction pays SOUL once per scene load rather than once per life.",
    ];
    Ok(jobj(vec![
        ("kind", js("HatcherBaby")),
        ("guest_enabled", Json::Bool(true)),
        ("body_bounds_local", float_list(&body)),
        ("no_corpse", Json::Bool(true)),
        ("cage_reserved", Json::Int(reserved as i64)),
        ("library_source", Json::Str(library_object.sid())),
        // Death is the enemy's own death effect, and the port removes the body instead of playing one.
        (
            "art_bindings",
            jobj(vec![("walk", js("Fly")), ("turn", js("Fly"))]),
        ),
        (
            "limitations",
            Json::List(limitations.iter().map(|s| js(s)).collect()),
        ),
    ]))
}

/// `m_Children` as (name, (gid, tid)), every child once per entry (the circles loop does not dedupe).
fn child_map_all(sc: &Scene, tid: i64) -> Result<Vec<(String, (i64, i64))>> {
    let mut out = Vec::new();
    for child in get(sc.transform(tid).ok_or("transform missing")?, "m_Children")?
        .list()
        .unwrap_or(&[])
    {
        let ctid = get(child, "m_PathID")?.int().unwrap_or(0);
        let kid = get(
            get(
                sc.transform(ctid).ok_or("child transform missing")?,
                "m_GameObject",
            )?,
            "m_PathID",
        )?
        .int()
        .unwrap_or(0);
        out.push((
            get(sc.go(kid).ok_or("child without a GameObject")?, "m_Name")?
                .str()
                .unwrap_or_default(),
            (kid, ctid),
        ));
    }
    Ok(out)
}
