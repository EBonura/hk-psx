//! Inspect one exact original FSM, preserving enabled actions and guarded transitions.
#[path = "../grants.rs"]
mod grants;
use hk_unity::{
    playmaker::{action_fields_with_plain_strings, enabled},
    Source, Value,
};
use serde_json::json;
use sha2::{Digest, Sha256};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 4 {
        return Err("usage: fsm-graph ORIGINAL_WINDOWS_DATA FILE:PATH_ID OUTPUT_JSON".into());
    }
    let source = Source::new(a[1].clone())?;
    let (file, id) = a[2]
        .rsplit_once(':')
        .ok_or("source ID requires file:path")?;
    let file = source.file(file)?;
    let object = source.object(&file, id.parse()?)?;
    let tree = source.read(&object)?;
    let fsm = tree.get("fsm").ok_or("object is not an FSM")?;
    let vars = grants::variables(fsm);
    let mut states = Vec::new();
    for state in fsm.get("states").and_then(Value::list).unwrap_or(&[]) {
        let mut actions = Vec::new();
        if let Some(data) = state.get("actionData") {
            for (kind, index) in enabled(state) {
                let fields = action_fields_with_plain_strings(data, index, false)?;
                let decoded:Vec<_>=fields.iter().map(|(name,value)|json!({"name":name,"value":grants::scalar(value,&vars),"value_from":value.get("useVariable").filter(|v|v.truthy()).and_then(|_|value.get("name")).and_then(Value::str),"original_debug":format!("{value:?}")})).collect();
                actions.push(json!({"action":kind,"fields":decoded}));
            }
        }
        let transitions:Vec<_>=state.get("transitions").and_then(Value::list).unwrap_or(&[]).iter().map(|t|json!({"event":t.get("fsmEvent").and_then(|e|e.get("name")).and_then(Value::str),"to_state":t.get("toState").and_then(Value::str)})).collect();
        states.push(json!({"state":state.get("name").and_then(Value::str),"enabled_actions":actions,"transitions":transitions}));
    }
    std::fs::write(
        &a[3],
        serde_json::to_vec_pretty(
            &json!({"source_id":object.sid(),"source_sha256":format!("{:x}",Sha256::digest(object.raw()?)),"fsm":fsm.get("name").and_then(Value::str),"instance_variables":vars.iter().map(|(name,value)|json!({"name":name,"value":grants::scalar(value,&vars)})).collect::<Vec<_>>(),"selected_grant":match grants::shiny(fsm){Ok(grant)=>json!({"grant":grant}),Err(error)=>json!({"error":error})},"states":states,"interpretation":"Authored graph only. Disabled actions excluded; guards, runtime substitution and event reachability need explicit audit. Original SaveGame actions are user-excluded automatic-save behaviour, not missing port features."}),
        )?,
    )?;
    Ok(())
}
