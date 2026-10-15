//! Cook the HKFSM03 banks (`cook-all`, or one of `cook-crawlers3`, `cook-gruzzer3`, `cook-vengeflies3`,
//! `cook-scripts3`, each over the repository root) or print one original FSM (`inspect DATA FILE:PATH_ID`).
use hk_fsm_cook::cook3;
use hk_unity::{playmaker::enabled, Source, Value};
type Res = Result<(), Box<dyn std::error::Error>>;
fn main() -> Res {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 3 {
        let root = std::path::Path::new(&args[2]);
        let done = match args[1].as_str() {
            "cook-all" => Some(cook3::cook_all(root)),
            "cook-crawlers3" => Some(cook3::cook_crawlers3(root)),
            "cook-gruzzer3" => Some(cook3::cook_gruzzer3(root)),
            "cook-vengeflies3" => Some(cook3::cook_vengeflies3(root)),
            "cook-scripts3" => Some(cook3::cook_scripts3(root)),
            _ => None,
        };
        if let Some(result) = done {
            return result.map_err(|e| e.to_string().into());
        }
    }
    if args.len() != 4 || args[1] != "inspect" {
        return Err("usage: hk-fsm-cook cook-all|cook-crawlers3|cook-gruzzer3|cook-vengeflies3|cook-scripts3 ROOT | inspect DATA FILE:PATH_ID".into());
    }
    let source = Source::new(&args[2])?;
    let (file, id) = args[3].rsplit_once(':').ok_or("source ID")?;
    let file = source.file(file)?;
    let object = source.object(&file, id.parse()?)?;
    let tree = source.read(&object)?;
    let fsm = tree.get("fsm").ok_or("FSM")?;
    println!("FSM {:?} START {:?} GLOBAL {:?}", fsm.get("name"), fsm.get("startState"), fsm.get("globalTransitions"));
    println!("VARIABLES {:?}", fsm.get("variables"));
    for state in fsm.get("states").and_then(Value::list).ok_or("states")? {
        let data = state.get("actionData").ok_or("action data")?;
        let list = |name: &str| -> Result<&[Value], Box<dyn std::error::Error>> { data.get(name).and_then(Value::list).ok_or_else(|| format!("missing {name}").into()) };
        let starts = list("actionStartIndex")?;
        let names = list("actionNames")?;
        let params = list("paramName")?;
        let kinds = list("paramDataType")?;
        let positions = list("paramDataPos")?;
        let sizes = list("paramByteDataSize")?;
        for (_, index) in enabled(state) {
            println!("STATE {:?} ACTION {:?}", state.get("name"), names[index]);
            let start = starts[index].int().ok_or("start")? as usize;
            let end = if index + 1 < names.len() { starts[index + 1].int().ok_or("end")? as usize } else { params.len() };
            for p in start..end {
                println!("  PARAM{p} name={:?} kind={:?} pos={:?} size={:?}", params[p], kinds.get(p), positions.get(p), sizes.get(p));
            }
            for (name, value) in hk_unity::playmaker::action_fields_with_plain_strings(data, index, true)? {
                println!("  FIELD {name}={value:?}");
            }
        }
    }
    Ok(())
}
