//! Source HealthManager actors and the recognizers that admit their movement
//! (host/actors.py `actor_sources`), ported one recognizer at a time.
//!
//! `scan` returns, for every enabled and active HealthManager of a scene, the
//! control record the ported recognizers give it. The recognizers are tried in
//! the order of `actor_sources`; an actor none of them admits has no control
//! here (the Python gives it one of the recognizers not ported yet, or a
//! refusal). The parity gate (hk-cook-parity actors) compares the controls of
//! the kinds ported against the Python oracle's.

use crate::aspid;
use crate::baldur;
use crate::climber;
use crate::false_knight;
use crate::common::{component_records, err, get, path_id, Result};
use crate::pyjson::Json;
use crate::runner;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};

/// One actor: where it is, and the control record a ported recognizer admitted.
pub struct Row {
    pub source: String,
    pub game_object: i64,
    pub name: String,
    pub control: Option<(String, Json)>,
}

/// `dict(control, guest_enabled=True)`: the key keeps its position.
fn guest_enabled(control: Json) -> Json {
    match control {
        Json::Obj(mut fields) => {
            if let Some(slot) = fields.iter_mut().find(|(k, _)| k == "guest_enabled") {
                slot.1 = Json::Bool(true);
            }
            Json::Obj(fields)
        }
        other => other,
    }
}

pub fn scan(sc: &Scene, source: &Source) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for o in &sc.objects {
        if o.typename != "HealthManager" || !get(&o.tree, "m_Enabled")?.truthy() {
            continue;
        }
        let gid = path_id(get(&o.tree, "m_GameObject")?).unwrap_or(0);
        if !sc.active(gid) {
            continue;
        }
        let name = get(sc.go(gid).ok_or_else(|| "no such GameObject".to_string())?, "m_Name")?.str().unwrap_or_default();
        let mut control = None;
        let records = component_records(sc, gid);
        if control.is_none() && name.starts_with("Roller") && records.iter().any(|r| r.1 == "LineOfSightDetector") {
            if let Ok(found) = baldur::recognize(sc, source, gid) {
                control = Some(("Baldur".to_string(), found));
            }
        }
        if control.is_none() && name.starts_with("Spitter") && records.iter().any(|r| r.1 == "PersonalObjectPool") {
            if let Ok(found) = aspid::recognize(sc, source, gid) {
                control = Some(("Aspid".to_string(), found));
            }
        }
        if control.is_none() && name.starts_with("False Knight") && records.iter().any(|r| r.1 == "EnemyHitEffectsArmoured") {
            if let Ok(found) = false_knight::recognize_placement(sc, source, gid, &o.tree) {
                control = Some(("FalseKnight".to_string(), found));
            }
        }
        if control.is_none() && records.iter().any(|r| r.1 == "Climber") {
            if let Ok(found) = climber::recognize(sc, source, gid) {
                control = Some(("Climber".to_string(), found));
            }
        }
        if control.is_none() && name.starts_with("Moss Walker") && records.iter().any(|r| r.1 == "NonBouncer") {
            if let Ok(found) = climber::recognize_moss_walker(sc, source, gid, &o.tree) {
                control = Some(("MossWalker".to_string(), found));
            }
        }
        // The Runner gate comes after the named gates in actor_sources; the
        // recognizers ported so far are disjoint from it by name and component.
        if control.is_none() {
            if let Some(found) = runner::candidate(sc, source, o)? {
                control = Some(("ZombieSwipeWalker".to_string(), guest_enabled(found.control)));
            }
        }
        rows.push(Row { source: sc.sid(o.id), game_object: gid, name, control });
    }
    Ok(rows)
}

/// Value helper for the recognizers: a clone of a field, or the missing-field error.
pub fn field(v: &Value, key: &str) -> Result<Value> {
    match v.get(key) {
        Some(x) => Ok(x.clone()),
        None => err(format!("missing field {key}")),
    }
}
