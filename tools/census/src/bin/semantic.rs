//! Semantic catalogues from typed original fields and selected authored grants.
//! These are evidence rows, not an inferred implementation or parity claim.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[path = "../catalogue.rs"]
mod catalogue;
#[path = "../overlays.rs"]
mod overlays;
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
    if a.len() != 3 && a.len() != 4 {
        return Err(
            "usage: semantic ORIGINAL_OUTPUT REPORT_OUTPUT [AUTHORED_GRAPH_DIRECTORY]".into(),
        );
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
    let mut graphs = BTreeMap::new();
    if let Some(directory) = a.get(3) {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let mut graph: Value = serde_json::from_slice(&bytes)?;
            graph["_overlay_file_sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
            let sid = graph["source_id"]
                .as_str()
                .ok_or("graph source ID absent")?
                .to_string();
            if graphs.insert(sid, graph).is_some() {
                return Err("duplicate graph source ID".into());
            }
        }
    }
    let mut validated_overlays = Vec::new();
    for line in catalogue::Catalogue::open(input)? {
        let r: Value = serde_json::from_str(&line?)?;
        let overlay = graphs
            .remove(r["sid"].as_str().unwrap_or(""))
            .map(|graph| overlays::selected(&graph, &r))
            .transpose()?;
        let component = r["component"].as_str().unwrap_or("");
        let mut add = |category: &str, key: String, contract: Value| {
            features.entry((category.into(),key)).or_default().push(json!({"scene":r["scene"],"sid":r["sid"],"game_object":r["game_object_name"],"source_sha256":r["source_sha256"],"contract":contract,"runtime_and_journey_audit":audited.get(r["sid"].as_str().unwrap_or("")).filter(|proof|proof["source_sha256"]==r["source_sha256"])}));
        };
        if let Some(proof) = overlay {
            for write in proof["writes"].as_array().ok_or("overlay writes missing")? {
                if let Some(field) = write["field"].as_str() {
                    if overlays::literal_write(write) {
                        add(
                            "authored graph selected write",
                            format!("{}={}", field, write["value"]),
                            json!({"graph_evidence":proof,"write":write,"runtime_reachability":"unknown","port_implementation":"unknown","journey_exercised":false}),
                        );
                        if field == "hasWalljump"
                            && (write["value"] == true || write["value"].as_i64() == Some(1))
                        {
                            add(
                                "ability authored write",
                                "hasWalljump=true".into(),
                                json!({"graph_evidence":proof,"write":write,"scope":"Mantis Claw authored acquisition write; canWallJump is a supporting flag, not another ability. Runtime collection and implementation remain unknown."}),
                            );
                        }
                    }
                }
            }
            validated_overlays.push(proof);
        }
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
    if !graphs.is_empty() {
        return Err("graph source IDs absent from checked catalogue".into());
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
    if a.len() == 4 {
        let total: usize = validated_overlays
            .iter()
            .map(|p| p["writes"].as_array().map_or(0, Vec::len))
            .sum();
        let mut markdown=format!("# Targeted authored grant overlays\n\nCount unit: authored selected write instances across {} original FSM instances. Missing implementation proven: 0; implemented but unexercised proven: 0; implementation unknown: {total}; total: {total}. All {total} are journey unexercised. Existing unresolved source grants and the complete missing/unexercised queue remain unchanged. Runtime acquisition is unknown for every row.\n\n| Queue ID | Priority | Scene and original ID | Authored state/write | Original raw SHA | Graph file SHA | Port implementation | Journey |\n|---|---|---|---|---|---|---|---|\n",validated_overlays.len());
        for row in overlays::ranked_rows(&validated_overlays) {
            let write = &row["write"];
            markdown.push_str(&format!("| grant:{}:{} | {} | {} / {} | {}: {}={} (key binding {}, literal evidence {}) | {} | {} | Unknown, no audited implementation binding | No, no structured behavior receipt |\n",row["source_id"].as_str().unwrap_or(""),row["index"],row["priority"],row["scene"].as_str().unwrap_or(""),row["source_id"].as_str().unwrap_or(""),write["authored_state"].as_str().unwrap_or(""),write["field"],write["value"],write["field_value_from"],write["literal_evidence_reason"],row["source_sha256"].as_str().unwrap_or(""),row["graph_file_sha256"].as_str().unwrap_or("")));
        }
        markdown.push_str("\nPriority: walljump progression first, then fragment acquisition, Godfinder, journal mask and grub pin. This ranks audit work, not proven progression blocking. Each JSON guard/trace retains the authored choice and stop reason. Automatic original SaveGame actions remain user-excluded and are never queued as missing implementation.\n");
        std::fs::write(out.join("authored-grant-overlays.md"), markdown)?;
    }
    std::fs::write(
        out.join("semantic-summary.json"),
        serde_json::to_vec_pretty(
            &json!({"counts_by_explicit_unit":counts,"semantic_census_complete":false,"unresolved_grants":unresolved,"validated_authored_graph_overlays":validated_overlays,"overlay_scope":"Additional authored selected write units only; existing runtime, implementation, journey and unresolved-grant dimensions are not promoted or removed.","coverage":"Typed scene and resource metadata, selected Shiny instance branches and explicit enabled PlayerData writes. External sharedasset prefabs, full grant reachability, enemy/boss aliases, localization, equipment catalogue and runtime/journey audits remain incomplete. Counts cannot be summed into a unique gameplay feature total."}),
        )?,
    )?;
    Ok(())
}
