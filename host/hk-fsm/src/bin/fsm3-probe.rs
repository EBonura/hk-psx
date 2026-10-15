use hk_fsm_cook::census::load_roots;
use hk_unity::{Source, Value};
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let a: Vec<_> = std::env::args().collect();
    let source = Source::new(&a[1])?;
    let roots = load_roots(&source, std::path::Path::new(&a[2]), 4)?;
    let r = roots.iter().find(|r| r.scene == a[3] && r.name == a[4]).ok_or("root")?;
    let want = &a[5];
    let mut shown = std::collections::HashSet::new();
    for (_, sid, _) in &r.fsms {
        let (f, p) = sid.rsplit_once(':').unwrap();
        let file = source.file(f)?;
        let obj = source.object(&file, p.parse()?)?;
        let tree = source.read(&obj)?;
        let fsm = tree.get("fsm").unwrap();
        for st in fsm.get("states").and_then(Value::list).unwrap() {
            let d = st.get("actionData").unwrap();
            let names = d.get("actionNames").and_then(Value::list).unwrap();
            let starts = d.get("actionStartIndex").and_then(Value::list).unwrap();
            let pn = d.get("paramName").and_then(Value::list).unwrap();
            let pk = d.get("paramDataType").and_then(Value::list).unwrap();
            let pp = d.get("paramDataPos").and_then(Value::list).unwrap();
            let ps = d.get("paramByteDataSize").and_then(Value::list).unwrap();
            for i in 0..names.len() {
                let n = names[i].str().unwrap();
                if !n.ends_with(want.as_str()) || !shown.insert(n.clone()) { continue; }
                let s = starts[i].int().unwrap() as usize;
                let e = if i + 1 < names.len() { starts[i + 1].int().unwrap() as usize } else { pn.len() };
                println!("ACTION {n}");
                for j in s..e { println!("  {:?} kind={} pos={} size={}", pn[j].str().unwrap(), pk[j].int().unwrap(), pp[j].int().unwrap(), ps[j].int().unwrap()); }
                for k in ["fsmEventTargetParams","functionCallParams","fsmVarParams","unityObjectParams","fsmObjectParams","fsmEnumParams","arrayParamSizes","arrayParamTypes"] {
                    if let Some(l) = d.get(k).and_then(Value::list) { if !l.is_empty() { println!("  {k}: {:?}", &l[..l.len().min(3)]); } }
                }
            }
        }
    }
    Ok(())
}
