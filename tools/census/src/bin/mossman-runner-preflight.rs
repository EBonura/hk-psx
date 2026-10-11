//! Bounded source and current-spec audit for five admitted-scene Mossman Runners.
//! This reports authored contracts; it never admits actors or proves runtime parity.
use hk_unity::{scene::Scene, Source, Value};
use serde_json::{json, Value as J};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};
const INSTANCES: &[(&str, &str, i64, i64, i64, i64, &str)] = &[
    (
        "Fungus1_01",
        "level128",
        46,
        736,
        7539,
        7923,
        "bc52daf81c9c7b5e80d200c4ac040568b71e4c7bb7a5de23410404bcf1f04ba1",
    ),
    (
        "Fungus1_05",
        "level134",
        49,
        110,
        6132,
        6465,
        "2f5b41e9538445c2a96d3276e407d5cb925ff21fd924d1ab6316603a0162294a",
    ),
    (
        "Fungus1_06",
        "level135",
        50,
        808,
        6450,
        6820,
        "7b4550c76ae19c24d3fa077acb76f501a90e4f58b383878d31e7c179322f49e7",
    ),
    (
        "Fungus1_07",
        "level136",
        51,
        510,
        5414,
        5743,
        "7a2d72b16986dd7c7f4510c10740d416ad7f442c484a5a8e2700bc014388564f",
    ),
    (
        "Fungus1_07",
        "level136",
        51,
        1082,
        5445,
        5742,
        "26d15b8d402f150547ded07e16fe3da377ceef9fd174d826e6d77e0fc0217e24",
    ),
];
fn typed(v: &Value) -> J {
    match v {
        Value::Bool(x) => json!(x),
        Value::Int(x) => json!(x),
        Value::UInt(x) => json!(x),
        Value::F32(x) => json!(x),
        Value::F64(x) => json!(x),
        Value::Str(x) => std::str::from_utf8(x)
            .map(|s| json!(s))
            .unwrap_or(json!({"unknown":"invalid UTF8"})),
        Value::Bytes(x) => json!({"byte_count":x.len(),"sha256":format!("{:x}",Sha256::digest(x))}),
        Value::List(x) => J::Array(x.iter().map(typed).collect()),
        Value::Map(x) => J::Object(x.iter().map(|(k, v)| (k.to_string(), typed(v))).collect()),
    }
}
fn run(original: &Path, cooked: &Path) -> Result<J, Box<dyn std::error::Error>> {
    let source = Source::new(original)?;
    let region_bytes = std::fs::read(cooked)?;
    let regions: J = serde_json::from_slice(&region_bytes)?;
    let settings = hk_unity::scene::build_settings(&source)?;
    let mut result = Vec::new();
    for &(name, file, scene_id, gid, fid, spec_id, expected) in INSTANCES {
        if settings.get(name).map(String::as_str) != Some(file) {
            return Err(format!("original BuildSettings scene binding drift: {name}").into());
        }
        let scenes: Vec<_> = regions["scenes"]
            .as_array()
            .ok_or("scene table missing")?
            .iter()
            .filter(|s| s["scene_id"].as_i64() == Some(scene_id))
            .collect();
        if scenes.len() != 1
            || scenes[0]["scene_name"].as_str() != Some(name)
            || scenes[0]["file"].as_str() != Some(file)
        {
            return Err(format!("current scene ID/name/file binding drift: {scene_id}").into());
        }
        let asset = source.file(file)?;
        let original_fsm = source.object(&asset, fid)?;
        let fsm_tree = source.read(&original_fsm)?;
        if fsm_tree.get("m_GameObject").and_then(Value::pptr) != Some((0, gid)) {
            return Err("original movement FSM belongs to another GameObject".into());
        }
        let digest = format!("{:x}", Sha256::digest(original_fsm.raw()?));
        if digest != expected {
            return Err(format!("original FSM drift {}", original_fsm.sid()).into());
        }
        let scene = Scene::new(&source, file)?;
        let game_object = source.object(&asset, gid)?;
        let game_object_digest = format!("{:x}", Sha256::digest(game_object.raw()?));
        let health = scene
            .object(spec_id)
            .ok_or("canonical health record absent")?;
        if health.typename != "HealthManager"
            || health.tree.get("m_GameObject").and_then(Value::pptr) != Some((0, gid))
        {
            return Err("canonical health record does not bind this original object".into());
        }
        let mut components = Vec::new();
        let mut libraries = Vec::new();
        for record in &scene.objects {
            if record.tree.get("m_GameObject").and_then(Value::pptr) != Some((0, gid)) {
                continue;
            }
            if ![
                "Walker",
                "HealthManager",
                "Recoil",
                "Rigidbody2D",
                "FSMActivator",
                "LineOfSightDetector",
                "tk2dSpriteAnimator",
                "DamageHero",
                "PlayMakerFSM",
                "Transform",
            ]
            .contains(&record.typename.as_str())
            {
                continue;
            }
            let object = source.object(&asset, record.id)?;
            components.push(json!({"source_id":object.sid(),"raw_sha256":format!("{:x}",Sha256::digest(object.raw()?)),"component":record.typename,"fields":if record.typename=="PlayMakerFSM" {json!({"enabled":record.tree.get("m_Enabled").map(typed),"fsm_name":record.tree.get("fsm").and_then(|f|f.get("name")).map(typed)})}else{typed(&record.tree)}}));
            if record.typename == "tk2dSpriteAnimator" {
                let library = scene.deref(
                    record
                        .tree
                        .get("library")
                        .ok_or("animator library absent")?,
                )?;
                let tree = source.read(&library)?;
                let clips = tree
                    .get("clips")
                    .and_then(Value::list)
                    .ok_or("library clips absent")?;
                libraries.push(json!({"source_id":library.sid(),"raw_sha256":format!("{:x}",Sha256::digest(library.raw()?)),"clips":clips.iter().map(|c|json!({"name":c.get("name").map(typed),"fps":c.get("fps").map(typed),"wrap_mode":c.get("wrapMode").map(typed),"frames":c.get("frames").and_then(Value::list).map(|f|f.len())})).collect::<Vec<_>>()}));
            }
        }
        let mut port = Vec::new();
        let mut seen = BTreeSet::new();
        for region in regions["regions"]
            .as_array()
            .ok_or("cooked regions missing")?
        {
            if region["scene_id"].as_i64() != Some(scene_id) {
                continue;
            }
            if region["scene_name"].as_str() != Some(name)
                || region["scene_file"].as_str() != Some(file)
            {
                return Err("current region scene name/file differs from scene table".into());
            }
            for actor in region["actors"].as_array().ok_or("region actors missing")? {
                if actor["spec_source_id"].as_i64() == Some(spec_id) {
                    if actor["game_object"].as_i64() != Some(gid) {
                        return Err("canonical spec maps to another original object".into());
                    }
                    seen.insert(
                        actor["source"]
                            .as_str()
                            .ok_or("actor source missing")?
                            .to_string(),
                    );
                    let bank = match region["actor_bank_bytes"].as_u64() {
                        Some(bytes) => {
                            json!({"status":"reported existing cook measurement","bytes":bytes,"reason":"actor_bank_bytes field in hashed cooked regions manifest; no new family art cooked"})
                        }
                        None => {
                            json!({"status":"unknown","bytes":null,"reason":"actor_bank_bytes absent or not a nonnegative integer in cooked manifest"})
                        }
                    };
                    port.push(json!({"chunk_id":region["chunk_id"],"source":actor["source"],"spec_source_id":actor["spec_source_id"],"position":actor["position"],"health":actor["health"],"movement_supported":actor["movement_supported"],"pending_movement_error":actor["pending_movement_error"],"current_actor_bank_evidence":bank}));
                }
            }
        }
        if seen != BTreeSet::from([format!("{file}:{spec_id}")]) {
            return Err(format!("expected exactly one canonical spec for {file}:{spec_id}").into());
        }
        result.push(json!({"scene":name,"scene_id":scene_id,"game_object_source":game_object.sid(),"game_object_raw_sha256":game_object_digest,"fsm_source":original_fsm.sid(),"fsm_raw_sha256":digest,"canonical_spec_source":format!("{file}:{spec_id}"),"components":components,"art_libraries":libraries,"current_port":port,"runtime_parity":"unknown","journey_exercised":false,"new_bank_growth_bytes":null,"new_code_growth_bytes":null,"new_static_ram_growth_bytes":null}));
    }
    Ok(
        json!({"scope":"Five selected admitted-scene instances; Fungus1_31 and other families excluded, not a full enemy census.","semantic_census_complete":false,"cook_performed":false,"cooked_regions_sha256":format!("{:x}",Sha256::digest(&region_bytes)),"instances":result}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "usage: mossman-runner-preflight ORIGINAL_DATA COOKED_REGIONS_JSON OUTPUT_JSON".into(),
        );
    }
    let report = run(Path::new(&args[1]), Path::new(&args[2]))?;
    let target = Path::new(&args[3]);
    let next = target.with_extension("json.next");
    std::fs::write(&next, serde_json::to_vec_pretty(&report)?)?;
    std::fs::rename(next, target)?;
    Ok(())
}
