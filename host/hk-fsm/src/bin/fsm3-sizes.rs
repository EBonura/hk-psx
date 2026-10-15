//! Lower the original PlayMaker trees of the first four consumers to the proposed
//! HKFSM03 layout and report real byte sizes (docs/FSM-V2-DESIGN.md sections 2.1, 3.2).
//!
//! fsm3-sizes ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ OUTPUT_DIR
//!
//! Host only: nothing here runs on the guest. Banks are written to OUTPUT_DIR (not
//! the repository); the decode-and-compare round trip must pass for every tree.
use hk_fsm_cook::census::load_roots;
use hk_fsm_cook::families::FAMILIES;
use hk_fsm_cook::v3::*;
use hk_unity::{Source, Value};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

const TREES: [(&str, &str, &str); 4] = [
    ("Crawler", "Crossroads_05", "Crawler"),
    ("Gruzzer", "Crossroads_04", "Fly"),
    ("Vengefly", "Fungus1_20_v02", "Buzzer"),
    ("Hornet 1", "Fungus1_04_boss", "Hornet Boss 1"),
];

fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 4 {
        return Err("usage: fsm3-sizes ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ OUTPUT_DIR".into());
    }
    let source = Source::new(&a[1])?;
    let roots = load_roots(&source, std::path::Path::new(&a[2]), 4)?;
    std::fs::create_dir_all(&a[3])?;
    let mut md = String::new();
    let mut summary = vec![];
    let mut all_schema: HashMap<(u8, u8), usize> = HashMap::new();
    for (label, scene, name) in TREES {
        let root = roots.iter().find(|r| r.scene == scene && r.name == name).ok_or("root not found")?;
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
        for (_, _, v) in &values {
            irs.push(Enc::new(&mut ctx, &mut stats, v)?.encode()?);
        }
        // Operand schema: base arity per (family, op), from the data itself.
        let mut schema: HashMap<(u8, u8), usize> = HashMap::new();
        let mut irregular: BTreeMap<(u8, u8), Vec<usize>> = BTreeMap::new();
        for ir in &irs {
            for s in &ir.states {
                for ac in &s.actions {
                    let base = ac.operands.len() + (ac.flags & F_OWNER != 0) as usize;
                    let e = schema.entry((ac.family, ac.op)).or_insert(base);
                    if *e != base {
                        let v = irregular.entry((ac.family, ac.op)).or_default();
                        if !v.contains(&base) { v.push(base) }
                        if !v.contains(e) { v.push(*e) }
                    }
                }
            }
        }
        all_schema.extend(schema.iter().map(|(k, v)| (*k, *v)));
        let (bank, sizes, overhead) = serialize(&irs, ctx.events.len(), ctx.entities.len(), ctx.strings.len(), 0)?;
        std::fs::write(format!("{}/{}.hkf3", a[3], label.to_lowercase().replace(' ', "")), &bank)?;
        let back = deserialize(&bank, &|f, o, fl| schema[&(f, o)] - (fl & F_OWNER != 0) as usize)?;
        let mut mismatches = 0;
        for (x, y) in irs.iter().zip(&back) {
            let same = x.states == y.states && x.globals == y.globals && x.arrays == y.arrays && x.pool == y.pool && x.init == y.init && x.obj_init == y.obj_init && x.start == y.start;
            if !same { mismatches += 1 }
        }
        // G0 against the source: same states, same enabled actions in order, same
        // transitions (event names through the tree table, targets by state name).
        let mut g0 = 0;
        for ((_, _, v), y) in values.iter().zip(&back) {
            let states = v.get("states").and_then(Value::list).ok_or("states")?;
            let names: Vec<String> = states.iter().map(|s| s.get("name").and_then(Value::str).unwrap_or_default()).collect();
            let ev = |t: &Value| t.get("fsmEvent").and_then(|e| e.get("name")).and_then(Value::str).unwrap_or_default();
            let to = |t: &Value| t.get("toState").and_then(Value::str).unwrap_or_default();
            let mut ok = states.len() == y.states.len() && names.get(y.start as usize).map(String::as_str) == v.get("startState").and_then(Value::str).as_deref();
            for (st, d) in states.iter().zip(&y.states) {
                let want: Vec<_> = hk_unity::playmaker::enabled(st).iter().map(|(n, _)| op_of(n)).collect();
                let got: Vec<_> = d.actions.iter().map(|a| Some((a.family, a.op))).collect();
                let tr: Vec<_> = st.get("transitions").and_then(Value::list).unwrap_or(&[]).iter().map(|t| (ev(t), to(t))).collect();
                let tg: Vec<_> = d.transitions.iter().map(|(e, s)| (ctx.events[*e as usize].clone(), names[*s as usize].clone())).collect();
                ok &= want == got && tr == tg;
            }
            let gl: Vec<_> = v.get("globalTransitions").and_then(Value::list).unwrap_or(&[]).iter().map(|t| (ev(t), to(t))).collect();
            let gg: Vec<_> = y.globals.iter().map(|(e, s)| (ctx.events[*e as usize].clone(), names[*s as usize].clone())).collect();
            ok &= gl == gg;
            if !ok { g0 += 1 }
        }
        let n_states: usize = irs.iter().map(|i| i.states.len()).sum();
        let n_actions: usize = irs.iter().map(|i| i.states.iter().map(|s| s.actions.len()).sum::<usize>()).sum();
        let n_trans: usize = irs.iter().map(|i| i.states.iter().map(|s| s.transitions.len()).sum::<usize>() + i.globals.len()).sum();
        let n_operands: usize = irs.iter().map(|i| i.states.iter().flat_map(|s| s.actions.iter()).map(|a| a.operands.len()).sum::<usize>()).sum();
        let total: usize = overhead + sizes.iter().map(|s| s.bank()).sum::<usize>();
        let runtime: usize = sizes.iter().map(|s| s.runtime).sum::<usize>();
        writeln!(md, "## {label} ({scene} / {name})\n")?;
        writeln!(md, "{} FSMs, {} states, {} enabled actions, {} operands, {} transitions (incl. global). Bank **{} B** (serialized length {} B). Decode round trip: {}. G0 listing against the source (states, enabled action order, transitions): {}.\n", irs.len(), n_states, n_actions, n_operands, n_trans, total, bank.len(), if mismatches == 0 { "identical".to_owned() } else { format!("{mismatches} FSMs DIFFER") }, if g0 == 0 { "clean".to_owned() } else { format!("{g0} FSMs DIFFER") })?;
        writeln!(md, "| FSM | states | actions | operands | dir | state tbl | trans | actions B | arrays | pool | init | pad | **bank B** | cells | bits | objs | scratch | runtime B |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")?;
        for (i, (ir, s)) in irs.iter().zip(&sizes).enumerate() {
            let acts: usize = ir.states.iter().map(|x| x.actions.len()).sum();
            let ops: usize = ir.states.iter().flat_map(|x| x.actions.iter()).map(|x| x.operands.len()).sum();
            writeln!(md, "| {} #{i} | {} | {acts} | {ops} | {} | {} | {} | {} | {} | {} | {} | {} | **{}** | {} | {} | {} | {} | {} |", ir.name, ir.states.len(), s.dir, s.state_table, s.transitions, s.actions, s.arrays, s.pool, s.init, s.pad, s.bank(), ir.cells, ir.bits, ir.objs, ir.scratch, s.runtime)?;
        }
        let sum = |f: &dyn Fn(&Sizes) -> usize| sizes.iter().map(f).sum::<usize>();
        writeln!(md, "| **sections total** | {n_states} | {n_actions} | {n_operands} | {} | {} | {} | {} | {} | {} | {} | {} | **{}** | {} | {} | {} | {} | {} |", sum(&|s| s.dir), sum(&|s| s.state_table), sum(&|s| s.transitions), sum(&|s| s.actions), sum(&|s| s.arrays), sum(&|s| s.pool), sum(&|s| s.init), sum(&|s| s.pad), sum(&|s| s.bank()), irs.iter().map(|i| i.cells as usize).sum::<usize>(), irs.iter().map(|i| i.bits as usize).sum::<usize>(), irs.iter().map(|i| i.objs as usize).sum::<usize>(), irs.iter().map(|i| i.scratch as usize).sum::<usize>(), sum(&|s| s.runtime))?;
        writeln!(md, "| tree header + event map ({} events x {} B mask) | | | | | | | | | | | | **{}** | | | | | |", ctx.events.len(), irs.len().div_ceil(8), overhead)?;
        writeln!(md, "\nRuntime block total {} B (sum of FSM blocks of 12 B header + cells + bit image + object handles + scratch + finished bitset; the entity table and the 64 B event queue are extra). Entities referenced: {} (subtree has {} GameObjects). Strings interned: {}. Tree events: {}.\n", runtime, ctx.entities.len(), root.ngo, ctx.strings.len(), ctx.events.len())?;
        let avg_ops = n_operands as f64 / n_actions.max(1) as f64;
        writeln!(md, "Per action: {:.2} operands, {:.2} B stream; bank {:.1} B per enabled action; {:.1} B per state in the state table plus blocks.\n", avg_ops, (2.0 + 2.0 * avg_ops), total as f64 / n_actions.max(1) as f64, 4.0)?;
        let tc = stats.tag_counts;
        writeln!(md, "Operand tags: {}.", (0..8).map(|i| format!("{}={}", ["CONST", "CELL", "BIT", "OBJ", "EVENT", "NONE", "INLINE", "ARRAY"][i], tc[i])).collect::<Vec<_>>().join(", "))?;
        writeln!(md, "Owner flag (operand dropped): {} actions; specific-object owner operands: {}; float/int literals inline vs pool: {} / {}; Q16.16 overflows: {}; class mismatches: {}.", stats.owner_flag, stats.specific_owner, stats.inline_hits, stats.const_hits, stats.float_overflow, stats.class_mismatch)?;
        if !stats.unbound_vars.is_empty() {
            writeln!(md, "Variables not declared on the FSM (PlayMaker globals, bound to hidden slots): {}.", stats.unbound_vars.iter().map(|(k, v)| format!("{k} x{v}")).collect::<Vec<_>>().join(", "))?;
        }
        if !stats.unsupported.is_empty() {
            writeln!(md, "UNSUPPORTED parameters: {:?}", stats.unsupported)?;
        }
        if !irregular.is_empty() {
            writeln!(md, "IRREGULAR operand arity (op schema is not fixed): {irregular:?}")?;
        }
        writeln!(md)?;
        let by_family: Vec<String> = FAMILIES.iter().enumerate().map(|(f, n)| {
            let c: usize = irs.iter().flat_map(|i| i.states.iter().flat_map(|s| s.actions.iter())).filter(|x| x.family as usize == f).count();
            format!("{n} {c}")
        }).collect();
        writeln!(md, "Actions by family: {}.\n", by_family.join(", "))?;
        let sch: BTreeMap<String, usize> = stats.arity.iter().map(|(k, v)| (k.clone(), *v.keys().max().unwrap_or(&0))).collect();
        summary.push((label, irs.len(), n_states, n_actions, n_operands, total, runtime, sch, irregular.len(), mismatches + g0));
    }
    writeln!(md, "## Summary\n\n| tree | FSMs | states | actions | operands | bank B | B/action | runtime B | irregular ops | round-trip failures |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")?;
    for (l, f, s, ac, o, t, r, _, irr, mm) in &summary {
        writeln!(md, "| {l} | {f} | {s} | {ac} | {o} | {t} | {:.1} | {r} | {irr} | {mm} |", *t as f64 / (*ac).max(1) as f64)?;
    }
    if let Some((_, _, _, _, _, _, _, sch, _, _)) = summary.last() {
        writeln!(md, "\n## Hornet 1 operand schema (base operands per action type, owner operand counted)\n")?;
        let mut rows: Vec<_> = sch.iter().collect();
        rows.sort();
        writeln!(md, "{}", rows.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(" "))?;
    }
    // The resident operand-count table the kernel needs to step over an action it does not run.
    {
        let mut names: Vec<Vec<&str>> = vec![vec![]; FAMILIES.len()];
        for t in hk_fsm_cook::families::all_types() {
            let (f, _) = op_of(t).ok_or("op")?;
            names[f as usize].push(t);
        }
        let mut base = vec![];
        let mut table = vec![];
        for (f, list) in names.iter_mut().enumerate() {
            list.sort();
            base.push(table.len());
            for (o, _) in list.iter().enumerate() {
                table.push(all_schema.get(&(f as u8, o as u8)).copied().unwrap_or(0));
            }
        }
        let mut rs = String::from("// Generated by fsm3-sizes: base operand count per (family, op); 0 = op not in the measured trees.\n");
        rs.push_str(&format!("pub static BASE: [u16; {}] = {:?};\n", base.len(), base));
        rs.push_str(&format!("pub static ARITY: [u8; {}] = {:?};\n", table.len(), table.iter().map(|x| *x as u8).collect::<Vec<_>>()));
        std::fs::write(format!("{}/arity.rs", a[3]), rs)?;
    }
    std::fs::write(format!("{}/fsm3-sizes.md", a[3]), &md)?;
    println!("{md}");
    let _ = Value::Bool(true);
    Ok(())
}
