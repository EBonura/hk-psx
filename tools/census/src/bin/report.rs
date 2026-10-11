//! Join exact original contracts to admitted cooked source IDs and reviewed runtime evidence.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};
#[path = "../catalogue.rs"]
mod catalogue;
fn load(p: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}
fn sources(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "source" {
                    if let Some(s) = x.as_str() {
                        out.insert(s.into());
                    }
                }
                sources(x, out)
            }
        }
        Value::Array(a) => {
            for x in a {
                sources(x, out)
            }
        }
        _ => {}
    }
}
fn strings(r: &Value) -> Vec<(String, String)> {
    r["strings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| Some((x[0].as_str()?.into(), x[1].as_str()?.into())))
        .collect()
}
fn valid_files(e: &Value, base: &Path, key: &str) -> bool {
    e[key].as_array().is_some_and(|items| {
        !items.is_empty()
            && items.iter().all(|v| {
                let Some(file) = v["file"].as_str() else {
                    return false;
                };
                let Ok(bytes) = std::fs::read(base.join(file)) else {
                    return false;
                };
                if v["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
                    return false;
                }
                v["exact_text"].as_str().is_some_and(|needle| {
                    !needle.is_empty() && String::from_utf8_lossy(&bytes).contains(needle)
                })
            })
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 5 {
        return Err(
            "usage: report ORIGINAL_OUTPUT COOKED_PORT_ROOT REPORT_OUTPUT AUDITED_REGISTRY_JSON"
                .into(),
        );
    }
    let original = Path::new(&a[1]);
    let port = Path::new(&a[2]);
    let out = Path::new(&a[3]);
    std::fs::create_dir_all(out)?;
    let manifest = load(&port.join("data/regions.json"))?;
    let summary = load(&original.join("source-summary.json"))?;
    let registry = load(Path::new(&a[4]))?;
    let audited: BTreeMap<String, Value> = registry["contracts"]
        .as_array()
        .ok_or("registry contracts absent")?
        .iter()
        .filter_map(|x| Some((x["sid"].as_str()?.to_string(), x.clone())))
        .collect();
    let mut admitted = BTreeSet::new();
    let mut evidence: BTreeMap<String, String> = BTreeMap::new();
    let dispatch=[("actors","game/src/enemies.rs: EnemyWorld::tick and shared/hk-sim source controllers; placement only, parity gaps retained"),("npcs","game/src/dialogue.rs: npc_tick; exact admitted NPC source only, conversation subset"),("breakables","game/src/main.rs: strike_props; cooked destruction may retain limitations"),("benches","game/src/bench.rs: State::tick"),("secrets","game/src/main.rs: reveal mask logic"),("pickups","game/src/pickups.rs: World::collect; source grants only"),("hazards","game/src/main.rs: hazard collision path")];
    for s in manifest["scenes"].as_array().unwrap() {
        admitted.insert(s["scene_name"].as_str().unwrap().to_string());
        for g in s["gates"].as_array().unwrap() {
            if let Some(id) = g["source"].as_str() {
                evidence.insert(id.into(),"game/src/scene_transition.rs: load; admitted source gate, destination admission checked separately".into());
            }
        }
    }
    for region in manifest["regions"].as_array().unwrap() {
        for (k, proof) in dispatch {
            let mut ids = BTreeSet::new();
            sources(&region[k], &mut ids);
            for id in ids {
                evidence.insert(id, proof.into());
            }
        }
    }
    let mut writer = std::io::BufWriter::new(std::fs::File::create(
        out.join("feature-contracts.jsonl.partial"),
    )?);
    let mut counts: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut types: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rows = 0;
    let mut queue = Vec::new();
    let mut unknown = 0;
    let role_components = [
        "TransitionPoint",
        "HealthManager",
        "BossSceneController",
        "RestBench",
        "Breakable",
        "PersistentBoolItem",
        "PersistentIntItem",
        "LookAnimNPC",
        "ShopMenuStock",
        "CharmItem",
        "InvCharmBackboard",
        "DreamPlant",
        "DreamPlantOrb",
        "Grub",
        "GeoControl",
        "ShinyItem",
        "SceneManager",
    ];
    for line in catalogue::Catalogue::open(original)? {
        let r: Value = serde_json::from_str(&line?)?;
        let component = r["component"].as_str().unwrap();
        if component != "PlayMakerFSM" && !role_components.contains(&component) {
            continue;
        }
        let ss = strings(&r);
        let component_set = r["components"].as_array().cloned().unwrap_or_default();
        let category = match component {
            "TransitionPoint" => "scene transition",
            "HealthManager" => {
                if component_set.iter().any(|x| x == "Breakable") {
                    "health-bearing prop"
                } else {
                    "health-bearing actor (enemy/boss role unresolved)"
                }
            }
            "BossSceneController" => "boss encounter controller (includes alternate arenas)",
            "RestBench" | "Breakable" | "PersistentBoolItem" | "PersistentIntItem"
            | "DreamPlant" | "DreamPlantOrb" => "interactable contract",
            "LookAnimNPC" | "ShopMenuStock" => "NPC/shop contract",
            "CharmItem" | "InvCharmBackboard" => "charm contract",
            "SceneManager" => "area/scene settings",
            _ => "FSM semantic review required",
        };
        let scene = r["scene"].as_str().unwrap();
        let sid = r["sid"].as_str().unwrap();
        let known = evidence.get(sid);
        let audit = audited.get(sid).filter(|e| {
            e["source_sha256"] == r["source_sha256"] && valid_files(e, port, "implementation")
        });
        let exclusions: Vec<_> = registry["policy_exclusions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["sid"] == r["sid"] && e["source_sha256"] == r["source_sha256"])
            .collect();
        // Exercise claims stay disabled until a structured proof validates source contract,
        // disc and journey hashes plus exact assertion PASS and excludes matching FAIL.
        let exercised = false;
        let state = if exercised {
            "implemented subset, journey proven"
        } else if audit.is_some() {
            "implemented subset, journey not proven"
        } else if !admitted.contains(scene) && known.is_none() {
            "missing scene admission"
        } else {
            "semantic review required"
        };
        if state == "semantic review required" {
            unknown += 1
        }
        let refs: BTreeSet<String> = ss
            .iter()
            .map(|(_, s)| s.clone())
            .filter(|s| {
                s.starts_with("gotCharm_")
                    || s.starts_with("equippedCharm_")
                    || [
                        "hasDash",
                        "hasWalljump",
                        "hasSuperDash",
                        "hasDoubleJump",
                        "hasShadowDash",
                        "hasDreamNail",
                        "hasDreamGate",
                        "hasAcidArmour",
                        "hasFireball",
                        "hasQuake",
                        "hasScream",
                        "hasLantern",
                        "hasTramPass",
                        "hasWhiteKey",
                        "hasLoveKey",
                        "hasKingsBrand",
                        "hasMap",
                        "hasRoyalCharm",
                        "hasGodfinder",
                    ]
                    .contains(&s.as_str())
            })
            .collect();
        let destinations: Vec<_> = ss
            .iter()
            .filter(|(p, _)| p == "/targetScene" || p == "/entryPoint")
            .collect();
        let row = json!({"category":category,"scene":scene,"sid":sid,"game_object":r["game_object_name"],"game_object_id":r["game_object_id"],"component":component,"source_sha256":r["source_sha256"],"state":state,"cooked_source_binding":known,"audited_runtime_evidence":audit,"journey_exercised":exercised,"journey_reason":if exercised{"Audited per-source-ID assertion, journey and receipt hashes validated"}else{"Exercise proof is disabled pending structured source-contract, disc, journey and exact assertion PASS/FAIL receipt validation; file text and hash matching alone are insufficient."},"playerdata_references":refs,"transition_target_fields":destinations,"scope_note":"A PlayerData reference may be a condition, not a grant. HealthManager may be a prop. Source type/name does not establish port behaviour. Separate source components on one GameObject are contract rows, not unique feature types."});
        let mut row = row;
        row["user_excluded_original_behaviour"] = serde_json::to_value(&exclusions)?;
        serde_json::to_writer(&mut writer, &row)?;
        writer.write_all(b"\n")?;
        let rank = match component {
            "TransitionPoint" => 10,
            "SceneManager" => 20,
            "BossSceneController" => 30,
            "HealthManager" => 40,
            "CharmItem" | "InvCharmBackboard" => 50,
            "LookAnimNPC" | "ShopMenuStock" => 60,
            "PlayMakerFSM" => 80,
            _ => 70,
        };
        queue.push(json!({"queue_id":format!("{sid}/{component}"),"priority":rank,"priority_reason":"Scene access and transitions first, then encounter controllers, actors, charms, NPCs and interactables; unclassified FSMs require semantic review. Priority is an audit order, not proven progression blocking.","contract":row}));
        rows += 1;
        *counts
            .entry(category.into())
            .or_default()
            .entry(state.into())
            .or_default() += 1;
        types.entry(category.into()).or_default().insert(format!(
            "{}:{}",
            component,
            r["game_object_name"].as_str().unwrap_or("")
        ));
    }
    writer.flush()?;
    std::fs::rename(
        out.join("feature-contracts.jsonl.partial"),
        out.join("feature-contracts.jsonl"),
    )?;
    queue.sort_by_key(|r| {
        (
            r["priority"].as_u64().unwrap(),
            r["queue_id"].as_str().unwrap().to_owned(),
        )
    });
    let mut queued = std::io::BufWriter::new(std::fs::File::create(
        out.join("missing-unexercised-queue.jsonl"),
    )?);
    let mut md = std::io::BufWriter::new(std::fs::File::create(
        out.join("MISSING-FEATURES-hk-psx.md"),
    )?);
    let missing = queue
        .iter()
        .filter(|q| q["contract"]["state"] == "missing scene admission")
        .count();
    let unexercised = queue
        .iter()
        .filter(|q| q["contract"]["state"] == "implemented subset, journey not proven")
        .count();
    let review = queue.len() - missing - unexercised;
    writeln!(md,"# Hollow Knight missing and unexercised source contracts\n\nMissing scene admission: {missing}. Implemented subset, not exercised: {unexercised}. Unknown semantic review: {review}. Total: {} source component contract instances, not unique gameplay features. Semantic census remains incomplete. Exercise claims are disabled pending structured assertion proof. Exact original hashes and validated runtime evidence appear in the adjacent JSONL queue.\n\n| Audit priority | Queue ID | Original scene and object | State | Port evidence | Journey |\n| --- | --- | --- | --- | --- | --- |",queue.len())?;
    let clean = |s: String| s.replace('|', "&#124;").replace('\n', " ");
    for q in &queue {
        serde_json::to_writer(&mut queued, q)?;
        queued.write_all(b"\n")?;
        let r = &q["contract"];
        let mut implementation = if let Some(audit) = r["audited_runtime_evidence"].as_object() {
            serde_json::to_string(&audit["implementation"])?
        } else {
            r["cooked_source_binding"]
                .as_str()
                .unwrap_or("No cooked source binding; implementation requires audit")
                .into()
        };
        if r["user_excluded_original_behaviour"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            implementation.push_str("; Intentional user exclusion, not a missing action: ");
            implementation.push_str(&serde_json::to_string(
                &r["user_excluded_original_behaviour"],
            )?);
        }
        writeln!(
            md,
            "| {} | {} | {} / {} | {} | {} | No: structured proof disabled |",
            q["priority"],
            clean(q["queue_id"].as_str().unwrap().into()),
            clean(r["scene"].as_str().unwrap().into()),
            clean(r["game_object"].as_str().unwrap_or("").into()),
            r["state"].as_str().unwrap(),
            clean(implementation)
        )?;
    }
    queued.flush()?;
    md.flush()?;
    let exercised_count: usize = counts
        .values()
        .map(|m| {
            m.get("implemented subset, journey proven")
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let result = json!({"original_scene_count":summary["original_scene_count"],"original_catalog_file_count":summary["scenes"].as_array().unwrap().len(),"original_decode_errors":summary["decode_errors"].as_array().unwrap().len(),"admitted_scene_count":admitted.len(),"contract_rows":rows,"counts":counts,"distinct_source_component_object_labels":types.iter().map(|(k,v)|(k,v.len())).collect::<BTreeMap<_,_>>(),"semantic_review_required_in_admitted_scenes":unknown,"journey_proven_contracts":exercised_count,"semantic_census_complete":false,"count_unit":"source component contract instances, never unique gameplay feature types"});
    std::fs::write(
        out.join("feature-summary.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
