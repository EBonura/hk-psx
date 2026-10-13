mod vengefly;
pub use vengefly::cook_vengeflies;
mod cook;
pub use cook::cook;
pub use cook::crawler_fields;
/// Regenerate the original Gruzzer graph used by the guest.
pub fn cook_gruzzer(root:&std::path::Path)->Result<(),Box<dyn std::error::Error>> {
 let source=hk_unity::Source::from_doctor(root)?;
 let file=source.file("level43")?;
 let object=source.object(&file,6396)?;
 let tree=source.read(&object)?;
 let bytes=cook(tree.get("fsm").ok_or("original Gruzzer FSM")?)?;
 let groups=hk_format::fsm::Program::parse(&bytes).map_err(|e|format!("Gruzzer graph {e:?}"))?.action_groups().map_err(|e|format!("Gruzzer groups {e:?}"))?;
 if groups!=(hk_format::fsm::GROUP_CORE|hk_format::fsm::GROUP_FLY){return Err("Gruzzer action residency changed".into());}
 std::fs::create_dir_all(root.join("data"))?;
 std::fs::write(root.join("data/gruzzer.fsm"),bytes)?;
 Ok(())
}
/// Preserve every admitted Crawler's own serialized graph and parameters.
pub fn cook_crawlers(root:&std::path::Path)->Result<(),Box<dyn std::error::Error>>{
    use std::{collections::BTreeMap,fmt::Write};
    let source=hk_unity::Source::from_doctor(root)?;
    let report:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("data/regions.json"))?)?;
    let mut selected=BTreeMap::new();
    for region in report["regions"].as_array().ok_or("regions")?{
        let scene=u16::try_from(region["scene_id"].as_u64().ok_or("scene")?)?;
        for actor in region["actors"].as_array().ok_or("actors")?{
            if actor["movement_control"]["kind"].as_str()!=Some("WalkLeftRight"){continue;}
            let owner=actor["game_object"].as_i64().ok_or("owner")?;
            let id=u32::try_from(actor["spec_source_id"].as_u64().or_else(||actor["source"].as_str()?.rsplit_once(':')?.1.parse().ok()).ok_or("source ID")?)?;
            let fsm=actor["movement_control"]["fsm"].as_str().ok_or("FSM source")?.to_owned();
            if let Some(previous)=selected.insert((scene,id),(owner,fsm.clone())){if previous!=(owner,fsm){return Err("conflicting Crawler aliases".into());}}
        }
    }
    if selected.len()!=10{return Err("admitted Crawler placement set changed".into());}
    let mut banks:Vec<Vec<u8>>=vec![];let mut offsets=vec![];let mut bundle=vec![];let mut rows=String::new();
    for ((scene,id),(owner,sid)) in selected{
        let (file,path)=sid.rsplit_once(':').ok_or("FSM ID")?;let file=source.file(file)?;
        let object=source.object(&file,path.parse()?)?;let tree=source.read(&object)?;
        if tree.get("m_GameObject").and_then(|v|v.get("m_PathID")).and_then(hk_unity::Value::int)!=Some(owner){return Err("Crawler FSM owner mismatch".into());}
        let fsm=tree.get("fsm").ok_or("FSM")?;
        if fsm.get("name").and_then(hk_unity::Value::str).as_deref()!=Some("Crawler"){return Err("Crawler graph name".into());}
        let bytes=cook(fsm)?;
        let groups=hk_format::fsm::Program::parse(&bytes).map_err(|e|format!("Crawler graph {e:?}"))?.action_groups().map_err(|e|format!("Crawler groups {e:?}"))?;
        if groups!=(hk_format::fsm::GROUP_CORE|hk_format::fsm::GROUP_WALK){return Err("Crawler requires an unadmitted action group".into());}
        let index=if let Some(i)=banks.iter().position(|b|*b==bytes){i}else{
            let i=banks.len();offsets.push(u16::try_from(bundle.len())?);bundle.extend_from_slice(&bytes);banks.push(bytes);i
        };
        writeln!(&mut rows,"({scene},{id},{},{}),",offsets[index],banks[index].len())?;
    }
    let table=format!("// Original Crawler graphs, deduplicated only after exact byte comparison.\npub static BANK:[u8;{}]=*include_bytes!(\"crawler.fsm\");\npub static GRAPHS:&[(u16,u32,u16,u16)]=&[\n{rows}];\n",bundle.len());
    std::fs::write(root.join("data/crawler.fsm"),&bundle)?;
    std::fs::write(root.join("data/crawler_fsm.rs"),table)?;
    println!("Crawler: 10 placements, {} distinct original graphs, {} bank bytes",banks.len(),bundle.len());
    Ok(())
}

#[cfg(test)]
mod residency_tests {
    #[test]
    fn retained_gruzzer_graph_uses_only_core_and_flying_actions() {
        let graph=include_bytes!("../../../data/gruzzer.fsm");
        let program=hk_format::fsm::Program::parse(graph).unwrap();
        assert_eq!(program.action_groups().unwrap(),hk_format::fsm::GROUP_CORE|hk_format::fsm::GROUP_FLY);
    }
}

#[cfg(test)]
mod vengefly_original_tests {
    #[test]
    #[ignore = "requires privately owned original installation"]
    fn every_admitted_vengefly_component_decodes_with_its_own_variables() {
        use hk_unity::{Source,Value};
        let root=std::path::PathBuf::from(std::env::var("HK_ORIGINAL_ROOT").expect("private original root"));
        let source=Source::from_doctor(&root).unwrap();
        let metadata:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("data/regions.json")).unwrap()).unwrap();
        let mut ids=std::collections::BTreeSet::new();
        for region in metadata["regions"].as_array().unwrap(){for actor in region["actors"].as_array().unwrap(){
            if actor["movement_control"]["kind"]=="Vengefly"{
                for id in actor["fsm_ids"].as_array().unwrap(){ids.insert(id.as_str().unwrap().to_owned());}
            }
        }}
        assert_eq!(ids.len(),40);let mut failures=Vec::new();
        for id in ids{
            let (file,path)=id.rsplit_once(':').unwrap();let file=source.file(file).unwrap();
            let object=source.object(&file,path.parse().unwrap()).unwrap();let tree=source.read(&object).unwrap();
            let fsm=tree.get("fsm").unwrap();
            match super::cook(fsm){
                Err(error)=>failures.push(format!("{id}: {error}")),
                Ok(bytes)=>{
                    let p=hk_format::fsm::Program::parse(&bytes).unwrap();
                    assert_eq!(p.action_groups().unwrap(),hk_format::fsm::GROUP_CORE|hk_format::fsm::GROUP_CHASE,"{id}");
                    let name=fsm.get("name").and_then(Value::str).unwrap();
                    assert_eq!(p.variable_count(),if name=="chaser"{8}else{3},"{id}");
                }
            }
        }
        assert!(failures.is_empty(),"{}",failures.join("\n"));
        // Original component ordering, enablement and broadcast descendants
        // are additionally checked by the same production admission entry.
        super::cook_vengeflies(&root).unwrap();
    }
}
