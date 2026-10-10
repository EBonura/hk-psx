//! Corpse, projectile and pooled-effect art for the actors a scene admits. Ported from
//! host/effects.py (`corpse_source` and its families, `_ClipCooker`, `append_corpse_art`,
//! `append_shot_art`, `append_guard_art`).
//!
//! Every family reads its prefab strictly and refuses a variant it was not written for, so a
//! changed source names the field rather than cooking the wrong body.

use crate::actor_art::{guest_wrap, ArtActor, ArtBank, Clip, Frame};
use crate::atlas::Quantizer;
use crate::common::{components, err, get, py_round, Result};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::cook_audio::u;
use crate::fsm_pins::{check_action, state as pins_state, Pin};
use crate::music::value_json;
use crate::prefab::num;
use crate::pyjson::Json;
use crate::runner::ticks;
use hk_pil::Image;
use hk_unity::playmaker::{action_fields, field, Fields};
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// A corpse record before its clips are cooked: the dict fields Python returned (without the
/// popped `library_object`, `clips` and `tiled` entries) plus those three.
pub struct CorpseSource {
    pub fields: Vec<(String, Json)>,
    pub library: Obj,
    pub clips: Vec<Value>,
    pub tiled: bool,
}

fn jv(v: &Value) -> Json {
    value_json(v)
}

fn ji(v: i64) -> Json {
    Json::Int(v)
}

fn jl(v: &[i64]) -> Json {
    Json::List(v.iter().map(|&x| Json::Int(x)).collect())
}

fn jstr(s: &str) -> Json {
    Json::Str(s.to_string())
}

fn q16(v: f64) -> i64 {
    py_round(v * 65536.0)
}

fn flt(v: &Value, k: &str) -> Result<f64> {
    num(get(v, k)?).ok_or_else(|| format!("{k} is not a number"))
}

fn flag(v: &Value, k: &str) -> Result<bool> {
    Ok(get(v, k)?.truthy())
}

/// `abs(v[k] - x) > 1e-6`.
fn far(v: &Value, k: &str, x: f64) -> Result<bool> {
    Ok((flt(v, k)? - x).abs() > 1e-6)
}

fn vec3(x: f64, y: f64, z: f64) -> Value {
    Value::Map(vec![
        ("x".into(), Value::F64(x)),
        ("y".into(), Value::F64(y)),
        ("z".into(), Value::F64(z)),
    ])
}

fn unit_color() -> Value {
    Value::Map(vec![
        ("r".into(), Value::F64(1.0)),
        ("g".into(), Value::F64(1.0)),
        ("b".into(), Value::F64(1.0)),
        ("a".into(), Value::F64(1.0)),
    ])
}

/// A prefab's components by type name.
struct Prefab {
    obj: Obj,
    go: Value,
    parts: Vec<(String, Obj, Value)>,
}

impl Prefab {
    fn read(source: &Source, sc: &Scene, reference: &Value, strict: bool) -> Result<Prefab> {
        let obj = u(sc.deref(reference))?;
        let go = u(source.read(&obj))?;
        let mut parts: Vec<(String, Obj, Value)> = Vec::new();
        for r in get(&go, "m_Component")?.list().unwrap_or(&[]) {
            let component = u(source.deref(&obj.file, get(r, "component")?))?;
            let typ = u(source.typename(&component))?;
            let tree = u(source.read(&component))?;
            match parts.iter_mut().find(|p| p.0 == typ) {
                Some(_) if strict => return err("duplicate corpse component"),
                Some(slot) => *slot = (typ, component, tree),
                None => parts.push((typ, component, tree)),
            }
        }
        Ok(Prefab { obj, go, parts })
    }

    fn has(&self, kind: &str) -> bool {
        self.parts.iter().any(|p| p.0 == kind)
    }

    fn part(&self, kind: &str) -> Result<&Value> {
        self.parts
            .iter()
            .find(|p| p.0 == kind)
            .map(|p| &p.2)
            .ok_or_else(|| format!("incomplete corpse prefab: {kind}"))
    }

    fn name(&self) -> String {
        self.go
            .get("m_Name")
            .and_then(Value::str)
            .unwrap_or_default()
    }
}

fn named_clip<'a>(library: &'a Value, name: &str) -> Result<&'a Value> {
    get(library, "clips")?
        .list()
        .unwrap_or(&[])
        .iter()
        .find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name))
        .ok_or_else(|| format!("no clip {name}"))
}

fn timing(c: &Value) -> Result<(f64, i64, usize)> {
    Ok((
        flt(c, "fps")?,
        get(c, "wrapMode")?.int().unwrap_or(-1),
        get(c, "frames")?.list().map_or(0, <[Value]>::len),
    ))
}

fn loop_start(c: &Value) -> i64 {
    c.get("loopStart").and_then(Value::int).unwrap_or(0)
}

fn has_trigger(c: &Value) -> bool {
    get(c, "frames")
        .ok()
        .and_then(Value::list)
        .unwrap_or(&[])
        .iter()
        .any(|f| f.get("triggerEvent").is_some_and(Value::truthy))
}

fn death_effects<'a>(records: &[(i64, &str, &'a Value)]) -> Result<&'a Value> {
    let deaths: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "EnemyDeathEffects")
        .map(|r| r.2)
        .collect();
    if deaths.len() != 1 {
        return err("actor needs exactly one EnemyDeathEffects");
    }
    Ok(deaths[0])
}

fn actor_scale(sc: &Scene, gid: i64) -> Result<([[f64; 4]; 4], f64, f64)> {
    let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    Ok((m, m[0][0].abs(), m[1][1].abs()))
}

fn untinted(sprite: &Value, transform: &Value) -> Result<bool> {
    Ok(get(sprite, "_color")?.py_eq(&unit_color())
        && get(sprite, "_scale")?.py_eq(&vec3(1.0, 1.0, 1.0))
        && get(transform, "m_LocalScale")?.py_eq(&vec3(1.0, 1.0, 1.0)))
}

fn pair(a: &Value, b: &Value) -> Vec<Value> {
    vec![a.clone(), b.clone()]
}

/// `corpse_source(source, scene, actor)`: the verified corpse of a controller, or `None` when it leaves none.
pub fn corpse_source(
    source: &Source,
    sc: &Scene,
    actor: &ArtActor,
) -> Result<Option<CorpseSource>> {
    let control = actor.control().cloned().unwrap_or(Json::Obj(Vec::new()));
    let ctl = |k: &str| -> Option<&Json> {
        if let Json::Obj(f) = &control {
            f.iter().find(|x| x.0 == k).map(|x| &x.1)
        } else {
            None
        }
    };
    let mut kind = match ctl("kind") {
        Some(Json::Str(k)) => k.clone(),
        _ => String::new(),
    };
    // A Gruz Mother reserve fly is the Gruzzer itself, `Corpse Fly` and all.
    if kind == "GruzzerReserve" {
        kind = "Gruzzer".into();
    }
    let gid = actor.row.game_object;
    match kind.as_str() {
        "Vengefly" | "Gruzzer" | "Mosquito" => {
            return breaker_corpse(source, sc, gid, &kind).map(Some)
        }
        "Baldur" => return roller_corpse(source, sc, gid).map(Some),
        "Aspid" => return aspid_corpse(source, sc, gid).map(Some),
        "AcidFlyer" => return acid_flyer_corpse(source, sc, gid).map(Some),
        "FatFly" => return fat_fly_corpse(source, sc, gid).map(Some),
        "PlantTrap" => {
            let lib = match ctl("library_source") {
                Some(Json::Str(s)) => s.clone(),
                _ => return err("Plant Trap without a library source"),
            };
            return plant_trap_corpse(source, sc, gid, &lib).map(Some);
        }
        "EggSac" => {
            let lib = match ctl("library_source") {
                Some(Json::Str(s)) => s.clone(),
                _ => String::new(),
            };
            return egg_sac_corpse(source, sc, gid, &lib).map(Some);
        }
        "HuskGuard" => {
            let lib = match ctl("library_source") {
                Some(Json::Str(s)) => s.clone(),
                _ => String::new(),
            };
            return guard_corpse(source, sc, gid, &lib).map(Some);
        }
        _ => {}
    }
    // A controller whose recognizer states it leaves no corpse is answered, not refused.
    if ctl("no_corpse").is_some_and(|j| matches!(j, Json::Bool(true))) {
        return Ok(None);
    }
    if ![
        "WalkLeftRight",
        "ZombieSwipeWalker",
        "Climber",
        "ZombieShield",
        "MossWalker",
        "MossCharger",
    ]
    .contains(&kind.as_str())
    {
        return err("unvalidated corpse controller family");
    }
    let library_source = match ctl("library_source") {
        Some(Json::Str(s)) => Some(s.clone()),
        _ => None,
    };
    // The Greenpath mossmen share the Runner's Walker and corpse shape under their own prefabs and clips.
    let variant = match ctl("variant") {
        Some(Json::Str(v)) => Some(v.clone()),
        _ => None,
    };
    zombie_corpse(source, sc, gid, &kind, library_source, variant.as_deref()).map(Some)
}

/// The conventional Zombie/Crawler/Climber/Moss Walker corpse.
fn zombie_corpse(
    source: &Source,
    sc: &Scene,
    gid: i64,
    kind: &str,
    library_source: Option<String>,
    variant: Option<&str>,
) -> Result<CorpseSource> {
    let runner = kind == "ZombieSwipeWalker" || kind == "ZombieShield";
    let climber = kind == "Climber";
    // `Corpse Moss Crawler`: the Crawler's launch and offset, bounce 0.2, a four-frame looping Death Air.
    let moss = kind == "MossWalker";
    let expected_offset = vec3(0.0, if runner || climber { 0.0 } else { 0.5 }, 0.0);
    let expected_bounce = if runner || moss {
        0.2
    } else if climber {
        0.45
    } else {
        0.3
    };
    let charger = kind == "MossCharger";
    let shaker = kind == "ZombieSwipeWalker" && variant == Some("Shaker");
    let mossman = kind == "ZombieSwipeWalker" && variant == Some("Mossman");
    let expected_timing: [(f64, i64, usize); 2] = if mossman {
        [(12.0, 2, 3), (12.0, 2, 3)]
    } else if shaker {
        [(12.0, 2, 3), (12.0, 1, 4)]
    } else if runner {
        [(30.0, 6, 1), (12.0, 2, 8)]
    } else if climber {
        [(30.0, 0, 8), (15.0, 2, 3)]
    } else if moss {
        [(12.0, 0, 4), (12.0, 2, 2)]
    } else if charger {
        [(12.0, 2, 2), (12.0, 2, 2)]
    } else {
        [(12.0, 2, 3), (12.0, 2, 2)]
    };
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    if [
        "isCorpseRecyclable",
        "corpseFacesRight",
        "lowCorpseArc",
        "rotateCorpse",
    ]
    .iter()
    .any(|k| death.get(k).is_some_and(Value::truthy))
        || flt(death, "corpseFlingSpeed")? != if climber { 20.0 } else { 15.0 }
    {
        return err("unsupported corpse launch");
    }
    let offset = get(death, "corpseSpawnPoint")?;
    if !offset.py_eq(&expected_offset) {
        return err("unsupported corpse spawn offset");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    // Runner, Barger and Hornhead corpses share one structure; identity is checked by name and player data.
    let name = prefab.name();
    if runner
        && !(name.starts_with("Corpse Zombie Basic ")
            || name == "Corpse Zombie Leaper"
            || name == "Corpse Zombie Shield"
            || (mossman && name == "Corpse_Mossman_Runner")
            || (shaker && name == "Corpse_Mossman_Shaker"))
    {
        return err(format!(
            "unvalidated corpse prefab for controller: {kind} spawns {name:?}"
        ));
    }
    // A killed Shaker's corpse is `CorpseFungusExplode`: it lands like any other and then bursts into gas.
    let corpse_part = if shaker {
        "CorpseFungusExplode"
    } else {
        "Corpse"
    };
    if [
        corpse_part,
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "Rigidbody2D",
        "BoxCollider2D",
        "ObjectBounce",
        "Transform",
    ]
    .iter()
    .any(|k| !prefab.has(k))
    {
        return err("incomplete corpse prefab");
    }
    let (corpse, body, bounce) = (
        prefab.part(corpse_part)?,
        prefab.part("Rigidbody2D")?,
        prefab.part("ObjectBounce")?,
    );
    // resetRotation only matters for a rotated owner; the guest corpse is drawn upright.
    let mut special = vec![
        "breaker",
        "bigBreaker",
        "chunker",
        "deathStun",
        "fungusExplode",
        "goopExplode",
        "hatcher",
        "instantChunker",
        "massless",
        "spineBurst",
        "zomHive",
    ];
    if !(climber || moss) {
        special.push("resetRotation");
    }
    if shaker {
        special.retain(|k| *k != "fungusExplode");
        if !flag(corpse, "fungusExplode")? {
            return err("Shaker corpse does not explode");
        }
        corpse_gas_box(source, &prefab)?;
    }
    for k in &special {
        if flag(corpse, k)? {
            return err("unsupported special corpse");
        }
    }
    if get(get(corpse, "landEffects")?, "m_PathID")?.truthy() {
        return err("unsupported additional corpse land effects");
    }
    if get(body, "m_BodyType")?.int() != Some(0)
        || flt(body, "m_LinearDamping")? != 0.0
        || far(body, "m_GravityScale", 0.8)?
        || get(body, "m_Constraints")?.int() != Some(4)
    {
        return err("unsupported corpse body");
    }
    if far(bounce, "bounceFactor", expected_bounce)?
        || flt(bounce, "speedThreshold")? != 1.0
        || ["playSound", "playAnimationOnBounce", "sendFSMEvent"]
            .iter()
            .any(|k| bounce.get(k).is_some_and(Value::truthy))
    {
        return err("unsupported corpse bounce");
    }
    let (sprite, anim, transform) = (
        prefab.part("tk2dSprite")?,
        prefab.part("tk2dSpriteAnimator")?,
        prefab.part("Transform")?,
    );
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    let selected = vec![
        named_clip(&library, "Death Air")?.clone(),
        named_clip(&library, "Death Land")?.clone(),
    ];
    if runner && Some(library_o.sid()) != library_source {
        return err("corpse animation library differs from the actor library");
    }
    let got: Vec<(f64, i64, usize)> = selected.iter().map(timing).collect::<Result<_>>()?;
    if got != expected_timing {
        return err("unvalidated corpse clip timing");
    }
    let starts: Vec<i64> = selected.iter().map(loop_start).collect();
    if starts != if shaker { [0, 1] } else { [0, 0] } || selected.iter().any(has_trigger) {
        return err("unsupported corpse animation events/loop start");
    }
    let (matrix, sx, sy) = actor_scale(sc, gid)?;
    let bx = prefab.part("BoxCollider2D")?;
    let (off, size) = (get(bx, "m_Offset")?, get(bx, "m_Size")?);
    if runner {
        let death_enabled = flag(death, "m_Enabled")?;
        let data_name = get(death, "playerDataName")?.str().unwrap_or_default();
        if !death_enabled
            || ![
                "ZombieRunner",
                "ZombieBarger",
                "ZombieHornhead",
                "ZombieLeaper",
                "ZombieShield",
                "MossmanRunner",
                "MossmanShaker",
            ]
            .contains(&data_name.as_str())
        {
            return err("unvalidated Runner corpse identity");
        }
        // A mirrored placement is admitted, so accept a plain x mirror; only the basis shape needs checking.
        if (sx - 1.0).abs() > 1e-6
            || (matrix[1][1] - 1.0).abs() > 1e-6
            || matrix[0][1].abs() > 1e-6
            || matrix[1][0].abs() > 1e-6
        {
            return err("unsupported Runner corpse actor transform");
        }
        let (szx, szy) = (flt(size, "x")?, flt(size, "y")?);
        if !flag(bx, "m_Enabled")?
            || flag(bx, "m_IsTrigger")?
            || flt(bx, "m_EdgeRadius")? != 0.0
            || !(0.0 < szx && szx < 4.0 && 0.0 < szy && szy < 4.0)
        {
            return err("unsupported Runner corpse collider");
        }
        if !flag(body, "m_Simulated")?
            || get(body, "m_CollisionDetection")?.int() != Some(0)
            || !flag(corpse, "m_Enabled")?
            || !flag(bounce, "m_Enabled")?
        {
            return err("unsupported Runner corpse component activation");
        }
        let default_clip = get(anim, "defaultClipId")?.int().unwrap_or(-1);
        let default_name = get(&library, "clips")?
            .list()
            .unwrap_or(&[])
            .get(default_clip as usize)
            .and_then(|c| c.get("name"))
            .and_then(Value::str);
        if !flag(anim, "m_Enabled")?
            || !flag(anim, "playAutomatically")?
            || flag(anim, "isRealtime")?
            || default_name.as_deref() != Some("Death Air")
        {
            return err("unsupported Runner corpse animator startup");
        }
        let material_o = u(source.deref(&prefab.obj.file, get(bx, "m_Material")?))?;
        let material = u(source.read(&material_o))?;
        if material_o.sid() != "resources.assets:1073"
            || far(&material, "friction", 0.2)?
            || flt(&material, "bounciness")? != 0.0
        {
            return err("unsupported Runner corpse physics material");
        }
    }
    let (ox, oy, w, h) = (
        flt(off, "x")?,
        flt(off, "y")?,
        flt(size, "x")?,
        flt(size, "y")?,
    );
    let bounds = [
        (ox - w / 2.0) * sx,
        (oy - h / 2.0) * sy,
        (ox + w / 2.0) * sx,
        (oy + h / 2.0) * sy,
    ];
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&bounds.map(q16))),
        ("spawn_offset".into(), jl(&[q16(flt(offset, "x")?), q16(flt(offset, "y")?)])),
        ("gravity".into(), ji(48 * 65536)),
        ("breaker".into(), Json::Bool(false)),
        ("fling_speed".into(), ji(q16(flt(death, "corpseFlingSpeed")?))),
        ("bounce_factor".into(), ji(q16(expected_bounce))),
        ("land_delay_ticks".into(), ji(60)),
        ("gas".into(), Json::Bool(shaker)),
        ("limitations".into(), Json::List(vec![jstr("Corpse Steam/Flame and infected wave/spatter remain separate particle/effect work"), jstr("Fixed-point terrain solver is not complete Box2D; bounce RNG is deterministic per source, not Unity global RNG")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: selected,
        tiled: charger || mossman || shaker,
    })
}

fn launch_flags(death: &Value) -> bool {
    [
        "isCorpseRecyclable",
        "corpseFacesRight",
        "lowCorpseArc",
        "rotateCorpse",
    ]
    .iter()
    .any(|k| death.get(k).is_some_and(Value::truthy))
}

/// Buzzer, Gruzzer (`Corpse Fly`) and Mosquito: a breaker that leaves on landing.
fn breaker_corpse(source: &Source, sc: &Scene, gid: i64, kind: &str) -> Result<CorpseSource> {
    let (gruzzer, mosquito) = (kind == "Gruzzer", kind == "Mosquito");
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    if launch_flags(death)
        || flt(death, "corpseFlingSpeed")? != if gruzzer { 20.0 } else { 15.0 }
        || !get(death, "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.0, 0.0))
    {
        return err("unsupported corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if [
        "Corpse",
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "Rigidbody2D",
        "BoxCollider2D",
        "Transform",
    ]
    .iter()
    .any(|k| !prefab.has(k))
        || prefab.has("ObjectBounce") != gruzzer
    {
        return err("unsupported breaker corpse prefab");
    }
    let (corpse, body) = (prefab.part("Corpse")?, prefab.part("Rigidbody2D")?);
    let smash = get(corpse, "smashBounces")?.int();
    if !flag(corpse, "breaker")?
        || smash != Some(if gruzzer { 3 } else { 0 })
        || [
            "bigBreaker",
            "chunker",
            "deathStun",
            "fungusExplode",
            "goopExplode",
            "hatcher",
            "instantChunker",
            "massless",
            "spineBurst",
            "zomHive",
            "resetRotation",
        ]
        .iter()
        .any(|k| corpse.get(k).is_some_and(Value::truthy))
    {
        return err("unsupported special corpse");
    }
    if get(get(corpse, "landEffects")?, "m_PathID")?.truthy() {
        return err("unsupported additional corpse land effects");
    }
    if get(body, "m_BodyType")?.int() != Some(0)
        || flt(body, "m_LinearDamping")? != 0.0
        || far(body, "m_GravityScale", 0.7)?
        || get(body, "m_Constraints")?.int() != Some(if gruzzer { 0 } else { 4 })
    {
        return err("unsupported corpse body");
    }
    let mut bounce = 0.0;
    if gruzzer {
        let ob = prefab.part("ObjectBounce")?;
        if far(ob, "bounceFactor", 0.7)?
            || flt(ob, "speedThreshold")? != 1.0
            || ["playSound", "playAnimationOnBounce", "sendFSMEvent"]
                .iter()
                .any(|k| ob.get(k).is_some_and(Value::truthy))
        {
            return err("unsupported corpse bounce");
        }
        bounce = 0.7;
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let bx = prefab.part("BoxCollider2D")?;
    let (off, size) = (get(bx, "m_Offset")?, get(bx, "m_Size")?);
    if !flag(bx, "m_Enabled")? || flag(bx, "m_IsTrigger")? || flt(bx, "m_EdgeRadius")? != 0.0 {
        return err("unsupported breaker corpse collider");
    }
    let (ox, oy, w, h) = (
        flt(off, "x")?,
        flt(off, "y")?,
        flt(size, "x")?,
        flt(size, "y")?,
    );
    let bounds = [
        (ox - w / 2.0) * sx,
        (oy - h / 2.0) * sy,
        (ox + w / 2.0) * sx,
        (oy + h / 2.0) * sy,
    ];
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    let selected = vec![
        named_clip(&library, "Death Air")?.clone(),
        named_clip(&library, "Death Land")?.clone(),
    ];
    let want: [(f64, i64, usize); 2] = if gruzzer {
        [(12.0, 2, 2), (15.0, 2, 6)]
    } else if mosquito {
        [(12.0, 2, 3), (30.0, 2, 1)]
    } else {
        [(12.0, 2, 3), (12.0, 2, 3)]
    };
    let got: Vec<(f64, i64, usize)> = selected.iter().map(timing).collect::<Result<_>>()?;
    if got != want {
        return err("unvalidated corpse clip timing");
    }
    if selected
        .iter()
        .any(|c| loop_start(c) != 0 || has_trigger(c))
    {
        return err("unsupported corpse animation events/loop start");
    }
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&bounds.map(q16))),
        ("spawn_offset".into(), jl(&[0, 0])),
        ("gravity".into(), ji(42 * 65536)),
        ("breaker".into(), Json::Bool(true)),
        ("smash_bounces".into(), ji(if gruzzer { 3 } else { 0 })),
        ("fling_speed".into(), ji(q16(flt(death, "corpseFlingSpeed")?))),
        ("bounce_factor".into(), ji(q16(bounce))),
        ("land_delay_ticks".into(), ji(0)),
        ("limitations".into(), Json::List(vec![jstr("Break pieces, spatter and infected wave on landing are not presented; the corpse is removed"), jstr("Fixed-point terrain solver is not complete Box2D")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: selected,
        tiled: false,
    })
}

/// `Corpse Roller Spawned`: a rolling circle body whose FSM shrinks and destroys it once it slows.
fn roller_corpse(source: &Source, sc: &Scene, gid: i64) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    let sp = get(death, "corpseSpawnPoint")?;
    if launch_flags(death)
        || flt(death, "corpseFlingSpeed")? != 15.0
        || flt(sp, "x")? != 0.0
        || far(sp, "y", 0.2)?
    {
        return err("unsupported corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if prefab.name() != "Corpse Roller Spawned" {
        return err("unvalidated Roller corpse identity");
    }
    // No Corpse component: the `corpse` FSM owns landing, shrink and destroy.
    if [
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "Rigidbody2D",
        "CircleCollider2D",
        "Transform",
        "PlayMakerFSM",
    ]
    .iter()
    .any(|k| !prefab.has(k))
        || prefab.has("ObjectBounce")
        || prefab.has("Corpse")
    {
        return err("unsupported Roller corpse prefab");
    }
    let body = prefab.part("Rigidbody2D")?;
    if get(body, "m_BodyType")?.int() != Some(0)
        || far(body, "m_LinearDamping", 0.7)?
        || far(body, "m_GravityScale", 0.8)?
        || get(body, "m_Constraints")?.int() != Some(0)
    {
        return err("unsupported corpse body");
    }
    let fsm = get(prefab.part("PlayMakerFSM")?, "fsm")?;
    let names: Vec<String> = get(fsm, "states")?
        .list()
        .unwrap_or(&[])
        .iter()
        .map(|s| s.get("name").and_then(Value::str).unwrap_or_default())
        .collect();
    if get(fsm, "name")?.str().as_deref() != Some("corpse")
        || names
            != [
                "Initiate",
                "In Air",
                "Landed",
                "Shrink",
                "Flame Check",
                "Start Flame",
                "Destroy",
            ]
    {
        return err("unsupported Roller corpse FSM");
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let circle = prefab.part("CircleCollider2D")?;
    let (off, r) = (get(circle, "m_Offset")?, flt(circle, "m_Radius")?);
    if flag(circle, "m_IsTrigger")?
        || (r - 0.43).abs() > 1e-6
        || flt(off, "x")? != 0.0
        || (flt(off, "y")? + 0.2).abs() > 1e-6
    {
        return err("unsupported Roller corpse collider");
    }
    let (ox, oy) = (flt(off, "x")?, flt(off, "y")?);
    let bounds = [(ox - r) * sx, (oy - r) * sy, (ox + r) * sx, (oy + r) * sy];
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    let selected = vec![
        named_clip(&library, "Death Air")?.clone(),
        named_clip(&library, "Death Land")?.clone(),
    ];
    let got: Vec<(f64, i64, usize)> = selected.iter().map(timing).collect::<Result<_>>()?;
    if got != [(12.0, 2, 3), (30.0, 6, 1)] {
        return err("unvalidated corpse clip timing");
    }
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&bounds.map(q16))),
        ("spawn_offset".into(), jl(&[0, q16(0.2)])),
        ("gravity".into(), ji(48 * 65536)),
        ("breaker".into(), Json::Bool(false)),
        ("smash_bounces".into(), ji(0)),
        ("remove_after_land".into(), ji(120)),
        ("fling_speed".into(), ji(15 * 65536)),
        ("bounce_factor".into(), ji(0)),
        ("land_delay_ticks".into(), ji(60)),
        ("limitations".into(), Json::List(vec![jstr("Circle body as its bounding box; the solver .2 slide replaces 0.7 linear damping; no spin"), jstr("Removed 120 ticks after landing instead of the source slow-down/shrink/destroy sequence")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: selected,
        tiled: false,
    })
}

/// `Corpse Spitter`: gravity .7, no collider (falls out of the scene).
fn aspid_corpse(source: &Source, sc: &Scene, gid: i64) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    if launch_flags(death)
        || flt(death, "corpseFlingSpeed")? != 15.0
        || !get(death, "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.0, 0.0))
    {
        return err("unsupported corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if prefab.name() != "Corpse Spitter" {
        return err("unvalidated Aspid corpse identity");
    }
    if [
        "Corpse",
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "Rigidbody2D",
        "Transform",
    ]
    .iter()
    .any(|k| !prefab.has(k))
        || ["BoxCollider2D", "CircleCollider2D", "ObjectBounce"]
            .iter()
            .any(|k| prefab.has(k))
    {
        return err("unsupported Aspid corpse prefab");
    }
    let (corpse, body) = (prefab.part("Corpse")?, prefab.part("Rigidbody2D")?);
    // `massless` is the source flag for a corpse without a collider.
    if !flag(corpse, "massless")?
        || [
            "breaker",
            "bigBreaker",
            "chunker",
            "deathStun",
            "fungusExplode",
            "goopExplode",
            "hatcher",
            "instantChunker",
            "spineBurst",
            "zomHive",
            "resetRotation",
        ]
        .iter()
        .any(|k| corpse.get(k).is_some_and(Value::truthy))
    {
        return err("unsupported special corpse");
    }
    if get(body, "m_BodyType")?.int() != Some(0)
        || flt(body, "m_LinearDamping")? != 0.0
        || far(body, "m_GravityScale", 0.7)?
        || get(body, "m_Constraints")?.int() != Some(4)
    {
        return err("unsupported corpse body");
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    let air = named_clip(&library, "Death Air")?.clone();
    if timing(&air)? != (12.0, 2, 6) {
        return err("unvalidated corpse clip timing");
    }
    // Half-unit box standing in for the missing collider.
    let bounds = [q16(-0.5 * sx), q16(-0.5 * sy), q16(0.5 * sx), q16(0.5 * sy)];
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&bounds)),
        ("spawn_offset".into(), jl(&[0, 0])),
        ("gravity".into(), ji(42 * 65536)),
        ("breaker".into(), Json::Bool(true)),
        ("smash_bounces".into(), ji(0)),
        ("remove_after_land".into(), ji(0)),
        ("fling_speed".into(), ji(15 * 65536)),
        ("bounce_factor".into(), ji(0)),
        ("land_delay_ticks".into(), ji(0)),
        ("limitations".into(), Json::List(vec![jstr("No source collider: removed on the first landing instead of falling out of the scene")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: pair(&air, &air),
        tiled: false,
    })
}

/// `Corpse Acid Fly`: a permanent physics prop holding Death Air for good.
fn acid_flyer_corpse(source: &Source, sc: &Scene, gid: i64) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    if launch_flags(death)
        || flt(death, "corpseFlingSpeed")? != 20.0
        || !get(death, "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.5, 0.0))
    {
        return err("unsupported corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if prefab.name() != "Corpse Acid Fly" {
        return err("unvalidated Acid Flyer corpse identity");
    }
    if [
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "Rigidbody2D",
        "CircleCollider2D",
        "ObjectBounce",
        "Transform",
        "PlayMakerFSM",
    ]
    .iter()
    .any(|k| !prefab.has(k))
        || ["Corpse", "BoxCollider2D"].iter().any(|k| prefab.has(k))
    {
        return err("unsupported Acid Flyer corpse prefab");
    }
    let fsm = get(prefab.part("PlayMakerFSM")?, "fsm")?;
    let shape: Vec<(String, usize, usize)> = get(fsm, "states")?
        .list()
        .unwrap_or(&[])
        .iter()
        .map(|st| {
            Ok((
                st.get("name").and_then(Value::str).unwrap_or_default(),
                get(st, "transitions")?.list().map_or(0, <[Value]>::len),
                get(get(st, "actionData")?, "actionNames")?
                    .list()
                    .map_or(0, <[Value]>::len),
            ))
        })
        .collect::<Result<_>>()?;
    if shape != [("State 1".to_string(), 0, 0)] {
        return err("unsupported Acid Flyer corpse FSM");
    }
    let (body, ob) = (prefab.part("Rigidbody2D")?, prefab.part("ObjectBounce")?);
    if get(body, "m_BodyType")?.int() != Some(0)
        || flt(body, "m_LinearDamping")? != 0.0
        || far(body, "m_GravityScale", 0.8)?
    {
        return err("unsupported corpse body");
    }
    if far(ob, "bounceFactor", 0.6)?
        || flt(ob, "speedThreshold")? != 1.0
        || ["playSound", "playAnimationOnBounce", "sendFSMEvent"]
            .iter()
            .any(|k| ob.get(k).is_some_and(Value::truthy))
    {
        return err("unsupported corpse bounce");
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let circle = prefab.part("CircleCollider2D")?;
    let (off, r) = (get(circle, "m_Offset")?, flt(circle, "m_Radius")?);
    if flag(circle, "m_IsTrigger")?
        || (r - 0.91).abs() > 1e-6
        || (flt(off, "x")? + 0.08).abs() > 1e-6
        || (flt(off, "y")? + 0.09).abs() > 1e-6
    {
        return err("unsupported Acid Flyer corpse collider");
    }
    let (ox, oy) = (flt(off, "x")?, flt(off, "y")?);
    let bounds = [(ox - r) * sx, (oy - r) * sy, (ox + r) * sx, (oy + r) * sy];
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    let air = named_clip(&library, "Death Air")?.clone();
    let air_index = get(&library, "clips")?
        .list()
        .unwrap_or(&[])
        .iter()
        .position(|c| c.py_eq(&air));
    if timing(&air)? != (30.0, 6, 1)
        || get(anim, "defaultClipId")?.int() != air_index.map(|i| i as i64)
        || !flag(anim, "playAutomatically")?
    {
        return err("unvalidated corpse clip timing");
    }
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&bounds.map(q16))),
        ("spawn_offset".into(), jl(&[0, q16(0.5)])),
        ("gravity".into(), ji(48 * 65536)),
        ("breaker".into(), Json::Bool(false)),
        ("smash_bounces".into(), ji(0)),
        ("remove_after_land".into(), ji(0)),
        ("fling_speed".into(), ji(20 * 65536)),
        ("bounce_factor".into(), ji(q16(0.6))),
        ("land_delay_ticks".into(), ji(0)),
        ("limitations".into(), Json::List(vec![jstr("Circle body as its bounding box with no spin; it stays where it comes to rest, as the source never removes it"), jstr("Corpse Steam is not presented")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: pair(&air, &air),
        tiled: false,
    })
}

/// `_corpse_gas_box`: a Shaker corpse's `Gas Hit Box` child, proved against the polygon `runner.rs`
/// carries. Its `damages_hero` FSM is the shape HeroController looks for on whatever it touches:
/// `damageDealt` and `hazardType` ints and no transitions of its own.
fn corpse_gas_box(source: &Source, prefab: &Prefab) -> Result<()> {
    type Child = (Value, Value, Vec<(String, Value)>);
    let mut found: Vec<Child> = Vec::new();
    for r in get(prefab.part("Transform")?, "m_Children")?
        .list()
        .unwrap_or(&[])
    {
        let transform = u(source.read(&u(source.deref(&prefab.obj.file, r))?))?;
        let go = u(source.read(&u(
            source.deref(&prefab.obj.file, get(&transform, "m_GameObject")?)
        )?))?;
        if get(&go, "m_Name")?.str().as_deref() != Some("Gas Hit Box") {
            continue;
        }
        let mut comps: Vec<(String, Value)> = Vec::new();
        for c in get(&go, "m_Component")?.list().unwrap_or(&[]) {
            let co = u(source.deref(&prefab.obj.file, get(c, "component")?))?;
            let kind = u(source.typename(&co))?;
            let tree = u(source.read(&co))?;
            match comps.iter_mut().find(|p| p.0 == kind) {
                Some(slot) => slot.1 = tree,
                None => comps.push((kind, tree)),
            }
        }
        found.push((go, transform, comps));
    }
    if found.len() != 1 {
        return err("Shaker corpse needs exactly one Gas Hit Box");
    }
    let (go, transform, comps) = &found[0];
    let comp = |k: &str| comps.iter().find(|p| p.0 == k).map(|p| &p.1);
    let polygon = comp("PolygonCollider2D");
    let fsm = comp("PlayMakerFSM").and_then(|f| f.get("fsm"));
    let (Some(polygon), Some(fsm)) = (polygon, fsm) else {
        return err("unsupported Shaker corpse Gas Hit Box");
    };
    if get(fsm, "name")?.str().as_deref() != Some("damages_hero")
        || !flag(polygon, "m_IsTrigger")?
        || flag(go, "m_IsActive")?
    {
        return err("unsupported Shaker corpse Gas Hit Box");
    }
    let mut ints: Vec<(String, &Value)> = Vec::new();
    for v in get(get(fsm, "variables")?, "intVariables")?
        .list()
        .unwrap_or(&[])
    {
        ints.push((get(v, "name")?.str().unwrap_or_default(), get(v, "value")?));
    }
    let int = |k: &str| ints.iter().rev().find(|p| p.0 == k).map(|p| p.1);
    let paths = get(get(polygon, "m_Points")?, "m_Paths")?
        .list()
        .unwrap_or(&[]);
    let oy = flt(get(polygon, "m_Offset")?, "y")?;
    let points: Option<Vec<[i64; 2]>> = if paths.len() == 1 {
        Some(
            paths[0]
                .list()
                .unwrap_or(&[])
                .iter()
                .map(|p| Ok([q16(flt(p, "x")?), q16(flt(p, "y")? + oy)]))
                .collect::<Result<_>>()?,
        )
    } else {
        None
    };
    let position = get(transform, "m_LocalPosition")?;
    let origin = [q16(flt(position, "x")?), q16(flt(position, "y")?)];
    if !int("damageDealt").is_some_and(|v| v.py_eq(&Value::Int(1)))
        || !int("hazardType").is_some_and(|v| v.py_eq(&Value::Int(1)))
        || points.as_deref() != Some(&crate::runner::GAS_POLYGON_Q16[..])
        || origin != [0, -65536]
    {
        return err("Shaker corpse Gas Hit Box is not the admitted polygon");
    }
    Ok(())
}

/// `Corpse Plant Trap`: no body. Its `corpse plant trap` FSM plays `Death` to completion where the
/// trap stood and then switches the renderer off, which is the hold form with one clip: held for the
/// clip's length, then removed.
fn plant_trap_corpse(
    source: &Source,
    sc: &Scene,
    gid: i64,
    library_source: &str,
) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    // rotateCorpse copies the owner's z rotation onto a corpse that never moves.
    if !flag(death, "m_Enabled")?
        || [
            "isCorpseRecyclable",
            "corpseFacesRight",
            "lowCorpseArc",
            "recycle",
        ]
        .iter()
        .any(|k| death.get(k).is_some_and(Value::truthy))
        || !get(death, "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.0, 0.0))
        || !get(death, "enemyDeathType")?.py_eq(&Value::Int(0))
        || get(death, "playerDataName")?.str().as_deref() != Some("SnapperTrap")
    {
        return err("unsupported Plant Trap corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if prefab.name() != "Corpse Plant Trap" {
        return err("unvalidated Plant Trap corpse identity");
    }
    let mut kinds: Vec<&str> = prefab.parts.iter().map(|p| p.0.as_str()).collect();
    kinds.sort();
    let mut want = vec![
        "Transform",
        "MeshFilter",
        "MeshRenderer",
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "PlayMakerFSM",
        "SpriteFlash",
    ];
    want.sort();
    if kinds != want {
        return err("unsupported Plant Trap corpse prefab");
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let fsm = get(prefab.part("PlayMakerFSM")?, "fsm")?;
    let states = get(fsm, "states")?.list().unwrap_or(&[]);
    let names: Vec<String> = states
        .iter()
        .map(|st| st.get("name").and_then(Value::str).unwrap_or_default())
        .collect();
    let mut transitions: Vec<(String, String)> = Vec::new();
    for st in states {
        for t in get(st, "transitions")?.list().unwrap_or(&[]) {
            transitions.push((
                get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(),
                get(t, "toState")?.str().unwrap_or_default(),
            ));
        }
    }
    if get(fsm, "name")?.str().as_deref() != Some("corpse plant trap")
        || get(fsm, "startState")?.str().as_deref() != Some("Retract")
        || names != ["Retract", "Death"]
        || transitions != [("FINISHED".to_string(), "Death".to_string())]
    {
        return err("unsupported Plant Trap corpse FSM");
    }
    check_action(
        "Plant Trap corpse",
        pins_state(fsm, "Retract")?,
        "Tk2dPlayAnimationWithEvents",
        &[
            ("clipName", Pin::S("Death")),
            ("animationTriggerEvent", Pin::S("")),
            ("animationCompleteEvent", Pin::S("FINISHED")),
        ],
    )?;
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    if library_o.sid() != library_source {
        return err("Plant Trap corpse animation library differs from the actor library");
    }
    let death_clip = named_clip(&library, "Death")?.clone();
    let (fps, wrap, frames) = timing(&death_clip)?;
    if (fps, wrap, frames, loop_start(&death_clip)) != (12.0, 2, 7, 0) || has_trigger(&death_clip) {
        return err("unvalidated Plant Trap corpse clip timing");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&[0, 0, 0, 0])),
        ("spawn_offset".into(), jl(&[0, 0])),
        ("gravity".into(), ji(0)),
        ("breaker".into(), Json::Bool(false)),
        ("smash_bounces".into(), ji(0)),
        ("fling_speed".into(), ji(0)),
        ("bounce_factor".into(), ji(0)),
        ("hold_ticks".into(), ji(ticks(frames as f64 / fps))),
        ("remove_after_land".into(), ji(1)),
        ("limitations".into(), Json::List(vec![jstr("No body: the corpse plays Death in place and is removed when it completes; the grass and orange puffs are not presented")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: pair(&death_clip, &death_clip),
        tiled: true,
    })
}

/// `Corpse Fat Fly`: a flung circle body (gravity 0.8, ObjectBounce 0.6, spun by SpinSelfSimple)
/// whose `Corpse` component sets no special flag, so it lands, plays Death Land and stays. The
/// circle stands in as its bounding box, as the Acid Flyer's does.
fn fat_fly_corpse(source: &Source, sc: &Scene, gid: i64) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    if launch_flags(death)
        || flt(death, "corpseFlingSpeed")? != 20.0
        || !get(death, "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.0, 0.0))
    {
        return err("unsupported corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if prefab.name() != "Corpse Fat Fly" {
        return err("unvalidated Fat Fly corpse identity");
    }
    if [
        "Corpse",
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "Rigidbody2D",
        "CircleCollider2D",
        "ObjectBounce",
        "Transform",
    ]
    .iter()
    .any(|k| !prefab.has(k))
        || prefab.has("BoxCollider2D")
        || prefab.has("PlayMakerFSM")
    {
        return err("unsupported Fat Fly corpse prefab");
    }
    let (corpse, body, ob) = (
        prefab.part("Corpse")?,
        prefab.part("Rigidbody2D")?,
        prefab.part("ObjectBounce")?,
    );
    if flag(corpse, "smashBounces")?
        || [
            "breaker",
            "bigBreaker",
            "chunker",
            "deathStun",
            "fungusExplode",
            "goopExplode",
            "hatcher",
            "instantChunker",
            "massless",
            "resetRotation",
            "spineBurst",
            "zomHive",
        ]
        .iter()
        .any(|k| corpse.get(k).is_some_and(Value::truthy))
        || get(get(corpse, "landEffects")?, "m_PathID")?.truthy()
    {
        return err("unsupported special corpse");
    }
    if get(body, "m_BodyType")?.int() != Some(0)
        || flt(body, "m_LinearDamping")? != 0.0
        || far(body, "m_GravityScale", 0.8)?
        || get(body, "m_Constraints")?.int() != Some(0)
    {
        return err("unsupported corpse body");
    }
    if far(ob, "bounceFactor", 0.6)?
        || flt(ob, "speedThreshold")? != 1.0
        || ["playSound", "playAnimationOnBounce", "sendFSMEvent"]
            .iter()
            .any(|k| ob.get(k).is_some_and(Value::truthy))
    {
        return err("unsupported corpse bounce");
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let circle = prefab.part("CircleCollider2D")?;
    let (off, r) = (get(circle, "m_Offset")?, flt(circle, "m_Radius")?);
    if flag(circle, "m_IsTrigger")?
        || (r - 0.6).abs() > 1e-6
        || (flt(off, "x")? - 0.02).abs() > 1e-6
        || (flt(off, "y")? + 0.01).abs() > 1e-6
    {
        return err("unsupported Fat Fly corpse collider");
    }
    let (ox, oy) = (flt(off, "x")?, flt(off, "y")?);
    let bounds = [(ox - r) * sx, (oy - r) * sy, (ox + r) * sx, (oy + r) * sy];
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    let selected = vec![
        named_clip(&library, "Death Air")?.clone(),
        named_clip(&library, "Death Land")?.clone(),
    ];
    let got: Vec<(f64, i64, usize)> = selected.iter().map(timing).collect::<Result<_>>()?;
    if got != [(12.0, 2, 2), (30.0, 0, 1)] {
        return err("unvalidated corpse clip timing");
    }
    if selected
        .iter()
        .any(|c| loop_start(c) != 0 || has_trigger(c))
    {
        return err("unsupported corpse animation events/loop start");
    }
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        (
            "scale".into(),
            Json::List(vec![Json::Float(sx), Json::Float(sy)]),
        ),
        ("bounds".into(), jl(&bounds.map(q16))),
        ("spawn_offset".into(), jl(&[0, 0])),
        ("gravity".into(), ji(48 * 65536)),
        ("breaker".into(), Json::Bool(false)),
        ("smash_bounces".into(), ji(0)),
        ("remove_after_land".into(), ji(0)),
        ("fling_speed".into(), ji(20 * 65536)),
        ("bounce_factor".into(), ji(q16(0.6))),
        ("land_delay_ticks".into(), ji(0)),
        (
            "limitations".into(),
            Json::List(vec![
                jstr("Circle body as its bounding box with no spin"),
                jstr("Corpse Flame, Steam and the spore clouds are not presented"),
            ]),
        ),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: selected,
        tiled: true,
    })
}

/// A compact FSM parameter that must be a literal (`scalar` in the Egg Sac validator).
fn literal(v: &Value) -> Result<Value> {
    if v.is_map() {
        if v.get("useVariable").is_some_and(Value::truthy) {
            return err("dynamic Egg Sac corpse parameter");
        }
        return v
            .get("value")
            .cloned()
            .ok_or_else(|| "no value".to_string());
    }
    Ok(v.clone())
}

/// `Corpse Egg Sac`: no Rigidbody2D, no collider and no Corpse component; the `Control` FSM is its lifetime.
fn egg_sac_corpse(
    source: &Source,
    sc: &Scene,
    gid: i64,
    library_source: &str,
) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let death = death_effects(&records)?;
    // rotateCorpse is set here, but only copies the owner's z rotation onto a corpse the guest draws upright.
    if !flag(death, "m_Enabled")?
        || [
            "isCorpseRecyclable",
            "corpseFacesRight",
            "lowCorpseArc",
            "recycle",
        ]
        .iter()
        .any(|k| death.get(k).is_some_and(Value::truthy))
        || flt(death, "corpseFlingSpeed")? != 0.0
        || !get(death, "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.0, 0.0))
        || get(death, "enemyDeathType")?.int() != Some(0)
        || get(death, "playerDataName")?.str().as_deref() != Some("EggSac")
    {
        return err("unsupported Egg Sac corpse launch");
    }
    let prefab = Prefab::read(source, sc, get(death, "corpsePrefab")?, true)?;
    if prefab.name() != "Corpse Egg Sac" {
        return err("unvalidated Egg Sac corpse identity");
    }
    let mut kinds: Vec<&str> = prefab.parts.iter().map(|p| p.0.as_str()).collect();
    kinds.sort();
    let mut want = vec![
        "Transform",
        "MeshFilter",
        "MeshRenderer",
        "tk2dSprite",
        "tk2dSpriteAnimator",
        "SetZ",
        "PlayMakerFSM",
        "AudioSource",
        "PreInstantiateGameObject",
    ];
    want.sort();
    if kinds != want {
        return err("unsupported Egg Sac corpse prefab");
    }
    let (sprite, transform) = (prefab.part("tk2dSprite")?, prefab.part("Transform")?);
    if !untinted(sprite, transform)? {
        return err("unsupported corpse scale/color");
    }
    let fsm = get(prefab.part("PlayMakerFSM")?, "fsm")?;
    let states = get(fsm, "states")?.list().unwrap_or(&[]);
    let names: Vec<String> = states
        .iter()
        .map(|s| s.get("name").and_then(Value::str).unwrap_or_default())
        .collect();
    let mut transitions: Vec<(String, String)> = Vec::new();
    for st in states {
        for t in get(st, "transitions")?.list().unwrap_or(&[]) {
            transitions.push((
                get(get(t, "fsmEvent")?, "name")?.str().unwrap_or_default(),
                get(t, "toState")?.str().unwrap_or_default(),
            ));
        }
    }
    let expected: Vec<(String, String)> = [
        ("FINISHED", "Spit"),
        ("FINISHED", "Burst"),
        ("FINISHED", "End"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    if get(fsm, "name")?.str().as_deref() != Some("Control")
        || get(fsm, "startState")?.str().as_deref() != Some("Init")
        || names != ["Init", "Spit", "Burst", "End"]
        || transitions != expected
    {
        return err("unsupported Egg Sac corpse FSM");
    }
    let state_of = |name: &str| -> Result<&Value> {
        states
            .iter()
            .rev()
            .find(|s| s.get("name").and_then(Value::str).as_deref() == Some(name))
            .ok_or_else(|| format!("no state {name}"))
    };
    let only = |state: &str, action: &str| -> Result<Fields> {
        let data = get(state_of(state)?, "actionData")?;
        let anames = get(data, "actionNames")?.list().unwrap_or(&[]);
        let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
        let found: Vec<usize> = anames
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                n.str()
                    .is_some_and(|s| s.rsplit('.').next() == Some(action))
                    && enabled.get(*i).is_some_and(Value::truthy)
            })
            .map(|(i, _)| i)
            .collect();
        if found.len() != 1 {
            return err(format!(
                "unsupported Egg Sac corpse action {state}/{action}"
            ));
        }
        u(action_fields(data, found[0], false))
    };
    let fv = |f: &Fields, k: &str| -> Result<Value> {
        field(f, k).cloned().ok_or_else(|| format!("missing {k}"))
    };
    let wait = only("Spit", "Wait")?;
    let hold = num(&literal(&fv(&wait, "time")?)?).ok_or("hold")?;
    let finish = literal(&fv(&wait, "finishEvent")?)?;
    if fv(&wait, "realTime")?.truthy()
        || finish.str().as_deref() != Some("FINISHED")
        || !(0.0 < hold && hold <= 10.0)
    {
        return err("unsupported Egg Sac corpse hold");
    }
    let burst = only("Burst", "Tk2dPlayAnimationWithEvents")?;
    if literal(&fv(&burst, "animationCompleteEvent")?)?
        .str()
        .as_deref()
        != Some("FINISHED")
        || literal(&fv(&burst, "animationTriggerEvent")?)?.truthy()
    {
        return err("unsupported Egg Sac corpse burst completion");
    }
    let names = [
        literal(&fv(&only("Spit", "Tk2dPlayAnimation")?, "clipName")?)?
            .str()
            .unwrap_or_default(),
        literal(&fv(&burst, "clipName")?)?.str().unwrap_or_default(),
    ];
    // The Shiny the burst releases is recorded, not presented.
    let trinket = only("Init", "SetFsmInt")?;
    if literal(&fv(&trinket, "fsmName")?)?.str().as_deref() != Some("Shiny Control")
        || literal(&fv(&trinket, "variableName")?)?.str().as_deref() != Some("Trinket Num")
    {
        return err("unsupported Egg Sac corpse drop");
    }
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    if library_o.sid() != library_source {
        return err("Egg Sac corpse animation library differs from the actor library");
    }
    let selected = vec![
        named_clip(&library, &names[0])?.clone(),
        named_clip(&library, &names[1])?.clone(),
    ];
    let got: Vec<(f64, i64, usize)> = selected.iter().map(timing).collect::<Result<_>>()?;
    if got != [(12.0, 1, 4), (18.0, 2, 4)] {
        return err("unvalidated Egg Sac corpse clip timing");
    }
    if loop_start(&selected[0]) != 1
        || loop_start(&selected[1]) != 0
        || selected.iter().any(has_trigger)
    {
        return err("unsupported Egg Sac corpse animation events/loop start");
    }
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let burst_seconds = get(&selected[1], "frames")?
        .list()
        .map_or(0, <[Value]>::len) as f64
        / flt(&selected[1], "fps")?;
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&[0, 0, 0, 0])),
        ("spawn_offset".into(), jl(&[0, 0])),
        ("gravity".into(), ji(0)),
        ("breaker".into(), Json::Bool(false)),
        ("smash_bounces".into(), ji(0)),
        ("fling_speed".into(), ji(0)),
        ("bounce_factor".into(), ji(0)),
        ("hold_ticks".into(), ji(ticks(hold))),
        ("remove_after_land".into(), ji(ticks(burst_seconds))),
        ("shiny_trinket_num".into(), jv(&literal(&fv(&trinket, "setValue")?)?)),
        ("limitations".into(), Json::List(vec![jstr("No rigid body or collider: the corpse holds the first clip in place and is removed when the second completes"), jstr("The Shiny it releases is recorded as its Trinket Num and never spawned; the EnemyKillShake, one shot, looping audio and particles are not presented")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: selected,
        tiled: false,
    })
}

/// `Corpse Zombie Guard`: its own FSM, no Corpse component; the hold form.
fn guard_corpse(
    source: &Source,
    sc: &Scene,
    gid: i64,
    library_source: &str,
) -> Result<CorpseSource> {
    let records = components(sc, gid)?;
    let deaths: Vec<&Value> = records
        .iter()
        .filter(|r| r.1 == "EnemyDeathEffects")
        .map(|r| r.2)
        .collect();
    if deaths.len() != 1
        || get(deaths[0], "playerDataName")?.str().as_deref() != Some("ZombieGuard")
        || !get(deaths[0], "corpseSpawnPoint")?.py_eq(&vec3(0.0, 0.0, 0.0))
    {
        return err("unsupported Husk Guard death");
    }
    let prefab = Prefab::read(source, sc, get(deaths[0], "corpsePrefab")?, false)?;
    if prefab.name() != "Corpse Zombie Guard" {
        return err("unvalidated Husk Guard corpse identity");
    }
    let fsm = get(prefab.part("PlayMakerFSM")?, "fsm")?;
    let stun = get(fsm, "states")?
        .list()
        .unwrap_or(&[])
        .iter()
        .rev()
        .find(|s| s.get("name").and_then(Value::str).as_deref() == Some("Death Stun"))
        .ok_or("no Death Stun state")?;
    let data = get(stun, "actionData")?;
    let anames = get(data, "actionNames")?.list().unwrap_or(&[]);
    let enabled = get(data, "actionEnabled")?.list().unwrap_or(&[]);
    let mut waits: Vec<Fields> = Vec::new();
    for (i, n) in anames.iter().enumerate() {
        if n.str().is_some_and(|s| s.ends_with(".Wait"))
            && enabled.get(i).is_some_and(Value::truthy)
        {
            waits.push(u(action_fields(data, i, false))?);
        }
    }
    let time = if waits.len() == 1 {
        field(&waits[0], "time").cloned()
    } else {
        None
    };
    let Some(time) = time.filter(|t| !t.get("useVariable").is_some_and(Value::truthy)) else {
        return err("unsupported Husk Guard corpse stun");
    };
    let anim = prefab.part("tk2dSpriteAnimator")?;
    let library_o = u(source.deref(&prefab.obj.file, get(anim, "library")?))?;
    let library = u(source.read(&library_o))?;
    if library_o.sid() != library_source {
        return err("Husk Guard corpse library differs from the actor library");
    }
    let default_clip = get(anim, "defaultClipId")?.int().unwrap_or(-1);
    let default_name = get(&library, "clips")?
        .list()
        .unwrap_or(&[])
        .get(default_clip as usize)
        .and_then(|c| c.get("name"))
        .and_then(Value::str);
    if default_name.as_deref() != Some("Death Stun") {
        return err("unsupported Husk Guard corpse start");
    }
    let selected = vec![
        named_clip(&library, "Death Stun")?.clone(),
        named_clip(&library, "Death Land")?.clone(),
    ];
    let (_, sx, sy) = actor_scale(sc, gid)?;
    let fields = vec![
        ("source".to_string(), Json::Str(prefab.obj.sid())),
        ("library".into(), Json::Str(library_o.sid())),
        ("scale".into(), Json::List(vec![Json::Float(sx), Json::Float(sy)])),
        ("bounds".into(), jl(&[0, 0, 0, 0])),
        ("spawn_offset".into(), jl(&[0, 0])),
        ("gravity".into(), ji(0)),
        ("breaker".into(), Json::Bool(false)),
        ("smash_bounces".into(), ji(0)),
        ("fling_speed".into(), ji(0)),
        ("bounce_factor".into(), ji(0)),
        ("hold_ticks".into(), ji(ticks(num(get(&time, "value")?).ok_or("hold")?))),
        ("remove_after_land".into(), ji(0)),
        ("limitations".into(), Json::List(vec![jstr("Hold form: Death Stun for the source wait, then Death Land held; the Death Air fall, the thrown club and the steam are not presented")])),
    ];
    Ok(CorpseSource {
        fields,
        library: library_o,
        clips: selected,
        tiled: true,
    })
}

/// A cooked sprite by (sprite, index, x scale, y scale).
type ImageKey = (String, i64, u64, u64);

/// `_ClipCooker`: append tk2d clips from any library into the region frame/clip tables.
pub struct ClipCooker {
    textures: HashMap<String, Arc<Image>>,
    collections: HashMap<String, Value>,
    images: HashMap<ImageKey, (usize, [f64; 4])>,
    cooked: HashMap<(String, String, u64, u64), usize>,
}

impl ClipCooker {
    pub fn new() -> ClipCooker {
        ClipCooker {
            textures: HashMap::new(),
            collections: HashMap::new(),
            images: HashMap::new(),
            cooked: HashMap::new(),
        }
    }

    /// `clip(library_sid, library_o, clip, sx, sy, tiled)`: the index of the cooked clip.
    #[allow(clippy::too_many_arguments)]
    pub fn clip(
        &mut self,
        source: &Source,
        bank: &mut ArtBank,
        library_sid: &str,
        library_o: &Obj,
        clip: &Value,
        sx: f64,
        sy: f64,
        tiled: bool,
        quantize: Quantizer,
    ) -> Result<usize> {
        let name = get(clip, "name")?.str().unwrap_or_default();
        let key = (
            library_sid.to_string(),
            name.clone(),
            sx.to_bits(),
            sy.to_bits(),
        );
        if !self.cooked.contains_key(&key) {
            let start = bank.frames.len();
            for frame in get(clip, "frames")?.list().unwrap_or(&[]) {
                let co = u(source.deref(&library_o.file, get(frame, "spriteCollection")?))?;
                let sid = co.sid();
                if !self.collections.contains_key(&sid) {
                    self.collections.insert(sid.clone(), u(source.read(&co))?);
                }
                let index = get(frame, "spriteId")?.int().unwrap_or(0);
                let image_key = (sid.clone(), index, sx.to_bits(), sy.to_bits());
                if !self.images.contains_key(&image_key) {
                    let (image, b) = tk_sprite(
                        source,
                        &co.file,
                        &self.collections[&sid],
                        index as usize,
                        &mut self.textures,
                    )?;
                    let b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                    let (w, h) = (
                        (b[2] - b[0]) * focal() / -CAM_Z,
                        (b[3] - b[1]) * focal() / -CAM_Z,
                    );
                    let texture = if tiled {
                        bank.atlas.add_tiled(&image, w, h, quantize)?
                    } else {
                        bank.atlas.add(&image, w, h, true, quantize)?
                    };
                    self.images.insert(image_key.clone(), (texture, b));
                }
                let (texture, b) = self.images[&image_key];
                bank.frames.push(Frame {
                    texture,
                    box_: b,
                    sprite: format!("{sid}:{index}"),
                    box_q16: None,
                    event: frame.clone(),
                });
            }
            self.cooked.insert(key.clone(), bank.clips.len());
            bank.clips.push(Clip {
                name: format!("{library_sid}/{name}"),
                start,
                count: get(clip, "frames")?.list().map_or(0, <[Value]>::len),
                fps: flt(clip, "fps")?,
                wrap: guest_wrap(clip)?,
                loop_start: loop_start(clip),
            });
        }
        Ok(self.cooked[&key])
    }
}

impl Default for ClipCooker {
    fn default() -> Self {
        ClipCooker::new()
    }
}

/// A `file:path_id` source id as the object it names.
fn object_of(source: &Source, sid: &str) -> Result<Obj> {
    let (file, id) = sid.rsplit_once(':').ok_or("malformed source id")?;
    let file = u(source.file(file))?;
    u(source.object(&file, id.parse().map_err(|_| "malformed source id")?))
}

fn ctl<'a>(c: &'a Json, k: &str) -> Option<&'a Json> {
    if let Json::Obj(f) = c {
        f.iter().find(|x| x.0 == k).map(|x| &x.1)
    } else {
        None
    }
}

/// The corpse records `append_corpse_art` leaves on the actors, by source id.
pub type Corpses = Vec<(String, Json)>;

/// `append_corpse_art(source, scene, actors, atlas, frames, clips)`.
pub fn append_corpse_art(
    source: &Source,
    sc: &Scene,
    actors: &mut [ArtActor],
    bank: &mut ArtBank,
    quantize: Quantizer,
) -> Result<Corpses> {
    let mut cooker = ClipCooker::new();
    let mut corpses: Corpses = Vec::new();
    for actor in actors.iter_mut() {
        if !actor.supported {
            continue;
        }
        let Some(record) = corpse_source(source, sc, actor)? else {
            continue;
        };
        let (sx, sy) = match record.fields.iter().find(|f| f.0 == "scale") {
            Some((_, Json::List(l))) => match (&l[0], &l[1]) {
                (Json::Float(a), Json::Float(b)) => (*a, *b),
                _ => return err("corpse scale"),
            },
            _ => return err("corpse scale"),
        };
        let library_sid = match record.fields.iter().find(|f| f.0 == "library") {
            Some((_, Json::Str(s))) => s.clone(),
            _ => return err("corpse library"),
        };
        let mut fields = record.fields.clone();
        for (kind, clip) in ["air", "land"].iter().zip(&record.clips) {
            let index = cooker.clip(
                source,
                bank,
                &library_sid,
                &record.library,
                clip,
                sx,
                sy,
                record.tiled,
                quantize,
            )?;
            fields.push((format!("{kind}_clip"), Json::Int(index as i64)));
        }
        corpses.push((actor.row.source.clone(), Json::Obj(fields)));
        // `actor['corpse']` is read back by the spec generator through `air_clip`/`land_clip`.
        let corpse = corpses.last().unwrap().1.clone();
        actor.corpse = Some(corpse);
    }
    append_shot_art(source, actors, bank, &mut cooker, quantize)?;
    append_guard_art(source, actors, bank, &mut cooker, quantize)?;
    Ok(corpses)
}

/// `append_shot_art`: pooled-projectile Idle/Impact clips at the prefab scale times the EnemyBullet scale.
fn append_shot_art(
    source: &Source,
    actors: &mut [ArtActor],
    bank: &mut ArtBank,
    cooker: &mut ClipCooker,
    quantize: Quantizer,
) -> Result<()> {
    for actor in actors.iter_mut() {
        let Some(control) = actor.control().cloned() else {
            continue;
        };
        let kind = match ctl(&control, "kind") {
            Some(Json::Str(k)) => k.clone(),
            _ => String::new(),
        };
        if !actor.supported || (kind != "Aspid" && kind != "Blocker" && kind != "FatFly") {
            continue;
        }
        let shot = ctl(&control, "shot").ok_or("shot control")?;
        let library_sid = match ctl(shot, "library") {
            Some(Json::Str(s)) => s.clone(),
            _ => return err("shot library"),
        };
        let library_o = object_of(source, &library_sid)?;
        let library = u(source.read(&library_o))?;
        let scale = match ctl(shot, "scale") {
            Some(Json::Float(f)) => *f,
            Some(Json::Int(i)) => *i as f64,
            _ => return err("shot scale"),
        };
        for (slot, name) in [("shot", "Idle"), ("impact", "Impact")] {
            let clip = named_clip(&library, name)?.clone();
            let index = cooker.clip(
                source,
                bank,
                &library_sid,
                &library_o,
                &clip,
                scale,
                scale,
                false,
                quantize,
            )?;
            actor.set_clip(&format!("{slot}_clip"), index as i64);
        }
    }
    Ok(())
}

/// `append_guard_art`: the Husk Guard's pooled `Shockwave Spurt` and `Slam Effect R` at unit scale.
fn append_guard_art(
    source: &Source,
    actors: &mut [ArtActor],
    bank: &mut ArtBank,
    cooker: &mut ClipCooker,
    quantize: Quantizer,
) -> Result<()> {
    for actor in actors.iter_mut() {
        let Some(control) = actor.control().cloned() else {
            continue;
        };
        if !actor.supported || ctl(&control, "kind") != Some(&Json::Str("HuskGuard".into())) {
            continue;
        }
        let Some(Json::Obj(extra)) = ctl(&control, "extra_art") else {
            return err("Husk Guard without extra art");
        };
        for (kind, reference) in extra {
            let (Some(Json::Str(file)), Some(Json::Int(path_id)), Some(Json::Str(clip_name))) = (
                ctl(reference, "file"),
                ctl(reference, "path_id"),
                ctl(reference, "clip"),
            ) else {
                return err("extra art reference");
            };
            let file = u(source.file(file))?;
            let library_o = u(source.object(&file, *path_id))?;
            let library = u(source.read(&library_o))?;
            let clip = named_clip(&library, clip_name)?.clone();
            let index = cooker.clip(
                source,
                bank,
                &library_o.sid(),
                &library_o,
                &clip,
                1.0,
                1.0,
                true,
                quantize,
            )?;
            actor.set_clip(&format!("{kind}_clip"), index as i64);
        }
    }
    Ok(())
}
