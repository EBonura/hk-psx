//! Admit the placed Brooding Mawlek (Crossroads_09) into the guest actor pool.
//! Ported from host/mawlek.py `recognize_placement`.
//!
//! The boss controller lives in shared/hk-sim/src/mawlek.rs; this is the actor
//! pool half: the object's shape, its FSM set, and the wake box the guest
//! watches. Every clip is `Dummy Blank` there. A state or child that is
//! missing, or a HealthManager variant the guest does not model, refuses the
//! cook rather than seating a half-understood boss.

use crate::common::{component_records, err, get, py_round, Result};
use crate::cook_audio::{jobj, js, sha, u};
use crate::gruzzer::fsm_state_signature;
use crate::pyjson::Json;
use crate::recog::children_of;
use hk_unity::scene::Scene;
use hk_unity::Value;

const BODY_NAME: &str = "Mawlek Body";
const CHILDREN: [&str; 6] = ["Dummy", "Mawlek Arm R", "Mawlek Arm L", "Mawlek Head", "Spit Effect", "Alert Range New"];

/// `_box_world`: a child's one BoxCollider2D as a world box [x0, y0, x1, y1].
fn box_world(sc: &Scene, gid: i64) -> Result<[f64; 4]> {
    let records = component_records(sc, gid);
    let name = get(sc.go(gid).ok_or("no such GameObject")?, "m_Name")?.str().unwrap_or_default();
    let boxes: Vec<&Value> = records.iter().filter(|r| r.1 == "BoxCollider2D").map(|r| r.2).collect();
    if boxes.len() != 1 {
        return err(format!("expected one BoxCollider2D on {name}"));
    }
    let b = boxes[0];
    let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("object has no transform")?))?;
    if m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 {
        return err(format!("{name} is rotated"));
    }
    let (size, offset) = (crate::recog::xy(b, "m_Size")?, crate::recog::xy(b, "m_Offset")?);
    let cx = m[0][3] + offset[0] * m[0][0];
    let cy = m[1][3] + offset[1] * m[1][1];
    let (hw, hh) = ((size[0] * m[0][0]).abs() / 2.0, (size[1] * m[1][1]).abs() / 2.0);
    Ok([cx - hw, cy - hh, cx + hw, cy + hh])
}

/// Admit the placed Brooding Mawlek, or refuse with the reason.
pub fn recognize_placement(sc: &Scene, gid: i64, health: &Value) -> Result<Json> {
    let go = sc.go(gid).ok_or("no such GameObject")?;
    if get(go, "m_Name")?.str().as_deref() != Some(BODY_NAME) || get(go, "m_Layer")?.int() != Some(11) {
        return err("not the Mawlek body on the enemy layer");
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
    let names: Vec<&str> = fsms.iter().map(|f| f.0.as_str()).collect();
    if names != ["Mawlek Control"] || !fsms[0].1.get("m_Enabled").is_some_and(Value::truthy) {
        let mut sorted = names.clone();
        sorted.sort();
        return err(format!("unsupported Mawlek FSM set: {}", sorted.join(", ")));
    }
    let control = get(fsms[0].1, "fsm")?;
    let states: Vec<String> = get(control, "states")?.list().unwrap_or(&[]).iter().filter_map(|s| s.get("name").and_then(Value::str)).collect();
    for needed in ["Dormant", "Wake", "Start", "Idle", "Super Select", "Shoot", "Jump", "Land 2", "Music"] {
        if !states.iter().any(|s| s == needed) {
            return err(format!("Mawlek Control lacks {needed}"));
        }
    }
    let children = children_of(sc, gid)?;
    let missing: Vec<&str> = CHILDREN.iter().copied().filter(|c| !children.iter().any(|h| h.0 == *c)).collect();
    if !missing.is_empty() {
        return err(format!("Mawlek lacks {}", missing.join(", ")));
    }
    // `Start` clears the serialized invincibility; the guest runtime owns that.
    if get(health, "hasSpecialDeath")?.truthy() || get(health, "damageOverride")?.truthy() || get(health, "invincibleFromDirection")?.truthy() {
        return err("unsupported Mawlek HealthManager variant");
    }
    let m = u(sc.world(*sc.go_transform.get(&gid).ok_or("actor has no transform")?))?;
    if m[0][1].abs() > 1e-6 || m[1][0].abs() > 1e-6 {
        return err("Mawlek is rotated");
    }
    let p = u(sc.point(gid, 0.0, 0.0, 0.0))?;
    let alert = children.iter().find(|c| c.0 == "Alert Range New").unwrap().1;
    let wake = box_world(sc, alert)?;
    let q = |a: f64, o: f64| Json::Int(py_round((a - o) * 65536.0));
    Ok(jobj(vec![
        ("kind", js("Mawlek")),
        ("guest_enabled", Json::Bool(true)),
        ("art_bindings", jobj(vec![("walk", js("Dummy Blank")), ("turn", js("Dummy Blank"))])),
        // `Mawlek Control` starts `Start` with SetInvincible false; the body is
        // serialized invincible so nothing hurts it while it lurks.
        ("no_corpse", Json::Bool(true)),
        ("wake_q16", Json::List(vec![q(wake[0], p[0]), q(wake[1], p[1]), q(wake[2], p[0]), q(wake[3], p[1])])),
        ("initial_direction", Json::Int(-1)),
        ("fsm_sha256", Json::Str(sha(fsm_state_signature(control)?.as_bytes()))),
        (
            "limitations",
            Json::List(
                [
                    "Every clip, the arms, the head, the shots and the corpse are cooked by host/mawlek_art.py into this scene alone; the ActorSpec clip fields point at Dummy Blank.",
                    "The wake roar's Roar Lock on the hero, the particles and the blood are not reproduced.",
                ]
                .iter()
                .map(|s| js(s))
                .collect(),
            ),
        ),
    ]))
}
