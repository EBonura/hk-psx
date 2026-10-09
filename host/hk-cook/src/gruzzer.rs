//! Recognize the verified Gruzzer (Fly) `Bouncer Control` variant, its Gruz
//! Mother's reserve flies and the Gruz Mother (`Giant Fly`) itself for guest
//! admission. Ported from host/gruzzer.py.
//!
//! The controllers live in shared/hk-sim/src/gruzzer.rs and gruz_mother.rs; this
//! module admits only placed instances whose FSM parameters and body match the
//! audited contract.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::pyjson::Json;
use crate::recog::{check_assemblies, clip_is, clips_by_name, near, only, scalar, state, states, transitions, variables, xy};
use crate::runner::axis_aligned_bounds;
use hk_unity::playmaker::{action_fields, field};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

fn n(x: f64) -> Value {
    Value::F64(x)
}
fn identity() -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

const CLIPS: [(&str, usize, f64, i64); 1] = [("Fly", 7, 10.0, 0)];
const BODY_SIZE: [f64; 2] = [0.890625, 0.84375];
const BODY_OFFSET: [f64; 2] = [0.0390625, -0.03125];
const RANGES: [(&str, f64, f64); 9] = [
    ("Aim", 0.0, 360.0),
    ("Up Right", 320.0, 350.0),
    ("Up Left", 190.0, 220.0),
    ("Down Right", 10.0, 40.0),
    ("Up Left 2", 140.0, 170.0),
    ("Right Down", 190.0, 220.0),
    ("Down Right 2", 140.0, 170.0),
    ("Left Down", 320.0, 350.0),
    ("Left Up", 10.0, 40.0),
];
const TRANSITIONS: [(&str, &[(&str, &str)]); 10] = [
    ("Initialise", &[("FINISHED", "Aim")]),
    ("Aim", &[("FINISHED", "Left or Right?")]),
    ("Left or Right?", &[("LEFT", "Face Left"), ("RIGHT", "Face Right")]),
    ("Face Left", &[("FINISHED", "Fly 2")]),
    ("Face Right", &[("FINISHED", "Fly 2")]),
    ("Fly 2", &[("BONK DOWN", "Hit Down"), ("BONK LEFT", "Hit Left"), ("BONK RIGHT", "Hit Right"), ("BONK UP", "Hit Up")]),
    ("Hit Up", &[("RIGHT", "Up Right"), ("LEFT", "Up Left")]),
    ("Hit Down", &[("RIGHT", "Down Right"), ("LEFT", "Up Left 2")]),
    ("Hit Right", &[("UP", "Down Right 2"), ("DOWN", "Right Down")]),
    ("Hit Left", &[("UP", "Left Up"), ("DOWN", "Left Down")]),
];

fn strings(v: &[&str]) -> Json {
    Json::List(v.iter().map(|s| js(s)).collect())
}

// --- scene identity --------------------------------------------------------------

/// `hatcher.scene_bounds`: the scene's own runtime bounds, which is what makes a
/// placement a placement (an enemy the room does not contain is not standing
/// anywhere the player goes). `catalogue` is (file, runtime_bounds, scene_name) per scene.
pub fn scene_bounds(sc: &Scene, catalogue: &crate::actors::Catalogue) -> Result<[f64; 4]> {
    let name = hk_unity::base_name(&sc.base.name);
    catalogue.iter().find(|(file, ..)| name == file).map(|c| c.1).ok_or_else(|| "Hatcher placement outside the admitted scene table".to_string())
}

// --- Gruz Mother (`Giant Fly`, Crossroads_04) and its reserve flies ------------------
//
// * `recognize_giant_fly` admits the one `Giant Fly` into the guest actor pool.
// * `reserve_origin` answers whether a `Fly` parked below the room is one of the
//   seven `Fly Spawn` children the burster releases, which `recognize` admits as
//   `GruzzerReserve` placements instead of refusing them.
const GIANT_FLY_NAME: &str = "Giant Fly";
const GIANT_FLY_STATES: [&str; 28] = [
    "Init", "Invincible", "Sleep", "Wake Sound", "Wake", "Fly", "Buzz", "Super Choose", "Charge Antic", "Charge", "Charge Recover L", "Charge Recover R", "Charge Recover U", "Charge Recover D", "Recover End", "Super End", "Slam Antic", "Check Direction", "Go Left", "Go Right", "Launch Up", "Launch Down",
    "Flying", "Turn Left", "Turn Right", "Slam Down", "Slam Up", "Slam End",
];
const GIANT_FLY_CHILDREN: [&str; 3] = ["Hero Damager", "Snore", "Battle Range"];
const SPAWN_NAME: &str = "Fly Spawn";

/// `giant_fly`: the scene's one active `Giant Fly`, if any.
fn giant_fly(sc: &Scene) -> Result<Option<i64>> {
    let mut found: Vec<i64> = sc.gos.keys().copied().filter(|g| sc.go(*g).and_then(|go| go.get("m_Name")).and_then(Value::str).as_deref() == Some(GIANT_FLY_NAME) && sc.active(*g)).collect();
    found.sort();
    if found.len() > 1 {
        return err("more than one Giant Fly in the scene");
    }
    Ok(found.first().copied())
}

/// `reserve_origin`: `Fly Spawn`'s world position if `gid` is one of its
/// children in a scene with a Gruz Mother.
fn reserve_origin(sc: &Scene, gid: i64) -> Result<Option<[f64; 2]>> {
    let t = sc.transform(*sc.go_transform.get(&gid).ok_or("actor has no transform")?).ok_or("transform missing")?;
    let parent = get(get(t, "m_Father")?, "m_PathID")?.int().unwrap_or(0);
    if parent == 0 {
        return Ok(None);
    }
    let spawn = get(get(sc.transform(parent).ok_or("parent transform missing")?, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
    if get(sc.go(spawn).ok_or("no such GameObject")?, "m_Name")?.str().as_deref() != Some(SPAWN_NAME) || giant_fly(sc)?.is_none() {
        return Ok(None);
    }
    // `Spawn Flies 2` SetPosition on `Fly Spawn` is in world space, so its own
    // parent (if any) does not change where the flies land.
    let p = u(sc.point(spawn, 0.0, 0.0, 0.0))?;
    Ok(Some([p[0], p[1]]))
}

/// Admit the Gruzzer (or Gruz Mother reserve fly) placement at `gid`.
pub fn recognize(sc: &Scene, source: &Source, gid: i64, catalogue: &crate::actors::Catalogue) -> Result<Json> {
    let records = component_records(sc, gid);
    // Gruz Mother's reserve (Crossroads_04 `Fly Spawn/Fly`..`Fly 6`) waits below
    // the room until the burster moves `Fly Spawn` to itself. Any other Fly
    // outside the room is refused: position is the honest discriminator.
    let bounds = scene_bounds(sc, catalogue)?;
    let p = u(sc.point(gid, 0.0, 0.0, 0.0))?;
    let mut origin = None;
    if !(bounds[0] <= p[0] && p[0] <= bounds[2] && bounds[1] <= p[1] && p[1] <= bounds[3]) {
        origin = reserve_origin(sc, gid)?;
        if origin.is_none() {
            return err("Gruzzer parked outside the room is not a Fly Spawn reserve");
        }
    }
    check_assemblies(source, "Gruzzer")?;
    let fsms: Vec<&Value> = records.iter().filter(|r| r.1 == "PlayMakerFSM").filter_map(|r| r.2.get("fsm")).collect();
    if fsms.len() != 1 || get(fsms[0], "name")?.str().as_deref() != Some("Bouncer Control") || get(fsms[0], "startState")?.str().as_deref() != Some("Initialise") {
        return err("no single Gruzzer Bouncer Control FSM");
    }
    let fsm = fsms[0];
    let vars = variables(fsm);
    let var = |k: &str| vars.iter().find(|(name, _)| name == k).map(|(_, v)| *v);
    let zero = |v: Option<&Value>| v.is_some_and(|v| !matches!(v, Value::Str(_)) && v.float() == Some(0.0));
    if !near(var("Speed"), 5.2) || !zero(var("Start Up")) || !zero(var("Starts Inactive")) {
        return err("unsupported Gruzzer variables");
    }
    let sts = states(fsm)?;
    for (name, expected) in TRANSITIONS {
        let ok = state(&sts, name).map(transitions).transpose()?.is_some_and(|t| t.len() == expected.len() && t.iter().zip(expected.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1));
        if !ok {
            return err(format!("unsupported Gruzzer transitions: {name}"));
        }
    }
    for (name, low, high) in RANGES {
        let data = get(state(&sts, name).ok_or("missing state")?, "actionData")?;
        let names = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let matches: Vec<usize> = names.iter().enumerate().filter(|(i, nm)| nm.str().is_some_and(|s| s.ends_with("RandomFloat")) && enabled.get(*i).is_some_and(Value::truthy)).map(|(i, _)| i).collect();
        if matches.len() != 1 {
            return err(format!("unsupported Gruzzer aim: {name}"));
        }
        let f = u(action_fields(data, matches[0], false))?;
        let g = |k: &str| field(&f, k).map(scalar).ok_or_else(|| format!("missing field {k}"));
        if !near(Some(&g("min")?), low) || !near(Some(&g("max")?), high) || g("storeResult")?.str().as_deref() != Some("Angle") {
            return err(format!("unsupported Gruzzer aim range: {name}"));
        }
    }
    let data = get(state(&sts, "Initialise").ok_or("missing state")?, "actionData")?;
    let names = get(data, "actionNames")?.list().unwrap_or(&[]);
    let compare: Vec<_> = names.iter().enumerate().filter(|(_, nm)| nm.str().is_some_and(|s| s.ends_with("FloatCompare"))).map(|(i, _)| u(action_fields(data, i, false))).collect::<Result<_>>()?;
    if compare.len() != 1 || !near(field(&compare[0], "float2").map(scalar).as_ref(), 44.0) || field(&compare[0], "lessThan").and_then(Value::str).as_deref() != Some("FINISHED") {
        return err("unsupported Gruzzer camera range");
    }
    let data = get(state(&sts, "Fly 2").ok_or("missing state")?, "actionData")?;
    let all = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    let short: Vec<String> = all.iter().enumerate().filter(|(i, _)| enabled.get(*i).is_some_and(Value::truthy)).map(|(_, nm)| nm.str().unwrap_or_default().rsplit('.').next().unwrap_or("").to_string()).collect();
    if short.len() < 2 || short[..2] != ["FaceDirection", "SetVelocityAsAngle"] || short.iter().filter(|s| *s == "CheckCollisionSide").count() != 3 || short.iter().filter(|s| *s == "CheckCollisionSideEnter").count() != 3 {
        return err("unsupported Gruzzer flight actions");
    }
    let face = u(action_fields(data, 0, false))?;
    for k in ["spriteFacesRight", "playNewAnimation", "pauseBetweenTurns"] {
        if field(&face, k).map(scalar).ok_or_else(|| format!("missing field {k}"))?.truthy() {
            return err("unsupported Gruzzer facing");
        }
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    if (0..2).any(|i| (0..2).any(|j| (m[i][j] - if i == j { 1.0 } else { 0.0 }).abs() > 1e-6)) {
        return err("unsupported Gruzzer initial rotation or scale");
    }
    let (_, body) = only(&records, "BoxCollider2D", "Gruzzer")?;
    let (size, offset) = (xy(body, "m_Size")?, xy(body, "m_Offset")?);
    if !get(body, "m_Enabled")?.truthy() || get(body, "m_IsTrigger")?.truthy() || get(body, "m_EdgeRadius")?.float() != Some(0.0) || (0..2).any(|k| !near(Some(&n(size[k])), BODY_SIZE[k]) || !near(Some(&n(offset[k])), BODY_OFFSET[k])) {
        return err("unsupported Gruzzer body collider");
    }
    let (_, rigid) = only(&records, "Rigidbody2D", "Gruzzer")?;
    if get(rigid, "m_BodyType")?.int() != Some(0) || get(rigid, "m_GravityScale")?.float() != Some(0.0) || get(rigid, "m_LinearDamping")?.float() != Some(0.0) || get(rigid, "m_Constraints")?.int() != Some(4) {
        return err("unsupported Gruzzer rigid body");
    }
    let (_, recoil) = only(&records, "Recoil", "Gruzzer")?;
    if get(recoil, "freezeInPlace")?.truthy() || get(recoil, "recoilSpeedBase")?.float() != Some(15.0) || !near(recoil.get("recoilDuration"), 0.15) || get(recoil, "preventRecoilUp")?.truthy() {
        return err("unsupported Gruzzer recoil variant");
    }
    let (_, animator) = only(&records, "tk2dSpriteAnimator", "Gruzzer")?;
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let by_name = clips_by_name(&library)?;
    for (name, frames, fps, wrap) in CLIPS {
        if !clip_is(by_name.iter().find(|(k, _)| k == name).map(|(_, v)| *v), frames, fps, wrap) {
            return err(format!("unsupported Gruzzer animation: {name}"));
        }
    }
    let bounds = axis_aligned_bounds(&identity(), BODY_OFFSET, BODY_SIZE)?;
    let bounds_json = Json::List(bounds.iter().map(|&f| Json::Float(f)).collect());
    if let Some(o) = origin {
        return Ok(jobj(vec![
            ("kind", js("GruzzerReserve")),
            ("guest_enabled", Json::Bool(true)),
            ("speed", Json::Float(5.2)),
            ("body_bounds_local", bounds_json),
            ("origin", Json::List(o.iter().map(|&v| Json::Int(py_round(v * 65536.0))).collect())),
            ("art_bindings", jobj(vec![("walk", js("Fly")), ("turn", js("Fly"))])),
            (
                "limitations",
                strings(&[
                    "Parked below the room with no tick, draw or hit until Gruz Mother's burster releases it at itself plus its offset from Fly Spawn; then an ordinary Gruzzer.",
                    "Each death decrements the arena's Battle Enemies (HealthManager.battleScene).",
                ]),
            ),
        ]));
    }
    Ok(jobj(vec![
        ("kind", js("Gruzzer")),
        ("guest_enabled", Json::Bool(true)),
        ("speed", Json::Float(5.2)),
        ("body_bounds_local", bounds_json),
        (
            "limitations",
            strings(&[
                "Gravity-free dynamic body on the bounded terrain solver; bonk sides come from the blocked solver axis instead of the three 0.08 contact rays.",
                "The live buzz loop audio is not presented; SetZ depth randomization and FSMActivator staggering are ignored.",
                "The breaker corpse smashes on its third landing without break pieces and does not spin.",
            ]),
        ),
    ]))
}

/// Python's `repr()` of `sorted((name, tuple(action names)))` over the states.
pub(crate) fn fsm_state_signature(control: &Value) -> Result<String> {
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for s in get(control, "states")?.list().ok_or("states is not a list")? {
        let names = get(get(s, "actionData")?, "actionNames")?.list().unwrap_or(&[]).iter().map(|x| x.str().unwrap_or_default()).collect();
        rows.push((get(s, "name")?.str().unwrap_or_default(), names));
    }
    rows.sort();
    let tuple = |v: &[String]| match v.len() {
        0 => "()".to_string(),
        1 => format!("({},)", crate::music_report::py_repr(&v[0])),
        _ => format!("({})", v.iter().map(|x| crate::music_report::py_repr(x)).collect::<Vec<_>>().join(", ")),
    };
    Ok(format!("[{}]", rows.iter().map(|(name, actions)| format!("({}, {})", crate::music_report::py_repr(name), tuple(actions))).collect::<Vec<_>>().join(", ")))
}

/// `recognize_giant_fly`: admit the placed Gruz Mother, or refuse with the reason.
pub fn recognize_giant_fly(sc: &Scene, gid: i64, health: &Value) -> Result<Json> {
    let go = sc.go(gid).ok_or("no such GameObject")?;
    if get(go, "m_Name")?.str().as_deref() != Some(GIANT_FLY_NAME) || get(go, "m_Layer")?.int() != Some(11) {
        return err("not the Giant Fly on the enemy layer");
    }
    let mut fsms: Vec<(String, &Value)> = Vec::new();
    for (_, kind, data) in component_records(sc, gid) {
        if kind == "PlayMakerFSM" {
            let name = data.get("fsm").and_then(|f| f.get("name")).and_then(Value::str).unwrap_or_default();
            match fsms.iter_mut().find(|f| f.0 == name) {
                Some(slot) => slot.1 = data,
                None => fsms.push((name, data)),
            }
        }
    }
    let mut names: Vec<&str> = fsms.iter().map(|f| f.0.as_str()).collect();
    names.sort();
    if names != ["Big Fly Control", "bouncer_control"] || !fsms.iter().all(|f| f.1.get("m_Enabled").is_some_and(Value::truthy)) {
        return err(format!("unsupported Giant Fly FSM set: {}", names.join(", ")));
    }
    let control = get(fsms.iter().find(|f| f.0 == "Big Fly Control").unwrap().1, "fsm")?;
    let have: Vec<String> = get(control, "states")?.list().unwrap_or(&[]).iter().filter_map(|s| s.get("name").and_then(Value::str)).collect();
    let missing: Vec<&str> = GIANT_FLY_STATES.iter().copied().filter(|s| !have.iter().any(|h| h == s)).collect();
    if !missing.is_empty() {
        return err(format!("Big Fly Control lacks {}", missing.join(", ")));
    }
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let mut children: Vec<String> = Vec::new();
    for o in sc.objects.iter().filter(|o| o.typename == "Transform") {
        if get(get(&o.tree, "m_Father")?, "m_PathID")?.int() != Some(tid) {
            continue;
        }
        let g = get(get(&o.tree, "m_GameObject")?, "m_PathID")?.int().unwrap_or(0);
        children.push(get(sc.go(g).ok_or("child without a GameObject")?, "m_Name")?.str().unwrap_or_default());
    }
    let missing: Vec<&str> = GIANT_FLY_CHILDREN.iter().copied().filter(|c| !children.iter().any(|h| h == c)).collect();
    if !missing.is_empty() {
        return err(format!("Giant Fly lacks {}", missing.join(", ")));
    }
    // Serialized invincible; `Sleep` clears it once the hero is in range.
    if !get(health, "invincible")?.truthy() || get(health, "hasSpecialDeath")?.truthy() || get(health, "damageOverride")?.truthy() || get(health, "invincibleFromDirection")?.truthy() || get(health, "hasAlternateHitAnimation")?.truthy() {
        return err("unsupported Giant Fly HealthManager variant");
    }
    let m = u(sc.world(tid))?;
    if m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 || m[0][0] <= 0.0 {
        return err("Giant Fly is rotated or authored mirrored");
    }
    Ok(jobj(vec![
        ("kind", js("GruzMother")),
        ("guest_enabled", Json::Bool(true)),
        // The generic actor bank binds one real one-frame clip for the two slots every ActorSpec carries.
        ("art_bindings", jobj(vec![("walk", js("Charge")), ("turn", js("Charge"))])),
        // The corpse prefab has no Rigidbody2D and runs its own FSM; the controller owns it.
        ("no_corpse", Json::Bool(true)),
        ("initial_direction", Json::Int(-1)),
        ("fsm_sha256", Json::Str(sha(fsm_state_signature(control)?.as_bytes()))),
        (
            "limitations",
            strings(&[
                "Every clip, the corpse and the burster are cooked by host/gruz_mother_art.py into this scene alone; the ActorSpec clip fields point at the one-frame Charge clip.",
                "CheckCollisionSide is the blocked axis of the bounded terrain step, not three rays a side.",
                "The snore zzz, the dust, slam effects, rocks, blood and steam particles are not presented.",
            ]),
        ),
    ]))
}
