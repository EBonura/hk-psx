//! Print the HKFSM03 lowering of the starter trees as readable listings.
//!
//! fsm3-dump ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ [TREE_LABEL]
use hk_fsm_cook::census::load_roots;
use hk_fsm_cook::v3::*;
use hk_unity::{Source, Value};
use std::collections::HashMap;
use std::fmt::Write;

const TREES: [(&str, &str, &str); 3] = [
    ("Crawler", "Crossroads_05", "Crawler"),
    ("Gruzzer", "Crossroads_04", "Fly"),
    ("Vengefly", "Fungus1_20_v02", "Buzzer"),
];

fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() < 3 {
        return Err("usage: fsm3-dump DATA CACHE [LABEL]".into());
    }
    let source = Source::new(&a[1])?;
    let roots = load_roots(&source, std::path::Path::new(&a[2]), 4)?;
    let mut names: HashMap<(u8, u8), &str> = HashMap::new();
    for t in hk_fsm_cook::families::all_types() {
        names.insert(op_of(t).ok_or("op")?, t);
    }
    if a.get(3).is_some_and(|l| l == "Scripts") {
        const SCRIPTS: [&str; 11] = ["level6:709", "level7:94", "level7:189", "level37:425", "level39:630", "level42:234", "level56:393", "level65:710", "level81:1205", "level76:1739", "level146:575"];
        let wanted: Vec<(&str, i64)> = SCRIPTS.iter().map(|s| { let (l, g) = s.split_once(':').unwrap(); (l, g.parse().unwrap()) }).collect();
        let found = hk_fsm_cook::census::fsms_on(std::path::Path::new(&a[2]), &wanted)?;
        for (level, go) in &wanted {
            for (sid, fname) in &found[&(level.to_string(), *go)] {
                let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
                let file = source.file(f)?;
                let obj = source.object(&file, p.parse()?)?;
                let tree = source.read(&obj)?;
                let fsm = tree.get("fsm").ok_or("fsm")?;
                let mut ctx = TreeCtx::new();
                let mut stats = Stats::default();
                ctx.entity(*go);
                let ir = Enc::new(&mut ctx, &mut stats, fsm)?.encode()?;
                println!("## script {level}:{go} {fname} start={} cells={} bits={} objs={} scratch={}", ir.state_names[ir.start as usize], ir.cells, ir.bits, ir.objs, ir.scratch);
                println!("pool={:?} init={:?} bit_init={:?} obj_init={:?}", ir.pool, ir.init, ir.bit_init, ir.obj_init);
                for (si, st) in ir.states.iter().enumerate() {
                    println!("state {si} {:?}", ir.state_names[si]);
                    for (e, t) in &st.transitions { println!("    on {} -> {}", ctx.events[*e as usize], ir.state_names[*t as usize]); }
                    for (ai, ac) in st.actions.iter().enumerate() {
                        let ops: Vec<String> = ac.operands.iter().map(|o| { let (tag, p) = (o >> 13, (o & 0x1fff) as i32); match tag { T_CONST => format!("pool[{p}]={}", ir.pool.get(p as usize).map(|x| *x as i32).unwrap_or(0)), T_CELL => format!("cell{p}"), T_BIT => format!("bit{p}"), T_OBJ => format!("obj{p}"), T_EVENT => format!("ev:{}", ctx.events[p as usize]), T_NONE => "none".to_owned(), T_INLINE => format!("lit{}", (p << 19) >> 19), _ => format!("arr@{p}") } }).collect();
                        println!("    a{ai} {} f={:#x} [{}]", names[&(ac.family, ac.op)], ac.flags, ops.join(", "));
                    }
                }
                let mut strs: Vec<_> = ctx.strings.iter().collect();
                strs.sort_by_key(|(_, i)| **i);
                println!("strings: {:?} unbound: {:?} unsupported {:?}", strs, stats.unbound_vars, stats.unsupported);
            }
        }
        return Ok(());
    }
    for (label, scene, name) in TREES {
        if a.get(3).is_some_and(|l| l != label) {
            continue;
        }
        let root = roots.iter().find(|r| r.scene == scene && r.name == name).ok_or("root not found")?;
        let mut ctx = TreeCtx::new();
        let mut stats = Stats::default();
        let mut values = vec![];
        for (go, sid, fname) in &root.fsms {
            let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
            let file = source.file(f)?;
            let obj = source.object(&file, p.parse()?)?;
            let tree = source.read(&obj)?;
            ctx.entity(*go);
            values.push((fname.clone(), sid.clone(), tree.get("fsm").ok_or("fsm")?.clone()));
        }
        let mut irs = vec![];
        for (_, _, v) in &values {
            irs.push(Enc::new(&mut ctx, &mut stats, v)?.encode()?);
        }
        let mut out = String::new();
        writeln!(out, "# {label}: {} FSMs", irs.len())?;
        for ((fname, sid, v), ir) in values.iter().zip(&irs) {
            let state_names: Vec<String> = v.get("states").and_then(Value::list).unwrap().iter().map(|s| s.get("name").and_then(Value::str).unwrap_or_default()).collect();
            writeln!(out, "\n## FSM {fname} ({sid}) start={} cells={} bits={} objs={} scratch={} fin_bytes={}", state_names[ir.start as usize], ir.cells, ir.bits, ir.objs, ir.scratch, ir.fin_bytes)?;
            writeln!(out, "pool={:?} init={:?} bit_init={:?} obj_init={:?} arrays={:?}", ir.pool, ir.init, ir.bit_init, ir.obj_init, ir.arrays)?;
            for (si, st) in ir.states.iter().enumerate() {
                writeln!(out, "state {si} {:?}", state_names[si])?;
                for (e, t) in &st.transitions {
                    writeln!(out, "    on {} -> {}", ctx.events[*e as usize], state_names[*t as usize])?;
                }
                for (ai, ac) in st.actions.iter().enumerate() {
                    let ops: Vec<String> = ac.operands.iter().map(|o| {
                        let (tag, p) = (o >> 13, (o & 0x1fff) as i32);
                        match tag {
                            T_CONST => format!("pool[{p}]={}", ir.pool.get(p as usize).map(|x| *x as i32).unwrap_or(0)),
                            T_CELL => format!("cell{p}"),
                            T_BIT => format!("bit{p}"),
                            T_OBJ => format!("obj{p}"),
                            T_EVENT => format!("ev:{}", ctx.events[p as usize]),
                            T_NONE => "none".to_owned(),
                            T_INLINE => format!("lit{}", (p << 19) >> 19),
                            _ => format!("arr@{p}"),
                        }
                    }).collect();
                    writeln!(out, "    a{ai} {} f={:#x} [{}]", names[&(ac.family, ac.op)], ac.flags, ops.join(", "))?;
                }
            }
            for (e, t) in &ir.globals {
                writeln!(out, "global on {} -> {}", ctx.events[*e as usize], state_names[*t as usize])?;
            }
        }
        writeln!(out, "\nevents: {:?}", ctx.events)?;
        let mut strs: Vec<_> = ctx.strings.iter().collect();
        strs.sort_by_key(|(_, i)| **i);
        writeln!(out, "strings: {:?}", strs)?;
        print!("{out}");
    }
    Ok(())
}
