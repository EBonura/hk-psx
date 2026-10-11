//! Read-only original-scene catalogue. Generated source records stay outside Git.
use hk_unity::scene::build_settings;
use hk_unity::{Obj, Source, Value};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufWriter, Write},
    path::PathBuf,
};
mod grants;
fn strings(v: &Value, p: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Str(s) => {
            let s = String::from_utf8_lossy(s);
            if !s.is_empty() {
                out.push((p.into(), s.into_owned()));
            }
        }
        Value::Map(m) => {
            for (k, x) in m {
                strings(x, &format!("{p}/{k}"), out)
            }
        }
        Value::List(l) => {
            for (i, x) in l.iter().enumerate() {
                strings(x, &format!("{p}/{i}"), out)
            }
        }
        _ => {}
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: hk-feature-census ORIGINAL_WINDOWS_DATA OUTPUT_DIR".into());
    }
    let source = Source::new(PathBuf::from(&args[1]))?;
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&out)?;
    let mut writer = BufWriter::new(std::fs::File::create(out.join("source-objects.jsonl"))?);
    let mut scenes: BTreeMap<_, _> = build_settings(&source)?
        .iter()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();
    let original_scene_count = scenes.len();
    scenes.insert(
        "@resources inventory and actor prefabs".into(),
        "resources.assets".into(),
    );
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut scene_rows = Vec::new();
    for (scene, level) in &scenes {
        eprintln!("{scene} {level}");
        let file = source.file(level)?;
        let mut records = Vec::new();
        let mut gos = BTreeMap::new();
        for info in &file.objects {
            if info.class_id == 43 {
                continue;
            }
            let obj = Obj {
                file: file.clone(),
                info: *info,
            };
            let kind = match source.typename(&obj) {
                Ok(k) => k,
                Err(e) => {
                    errors.push(
                        json!({"scene":scene,"sid":obj.sid(),"phase":"type","error":e.to_string()}),
                    );
                    continue;
                }
            };
            match source.read(&obj){Ok(tree)=>{if kind=="GameObject"{gos.insert(obj.path_id(),tree.get("m_Name").and_then(Value::str).unwrap_or_default());}records.push((obj,kind,tree));},Err(e)=>errors.push(json!({"scene":scene,"sid":obj.sid(),"phase":"decode","kind":kind,"error":e.to_string()}))}
        }
        let mut components: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
        for (_, kind, tree) in &records {
            if let Some(g) = tree
                .get("m_GameObject")
                .and_then(|x| x.get("m_PathID"))
                .and_then(Value::int)
            {
                components.entry(g).or_default().insert(kind.clone());
            }
        }
        let mut n = 0;
        for (obj, kind, tree) in records {
            if obj.class_id() != 114 && kind != "GameObject" {
                continue;
            }
            let gid = tree
                .get("m_GameObject")
                .and_then(|x| x.get("m_PathID"))
                .and_then(Value::int)
                .unwrap_or(obj.path_id());
            let mut fields = Vec::new();
            strings(&tree, "", &mut fields);
            *counts.entry(kind.clone()).or_default() += 1;
            let fsm = tree.get("fsm");
            let grant = fsm.map(grants::shiny).transpose();
            let (shiny_grant, grant_error) = match grant {
                Ok(value) => (value.flatten(), None),
                Err(e) => (None, Some(e)),
            };
            let pd_writes = fsm.map(grants::pd_writes).unwrap_or_default();
            let scalars: BTreeMap<_, _> = [
                "charmNum",
                "hp",
                "enemyType",
                "m_IsActive",
                "mapZone",
                "environmentType",
            ]
            .iter()
            .filter_map(|name| {
                tree.get(name)
                    .map(|v| (*name, grants::scalar(v, &BTreeMap::new())))
            })
            .collect();
            let row = json!({"scene":scene,"level":level,"sid":obj.sid(),"game_object_id":gid,"game_object_name":gos.get(&gid),"component":kind,"components":components.get(&gid),"source_sha256":format!("{:x}",Sha256::digest(obj.raw()?)),"strings":fields,"scalars":scalars,"fsm_name":fsm.and_then(|f|f.get("name")).and_then(Value::str),"shiny_grant":shiny_grant,"grant_error":grant_error,"playerdata_writes":pd_writes});
            serde_json::to_writer(&mut writer, &row)?;
            writer.write_all(b"\n")?;
            n += 1;
        }
        scene_rows.push(
            json!({"scene":scene,"level":level,"objects":file.objects.len(),"contract_records":n}),
        );
        writer.flush()?;
    }
    let summary = json!({"unity_version":hk_unity::UNITY_VERSION,"source":args[1],"original_scene_count":original_scene_count,"scenes":scene_rows,"component_counts":counts,"decode_errors":errors,"scope":"Every BuildSettings scene file is read separately, including additive scenes and inactive objects, plus resources.assets inventory/actor prefabs. Records enumerate all GameObjects and MonoBehaviours; role and implementation evidence are separate. Decode errors prevent an exhaustive semantic claim."});
    std::fs::write(
        out.join("source-summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(())
}
