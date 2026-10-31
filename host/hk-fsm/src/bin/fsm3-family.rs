//! Family census for the enemy pipeline: the PlayMaker trees of every enemy whose GameObject name starts
//! with a prefix in one scene file, their action vocabulary, and which of those actions the light kernel
//! and the boss tier already have handlers for.
//!
//! fsm3-family REPO_ROOT LEVEL_FILE NAME_PREFIX [NAME_PREFIX...]
//!
//! LEVEL_FILE is the scene's Unity file (`level139` for Fungus1_10; `data/regions.json` has `scene_file`).
//! Retail files are read only. Nothing is written.
use hk_fsm_cook::families::{all_types, family};
use hk_fsm_cook::v3::{op_of, serialize, Enc, Stats, TreeCtx};
use hk_unity::{Source, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

fn read_all(dir: &std::path::Path) -> String {
    let mut text = String::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return text };
    let mut names: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    names.sort();
    for p in names {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_file() && n.ends_with(".rs") && (n.starts_with("fsm3_") || dir.ends_with("fsm3b")) {
            text.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
        }
    }
    text
}

fn has_token(text: &str, token: &str) -> bool {
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(i) = text[from..].find(token) {
        let s = from + i;
        let e = s + token.len();
        let ok_l = s == 0 || !(b[s - 1].is_ascii_alphanumeric() || b[s - 1] == b'_');
        let ok_r = e >= b.len() || !(b[e].is_ascii_alphanumeric() || b[e] == b'_');
        if ok_l && ok_r {
            return true;
        }
        from = e;
    }
    false
}

fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() < 4 {
        return Err("usage: fsm3-family REPO_ROOT LEVEL_FILE NAME_PREFIX...".into());
    }
    let root = std::path::PathBuf::from(&a[1]);
    let source = Source::from_doctor(&root)?;
    let light = read_all(&root.join("shared/hk-sim/src"));
    let boss = read_all(&root.join("shared/hk-sim/src/fsm3b"));
    let known: BTreeSet<&str> = all_types().collect();
    let file = source.file(&a[2])?;
    // GameObject names, transform children, and every component list.
    let mut go_name: BTreeMap<i64, String> = BTreeMap::new();
    let mut comps: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    let mut tr_go: BTreeMap<i64, i64> = BTreeMap::new();
    let mut tr_father: BTreeMap<i64, i64> = BTreeMap::new();
    let mut hm: Vec<i64> = vec![];
    for info in &file.objects {
        match info.class_id {
            1 => {
                let obj = source.object(&file, info.path_id)?;
                let tree = source.read(&obj)?;
                go_name.insert(info.path_id, tree.get("m_Name").and_then(Value::str).unwrap_or_default());
                comps.insert(
                    info.path_id,
                    tree.get("m_Component").and_then(Value::list).unwrap_or(&[]).iter().filter_map(|c| c.get("component").cloned()).collect(),
                );
            }
            4 | 224 => {
                let obj = source.object(&file, info.path_id)?;
                let Ok(tree) = source.read(&obj) else { continue };
                let g = tree.get("m_GameObject").and_then(Value::pptr).map(|p| p.1).unwrap_or(0);
                let f = tree.get("m_Father").and_then(Value::pptr).map(|p| p.1).unwrap_or(0);
                tr_go.insert(info.path_id, g);
                tr_father.insert(info.path_id, f);
            }
            _ => {}
        }
    }
    let mut kids: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let go_tr: BTreeMap<i64, i64> = tr_go.iter().map(|(t, g)| (*g, *t)).collect();
    for (t, f) in &tr_father {
        if let (Some(cg), Some(pg)) = (tr_go.get(t), tr_go.get(f)) {
            kids.entry(*pg).or_default().push(*cg);
        }
    }
    for info in &file.objects {
        if info.class_id != 114 {
            continue;
        }
        let obj = source.object(&file, info.path_id)?;
        if source.typename(&obj).is_ok_and(|t| t == "HealthManager") {
            let head = source.mono_head(&obj)?;
            if let Some((_, g)) = head.get("m_GameObject").and_then(Value::pptr) {
                hm.push(g);
            }
        }
    }
    let _ = go_tr;
    let mut out = String::new();
    let mut seen_names = BTreeMap::<String, u32>::new();
    for g in &hm {
        let name = go_name.get(g).cloned().unwrap_or_default();
        if !a[3..].iter().any(|p| name.starts_with(p.as_str())) {
            continue;
        }
        *seen_names.entry(name.clone()).or_default() += 1;
        if seen_names[&name] > 1 {
            continue;
        }
        let mut subtree = vec![];
        let mut stack = vec![*g];
        while let Some(x) = stack.pop() {
            subtree.push(x);
            if let Some(c) = kids.get(&x) {
                stack.extend(c.iter().rev().copied());
            }
        }
        writeln!(out, "=== {name} (GameObject {g}), {} GameObjects in the subtree", subtree.len())?;
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        let mut total_states = 0;
        let mut total_actions = 0;
        let mut components: BTreeMap<String, usize> = BTreeMap::new();
        let mut trees: Vec<(i64, Value)> = vec![];
        for x in &subtree {
            for c in comps.get(x).into_iter().flatten() {
                let Ok(obj) = source.deref(&file, c) else { continue };
                let ty = source.typename(&obj).unwrap_or_default();
                if ty != "PlayMakerFSM" {
                    if obj.class_id() == 114 {
                        *components.entry(ty).or_default() += 1;
                    }
                    continue;
                }
                let tree = source.read(&obj)?;
                let Some(fsm) = tree.get("fsm") else { continue };
                let fname = fsm.get("name").and_then(Value::str).unwrap_or_default();
                let states = fsm.get("states").and_then(Value::list).unwrap_or(&[]);
                let enabled = tree.get("m_Enabled").and_then(Value::int).unwrap_or(1);
                let events = fsm.get("events").and_then(Value::list).map_or(0, <[_]>::len);
                trees.push((*x, fsm.clone()));
                if std::env::var_os("FSM3_STATES").is_some() {
                    for st in states {
                        let sn = st.get("name").and_then(Value::str).unwrap_or_default();
                        let acts: Vec<String> = hk_unity::playmaker::enabled(st).into_iter().map(|(n, _)| n).collect();
                        let tr: Vec<String> = st
                            .get("transitions")
                            .and_then(Value::list)
                            .unwrap_or(&[])
                            .iter()
                            .map(|t| {
                                format!(
                                    "{}->{}",
                                    t.get("fsmEvent").and_then(|e| e.get("name")).and_then(Value::str).unwrap_or_default(),
                                    t.get("toState").and_then(Value::str).unwrap_or_default()
                                )
                            })
                            .collect();
                        writeln!(out, "      [{fname}] {sn}: {} | {}", acts.join(" "), tr.join(" "))?;
                    }
                }
                let mut n_actions = 0;
                for st in states {
                    for (t, _) in hk_unity::playmaker::enabled(st) {
                        *types.entry(t).or_default() += 1;
                        n_actions += 1;
                    }
                }
                total_states += states.len();
                total_actions += n_actions;
                writeln!(
                    out,
                    "  FSM '{fname}' on '{}' enabled={enabled}: {} states, {n_actions} actions, {events} events, {} globals, start '{}'",
                    go_name.get(x).cloned().unwrap_or_default(),
                    states.len(),
                    fsm.get("globalTransitions").and_then(Value::list).map_or(0, <[_]>::len),
                    fsm.get("startState").and_then(Value::str).unwrap_or_default()
                )?;
            }
        }
        writeln!(out, "  components (non-FSM scripts): {}", components.iter().map(|(k, v)| format!("{k}x{v}")).collect::<Vec<_>>().join(", "))?;
        writeln!(out, "  totals: {total_states} states, {total_actions} enabled actions, {} distinct action types", types.len())?;
        let mut miss_light = vec![];
        let mut miss_boss = vec![];
        let mut rows = String::new();
        for (t, n) in &types {
            let tok = upper_snake(t);
            let l = has_token(&light, &tok);
            let b = has_token(&boss, &tok);
            let fam = family(t);
            let cooked = op_of(t).is_some_and(|(f, o)| hk_format::fsm3::ops::known(f, o));
            let ranked = if known.contains(t.as_str()) { "" } else { " (NOT IN FAMILY MAP)" };
            writeln!(rows, "    {t:<34} x{n:<3} {fam:<22} light={} boss={} arity_known={}{ranked}", l as u8, b as u8, cooked as u8)?;
            if !l {
                miss_light.push(t.clone());
            }
            if !b {
                miss_boss.push(t.clone());
            }
        }
        out.push_str(&rows);
        {
            let mut ctx = TreeCtx::new();
            let mut stats = Stats::default();
            for (go, _) in &trees {
                ctx.entity(*go);
            }
            let mut irs = vec![];
            let mut fail = None;
            for (_, v) in &trees {
                match Enc::new(&mut ctx, &mut stats, v).and_then(|e| e.encode()) {
                    Ok(ir) => irs.push(ir),
                    Err(e) => fail = Some(e.to_string()),
                }
            }
            if let Some(e) = fail {
                writeln!(out, "  lowering failed: {e}")?;
            } else {
                let (bank, sizes, _) = serialize(&irs, ctx.events.len(), ctx.entities.len(), ctx.strings.len(), 0)?;
                let rt: usize = sizes.iter().map(|s| s.runtime).sum();
                // Art: the clips the tree names, per animated object, and the frames they hold.
                let names: std::collections::HashSet<&str> = ctx.strings.keys().map(String::as_str).collect();
                let mut frames_total = 0usize;
                let mut distinct: BTreeSet<(i64, i64)> = BTreeSet::new();
                let mut libs = BTreeSet::new();
                for info in &file.objects {
                    if info.class_id != 114 {
                        continue;
                    }
                    let o = source.object(&file, info.path_id)?;
                    if !source.typename(&o).is_ok_and(|t| t == "tk2dSpriteAnimator") {
                        continue;
                    }
                    let t = source.read(&o)?;
                    let Some(go) = t.get("m_GameObject").and_then(Value::pptr).map(|p| p.1) else { continue };
                    if !subtree.contains(&go) {
                        continue;
                    }
                    let Some(lib) = t.get("library").filter(|l| l.pptr().is_some_and(|p| p.1 != 0)) else { continue };
                    let lo = source.deref(&file, lib)?;
                    libs.insert(lo.sid());
                    let library = source.read(&lo)?;
                    for clip in library.get("clips").and_then(Value::list).unwrap_or(&[]) {
                        let cname = clip.get("name").and_then(Value::str).unwrap_or_default();
                        if !names.contains(cname.as_str()) {
                            continue;
                        }
                        for f in clip.get("frames").and_then(Value::list).unwrap_or(&[]) {
                            frames_total += 1;
                            let coll = f.get("spriteCollection").and_then(Value::pptr).map_or(0, |p| p.1);
                            distinct.insert((coll, f.get("spriteId").and_then(Value::int).unwrap_or(-1)));
                        }
                    }
                }
                writeln!(out, "  art: {} libraries, {} frames in the clips the tree names, {} distinct sprites", libs.len(), frames_total, distinct.len())?;
                writeln!(out, "  HKFSM03 bank {} B, runtime block {} B, {} entities, {} events, {} strings; unsupported {:?}", bank.len(), rt, ctx.entities.len(), ctx.events.len(), ctx.strings.len(), stats.unsupported)?;
            }
        }
        writeln!(out, "  without a light handler ({}): {}", miss_light.len(), miss_light.join(", "))?;
        writeln!(out, "  without a boss-tier handler ({}): {}\n", miss_boss.len(), miss_boss.join(", "))?;
    }
    for (n, c) in &seen_names {
        if *c > 1 {
            writeln!(out, "({n}: {c} placements in this scene, first one shown)")?;
        }
    }
    print!("{out}");
    Ok(())
}
