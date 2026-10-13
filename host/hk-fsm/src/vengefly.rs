//! Original per-placement Vengefly graph admission. Both owned FSMs are required;
//! unsupported actions fail before any bank or dispatch table is published.
use std::{collections::BTreeMap, error::Error, path::Path};
use hk_unity::{Source, Value};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub fn cook_vengeflies(root: &Path) -> Result<()> {
    let source = Source::from_doctor(root)?;
    let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("data/regions.json"))?)?;
    let mut placements = BTreeMap::new();
    for region in metadata["regions"].as_array().ok_or("regions")? {
        for actor in region["actors"].as_array().ok_or("actors")? {
            if actor["movement_control"]["kind"].as_str() != Some("Vengefly") { continue; }
            let key = (region["scene_id"].as_u64().ok_or("scene")?, actor["spec_source_id"].as_u64().ok_or("source")?);
            let owner = actor["game_object"].as_i64().ok_or("owner")?;
            let graphs = actor["fsm_ids"].as_array().ok_or("FSM ids")?.iter()
                .map(|id| id.as_str().map(str::to_owned).ok_or("FSM id"))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if let Some(previous) = placements.insert(key, (owner, graphs.clone())) {
                if previous != (owner, graphs) { return Err("conflicting Vengefly region aliases".into()); }
            }
        }
    }
    if placements.len() != 20 { return Err("admitted Vengefly placement set changed".into()); }
    let mut banks: Vec<Vec<u8>> = Vec::new();
    let mut table = Vec::new();
    for ((scene, id), (owner, graphs)) in placements {
        if graphs.len() != 2 { return Err("Vengefly requires both owned original FSMs".into()); }
        let mut named = BTreeMap::new();
        for sid in graphs {
            let (file, path) = sid.rsplit_once(':').ok_or("FSM identity")?;
            let file = source.file(file)?;
            let object = source.object(&file, path.parse()?)?;
            let tree = source.read(&object)?;
            if tree.get("m_GameObject").and_then(|v|v.get("m_PathID")).and_then(Value::int) != Some(owner) {
                return Err("Vengefly FSM owner mismatch".into());
            }
            let fsm = tree.get("fsm").ok_or("FSM")?;
            let name = fsm.get("name").and_then(Value::str).ok_or("FSM name")?;
            if !["chaser", "flyer_receive_direction_msg"].contains(&name.as_str()) || named.contains_key(&name) {
                return Err("unexpected Vengefly FSM graph set".into());
            }
            let bytes = crate::cook(fsm)?;
            let index = if let Some(index) = banks.iter().position(|bank| bank == &bytes) { index }
                else { let index = banks.len(); banks.push(bytes); index };
            named.insert(name, index);
        }
        table.push((scene, id, *named.get("chaser").ok_or("chaser")?,
            *named.get("flyer_receive_direction_msg").ok_or("direction receiver")?));
    }
    // Publication remains unavailable until every enabled action is admitted.
    // A following integration adds runtime scheduling and the exact bank layout.
    println!("Vengefly: {} placements, {} original graph banks admitted", table.len(), banks.len());
    Ok(())
}
