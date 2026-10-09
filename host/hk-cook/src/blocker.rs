//! Recognize the two Crossroads Blockers for guest admission. Ported from host/blocker.py.
//!
//! The controller lives in shared/hk-sim/src/blocker.rs; this module admits only a
//! placement whose serialized shape matches the one that was read to write it.
//!
//! The Blocker is a turret: no `Rigidbody2D`, `Walker`, `Recoil` or `DamageHero`.
//! It stands on a `Terrain Block` child the world cook already lays down as static
//! terrain, and opens, lobs a `Shot Mawlek`, and shuts. Two placements are admitted
//! and differ in exactly one authored value, `Unalert Range`: false with a trigger
//! child that writes the bool (Crossroads_11_alt, it can return to `Dormant`), or true
//! with a bare `Transform` child (Crossroads_ShamanTemple, it never sleeps once open).
//! Both halves are checked. The three trigger boxes are constants in `blocker.rs` and
//! this module proves the placement's own world boxes against them.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::pyjson::Json;
use crate::recog::{
    body_box, check_assemblies, child_map, clips_ok, near, scalar, state, states, variables, xy,
};
use crate::runner::{axis_aligned_bounds, fsm_fingerprint, ASSEMBLIES};
use hk_unity::playmaker::{action_fields, field};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const FSM_NAME: &str = "Blocker Control";
/// Fingerprint of `Blocker Control` per placement, and what the hash proves: whether it sleeps.
const FSM_SHA256: [(&str, bool); 2] = [
    (
        "32194e421ca1a5ab7e2f1421620b0e2b534e0c2849edf2b9fde9d96e8a3a6c64",
        true,
    ),
    (
        "854257702c33b7f1441e6c4a530f488a3512f11e752476cacc7407c5e5ba4dd2",
        false,
    ),
];
const COMPONENTS: [&str; 17] = [
    "AudioSource",
    "BoxCollider2D",
    "EnemyDeathEffects",
    "EnemyDreamnailReaction",
    "ExtraDamageable",
    "HealthManager",
    "InfectedEnemyEffects",
    "MeshFilter",
    "MeshRenderer",
    "PersistentBoolItem",
    "PersonalObjectPool",
    "PlayMakerFSM",
    "PlayMakerFixedUpdate",
    "SpriteFlash",
    "Transform",
    "tk2dSprite",
    "tk2dSpriteAnimator",
];
const BODY_SIZE: [f64; 2] = [2.78125, 3.15625];
const BODY_OFFSET: [f64; 2] = [0.5625, -0.640625];
/// name: (frames, fps, wrapMode).
const CLIPS: [(&str, usize, f64, i64, Option<i64>); 8] = [
    ("Idle", 7, 12.0, 0, Some(0)),
    ("Closed", 1, 30.0, 0, Some(0)),
    ("Open", 4, 15.0, 2, Some(0)),
    ("Close1", 2, 15.0, 2, Some(0)),
    ("Close2", 4, 15.0, 2, Some(0)),
    ("Shoot Antic", 3, 15.0, 2, Some(0)),
    ("Shoot CD", 4, 15.0, 2, Some(0)),
    ("Hit", 7, 12.0, 2, Some(0)),
];
/// `blocker::Clip::slot()` order with the clip each names.
pub(crate) const CLIP_SLOTS: [(&str, &str); 6] = [
    ("open", "Open"),
    ("close1", "Close1"),
    ("close2", "Close2"),
    ("antic", "Shoot Antic"),
    ("cooldown", "Shoot CD"),
    ("hit", "Hit"),
];
const SHOT_CLIPS: [(&str, usize, f64, i64, Option<i64>); 2] =
    [("Idle", 4, 20.0, 0, None), ("Impact", 6, 20.0, 2, None)];
const SHOT_NAME: &str = "Shot Mawlek";
const SHOT_GRAVITY: f64 = 0.6;
const CHILDREN: [&str; 6] = [
    "Alert Range New",
    "Attack Range",
    "Spit Effect",
    "Unalert Range",
    "Terrain Block",
    "Pt Close",
];
const ALERT_Q16: [i64; 4] = [-37122, -226637, 957772, 512589];
const ATTACK_Q16: [i64; 4] = [-162202, -229293, 427134, 733486];
const UNALERT_Q16: [i64; 4] = [-785266, -652750, 1873773, 771067];
const SHOT_ORIGIN_Q16: [i64; 2] = [180879, 51118];
const SHOT_VX_Q16: [i64; 2] = [196608, 983040];
const SHOT_VY_Q16: i64 = 1310720;
const IDLE_TICKS: [i64; 2] = [48, 72];
const CORPSE_NAME: &str = "Corpse Blocker";
const CORPSE_PLAYER_DATA: &str = "Blocker";

fn n(x: f64) -> Value {
    Value::F64(x)
}

/// Python `float(v)` of a number or bool.
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(*b as i64 as f64),
        o => o.float(),
    }
}

fn trigger_box(
    sc: &Scene,
    children: &[(String, (i64, i64))],
    name: &str,
    origin: [f64; 2],
    expected: [i64; 4],
) -> Result<()> {
    let Some(&(_, (gid, tid))) = children.iter().find(|c| c.0 == name) else {
        return err(format!("Blocker is missing its {name} child"));
    };
    if !sc.active(gid) {
        return err(format!("inactive Blocker {name} child"));
    }
    let records = component_records(sc, gid);
    let boxes: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    if boxes.len() != 1
        || !get(boxes[0], "m_Enabled")?.truthy()
        || !get(boxes[0], "m_IsTrigger")?.truthy()
        || get(boxes[0], "m_EdgeRadius")?.float() != Some(0.0)
    {
        return err(format!("unsupported Blocker {name} trigger"));
    }
    let world = axis_aligned_bounds(
        &u(sc.world(tid))?,
        xy(boxes[0], "m_Offset")?,
        xy(boxes[0], "m_Size")?,
    )?;
    let actual: Vec<i64> = world
        .iter()
        .enumerate()
        .map(|(i, v)| py_round((v - origin[i % 2]) * 65536.0))
        .collect();
    if actual != expected {
        return err(format!(
            "Blocker {name} box {actual:?} is not the admitted {expected:?}"
        ));
    }
    Ok(())
}

fn alert_range(sc: &Scene, children: &[(String, (i64, i64))], name: &str) -> Result<()> {
    let gid = children
        .iter()
        .find(|c| c.0 == name)
        .ok_or("missing child")?
        .1
         .0;
    let alerts: Vec<&Value> = component_records(sc, gid)
        .into_iter()
        .filter(|r| r.1 == "AlertRange")
        .map(|r| r.2)
        .collect();
    if alerts.len() != 1 || !get(alerts[0], "m_Enabled")?.truthy() {
        return err(format!("unsupported Blocker {name} AlertRange component"));
    }
    Ok(())
}

/// The `Unalert Range` child, which is the two placements' one difference.
fn unalert(
    sc: &Scene,
    children: &[(String, (i64, i64))],
    origin: [f64; 2],
    sleeps: bool,
) -> Result<()> {
    let Some(&(_, (gid, _))) = children.iter().find(|c| c.0 == "Unalert Range") else {
        return err("Blocker is missing its Unalert Range child");
    };
    let records = component_records(sc, gid);
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    if !sleeps {
        if kinds != ["Transform"] {
            return err(format!(
                "Blocker authors Unalert Range true but carries {}",
                kinds.join(", ")
            ));
        }
        return Ok(());
    }
    if kinds != ["BoxCollider2D", "PlayMakerFSM", "Transform"] {
        return err(format!(
            "unsupported Blocker Unalert Range child: {}",
            kinds.join(", ")
        ));
    }
    let fsm = records
        .iter()
        .find(|r| r.1 == "PlayMakerFSM")
        .and_then(|r| r.2.get("fsm"))
        .ok_or("no FSM")?;
    let vars = variables(fsm);
    let var = |k: &str| vars.iter().find(|(name, _)| name == k).map(|(_, v)| *v);
    if var("FSM Name").and_then(Value::str).as_deref() != Some(FSM_NAME)
        || var("Bool Name").and_then(Value::str).as_deref() != Some("Unalert Range")
    {
        return err("Blocker Unalert Range trigger writes a different gate");
    }
    trigger_box(sc, children, "Unalert Range", origin, UNALERT_Q16)
}

fn ints(v: Option<&Value>) -> Vec<i64> {
    v.and_then(Value::list)
        .unwrap_or(&[])
        .iter()
        .map(|x| x.int().unwrap_or(0))
        .collect()
}

/// `Goop`'s `Shot Mawlek`, the projectile `Fire` takes from the pool.
fn shot(sc: &Scene, source: &Source, fsm: &Value) -> Result<Json> {
    let sts = states(fsm)?;
    let goop = state(&sts, "Goop").ok_or("no Goop state")?;
    let data = get(goop, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    let assigns: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(i, nm)| {
            nm.str()
                .is_some_and(|s| s.rsplit('.').next() == Some("SetGameObject"))
                && enabled.get(*i).is_some_and(Value::truthy)
        })
        .map(|(i, _)| i)
        .collect();
    if assigns.len() != 1 {
        return err("Blocker Goop no longer assigns a single projectile");
    }
    let starts = ints(data.get("actionStartIndex"));
    let (types, pos) = (
        ints(data.get("paramDataType")),
        ints(data.get("paramDataPos")),
    );
    let params = get(data, "paramName")?.list().unwrap_or(&[]).len();
    let a = assigns[0];
    let start = starts[a] as usize;
    let end = if a + 1 < names.len() {
        starts[a + 1] as usize
    } else {
        params
    };
    let gos = get(data, "fsmGameObjectParams")?.list().unwrap_or(&[]);
    let mut literal: Vec<&Value> = Vec::new();
    for k in start..end {
        if types.get(k) == Some(&19) {
            let r = gos.get(pos[k] as usize).ok_or("param index")?;
            if !get(r, "useVariable")?.truthy()
                && get(r, "value")?
                    .get("m_PathID")
                    .and_then(Value::int)
                    .unwrap_or(0)
                    != 0
            {
                literal.push(get(r, "value")?);
            }
        }
    }
    if literal.len() != 1 {
        return err("Blocker shot prefab reference missing");
    }
    let obj = u(sc.deref(literal[0]))?;
    let go = u(source.read(&obj))?;
    if get(&go, "m_Name")?.str().as_deref() != Some(SHOT_NAME) {
        return err("unsupported Blocker shot prefab");
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
            return err("unsupported Blocker shot component set");
        }
    }
    if !near(
        part("Rigidbody2D").unwrap().get("m_GravityScale"),
        SHOT_GRAVITY,
    ) || part("DamageHero").unwrap().get("damageDealt").and_then(num) != Some(1.0)
    {
        return err("unsupported Blocker shot body");
    }
    let bsize = xy(part("BoxCollider2D").unwrap(), "m_Size")?;
    if !near(Some(&n(bsize[0])), 0.640625) || !near(Some(&n(bsize[1])), 0.5625) {
        return err("Blocker shot box differs from the pooled projectile box");
    }
    let library_o = u(source.deref(
        &obj.file,
        get(part("tk2dSpriteAnimator").unwrap(), "library")?,
    ))?;
    let library = u(source.read(&library_o))?;
    if let Some(name) = clips_ok(&library, &SHOT_CLIPS)? {
        return err(format!("unsupported Blocker shot animation: {name}"));
    }
    let scale = xy(part("Transform").unwrap(), "m_LocalScale")?[0]
        * get(part("EnemyBullet").unwrap(), "scaleMin")?
            .float()
            .ok_or("scaleMin")?;
    Ok(jobj(vec![
        ("source", Json::Str(obj.sid())),
        ("library", Json::Str(library_o.sid())),
        ("library_object", js("<ObjectReader>")),
        ("scale", Json::Float(scale)),
    ]))
}

fn enabled_actions(data: &Value, suffix: &str) -> Vec<usize> {
    let names = data.get("actionNames").and_then(Value::list).unwrap_or(&[]);
    let enabled = data
        .get("actionEnabled")
        .and_then(Value::list)
        .unwrap_or(&[]);
    names
        .iter()
        .enumerate()
        .filter(|(i, nm)| {
            nm.str().is_some_and(|s| s.ends_with(suffix))
                && enabled.get(*i).is_some_and(Value::truthy)
        })
        .map(|(i, _)| i)
        .collect()
}

/// `Direction` -> `Right`, the only launch branch an admitted Blocker takes.
fn launch(fsm: &Value) -> Result<()> {
    let vars: Vec<(String, &Value)> = variables(fsm)
        .into_iter()
        .filter(|(_, v)| !v.is_map())
        .collect();
    let var = |k: &str| vars.iter().find(|(name, _)| name == k).map(|(_, v)| *v);
    if var("Facing Right").and_then(num) != Some(1.0) {
        return err("a left-facing Blocker takes the unaudited Left launch branch");
    }
    let vy = var("Shot Y Speed").cloned().unwrap_or(Value::Int(0));
    if !near(Some(&vy), SHOT_VY_Q16 as f64 / 65536.0) {
        return err("unsupported Blocker Shot Y Speed");
    }
    let sts = states(fsm)?;
    let right = state(&sts, "Right").ok_or("no Right state")?;
    let data = get(right, "actionData")?;
    let mut written: Vec<(String, Value)> = Vec::new();
    for i in enabled_actions(data, "SetFloatValue") {
        let fields = u(action_fields(data, i, false))?;
        let (target, value) = (
            field(&fields, "floatVariable"),
            field(&fields, "floatValue"),
        );
        let (Some(t), Some(v)) = (target, value) else {
            return err("unsupported Blocker Right launch assignment");
        };
        if !t.is_map()
            || !t.get("useVariable").is_some_and(Value::truthy)
            || !v.is_map()
            || v.get("useVariable").is_some_and(Value::truthy)
        {
            return err("unsupported Blocker Right launch assignment");
        }
        let (name, val) = (
            t.get("name").and_then(Value::str).unwrap_or_default(),
            v.get("value").cloned().ok_or("no value")?,
        );
        match written.iter_mut().find(|w| w.0 == name) {
            Some(slot) => slot.1 = val,
            None => written.push((name, val)),
        }
    }
    let speed = |k: &str| {
        written
            .iter()
            .find(|w| w.0 == k)
            .map_or(0, |w| py_round(num(&w.1).unwrap_or(f64::NAN) * 65536.0))
    };
    let speeds = [speed("X Speed Min"), speed("X Speed Max")];
    if speeds != SHOT_VX_Q16 {
        return err(format!(
            "Blocker launch speeds {speeds:?} are not the admitted {SHOT_VX_Q16:?}"
        ));
    }
    let origins = enabled_actions(data, "SetVector3XYZ");
    if origins.len() != 1 {
        return err("Blocker Right no longer writes a single Shot Origin");
    }
    let fields = u(action_fields(data, origins[0], false))?;
    let mut xyz = Vec::new();
    for key in ["x", "y", "z"] {
        let v = field(&fields, key).map_or(Value::F64(0.0), scalar);
        if matches!(v, Value::Str(_)) {
            return err("Blocker Shot Origin is written from a variable");
        }
        xyz.push(num(&v).unwrap_or(f64::NAN));
    }
    let origin = [py_round(xyz[0] * 65536.0), py_round(xyz[1] * 65536.0)];
    if origin != SHOT_ORIGIN_Q16 {
        return err(format!(
            "Blocker Shot Origin {origin:?} is not the admitted {SHOT_ORIGIN_Q16:?}"
        ));
    }
    if !near(Some(&n(xyz[2])), 0.0) {
        return err("Blocker Shot Origin leaves the guest source plane");
    }
    Ok(())
}

/// `Idle`'s `WaitRandom`, the whole of the Blocker's fire rate.
fn idle_wait(fsm: &Value) -> Result<()> {
    let sts = states(fsm)?;
    let data = get(state(&sts, "Idle").ok_or("no Idle state")?, "actionData")?;
    let waits = enabled_actions(data, "WaitRandom");
    if waits.len() != 1 {
        return err("Blocker Idle no longer carries a single WaitRandom");
    }
    let fields = u(action_fields(data, waits[0], false))?;
    let mut ticks = Vec::new();
    let mut variable = false;
    for key in ["timeMin", "timeMax"] {
        let f = field(&fields, key).ok_or("missing wait field")?;
        ticks.push(py_round(
            f.get("value").and_then(num).ok_or("no value")? * 60.0,
        ));
        variable |= f.get("useVariable").is_some_and(Value::truthy);
    }
    if variable || ticks != IDLE_TICKS {
        return err(format!(
            "Blocker idle wait {ticks:?} is not the admitted {IDLE_TICKS:?}"
        ));
    }
    Ok(())
}

/// What `EnemyDeathEffects` names, which decides `no_corpse`.
fn corpse(sc: &Scene, source: &Source, records: &[(i64, &str, &Value)]) -> Result<String> {
    let deaths: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "EnemyDeathEffects")
        .map(|r| r.2)
        .collect();
    if deaths.len() != 1 || !get(deaths[0], "m_Enabled")?.truthy() {
        return err("Blocker needs exactly one enabled EnemyDeathEffects");
    }
    if get(deaths[0], "playerDataName")?.str().as_deref() != Some(CORPSE_PLAYER_DATA) {
        return err("unvalidated Blocker kill counter");
    }
    let obj = u(sc.deref(get(deaths[0], "corpsePrefab")?))?;
    let go = u(source.read(&obj))?;
    if get(&go, "m_Name")?.str().as_deref() != Some(CORPSE_NAME) {
        return err("unvalidated Blocker corpse prefab");
    }
    for r in get(&go, "m_Component")?.list().unwrap_or(&[]) {
        let kind = u(source.typename(&u(source.deref(&obj.file, get(r, "component")?))?))?;
        if kind == "Corpse" || kind == "Rigidbody2D" {
            return err(
                "Blocker corpse is a falling Corpse after all; wire it rather than dropping it",
            );
        }
    }
    Ok(obj.sid())
}

/// A stationary shell that opens, lobs a goop and shuts again.
pub fn recognize(sc: &Scene, source: &Source, gid: i64, position: [f64; 3]) -> Result<Json> {
    check_assemblies(source, "Blocker")?;
    let records = component_records(sc, gid);
    let mut kinds: Vec<&str> = records.iter().map(|r| r.1).collect();
    kinds.sort();
    let mut want = COMPONENTS.to_vec();
    want.sort();
    if kinds != want {
        return err("unsupported Blocker component set");
    }
    let fsm_records: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .map(|r| r.2)
        .collect();
    if fsm_records.len() != 1
        || get(get(fsm_records[0], "fsm")?, "name")?.str().as_deref() != Some(FSM_NAME)
    {
        return err("unsupported Blocker FSM set");
    }
    let fsm = get(fsm_records[0], "fsm")?;
    let fingerprint = fsm_fingerprint(fsm)?;
    let sleeps = FSM_SHA256.iter().find(|f| f.0 == fingerprint).map(|f| f.1);
    let (true, Some(sleeps)) = (get(fsm_records[0], "m_Enabled")?.truthy(), sleeps) else {
        return err("unverified Blocker FSM variant");
    };
    if get(fsm, "startState")?.str().as_deref() != Some("Pause") {
        return err("Blocker begins in an unsupported state");
    }
    let mut globals: Vec<(String, String)> = Vec::new();
    for t in fsm
        .get("globalTransitions")
        .and_then(Value::list)
        .unwrap_or(&[])
    {
        globals.push((
            get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(),
            get(t, "toState")?.str().unwrap_or_default(),
        ));
    }
    if globals != [("TOOK DAMAGE".to_string(), "Hit Pause".to_string())] {
        return err("unsupported Blocker global transitions");
    }
    if get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(11) {
        return err("Blocker outside the enemy layer");
    }
    if position[2].abs() > 0.01 {
        return err("Blocker depth differs from guest source plane");
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    // Both admitted placements are mirrored, the pose the constant boxes were measured in.
    if m[0][0] >= 0.0
        || (m[0][0].abs() - 1.0).abs() > 1e-6
        || (m[1][1] - 1.0).abs() > 1e-6
        || m[0][1].abs() > 1e-6
        || m[1][0].abs() > 1e-6
    {
        return err("unsupported Blocker layer or initial scale");
    }
    let bx = body_box(&records, "Blocker")?;
    let (bs, bo) = (xy(bx, "m_Size")?, xy(bx, "m_Offset")?);
    if !get(bx, "m_Enabled")?.truthy()
        || get(bx, "m_IsTrigger")?.truthy()
        || get(bx, "m_EdgeRadius")?.float() != Some(0.0)
        || (0..2)
            .any(|k| !near(Some(&n(bs[k])), BODY_SIZE[k]) || !near(Some(&n(bo[k])), BODY_OFFSET[k]))
    {
        return err("unsupported Blocker body collider");
    }
    let children = child_map(sc, tid)?;
    if CHILDREN.iter().any(|c| !children.iter().any(|k| k.0 == *c)) {
        return err("Blocker is missing children");
    }
    let origin = [position[0], position[1]];
    trigger_box(sc, &children, "Alert Range New", origin, ALERT_Q16)?;
    trigger_box(sc, &children, "Attack Range", origin, ATTACK_Q16)?;
    for name in ["Alert Range New", "Attack Range"] {
        alert_range(sc, &children, name)?;
    }
    unalert(sc, &children, origin, sleeps)?;
    // `Terrain Block` is cooked as static terrain regardless; checked only so a
    // placement whose solid footprint differs from its body box is not admitted quietly.
    let terrain_gid = children
        .iter()
        .find(|c| c.0 == "Terrain Block")
        .unwrap()
        .1
         .0;
    let terrain: Vec<&Value> = component_records(sc, terrain_gid)
        .into_iter()
        .filter(|r| r.1 == "BoxCollider2D")
        .map(|r| r.2)
        .collect();
    if get(sc.go(terrain_gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(8)
        || terrain.len() != 1
        || get(terrain[0], "m_IsTrigger")?.truthy()
        || !get(terrain[0], "m_Size")?.py_eq(get(bx, "m_Size")?)
        || !get(terrain[0], "m_Offset")?.py_eq(get(bx, "m_Offset")?)
    {
        return err("unsupported Blocker Terrain Block");
    }
    launch(fsm)?;
    idle_wait(fsm)?;
    let shot = shot(sc, source, fsm)?;
    let corpse_source = corpse(sc, source, &records)?;
    // `_clips`.
    let animator = records
        .iter()
        .rev()
        .find(|r| r.1 == "tk2dSpriteAnimator")
        .map(|r| r.2)
        .ok_or("no tk2dSpriteAnimator")?;
    if !get(animator, "m_Enabled")?.truthy() || get(animator, "isRealtime")?.truthy() {
        return err("Blocker requires enabled scaled-time animation");
    }
    let library_object = u(sc.deref(get(animator, "library")?))?;
    let library = u(source.read(&library_object))?;
    if let Some(name) = clips_ok(&library, &CLIPS)? {
        return err(format!("unsupported Blocker animation: {name}"));
    }
    let by_name = crate::recog::clips_by_name(&library)?;
    for (name, ..) in CLIPS {
        let c = by_name
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| *v)
            .ok_or("clip")?;
        // The guest's clip clock is the cooked frame count; a trigger frame would make the FSM act partway through one.
        if c.get("frames")
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .any(|f| f.get("triggerEvent").is_some_and(Value::truthy))
        {
            return err(format!("unsupported Blocker frame event: {name}"));
        }
    }
    let mut art = vec![
        ("walk".to_string(), js("Idle")),
        ("turn".to_string(), js("Closed")),
    ];
    art.extend(CLIP_SLOTS.iter().map(|(k, v)| (k.to_string(), js(v))));
    let limitations: Vec<String> = vec![
        "The Roller half of `Attack Choose` is presented once PlayerData `fireballLevel` is above 0 (host/blocker_roller.py), but the 50/50 is only drawn then: the source draws it every attack and discards it at `Can Roller?`, which only shifts which draws land where. The spell cheat does not raise `fireballLevel`, so it keeps the goop.".into(),
        "`Corpse Blocker` is not presented: it carries no `Corpse` component, no rigid body and no collider, so it is a static prop with its own `corpse` FSM rather than anything the cook's corpse contract can express. The body is removed on death and the Death and Death Stun clips are not cooked.".into(),
        "The `Terrain Block` child is cooked as static world terrain, so the Blocker stays solid after it dies, where the source destroys the whole GameObject and takes the block with it. That footprint is world geometry rather than part of this actor.".into(),
        "Shots are the shared projectile pool rather than this object's own `PersonalObjectPool` reserve of two, so more than two goops could in principle be live at once; the fire cycle is long enough that the source limit is not reached.".into(),
        "The shot flies under gravity .6 without the source stretch, rotation or EnemyBullet scale jitter, and ends on the first terrain or hero contact.".into(),
        "`Hit Pause` is one `NextFrameEvent` before `Hit` and is not reproduced, and the `BLOCKER DAMAGED` it sends to a host FSM goes nowhere in the source either.".into(),
        "The idle wait and the shot speed are per-actor deterministic samples of the source `WaitRandom(0.8, 1.2)` and `RandomFloat(3, 15)`, not Unity RNG parity.".into(),
        "The `Spit Effect` child, the `Pt Close` particle emitter, the goop spatter `FlingObjectsFromGlobalPool` burst, the `ObjectJitter` shake in `Hit`, the audio snapshot transition and every one shot are not presented.".into()    ];
    Ok(jobj(vec![
        ("kind", js("Blocker")),
        ("guest_enabled", Json::Bool(true)),
        ("no_corpse", Json::Bool(true)),
        // One CLUT per clip rather than per frame.
        ("shared_palette", Json::Bool(true)),
        ("fsm_sha256", Json::Str(fingerprint)),
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
        ("sleeps", Json::Bool(sleeps)),
        ("shot", shot),
        ("corpse_source", Json::Str(corpse_source)),
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
