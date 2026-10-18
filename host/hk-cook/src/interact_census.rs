//! Development reader for the nail-struck switches, gates, lifts and stags of a scene: which
//! GameObjects carry which PlayMaker FSMs, where they stand, and (with `--fsm NAME`) the whole
//! graph of each FSM of that name. The cookers in `interactables.rs` pin what they read against
//! this listing.
//!
//!   hk-cook interact-census SCENE... [--fsm NAME] [--like WORD]
//!
//! SCENE is a scene name (`Crossroads_03`) or a data file (`level133`).
use crate::breakables::{file_of, k, ki, kl};
use crate::common::{components, Result};
use crate::music::value_json;
use crate::pyjson::dumps_sorted_compact;
use hk_unity::playmaker::{action_fields_with_plain_strings, enabled};
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn file_for(source: &Source, name: &str) -> String {
    crate::quality::SCENE_TABLE
        .iter()
        .find(|r| r.scene_name == name)
        .map(|r| r.file.to_owned())
        .or_else(|| hk_unity::scene::build_settings(source).ok().and_then(|m| m.get(name).cloned()))
        .unwrap_or_else(|| name.to_owned())
}

fn path_of(sc: &Scene, gid: i64) -> String {
    let mut names = Vec::new();
    let mut at = gid;
    while let Some(go) = sc.go(at) {
        names.push(go.get("m_Name").and_then(Value::str).unwrap_or_default());
        let Some(&tid) = sc.go_transform.get(&at) else { break };
        let Some(father) = sc.transform(tid).and_then(|t| t.get("m_Father")).and_then(|f| f.get("m_PathID")).and_then(Value::int) else { break };
        if father == 0 {
            break;
        }
        match sc.transform(father).and_then(|t| t.get("m_GameObject")).and_then(|g| g.get("m_PathID")).and_then(Value::int) {
            Some(parent) => at = parent,
            None => break,
        }
    }
    names.reverse();
    names.join("/")
}

fn dump_fsm(fsm: &Value, out: &mut String) -> Result<()> {
    out.push_str(&format!("    start: {}\n", fsm.get("startState").and_then(Value::str).unwrap_or_default()));
    if let Some(Value::Map(groups)) = fsm.get("variables") {
        for (group, items) in groups {
            for v in items.list().unwrap_or(&[]) {
                if let (Some(name), Some(value)) = (v.get("name").and_then(Value::str), v.get("value")) {
                    if !name.is_empty() && !matches!(value, Value::Map(_)) {
                        out.push_str(&format!("    var {group}.{name} = {}\n", dumps_sorted_compact(&value_json(value))));
                    } else if !name.is_empty() {
                        out.push_str(&format!("    var {group}.{name} (ref)\n"));
                    }
                }
            }
        }
    }
    for t in fsm.get("globalTransitions").and_then(Value::list).unwrap_or(&[]) {
        let ev = t.get("fsmEvent").and_then(|e| e.get("name")).and_then(Value::str).unwrap_or_default();
        let to = t.get("toState").and_then(Value::str).unwrap_or_default();
        out.push_str(&format!("    global on {ev} -> {to}\n"));
    }
    for state in kl(fsm, "states")? {
        out.push_str(&format!("    state {}\n", state.get("name").and_then(Value::str).unwrap_or_default()));
        for t in state.get("transitions").and_then(Value::list).unwrap_or(&[]) {
            let ev = t.get("fsmEvent").and_then(|e| e.get("name")).and_then(Value::str).unwrap_or_default();
            let to = t.get("toState").and_then(Value::str).unwrap_or_default();
            out.push_str(&format!("      on {ev} -> {to}\n"));
        }
        let data = k(state, "actionData")?;
        for (name, index) in enabled(state) {
            let fields = action_fields_with_plain_strings(data, index, true).unwrap_or_default();
            let text: Vec<String> = fields
                .iter()
                .map(|(key, v)| format!("{key}={}", dumps_sorted_compact(&value_json(v))))
                .collect();
            out.push_str(&format!("      do {name}({})\n", text.join(", ")));
        }
    }
    Ok(())
}

pub fn main(root: &Path, args: &[String], source: Option<&Path>) -> std::result::Result<(), String> {
    let source = match source {
        Some(p) => Source::new(p),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let mut scenes: Vec<&String> = Vec::new();
    let mut wanted: Option<&str> = None;
    let mut like: Vec<&str> = Vec::new();
    let mut component: Option<&str> = None;
    let mut animated = false;
    let mut playerdata: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--fsm" => {
                wanted = args.get(i + 1).map(String::as_str);
                i += 1;
            }
            "--like" => {
                like.push(args.get(i + 1).map(String::as_str).unwrap_or(""));
                i += 1;
            }
            "--component" => {
                component = args.get(i + 1).map(String::as_str);
                i += 1;
            }
            "--animated" => animated = true,
            "--playerdata" => {
                playerdata.push(args.get(i + 1).map(String::as_str).unwrap_or(""));
                i += 1;
            }
            "--source" => i += 1,
            _ => scenes.push(&args[i]),
        }
        i += 1;
    }
    if !playerdata.is_empty() {
        let defaults = source.fresh_save().map_err(|e| e.to_string())?;
        for key in &playerdata {
            println!("PlayerData {key}: {:?}", defaults.get(*key));
        }
    }
    for name in scenes {
        let sc = Scene::new(&source, &file_for(&source, name)).map_err(|e| e.to_string())?;
        println!("== {name} ({})", file_of(&sc));
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for o in sc.objects.iter().filter(|o| o.typename == "PlayMakerFSM") {
            let own = k(&o.tree, "fsm")?;
            let mut template_name = String::new();
            let template_asset;
            let fsm = match o.tree.get("fsmTemplate").and_then(|t| t.get("m_PathID")).and_then(Value::int) {
                Some(id) if id != 0 => {
                    let obj = source.deref(&sc.base, o.tree.get("fsmTemplate").unwrap()).map_err(|e| e.to_string())?;
                    template_asset = source.read(&obj).map_err(|e| e.to_string())?;
                    template_name = template_asset.get("m_Name").and_then(Value::str).unwrap_or_default();
                    template_asset.get("fsm").unwrap_or(own)
                }
                _ => own,
            };
            let fname = own.get("name").and_then(Value::str).unwrap_or_default();
            *counts.entry(fname.clone()).or_default() += 1;
            let gid = o.tree.get("m_GameObject").and_then(|g| g.get("m_PathID")).and_then(Value::int).unwrap_or(0);
            let hit = match wanted {
                Some(w) => fname == w || template_name == w,
                None => like.iter().any(|w| fname.to_lowercase().contains(&w.to_lowercase()) || template_name.to_lowercase().contains(&w.to_lowercase())),
            };
            if !hit {
                continue;
            }
            let go_name = sc.go(gid).and_then(|g| g.get("m_Name")).and_then(Value::str).unwrap_or_default();
            let at = sc.point(gid, 0.0, 0.0, 0.0).map(|p| format!("({:.3}, {:.3}, {:.3})", p[0], p[1], p[2])).unwrap_or_default();
            let layer = sc.go(gid).and_then(|g| ki(g, "m_Layer").ok()).unwrap_or(-1);
            let active = sc.active(gid);
            let comps: Vec<String> = components(sc_ref(&sc), gid)
                .map(|c| c.iter().map(|x| x.1.to_owned()).collect())
                .unwrap_or_default();
            println!(
                "  fsm {fname:?} template {template_name:?} id {} on {go_name:?} gid {gid} path {} at {at} layer {layer} active {active}\n    components {}",
                o.id,
                path_of(&sc, gid),
                comps.join(" ")
            );
            if wanted.is_some() {
                let mut text = String::new();
                dump_fsm(fsm, &mut text)?;
                print!("{text}");
            }
        }
        if animated {
            for o in sc.objects.iter().filter(|o| o.typename == "tk2dSpriteAnimator") {
                let gid = o.tree.get("m_GameObject").and_then(|g| g.get("m_PathID")).and_then(Value::int).unwrap_or(0);
                let go_name = sc.go(gid).and_then(|g| g.get("m_Name")).and_then(Value::str).unwrap_or_default();
                let at = sc.point(gid, 0.0, 0.0, 0.0).map(|p| format!("({:.2}, {:.2}, {:.2})", p[0], p[1], p[2])).unwrap_or_default();
                let layer = sc.go(gid).and_then(|g| ki(g, "m_Layer").ok()).unwrap_or(-1);
                let default = o.tree.get("library").and_then(|l| source.deref(&sc.base, l).ok()).and_then(|lo| source.read(&lo).ok()).map(|lib| {
                    let id = o.tree.get("defaultClipId").and_then(Value::int).unwrap_or(-1);
                    let names: Vec<String> = lib.get("clips").and_then(Value::list).unwrap_or(&[]).iter().map(|c| c.get("name").and_then(Value::str).unwrap_or_default()).collect();
                    format!("default {:?} of {}", names.get(id as usize), names.join("|"))
                }).unwrap_or_default();
                println!("  animator id {} on {go_name:?} gid {gid} active {} layer {layer} at {at} path {} {default}", o.id, sc.active(gid), path_of(&sc, gid));
            }
        }
        if let Some(name) = component {
            for o in sc.objects.iter().filter(|o| o.typename == name) {
                let gid = o.tree.get("m_GameObject").and_then(|g| g.get("m_PathID")).and_then(Value::int).unwrap_or(0);
                let go_name = sc.go(gid).and_then(|g| g.get("m_Name")).and_then(Value::str).unwrap_or_default();
                let at = sc.point(gid, 0.0, 0.0, 0.0).map(|p| format!("({:.3}, {:.3}, {:.3})", p[0], p[1], p[2])).unwrap_or_default();
                println!("  {name} id {} on {go_name:?} gid {gid} at {at} path {}: {}", o.id, path_of(&sc, gid), dumps_sorted_compact(&value_json(&o.tree)));
            }
        }
        if wanted.is_none() && like.is_empty() && component.is_none() {
            for (n, c) in &counts {
                println!("  {c:3} x {n}");
            }
        }
    }
    Ok(())
}

fn sc_ref<'a, 's>(sc: &'a Scene<'s>) -> &'a Scene<'s> {
    sc
}
