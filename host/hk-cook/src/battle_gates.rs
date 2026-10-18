//! Source `BG Control` arena gates, joined to the terrain edges the cook baked
//! (the Rust port of host/battle_gates.py).
//!
//! `breakables::battle_gate` reads the one decision a gate makes on load:
//! `Opened` tests `Start Closed`, and a gate that has it set turns its box back
//! on in `Quick Close` and then waits for the `BG OPEN` its arena sends when the
//! fight is won. The region cook bakes every gate's collider as terrain whatever
//! that verdict says, so the room pack holds an edge for all of them.
//!
//! `main` is the join that lets the runtime move them: for every catalogue slot,
//! the edge indices in that slot's room pack that belong to a gate collider,
//! keyed by gate. `game/src/battle_gates.rs` excludes the edges of every gate
//! that is currently open, and it starts from the gates that are open on load,
//! which is the complement of `PLACEMENT_CLOSED`. Both directions of that join
//! have to hold. A missing row is an invisible wall, so `bind` derives the rows
//! from the cook's own `edge_sources` and never from a second copy of the rule,
//! and `main` reports any gate the cook baked no terrain for at all.
//!
//! `main` reads `data/regions.json` and rewrites `data/battle_gates.rs` and
//! `.hkpsx/battle-gates.json`. The table is only as current as the report.
//!
//! The gate art is the other half. The region postpass (host/regions.py
//! `postpack_actor_bank`) puts the gate's five clips and the slam flash in the
//! scene actor bank; `cook_art` is the cooker for that bank entry. It runs before
//! the region cook (it reads the source scenes only), cuts the sprites out of the
//! tk2d collection, scales them to the gameplay plane, packs them into 64x64
//! sheets and leaves the sheets and the clip tables in `.hkpsx/sheet-art/`, which
//! the postpass appends to the atlas as streamed frames.
use crate::actor_art::guest_wrap_of;
use crate::breakables::{battle_gates, file_of, jget, jint, jstr, k, ki, kl};
use crate::common::{components, err, py_round, Result};
use crate::const_eval::rust_constants;
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::false_knight::fsm_digest;
use crate::pyjson::{dumps, Json};
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use serde_json::Value as J;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// `game/src/battle_gates.rs` keeps the closed set in one u32, which is the whole
/// reason the runtime costs a single test before it looks at anything.
pub const MAX_GATES: usize = 32;

/// Fungus1_21's `Battle Gate Bone`: BG Control with its own tk2d clips, so its
/// digest differs from the family's and `battle_gates` refuses it. It sits on
/// layer 25, not the terrain layer, so the cook bakes no edge for it and there is
/// no collider here to open or close: the variant is recognised only so that it
/// is not mistaken for an unreadable gate.
const BONE_GATE_SHA256: &str = "525bab5a6c08ab2800b7909b6a564f2da7c0dedea43d10ea5f23ffe41d6bc8bc";
const PLACEMENT_VARIABLE: &str = "Start Closed";

/// The clip each FSM state plays, in the order `game/src/battle_gates.rs`
/// indexes them. `Opened` and `Quick Open` play `BG Opened`; `Quick Close` and
/// `Double Close` play `BG Closed`; `Close 1` plays `BG Close 1` and, when that
/// clip finishes, `Close 2` plays `BG Close 2`; `Open` plays `BG Open`.
pub const GATE_CLIPS: [&str; 5] = ["BG Opened", "BG Close 1", "BG Close 2", "BG Closed", "BG Open"];
/// What the runtime assumes of them: one held frame each for the two poses, and
/// play-once clips for the three transitions (tk2d wrap modes 6 Single and 2
/// Once), so a clip's last frame is the pose it leaves the gate in.
const GATE_CLIP_WRAPS: [i64; 5] = [6, 2, 2, 2, 2];

/// `Close 2` restarts the gate's `Close Effect` child on frame 0 of its clip,
/// `BG Effect` (4 frames at 12 fps, played once), and turns its renderer on.
const SLAM_EFFECT_CHILD: &str = "Close Effect";
const SLAM_EFFECT_CLIP: &str = "BG Effect";
const SLAM_EFFECT_EMPTY: f64 = 0.05;
const SLAM_EFFECT_DENSITY: f64 = 0.8;
/// The transparent texels between packed sprites: twice the quantizer's edge
/// reach (`quantize::EDGE_REACH`), so the texels either sprite's edge
/// classification looks at are all its own side of the gap.
const SHEET_GAP: usize = 2 * crate::atlas::EDGE_REACH;
const SHEET_SLOT: usize = 64;

/// One arena gate of a scene.
#[derive(Clone, Debug)]
pub struct Gate {
    pub gid: i64,
    pub name: String,
    pub source: String,
    pub collider_source: String,
    pub solid_on_load: bool,
    pub art: bool,
    pub box_: [f64; 4],
    pub position: [f64; 3],
}

/// Python's `round(x, 4)`: the correctly rounded decimal, read back.
fn round4(x: f64) -> f64 {
    format!("{x:.4}").parse().unwrap_or(x)
}

fn jfloat(j: &Json) -> f64 {
    match j {
        Json::Float(f) => *f,
        Json::Int(i) => *i as f64,
        _ => 0.0,
    }
}

fn jfloats_of(j: Option<&Json>) -> Vec<f64> {
    match j {
        Some(Json::List(l)) => l.iter().map(jfloat).collect(),
        _ => vec![],
    }
}

/// `_upright`: whether the gate is drawn as the runtime draws every gate: upright,
/// unscaled, untinted, with its sprite, animator and an enabled renderer.
fn upright(sc: &Scene, gid: i64) -> Result<bool> {
    let found = components(sc, gid)?;
    let find = |t: &str| found.iter().rev().find(|c| c.1 == t).map(|c| c.2);
    let (Some(sprite), Some(_animator), Some(renderer)) =
        (find("tk2dSprite"), find("tk2dSpriteAnimator"), find("MeshRenderer"))
    else {
        return Ok(false);
    };
    if !k(renderer, "m_Enabled")?.truthy() {
        return Ok(false);
    }
    let matrix = sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?).map_err(|e| e.to_string())?;
    for i in 0..2 {
        for j in 0..2 {
            if (matrix[i][j] - if i == j { 1.0 } else { 0.0 }).abs() > 1e-6 {
                return Ok(false);
            }
        }
    }
    let scale = k(sprite, "_scale")?;
    let color = k(sprite, "_color")?;
    for key in ["x", "y"] {
        if (crate::breakables::kf(scale, key)? - 1.0).abs() > 1e-6 {
            return Ok(false);
        }
    }
    for key in ["r", "g", "b", "a"] {
        if (crate::breakables::kf(color, key)? - 1.0).abs() > 1e-6 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `all_gates`: every active arena gate of the scene that is terrain: the family's.
///
/// The Bone variant is dropped; any other refusal goes to `errors`, or fails
/// when there is none.
pub fn all_gates(sc: &Scene, mut errors: Option<&mut Vec<Json>>) -> Result<Vec<Gate>> {
    let mut refused: Vec<Json> = Vec::new();
    let found = battle_gates(sc, Some(&mut refused))?;
    let mut out = Vec::new();
    for record in &found {
        let gid = jint(jget(record, "gid").ok_or("gate without gid")?);
        out.push(Gate {
            gid,
            name: jstr(jget(record, "name").ok_or("gate without name")?),
            source: jstr(jget(record, "source").ok_or("gate without source")?),
            collider_source: jstr(jget(record, "collider_source").ok_or("gate without collider")?),
            solid_on_load: matches!(jget(record, "solid_on_load"), Some(Json::Bool(true))),
            art: upright(sc, gid)?,
            box_: {
                let b = jfloats_of(jget(record, "box"));
                [b[0], b[1], b[2], b[3]]
            },
            position: {
                let p = jfloats_of(jget(record, "position"));
                [p[0], p[1], p[2]]
            },
        });
    }
    for error in refused {
        let id = jstr(jget(&error, "id").ok_or("refusal without id")?);
        let object: i64 = id.rsplit(':').next().and_then(|s| s.parse().ok()).ok_or("refusal id")?;
        let tree = &sc.object(object).ok_or("refused object")?.tree;
        if fsm_digest(k(tree, "fsm")?, &[PLACEMENT_VARIABLE])? == BONE_GATE_SHA256 {
            continue;
        }
        match errors.as_deref_mut() {
            None => return err(jstr(jget(&error, "error").ok_or("refusal without error")?)),
            Some(list) => list.push(error),
        }
    }
    Ok(out)
}

/// `edge_scratch_slots`: the shared exclusion scratch, read out of the guest.
pub fn edge_scratch_slots(root: &Path) -> Result<usize> {
    let text = std::fs::read_to_string(root.join("game/src/world.rs")).map_err(|e| e.to_string())?;
    rust_constants(&text)
        .into_iter()
        .find(|(n, _)| n == "SCRIPT_EDGE_SLOTS")
        .and_then(|(_, v)| v.as_f64())
        .map(|v| v as usize)
        .ok_or_else(|| "cannot read SCRIPT_EDGE_SLOTS out of game/src/world.rs".to_string())
}

/// The source without comments or whitespace and without a trailing comma before a closing
/// bracket (host/rustsrc.py `compact`, for the generated data tables which hold no strings).
fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("");
        out.extend(line.chars().filter(|c| !c.is_whitespace()));
    }
    let mut cleaned = String::with_capacity(out.len());
    let chars: Vec<char> = out.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == ',' && matches!(chars.get(i + 1), Some(')' | ']' | '}')) {
            continue;
        }
        cleaned.push(c);
    }
    cleaned
}

fn count(body: &str) -> usize {
    body.split(',').filter(|v| !v.trim().is_empty()).count()
}

/// Every `(slot,Binding{<head>...],edges:&[body]})` of a compacted table: (slot, edge count).
fn binding_counts(text: &str, head: &str) -> Vec<(usize, usize)> {
    let marker = format!(",Binding{{{head}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(&marker) {
        let at = from + rel;
        from = at + marker.len();
        // `(` digits before the comma.
        let before = &text[..at];
        let digits: String = before.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        if digits.is_empty() || !before[..before.len() - digits.len()].ends_with('(') {
            continue;
        }
        // After the head: [^\]]* then `],edges:&[` body `]})`.
        let rest = &text[from..];
        let Some(close) = rest.find(']') else { continue };
        let tail = &rest[close..];
        let Some(tail) = tail.strip_prefix("],edges:&[") else { continue };
        let Some(end) = tail.find(']') else { continue };
        if !tail[end..].starts_with("]})") {
            continue;
        }
        out.push((digits.parse().unwrap(), count(&tail[..end])));
    }
    out
}

/// `neighbour_edges`: exclusions per catalogue slot that the other controllers already own.
///
/// The Lifeblood cocoons and the Great Door share the scratch, so the budget is the union in one
/// slot rather than this table's own rows. Read off the generated files the guest links.
pub fn neighbour_edges(root: &Path) -> HashMap<usize, usize> {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    let read = |name: &str| std::fs::read_to_string(root.join(name)).ok().map(|t| compact(&t));
    if let Some(text) = read("data/lifeblood.rs") {
        for (slot, n) in binding_counts(&text, "off:&[") {
            *counts.entry(slot).or_default() += n;
        }
    }
    if let Some(text) = read("data/great_door.rs") {
        for (slot, n) in binding_counts(&text, "frames:[") {
            *counts.entry(slot).or_default() += n;
        }
    }
    // The False Knight's broken floor lifts `Break Floor` through the same scratch, and on a won
    // arena it does so with every gate open.
    if let Some(text) = read("data/false_knight_art.rs") {
        if let Some(at) = text.find("FK_BREAK_FLOOR_EDGES:[(u16,u16);") {
            if let Some(eq) = text[at..].find("]=[") {
                let body = &text[at + eq + 3..];
                let body = &body[..body.find(']').unwrap_or(body.len())];
                for pair in body.split("),(") {
                    let pair = pair.trim_start_matches('(').trim_end_matches(')');
                    if let Some((slot, edge)) = pair.split_once(',') {
                        if let (Ok(slot), Ok(_)) = (slot.parse::<usize>(), edge.parse::<usize>()) {
                            *counts.entry(slot).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    counts
}

/// A scene of the region report.
struct SceneMeta {
    id: usize,
    name: String,
    file: String,
}

fn scenes_of(report: &J) -> Result<Vec<SceneMeta>> {
    let mut scenes: Vec<SceneMeta> = report["scenes"]
        .as_array()
        .ok_or("report without scenes")?
        .iter()
        .map(|s| {
            Ok(SceneMeta {
                id: s["scene_id"].as_u64().ok_or("scene_id")? as usize,
                name: s["scene_name"].as_str().ok_or("scene_name")?.to_owned(),
                file: s["file"].as_str().ok_or("scene file")?.to_owned(),
            })
        })
        .collect::<Result<_>>()?;
    scenes.sort_by_key(|s| s.id);
    Ok(scenes)
}

/// A surveyed gate with its scene.
#[derive(Clone, Debug)]
pub struct Surveyed {
    pub scene: usize,
    pub scene_name: String,
    pub gate: Gate,
}

/// `survey`: every admitted scene's `BG Control` gates, in a stable order.
///
/// Ordered by (scene id, source object id) rather than by name: two gates in one room share the
/// name `Battle Gate 2` up to a suffix, and the bit a gate owns has to survive a rename.
fn survey(source: &Source, report: &J) -> Result<Vec<Surveyed>> {
    let mut gates = Vec::new();
    let mut refused = Vec::new();
    for meta in scenes_of(report)? {
        let sc = Scene::new(source, &meta.file).map_err(|e| e.to_string())?;
        let mut errors = Vec::new();
        let mut found = all_gates(&sc, Some(&mut errors))?;
        for e in errors {
            refused.push(format!("{}: {}", meta.name, dumps(&e)));
        }
        found.sort_by_key(|g| g.source.rsplit(':').next().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0));
        for gate in found {
            gates.push(Surveyed { scene: meta.id, scene_name: meta.name.clone(), gate });
        }
    }
    if !refused.is_empty() {
        return err(format!("unreadable BG Control gates: {refused:?}"));
    }
    if gates.len() > MAX_GATES {
        return err(format!("{} arena gates exceed the {MAX_GATES}-bit closed set", gates.len()));
    }
    Ok(gates)
}

/// (catalogue slot, gate, edges) for every gate the cook baked as terrain.
///
/// `edge_sources` is the cook's own join key: it names, per cooked edge, the source collider the
/// edge came out of. Reading the answer off the cook's output rather than re-deriving it is the
/// point: a row that disagrees with the pack is an invisible wall now that the open gates are
/// baked too.
fn bind(root: &Path, gates: &[Surveyed], report: &J) -> Result<Vec<(usize, usize, Vec<usize>)>> {
    let mut owner: HashMap<&str, usize> = HashMap::new();
    for (index, g) in gates.iter().enumerate() {
        owner.insert(g.gate.collider_source.as_str(), index);
    }
    if owner.len() != gates.len() {
        return err("two arena gates share one collider");
    }
    let budget = edge_scratch_slots(root)?;
    let neighbours = neighbour_edges(root);
    let mut rows = Vec::new();
    for (slot, region) in report["regions"].as_array().ok_or("report without regions")?.iter().enumerate() {
        let mut found: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
        let scene_id = region["scene_id"].as_u64().ok_or("scene_id")? as usize;
        for (edge, source) in region["edge_sources"].as_array().ok_or("edge_sources")?.iter().enumerate() {
            let Some(&index) = source.as_str().and_then(|s| owner.get(s)) else { continue };
            if gates[index].scene != scene_id {
                return err(format!("gate {} bound outside its own scene", gates[index].gate.name));
            }
            found.entry(index).or_default().push(edge);
        }
        let spent = found.values().map(Vec::len).sum::<usize>() + neighbours.get(&slot).copied().unwrap_or(0);
        if spent > budget {
            return err(format!(
                "catalogue slot {slot} ({}) needs {spent} scripted edge exclusions against {budget} in game/src/world.rs",
                region["scene_name"].as_str().unwrap_or("")
            ));
        }
        for (index, edges) in found {
            if slot >= 65536 {
                return err("arena gate binding slot exceeds u16");
            }
            rows.push((slot, index, edges));
        }
    }
    Ok(rows)
}

fn q16(v: f64) -> i64 {
    py_round(v * 65536.0)
}

fn mask(bits: u64, gates: usize) -> String {
    format!("0b{:0width$b}", bits, width = gates.max(1))
}

/// The art half of the table: gate positions, clips and each view's frames.
fn generate_art(gates: &[Surveyed], report: &J) -> Result<Vec<String>> {
    let names: Vec<&str> = GATE_CLIPS.iter().copied().chain([SLAM_EFFECT_CLIP]).collect();
    let mut art: Vec<(usize, &J)> = Vec::new();
    for (slot, row) in report["regions"].as_array().ok_or("regions")?.iter().enumerate() {
        if let Some(a) = row.get("battle_gate_art") {
            if !(a.is_null() || a.as_object().is_some_and(|o| o.is_empty())) {
                art.push((slot, a));
            }
        }
    }
    let empty: Vec<J> = names
        .iter()
        .map(|n| serde_json::json!({"name": n, "fps": 0, "frames": []}))
        .collect();
    let clips: Vec<J> = match art.first() {
        Some((_, a)) => a["clips"].as_array().ok_or("clips")?.clone(),
        None => empty,
    };
    if clips.iter().map(|c| c["name"].as_str().unwrap_or("")).collect::<Vec<_>>() != names {
        return err("gate art clips are not in the runtime order");
    }
    let first = art.first().map(|(_, a)| *a);
    let empty_first = serde_json::json!({"rects": [], "boxes": [], "sheets": 0, "effect_offset": [0, 0], "sprites": []});
    let first = first.unwrap_or(&empty_first);
    for (_, a) in &art {
        if a["clips"].as_array() != Some(&clips)
            || a["sprites"] != first["sprites"]
            || a["rects"] != first["rects"]
            || a["sheets"] != first["sheets"]
        {
            return err("views disagree about the gate frames");
        }
    }
    let fps_of = |c: &J| c["fps"].as_f64().unwrap_or(0.0);
    if clips.iter().any(|c| fps_of(c) != fps_of(c).trunc() || !(0.0..256.0).contains(&fps_of(c))) {
        return err("gate clip rate is not a whole number of frames per second");
    }
    for (slot, a) in &art {
        if *slot >= 65536 || a["frame_base"].as_u64().unwrap_or(0) + a["sheets"].as_u64().unwrap_or(0) >= 65536 {
            return err("gate art binding exceeds u16");
        }
    }
    let list = |v: &J| -> Vec<f64> { v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default() };
    let mut lines = vec![
        "/// Each gate's transform origin on the gameplay plane (q16 world units).".to_owned(),
        "/// host/battle_gates.py refuses a gate that is rotated, scaled or tinted.".to_owned(),
        format!(
            "pub static POSITION:[[i32;2];GATES]=[{}];",
            gates
                .iter()
                .map(|g| format!("[{},{}],", q16(g.gate.position[0]), q16(g.gate.position[1])))
                .collect::<String>()
        ),
        "/// `BG Opened`, `BG Close 1`, `BG Close 2`, `BG Closed`, `BG Open` and the".to_owned(),
        "/// slam effect's `BG Effect`: each clip's sprites (indices into SPRITE_RECT),".to_owned(),
        "/// and its rate in whole frames per second.".to_owned(),
        format!(
            "pub const CLIP_FRAMES:[&[u8];{}]=[{}];",
            clips.len(),
            clips
                .iter()
                .map(|c| format!(
                    "&[{}],",
                    c["frames"].as_array().map(|f| f.iter().map(|x| x.as_i64().unwrap_or(0).to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default()
                ))
                .collect::<String>()
        ),
        format!("pub const CLIP_FPS:[u8;{}]=[{}];", clips.len(), clips.iter().map(|c| format!("{},", fps_of(c) as i64)).collect::<String>()),
        "/// Each sprite's sheet and texel rectangle [sheet, x, y, w, h], and its box".to_owned(),
        "/// around its origin [x0, y0, x1, y1] (q16 units, the effect's already scaled).".to_owned(),
        format!(
            "pub static SPRITE_RECT:&[[u8;5]]=&[{}];",
            first["rects"]
                .as_array()
                .map(|a| a.iter().map(|r| format!("[{}],", r.as_array().unwrap().iter().map(|x| x.as_i64().unwrap_or(0).to_string()).collect::<Vec<_>>().join(","))).collect::<String>())
                .unwrap_or_default()
        ),
        format!(
            "pub static SPRITE_BOX:&[[i32;4]]=&[{}];",
            first["boxes"]
                .as_array()
                .map(|a| a.iter().map(|b| format!("[{}],", list(b).iter().map(|v| q16(*v).to_string()).collect::<Vec<_>>().join(","))).collect::<String>())
                .unwrap_or_default()
        ),
        "/// The sheets one view carries (consecutive frames from its ART entry).".to_owned(),
        format!("pub const SHEETS:usize={};", first["sheets"].as_u64().unwrap_or(0)),
        "/// The `Close Effect` child's offset from its gate (q16 units).".to_owned(),
    ];
    let offset = list(&first["effect_offset"]);
    lines.push(format!("pub const EFFECT_OFFSET:[i32;2]=[{},{}];", q16(offset[0]), q16(offset[1])));
    lines.push("/// (catalogue slot, the view's first gate sheet frame): every view of a".to_owned());
    lines.push("/// scene with gates carries the sheets in its actor bank, sorted by slot.".to_owned());
    lines.push(format!(
        "pub static ART:&[(u16,u16)]=&[{}];",
        art.iter().map(|(slot, a)| format!("({},{}),", slot, a["frame_base"].as_u64().unwrap_or(0))).collect::<String>()
    ));
    Ok(lines)
}

fn generate(gates: &[Surveyed], rows: &[(usize, usize, Vec<usize>)], report: &J) -> Result<String> {
    let n = gates.len();
    let mut cooked: Vec<usize> = rows.iter().map(|r| r.1).collect();
    cooked.sort_unstable();
    cooked.dedup();
    let mut scene_gates: std::collections::BTreeMap<usize, u64> = Default::default();
    for (index, g) in gates.iter().enumerate() {
        *scene_gates.entry(g.scene).or_default() |= 1 << index;
    }
    let placement = gates.iter().enumerate().filter(|(_, g)| g.gate.solid_on_load).fold(0u64, |a, (i, _)| a | 1 << i);
    let cooked_bits = cooked.iter().fold(0u64, |a, &i| a | 1 << i);
    let art_bits = gates.iter().enumerate().filter(|(_, g)| g.gate.art).fold(0u64, |a, (i, _)| a | 1 << i);
    let mut lines: Vec<String> = vec![
        "// Generated from the source `BG Control` gates and the cooked region edge".to_owned(),
        "// sources. host/hk-cook/src/battle_gates.rs is the generator.".to_owned(),
        "// Gate order is (scene id, source object id); a gate owns its bit for the".to_owned(),
        "// life of this table, so a rename in the source cannot move it.".to_owned(),
        format!("pub const GATES:usize={n};"),
        "/// The collider state `Opened` leaves each gate in on load: set means the".to_owned(),
        "/// gate carries `Start Closed`, turned its box back on in `Quick Close`, and".to_owned(),
        "/// is waiting for a `BG OPEN` its arena has not sent yet.".to_owned(),
        format!("pub const PLACEMENT_CLOSED:u32={};", mask(placement, n)),
        "/// The gates whose collider reached a room pack as terrain. The region cook".to_owned(),
        "/// bakes every gate, so this is normally all of them; a gate outside every".to_owned(),
        "/// region's collision bounds would be the one exception, and closing it".to_owned(),
        "/// would move nothing because there is no edge of it anywhere to restore.".to_owned(),
        format!("pub const COOKED:u32={};", mask(cooked_bits, n)),
        "/// The gates with the family's clip set and an upright, unscaled sprite: the".to_owned(),
        "/// ones the gate draw may put on screen. The rest are colliders only.".to_owned(),
        format!("pub const ART_MASK:u32={};", mask(art_bits, n)),
        "/// Guest scene id and the gates a `Battle Scene` there broadcasts to. The".to_owned(),
        "/// source's BG CLOSE/BG OPEN reach every gate in the room, not only the two".to_owned(),
        "/// at the arena ends, so the mask is per scene rather than per arena.".to_owned(),
        format!(
            "pub static SCENE_GATES:&[(u8,u32)]=&[{}];",
            scene_gates.iter().map(|(scene, bits)| format!("({},{}),", scene, mask(*bits, n))).collect::<String>()
        ),
        "/// (catalogue slot, gate, that gate's terrain edges in the slot's room".to_owned(),
        "/// pack), sorted by slot then gate so the runtime can binary search it.".to_owned(),
        format!(
            "pub static REGIONS:&[(u16,u8,&[u16])]=&[{}];",
            rows.iter()
                .map(|(slot, index, edges)| format!("({},{},&[{}]),", slot, index, edges.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(",")))
                .collect::<String>()
        ),
    ];
    lines.extend(generate_art(gates, report)?);
    Ok(lines.join("\n") + "\n")
}

/// The generator: `data/battle_gates.rs` and `.hkpsx/battle-gates.json`, from the region report.
pub fn main(root: &Path, source: Option<&Path>) -> Result<()> {
    let source = match source {
        Some(p) => Source::new(p),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let report: J = serde_json::from_slice(&std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let gates = survey(&source, &report)?;
    let rows = bind(root, &gates, &report)?;
    std::fs::write(root.join("data/battle_gates.rs"), generate(&gates, &rows, &report)?).map_err(|e| e.to_string())?;
    // A gate with no row carries no terrain anywhere, so its arena cannot seal at that end. That is
    // not a refusal, because it is a property of the source placement against the region bounds
    // rather than of this join, but it is the one thing a reader of this table would otherwise
    // have to derive.
    let bound: std::collections::BTreeSet<usize> = rows.iter().map(|r| r.1).collect();
    let mut limitations: Vec<String> = (0..gates.len())
        .filter(|i| !bound.contains(i))
        .map(|i| {
            format!(
                "{} {} has no cooked terrain in any region, so BG CLOSE cannot seal that end of its arena.",
                gates[i].scene_name, gates[i].gate.name
            )
        })
        .collect();
    limitations.push(
        "The gate sprite and its five clips are drawn; the slam effect, dust and camera shake of Close 2 and the raise dust of Open are not reproduced."
            .to_owned(),
    );
    let fl = |v: &[f64]| Json::List(v.iter().map(|f| Json::Float(*f)).collect());
    let record = Json::Obj(vec![
        (
            "gates".into(),
            Json::List(
                gates
                    .iter()
                    .map(|g| {
                        Json::Obj(vec![
                            ("scene".into(), Json::Int(g.scene as i64)),
                            ("scene_name".into(), Json::Str(g.scene_name.clone())),
                            ("name".into(), Json::Str(g.gate.name.clone())),
                            ("source".into(), Json::Str(g.gate.source.clone())),
                            ("collider_source".into(), Json::Str(g.gate.collider_source.clone())),
                            ("solid_on_load".into(), Json::Bool(g.gate.solid_on_load)),
                            ("art".into(), Json::Bool(g.gate.art)),
                            ("box".into(), fl(&g.gate.box_)),
                            ("position".into(), fl(&g.gate.position)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "bindings".into(),
            Json::List(
                rows.iter()
                    .map(|(slot, index, edges)| {
                        Json::Obj(vec![
                            ("slot".into(), Json::Int(*slot as i64)),
                            ("gate".into(), Json::Int(*index as i64)),
                            ("name".into(), Json::Str(gates[*index].gate.name.clone())),
                            ("scene_name".into(), Json::Str(gates[*index].scene_name.clone())),
                            ("edges".into(), Json::List(edges.iter().map(|e| Json::Int(*e as i64)).collect())),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("limitations".into(), Json::List(limitations.iter().map(|s| Json::Str(s.clone())).collect())),
    ]);
    std::fs::write(root.join(".hkpsx/battle-gates.json"), dumps(&record) + "\n").map_err(|e| e.to_string())?;
    let mut cooked: Vec<String> = rows.iter().map(|r| gates[r.1].gate.name.clone()).collect();
    cooked.sort();
    cooked.dedup();
    println!("{} arena gates, {} cooked as terrain, {} region bindings: {:?}", gates.len(), cooked.len(), rows.len(), cooked);
    for line in &limitations {
        println!("  note: {line}");
    }
    Ok(())
}

// ------------------------------------------------------------------ gate art

struct Sized {
    image: Image,
    box_: [f64; 4],
}

/// The sheets a scene's gate art cooks to.
pub struct GateArt {
    pub scene: String,
    pub sheets: Vec<Image>,
    pub sprites: Vec<i64>,
    pub clips: Vec<(String, f64, Vec<usize>)>,
    pub rects: Vec<[usize; 5]>,
    pub boxes: Vec<[f64; 4]>,
    pub effect_offset: [f64; 2],
    pub collection: String,
    pub library: String,
}

fn gate_parts(source: &Source, sc: &Scene, gid: i64) -> Result<(Obj, Obj)> {
    let found = components(sc, gid)?;
    let find = |t: &str| found.iter().rev().find(|c| c.1 == t).map(|c| c.2);
    let (Some(sprite), Some(animator), Some(renderer)) =
        (find("tk2dSprite"), find("tk2dSpriteAnimator"), find("MeshRenderer"))
    else {
        return err("arena gate without its tk2dSprite, animator and renderer");
    };
    if !k(renderer, "m_Enabled")?.truthy() {
        return err("arena gate renderer is disabled");
    }
    if !upright(sc, gid)? {
        // Every gate in the shipped scenes is upright, unscaled and untinted; the runtime draws
        // exactly that, so anything else is refused here.
        return err("arena gate is rotated, scaled or tinted");
    }
    let collection = source.deref(&sc.base, k(sprite, "collection")?).map_err(|e| e.to_string())?;
    let library = source.deref(&sc.base, k(animator, "library")?).map_err(|e| e.to_string())?;
    Ok((collection, library))
}

fn clip_named<'a>(library: &'a Value, name: &str) -> Option<&'a Value> {
    library.get("clips")?.list()?.iter().rev().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name))
}

struct Slam {
    sprites: Vec<i64>,
    scale: f64,
    offset: [f64; 2],
    fps: f64,
}

/// `slam_effect`: the slam flash every gate shares: its sprites, scale, rate and offset.
fn slam_effect(source: &Source, sc: &Scene, gates: &[&Gate], library_o: &Obj, collection_o: &Obj, library: &Value) -> Result<Slam> {
    let mut found: Vec<(f64, f64, f64)> = Vec::new();
    for g in gates {
        let tid = *sc.go_transform.get(&g.gid).ok_or("gate transform")?;
        let transform = sc.transform(tid).ok_or("gate transform tree")?;
        let mut children = Vec::new();
        for c in kl(transform, "m_Children")? {
            let child_transform = sc.transform(ki(c, "m_PathID")?).ok_or("child transform")?;
            children.push(ki(k(child_transform, "m_GameObject")?, "m_PathID")?);
        }
        let named: Vec<i64> = children
            .into_iter()
            .filter(|c| sc.go(*c).and_then(|go| go.get("m_Name")).and_then(Value::str).as_deref() == Some(SLAM_EFFECT_CHILD))
            .collect();
        if named.len() != 1 {
            return err(format!("arena gate {} has {} {SLAM_EFFECT_CHILD:?} children", g.name, named.len()));
        }
        let local = sc.transform(*sc.go_transform.get(&named[0]).ok_or("child go transform")?).ok_or("child tree")?;
        let comps = components(sc, named[0])?;
        let find = |t: &str| comps.iter().rev().find(|c| c.1 == t).map(|c| c.2);
        let (Some(animator), Some(sprite)) = (find("tk2dSpriteAnimator"), find("tk2dSprite")) else {
            return err("slam effect without sprite and animator");
        };
        let lib = source.deref(&sc.base, k(animator, "library")?).map_err(|e| e.to_string())?;
        if lib.sid() != library_o.sid() {
            return err("slam effect animates from another library");
        }
        let scale = k(sprite, "_scale")?;
        let ls = k(local, "m_LocalScale")?;
        if (crate::breakables::kf(scale, "x")? - 1.0).abs() > 1e-6
            || (crate::breakables::kf(scale, "y")? - 1.0).abs() > 1e-6
            || (crate::breakables::kf(ls, "x")? - crate::breakables::kf(ls, "y")?).abs() > 1e-6
            || crate::breakables::kf(k(local, "m_LocalRotation")?, "z")?.abs() > 1e-6
        {
            return err("slam effect is scaled unevenly or rotated");
        }
        let lp = k(local, "m_LocalPosition")?;
        let key = (round4(crate::breakables::kf(lp, "x")?), round4(crate::breakables::kf(lp, "y")?), round4(crate::breakables::kf(ls, "x")?));
        if !found.contains(&key) {
            found.push(key);
        }
    }
    if found.len() != 1 {
        return err(format!("arena gates place their slam effect differently: {found:?}"));
    }
    let (x, y, scale) = found[0];
    let clip = clip_named(library, SLAM_EFFECT_CLIP).ok_or("arena gate library has no 'BG Effect'")?;
    if clip.get("wrapMode").and_then(Value::int) != Some(2) {
        return err("slam effect clip is no longer play-once");
    }
    let collection = source.read(collection_o).map_err(|e| e.to_string())?;
    let definitions = collection.get("spriteDefinitions").and_then(Value::list).ok_or("spriteDefinitions")?;
    let mut sprites = Vec::new();
    for frame in clip.get("frames").and_then(Value::list).unwrap_or(&[]) {
        let id = ki(frame, "spriteId")?;
        let positions = definitions.get(id as usize).and_then(|d| d.get("positions")).and_then(Value::list).ok_or("sprite positions")?;
        let xs: Vec<f64> = positions.iter().filter_map(|p| p.get("x").and_then(Value::float)).collect();
        let (lo, hi) = (crate::common::py_min(xs.iter().copied()), crate::common::py_max(xs.iter().copied()));
        if (hi - lo) * scale < SLAM_EFFECT_EMPTY {
            break;
        }
        sprites.push(id);
    }
    Ok(Slam { sprites, scale, offset: [x, y], fps: clip.get("fps").and_then(Value::float).unwrap_or(0.0) })
}

/// `gate_sheets`: sprites at gameplay-plane scale, packed into as few slot-sized sheets as fit.
///
/// `sprites` is [(sprite id, world scale, texel density)]: the box is scaled by the first, the
/// texels by both. Columns, tallest first; a column takes further sprites below while they fit, a
/// sheet takes further columns while they fit, and a new sheet starts when neither does.
fn gate_sheets(
    source: &Source,
    collection_o: &Obj,
    collection: &Value,
    sprites: &[(i64, f64, f64)],
    scale: f64,
) -> Result<(Vec<Image>, Vec<[usize; 5]>, Vec<[f64; 4]>)> {
    let slot = SHEET_SLOT;
    let mut textures: HashMap<String, Arc<Image>> = HashMap::new();
    let mut sized: Vec<Sized> = Vec::new();
    for &(index, k, density) in sprites {
        let (image, box_) = tk_sprite(source, &collection_o.file, collection, index as usize, &mut textures)?;
        let b = [box_[0] * k, box_[1] * k, box_[2] * k, box_[3] * k];
        let w = (((b[2] - b[0]) * scale * density).ceil() as usize).max(1);
        let h = (((b[3] - b[1]) * scale * density).ceil() as usize).max(1);
        if w > slot || h > slot {
            return err(format!("arena gate sprite {index} is {w}x{h}, past one {slot}x{slot} slot"));
        }
        sized.push(Sized { image: image.to_rgba().resize(w, h, Filter::Lanczos), box_: b });
    }
    let gap = SHEET_GAP;
    let mut order: Vec<usize> = (0..sized.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(sized[i].image.height), i));

    // Columns of tall sprites, each taking more below while they fit.
    let columns = || -> (usize, Vec<(usize, usize, usize)>) {
        let mut sheets: Vec<Vec<[usize; 3]>> = Vec::new();
        let mut placed = vec![(0, 0, 0); sized.len()];
        for &i in &order {
            let im = &sized[i].image;
            let mut spot: Option<(usize, usize)> = None;
            for (n, cols) in sheets.iter_mut().enumerate() {
                let mut col = cols.iter().position(|c| c[2] + im.height <= slot && im.width <= c[1]);
                if col.is_none() {
                    let last = cols.last().unwrap();
                    let x = last[0] + last[1] + gap;
                    if x + im.width <= slot {
                        cols.push([x, im.width, 0]);
                        col = Some(cols.len() - 1);
                    }
                }
                if let Some(c) = col {
                    spot = Some((n, c));
                    break;
                }
            }
            let (n, c) = match spot {
                Some(s) => s,
                None => {
                    sheets.push(vec![[0, im.width, 0]]);
                    (sheets.len() - 1, 0)
                }
            };
            let col = &mut sheets[n][c];
            placed[i] = (n, col[0], col[2]);
            col[2] += im.height + gap;
        }
        (sheets.len(), placed)
    };
    // Rows of small sprites, left to right, a new row when one is full.
    let shelves = || -> (usize, Vec<(usize, usize, usize)>) {
        let mut sheets: Vec<[usize; 3]> = Vec::new(); // per sheet: [y of the current row, its height, next x]
        let mut placed = vec![(0, 0, 0); sized.len()];
        for &i in &order {
            let im = &sized[i].image;
            let mut spot: Option<usize> = None;
            for (n, row) in sheets.iter_mut().enumerate() {
                if row[2] + im.width <= slot && im.height <= row[1] {
                    spot = Some(n);
                    break;
                }
                let y = row[0] + row[1] + gap;
                if y + im.height <= slot {
                    *row = [y, im.height, 0];
                    spot = Some(n);
                    break;
                }
            }
            let spot = match spot {
                Some(s) => s,
                None => {
                    sheets.push([0, im.height, 0]);
                    sheets.len() - 1
                }
            };
            let row = &mut sheets[spot];
            placed[i] = (spot, row[2], row[0]);
            row[2] += im.width + gap;
        }
        (sheets.len(), placed)
    };
    let (by_columns, by_shelves) = (columns(), shelves());
    let (count, placed) = if by_shelves.0 < by_columns.0 { by_shelves } else { by_columns };
    let mut images = Vec::new();
    for n in 0..count {
        let mine: Vec<usize> = order.iter().copied().filter(|&i| placed[i].0 == n).collect();
        let width = mine.iter().map(|&i| placed[i].1 + sized[i].image.width).max().unwrap_or(1);
        let height = mine.iter().map(|&i| placed[i].2 + sized[i].image.height).max().unwrap_or(1);
        let mut sheet = Image::new(Mode::Rgba, width, height);
        for &i in &mine {
            sheet.paste(&sized[i].image, placed[i].1 as i64, placed[i].2 as i64, None);
        }
        images.push(sheet);
    }
    let rects = (0..sized.len()).map(|i| [placed[i].0, placed[i].1, placed[i].2, sized[i].image.width, sized[i].image.height]).collect();
    Ok((images, rects, sized.iter().map(|s| s.box_).collect()))
}

/// `append_art`'s cooking half: the gate clips' frames of a scene, or `None` without gates.
///
/// Every gate in the catalogue shares one collection and one animation library (refused
/// otherwise), so a scene cooks the frames once, whatever its gate count, and the gates differ
/// only in position and state.
pub fn scene_art(source: &Source, sc: &Scene) -> Result<Option<GateArt>> {
    // A variant without a cooked clip set (Fungus1_21's Bone Gate) is a collider only.
    let all = all_gates(sc, None)?;
    let gates: Vec<&Gate> = all.iter().filter(|g| g.art).collect();
    if gates.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::new();
    for g in &gates {
        parts.push(gate_parts(source, sc, g.gid)?);
    }
    let ids: std::collections::BTreeSet<(String, String)> = parts.iter().map(|(c, l)| (c.sid(), l.sid())).collect();
    if ids.len() != 1 {
        return err("arena gates in one scene use different art");
    }
    let (collection_o, library_o) = parts.remove(0);
    let collection = source.read(&collection_o).map_err(|e| e.to_string())?;
    let library = source.read(&library_o).map_err(|e| e.to_string())?;
    let mut order: Vec<i64> = Vec::new();
    let mut table: Vec<(String, f64, Vec<usize>)> = Vec::new();
    for (name, wrap) in GATE_CLIPS.iter().zip(GATE_CLIP_WRAPS) {
        let clip = clip_named(&library, name).ok_or_else(|| format!("arena gate library has no {name:?}"))?;
        let frames = clip.get("frames").and_then(Value::list).unwrap_or(&[]);
        let mode = clip.get("wrapMode").and_then(Value::int).unwrap_or(-1);
        if mode != wrap || (mode == 6 && frames.len() != 1) {
            return err(format!("arena gate clip {name:?} is no longer a held pose or a play-once clip"));
        }
        let mut ids = Vec::new();
        for frame in frames {
            let from = source.deref(&library_o.file, k(frame, "spriteCollection")?).map_err(|e| e.to_string())?;
            if from.path_id() != collection_o.path_id() {
                return err(format!("arena gate clip {name:?} draws from another collection"));
            }
            let sprite = ki(frame, "spriteId")?;
            let at = match order.iter().position(|&o| o == sprite) {
                Some(at) => at,
                None => {
                    order.push(sprite);
                    order.len() - 1
                }
            };
            ids.push(at);
        }
        table.push((name.to_string(), clip.get("fps").and_then(Value::float).unwrap_or(0.0), ids));
    }
    let scale = focal() / -CAM_Z;
    if gates.iter().any(|g| g.position[2].abs() > 0.01) {
        return err("arena gate off the gameplay plane");
    }
    let effect = slam_effect(source, sc, &gates, &library_o, &collection_o, &library)?;
    // The flash's three frames are 30-31 by 36-38 texels at full density, so they need two sheets;
    // at 0.8 they share one. The False Knight's views have room for two gate texture records and
    // no more, and the flash is on screen for a quarter of a second, so it is cooked at 0.8 texels
    // per pixel and drawn full size.
    let mut sprites: Vec<(i64, f64, f64)> = order.iter().map(|&i| (i, 1.0, 1.0)).collect();
    sprites.extend(effect.sprites.iter().map(|&i| (i, effect.scale, SLAM_EFFECT_DENSITY)));
    let (sheets, rects, boxes) = gate_sheets(source, &collection_o, &collection, &sprites, scale)?;
    let mut clips = table;
    clips.push((SLAM_EFFECT_CLIP.to_string(), effect.fps, (0..effect.sprites.len()).map(|i| order.len() + i).collect()));
    let mut all_sprites = order.clone();
    all_sprites.extend(&effect.sprites);
    // The wrap mode the guest plays is fixed by GATE_CLIP_WRAPS above; checked here once so a
    // library change cannot slip a looping clip in under the same names.
    for name in GATE_CLIPS {
        let clip = clip_named(&library, name).unwrap();
        guest_wrap_of(clip, clip.get("frames").and_then(Value::list).map_or(0, |f| f.len())).map_err(|e| e.to_string())?;
    }
    Ok(Some(GateArt {
        scene: file_of(sc),
        sheets,
        sprites: all_sprites,
        clips,
        rects,
        boxes,
        effect_offset: effect.offset,
        collection: collection_o.sid(),
        library: library_o.sid(),
    }))
}

/// Cook every admitted scene's gate art into `.hkpsx/sheet-art/`: `index.json`, and each sheet as
/// raw RGBA. The postpass in host/regions.py appends them to the scene actor banks.
pub fn cook_art(root: &Path) -> Result<()> {
    let source = Source::from_doctor(root).map_err(|e| e.to_string())?;
    let dir = root.join(".hkpsx/sheet-art");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut scenes: Vec<(String, Json)> = Vec::new();
    for row in crate::quality::SCENE_TABLE.iter() {
        let sc = Scene::new(&source, row.file).map_err(|e| e.to_string())?;
        // `gates` says whether the scene has a gate the postpass must give a bank to, art or not.
        let gates = all_gates(&sc, None)?.len();
        let art = match scene_art(&source, &sc)? {
            None => Json::Null,
            Some(a) => {
                let mut sheets = Vec::new();
                for (i, sheet) in a.sheets.iter().enumerate() {
                    let file = format!("{}-{i}.rgba", a.scene);
                    std::fs::write(dir.join(&file), &sheet.data).map_err(|e| e.to_string())?;
                    sheets.push(Json::Obj(vec![
                        ("w".into(), Json::Int(sheet.width as i64)),
                        ("h".into(), Json::Int(sheet.height as i64)),
                        ("file".into(), Json::Str(file)),
                    ]));
                }
                Json::Obj(vec![
                    ("scene".into(), Json::Str(a.scene.clone())),
                    ("sheets".into(), Json::List(sheets)),
                    ("sprites".into(), Json::List(a.sprites.iter().map(|s| Json::Int(*s)).collect())),
                    (
                        "clips".into(),
                        Json::List(
                            a.clips
                                .iter()
                                .map(|(name, fps, frames)| {
                                    Json::Obj(vec![
                                        ("name".into(), Json::Str(name.clone())),
                                        ("fps".into(), Json::Float(*fps)),
                                        ("frames".into(), Json::List(frames.iter().map(|f| Json::Int(*f as i64)).collect())),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    (
                        "rects".into(),
                        Json::List(a.rects.iter().map(|r| Json::List(r.iter().map(|v| Json::Int(*v as i64)).collect())).collect()),
                    ),
                    (
                        "boxes".into(),
                        Json::List(a.boxes.iter().map(|b| Json::List(b.iter().map(|v| Json::Float(*v)).collect())).collect()),
                    ),
                    ("effect_offset".into(), Json::List(a.effect_offset.iter().map(|v| Json::Float(*v)).collect())),
                    ("collection".into(), Json::Str(a.collection.clone())),
                    ("library".into(), Json::Str(a.library.clone())),
                ])
            }
        };
        let drips = crate::props::drip_sources(&sc)?.len();
        let drip = match drip_art(&source, &sc)? {
            None => Json::Null,
            Some(d) => {
                let file = format!("{}-drip.rgba", row.file);
                std::fs::write(dir.join(&file), &d.sheet.data).map_err(|e| e.to_string())?;
                Json::Obj(vec![
                    ("sheet".into(), Json::Obj(vec![("w".into(), Json::Int(d.sheet.width as i64)), ("h".into(), Json::Int(d.sheet.height as i64)), ("file".into(), Json::Str(file))])),
                    (
                        "clips".into(),
                        Json::List(
                            d.clips
                                .iter()
                                .map(|(name, fps, frames)| {
                                    Json::Obj(vec![
                                        ("name".into(), Json::Str(name.clone())),
                                        ("fps".into(), Json::Float(*fps)),
                                        ("frames".into(), Json::List(frames.iter().map(|f| Json::Int(*f as i64)).collect())),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    ("rects".into(), Json::List(d.rects.iter().map(|r| Json::List(r.iter().map(|v| Json::Int(*v as i64)).collect())).collect())),
                    ("boxes".into(), Json::List(d.boxes.iter().map(|b| Json::List(b.iter().map(|v| Json::Float(*v)).collect())).collect())),
                    ("library".into(), Json::Str(d.library.clone())),
                    ("collection".into(), Json::Str(d.collection.clone())),
                ])
            }
        };
        scenes.push((
            row.file.to_owned(),
            Json::Obj(vec![
                ("gates".into(), Json::Int(gates as i64)),
                ("art".into(), art),
                ("drips".into(), Json::Int(drips as i64)),
                ("drip_art".into(), drip),
            ]),
        ));
    }
    std::fs::write(dir.join("index.json"), dumps(&Json::Obj(scenes)) + "\n").map_err(|e| e.to_string())?;
    Ok(())
}

/// Scripted-edge exclusions the generated gate table already holds per catalogue slot
/// (blocker_terrain.py read them off `data/battle_gates.rs` the same way).
pub fn region_edge_counts(root: &Path) -> HashMap<usize, usize> {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    let Ok(text) = std::fs::read_to_string(root.join("data/battle_gates.rs")) else {
        return counts;
    };
    let text = compact(&text);
    // `(slot,gate,&[edges])` rows.
    let bytes = text.as_bytes();
    let mut at = 0;
    while let Some(rel) = text[at..].find(",&[") {
        let mark = at + rel;
        at = mark + 3;
        // `(` digits `,` digits before the mark.
        let before = &text[..mark];
        let gate: String = before.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        if gate.is_empty() {
            continue;
        }
        let rest = &before[..before.len() - gate.len()];
        let Some(rest) = rest.strip_suffix(',') else { continue };
        let slot: String = rest.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        if slot.is_empty() || !rest[..rest.len() - slot.len()].ends_with('(') {
            continue;
        }
        let Some(close) = text[at..].find(']') else { continue };
        if bytes.get(at + close + 1) != Some(&b')') {
            continue;
        }
        *counts.entry(slot.parse().unwrap()).or_default() += count(&text[at..at + close]);
    }
    counts
}

/// The clips a drip plays, in the order `game/src/drip.rs` indexes them.
const DRIP_CLIPS: [&str; 4] = ["Idle", "Drip", "Fall", "Impact"];

/// A scene's water-drip sheet (props.py `drip_art`, cooking half).
pub struct DripArt {
    pub sheet: Image,
    pub clips: Vec<(String, f64, Vec<usize>)>,
    pub rects: Vec<[usize; 4]>,
    pub boxes: Vec<[f64; 4]>,
    pub library: String,
    pub collection: String,
}

pub fn drip_art(source: &Source, sc: &Scene) -> Result<Option<DripArt>> {
    let drips = crate::props::drip_sources(sc)?;
    if drips.is_empty() {
        return Ok(None);
    }
    let mut libraries = std::collections::BTreeSet::new();
    let mut collections = std::collections::BTreeSet::new();
    for d in &drips {
        libraries.insert(source.deref(&sc.base, k(&d.animator, "library")?).map_err(|e| e.to_string())?.sid());
        collections.insert(source.deref(&sc.base, k(&d.sprite, "collection")?).map_err(|e| e.to_string())?.sid());
    }
    if libraries.len() != 1 || collections.len() != 1 {
        return err("water drips in one scene use different art");
    }
    let d0 = &drips[0];
    let library_o = source.deref(&sc.base, k(&d0.animator, "library")?).map_err(|e| e.to_string())?;
    let collection_o = source.deref(&sc.base, k(&d0.sprite, "collection")?).map_err(|e| e.to_string())?;
    let library = source.read(&library_o).map_err(|e| e.to_string())?;
    let collection = source.read(&collection_o).map_err(|e| e.to_string())?;
    let mut order: Vec<i64> = Vec::new();
    let mut table = Vec::new();
    for name in DRIP_CLIPS {
        let clip = clip_named(&library, name).ok_or_else(|| format!("'{name}'"))?;
        let mode = clip.get("wrapMode").and_then(Value::int).unwrap_or(-1);
        let fps = clip.get("fps").and_then(Value::float).unwrap_or(0.0);
        if (mode != 2 && mode != 6) || fps != fps.trunc() {
            return err(format!("water drip clip {name:?} is no longer a whole-rate play-once clip"));
        }
        let mut ids = Vec::new();
        for frame in clip.get("frames").and_then(Value::list).unwrap_or(&[]) {
            let sprite = ki(frame, "spriteId")?;
            let at = match order.iter().position(|&o| o == sprite) {
                Some(at) => at,
                None => {
                    order.push(sprite);
                    order.len() - 1
                }
            };
            ids.push(at);
        }
        table.push((name.to_string(), fps, ids));
    }
    let sprites: Vec<(i64, f64, f64)> = order.iter().map(|&i| (i, 1.0, 1.0)).collect();
    let (mut sheets, rects, boxes) = gate_sheets(source, &collection_o, &collection, &sprites, focal() / -CAM_Z)?;
    if sheets.len() != 1 {
        return err("water drip art needs more than one sheet");
    }
    Ok(Some(DripArt {
        sheet: sheets.remove(0),
        clips: table,
        rects: rects.iter().map(|r| [r[1], r[2], r[3], r[4]]).collect(),
        boxes,
        library: library_o.sid(),
        collection: collection_o.sid(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn compact_drops_comments_whitespace_and_trailing_commas() {
        assert_eq!(compact("pub static A: &[u8] = &[1, 2,\n 3,]; // note"), "pubstaticA:&[u8]=&[1,2,3];");
        assert_eq!(compact("(1, 2,)"), "(1,2)");
    }

    #[test]
    fn bindings_are_read_however_the_table_is_laid_out() {
        let text = compact("pub static L:&[(u16,Binding)]=&[(43, Binding{off:&[349, 2],edges:&[22,23,24]}),(44,Binding{off:&[386],edges:&[]})];");
        assert_eq!(binding_counts(&text, "off:&["), [(43, 3), (44, 0)]);
        let doors = compact("&[(0,Binding{frames:[262,263,264],edges:&[5]}),];");
        assert_eq!(binding_counts(&doors, "frames:["), [(0, 1)]);
    }

    #[test]
    fn gate_rows_count_their_edges_per_slot() {
        let dir = std::env::temp_dir().join(format!("hk-gate-rows-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data/battle_gates.rs"), "pub static REGIONS:&[(u16,u8,&[u16])]=&[(169,1,&[3,4,5]),(169,2,&[9]),(170,0,&[0,1,2]),];\n").unwrap();
        let counts = region_edge_counts(&dir);
        assert_eq!(counts.get(&169), Some(&4));
        assert_eq!(counts.get(&170), Some(&3));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Gates, the Great Door and the Lifeblood cocoons share one exclusion list in the guest, so a
    /// catalogue slot's total across the generated tables has to fit it (was a check in
    /// tests/test_boss_arena.py). Skipped before the first cook.
    #[test]
    fn the_shared_exclusion_scratch_holds_every_slot() {
        let root = root();
        if !root.join("data/battle_gates.rs").is_file() {
            return;
        }
        let budget = edge_scratch_slots(&root).unwrap();
        let mut spent = neighbour_edges(&root);
        for (slot, n) in region_edge_counts(&root) {
            *spent.entry(slot).or_default() += n;
        }
        let worst = spent.iter().max_by_key(|(_, n)| **n).map(|(s, n)| (*s, *n)).unwrap_or((0, 0));
        assert!(worst.1 <= budget, "catalogue slot {} needs {}", worst.0, worst.1);
    }
}
