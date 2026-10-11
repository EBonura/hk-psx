//! Resolve each Shiny Control instance's authored routing, not every prefab branch.
use hk_unity::{
    playmaker::{action_fields, enabled, field},
    Value,
};
use serde_json::{json, Value as J};
use std::collections::BTreeMap;
pub fn scalar(v: &Value, vars: &BTreeMap<String, Value>) -> J {
    if v.get("useVariable").is_some_and(Value::truthy) {
        return v
            .get("name")
            .and_then(Value::str)
            .and_then(|n| vars.get(&n))
            .map(|x| scalar(x, &BTreeMap::new()))
            .unwrap_or(J::Null);
    }
    if let Some(x) = v.get("value") {
        return scalar(x, vars);
    }
    match v {
        Value::Bool(x) => json!(x),
        Value::Int(x) => json!(x),
        Value::UInt(x) => json!(x),
        Value::F32(x) => json!(x),
        Value::F64(x) => json!(x),
        Value::Str(x) => json!(String::from_utf8_lossy(x)),
        _ => J::Null,
    }
}
pub fn variables(fsm: &Value) -> BTreeMap<String, Value> {
    let mut vars = BTreeMap::new();
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (_, list) in groups {
            for v in list.list().unwrap_or(&[]) {
                if let (Some(name), Some(value)) =
                    (v.get("name").and_then(Value::str), v.get("value"))
                {
                    vars.insert(name, value.clone());
                }
            }
        }
    }
    vars
}
fn action_list(state: &Value, kind: &str) -> Result<Vec<hk_unity::playmaker::Fields>, String> {
    let d = state.get("actionData").ok_or("no actionData")?;
    enabled(state)
        .into_iter()
        .filter(|(n, _)| n == kind)
        .map(|(_, i)| action_fields(d, i, false).map_err(|e| e.to_string()))
        .collect()
}
fn target(state: &Value, event: &str) -> Option<String> {
    state
        .get("transitions")?
        .list()?
        .iter()
        .find(|t| {
            t.get("fsmEvent")
                .and_then(|x| x.get("name"))
                .and_then(Value::str)
                .as_deref()
                == Some(event)
        })
        .and_then(|t| t.get("toState"))
        .and_then(Value::str)
}
fn writes(state: &Value, vars: &BTreeMap<String, Value>) -> Result<Vec<J>, String> {
    let d = state.get("actionData").ok_or("no actionData")?;
    let mut out = Vec::new();
    for (kind, index) in enabled(state) {
        if ![
            "SetPlayerDataBool",
            "SetPlayerDataInt",
            "IncrementPlayerDataInt",
            "PlayerDataIntAdd",
            "SetPlayerDataFloat",
        ]
        .contains(&kind.as_str())
        {
            continue;
        }
        let fields = action_fields(d, index, false).map_err(|e| e.to_string())?;
        let name = field(&fields, "boolName")
            .or_else(|| field(&fields, "intName"))
            .or_else(|| field(&fields, "floatName"))
            .map(|x| scalar(x, vars));
        let amount = field(&fields, "value").or_else(|| field(&fields, "amount"));
        out.push(json!({"action":kind,"field":name,"value":amount.map(|x|scalar(x,vars)).unwrap_or(json!(1)),"value_from":amount.filter(|x|x.get("useVariable").is_some_and(Value::truthy)).and_then(|x|x.get("name")).and_then(Value::str)}));
    }
    Ok(out)
}
/// Follow only a unique FINISHED edge from the instance-selected state.
/// Multiple/event-dependent edges stop the trace rather than selecting a branch.
fn finished_writes(
    states: &BTreeMap<String, &Value>,
    first: &str,
    vars: &BTreeMap<String, Value>,
) -> Result<J, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut name = first.to_string();
    let mut trace = Vec::new();
    let mut all = Vec::new();
    let reason = loop {
        if !seen.insert(name.clone()) {
            break "cycle requires event audit";
        }
        let state = states.get(&name).ok_or("missing follow-on state")?;
        trace.push(name.clone());
        for mut write in writes(state, vars)? {
            write["authored_state"] = json!(name);
            all.push(write);
        }
        let transitions = state
            .get("transitions")
            .and_then(Value::list)
            .unwrap_or(&[]);
        if transitions.is_empty() {
            break "terminal selected path";
        }
        if transitions.len() != 1 {
            break "multiple event edges require semantic audit";
        }
        let transition = &transitions[0];
        if transition
            .get("fsmEvent")
            .and_then(|e| e.get("name"))
            .and_then(Value::str)
            .as_deref()
            != Some("FINISHED")
        {
            break "non-FINISHED event requires reachability audit";
        }
        name = transition
            .get("toState")
            .and_then(Value::str)
            .ok_or("follow-on transition has no target")?;
    };
    Ok(
        json!({"states":trace,"writes":all,"stop_reason":reason,"scope":"Authored unique FINISHED trace only; runtime completion and external events are not proven."}),
    )
}
pub fn shiny(fsm: &Value) -> Result<Option<J>, String> {
    if fsm.get("name").and_then(Value::str).as_deref() != Some("Shiny Control") {
        return Ok(None);
    }
    let vars = variables(fsm);
    let states: BTreeMap<_, _> = fsm
        .get("states")
        .and_then(Value::list)
        .ok_or("no states")?
        .iter()
        .filter_map(|s| Some((s.get("name")?.str()?, s)))
        .collect();
    if vars.get("Charm").is_some_and(Value::truthy) {
        let state = states.get("Get Charm").ok_or("no Get Charm")?;
        return Ok(Some(
            json!({"route":"charm","charm_id":vars.get("Charm ID").map(|x|scalar(x,&vars)),"state":"Get Charm","writes":writes(state,&vars)?}),
        ));
    }
    let choice = states.get("Item Choice").ok_or("no Item Choice")?;
    for fields in action_list(choice, "BoolTest")? {
        let Some(test) = field(&fields, "boolVariable") else {
            continue;
        };
        let flag = test.get("name").and_then(Value::str).unwrap_or_default();
        if vars.get(&flag).is_some_and(Value::truthy) {
            let event = field(&fields, "isTrue")
                .and_then(Value::str)
                .ok_or("BoolTest no true event")?;
            let name = target(choice, &event).ok_or("no true transition")?;
            states.get(&name).ok_or("no chosen state")?;
            let trace = finished_writes(&states, &name, &vars)?;
            return Ok(Some(
                json!({"route":"ability/big item","state":name,"source_choice_variable":flag,"writes":trace["writes"],"authored_finished_trace":trace}),
            ));
        }
    }
    let num = vars.get("Trinket Num").and_then(Value::int).unwrap_or(0);
    if num > 0 {
        let state = states.get("Trinket Type").ok_or("no Trinket Type")?;
        let actions = action_list(state, "IntSwitch")?;
        let fields = actions.first().ok_or("no IntSwitch")?;
        let compares: Vec<_> = fields
            .iter()
            .skip(1)
            .filter_map(|(_, v)| v.get("value").and_then(Value::int))
            .collect();
        let events: Vec<_> = fields.iter().skip(1).filter_map(|(_, v)| v.str()).collect();
        if compares.len() != events.len() {
            return Err("IntSwitch compared/event arity mismatch".into());
        }
        let event = compares
            .iter()
            .zip(&events)
            .find(|(n, _)| **n == num)
            .map(|(_, e)| e)
            .ok_or("trinket has no event")?;
        let name = target(state, event).ok_or("trinket no transition")?;
        let selected = states.get(&name).ok_or("trinket no selected state")?;
        return Ok(Some(
            json!({"route":"item/relic/key/resource","trinket_num":num,"state":name,"writes":writes(selected,&vars)?}),
        ));
    }
    Ok(Some(
        json!({"route":"assigned at runtime","writes":[],"semantic_review_required":true}),
    ))
}
pub fn pd_writes(fsm: &Value) -> Vec<J> {
    let vars = variables(fsm);
    let mut out = Vec::new();
    for state in fsm.get("states").and_then(Value::list).unwrap_or(&[]) {
        match writes(state, &vars) {
            Ok(w) => {
                for write in w {
                    out.push(json!({"state":state.get("name").and_then(Value::str),"write":write}))
                }
            }
            Err(e) => out.push(json!({"state":state.get("name").and_then(Value::str),"error":e})),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(j: J) -> Value {
        match j {
            J::Null => Value::Map(vec![]),
            J::Bool(x) => Value::Bool(x),
            J::Number(x) => Value::Int(x.as_i64().unwrap()),
            J::String(x) => Value::Str(x.into_bytes()),
            J::Array(x) => Value::List(x.into_iter().map(value).collect()),
            J::Object(x) => Value::Map(x.into_iter().map(|(k, v)| (k.into(), value(v))).collect()),
        }
    }
    fn write_state(name: &str, field_name: &str, amount_variable: Option<&str>) -> J {
        let mut raw = vec![1u8, if amount_variable.is_some() { 1 } else { 0 }];
        if let Some(s) = amount_variable {
            raw.extend(s.as_bytes());
        }
        json!({"name":name,"actionData":{"actionNames":["HutongGames.PlayMaker.Actions.SetPlayerDataBool"],"actionEnabled":[true],"actionStartIndex":[0],"paramName":["boolName","value"],"paramDataType":[18,17],"paramDataPos":[0,0],"paramByteDataSize":[0,raw.len()],"byteData":raw,"fsmStringParams":[{"value":field_name,"name":"","useVariable":false}]}})
    }
    #[test]
    fn chosen_charm_instance_does_not_count_unused_ability_prefab_branch() {
        let fsm = value(
            json!({"name":"Shiny Control","variables":{"boolVariables":[{"name":"Charm","value":true}],"intVariables":[{"name":"Charm ID","value":7}]},"states":[write_state("Get Charm","gotCharm_7",None),write_state("Get Dash","hasDash",None)]}),
        );
        let grant = shiny(&fsm).unwrap().unwrap();
        assert_eq!(grant["charm_id"], 7);
        assert_eq!(grant["writes"][0]["field"], "gotCharm_7");
        assert_eq!(grant["writes"].as_array().unwrap().len(), 1);
        assert!(!grant.to_string().contains("hasDash"));
    }
    #[test]
    fn selected_intermediate_state_follows_only_finished_and_stops_before_alternatives() {
        let mut event = write_state("Zote Event", "zoteDead", None);
        event["transitions"] = json!([{"fsmEvent":{"name":"FINISHED"},"toState":"Walljump"}]);
        let mut wall = write_state("Walljump", "hasWalljump", None);
        wall["transitions"] = json!([{"fsmEvent":{"name":"YES"},"toState":"Unused Dash"},{"fsmEvent":{"name":"NO"},"toState":"Done"}]);
        let values = [
            value(event),
            value(wall),
            value(write_state("Unused Dash", "hasDash", None)),
        ];
        let states = values
            .iter()
            .map(|state| (state.get("name").unwrap().str().unwrap(), state))
            .collect();
        let trace = finished_writes(&states, "Zote Event", &BTreeMap::new()).unwrap();
        assert_eq!(trace["writes"][1]["field"], "hasWalljump");
        assert_eq!(trace["states"], json!(["Zote Event", "Walljump"]));
        assert!(!trace.to_string().contains("hasDash"));
    }
    #[test]
    fn runtime_amount_variable_retains_uncertainty_instead_of_granting_default() {
        let fsm = value(
            json!({"variables":{},"states":[write_state("Give Dash","hasDash",Some("Runtime Bool"))]}),
        );
        let writes = pd_writes(&fsm);
        assert_eq!(writes[0]["write"]["value"], J::Null);
        assert_eq!(writes[0]["write"]["value_from"], "Runtime Bool");
    }
}
