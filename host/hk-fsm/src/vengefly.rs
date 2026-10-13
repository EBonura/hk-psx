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
            let body=&actor["Rigidbody2D"];
            if body["m_BodyType"].as_i64()!=Some(0)||body["m_Mass"].as_f64()!=Some(1.0)||body["m_LinearDamping"].as_f64()!=Some(0.0)||body["m_GravityScale"].as_f64()!=Some(0.0)||body["m_Constraints"].as_i64()!=Some(4){return Err("Vengefly fixed physics body outside admitted subset".into());}
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
        let owner_file_name=graphs[0].rsplit_once(':').ok_or("FSM identity")?.0;
        let owner_file=source.file(owner_file_name)?;
        let owner_object=source.object(&owner_file,owner)?;
        let owner_tree=source.read(&owner_object)?;
        let mut source_order=Vec::new();
        for reference in owner_tree.get("m_Component").and_then(Value::list).ok_or("owner component order")? {
            let reference=reference.get("component").ok_or("component reference")?;
            if reference.get("m_FileID").and_then(Value::int)!=Some(0){return Err("external owner component unsupported".into());}
            let path=reference.get("m_PathID").and_then(Value::int).ok_or("component path")?;
            let object=source.object(&owner_file,path)?;
            if object.class_id()==114 && source.typename(&object)?=="PlayMakerFSM" {
                source_order.push(format!("{owner_file_name}:{path}"));
            }
        }
        if source_order!=graphs{return Err("Vengefly metadata differs from original component order".into());}
        // Notify Self broadcasts into descendants too. Admit the paired
        // executor only when those descendants add no original FSM recipient.
        let mut pending=Vec::new();
        for reference in owner_tree.get("m_Component").and_then(Value::list).ok_or("owner components")?{
            let path=reference.get("component").and_then(|v|v.get("m_PathID")).and_then(Value::int).ok_or("component path")?;
            let object=source.object(&owner_file,path)?;
            if object.class_id()==4{
                let transform=source.read(&object)?;
                for child in transform.get("m_Children").and_then(Value::list).ok_or("transform children")?{
                    if child.get("m_FileID").and_then(Value::int)!=Some(0){return Err("external child transform unsupported".into());}
                    pending.push(child.get("m_PathID").and_then(Value::int).ok_or("child transform path")?);
                }
            }
        }
        let mut visited=std::collections::BTreeSet::new();
        while let Some(path)=pending.pop(){
            if !visited.insert(path)||visited.len()>64{return Err("Vengefly child hierarchy unsupported".into());}
            let transform=source.read(&source.object(&owner_file,path)?)?;
            let reference=transform.get("m_GameObject").ok_or("child owner")?;
            if reference.get("m_FileID").and_then(Value::int)!=Some(0){return Err("external child owner unsupported".into());}
            let child_owner=source.read(&source.object(&owner_file,reference.get("m_PathID").and_then(Value::int).ok_or("child owner path")?)?)?;
            for reference in child_owner.get("m_Component").and_then(Value::list).ok_or("child components")?{
                let reference=reference.get("component").ok_or("child component")?;
                if reference.get("m_FileID").and_then(Value::int)!=Some(0){return Err("external child component unsupported".into());}
                let object=source.object(&owner_file,reference.get("m_PathID").and_then(Value::int).ok_or("child component path")?)?;
                if object.class_id()==114&&source.typename(&object)?=="PlayMakerFSM"{return Err("Vengefly descendant FSM recipient unsupported".into());}
            }
            for child in transform.get("m_Children").and_then(Value::list).ok_or("child transform children")?{
                if child.get("m_FileID").and_then(Value::int)!=Some(0){return Err("external child transform unsupported".into());}
                pending.push(child.get("m_PathID").and_then(Value::int).ok_or("child transform path")?);
            }
        }

        let mut named = BTreeMap::new();
        let mut ordered = Vec::new();
        for sid in graphs {
            let (file, path) = sid.rsplit_once(':').ok_or("FSM identity")?;
            let file = source.file(file)?;
            let object = source.object(&file, path.parse()?)?;
            let tree = source.read(&object)?;
            if tree.get("m_GameObject").and_then(|v|v.get("m_PathID")).and_then(Value::int) != Some(owner) {
                return Err("Vengefly FSM owner mismatch".into());
            }
            let enabled=tree.get("m_Enabled").and_then(Value::int).ok_or("FSM enablement")?;
            if enabled!=0&&enabled!=1{return Err("FSM enablement value".into());}
            let fsm = tree.get("fsm").ok_or("FSM")?;
            let name = fsm.get("name").and_then(Value::str).ok_or("FSM name")?;
            if !["chaser", "flyer_receive_direction_msg"].contains(&name.as_str()) || named.contains_key(&name) {
                return Err("unexpected Vengefly FSM graph set".into());
            }
            // The original broadcast DIR MSG has no recipient transition in
            // this admitted pair. Never reinterpret it as HealthManager's
            // independent TOOK DAMAGE event.
            for state in fsm.get("states").and_then(Value::list).ok_or("FSM states")?{
                for edge in state.get("transitions").and_then(Value::list).ok_or("state transitions")?{
                    if edge.get("fsmEvent").and_then(|v|v.get("name")).and_then(Value::str).as_deref()==Some("DIR MSG"){return Err("new DIR MSG recipient transition unsupported".into());}
                }
            }
            for edge in fsm.get("globalTransitions").and_then(Value::list).ok_or("global transitions")?{
                if edge.get("fsmEvent").and_then(|v|v.get("name")).and_then(Value::str).as_deref()==Some("DIR MSG"){return Err("new global DIR MSG recipient unsupported".into());}
            }
            let bytes = crate::cook(fsm)?;
            let index = if let Some(index) = banks.iter().position(|bank| bank == &bytes) { index }
                else { let index = banks.len(); banks.push(bytes); index };
            ordered.push((index,enabled!=0));
            named.insert(name, index);
        }
        if !named.contains_key("chaser")||!named.contains_key("flyer_receive_direction_msg") {
            return Err("Vengefly component graph missing".into());
        }
        table.push((scene, id, ordered));
    }
    // Publication remains unavailable until every enabled action is admitted.
    // A following integration adds runtime scheduling and the exact bank layout.
    println!("Vengefly: {} placements, {} original graph banks admitted", table.len(), banks.len());
    Ok(())
}

/// Decode the additional compact vector/object forms used by these two graphs.
/// This stays local to Vengefly admission; existing Unity/cook fingerprints do
/// not change and unknown parameter forms remain an error.
pub fn action_fields(data:&Value,index:usize)->Result<hk_unity::playmaker::Fields> {
    use hk_unity::playmaker::action_fields_with_plain_strings;
    let mut fields=action_fields_with_plain_strings(data,index,true)?;
    let list=|key|data.get(key).and_then(Value::list).ok_or("Vengefly parameter list");
    let starts=list("actionStartIndex")?;let names=list("actionNames")?;
    let parameters=list("paramName")?;let kinds=list("paramDataType")?;
    let positions=list("paramDataPos")?;let sizes=list("paramByteDataSize")?;
    if [kinds.len(),positions.len(),sizes.len()].iter().any(|n|*n!=parameters.len()) {return Err("Vengefly parameter lengths".into());}
    let start=usize::try_from(starts.get(index).and_then(Value::int).ok_or("parameter start")?)?;
    let end=if index+1<names.len(){usize::try_from(starts.get(index+1).and_then(Value::int).ok_or("parameter end")?)?}else{parameters.len()};
    if start>end||end>parameters.len(){return Err("Vengefly parameter bounds".into());}
    for at in start..end {
        let name=parameters[at].str().filter(|s|!s.is_empty()).unwrap_or_else(||at.to_string());
        let kind=kinds[at].int().ok_or("parameter kind")?;
        let pos=usize::try_from(positions[at].int().ok_or("parameter position")?)?;
        let size=usize::try_from(sizes[at].int().ok_or("parameter size")?)?;
        let value=match kind {
            5|24 if size==0=>{
                let key=if kind==5{"unityObjectParams"}else{"fsmObjectParams"};
                list(key)?.get(pos).cloned().ok_or("object parameter index")?
            }
            7 if size==4=>{
                let bytes=list("byteData")?.get(pos..pos.checked_add(4).ok_or("enum overflow")?).ok_or("enum bounds")?;
                let mut raw=[0;4];for (i,v) in bytes.iter().enumerate(){raw[i]=u8::try_from(v.int().ok_or("enum byte")?)?;}
                Value::Int(i32::from_le_bytes(raw) as i64)
            }
            28|35|37=>{
                let coordinates: &[&str]=match kind{28=>&["x","y","z"],35=>&["x","y","z","w"],_=>&["x","y"]};
                let scalar_bytes=coordinates.len()*4;
                if size<scalar_bytes+1{return Err("compact vector/quaternion size".into());}
                let bytes=list("byteData")?.get(pos..pos.checked_add(size).ok_or("vector overflow")?).ok_or("vector bounds")?;
                let mut raw=Vec::with_capacity(size);
                for v in bytes{raw.push(u8::try_from(v.int().ok_or("vector byte")?)?);}
                if raw[scalar_bytes]>1{return Err("compact vector variable flag".into());}
                let variable_name=std::str::from_utf8(&raw[scalar_bytes+1..])?.as_bytes().to_vec();
                let mut values=Vec::new();
                for (i,name) in coordinates.iter().enumerate(){
                    let value=f32::from_le_bytes(raw[i*4..i*4+4].try_into()?);
                    if !value.is_finite(){return Err("nonfinite compact vector".into());}
                    values.push(((*name).into(),Value::F32(value)));
                }
                Value::Map(vec![("value".into(),Value::Map(values)),("useVariable".into(),Value::Bool(raw[scalar_bytes]!=0)),("name".into(),Value::Str(variable_name))])
            }
            1|3|12|15|16|17|18|19|20|23|31=>continue,
            _=>return Err(format!("unsupported Vengefly parameter type {kind}/{size}").into()),
        };
        fields.retain(|(n,_)|*n!=name);fields.push((name,value));
    }
    Ok(fields)
}
