//! Regenerate `shared/hk-format/src/fsm3/ops.rs`: the op numbering of the 338 census action
//! types and the operand count of every type the cooked trees use.
//!
//! fsm3-ops ORIGINAL_WINDOWS_DATA SOURCE_OBJECTS_JSONL_GZ OUTPUT.rs
//!
//! The numbering is the position of the type's name among its family's names (families.rs), so it
//! is stable as long as the census list is. Arities are measured from the data: the four starter
//! trees and the scene scripts. A type no cooked tree uses has no arity (0xff) and the cooker
//! refuses it, so adding a consumer extends the table without renumbering anything.
use hk_fsm_cook::census::load_roots;
use hk_fsm_cook::families::{all_types, FAMILIES};
use hk_fsm_cook::v3::*;
use hk_unity::Source;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

pub const TREES: [(&str, &str); 4] =
    [("Crossroads_05", "Crawler"), ("Crossroads_04", "Fly"), ("Fungus1_20_v02", "Buzzer"), ("Fungus1_04_boss", "Hornet Boss 1")];
/// Single FSMs measured on their own (`file:path id`): Hornet's encounter and the corpse prefab it leaves, and the
/// Plant Trap's control FSM (the enemy-family pipeline's first tree; it measures StopParticleEmitter).
pub const EXTRA: [&str; 3] = ["level133:239", "sharedassets105.assets:62", "level150:10095"];
/// The scene scripts HKSCR01 carried (.hkpsx/script-bank.json), now cooked as HKFSM03.
pub const SCRIPTS: [&str; 11] = [
    "level6:709", "level7:94", "level7:189", "level37:425", "level39:630", "level42:234", "level56:393", "level65:710", "level81:1205",
    "level76:1739", "level146:575",
];

const MODS: [&str; 14] = [
    "flow", "math", "events", "timers", "transform", "ai", "raycast", "animation", "object", "audio", "fx", "collider", "combat", "playerdata",
];

fn upper_snake(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit()) {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 4 {
        return Err("usage: fsm3-ops DATA CACHE OUTPUT.rs".into());
    }
    let source = Source::new(&a[1])?;
    let roots = load_roots(&source, std::path::Path::new(&a[2]), 4)?;
    let mut schema: HashMap<(u8, u8), usize> = HashMap::new();
    let mut irregular = vec![];
    let mut measure = |value: &hk_unity::Value, ctx: &mut TreeCtx, stats: &mut Stats| -> Result<()> {
        let ir = Enc::new(ctx, stats, value)?.encode()?;
        for s in &ir.states {
            for ac in &s.actions {
                let base = ac.operands.len() + (ac.flags & F_OWNER != 0) as usize;
                let e = schema.entry((ac.family, ac.op)).or_insert(base);
                if *e != base {
                    irregular.push((ac.family, ac.op, *e, base));
                }
            }
        }
        Ok(())
    };
    for (scene, name) in TREES {
        let root = roots.iter().find(|r| r.scene == scene && r.name == name).ok_or("root not found")?;
        let mut ctx = TreeCtx::new();
        let mut stats = Stats::default();
        for (go, sid, _) in &root.fsms {
            let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
            let file = source.file(f)?;
            let obj = source.object(&file, p.parse()?)?;
            let tree = source.read(&obj)?;
            ctx.entity(*go);
            measure(tree.get("fsm").ok_or("fsm")?, &mut ctx, &mut stats)?;
        }
    }
    for sid in EXTRA {
        let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
        let file = source.file(f)?;
        let obj = source.object(&file, p.parse()?)?;
        let tree = source.read(&obj)?;
        let mut ctx = TreeCtx::new();
        let mut stats = Stats::default();
        measure(tree.get("fsm").ok_or("fsm")?, &mut ctx, &mut stats)?;
    }
    let wanted: Vec<(&str, i64)> = SCRIPTS.iter().map(|s| { let (l, g) = s.split_once(':').unwrap(); (l, g.parse().unwrap()) }).collect();
    let found = hk_fsm_cook::census::fsms_on(std::path::Path::new(&a[2]), &wanted)?;
    for (level, go) in &wanted {
        for (sid, _) in found.get(&(level.to_string(), *go)).ok_or("script owner has no FSM")? {
            let (f, p) = sid.rsplit_once(':').ok_or("sid")?;
            let file = source.file(f)?;
            let obj = source.object(&file, p.parse()?)?;
            let tree = source.read(&obj)?;
            let mut ctx = TreeCtx::new();
            let mut stats = Stats::default();
            measure(tree.get("fsm").ok_or("fsm")?, &mut ctx, &mut stats)?;
        }
    }
    if !irregular.is_empty() {
        return Err(format!("irregular operand counts: {irregular:?}").into());
    }
    let mut by_family: Vec<Vec<&str>> = vec![vec![]; FAMILIES.len()];
    for t in all_types() {
        let (f, _) = op_of(t).ok_or("op")?;
        by_family[f as usize].push(t);
    }
    let mut base = vec![];
    let mut table = vec![];
    let mut consts: BTreeMap<usize, Vec<(String, usize)>> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for (f, list) in by_family.iter_mut().enumerate() {
        list.sort();
        base.push(table.len());
        for (o, name) in list.iter().enumerate() {
            table.push(schema.get(&(f as u8, o as u8)).map(|n| *n as u8).unwrap_or(0xff));
            let id = upper_snake(name);
            if !seen.insert((f, id.clone())) {
                return Err(format!("duplicate op constant {id}").into());
            }
            consts.entry(f).or_default().push((id, o));
        }
    }
    let mut rs = String::new();
    writeln!(rs, "//! Generated by host/hk-fsm `fsm3-ops`; do not edit. Op numbering and operand counts (docs/FSM-V2-DESIGN.md 2.1).")?;
    writeln!(rs, "//! Numbering: position of the action type among its family's types in name order. 0xff = no cooked tree uses the type yet.")?;
    writeln!(rs, "pub static BASE: [u16; {}] = {:?};", base.len(), base)?;
    writeln!(rs, "pub static ARITY: [u8; {}] = {:?};", table.len(), table)?;
    writeln!(rs, "#[inline(always)]\npub fn known(family: u8, op: u8) -> bool {{\n    BASE.get(family as usize).is_some_and(|b| ARITY.get(*b as usize + op as usize).is_some_and(|a| *a != 0xff)) && (op as usize) < FAMILY_OPS[family as usize] as usize\n}}")?;
    let counts: Vec<usize> = by_family.iter().map(Vec::len).collect();
    writeln!(rs, "pub static FAMILY_OPS: [u8; {}] = {:?};", counts.len(), counts.iter().map(|c| *c as u8).collect::<Vec<_>>())?;
    writeln!(rs, "/// Operand count of an action: the schema count, less the omitted owner operand.\n#[inline(always)]\npub fn arity(family: u8, op: u8, flags: u8) -> usize {{\n    ARITY[BASE[family as usize] as usize + op as usize] as usize - (flags & super::F_OWNER != 0) as usize\n}}")?;
    for (f, items) in &consts {
        writeln!(rs, "/// Family {} ({}).\npub mod {} {{", f, FAMILIES[*f], MODS[*f])?;
        for (id, o) in items {
            writeln!(rs, "    pub const {id}: u8 = {o};")?;
        }
        writeln!(rs, "}}")?;
    }
    std::fs::write(&a[3], rs)?;
    let known = table.iter().filter(|a| **a != 0xff).count();
    println!("{} types, {} with a measured arity", table.len(), known);
    Ok(())
}
