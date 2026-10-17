//! The enemies whose death the source keeps, as `data/actor_persistence.rs`.
//!
//! `HealthManager` hooks its object's `PersistentBoolItem`: a death writes the
//! item (`EnemyDeathEffects.RecieveDeathEvent` calls its `SaveState`), and a
//! scene that loads with the item set answers by marking the enemy dead and
//! deactivating it. The item's identity is its `persistentBoolData.id` and its
//! scene, and an empty id is replaced by the owner's name when the item starts,
//! so every placement of one name in a scene shares one state. `dontSave` items
//! never write, and `semiPersistent` ones are reset by a bench rest. All of that
//! is read off the installed `Assembly-CSharp.dll`'s IL (HealthManager.Awake,
//! `<Awake>b__69_1`, EnemyDeathEffects.RecieveDeathEvent, PersistentBoolItem
//! Start/SaveState/ResetState, GameManager.ResetSemiPersistentItems); none of it
//! is copied.
//!
//! Which placements: the ones the cook admits (`actors::scan`'s supported rows)
//! whose HealthManager object carries an enabled-or-not `PersistentBoolItem`
//! that saves. Families that keep a death of their own are left out: a boss
//! (the arena's `Activated`, `game/src/battle_gates.rs`) and the Blocker (its
//! terrain block, `game/src/blocker_terrain.rs`). An object merged in from an
//! additive scene (source id 100000 and up) is read from another file than the
//! one scanned here, and only the False Knight is admitted that way.
//!
//! The table is sorted by (scene, source id), one row per placement; the third
//! word is the state group (the order its first placement was met in) with
//! `hk_sim::SEMI_PERSISTENT` set when a bench rest resets it.

use crate::actors;
use crate::common::{component_records, err, Result};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use serde_json::Value as J;
use std::collections::HashMap;
use std::path::Path;

/// Families that keep a death of their own.
const OWN_PERSISTENCE: &[&str] = &["Blocker", "Mawlek", "FalseKnight", "GruzMother"];
/// The first source id of an object merged in from an additive scene.
const ADDITIVE_BASE: i64 = 100_000;
const LOCAL_BASE: usize = 16;
const MAX_LOCAL: usize = 1 << 10;

// The original persistent state identity is its serialized id, or the owner
// name when that id is explicitly empty. Admission order must not change a save.
fn state_local(state: &str) -> usize {
    let hash = state.bytes().fold(0x811c9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
    LOCAL_BASE + hash as usize % (MAX_LOCAL - LOCAL_BASE)
}
fn reserve_local(locals: &mut HashMap<(usize, usize), String>, scene: usize, state: &str) -> Result<()> {
    let key = (scene, state_local(state));
    if let Some(other) = locals.get(&key) {
        if other != state { return err(format!("persistent state key collision: {other} and {state}")); }
    } else {
        locals.insert(key, state.to_owned());
    }
    Ok(())
}

/// One persistent placement of a scene.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub scene: usize,
    pub source_id: i64,
    /// Index of its state among the table's groups.
    pub group: usize,
    pub name: String,
    pub semi: bool,
    pub local: usize,
}

fn flag(tree: &Value, key: &str) -> Result<bool> {
    match tree.get(key) {
        Some(Value::Bool(v)) => Ok(*v),
        Some(Value::Int(0) | Value::UInt(0)) => Ok(false),
        Some(Value::Int(1) | Value::UInt(1)) => Ok(true),
        _ => err(format!("PersistentBoolItem missing or untyped flag {key}")),
    }
}

/// The persistent placements of one scene, groups numbered from `first_group`.
/// `groups` maps (scene, state id) to the group, extended as new states are met.
pub fn scene_rows(
    sc: &Scene,
    source: &Source,
    catalogue: &actors::Catalogue,
    scene: usize,
    groups: &mut HashMap<(usize, String), (usize, bool)>,
) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    let actors = if scene == 21 {
        actors::scan_crossroads22(sc, source, catalogue)?
    } else { actors::scan(sc, source, catalogue)? };
    for actor in actors {
        if !actor.supported || actor.spec_source_id >= ADDITIVE_BASE {
            continue;
        }
        u32::try_from(actor.spec_source_id).map_err(|_| "actor source id does not fit the runtime key")?;
        if actor.control.as_ref().is_some_and(|c| OWN_PERSISTENCE.contains(&c.0.as_str())) {
            continue;
        }
        let records = component_records(sc, actor.game_object);
        let Some(item) = records.iter().find(|r| r.1 == "PersistentBoolItem") else { continue };
        let tree = item.2;
        if flag(tree, "dontSave")? {
            continue;
        }
        let semi = flag(tree, "semiPersistent")?;
        // `persistentBoolData.id` is empty in the scene files and becomes the
        // owner's name; an authored one would be used as it is.
        let authored = tree.get("persistentBoolData").and_then(|d| d.get("id")).and_then(Value::str)
            .ok_or("PersistentBoolItem missing its serialized state id")?;
        let state = if authored.is_empty() { actor.name.clone() } else { authored };
        let next = groups.len();
        let (group, was_semi) = *groups.entry((scene, state.clone())).or_insert((next, semi));
        if was_semi != semi {
            return err(format!("{}: placements named {state} disagree about semiPersistent", sc.base.name));
        }
        rows.push(Row { scene, source_id: actor.spec_source_id, group, name: actor.name.clone(), semi, local: state_local(&state) });
    }
    Ok(rows)
}

/// The generated Rust source for a sorted set of rows.
pub fn render(rows: &[Row], groups: usize) -> String {
    let mut out = String::new();
    out.push_str("// Generated by hk-cook actor-persistence. Do not edit.\n");
    out.push_str("// Enemies whose source `PersistentBoolItem` keeps their death: (catalogue scene, source id,\n");
    out.push_str("// stable local id | SEMI_PERSISTENT when a bench rest resets it). Sorted by scene, then source id.\n");
    out.push_str(&format!("pub const GROUPS: usize = {groups};\n"));
    out.push_str("pub const PERSISTENT_ACTORS: &[hk_sim::PersistentActor] = &[\n");
    for r in rows {
        let word = r.local as u16 | if r.semi { 0x8000 } else { 0 };
        out.push_str(&format!("    ({}, {}, {:#06x}), // {}\n", r.scene, r.source_id, word, r.name));
    }
    out.push_str("];\n");
    out
}

pub fn main(root: &Path, source_dir: Option<&Path>) -> Result<()> {
    let source = match source_dir {
        Some(d) => Source::new(d),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let text = std::fs::read(root.join("data/regions.json")).map_err(|e| format!("data/regions.json: {e}"))?;
    let report: J = serde_json::from_slice(&text).map_err(|e| format!("data/regions.json: {e}"))?;
    let scenes = report["scenes"].as_array().ok_or("report without scenes")?;
    let catalogue: Vec<(String, [f64; 4], String)> = scenes
        .iter()
        .map(|s| {
            let b: Vec<f64> = s["runtime_bounds"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
            if b.len() != 4 {
                return err("scene without runtime_bounds");
            }
            Ok((s["file"].as_str().unwrap_or("").to_string(), [b[0], b[1], b[2], b[3]], s["scene_name"].as_str().unwrap_or("").to_string()))
        })
        .collect::<Result<_>>()?;
    let mut groups = HashMap::new();
    let mut rows = Vec::new();
    let mut locals = HashMap::new();
    for (index, s) in scenes.iter().enumerate() {
        let scene = s["scene_id"].as_u64().unwrap_or(index as u64) as usize;
        let sc = Scene::new(&source, s["file"].as_str().unwrap_or("")).map_err(|e| e.to_string())?;
        rows.extend(scene_rows(&sc, &source, &catalogue, scene, &mut groups)?);
    }
    for ((scene, state), _) in &groups {
        if *scene >= 1 << 10 { return err("persistent scene does not fit HKS5"); }
        reserve_local(&mut locals, *scene, state)?;
    }
    rows.sort_by_key(|r| (r.scene, r.source_id));
    if groups.len() >= 0x8000 {
        return err("more persistent states than a group word holds");
    }
    let path = root.join("data/actor_persistence.rs");
    std::fs::write(&path, render(&rows, groups.len())).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("actor persistence: {} placements in {} states, {} semi-persistent", rows.len(), groups.len(), rows.iter().filter(|r| r.semi).count());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_non_boolean_flags_cannot_invent_a_persistent_death() {
        assert!(flag(&Value::Map(vec![]), "dontSave").is_err());
        for value in [Value::Str(b"false".to_vec()), Value::Int(2), Value::F32(0.0)] {
            let tree = Value::Map(vec![("dontSave".into(), value)]);
            assert!(flag(&tree, "dontSave").is_err());
        }
        let tree = Value::Map(vec![("dontSave".into(), Value::Bool(false)), ("semiPersistent".into(), Value::Int(1))]);
        assert_eq!(flag(&tree, "dontSave").unwrap(), false);
        assert_eq!(flag(&tree, "semiPersistent").unwrap(), true);
    }

    #[test]
    fn rows_render_as_the_sorted_table_the_guest_searches() {
        let rows = vec![
            Row { scene: 2, source_id: 125050, group: 0, name: "Zombie Runner 1".into(), semi: false, local: state_local("Zombie Runner 1") },
            Row { scene: 19, source_id: 77, group: 1, name: "Hatcher".into(), semi: true, local: state_local("Hatcher") },
        ];
        let text = render(&rows, 2);
        assert!(text.contains("pub const GROUPS: usize = 2;"));
        assert!(text.contains(&format!("(2, 125050, {:#06x}), // Zombie Runner 1", state_local("Zombie Runner 1"))));
        assert!(text.contains(&format!("(19, 77, {:#06x}), // Hatcher", state_local("Hatcher") | 0x8000)));
    }

    #[test]
    fn permanent_state_identity_is_stable_and_cannot_enter_blocker_namespace() {
        let key = state_local("Egg Sac");
        assert!((LOCAL_BASE..MAX_LOCAL).contains(&key));
        assert_eq!(state_local("Egg Sac"), key);
        assert_ne!(state_local("Egg Sac"), state_local("Egg Sac 1"));
    }
    #[test]
    fn colliding_different_source_identities_fail_but_aliases_and_other_scenes_are_valid() {
        let mut seen = HashMap::new();
        let mut pair = None;
        for n in 0..=MAX_LOCAL {
            let state = format!("fixture-state-{n}");
            if let Some(first) = seen.insert(state_local(&state), state.clone()) {
                pair = Some((first, state)); break;
            }
        }
        let (a,b) = pair.expect("more identities than available locals must collide");
        let mut locals = HashMap::new();
        reserve_local(&mut locals, 3, &a).unwrap();
        reserve_local(&mut locals, 3, &a).unwrap();
        assert!(reserve_local(&mut locals, 3, &b).is_err());
        reserve_local(&mut locals, 4, &b).unwrap();
    }
}
