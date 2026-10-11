//! Semantic catalogues from typed original fields and selected authored grants.
//! These are evidence rows, not an inferred implementation or parity claim.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::Path,
};
fn string<'a>(r: &'a Value, path: &str) -> Option<&'a str> {
    r["strings"].as_array()?.iter().find_map(|p| {
        (p[0].as_str() == Some(path))
            .then(|| p[1].as_str())
            .flatten()
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 3 {
        return Err("usage: semantic ORIGINAL_OUTPUT REPORT_OUTPUT".into());
    }
    let input = Path::new(&a[1]);
    let out = Path::new(&a[2]);
    std::fs::create_dir_all(out)?;
    let mut audited: BTreeMap<String, Value> = BTreeMap::new();
    if let Ok(file) = std::fs::File::open(out.join("feature-contracts.jsonl")) {
        for line in BufReader::new(file).lines() {
            let row: Value = serde_json::from_str(&line?)?;
            if let Some(sid) = row["sid"].as_str() {
                audited.insert(sid.into(), row);
            }
        }
    }
    let mut features: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    let mut unresolved = Vec::new();
    for line in BufReader::new(std::fs::File::open(input.join("source-objects.jsonl"))?).lines() {
        let r: Value = serde_json::from_str(&line?)?;
        let component = r["component"].as_str().unwrap_or("");
        let mut add = |category: &str, key: String, contract: Value| {
            features.entry((category.into(),key)).or_default().push(json!({"scene":r["scene"],"sid":r["sid"],"game_object":r["game_object_name"],"source_sha256":r["source_sha256"],"contract":contract,"runtime_and_journey_audit":audited.get(r["sid"].as_str().unwrap_or("")).filter(|proof|proof["source_sha256"]==r["source_sha256"])}));
        };
        if component.starts_with("EnemyDeathEffects") {
            if let Some(key) = string(&r, "/playerDataName") {
                if !key.is_empty() {
                    add(
                        "enemy journal family",
                        key.into(),
                        json!({"component":component,"role_note":"Journal association is an authored family key, not proof that every health-bearing object is this enemy."}),
                    );
                }
            }
        }
        if component == "BossStatue" {
            if let Some(key) = string(&r, "/bossDetails/nameKey") {
                add(
                    "boss statue catalogue",
                    key.into(),
                    json!({"name_key":key,"description_key":string(&r,"/bossDetails/descriptionKey"),"variant_note":"Godhome statues and alternate arenas require encounter alias audit, not additional unique boss counts."}),
                );
            }
        }
        if component == "InvCharmBackboard" {
            if let Some(id) = r["scalars"]["charmNum"].as_i64() {
                if (1..=40).contains(&id) {
                    add(
                        "charm",
                        id.to_string(),
                        json!({"original_id":id,"inventory_fields":r["strings"],"scope":"Acquisition and equipped effect need separate implementation audits."}),
                    );
                }
            }
        }
        if component == "TransitionPoint" {
            add(
                "scene transition",
                r["sid"].as_str().unwrap().into(),
                json!({"target_scene":string(&r,"/targetScene"),"entry_point":string(&r,"/entryPoint")}),
            );
        }
        if component == "SceneManager" {
            add(
                "area scene",
                r["scene"].as_str().unwrap().into(),
                json!({"settings":r["scalars"],"scope":"Scene settings include menu, test, dream and additive scenes; gameplay-area aliases still need audit."}),
            );
        }
        if component == "RestBench"
            || component == "Breakable"
            || component == "DreamPlant"
            || component == "Grub"
        {
            add(
                "interactable source contract",
                format!("{}:{}", component, r["sid"].as_str().unwrap()),
                json!({"component":component}),
            );
        }
        if r["fsm_name"] == "Conversation Control" {
            add(
                "NPC conversation instance",
                r["sid"].as_str().unwrap().into(),
                json!({"fsm":"Conversation Control","scope":"One original conversation instance; not a unique character count."}),
            );
        }
        if let Some(grant) = r["shiny_grant"].as_object() {
            let g = Value::Object(grant.clone());
            if let Some(id) = g["charm_id"].as_i64() {
                if (1..=40).contains(&id) {
                    add("charm", id.to_string(), json!({"selected_pickup":g}));
                } else {
                    unresolved.push(json!({"sid":r["sid"],"reason":"selected charm ID outside inventory 1..40", "charm_id":id}));
                }
            } else {
                for w in g["writes"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(field) = w["field"].as_str() {
                        if !w["value"].is_null() && w["value_from"].is_null() {
                            add(
                                "selected pickup grant",
                                format!("{}={}", field, w["value"]),
                                json!({"selected_route":g,"write":w,"scope":"Selected first target only; onward graph may grant additional abilities. Zote Event is not a Mantis Claw grant."}),
                            );
                        } else {
                            unresolved.push(json!({"sid":r["sid"],"reason":"runtime assigned grant value","write":w}));
                        }
                    }
                }
            }
        }
        if !r["grant_error"].is_null() {
            unresolved.push(json!({"sid":r["sid"],"scene":r["scene"],"reason":r["grant_error"]}));
        }
        if r["fsm_name"] != "Shiny Control" {
            for w in r["playerdata_writes"].as_array().unwrap_or(&Vec::new()) {
                let f = w["write"]["field"].as_str().unwrap_or("");
                if [
                    "hasShadowDash",
                    "hasDoubleJump",
                    "hasSuperDash",
                    "hasDreamNail",
                    "hasDreamGate",
                    "hasAcidArmour",
                    "fireballLevel",
                    "quakeLevel",
                    "screamLevel",
                ]
                .contains(&f)
                {
                    let value = &w["write"]["value"];
                    if !value.is_null()
                        && w["write"]["value_from"].is_null()
                        && (value == &json!(true) || value.as_i64().is_some_and(|x| x > 0))
                    {
                        add(
                            "ability authored write",
                            format!("{}={}", f, value),
                            json!({"fsm":r["fsm_name"],"write":w,"scope":"Enabled authored write, reachability and acquisition trigger still require graph audit."}),
                        );
                    }
                }
            }
        }
    }
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(out.join("semantic-features.jsonl"))?);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for ((category, key), locations) in &features {
        *counts.entry(category.clone()).or_default() += 1;
        serde_json::to_writer(
            &mut file,
            &json!({"category":category,"original_key":key,"original_locations":locations,"implementation_status":"Review per-location runtime_and_journey_audit; source alias and full semantic parity remain unresolved","journey_exercised":locations.iter().all(|location|location["runtime_and_journey_audit"]["journey_exercised"]==true),"journey_reason":"Only currently validated per-location original-behaviour assertions and receipt hashes count; scene visits do not prove the semantic feature."}),
        )?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    std::fs::write(
        out.join("semantic-summary.json"),
        serde_json::to_vec_pretty(
            &json!({"counts_by_explicit_unit":counts,"semantic_census_complete":false,"unresolved_grants":unresolved,"coverage":"Typed scene and resource metadata, selected Shiny instance branches and explicit enabled PlayerData writes. External sharedasset prefabs, full grant reachability, enemy/boss aliases, localization, equipment catalogue and runtime/journey audits remain incomplete. Counts cannot be summed into a unique gameplay feature total."}),
        )?,
    )?;
    Ok(())
}
