//! Additional authored graph evidence never promotes runtime or journey status.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
fn field<'a>(action: &'a Value, name: &str) -> Option<&'a Value> {
    action["fields"]
        .as_array()?
        .iter()
        .find(|v| v["name"] == name)
}
fn target(state: &Value, event: &str) -> Option<String> {
    let edges: Vec<_> = state["transitions"]
        .as_array()?
        .iter()
        .filter(|e| e["event"] == event)
        .collect();
    (edges.len() == 1)
        .then(|| edges[0]["to_state"].as_str().map(str::to_string))
        .flatten()
}
fn truth(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        value
            .as_i64()
            .filter(|n| *n == 0 || *n == 1)
            .map(|n| n != 0)
    })
}
pub fn selected(graph: &Value, row: &Value) -> Result<Value, String> {
    let sid = row["sid"].as_str().ok_or("cache source ID missing")?;
    let hash = row["source_sha256"]
        .as_str()
        .ok_or("cache raw hash missing")?;
    if hash.len() != 64 || graph["source_id"] != sid || graph["source_sha256"] != hash {
        return Err("graph source ID/raw hash differs from cached original".into());
    }
    if row["fsm_name"] != "Shiny Control" || graph["fsm"] != "Shiny Control" {
        return Err("overlay is not a Shiny Control source contract".into());
    }
    let mut states = BTreeMap::new();
    for state in graph["states"].as_array().ok_or("states missing")? {
        let name = state["state"].as_str().ok_or("state name missing")?;
        if states.insert(name, state).is_some() {
            return Err("duplicate graph state".into());
        }
    }
    let mut vars = BTreeMap::new();
    for v in graph["instance_variables"]
        .as_array()
        .ok_or("instance variables missing")?
    {
        let name = v["name"].as_str().ok_or("variable name missing")?;
        if vars.insert(name, &v["value"]).is_some() {
            return Err("duplicate instance variable".into());
        }
    }
    let mut name = "Big Get Flash".to_string();
    let mut seen = BTreeSet::new();
    let mut trace = Vec::new();
    let mut writes = Vec::new();
    let mut guards = Vec::new();
    let stop = loop {
        if !seen.insert(name.clone()) {
            break "cycle requires runtime audit";
        }
        let state = states
            .get(name.as_str())
            .ok_or("selected graph target missing")?;
        trace.push(name.clone());
        let actions = state["enabled_actions"]
            .as_array()
            .ok_or("enabled actions missing")?;
        for action in actions {
            let kind = action["action"].as_str().ok_or("action kind missing")?;
            if ![
                "SetPlayerDataBool",
                "SetPlayerDataInt",
                "IncrementPlayerDataInt",
            ]
            .contains(&kind)
            {
                continue;
            }
            let key = field(action, "boolName").or_else(|| field(action, "intName"));
            let value = field(action, "value").or_else(|| field(action, "amount"));
            let resolved = value.map(|v| v["value"].clone()).unwrap_or(Value::Null);
            writes.push(json!({"authored_state":name,"action":kind,"field":key.map(|k|k["value"].clone()).unwrap_or(Value::Null),"field_value_from":key.map(|k|k["value_from"].clone()).unwrap_or(Value::Null),"value":resolved,"value_from":value.map(|v|v["value_from"].clone()).unwrap_or(Value::Null),"literal_evidence_reason":if key.is_none(){"field parameter absent: unknown"}else if value.is_none(){"value/amount parameter absent: unknown, no default inferred"}else if key.is_some_and(|k|!k["value_from"].is_null()){ "field name bound to variable: unknown" }else{"review actual typed parameters"},"runtime_execution":"unknown"}));
        }
        // Only these exact choice nodes are supported. External events never select a grant.
        if name == "Item Choice" || name == "Type" {
            let tests: Vec<_> = actions
                .iter()
                .filter(|a| a["action"] == "BoolTest")
                .collect();
            if tests.is_empty() || (name == "Type" && tests.len() != 1) {
                break "choice shape requires semantic audit";
            }
            let mut next = None;
            for test in tests {
                let input = field(test, "boolVariable").ok_or("choice input missing")?;
                let variable = input["value_from"]
                    .as_str()
                    .ok_or("choice has no explicit instance variable binding")?;
                let authored = vars.get(variable).ok_or("choice variable absent")?;
                if **authored != input["value"] {
                    return Err("choice value differs from instance variable".into());
                }
                let yes = truth(authored).ok_or("choice variable is dynamic or nonboolean")?;
                // A variable assignment in any enabled state makes its initial value insufficient.
                if states.values().any(|s| {
                    s["enabled_actions"].as_array().is_some_and(|a| {
                        a.iter().any(|a| {
                            a["action"] == "SetBoolValue"
                                && field(a, "boolVariable")
                                    .is_some_and(|f| f["value_from"] == variable)
                        })
                    })
                }) {
                    break;
                }
                if name == "Item Choice" && !yes {
                    continue;
                }
                let event = field(test, if yes { "isTrue" } else { "isFalse" })
                    .and_then(|f| f["value"].as_str())
                    .ok_or("choice event missing")?;
                let selected = target(state, event).ok_or("choice event target is ambiguous")?;
                guards.push(json!({"state":name,"variable":variable,"authored_value":authored,"event":event,"target":selected,"runtime_value":"unknown"}));
                next = Some(selected);
                break;
            }
            if let Some(next) = next {
                name = next;
                continue;
            }
            break "choice not established by checked authored selection";
        }
        let edges = state["transitions"]
            .as_array()
            .ok_or("transitions missing")?;
        if edges.is_empty() {
            break "terminal authored path";
        }
        if edges.len() != 1 || edges[0]["event"] != "FINISHED" {
            break "external or alternative events require runtime audit";
        }
        name = edges[0]["to_state"]
            .as_str()
            .ok_or("FINISHED target missing")?
            .into();
    };
    Ok(
        json!({"source_id":sid,"scene":row["scene"],"source_sha256":hash,"graph_file_sha256":graph["_overlay_file_sha256"],"authored_states":trace,"authored_guards":guards,"writes":writes,"stop_reason":stop,"authored_scope":"Checked authored selection and unique FINISHED edges; SetBoolValue assignments checked, other mutation mechanisms and runtime reachability unknown","runtime_reachability":"unknown","port_implementation":"unknown","journey_exercised":false,"journey_reason":"No structured original-contract and disc-bound execution receipt"}),
    )
}
pub fn literal_write(write: &Value) -> bool {
    let typed_value = match write["action"].as_str() {
        Some("SetPlayerDataBool") => write["value"].is_boolean(),
        Some("SetPlayerDataInt" | "IncrementPlayerDataInt") => write["value"].as_i64().is_some(),
        _ => false,
    };
    write["field"].as_str().is_some()
        && write["field_value_from"].is_null()
        && !write["value"].is_null()
        && write["value_from"].is_null()
        && typed_value
}
pub fn ranked_rows(proofs: &[Value]) -> Vec<Value> {
    let mut rows = Vec::new();
    for proof in proofs {
        let rank = match proof["source_id"].as_str().unwrap_or("") {
            "level178:7543" => 10,
            "level348:1225" | "level373:6394" => 30,
            "level423:6506" => 40,
            "level482:2465" => 60,
            "level124:6133" => 70,
            _ => 80,
        };
        for (index, write) in proof["writes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .enumerate()
        {
            rows.push(json!({"priority":rank,"source_id":proof["source_id"],"scene":proof["scene"],"source_sha256":proof["source_sha256"],"graph_file_sha256":proof["graph_file_sha256"],"index":index,"write":write}));
        }
    }
    rows.sort_by(|a, b| {
        (
            a["priority"].as_u64(),
            a["source_id"].as_str(),
            a["write"]["authored_state"].as_str(),
            a["write"]["field"].as_str(),
            a["index"].as_u64(),
        )
            .cmp(&(
                b["priority"].as_u64(),
                b["source_id"].as_str(),
                b["write"]["authored_state"].as_str(),
                b["write"]["field"].as_str(),
                b["index"].as_u64(),
            ))
    });
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(queen: bool) -> (Value, Value) {
        let hash = "a".repeat(64);
        let row = json!({"sid":"level1:2","source_sha256":hash,"fsm_name":"Shiny Control"});
        let write = |state: &str, key: &str| json!({"state":state,"enabled_actions":[{"action":"SetPlayerDataBool","fields":[{"name":"boolName","value":key},{"name":"value","value":true,"value_from":null}]}],"transitions":[{"event":"EXTERNAL","to_state":"Unused"}]});
        let graph = json!({"source_id":"level1:2","source_sha256":hash,"fsm":"Shiny Control","instance_variables":[{"name":"Queen Charm","value":queen}],"states":[{"state":"Big Get Flash","enabled_actions":[],"transitions":[{"event":"FINISHED","to_state":"Type"}]},{"state":"Type","enabled_actions":[{"action":"BoolTest","fields":[{"name":"boolVariable","value":queen,"value_from":"Queen Charm"},{"name":"isTrue","value":"QUEEN"},{"name":"isFalse","value":"KING"}]}],"transitions":[{"event":"QUEEN","to_state":"Queen"},{"event":"KING","to_state":"King"}]},write("Queen","gotQueenFragment"),write("King","gotKingFragment"),write("Unused","hasDash")]});
        (graph, row)
    }
    #[test]
    fn hash_drift_rejects_overlay() {
        let (mut graph, row) = fixture(true);
        graph["source_sha256"] = json!("b".repeat(64));
        assert!(selected(&graph, &row).is_err());
        let (mut graph, row) = fixture(true);
        graph["source_id"] = json!("level1:3");
        assert!(selected(&graph, &row).is_err());
    }
    #[test]
    fn queen_king_instance_choices_are_distinct() {
        for queen in [true, false] {
            let (graph, row) = fixture(queen);
            let proof = selected(&graph, &row).unwrap();
            assert_eq!(
                proof["writes"][0]["field"],
                if queen {
                    "gotQueenFragment"
                } else {
                    "gotKingFragment"
                }
            );
            assert_eq!(proof["writes"].as_array().unwrap().len(), 1);
            assert_eq!(proof["journey_exercised"], false);
        }
    }
    #[test]
    fn external_branch_never_adds_unused_grant() {
        let (graph, row) = fixture(true);
        assert!(!selected(&graph, &row)
            .unwrap()
            .to_string()
            .contains("hasDash"));
    }
    #[test]
    fn mutable_choice_variable_cannot_select_authored_grant() {
        let (mut graph, row) = fixture(true);
        graph["states"][0]["enabled_actions"] = json!([{"action":"SetBoolValue","fields":[{"name":"boolVariable","value":true,"value_from":"Queen Charm"},{"name":"boolValue","value":false}]}]);
        let proof = selected(&graph, &row).unwrap();
        assert!(proof["writes"].as_array().unwrap().is_empty());
        assert_eq!(
            proof["stop_reason"],
            "choice not established by checked authored selection"
        );
    }
    #[test]
    fn absent_set_values_are_unknown_without_invented_defaults() {
        for kind in [
            "SetPlayerDataBool",
            "SetPlayerDataInt",
            "IncrementPlayerDataInt",
        ] {
            let (mut graph, row) = fixture(true);
            graph["states"][2]["enabled_actions"] = json!([{"action":kind,"fields":[{"name":"intName","value":"someField","value_from":null}]}]);
            let proof = selected(&graph, &row).unwrap();
            let write = &proof["writes"][0];
            assert!(write["value"].is_null());
            assert!(!literal_write(write));
        }
    }
    #[test]
    fn bound_field_name_is_preserved_and_excluded_from_literal_counts() {
        let (mut graph, row) = fixture(true);
        graph["states"][2]["enabled_actions"][0]["fields"][0]["value_from"] =
            json!("Runtime Field");
        let proof = selected(&graph, &row).unwrap();
        assert_eq!(proof["writes"][0]["field_value_from"], "Runtime Field");
        assert!(!literal_write(&proof["writes"][0]));
    }
    #[test]
    fn wrong_parameter_type_cannot_count_as_literal_write() {
        let (graph, row) = fixture(true);
        let mut write = selected(&graph, &row).unwrap()["writes"][0].clone();
        write["value"] = json!("true");
        assert!(!literal_write(&write));
    }
    #[test]
    fn ranked_rows_put_progression_before_source_order() {
        let (graph, row) = fixture(true);
        let mut late = selected(&graph, &row).unwrap();
        late["source_id"] = json!("level124:6133");
        let mut early = late.clone();
        early["source_id"] = json!("level178:7543");
        let rows = ranked_rows(&[late, early]);
        assert_eq!(rows[0]["priority"], 10);
        assert_eq!(rows[1]["priority"], 70);
    }
}
