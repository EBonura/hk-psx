//! The False Knight's placement admission and the structural FSM digest.
//! Ported from host/false_knight.py (`fsm_digest`, `recognize_placement` and
//! what it reads: `clip_contract`, `barrel_source`, `arena_trigger_world_box`).
//!
//! The boss controller lives in shared/hk-sim/src/false_knight.rs; this module
//! admits only the placed boss whose FSMs hash to the audited digests. The
//! whole-fight source contract (`recognize`, `arena_sources`) belongs to the
//! arena cook and is not ported here.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::music::value_json;
use crate::pyjson::{dumps_sorted_compact, Json};
use crate::recog::{check_assemblies, clips_by_name, near, state, states, xy};
use crate::runner::ticks;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const BODY_HEALTH: i64 = 65;
const HEAD_HEALTH: i64 = 40;
const BODY_INVULNERABLE_TIME: f64 = 0.25;
const STAGGERS_TO_DEATH: i64 = 3;
const HIT_EVASION_SECONDS: f64 = 0.2;
/// FalseyControl clip inventory: (name, frames, fps, wrapMode, loopStart).
const CLIPS: [(&str, i64, f64, i64, i64); 34] = [
    ("Idle", 5, 12.0, 0, 0), ("Jump Antic", 3, 10.0, 2, 0), ("Land", 5, 10.0, 2, 0), ("Jump", 4, 12.0, 0, 0),
    ("Attack Antic", 6, 12.0, 1, 4), ("Turn", 2, 12.0, 2, 0), ("Jump Attack Up", 5, 12.0, 2, 0), ("Jump Attack Hit 1", 2, 12.0, 2, 0),
    ("Jump Attack Hit 2", 2, 12.0, 2, 0), ("Jump Attack Hit 3", 2, 12.0, 2, 0), ("Attack", 3, 15.0, 2, 0), ("Attack Recover", 5, 12.0, 2, 0),
    ("Blank", 1, 30.0, 6, 0), ("Run Antic", 2, 12.0, 2, 0), ("Run", 5, 12.0, 1, 1), ("Stun Roll", 5, 12.0, 1, 2),
    ("Stun Roll End", 4, 12.0, 2, 0), ("Stun Open", 4, 12.0, 2, 0), ("Stun Hit", 3, 12.0, 2, 0), ("Stun Recover", 6, 12.0, 2, 0),
    ("Rage", 5, 12.0, 2, 0), ("Death Fall", 3, 12.0, 1, 1), ("Head Idle", 5, 12.0, 0, 0), ("Death Land", 5, 12.0, 2, 0),
    ("Head Hit", 8, 12.0, 1, 3), ("Death Head 1", 10, 10.0, 2, 0), ("Death Head 2", 4, 10.0, 2, 0), ("Death Spaz", 3, 12.0, 0, 0),
    ("Body", 1, 30.0, 6, 0), ("Mace Emerge", 16, 12.0, 2, 0), ("Mace Leave", 4, 12.0, 0, 0), ("Stun Opened", 1, 12.0, 2, 0),
    ("Head Spaz", 3, 12.0, 0, 0), ("Mace Roll", 7, 20.0, 2, 0),
];
const BODY_SCALE: f64 = 1.2999999523162842;
/// Local BoxCollider2D size/offset, before the 1.3 transform scale.
const BODY_BOX: [[f64; 2]; 2] = [[3.359375, 3.875], [0.0546875, -2.359375]];
const BARREL_SPAWN_X: [f64; 2] = [13.180000305175781, 44.27000045776367];
const BARREL_SPAWN_GAP: [f64; 2] = [0.15000000596046448, 0.25];
const BARREL_PREFAB: &str = "Falling Barrel";
const BARREL_POOL: i64 = 8;
const BARREL_LAYER: i64 = 17;
const BARREL_BOX: [[f64; 2]; 2] = [[1.1799999475479126, 1.1100000143051147], [0.0, 0.0]];
const BARREL_GRAVITY_SCALE: f64 = 0.32499998807907104;
const BARREL_DAMAGE: i64 = 1;
const BARREL_HAZARD: i64 = 1;
const BARREL_RANDOM_SCALE: [f64; 2] = [0.800000011920929, 1.0];
const FALL_BARREL_SHA256: &str = "25064c55a01b08378bddcb4fe51b2dfd76ec74302e06ea443f21c224e5c7dad4";
/// (art slot, clip).
const ART_BINDINGS: [(&str, &str); 6] = [("walk", "Idle"), ("turn", "Turn"), ("jump_antic", "Jump Antic"), ("land", "Land"), ("stun_opened", "Stun Opened"), ("attack", "Attack")];
const FALSEY_CONTROL_SHA256: &str = "21599984415d020af3289824aa6a0d33ab51c9f2c24278e7f34ed72506328f57";
const CHECK_HEALTH_SHA256: &str = "4f68e313367ba807b5659ab8b8e6e3a623a3f662acfa2b7c7c95587ced8751bc";

const SCALAR_PARAM_TABLES: [&str; 8] = ["fsmFloatParams", "fsmIntParams", "fsmBoolParams", "fsmStringParams", "fsmVector2Params", "fsmVector3Params", "fsmColorParams", "fsmRectParams"];

/// `_scalar_param`: a typed PlayMaker parameter without its per-scene object reference.
pub(crate) fn scalar_param(v: &Value) -> Json {
    if !v.is_map() {
        return value_json(v);
    }
    if v.get("useVariable").is_some_and(Value::truthy) {
        let name = match v.get("name") {
            Some(n) => n.str().unwrap_or_else(|| crate::runner::py_value_repr(n)),
            None => "None".into(),
        };
        return Json::Str(format!("VAR:{name}"));
    }
    v.get("value").map_or(Json::Null, value_json)
}

/// `fsm_digest`: structure and serialized scalar parameters of one FSM.
///
/// It hashes the raw serialized action bytes and the typed scalar tables, so it
/// covers actions no decoder handles while ignoring the PPtrs that differ per
/// scene instance. `ignore` drops the placement variables a family varies.
pub fn fsm_digest(fsm: &Value, ignore: &[&str]) -> Result<String> {
    let pair = |a: String, b: String| Json::List(vec![Json::Str(a), Json::Str(b)]);
    let mut variables: Vec<(String, String, String)> = Vec::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (group, items) in groups {
            let Some(items) = items.list() else { continue };
            for v in items {
                let (Some(name), Some(value)) = (v.get("name"), v.get("value")) else { continue };
                if !v.is_map() {
                    continue;
                }
                let name = name.str().unwrap_or_default();
                if ignore.contains(&name.as_str()) || (value.is_map() && value.get("m_PathID").is_some()) {
                    continue;
                }
                variables.push((group.to_string(), name, dumps_sorted_compact(&value_json(value))));
            }
        }
    }
    variables.sort();
    let transitions = |list: Option<&Value>| -> Result<Vec<Json>> {
        list.and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .map(|t| Ok(pair(get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(), get(t, "toState")?.str().unwrap_or_default())))
            .collect()
    };
    let mut state_rows = Vec::new();
    for st in get(fsm, "states")?.list().ok_or("states is not a list")? {
        let data = get(st, "actionData")?;
        let names = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let actions: Vec<Json> = names.iter().enumerate().map(|(i, n)| Json::List(vec![Json::Str(n.str().unwrap_or_default()), Json::Int(enabled.get(i).map_or(0, |e| e.int().unwrap_or(0)))])).collect();
        let list = |k: &str| get(data, k).map(|v| v.list().unwrap_or(&[]).to_vec());
        let (pn, pt, ps) = (list("paramName")?, list("paramDataType")?, list("paramByteDataSize")?);
        let params: Vec<Json> = pn.iter().zip(&pt).zip(&ps).map(|((n, t), s)| Json::List(vec![value_json(n), value_json(t), value_json(s)])).collect();
        let bytes: String = match get(data, "byteData")? {
            Value::Bytes(b) => b.iter().map(|x| format!("{x:02x}")).collect(),
            other => other.list().unwrap_or(&[]).iter().map(|x| format!("{:02x}", x.int().unwrap_or(0) as u8)).collect(),
        };
        let mut typed = Vec::new();
        for table in SCALAR_PARAM_TABLES {
            if let Some(t) = data.get(table) {
                typed.push((table.to_string(), Json::List(t.list().unwrap_or(&[]).iter().map(scalar_param).collect())));
            }
        }
        state_rows.push(Json::Obj(vec![
            ("name".into(), Json::Str(get(st, "name")?.str().unwrap_or_default())),
            ("transitions".into(), Json::List(transitions(st.get("transitions"))?)),
            ("actions".into(), Json::List(actions)),
            ("starts".into(), value_json(get(data, "actionStartIndex")?)),
            ("params".into(), Json::List(params)),
            ("bytes".into(), Json::Str(bytes)),
            ("typed".into(), Json::Obj(typed)),
        ]));
    }
    let summary = Json::Obj(vec![
        ("name".into(), Json::Str(get(fsm, "name")?.str().unwrap_or_default())),
        ("start".into(), Json::Str(get(fsm, "startState")?.str().unwrap_or_default())),
        ("variables".into(), Json::List(variables.into_iter().map(|(g, n, v)| Json::List(vec![Json::Str(g), Json::Str(n), Json::Str(v)])).collect())),
        ("globals".into(), Json::List(transitions(fsm.get("globalTransitions"))?)),
        ("states".into(), Json::List(state_rows)),
    ]);
    Ok(sha(dumps_sorted_compact(&summary).as_bytes()))
}

/// `_fsms`: the game object's PlayMakerFSM components by FSM name (the last of a name wins).
pub(crate) fn fsms<'a>(sc: &'a Scene, gid: i64) -> Vec<(String, &'a Value)> {
    let mut out: Vec<(String, &Value)> = Vec::new();
    for (_, kind, data) in component_records(sc, gid) {
        if kind == "PlayMakerFSM" {
            let name = data.get("fsm").and_then(|f| f.get("name")).and_then(Value::str).unwrap_or_default();
            match out.iter_mut().find(|(k, _)| *k == name) {
                Some(slot) => slot.1 = data,
                None => out.push((name, data)),
            }
        }
    }
    out
}

/// `_box`: the game object's `index`th BoxCollider2D as ((w, h), (ox, oy)).
fn box_of(sc: &Scene, gid: i64, index: usize) -> Result<[[f64; 2]; 2]> {
    let records = component_records(sc, gid);
    let boxes: Vec<&Value> = records.iter().filter(|r| r.1 == "BoxCollider2D").map(|r| r.2).collect();
    let b = boxes.get(index).ok_or_else(|| format!("missing BoxCollider2D on game object {gid}"))?;
    Ok([xy(b, "m_Size")?, xy(b, "m_Offset")?])
}

/// `_named`: the one game object of the scene with this name.
pub(crate) fn named(sc: &Scene, name: &str) -> Result<i64> {
    let mut ids: Vec<i64> = sc.gos.keys().copied().filter(|g| sc.go(*g).and_then(|go| go.get("m_Name")).and_then(Value::str).as_deref() == Some(name)).collect();
    ids.sort();
    if ids.len() != 1 {
        return err(format!("expected exactly one {name:?} in {}", sc.base.name));
    }
    Ok(ids[0])
}

fn near_all(a: &[[f64; 2]; 2], b: &[[f64; 2]; 2]) -> bool {
    (0..2).all(|i| (0..2).all(|j| near(Some(&Value::F64(a[i][j])), b[i][j])))
}

/// `arena_trigger_world_box`: the Battle Scene trigger as a world box.
fn arena_trigger_world_box(sc: &Scene) -> Result<Vec<f64>> {
    let battle = named(sc, "Battle Scene")?;
    let m = u(sc.world(*sc.go_transform.get(&battle).ok_or("Battle Scene has no transform")?))?;
    if !near(Some(&Value::F64(m[0][0])), 1.0) || !near(Some(&Value::F64(m[1][1])), 1.0) || m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 {
        return err("arena trigger carries an unsupported rotation or scale");
    }
    let [[w, h], [ox, oy]] = box_of(sc, battle, 0)?;
    let p = u(sc.point(battle, 0.0, 0.0, 0.0))?;
    let (x, y) = (p[0], p[1]);
    Ok(vec![x + ox - w / 2.0, y + oy - h / 2.0, x + ox + w / 2.0, y + oy + h / 2.0])
}

fn pair_json(a: &[[f64; 2]; 2]) -> Json {
    Json::List(a.iter().map(|p| Json::List(p.iter().map(|&f| Json::Float(f)).collect())).collect())
}

/// `clip_contract`: the shared tk2d library behind the body, Hitter, Head and Death Head.
fn clip_contract(sc: &Scene, source: &Source, gid: i64) -> Result<(String, Vec<[i64; 4]>)> {
    let records = component_records(sc, gid);
    let animators: Vec<&Value> = records.iter().filter(|r| r.1 == "tk2dSpriteAnimator").map(|r| r.2).collect();
    if animators.len() != 1 {
        return err("expected one tk2dSpriteAnimator on the False Knight");
    }
    let library_object = u(sc.deref(get(animators[0], "library")?))?;
    let library = u(source.read(&library_object))?;
    let by_name = clips_by_name(&library)?;
    let mut have: Vec<&str> = by_name.iter().map(|c| c.0.as_str()).collect();
    let mut want: Vec<&str> = CLIPS.iter().map(|c| c.0).collect();
    have.sort();
    want.sort();
    if have != want {
        return err("unsupported False Knight animation inventory");
    }
    let mut rows = Vec::new();
    for (name, frames, fps, wrap, loop_start) in CLIPS {
        let clip = by_name.iter().find(|c| c.0 == name).unwrap().1;
        let ok = get(clip, "frames")?.list().map(<[Value]>::len) == Some(frames as usize)
            && get(clip, "fps")?.float() == Some(fps)
            && get(clip, "wrapMode")?.int() == Some(wrap)
            && clip.get("loopStart").map_or(0, |l| l.int().unwrap_or(-1)) == loop_start;
        if !ok {
            return err(format!("unsupported False Knight animation: {name}"));
        }
        rows.push([frames, wrap, loop_start, ticks(frames as f64 / fps)]);
    }
    Ok((library_object.sid(), rows))
}

fn unit(keys: &[(&str, f64)]) -> Value {
    Value::Map(keys.iter().map(|(k, v)| ((*k).into(), Value::F64(*v))).collect())
}

/// `barrel_source`: `FK Barrel Summon`'s pooled `Falling Barrel`, or a refusal.
fn barrel_source(sc: &Scene, source: &Source) -> Result<Json> {
    let summoner = named(sc, "FK Barrel Summon")?;
    let records = component_records(sc, summoner);
    let pools: Vec<&Value> = records.iter().filter(|r| r.1 == "PersonalObjectPool").map(|r| r.2).collect();
    if pools.len() != 1 || get(pools[0], "startupPool")?.list().map(<[Value]>::len) != Some(1) {
        return err("FK Barrel Summon no longer holds exactly one pooled prefab");
    }
    let entry = &get(pools[0], "startupPool")?.list().unwrap()[0];
    if get(entry, "size")?.int() != Some(BARREL_POOL) || get(entry, "initialiseSpawnedObjects")?.truthy() {
        return err("barrel pool reserve changed");
    }
    let prefab = u(sc.deref(get(entry, "prefab")?))?;
    let file = prefab.file.clone();
    let go = u(source.read(&prefab))?;
    if get(&go, "m_Name")?.str().as_deref() != Some(BARREL_PREFAB) || get(&go, "m_Layer")?.int() != Some(BARREL_LAYER) {
        return err("unsupported barrel prefab identity");
    }
    let mut parts: Vec<(String, Value)> = Vec::new();
    let mut fsm_parts: Vec<(String, Value)> = Vec::new();
    for r in get(&go, "m_Component")?.list().unwrap_or(&[]) {
        let id = get(get(r, "component")?, "m_PathID")?.int().unwrap_or(0);
        let component = u(source.object(&file, id))?;
        let kind = u(source.typename(&component))?;
        let tree = u(source.read(&component))?;
        if kind == "PlayMakerFSM" {
            let n = get(get(&tree, "fsm")?, "name")?.str().unwrap_or_default();
            match fsm_parts.iter_mut().find(|p| p.0 == n) {
                Some(slot) => slot.1 = tree,
                None => fsm_parts.push((n, tree)),
            }
        } else {
            if parts.iter().any(|p| p.0 == kind) {
                return err(format!("duplicate barrel component: {kind}"));
            }
            parts.push((kind, tree));
        }
    }
    let part = |k: &str| -> Result<&Value> { parts.iter().find(|p| p.0 == k).map(|p| &p.1).ok_or_else(|| format!("barrel prefab is missing a {k}")) };
    for kind in ["Transform", "BoxCollider2D", "Rigidbody2D", "SpriteRenderer", "DamageHero", "RandomScale"] {
        part(kind)?;
    }
    let control = fsm_parts.iter().find(|p| p.0 == "Fall Barrel Control").map(|p| &p.1);
    let control = match control {
        Some(c) if get(c, "m_Enabled")?.truthy() => c,
        _ => return err("barrel prefab has no enabled Fall Barrel Control"),
    };
    let digest = fsm_digest(get(control, "fsm")?, &[])?;
    if digest != FALL_BARREL_SHA256 {
        return err(format!("unverified Fall Barrel Control variant: {digest}"));
    }
    let transform = part("Transform")?;
    if !get(transform, "m_LocalScale")?.py_eq(&unit(&[("x", 1.0), ("y", 1.0), ("z", 1.0)])) || !get(transform, "m_LocalRotation")?.py_eq(&unit(&[("x", 0.0), ("y", 0.0), ("z", 0.0), ("w", 1.0)])) {
        return err("unsupported barrel prefab transform");
    }
    let bx = part("BoxCollider2D")?;
    let shape = [xy(bx, "m_Size")?, xy(bx, "m_Offset")?];
    // The box is a trigger, which is why the barrel falls through the arena
    // floor instead of resting on it.
    if !get(bx, "m_IsTrigger")?.truthy() || !near_all(&shape, &BARREL_BOX) {
        return err("unsupported barrel collider");
    }
    let body = part("Rigidbody2D")?;
    if get(body, "m_BodyType")?.int() != Some(0) || get(body, "m_LinearDamping")?.float() != Some(0.0) || !near(body.get("m_GravityScale"), BARREL_GRAVITY_SCALE) {
        return err("unsupported barrel body");
    }
    let hurt = part("DamageHero")?;
    if get(hurt, "damageDealt")?.int() != Some(BARREL_DAMAGE) || get(hurt, "hazardType")?.int() != Some(BARREL_HAZARD) || get(hurt, "shadowDashHazard")?.truthy() {
        return err("unsupported barrel DamageHero");
    }
    let render = part("SpriteRenderer")?;
    if !get(render, "m_Enabled")?.truthy()
        || get(get(render, "m_Sprite")?, "m_PathID")?.int().unwrap_or(0) == 0
        || get(render, "m_FlipX")?.truthy()
        || get(render, "m_FlipY")?.truthy()
        || !get(render, "m_Color")?.py_eq(&unit(&[("r", 1.0), ("g", 1.0), ("b", 1.0), ("a", 1.0)]))
    {
        return err("unsupported barrel SpriteRenderer");
    }
    let scaler = part("RandomScale")?;
    if !near(scaler.get("minScale"), BARREL_RANDOM_SCALE[0]) || !near(scaler.get("maxScale"), BARREL_RANDOM_SCALE[1]) {
        return err("barrel RandomScale range changed");
    }
    // `Determine Spawns` carries a disabled RandomInt 6..8: the live count is the
    // one FalseyControl writes into `Spawns` before it sends SUMMON.
    let all = fsms(sc, summoner);
    let summon = get(all.iter().find(|f| f.0 == "summon").ok_or("no summon FSM")?.1, "fsm")?;
    let sts = states(summon)?;
    let data = get(state(&sts, "Determine Spawns").ok_or("no Determine Spawns state")?, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    if names.iter().zip(enabled).any(|(n, e)| n.str().is_some_and(|n| n.ends_with("RandomInt")) && e.truthy()) {
        return err("summon now chooses its own spawn count");
    }
    let sprite = u(source.deref(&file, get(render, "m_Sprite")?))?;
    let spawn = u(sc.point(summoner, 0.0, 0.0, 0.0))?;
    Ok(jobj(vec![
        ("source", Json::Str(prefab.sid())),
        ("sprite", Json::Str(sprite.sid())),
        ("pool", Json::Int(BARREL_POOL)),
        ("damage", Json::Int(BARREL_DAMAGE)),
        ("collider_local", pair_json(&shape)),
        ("gravity_scale", Json::Float(BARREL_GRAVITY_SCALE)),
        ("spawn_x", Json::List(BARREL_SPAWN_X.iter().map(|&f| Json::Float(f)).collect())),
        ("spawn_world", Json::List(spawn.iter().map(|&f| Json::Float(f)).collect())),
        ("gap_ticks", Json::List(BARREL_SPAWN_GAP.iter().map(|&g| Json::Int(ticks(g))).collect())),
        (
            "limitations",
            Json::List(
                [
                    "The 720 degrees per second `Rotate` the barrel spins at is not reproduced: the draw path emits an axis-aligned quad from the frame box and turning one would cost a rotation per barrel per frame on a stall-bound frame.",
                    "RandomScale 0.8 to 1.0 is not reproduced; every barrel is the authored size.",
                    "`Check Direct` is not reproduced: the source lets the nail knock a barrel away at 45 units/s, and here a barrel is not a nail target.",
                    "The break is the collider and the sprite going away. `Break`'s Bits and Dust Puff emitters, its Splat, its SpawnBlood and its one-shot are particles and audio.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}

/// `recognize_placement`: admit the one placed False Knight into the guest
/// actor pool, or refuse. `health` is the actor's HealthManager tree.
pub fn recognize_placement(sc: &Scene, source: &Source, gid: i64, health: &Value) -> Result<Json> {
    check_assemblies(source, "False Knight")?;
    if get(sc.go(gid).ok_or("no such GameObject")?, "m_Layer")?.int() != Some(11) {
        return err("False Knight outside the enemy layer");
    }
    let all = fsms(sc, gid);
    let mut names: Vec<&str> = all.iter().map(|f| f.0.as_str()).collect();
    names.sort();
    if names != ["Check Health", "FalseyControl"] {
        return err(format!("unsupported False Knight FSM set: {}", names.join(", ")));
    }
    let mut digests = Vec::new();
    for (name, expected) in [("FalseyControl", FALSEY_CONTROL_SHA256), ("Check Health", CHECK_HEALTH_SHA256)] {
        let data = all.iter().find(|f| f.0 == name).unwrap().1;
        if !get(data, "m_Enabled")?.truthy() {
            return err(format!("disabled {name:?} FSM"));
        }
        let digest = fsm_digest(get(data, "fsm")?, &[])?;
        if digest != expected {
            return err(format!("unverified {name} FSM variant: {digest}"));
        }
        digests.push((name.to_string(), Json::Str(digest)));
    }
    let control = get(all.iter().find(|f| f.0 == "FalseyControl").unwrap().1, "fsm")?;
    if get(control, "startState")?.str().as_deref() != Some("State 4") {
        return err("False Knight starts in an unsupported state");
    }
    if get(health, "hp")?.int() != Some(BODY_HEALTH) || !near(health.get("invulnerableTime"), BODY_INVULNERABLE_TIME) || !get(health, "hasSpecialDeath")?.truthy() || get(health, "invincible")?.truthy() || get(health, "damageOverride")?.truthy() {
        return err("unsupported False Knight HealthManager variant");
    }
    if !near_all(&box_of(sc, gid, 0)?, &BODY_BOX) {
        return err("unsupported False Knight body collider");
    }
    let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if !near(Some(&Value::F64(m[0][0].abs())), BODY_SCALE) || !near(Some(&Value::F64(m[1][1])), BODY_SCALE) || m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 {
        return err("unsupported False Knight rotation or scale");
    }
    let (library_source, _rows) = clip_contract(sc, source, gid)?;
    let facing_right = m[0][0] > 0.0;
    // `FalseKnight::new` starts the guest controller at Facing Right false, so a
    // placement mirrored the other way would start the fight facing the wrong
    // side. There is one placement and it faces left; refuse rather than disagree.
    if facing_right {
        return err("False Knight placement faces right; the guest controller starts facing left");
    }
    let turn = CLIPS.iter().find(|c| c.0 == "Turn").unwrap();
    let _ = (HEAD_HEALTH, py_round(0.0));
    Ok(jobj(vec![
        ("kind", js("FalseKnight")),
        ("guest_enabled", Json::Bool(true)),
        ("art_bindings", Json::Obj(ART_BINDINGS.iter().map(|(k, v)| (k.to_string(), js(v))).collect())),
        // Both HealthManagers restore rather than die, so nothing of the boss ever falls.
        ("no_corpse", Json::Bool(true)),
        // Crossroads_10 merges Crossroads_10_boss in for this actor.
        ("admit_from_additive_scene", Json::Bool(true)),
        ("facing_right", Json::Bool(facing_right)),
        ("initial_direction", Json::Int(if facing_right { -1 } else { 1 })),
        ("invincible", Json::Bool(false)),
        ("special_death", Json::Bool(true)),
        ("head_health", Json::Int(HEAD_HEALTH)),
        ("head_invulnerable_ticks", Json::Int(ticks(HIT_EVASION_SECONDS))),
        ("staggers", Json::Int(STAGGERS_TO_DEATH)),
        ("arena_trigger_world", Json::List(arena_trigger_world_box(sc)?.into_iter().map(Json::Float).collect())),
        ("barrel", barrel_source(sc, source)?),
        ("turn_ticks", Json::Int(ticks(turn.1 as f64 / turn.2))),
        ("library_source", Json::Str(library_source)),
        ("fsm_sha256", Json::Obj(digests)),
        (
            "limitations",
            Json::List(
                [
                    "Six of the thirty-four clips are cooked (Idle, Turn, Jump Antic, Land, Stun Opened, Attack); the rest of the fight plays the nearest of those. The limit is Crossroads_10's 393,216-byte room budget, not palettes: see docs/FALSE_KNIGHT.md.",
                    "BG OPEN and BG QUICK OPEN reach the two gates the source loads closed, which lift for good once the fight is won. BG CLOSE reaches the other three and moves nothing: they are open on the first frame, so host/cook.py bakes no terrain for them and the arena never seals at its ends.",
                    "The three pre-battle Zombies are recorded rather than admitted, so KILL ALL ENEMIES has no target; the arena counts only the boss, as the source does.",
                    "The Hitter overlay plays its attack clip on the body rather than as a second draw, and its DamageHero trigger is reproduced as a box.",
                    "The barrels fall and hurt. What the barrel does not do is spin, vary its scale or answer the nail; the break is the collider and the sprite going away, with no Bits, Dust Puff, Splat or one-shot.",
                    "The shockwave, the floor crack and break, the staff, the Death Head and every particle are recorded, not presented.",
                    "Rise and Fall shape vertical speed per 50 Hz FixedUpdate in the source and per 60 Hz tick in the guest, so airtime differs slightly.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
        ("source_methods", Json::List(["HealthManager.Hit", "HealthManager.Die", "tk2dSpriteAnimator.Play", "CameraLockArea.OnTriggerEnter2D"].iter().map(|s| js(s)).collect())),
    ]))
}
