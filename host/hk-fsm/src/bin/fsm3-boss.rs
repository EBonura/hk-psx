//! Cook a boss tree to `data/<label>.hkf3`, `data/<label>_fsm3.rs` and `data/<label>_names.txt`.
//!
//! fsm3-boss DATA CACHE ROOT hornet
//!
//! `hornet` is Greenpath's Hornet (Fungus1_04_boss): the fight's FSMs and the encounter's.
use hk_fsm_cook::boss::{cook_boss_tree, rust_table};
use hk_fsm_cook::census::{fsms_on, load_roots};
use hk_unity::{Source, Value};

type Res = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn main() -> Res {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 5 || a[4] != "hornet" {
        return Err("usage: fsm3-boss DATA CACHE ROOT hornet".into());
    }
    let source = Source::new(&a[1])?;
    let cache = std::path::Path::new(&a[2]);
    let root_dir = std::path::Path::new(&a[3]);
    let roots = load_roots(&source, cache, 4)?;
    let root = roots.iter().find(|r| r.scene == "Fungus1_04_boss" && r.name == "Hornet Boss 1").ok_or("Hornet root")?;
    // The encounter's FSM rides in the same tree: it wakes the boss and shuts the gates.
    let encounter = fsms_on(cache, &[("level133", 38)])?;
    let mut sources: Vec<(i64, String)> = root.fsms.iter().map(|(go, sid, _)| (*go, sid.clone())).collect();
    for (sid, _) in encounter.get(&("level133".to_owned(), 38)).ok_or("encounter FSM")? {
        sources.push((38, sid.clone()));
    }
    let mut values: Vec<(i64, Value)> = vec![];
    for (go, sid) in &sources {
        let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
        let file = source.file(f)?;
        let tree = source.read(&source.object(&file, p.parse()?)?)?;
        values.push((*go, tree.get("fsm").ok_or("fsm")?.clone()));
    }
    let fsms: Vec<(i64, &Value)> = values.iter().map(|(g, v)| (*g, v)).collect();
    // The PlayerData fields Hornet's FSMs read and write, in the order of the adapter's local reserve (`HORNET_FIELDS` in
    // game/src/hornet.rs maps each to the save record).
    let fields: Vec<String> =
        ["hornetGreenpath", "hornet1Defeated", "disablePause", "dreamerScene1", "hasDash", "equippedCharm_15"].iter().map(|s| s.to_string()).collect();
    let cooked = match cook_boss_tree(&source, "level133", &fsms, &[root.go], Some(&fields)) {
        Ok(c) => c,
        Err(e) => return Err(format!("{e}").into()),
    };
    let label = "hornet3";
    std::fs::write(root_dir.join(format!("data/{label}.hkf3")), &cooked.cooked.bank)?;
    std::fs::write(root_dir.join(format!("data/{label}_fsm3.rs")), rust_table(label, &cooked, ""))?;
    hk_fsm_cook::cook3::write_names_pub(root_dir, label, &cooked.cooked)?;
    println!("{label}: {} FSMs, {} objects, {} bank bytes", cooked.cooked.irs.len(), cooked.ents.len(), cooked.cooked.bank.len());
    Ok(())
}
