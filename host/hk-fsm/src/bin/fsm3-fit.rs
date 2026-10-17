//! Per-boss module fit inputs: lower every boss and enemy tree of the census to HKFSM03 and report the
//! bank, the runtime block and the action-type set of each (docs/FSM-V2-DESIGN.md sections 3.2, 3.3).
//!
//! fsm3-fit ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ OUTPUT_DIR
//! fsm3-fit ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ --list MIN_FSMS
//!
//! Writes OUTPUT_DIR/fit.json (one record per tree: sizes, counts, and `types`: action type -> (family, instances))
//! and prints a markdown table. Host only; retail data is read only.
use hk_fsm_cook::census::load_roots;
use hk_fsm_cook::families::FAMILIES;
use hk_fsm_cook::v3::*;
use hk_unity::{Source, Value};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

/// (label, scene, GameObject). The base game first, in the order of the design, then the DLC: Grimm Troupe and
/// Godhome. Godhome statue fights reuse the base game's trees with changed numbers, so their trees are the
/// same lowering; the entries below are the ones with a tree of their own.
const TREES: &[(&str, &str, &str)] = &[
    ("Crawler", "Crossroads_05", "Crawler"),
    ("Gruzzer", "Crossroads_04", "Fly"),
    ("Vengefly", "Fungus1_20_v02", "Buzzer"),
    ("Hornet 1", "Fungus1_04_boss", "Hornet Boss 1"),
    ("Mantis Lords", "Fungus2_15_boss", "Mantis Lord"),
    ("Soul Master", "Ruins1_24_boss", "Mage Lord"),
    ("Collector", "Ruins2_11_boss", "Jar Collector"),
    ("Watcher Knights", "Ruins2_03_boss", "Black Knight 1"),
    ("Dung Defender", "Waterways_05_boss", "Dung Defender"),
    ("Flukemarm", "Waterways_12_boss", "Fluke Mother"),
    ("Uumuu", "Fungus3_archive_02_boss", "Mega Jellyfish"),
    ("Traitor Lord", "Fungus3_23_boss", "Mantis Traitor Lord"),
    ("Crystal Guardian", "Mines_18_boss", "Mega Zombie Beam Miner (1)"),
    ("Nosk", "Deepnest_32", "Mimic Spider"),
    ("Hive Knight", "Hive_05", "Hive Knight"),
    ("Hornet 2", "Deepnest_East_Hornet_boss", "Hornet Boss 2"),
    ("Broken Vessel", "Abyss_19", "Infected Knight"),
    ("Grimm", "Grimm_Main_Tent_boss", "Grimm Boss"),
    ("White Defender", "Dream_04_White_Defender", "White Defender"),
    ("Radiance", "Dream_Final_Boss", "Radiance"),
    ("Hollow Knight", "Room_Final_Boss_Core", "Hollow Knight Boss"),
    ("False Knight", "Crossroads_10_boss", "False Knight New"),
    ("Gruz Mother", "Crossroads_04", "Giant Fly"),
    ("Brooding Mawlek", "Crossroads_09", "Mawlek Body"),
    ("Lost Kin", "Dream_03_Infected_Knight", "Lost Kin"),
    ("Grey Prince Zote", "Dream_Mighty_Zote", "Grey Prince"),
];

fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() < 4 {
        return Err("usage: fsm3-fit DATA CACHE OUTDIR | fsm3-fit DATA CACHE --list MIN_FSMS".into());
    }
    let source = Source::new(&a[1])?;
    let roots = load_roots(&source, std::path::Path::new(&a[2]), 4)?;
    if a[3] == "--list" {
        let min: usize = a.get(4).ok_or("MIN_FSMS")?.parse()?;
        for r in &roots {
            if r.fsms.len() >= min {
                println!("{}\t{}\t{}\tgo={}\tfsms={}", r.scene, r.name, r.level, r.ngo, r.fsms.len());
            }
        }
        return Ok(());
    }
    std::fs::create_dir_all(&a[3])?;
    let mut by_name: HashMap<usize, String> = HashMap::new();
    for t in hk_fsm_cook::families::all_types() {
        let (f, o) = op_of(t).ok_or("op")?;
        by_name.insert(f as usize * 256 + o as usize, t.to_owned());
    }
    let mut out = vec![];
    let mut md = String::new();
    writeln!(md, "| tree | FSMs | states | actions | distinct types | bank B | FSM runtime B | entities | strings | events |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")?;
    let mut extra: Vec<(String, String, String)> = vec![];
    if let Ok(list) = std::fs::read_to_string(format!("{}/extra.tsv", a[3])) {
        for l in list.lines() {
            let f: Vec<_> = l.split('\t').collect();
            if f.len() == 3 {
                extra.push((f[0].to_owned(), f[1].to_owned(), f[2].to_owned()));
            }
        }
    }
    let all: Vec<(String, String, String)> = TREES
        .iter()
        .map(|(l, s, n)| (l.to_string(), s.to_string(), n.to_string()))
        .chain(extra)
        .collect();
    for (label, scene, name) in &all {
        let Some(root) = roots.iter().filter(|r| &r.scene == scene && &r.name == name).max_by_key(|r| r.fsms.len()) else {
            eprintln!("{label}: root {scene}/{name} not found");
            continue;
        };
        let mut values = vec![];
        for (go, sid, fname) in &root.fsms {
            let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
            let file = source.file(f)?;
            let obj = source.object(&file, p.parse()?)?;
            let tree = source.read(&obj)?;
            values.push((*go, fname.clone(), tree.get("fsm").ok_or("fsm")?.clone()));
        }
        let mut ctx = TreeCtx::new();
        let mut stats = Stats::default();
        for (go, _, _) in &values {
            ctx.entity(*go);
        }
        let mut irs = vec![];
        let mut failed = None;
        for (_, fname, v) in &values {
            match Enc::new(&mut ctx, &mut stats, v).and_then(|e| e.encode()) {
                Ok(ir) => irs.push(ir),
                Err(e) => {
                    failed = Some(format!("{fname}: {e}"));
                    break;
                }
            }
        }
        if let Some(e) = failed {
            eprintln!("{label}: lowering failed: {e}");
            writeln!(md, "| {label} | lowering failed: {e} | | | | | | | | |")?;
            continue;
        }
        let (_, sizes, overhead) = serialize(&irs, ctx.events.len(), ctx.entities.len(), ctx.strings.len(), 0)?;
        let bank: usize = overhead + sizes.iter().map(|s| s.bank()).sum::<usize>();
        let runtime: usize = sizes.iter().map(|s| s.runtime).sum();
        let mut types: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut states = 0;
        let mut actions = 0;
        for ir in &irs {
            states += ir.states.len();
            for s in &ir.states {
                for ac in &s.actions {
                    actions += 1;
                    let n = by_name.get(&(ac.family as usize * 256 + ac.op as usize)).cloned().unwrap_or_else(|| format!("?{}:{}", ac.family, ac.op));
                    types.entry(n).or_insert((ac.family as usize, 0)).1 += 1;
                }
            }
        }
        writeln!(
            md,
            "| {label} | {} | {states} | {actions} | {} | {bank} | {runtime} | {} | {} | {} |",
            irs.len(),
            types.len(),
            ctx.entities.len(),
            ctx.strings.len(),
            ctx.events.len()
        )?;
        let tj: serde_json::Map<String, serde_json::Value> = types
            .iter()
            .map(|(k, (f, n))| (k.clone(), serde_json::json!([FAMILIES[*f], n])))
            .collect();
        out.push(serde_json::json!({
            "label": label, "scene": scene, "root": name, "fsms": irs.len(), "states": states, "actions": actions,
            "bank_bytes": bank, "fsm_runtime_bytes": runtime, "entities": ctx.entities.len(), "strings": ctx.strings.len(),
            "events": ctx.events.len(), "game_objects": root.ngo, "types": tj,
        }));
    }
    std::fs::write(format!("{}/fit.json", a[3]), serde_json::to_string_pretty(&out)?)?;
    std::fs::write(format!("{}/fit.md", a[3]), &md)?;
    println!("{md}");
    let _ = Value::Bool(true);
    Ok(())
}
