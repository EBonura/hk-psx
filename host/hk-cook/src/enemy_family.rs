//! The table-driven recognizer for enemies that run their original PlayMaker trees on the compact actor tier
//! (host/enemy_families.json). Ported from host/enemy_family.py; the parity gate compares the two control records.
//!
//! A row names the placements (name prefixes and components), the FSMs they carry with the digest of each, the child
//! object whose trigger box the tree listens on, the clips the guest binds and the clip the object rests on. The
//! cooker for the banks is host/hk-fsm `enemy_family.rs`.

use crate::common::{component_records, err, get, kid, kids, py_round, Result};
use crate::cook_audio::{js, u};
use crate::false_knight::fsm_digest;
use crate::pyjson::Json;
use crate::recog::xy;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

const MANIFEST: &str = include_str!("../../enemy_families.json");
const BASIS_TOLERANCE: f64 = 1e-5;

/// One FSM a row pins.
struct FsmRow {
    on: String,
    name: String,
    digest: String,
}

/// One manifest row.
struct Row {
    name: String,
    id: i64,
    prefixes: Vec<String>,
    components: Vec<String>,
    fsms: Vec<FsmRow>,
    trigger_child: String,
    rest: String,
    clips: Vec<String>,
    limitations: Vec<String>,
}

fn rows() -> Result<Vec<Row>> {
    let json: serde_json::Value = serde_json::from_str(MANIFEST).map_err(|e| e.to_string())?;
    let strings = |row: &serde_json::Value, key: &str| -> Result<Vec<String>> {
        row[key]
            .as_array()
            .ok_or(format!("manifest row without {key}"))?
            .iter()
            .map(|s| s.as_str().map(str::to_owned).ok_or_else(|| "manifest string".to_string()))
            .collect()
    };
    let mut out = Vec::new();
    for row in json["families"].as_array().ok_or("families")? {
        let mut fsms = Vec::new();
        for f in row["fsms"].as_array().ok_or("fsms")? {
            fsms.push(FsmRow {
                on: f["on"].as_str().ok_or("fsm.on")?.to_owned(),
                name: f["name"].as_str().ok_or("fsm.name")?.to_owned(),
                digest: f["digest"].as_str().ok_or("fsm.digest")?.to_owned(),
            });
        }
        out.push(Row {
            name: row["name"].as_str().ok_or("name")?.to_owned(),
            id: row["id"].as_i64().ok_or("id")?,
            prefixes: strings(row, "name_prefixes")?,
            components: strings(row, "components")?,
            fsms,
            trigger_child: row["trigger_child"].as_str().ok_or("trigger_child")?.to_owned(),
            rest: row["rest"].as_str().ok_or("rest")?.to_owned(),
            clips: strings(row, "clips")?,
            limitations: strings(row, "limitations")?,
        });
    }
    Ok(out)
}

/// Whether a manifest row names a placement called `name`.
pub fn named(name: &str) -> Result<bool> {
    Ok(rows()?.iter().any(|r| r.prefixes.iter().any(|p| name.starts_with(p.as_str()))))
}

/// The control record of a placement of a manifest family, or an error naming what differs.
pub fn recognize(sc: &Scene, source: &Source, gid: i64, name: &str) -> Result<Json> {
    let all = rows()?;
    let row = all
        .iter()
        .find(|r| r.prefixes.iter().any(|p| name.starts_with(p.as_str())))
        .ok_or_else(|| format!("no enemy family row names {name}"))?;
    let records = component_records(sc, gid);
    if row.components.iter().any(|c| !records.iter().any(|r| r.1 == c)) {
        return err(format!("{}: a required component is missing", row.name));
    }
    let children = kids(sc, gid)?;
    let mut found: Vec<(String, String, &Value, bool)> = Vec::new();
    for r in records.iter().filter(|r| r.1 == "PlayMakerFSM") {
        if let Some(fsm) = r.2.get("fsm") {
            let n = fsm.get("name").and_then(Value::str).unwrap_or_default();
            found.push((String::new(), n, fsm, get(r.2, "m_Enabled")?.truthy()));
        }
    }
    for want in &row.fsms {
        if !want.on.is_empty() && kid(&children, &want.on).is_none() {
            return err(format!("{}: missing child {}", row.name, want.on));
        }
    }
    for (on, k) in &children {
        for r in component_records(sc, *k).into_iter().filter(|r| r.1 == "PlayMakerFSM") {
            if let Some(fsm) = r.2.get("fsm") {
                let n = fsm.get("name").and_then(Value::str).unwrap_or_default();
                found.push((on.clone(), n, fsm, get(r.2, "m_Enabled")?.truthy()));
            }
        }
    }
    let mut keys: Vec<(&str, &str)> = found.iter().map(|f| (f.0.as_str(), f.1.as_str())).collect();
    keys.sort();
    keys.dedup();
    let mut wanted: Vec<(&str, &str)> = row.fsms.iter().map(|f| (f.on.as_str(), f.name.as_str())).collect();
    wanted.sort();
    if keys != wanted {
        return err(format!("{}: unexpected PlayMaker FSM set", row.name));
    }
    for want in &row.fsms {
        let Some(hit) = found.iter().rev().find(|f| f.0 == want.on && f.1 == want.name) else {
            return err(format!("{}: FSM {} missing", row.name, want.name));
        };
        if !hit.3 || fsm_digest(hit.2, &[])? != want.digest {
            return err(format!("{}: FSM {} differs from the pinned original", row.name, want.name));
        }
    }
    let trigger = kid(&children, &row.trigger_child).ok_or("trigger child")?;
    let ttid = *sc.go_transform.get(&trigger).ok_or("trigger transform")?;
    let local = sc.transform(ttid).ok_or("transform missing")?;
    let zero = |v: &Value, keys: &[&str], want: f64| -> bool {
        keys.iter().all(|k| v.get(k).and_then(Value::float) == Some(want))
    };
    if !sc.active(trigger)
        || !zero(get(local, "m_LocalPosition")?, &["x", "y", "z"], 0.0)
        || !zero(get(local, "m_LocalScale")?, &["x", "y", "z"], 1.0)
        || !(zero(get(local, "m_LocalRotation")?, &["x", "y", "z"], 0.0)
            && get(local, "m_LocalRotation")?.get("w").and_then(Value::float) == Some(1.0))
    {
        return err(format!("{}: the {} child is not seated on the actor", row.name, row.trigger_child));
    }
    let trigger_records = component_records(sc, trigger);
    let boxes: Vec<&Value> = trigger_records.iter().filter(|r| r.1 == "BoxCollider2D").map(|r| r.2).collect();
    if boxes.len() != 1 || !get(boxes[0], "m_Enabled")?.truthy() || !get(boxes[0], "m_IsTrigger")?.truthy() {
        return err(format!("{}: unsupported {} trigger", row.name, row.trigger_child));
    }
    let offset = xy(boxes[0], "m_Offset")?;
    let size = xy(boxes[0], "m_Size")?;
    let trigger_q16 = [
        py_round((offset[0] - size[0] / 2.0) * 65536.0),
        py_round((offset[1] - size[1] / 2.0) * 65536.0),
        py_round((offset[0] + size[0] / 2.0) * 65536.0),
        py_round((offset[1] + size[1] / 2.0) * 65536.0),
    ];
    // The quarter turn of the world basis: a proper rotation of unit scale.
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let m = u(sc.world(tid))?;
    let quarter = py_round(m[1][0].atan2(m[0][0]).to_degrees() / 90.0).rem_euclid(4);
    let (cos, sin) = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)][quarter as usize];
    let basis = [[cos, -sin], [sin, cos]];
    if (0..2).any(|i| (0..2).any(|j| (m[i][j] - basis[i][j]).abs() > BASIS_TOLERANCE)) {
        return err("unsupported enemy family rotation or scale");
    }
    let animator = records.iter().rev().find(|r| r.1 == "tk2dSpriteAnimator").map(|r| r.2);
    let sprite = records.iter().rev().find(|r| r.1 == "tk2dSprite").map(|r| r.2);
    let (Some(animator), Some(sprite)) = (animator, sprite) else {
        return err(format!("{}: unsupported sprite scale", row.name));
    };
    let scale = get(sprite, "_scale")?;
    if !zero(scale, &["x", "y", "z"], 1.0) {
        return err(format!("{}: unsupported sprite scale", row.name));
    }
    let library = u(source.read(&u(sc.deref(get(animator, "library")?))?))?;
    let clips = get(&library, "clips")?.list().unwrap_or(&[]);
    let by_name = |n: &str| clips.iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(n));
    if row.clips.iter().any(|c| by_name(c).is_none()) {
        return err(format!("{}: animation library lacks a clip", row.name));
    }
    let rest = by_name(&row.rest).ok_or("rest clip")?;
    let last = get(rest, "frames")?.list().unwrap_or(&[]).last().and_then(|f| f.get("spriteId")).and_then(Value::int);
    if last != get(sprite, "_spriteId")?.int() {
        return err(format!("{}: the authored sprite is not the last frame of {}", row.name, row.rest));
    }
    let mut bindings = vec![("walk".to_string(), js(&row.rest)), ("turn".to_string(), js(&row.rest))];
    for (i, c) in row.clips.iter().enumerate() {
        bindings.push((format!("f{i}"), js(c)));
    }
    Ok(Json::Obj(vec![
        ("kind".into(), js("FsmActor")),
        ("guest_enabled".into(), Json::Bool(true)),
        ("family".into(), js(&row.name)),
        ("family_id".into(), Json::Int(row.id)),
        ("rotation_q16".into(), Json::Int(quarter * 90 * 65536)),
        ("trigger_q16".into(), Json::List(trigger_q16.iter().map(|&v| Json::Int(v)).collect())),
        ("no_corpse".into(), Json::Bool(true)),
        ("art_axes".into(), js("basis")),
        ("art_bindings".into(), Json::Obj(bindings)),
        ("limitations".into(), Json::List(row.limitations.iter().map(|s| js(s)).collect())),
    ]))
}
