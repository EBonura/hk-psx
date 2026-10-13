//! Original FSM cooker input decoding. Unsupported parameter forms stay explicit.
use hk_fsm_cook::cook;
use hk_unity::{Source, Value, playmaker::enabled};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len()==3&&args[1]=="cook-crawlers"{return hk_fsm_cook::cook_crawlers(std::path::Path::new(&args[2]));}
    if (args.len() != 4 || args[1] != "inspect") && (args.len() != 5 || args[1] != "cook") {
        return Err("usage: hk-fsm-cook inspect DATA FILE:PATH_ID | cook DATA FILE:PATH_ID OUTPUT".into());
    }
    let source = Source::new(&args[2])?;
    let (file, id) = args[3].rsplit_once(':').ok_or("source ID")?;
    let file = source.file(file)?;
    let object = source.object(&file, id.parse()?)?;
    let tree = source.read(&object)?;
    let fsm = tree.get("fsm").ok_or("FSM")?;
    if args[1] == "cook" {
        let bytes = cook(fsm)?;
        let p = hk_format::fsm::Program::parse(&bytes).unwrap();
        std::fs::write(&args[4], &bytes)?;
        println!("Cooked {} states, {} actions, {} variables, {} bytes",p.state_count(),p.action_count(),p.variable_count(),bytes.len());
        return Ok(());
    }
    println!("FSM {:?} START {:?} GLOBAL {:?}", fsm.get("name"), fsm.get("startState"), fsm.get("globalTransitions"));
    println!("VARIABLES {:?}", fsm.get("variables"));
    for state in fsm.get("states").and_then(Value::list).ok_or("states")? {
        let data = state.get("actionData").ok_or("action data")?;
        let list = |name: &str| -> Result<&[Value],Box<dyn std::error::Error>> {
            data.get(name).and_then(Value::list).ok_or_else(|| format!("missing {name}").into())
        };
        let starts = list("actionStartIndex")?;
        let names = list("actionNames")?;
        let params = list("paramName")?;
        let kinds = list("paramDataType")?;
        let positions = list("paramDataPos")?;
        let sizes = list("paramByteDataSize")?;
        if let Value::Map(fields)=data { for (name,value) in fields {if name.to_lowercase().contains("array") {println!("ARRAY {name}={value:?}");}} }
        for (_, index) in enabled(state) {
            println!("STATE {:?} ACTION {:?}", state.get("name"), names[index]);
            let start = starts[index].int().ok_or("start")? as usize;
            let end = if index + 1 < names.len() { starts[index + 1].int().ok_or("end")? as usize } else { params.len() };
            for p in start..end {
                println!("  PARAM{p} name={:?} kind={:?} pos={:?} size={:?}", params[p], kinds.get(p), positions.get(p), sizes.get(p));
            }
            for (name, value) in hk_fsm_cook::crawler_fields(data, index)? {
                println!("  FIELD {name}={value:?}");
            }
        }
    }
    Ok(())
}
