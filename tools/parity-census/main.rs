//! Parity census: every renderable the original can show in each shipped room,
//! against what the cook actually put in that room's packs.
//!
//! Source side: the whole-world import (`.hkpsx/world-import/<file>/`), which
//! keeps every SpriteRenderer, tk2dSprite, MeshRenderer and ParticleSystem of a
//! scene with its authored activity, plus the component records that name the
//! behaviour driving each object. Cooked side: every view's `scene.json` draws
//! and `unsupported.json` refusals under `data/regions/region-NNN/`, the region
//! metadata in `.hkpsx/selected-regions.json`, and any extra cooker reports
//! passed with `--extra` (props, pickups, ...), scanned for the source ids they
//! carry.
//!
//! Every active renderable lands in exactly one status:
//!   drawn      a view draws it as scenery (or as a tilemap fill);
//!   system     a gameplay system's metadata names it, its object or an
//!              ancestor (actors, breakables, props, ...): the art is that
//!              system's to show;
//!   refused    the cook recorded an explicit refusal for it;
//!   culled     a SpriteRenderer the scenery pass dropped by its own rules
//!              (near the camera, outside every view, below half a pixel);
//!   missing    nothing in the cook mentions it.
//! Inactive renderables are counted per family separately, because whether the
//! original ever shows them depends on scripts this census does not run.
//!
//! Usage (from the repository root):
//!   cargo run --release --manifest-path tools/parity-census/Cargo.toml -- \
//!       --world-import .hkpsx/world-import --out .hkpsx/parity-census.json \
//!       [--extra .hkpsx/props-provenance.json ...]
use flate2::read::GzDecoder;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

/// host/cook.py's camera constants: FOCAL = 120 / tan(12 deg), CAM_Z = -38.1.
const CAM_Z: f64 = -38.1;
fn focal() -> f64 { 120.0 / 12f64.to_radians().tan() }

fn read_json(path: &Path) -> Value {
    let file = File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut text = String::new();
    if path.extension().map_or(false, |x| x == "gz") {
        GzDecoder::new(BufReader::new(file)).read_to_string(&mut text).unwrap();
    } else {
        BufReader::new(file).read_to_string(&mut text).unwrap();
    }
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `levelN:ID` strings anywhere under `value`, each with the top-level key it
/// was found under.
fn scan_ids(value: &Value, label: &str, out: &mut HashMap<String, BTreeSet<String>>) {
    scan_ids_in(value, label, None, out)
}

/// As `scan_ids`, also reading bare integer `game_object`/`gid` fields as
/// objects of `file` (the actor and breakable rows store them that way).
fn scan_ids_in(value: &Value, label: &str, file: Option<&str>, out: &mut HashMap<String, BTreeSet<String>>) {
    match value {
        Value::String(s) => {
            if let Some((file, id)) = s.split_once(':') {
                if file.starts_with("level") && file[5..].bytes().all(|b| b.is_ascii_digit())
                    && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
                    out.entry(s.clone()).or_default().insert(label.to_string());
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|v| scan_ids_in(v, label, file, out)),
        Value::Object(map) => {
            for (k, v) in map {
                if let (Some(file), true, Some(id)) = (file, k == "game_object" || k == "gid", v.as_i64()) {
                    out.entry(format!("{file}:{id}")).or_default().insert(label.to_string());
                }
                scan_ids_in(v, label, file, out);
            }
        }
        _ => {}
    }
}

fn file_of(sid: &str) -> &str { sid.split_once(':').map_or(sid, |(f, _)| f) }

fn local_id(file: &str, ptr: &Value) -> Option<String> {
    let id = ptr.get("m_PathID")?.as_i64()?;
    if id == 0 { return None; }
    Some(format!("{file}:{id}"))
}

/// Object name without editor copy suffixes or trailing numbering, so
/// `Battle Gate 2 (1)` and `Battle Gate 3` share `Battle Gate`.
fn family_name(name: &str) -> String {
    let mut s = name.trim().to_string();
    loop {
        let before = s.clone();
        if s.ends_with(')') {
            if let Some(open) = s.rfind(" (") {
                if s[open + 2..s.len() - 1].bytes().all(|b| b.is_ascii_digit()) { s.truncate(open); }
            }
        }
        s = s.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ' || c == '_' || c == '-' || c == '.').to_string();
        if s == before { break; }
    }
    if s.is_empty() { name.to_string() } else { s }
}

/// Component types that say nothing about what drives an object.
const PLAIN: &[&str] = &[
    "GameObject", "Transform", "RectTransform", "SpriteRenderer", "MeshRenderer", "MeshFilter",
    "tk2dSprite", "tk2dSpriteAnimator", "tk2dSlicedSprite", "tk2dClippedSprite", "Animator",
    "PlayFromRandomFrameMecanim", "SetZ", "SetZRandom", "AudioSource", "BoxCollider2D",
    "CircleCollider2D", "PolygonCollider2D", "EdgeCollider2D", "Rigidbody2D", "ParticleSystem",
    "ParticleSystemRenderer", "ParticleSystemAutoRecycle", "ParticleSystemCollisionLagFix",
    "ReduceParticleEffects", "VibrationPlayer", "NonBouncer", "NonThunker", "SpriteFlash",
    "ObjectBounce", "SpinSelfSimple", "Animation", "CanvasRenderer", "LineRenderer",
    "TrailRenderer", "AudioSourceGamePause", "RandomScale", "RandomRotation", "Light",
];

struct Scene {
    gos: HashMap<String, Go>,
    /// Component id -> owning GameObject id.
    comp_go: HashMap<String, String>,
    /// GameObject id -> parent GameObject id.
    parent: HashMap<String, String>,
}

struct Go {
    name: String,
    scripts: Vec<String>,
}

fn load_components(path: &Path) -> Scene {
    let doc = read_json(path);
    let mut types: HashMap<String, (String, Value)> = HashMap::new();
    for o in doc["objects"].as_array().unwrap() {
        types.insert(o["source"].as_str().unwrap().to_string(),
                     (o["type"].as_str().unwrap().to_string(), o["data"].clone()));
    }
    let mut gos = HashMap::new();
    let mut comp_go = HashMap::new();
    for (sid, (typ, data)) in &types {
        if typ == "GameObject" { continue; }
        if let Some(go) = local_id(file_of(sid), &data["m_GameObject"]) { comp_go.insert(sid.clone(), go); }
    }
    let mut tf_go: HashMap<String, String> = HashMap::new();
    let mut go_father_tf: HashMap<String, String> = HashMap::new();
    for (sid, (typ, data)) in &types {
        let file = file_of(sid);
        if typ == "Transform" || typ == "RectTransform" {
            if let Some(go) = local_id(file, &data["m_GameObject"]) {
                tf_go.insert(sid.clone(), go.clone());
                if let Some(father) = local_id(file, &data["m_Father"]) { go_father_tf.insert(go, father); }
            }
        }
    }
    for (sid, (typ, data)) in &types {
        if typ != "GameObject" { continue; }
        let file = file_of(sid);
        let mut scripts = Vec::new();
        for c in data["m_Component"].as_array().into_iter().flatten() {
            let Some(cid) = local_id(file, &c["component"]) else { continue };
            let Some((ctyp, cdata)) = types.get(&cid) else { continue };
            if PLAIN.contains(&ctyp.as_str()) { continue; }
            if ctyp == "PlayMakerFSM" {
                let name = cdata["fsm"]["name"].as_str().unwrap_or("");
                scripts.push(format!("FSM:{name}"));
            } else {
                scripts.push(ctyp.clone());
            }
        }
        scripts.sort();
        scripts.dedup();
        gos.insert(sid.clone(), Go {
            name: data["m_Name"].as_str().unwrap_or("").to_string(),
            scripts,
        });
    }
    let parent = go_father_tf.into_iter()
        .filter_map(|(go, tf)| tf_go.get(&tf).map(|p| (go, p.clone()))).collect();
    Scene { gos, comp_go, parent }
}

impl Scene {
    fn chain(&self, go: &str) -> Vec<String> {
        let mut out = vec![go.to_string()];
        let mut at = go.to_string();
        while let Some(p) = self.parent.get(&at) {
            if out.len() > 64 { break; }
            out.push(p.clone());
            at = p.clone();
        }
        out
    }
    /// The nearest object on the chain that carries a behaviour, which is what
    /// decides whether and how the renderer shows.
    fn owner(&self, go: &str) -> Option<String> {
        self.chain(go).into_iter().find(|g| self.gos.get(g).map_or(false, |o| !o.scripts.is_empty()))
    }
}

#[derive(Default)]
struct Cooked {
    drawn: HashSet<String>,
    refused: HashMap<String, (String, String)>,
    referenced: HashMap<String, BTreeSet<String>>,
    /// (camera x range, camera y range) per view.
    views: Vec<([f64; 2], [f64; 2])>,
    files: BTreeSet<String>,
}

fn f(v: &Value) -> f64 { v.as_f64().unwrap_or(0.0) }

fn main() {
    let mut args = std::env::args().skip(1);
    let mut world = PathBuf::from(".hkpsx/world-import");
    let mut regions_meta = PathBuf::from(".hkpsx/selected-regions.json");
    let mut regions_dir = PathBuf::from("data/regions");
    let mut out = PathBuf::from(".hkpsx/parity-census.json");
    let mut extras = Vec::new();
    let mut activation: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        let v = args.next().expect("flag needs a value");
        match a.as_str() {
            "--world-import" => world = v.into(),
            "--regions-meta" => regions_meta = v.into(),
            "--regions-dir" => regions_dir = v.into(),
            "--out" => out = v.into(),
            "--extra" => extras.push(PathBuf::from(v)),
            "--activation" => activation = Some(v.into()),
            _ => panic!("unknown flag {a}"),
        }
    }
    let focal = focal();
    let meta = read_json(&regions_meta);
    let catalog = read_json(&world.join("catalog.json"));
    // host/activation.py's fresh-save PlayerData gates: per scene, the objects
    // the cook removed and the authored-inactive objects the original turns on.
    let gates = activation.as_deref().map(read_json).unwrap_or(Value::Null);
    let file_scene: HashMap<String, String> = catalog.as_array().unwrap().iter()
        .map(|e| (e["file"].as_str().unwrap().to_string(), e["scene_name"].as_str().unwrap().to_string())).collect();

    // Extra cooker reports apply to whichever scene owns the id's file.
    let mut extra_ids: HashMap<String, BTreeSet<String>> = HashMap::new();
    for path in &extras {
        let label = path.file_stem().unwrap().to_string_lossy().to_string();
        scan_ids(&read_json(path), &format!("extra:{label}"), &mut extra_ids);
    }

    let mut scenes: Vec<(String, String)> = Vec::new();
    for s in meta["scenes"].as_array().unwrap() {
        scenes.push((s["scene_name"].as_str().unwrap().to_string(), s["file"].as_str().unwrap().to_string()));
    }
    let mut cooked: BTreeMap<String, Cooked> = BTreeMap::new();
    for r in meta["regions"].as_array().unwrap() {
        let scene = r["scene_name"].as_str().unwrap().to_string();
        let chunk = r["chunk_id"].as_u64().unwrap();
        let c = cooked.entry(scene.clone()).or_default();
        c.views.push(([f(&r["camera_x"][0]), f(&r["camera_x"][1])], [f(&r["camera_y"][0]), f(&r["camera_y"][1])]));
        if let Value::Object(map) = r {
            for (k, v) in map {
                if k == "edge_sources" || k == "texture_request_to_canonical" { continue; }
                scan_ids_in(v, &format!("meta:{k}"), r["scene_file"].as_str(), &mut c.referenced);
            }
        }
        let dir = regions_dir.join(format!("region-{chunk:03}"));
        let scene_json = read_json(&dir.join("scene.json"));
        for d in scene_json["draws"].as_array().into_iter().flatten() {
            if let Some(s) = d["source"].as_str() { c.drawn.insert(s.to_string()); c.files.insert(file_of(s).to_string()); }
        }
        if let Value::Object(map) = &scene_json {
            for (k, v) in map {
                if matches!(k.as_str(), "draws" | "atlas" | "texture_request_to_canonical" | "edges" | "frames" | "clips") { continue; }
                scan_ids(v, &format!("view:{k}"), &mut c.referenced);
            }
        }
        let fill = dir.join("tilemap-fill.json");
        if fill.is_file() {
            let mut ids = HashMap::new();
            scan_ids(&read_json(&fill), "tilemap", &mut ids);
            c.drawn.extend(ids.into_keys());
        }
        let uns = read_json(&dir.join("unsupported.json"));
        for e in uns["errors"].as_array().into_iter().flatten() {
            if let Some(id) = e["id"].as_str() {
                c.refused.entry(id.to_string()).or_insert((
                    e["type"].as_str().unwrap_or("").to_string(),
                    e["error"].as_str().unwrap_or("").chars().take(200).collect()));
            }
        }
    }

    let mut report_scenes = Vec::new();
    // family key -> aggregate
    let mut families: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for (scene_name, scene_file) in &scenes {
        let empty = Cooked::default();
        let c = cooked.get(scene_name).unwrap_or(&empty);
        let mut removed: HashMap<String, String> = HashMap::new();
        let mut activates: HashMap<String, String> = HashMap::new();
        for (list, into) in [("removed", &mut removed), ("activates", &mut activates)] {
            for g in gates["scenes"][scene_name.as_str()][list].as_array().into_iter().flatten() {
                // `removed` names the object; `activates` names the FSM, whose
                // owner's direct children it turns on.
                let key = if list == "removed" { "object_id" } else { "source" };
                let id = g[key].as_i64().unwrap_or(0);
                into.insert(format!("{scene_file}:{id}"), format!("{}:{}", g["gate"].as_str().unwrap_or(""), g["field"].as_str().unwrap_or("")));
            }
        }
        let mut files: BTreeSet<String> = c.files.clone();
        files.insert(scene_file.clone());
        for id in c.referenced.keys() { files.insert(file_of(id).to_string()); }
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut items = Vec::new();
        for file in &files {
            let gpath = world.join(file).join("geometry.json.gz");
            if !gpath.is_file() { continue; }
            let geo = read_json(&gpath);
            let comps = load_components(&world.join(file).join("components.json.gz"));
            let additive = file != scene_file;
            // tk2d sprites render through a MeshRenderer on the same object;
            // that renderer is the tk2d record, not a second item.
            let tk2d_gos: HashSet<String> = geo["tk2d_sprites"].as_array().into_iter().flatten()
                .filter_map(|t| t["game_object"].as_str().map(str::to_string)).collect();
            for (kind, list) in [("sprite", "sprites"), ("tk2d", "tk2d_sprites"), ("mesh", "meshes"), ("particles", "particle_systems")] {
                for rec in geo[list].as_array().into_iter().flatten() {
                    let src = rec["source"].as_str().unwrap().to_string();
                    let go = rec["game_object"].as_str().unwrap_or("").to_string();
                    if kind == "mesh" && tk2d_gos.contains(&go) { continue; }
                    let active = rec["active_hierarchy"].as_bool().unwrap_or(false);
                    let enabled = rec["enabled"].as_bool().unwrap_or(false);
                    let chain = comps.chain(&go);
                    let owner = comps.owner(&go);
                    let owner_go = owner.as_ref().and_then(|o| comps.gos.get(o));
                    let own_name = comps.gos.get(&go).map(|g| g.name.clone()).unwrap_or_default();
                    let family = match owner_go {
                        Some(o) => format!("{kind}|{}|{}", family_name(&o.name), o.scripts.join("+")),
                        None => format!("{kind}|{}|", family_name(&own_name)),
                    };
                    let mut system: BTreeSet<String> = BTreeSet::new();
                    for id in std::iter::once(&src).chain(chain.iter()) {
                        if let Some(l) = c.referenced.get(id) { system.extend(l.iter().cloned()); }
                        if let Some(l) = extra_ids.get(id) { system.extend(l.iter().cloned()); }
                    }
                    let refused = std::iter::once(&src).chain(chain.iter()).find_map(|id| c.refused.get(id));
                    // Screen size estimate in PS1 pixels at the view's scale.
                    let (z, area) = match kind {
                        "sprite" => {
                            let q = &rec["world_quad"];
                            let p = |i: usize| (f(&q[i][0]), f(&q[i][1]), f(&q[i][2]));
                            let (a, b, d) = (p(0), p(1), p(2));
                            let w = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                            let h = ((d.0 - a.0).powi(2) + (d.1 - a.1).powi(2)).sqrt();
                            (a.2, w * h)
                        }
                        "tk2d" => {
                            let v = rec["world_vertices"].as_array().cloned().unwrap_or_default();
                            let xs: Vec<f64> = v.iter().map(|p| f(&p[0])).collect();
                            let ys: Vec<f64> = v.iter().map(|p| f(&p[1])).collect();
                            let z = v.first().map_or(0.0, |p| f(&p[2]));
                            let span = |s: &[f64]| s.iter().cloned().fold(f64::MIN, f64::max) - s.iter().cloned().fold(f64::MAX, f64::min);
                            (z, if xs.is_empty() { 0.0 } else { span(&xs) * span(&ys) })
                        }
                        _ => (f(&rec["position"][2]), 0.0),
                    };
                    let scale = if z > CAM_Z { focal / (z - CAM_Z) } else { 0.0 };
                    let pixels = area * scale * scale;
                    let gated = chain.iter().find_map(|g| removed.get(g));
                    // A child of an object whose fresh-save gate activates its children.
                    let turned_on = chain.get(1).and_then(|p| {
                        activates.iter().find(|(fsm, _)| comps.comp_go.get(*fsm) == Some(p)).map(|(_, v)| v)
                    });
                    let status = if c.drawn.contains(&src) {
                        "drawn".to_string()
                    } else if kind == "sprite" && rec["sprite"].is_null() {
                        "empty:no-sprite".to_string()
                    } else if z <= CAM_Z && kind != "particles" && kind != "mesh" {
                        "empty:behind-camera".to_string()
                    } else if let Some(g) = gated {
                        let _ = g;
                        "gated".to_string()
                    } else if !active && turned_on.is_some() {
                        "missing:activated-on-fresh-save".to_string()
                    } else if !active {
                        "inactive".to_string()
                    } else if !enabled {
                        "disabled".to_string()
                    } else if !system.is_empty() {
                        "system".to_string()
                    } else if let Some((t, _)) = refused {
                        format!("refused:{t}")
                    } else if kind == "sprite" {
                        // The cook's own test: the quad's extent against each view's
                        // camera range widened by half a screen at this depth.
                        let q = &rec["world_quad"];
                        let xs: Vec<f64> = (0..4).map(|i| f(&q[i][0])).collect();
                        let ys: Vec<f64> = (0..4).map(|i| f(&q[i][1])).collect();
                        let (x0, x1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
                        let (y0, y1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
                        if z <= CAM_Z + 2.0 {
                            "culled:near-camera".to_string()
                        } else if pixels < 0.25 {
                            "culled:tiny".to_string()
                        } else if c.views.iter().all(|(cx, cy)| {
                            x1 < cx[0] - 160.0 / scale || x0 > cx[1] + 160.0 / scale
                                || y1 < cy[0] - 120.0 / scale || y0 > cy[1] + 120.0 / scale
                        }) {
                            "culled:outside-views".to_string()
                        } else {
                            "missing".to_string()
                        }
                    } else {
                        "missing".to_string()
                    };
                    *counts.entry(format!("{kind}:{status}")).or_default() += 1;
                    let fam = families.entry(family.clone()).or_insert_with(Map::new);
                    let key = format!("{}{}", if additive { "add:" } else { "" }, status);
                    let e = fam.entry("statuses").or_insert(json!({}));
                    let n = e.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
                    e[&key] = json!(n);
                    let sc = fam.entry("scenes").or_insert(json!([]));
                    if !sc.as_array().unwrap().iter().any(|s| s == scene_name) { sc.as_array_mut().unwrap().push(json!(scene_name)); }
                    if status != "drawn" && status != "inactive" && status != "disabled" && !status.starts_with("empty") && status != "gated" {
                        let px = fam.get("missing_pixels").and_then(Value::as_f64).unwrap_or(0.0)
                            + if status.starts_with("missing") { pixels } else { 0.0 };
                        fam.insert("missing_pixels".into(), json!(px));
                        items.push(json!({
                            "source": src, "game_object": go, "name": own_name, "kind": kind,
                            "geometry_status": rec.get("geometry_status"), "frame": rec.get("frame_name"),
                            "status": status, "family": family, "owner": owner,
                            "system": system, "refusal": refused.map(|(t, e)| format!("{t}: {e}")),
                            "position": rec["position"], "z": z, "pixels": pixels.round(),
                            "additive": additive, "file": file, "origin_scene": file_scene.get(file),
                            // A looping emitter that plays on awake runs for as long as the
                            // room is loaded: ambience rather than an event effect.
                            "ambient": kind == "particles" && rec["serialized"]["looping"].as_bool() == Some(true)
                                && rec["serialized"]["playOnAwake"].as_bool() == Some(true),
                        }));
                    }
                }
            }
        }
        report_scenes.push(json!({"scene": scene_name, "file": scene_file, "files": files, "counts": counts, "items": items}));
    }
    // Texture-record headroom per scene (hk-format MAX_TEXTURES = 640 per view)
    // and the pickup levels the bank fell back through: the two places where
    // something appended to a scene bank pushes something else out.
    let mut headroom: BTreeMap<String, Value> = BTreeMap::new();
    for r in meta["regions"].as_array().unwrap() {
        let scene = r["scene_name"].as_str().unwrap().to_string();
        let t = r["textures"].as_u64().unwrap_or(0);
        let e = headroom.entry(scene).or_insert(json!({"max_textures": 0, "views": 0}));
        if t > e["max_textures"].as_u64().unwrap() { e["max_textures"] = json!(t); }
        e["views"] = json!(e["views"].as_u64().unwrap() + 1);
    }
    for (_, e) in headroom.iter_mut() { e["free_records"] = json!(640 - e["max_textures"].as_u64().unwrap() as i64); }
    let report = json!({
        "format": "HKPARITY01",
        "texture_headroom": headroom,
        "refused_pickups": meta.get("refused_pickups"),
        "camera": {"focal": focal, "cam_z": CAM_Z},
        "scenes": report_scenes,
        "families": families,
    });
    std::fs::write(&out, serde_json::to_string(&report).unwrap()).unwrap();
    eprintln!("wrote {}", out.display());
}
