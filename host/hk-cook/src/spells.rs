//! Spell definitions from the Hero's `Spell Control` FSM and the spell prefabs (host/spells.py).
//!
//! Only Vengeful Spirit is bound; Desolate Dive and Howling Wraiths are catalogued with their gates.

use crate::breakables::{jf, jget, jl, k, ki, kl, ks};
use crate::common::{err, py_round, Result};
use crate::cook_audio::{jobj, js, u};
use crate::dream_nail::knight_animation;
use crate::focus::{assert_that, field, hero_fsm, FsmReader};
use crate::music::value_json;
use crate::pyjson::Json;
use hk_unity::{Obj, Source, Value};

/// `_reader(fsm).scalar`: a field's value, the variable's when it uses one (None-like when missing).
fn scalar(r: &FsmReader, value: &Value) -> Value {
    if matches!(value, Value::Map(_))
        && value.get("value").is_some()
        && value.get("useVariable").is_some()
    {
        if value.get("useVariable").is_some_and(Value::truthy) {
            let name = value.get("name").and_then(Value::str).unwrap_or_default();
            return r
                .variables
                .iter()
                .find(|e| e.0 == name)
                .map_or(Value::List(vec![]), |e| e.1.clone());
        }
        return value.get("value").cloned().unwrap();
    }
    value.clone()
}

/// `_named_child(source, file, state, name)`: the prefab a state's SpawnObjectFromGlobalPool points at.
fn named_child(
    source: &Source,
    file: &std::sync::Arc<hk_unity::serialized::SerializedFile>,
    state: &Value,
    name: &str,
) -> Result<Value> {
    for param in kl(k(state, "actionData")?, "fsmGameObjectParams")? {
        let path_id = ki(k(param, "value")?, "m_PathID")?;
        if path_id == 0 {
            continue;
        }
        let obj = u(source.object(file, path_id))?;
        let tree = u(source.read(&obj))?;
        if tree.get("m_Name").and_then(Value::str).as_deref() == Some(name) {
            return Ok(tree);
        }
    }
    err(format!(
        "no {name} prefab spawned by {}",
        ks(state, "name")?
    ))
}

fn fsm_on(
    source: &Source,
    file: &std::sync::Arc<hk_unity::serialized::SerializedFile>,
    prefab: &Value,
    name: &str,
) -> Result<Value> {
    for component in kl(prefab, "m_Component")? {
        let obj = u(source.deref(file, k(component, "component")?))?;
        if u(source.typename(&obj))? != "PlayMakerFSM" {
            continue;
        }
        let fsm = k(&u(source.read(&obj))?, "fsm")?.clone();
        if ks(&fsm, "name")? == name {
            return Ok(fsm);
        }
    }
    err(format!("{} carries no {name} FSM", ks(prefab, "m_Name")?))
}

fn collider(
    source: &Source,
    file: &std::sync::Arc<hk_unity::serialized::SerializedFile>,
    prefab: &Value,
) -> Result<Json> {
    for component in kl(prefab, "m_Component")? {
        let obj: Obj = u(source.deref(file, k(component, "component")?))?;
        // BoxCollider2D
        if obj.class_id() == 61 {
            let b = u(source.read(&obj))?;
            return Ok(jobj(vec![
                ("offset", value_json(k(&b, "m_Offset")?)),
                ("size", value_json(k(&b, "m_Size")?)),
            ]));
        }
    }
    err(format!(
        "{} carries no BoxCollider2D",
        ks(prefab, "m_Name")?
    ))
}

fn eq(a: &Value, b: &Value) -> bool {
    a.py_eq(b)
}

/// `source_spell_values(source)`.
pub fn source_spell_values(source: &Source) -> Result<Json> {
    let file = u(source.file("resources.assets"))?;
    let (_, control) = hero_fsm(source, "Spell Control")?;
    let r = FsmReader::new(&control)?;
    let var = |n: &str| -> Result<Value> {
        r.variables
            .iter()
            .find(|e| e.0 == n)
            .map(|e| e.1.clone())
            .ok_or_else(|| crate::breakables::pystr(n))
    };
    let tap = var("Button Down Time")?;
    let cost = var("MP Cost")?;
    let gate = r.actions("Can Cast?", "IntCompare")?;
    assert_that(
        gate.len() == 1 && eq(&scalar(&r, field(&gate[0], "integer2")?), &cost),
        "cast gate",
    )?;
    let mp = r.actions("Can Cast?", "GetPlayerDataInt")?;
    assert_that(
        scalar(&r, field(&mp[0], "intName")?).str().as_deref() == Some("MPCharge"),
        "MPCharge",
    )?;
    let level = r.actions("Has Fireball?", "GetPlayerDataInt")?;
    assert_that(
        level.len() == 1
            && scalar(&r, field(&level[0], "intName")?).str().as_deref() == Some("fireballLevel"),
        "fireballLevel",
    )?;
    for (state, clip) in [
        ("Fireball Antic", "Fireball Antic"),
        ("Fireball 1", "Fireball1 Cast"),
    ] {
        let played = r.actions(state, "Tk2dPlayAnimationWithEvents")?;
        assert_that(
            played.len() == 1
                && scalar(&r, field(&played[0], "clipName")?).str().as_deref() == Some(clip),
            "cast clip",
        )?;
    }
    let table = knight_animation(source)?;
    let mut clips: Vec<(String, f64)> = Vec::new();
    for c in kl(&table, "clips")? {
        let name = ks(c, "name")?;
        if name == "Fireball Antic" || name == "Fireball1 Cast" {
            let seconds = kl(c, "frames")?.len() as f64 / crate::breakables::kf(c, "fps")?;
            match clips.iter_mut().find(|e| e.0 == name) {
                Some(slot) => slot.1 = seconds,
                None => clips.push((name, seconds)),
            }
        }
    }
    assert_that(clips.len() == 2, "Fireball clips")?;
    let clip_of = |n: &str| clips.iter().find(|e| e.0 == n).unwrap().1;
    let states = |fsm: &Value| -> Result<Vec<(String, Value)>> {
        kl(fsm, "states")?
            .iter()
            .map(|s| Ok((ks(s, "name")?, s.clone())))
            .collect()
    };
    let control_states = states(&control)?;
    let caster = named_child(
        source,
        &file,
        &control_states
            .iter()
            .find(|e| e.0 == "Fireball 1")
            .ok_or("'Fireball 1'")?
            .1,
        "Fireball Top",
    )?;
    let cast = fsm_on(source, &file, &caster, "Fireball Cast")?;
    let cr = FsmReader::new(&cast)?;
    let speed = scalar(
        &cr,
        field(&cr.actions("Cast Right", "SetVelocityAsAngle")?[0], "speed")?,
    );
    let left = scalar(
        &cr,
        field(&cr.actions("Cast Left", "SetVelocityAsAngle")?[0], "speed")?,
    );
    assert_that(eq(&speed, &left), "the two directions share a speed")?;
    let recycle = scalar(&cr, field(&cr.actions("Wait", "Wait")?[0], "time")?);
    let cast_states = states(&cast)?;
    let ball = named_child(
        source,
        &file,
        &cast_states
            .iter()
            .find(|e| e.0 == "Cast Right")
            .ok_or("'Cast Right'")?
            .1,
        "Fireball",
    )?;
    let ball_control = fsm_on(source, &file, &ball, "Fireball Control")?;
    let br = FsmReader::new(&ball_control)?;
    let lifetime = scalar(&br, field(&br.actions("Idle", "Wait")?[0], "time")?);
    let damages = br.actions("Set Damage", "SetFsmInt")?;
    assert_that(damages.len() == 2, "Set Damage values")?;
    let damage = scalar(&br, field(&damages[0], "setValue")?);
    let walls = br.actions("Idle", "Collision2dEventLayer")?;
    assert_that(
        !walls.is_empty()
            && walls
                .iter()
                .all(|a| field(a, "sendEvent").is_ok_and(|e| e.str().as_deref() == Some("WALL"))),
        "wall stop",
    )?;
    Ok(jobj(vec![
        (
            "fireball",
            jobj(vec![
                ("tap_seconds", value_json(&tap)),
                ("antic_seconds", jf(clip_of("Fireball Antic"))),
                ("cast_seconds", jf(clip_of("Fireball1 Cast"))),
                ("cost", value_json(&cost)),
                ("speed", value_json(&speed)),
                ("lifetime", value_json(&lifetime)),
                ("recycle", value_json(&recycle)),
                ("damage", value_json(&damage)),
                ("collider", collider(source, &file, &ball)?),
                ("gate", js("fireballLevel > 0")),
            ]),
        ),
        (
            "catalogued_only",
            jobj(vec![
                ("quake", js("Desolate Dive, gated on quakeLevel; no admitted scene grants or needs it")),
                ("scream", js("Howling Wraiths, gated on screamLevel; likewise")),
            ]),
        ),
        (
            "limitations",
            jl(vec![
                js("Only Vengeful Spirit is implemented. Desolate Dive and Howling Wraiths are catalogued with their PlayerData gates and nothing more."),
                js("Charm variants (Shaman Stone damage, Flukenest, Defenders Crest, Spell Twister) are read but not applied, since charms do not exist yet."),
                js("The Fireball Antic and cast clips, the ball sprite and every impact effect are not cooked."),
            ]),
        ),
    ]))
}

fn jnum(j: &Json, key: &str) -> Result<f64> {
    match jget(j, key) {
        Some(Json::Float(f)) => Ok(*f),
        Some(Json::Int(i)) => Ok(*i as f64),
        _ => Err(format!("'{key}'")),
    }
}

/// `generated_spell_params(values)`.
pub fn generated_spell_params(values: &Json) -> Result<String> {
    let ticks = |seconds: f64| py_round(seconds * 60.0).max(1);
    let f = jget(values, "fireball").ok_or("'fireball'")?;
    let mut out = format!(
        "pub const FIREBALL_ANTIC_TICKS: u16 = {};\npub const FIREBALL_CAST_TICKS: u16 = {};\n",
        ticks(jnum(f, "antic_seconds")?),
        ticks(jnum(f, "cast_seconds")?)
    );
    let boxj = jget(f, "collider").ok_or("'collider'")?;
    let (off, size) = (
        jget(boxj, "offset").ok_or("'offset'")?,
        jget(boxj, "size").ok_or("'size'")?,
    );
    let (hw, hh) = (jnum(size, "x")? / 2.0, jnum(size, "y")? / 2.0);
    let bounds = [
        jnum(off, "x")? - hw,
        jnum(off, "y")? - hh,
        jnum(off, "x")? + hw,
        jnum(off, "y")? + hh,
    ];
    let cost = match jget(f, "cost") {
        Some(Json::Int(i)) => i.to_string(),
        Some(Json::Float(v)) => crate::pyfloat::repr(*v),
        _ => return err("'cost'"),
    };
    let damage = match jget(f, "damage") {
        Some(Json::Int(i)) => i.to_string(),
        Some(Json::Float(v)) => crate::pyfloat::repr(*v),
        _ => return err("'damage'"),
    };
    out += "pub const FIREBALL_PARAMS: hk_sim::FireballParams = hk_sim::FireballParams {";
    out += &[
        ("tap_ticks", ticks(jnum(f, "tap_seconds")?).to_string()),
        ("cost", cost),
        ("speed", py_round(jnum(f, "speed")? * 65536.0).to_string()),
        ("life_ticks", ticks(jnum(f, "lifetime")?).to_string()),
        ("damage", damage),
        (
            "bounds",
            format!(
                "[{}]",
                bounds
                    .iter()
                    .map(|v| py_round(v * 65536.0).to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
    ]
    .iter()
    .map(|(k, v)| format!("{k}:{v}"))
    .collect::<Vec<_>>()
    .join(",");
    out += "};\n";
    Ok(out)
}
