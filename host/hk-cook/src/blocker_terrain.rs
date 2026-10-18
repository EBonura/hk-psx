//! The admitted Blockers' `Terrain Block` edges, so the runtime can lift them (the Rust port of
//! host/blocker_terrain.py).
//!
//! A Blocker (Elder Baldur) sits on a `Terrain Block` child: a layer 8 box the world cook bakes
//! into the room pack as ordinary terrain (`blocker.rs` checks its footprint and leaves it to that
//! cook). In the source the whole Blocker GameObject is destroyed on death, block included, and its
//! `PersistentBoolItem` keeps it destroyed.
//!
//! This is the same join `battle_gates.rs` makes for arena gates: per catalogue slot, the edge
//! indices in that slot's room pack whose source is the block's collider, keyed by Blocker.
//! `game/src/blocker_terrain.rs` excludes them once the Blocker is dead. It reads
//! `data/regions.json` and the source scene and cooks nothing.
use crate::battle_gates::{edge_scratch_slots, neighbour_edges, region_edge_counts};
use crate::common::{components, err, kids, Result};
use crate::pyjson::{dumps, Json};
use hk_unity::scene::Scene;
use hk_unity::Source;
use serde_json::Value as J;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// `game/src/blocker_terrain.rs` keeps the dead set in one u8.
pub const MAX_BLOCKERS: usize = 8;

/// One admitted Blocker placement.
#[derive(Clone, Debug, PartialEq)]
pub struct Blocker {
    pub scene_id: usize,
    pub scene_file: String,
    pub source: String,
    pub game_object: i64,
    pub spec_source_id: i64,
}

/// `blockers`: every admitted Blocker placement.
pub fn blockers(report: &J) -> Result<Vec<Blocker>> {
    let mut found: Vec<Blocker> = Vec::new();
    for region in report["regions"].as_array().ok_or("regions")? {
        for actor in region["actors"].as_array().ok_or("actors")? {
            if actor["name"].as_str() != Some("Blocker") || actor["movement_supported"].as_bool() != Some(true) {
                continue;
            }
            let scene_id = region["scene_id"].as_u64().ok_or("scene_id")? as usize;
            let source = actor["source"].as_str().ok_or("actor source")?.to_owned();
            if found.iter().any(|b| b.scene_id == scene_id && b.source == source) {
                continue;
            }
            found.push(Blocker {
                scene_id,
                scene_file: region["scene_file"].as_str().ok_or("scene_file")?.to_owned(),
                source,
                game_object: actor["game_object"].as_i64().ok_or("game_object")?,
                spec_source_id: actor["spec_source_id"].as_i64().ok_or("spec_source_id")?,
            });
        }
    }
    let id = |b: &Blocker| b.source.rsplit(':').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    found.sort_by_key(|b| (b.scene_id, id(b)));
    if found.len() > MAX_BLOCKERS {
        return err(format!("{} Blockers exceed the {MAX_BLOCKERS}-bit dead set", found.len()));
    }
    Ok(found)
}

/// `terrain_colliders`: the `Terrain Block` child's collider sid for each Blocker.
fn terrain_colliders(source: &Source, rows: &[Blocker]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for b in rows {
        let sc = Scene::new(source, &b.scene_file).map_err(|e| e.to_string())?;
        let children = kids(&sc, b.game_object)?;
        // Python's dict keeps the last child of a name.
        let Some(&(_, gid)) = children.iter().rev().find(|c| c.0 == "Terrain Block") else {
            return err(format!("{} has no Terrain Block child", b.source));
        };
        let colliders: Vec<String> = components(&sc, gid)?
            .iter()
            .filter(|c| c.1 == "BoxCollider2D")
            .map(|c| format!("{}:{}", b.scene_file, c.0))
            .collect();
        if colliders.len() != 1 {
            return err(format!("{} Terrain Block carries {} box colliders", b.source, colliders.len()));
        }
        out.push(colliders[0].clone());
    }
    Ok(out)
}

/// `bind`: (catalogue slot, blocker, edges) for every region a block reached.
pub fn bind(
    rows: &[Blocker],
    colliders: &[String],
    report: &J,
    budget: usize,
    neighbours: &HashMap<usize, usize>,
    gates: &HashMap<usize, usize>,
) -> Result<Vec<(usize, usize, Vec<usize>)>> {
    let owner: HashMap<&str, usize> = colliders.iter().enumerate().map(|(i, c)| (c.as_str(), i)).collect();
    let mut out = Vec::new();
    for (slot, region) in report["regions"].as_array().ok_or("regions")?.iter().enumerate() {
        let mut found: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (edge, source) in region["edge_sources"].as_array().ok_or("edge_sources")?.iter().enumerate() {
            if let Some(&index) = source.as_str().and_then(|s| owner.get(s)) {
                if rows[index].scene_id != region["scene_id"].as_u64().unwrap_or(u64::MAX) as usize {
                    return err("a Blocker terrain edge outside its own scene");
                }
                found.entry(index).or_default().push(edge);
            }
        }
        let spent = found.values().map(Vec::len).sum::<usize>()
            + neighbours.get(&slot).copied().unwrap_or(0)
            + gates.get(&slot).copied().unwrap_or(0);
        if !found.is_empty() && spent > budget {
            return err(format!(
                "catalogue slot {slot} ({}) needs {spent} scripted edge exclusions against {budget} in game/src/world.rs",
                region["scene_name"].as_str().unwrap_or("")
            ));
        }
        for (index, edges) in found {
            out.push((slot, index, edges));
        }
    }
    Ok(out)
}

pub fn generate(rows: &[Blocker], bindings: &[(usize, usize, Vec<usize>)]) -> String {
    let lines = [
        "// Generated from the admitted Blockers and the cooked region edge sources.".to_owned(),
        "// host/hk-cook/src/blocker_terrain.rs is the generator.".to_owned(),
        format!("pub const BLOCKERS:usize={};", rows.len()),
        "/// (guest scene id, actor source id) per Blocker; the index is its bit in".to_owned(),
        "/// the dead set and its persist local id.".to_owned(),
        format!(
            "pub static SOURCES:&[(u16,u32)]=&[{}];",
            rows.iter().map(|r| format!("({},{}),", r.scene_id, r.spec_source_id)).collect::<String>()
        ),
        "/// (catalogue slot, Blocker, its Terrain Block edges in that slot's pack),".to_owned(),
        "/// sorted by slot so the runtime can binary search it.".to_owned(),
        format!(
            "pub static REGIONS:&[(u16,u8,&[u16])]=&[{}];",
            bindings
                .iter()
                .map(|(s, i, e)| format!("({},{},&[{}]),", s, i, e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")))
                .collect::<String>()
        ),
    ];
    lines.join("\n") + "\n"
}

pub fn main(root: &Path, source: Option<&Path>) -> Result<()> {
    let source = match source {
        Some(p) => Source::new(p),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let report: J = serde_json::from_slice(&std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let rows = blockers(&report)?;
    let colliders = terrain_colliders(&source, &rows)?;
    let bindings = bind(
        &rows,
        &colliders,
        &report,
        edge_scratch_slots(root)?,
        &neighbour_edges(root),
        &region_edge_counts(root),
    )?;
    std::fs::write(root.join("data/blocker_terrain.rs"), generate(&rows, &bindings)).map_err(|e| e.to_string())?;
    let record = Json::Obj(vec![
        (
            "blockers".into(),
            Json::List(
                rows.iter()
                    .zip(&colliders)
                    .map(|(r, c)| {
                        Json::Obj(vec![
                            ("scene".into(), Json::Int(r.scene_id as i64)),
                            ("source".into(), Json::Str(r.source.clone())),
                            ("terrain_collider".into(), Json::Str(c.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "bindings".into(),
            Json::List(
                bindings
                    .iter()
                    .map(|(s, i, e)| {
                        Json::Obj(vec![
                            ("slot".into(), Json::Int(*s as i64)),
                            ("blocker".into(), Json::Int(*i as i64)),
                            ("edges".into(), Json::List(e.iter().map(|x| Json::Int(*x as i64)).collect())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    std::fs::write(root.join(".hkpsx/blocker-terrain.json"), dumps(&record) + "\n").map_err(|e| e.to_string())?;
    println!(
        "{} Blockers, {} region bindings: {}",
        rows.len(),
        bindings.len(),
        rows.iter().zip(&colliders).map(|(r, c)| format!("{} block {c}", r.source)).collect::<Vec<_>>().join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> J {
        let blocker = json!({"name": "Blocker", "movement_supported": true, "source": "level50:4585",
            "game_object": 729, "spec_source_id": 4585});
        json!({"regions": [
            {"scene_id": 19, "scene_file": "level50", "scene_name": "X", "actors": [blocker.clone()],
             "edge_sources": ["level50:1", "level50:2936", "level50:2936"]},
            {"scene_id": 19, "scene_file": "level50", "scene_name": "X", "actors": [blocker],
             "edge_sources": ["level50:2"]},
            {"scene_id": 20, "scene_file": "level51", "scene_name": "Y", "actors": [], "edge_sources": []}]})
    }

    #[test]
    fn one_row_per_placement_and_every_block_edge_bound() {
        let r = report();
        let rows = blockers(&r).unwrap();
        assert_eq!(
            rows,
            [Blocker { scene_id: 19, scene_file: "level50".into(), source: "level50:4585".into(), game_object: 729, spec_source_id: 4585 }]
        );
        let none = HashMap::new();
        let bindings = bind(&rows, &["level50:2936".to_owned()], &r, 20, &none, &none).unwrap();
        assert_eq!(bindings, [(0, 0, vec![1, 2])]);
        let text = generate(&rows, &bindings);
        assert!(text.contains("pub static SOURCES:&[(u16,u32)]=&[(19,4585),];"));
        assert!(text.contains("pub static REGIONS:&[(u16,u8,&[u16])]=&[(0,0,&[1,2]),];"));
    }

    #[test]
    fn a_slot_over_the_scratch_budget_is_refused() {
        let r = report();
        let rows = blockers(&r).unwrap();
        // Two block edges plus a slot one short of full: over by one, whatever the budget is.
        let budget = 20;
        let neighbours: HashMap<usize, usize> = [(0, budget - 1)].into();
        let none = HashMap::new();
        let refused = bind(&rows, &["level50:2936".to_owned()], &r, budget, &neighbours, &none).unwrap_err();
        assert!(refused.contains("scripted edge exclusions"), "{refused}");
    }
}
