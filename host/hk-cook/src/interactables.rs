//! The nail-struck switches and the gates, pillars and toll gates they open: the cooker.
//!
//! The source runs these as PlayMaker FSMs (`toll switch`, `Switch Control`, `Toll Gate`,
//! `Pillar Control`, `Gate`). The port runs them as the small tables of `hk-sim switches.rs` (a state
//! has the actions it runs on entry, a wait that leads on, and the events it answers), so a script bank
//! can hold the same shape later. This cooker is the other half: it reads each admitted scene's
//! switches and targets, checks every FSM it takes against the contract the table encodes (waits, clip
//! names, events) and refuses a scene whose FSM says anything else, and writes
//!
//! * `data/interactables.rs`: the instances (position, scale, hit box, clips, targets, persistence id),
//!   the clips and their sprites, the sprite pieces and the terrain edges each target stands in,
//! * the sprite sheets (`scene_art`) that `.hkpsx/sheet-art/` hands the region postpass, which appends
//!   them to the scene actor bank like the arena gates' (`battle_gates.rs`).
//!
//! Nothing here is a cooker input of a single region: it reads the source scenes and, for the edge join,
//! the region report (`edge_sources`), like the arena gate table.
use crate::battle_gates::{edge_scratch_slots, neighbour_edges, region_edge_counts};
use crate::breakables::{file_of, k, kf, ki, kl, ks};
use crate::common::{actions, components, err, collider_polygons, field, py_max, py_min, scalar, state, states, variables, Result};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::false_knight::fsm_digest;
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::scene::Scene;
use hk_unity::{Obj, Source, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The terrain layer the world cook bakes solid boxes from.
const TERRAIN_LAYER: i64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchKind {
    /// `toll switch`: finds its two gates by name, sends them OPEN.
    Toll,
    /// `Switch Control`: sends ACTIVATE to the objects its variables name.
    Control,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    TollGate,
    Pillar,
    MetalGate,
}

fn num(v: &Value) -> f64 {
    scalar(v).float().unwrap_or(0.0)
}

fn text(v: &Value) -> String {
    scalar(v).str().unwrap_or_default()
}

/// The literal of an action field (a compact scalar or a plain string), refusing a variable.
fn literal(fields: &hk_unity::playmaker::Fields, key: &str) -> Result<Value> {
    let v = field(fields, key)?;
    if v.get("useVariable").is_some_and(Value::truthy) {
        return err(format!("FSM reads a variable where {key} was expected"));
    }
    Ok(scalar(v))
}

fn clip_of_state(fsm: &Value, who: &str, name: &str) -> Result<String> {
    let all = states(fsm, &[], who)?;
    let s = state(&all, name)?;
    let mut names = Vec::new();
    for kind in ["Tk2dPlayAnimation", "Tk2dPlayAnimationWithEvents"] {
        for f in actions(s, kind)? {
            names.push(literal(&f, "clipName")?.str().unwrap_or_default());
        }
    }
    if names.len() != 1 {
        return err(format!("{who} state {name:?} plays {} clips", names.len()));
    }
    Ok(names.remove(0))
}

fn wait_of_state(fsm: &Value, who: &str, name: &str) -> Result<f64> {
    let all = states(fsm, &[], who)?;
    let s = state(&all, name)?;
    let waits = actions(s, "Wait")?;
    if waits.len() != 1 {
        return err(format!("{who} state {name:?} has {} waits", waits.len()));
    }
    literal(&waits[0], "time")?.float().ok_or_else(|| "wait is not a number".to_string())
}

/// The world position, x and y scale (signed) of an object that is neither rotated nor skewed.
fn placed(sc: &Scene, gid: i64) -> Result<([f64; 3], [f64; 2])> {
    let m = sc.world(*sc.go_transform.get(&gid).ok_or("no transform")?).map_err(|e| e.to_string())?;
    if m[0][1].abs() > 1e-4 || m[1][0].abs() > 1e-4 {
        return err(format!("{} is rotated", ks(sc.go(gid).ok_or("go")?, "m_Name")?));
    }
    let p = sc.point(gid, 0.0, 0.0, 0.0).map_err(|e| e.to_string())?;
    Ok((p, [m[0][0], m[1][1]]))
}

/// A sprite object's parts: tk2d sprite, animator, renderer, with a unit sprite scale.
fn art_parts<'a>(sc: &'a Scene, gid: i64) -> Result<(&'a Value, &'a Value)> {
    let found = components(sc, gid)?;
    let find = |t: &str| found.iter().rev().find(|c| c.1 == t).map(|c| c.2);
    let (Some(sprite), Some(animator), Some(renderer)) = (find("tk2dSprite"), find("tk2dSpriteAnimator"), find("MeshRenderer")) else {
        return err(format!("{} has no tk2d sprite, animator and renderer", ks(sc.go(gid).ok_or("go")?, "m_Name")?));
    };
    let scale = k(sprite, "_scale")?;
    if (kf(scale, "x")? - 1.0).abs() > 1e-6 || (kf(scale, "y")? - 1.0).abs() > 1e-6 {
        return err("an interactable has a tk2d sprite scale");
    }
    let _ = renderer;
    Ok((sprite, animator))
}

/// One switch.
#[derive(Clone, Debug)]
pub struct Switch {
    pub kind: SwitchKind,
    pub gid: i64,
    pub name: String,
    pub source: String,
    pub position: [f64; 3],
    pub scale: [f64; 2],
    pub hit_box: [f64; 4],
    pub wait: f64,
    pub idle: String,
    pub hit: String,
    pub activated: String,
    pub activated_frame: Option<i64>,
    pub targets: Vec<i64>,
    pub player_data: Option<String>,
}

/// One gate, pillar or toll gate.
#[derive(Clone, Debug)]
pub struct Target {
    pub kind: TargetKind,
    pub gid: i64,
    pub name: String,
    pub source: String,
    pub position: [f64; 3],
    pub scale: [f64; 2],
    pub collider_source: String,
    pub box_: [f64; 4],
    pub idle: String,
    pub clips: Vec<(String, String)>,
    pub active: bool,
}

fn fsm_name(tree: &Value) -> Result<String> {
    ks(k(tree, "fsm")?, "name")
}

fn default_clip(source: &Source, sc: &Scene, animator: &Value) -> Result<String> {
    let library_o = source.deref(&sc.base, k(animator, "library")?).map_err(|e| e.to_string())?;
    let library = source.read(&library_o).map_err(|e| e.to_string())?;
    let id = ki(animator, "defaultClipId")?;
    let clips = kl(&library, "clips")?;
    let clip = clips.get(id as usize).ok_or("default clip out of range")?;
    ks(clip, "name")
}

fn hit_box(sc: &Scene, gid: i64) -> Result<[f64; 4]> {
    let triggers: Vec<_> = components(sc, gid)?
        .into_iter()
        .filter(|c| c.1.ends_with("Collider2D") && k(c.2, "m_IsTrigger").map(|v| v.truthy()).unwrap_or(false))
        .collect();
    if triggers.len() != 1 {
        return err(format!("{} has {} trigger colliders", ks(sc.go(gid).ok_or("go")?, "m_Name")?, triggers.len()));
    }
    let polys = collider_polygons(sc, gid, triggers[0].1, triggers[0].2)?;
    let xs = polys.iter().flatten().map(|p| p.0);
    let ys = polys.iter().flatten().map(|p| p.1);
    Ok([py_min(xs.clone()), py_min(ys.clone()), py_max(xs), py_max(ys)])
}

fn named_object(sc: &Scene, name: &str) -> Option<i64> {
    let mut hits: Vec<i64> = sc.gos.keys().copied().filter(|g| sc.go(*g).and_then(|go| go.get("m_Name")).and_then(Value::str).as_deref() == Some(name) && sc.active(*g)).collect();
    hits.sort();
    hits.first().copied()
}

/// Every switch of the scene, with its contract checked.
pub fn switches(source: &Source, sc: &Scene) -> Result<Vec<Switch>> {
    let mut out = Vec::new();
    let mut ids: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().filter(|o| o.typename == "PlayMakerFSM").collect();
    ids.sort_by_key(|o| o.id);
    for o in ids {
        let name = fsm_name(&o.tree)?;
        let kind = match name.as_str() {
            "toll switch" => SwitchKind::Toll,
            "Switch Control" => SwitchKind::Control,
            _ => continue,
        };
        let gid = crate::common::go_of(&o.tree).ok_or("fsm without object")?;
        if sc.go(gid).is_none() || !sc.active(gid) {
            continue;
        }
        let fsm = k(&o.tree, "fsm")?;
        let who = if kind == SwitchKind::Toll { "toll switch" } else { "Switch Control" };
        let vars = variables(fsm);
        let var = |n: &str| vars.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        // The town's mines lever is a Switch Control of another shape (it is struck only while the lift is
        // in range): its lift is not in the port, so it is reported and left out.
        let all = states(fsm, &[], who)?;
        if kind == SwitchKind::Control && state(&all, "Check If Nail").is_ok() {
            continue;
        }
        let hit = clip_of_state(fsm, who, "Hit")?;
        let wait = wait_of_state(fsm, who, "Hit")?;
        let (activated, frame) = if kind == SwitchKind::Toll {
            let s = state(&all, "Activate")?;
            let frames = actions(s, "Tk2dPlayFrame")?;
            (clip_of_state(fsm, who, "Activate")?, Some(literal(frames.first().ok_or("toll switch without a frame")?, "frame")?.int().unwrap_or(0)))
        } else {
            (clip_of_state(fsm, who, "Activated")?, None)
        };
        let targets: Vec<i64> = if kind == SwitchKind::Toll {
            let init = state(&all, "Initiate")?;
            let mut v = Vec::new();
            for f in actions(init, "FindGameObject")? {
                let n = literal(&f, "objectName")?.str().unwrap_or_default();
                if n.is_empty() || n == "CameraParent" {
                    continue;
                }
                match named_object(sc, &n) {
                    Some(t) => v.push(t),
                    None => return err(format!("toll switch finds no {n:?}")),
                }
            }
            v
        } else {
            let open = state(&all, "Open")?;
            let mut v = Vec::new();
            for f in actions(open, "SendEventByName")? {
                let target = field(&f, "eventTarget")?;
                let go = target.get("gameObject").and_then(|g| g.get("gameObject")).ok_or("eventTarget without object")?;
                let path = if go.get("useVariable").is_some_and(Value::truthy) {
                    let n = go.get("name").and_then(Value::str).unwrap_or_default();
                    var(&n).and_then(|x| x.get("m_PathID").and_then(Value::int)).unwrap_or(0)
                } else {
                    go.get("value").and_then(|x| x.get("m_PathID")).and_then(Value::int).unwrap_or(0)
                };
                if path != 0 {
                    v.push(path);
                }
            }
            v
        };
        let player_data = if kind == SwitchKind::Control && var("SetPlayerData").is_some_and(|v| v.truthy()) {
            var("Player Data").map(|v| text(&v)).filter(|s| !s.is_empty())
        } else {
            None
        };
        let (position, scale) = placed(sc, gid)?;
        let (_, animator) = art_parts(sc, gid)?;
        out.push(Switch {
            kind,
            gid,
            name: ks(sc.go(gid).ok_or("go")?, "m_Name")?,
            source: format!("{}:{}", file_of(sc), o.id),
            position,
            scale,
            hit_box: hit_box(sc, gid)?,
            wait,
            idle: default_clip(source, sc, animator)?,
            hit,
            activated,
            activated_frame: frame,
            targets,
            player_data,
        });
    }
    Ok(out)
}

/// Every gate, pillar and toll gate of the scene.
pub fn targets(source: &Source, sc: &Scene) -> Result<Vec<Target>> {
    let mut out = Vec::new();
    let mut ids: Vec<&hk_unity::scene::SceneObject> = sc.objects.iter().filter(|o| o.typename == "PlayMakerFSM").collect();
    ids.sort_by_key(|o| o.id);
    for o in ids {
        let name = fsm_name(&o.tree)?;
        let kind = match name.as_str() {
            "Toll Gate" => TargetKind::TollGate,
            "Pillar Control" => TargetKind::Pillar,
            "Gate" => TargetKind::MetalGate,
            _ => continue,
        };
        let gid = crate::common::go_of(&o.tree).ok_or("fsm without object")?;
        if sc.go(gid).is_none() {
            continue;
        }
        let active = sc.active(gid);
        let fsm = k(&o.tree, "fsm")?;
        let who = name.as_str();
        let (position, scale) = placed(sc, gid)?;
        let (_, animator) = art_parts(sc, gid)?;
        let colliders: Vec<_> = components(sc, gid)?
            .into_iter()
            .filter(|c| c.1 == "BoxCollider2D" && !k(c.2, "m_IsTrigger").map(|v| v.truthy()).unwrap_or(true))
            .collect();
        if colliders.len() != 1 || ki(sc.go(gid).ok_or("go")?, "m_Layer")? != TERRAIN_LAYER {
            return err(format!("{} is not one solid terrain box", ks(sc.go(gid).ok_or("go")?, "m_Name")?));
        }
        let polys = collider_polygons(sc, gid, colliders[0].1, colliders[0].2)?;
        let xs = polys.iter().flatten().map(|p| p.0);
        let ys = polys.iter().flatten().map(|p| p.1);
        let clips: Vec<(String, String)> = match kind {
            TargetKind::TollGate => vec![("Opening".into(), clip_of_state(fsm, who, "Open")?)],
            TargetKind::Pillar => vec![
                ("Shake".into(), clip_of_state(fsm, who, "Shake")?),
                ("Opening".into(), clip_of_state(fsm, who, "Open")?),
                ("Finish".into(), clip_of_state(fsm, who, "Finish Opening")?),
                ("Opened".into(), clip_of_state(fsm, who, "Opened")?),
            ],
            TargetKind::MetalGate => vec![
                ("Opening".into(), clip_of_state(fsm, who, "Open")?),
                ("Opened".into(), clip_of_state(fsm, who, "Activate")?),
            ],
        };
        out.push(Target {
            kind,
            gid,
            name: ks(sc.go(gid).ok_or("go")?, "m_Name")?,
            source: format!("{}:{}", file_of(sc), o.id),
            position,
            scale,
            collider_source: sc.sid(colliders[0].0),
            box_: [py_min(xs.clone()), py_min(ys.clone()), py_max(xs), py_max(ys)],
            idle: default_clip(source, sc, animator)?,
            clips,
            active,
        });
    }
    Ok(out)
}

// ------------------------------------------------------------------ the scenes

/// One admitted scene's interactables.
pub struct SceneSet {
    pub guest: usize,
    pub file: String,
    pub name: String,
    pub switches: Vec<Switch>,
    pub targets: Vec<Target>,
}

/// Every admitted scene that holds a switch or a target.
pub fn survey(source: &Source) -> Result<Vec<SceneSet>> {
    let mut out = Vec::new();
    for row in crate::quality::SCENE_TABLE.iter() {
        let sc = Scene::new(source, row.file).map_err(|e| e.to_string())?;
        let switches = switches(source, &sc)?;
        let targets: Vec<Target> = targets(source, &sc)?.into_iter().filter(|t| t.active).collect();
        if switches.is_empty() && targets.is_empty() {
            continue;
        }
        for s in &switches {
            for t in &s.targets {
                // A target that is not a gate, pillar or toll gate of this table (the town's lift) is the
                // lift's business; a switch that points at nothing known is refused.
                if !targets.iter().any(|x| x.gid == *t) {
                    return err(format!("{}: switch {:?} opens object {t}, which no cooked target is", row.scene_name, s.name));
                }
            }
        }
        out.push(SceneSet { guest: row.scene_id, file: row.file.to_owned(), name: row.scene_name.to_owned(), switches, targets });
    }
    Ok(out)
}

// ------------------------------------------------------------------ art

/// The clips an instance plays, by role: Idle, Hit, Activated, Opening, Shake, Finish, Opened.
pub const ROLES: usize = 7;
pub const NO_CLIP: u16 = u16::MAX;

/// One sprite at one object scale, cut into pieces of at most one slot.
#[derive(Clone, Debug)]
pub struct SpriteRec {
    pub first: usize,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct PieceRec {
    pub sheet: usize,
    pub rect: [usize; 4],
    pub box_: [f64; 4],
}

#[derive(Clone, Debug)]
pub struct ClipRec {
    pub fps: f64,
    pub looping: bool,
    pub sprites: Vec<usize>,
}

/// A scene's sheets and the tables that find a sprite on them.
pub struct SceneArt {
    pub sheets: Vec<Image>,
    pub clips: Vec<ClipRec>,
    pub sprites: Vec<SpriteRec>,
    pub pieces: Vec<PieceRec>,
    /// (library sid, clip name, |x scale| bits, |y scale| bits) to the clip.
    index: HashMap<(String, String, u64, u64), usize>,
}

impl SceneArt {
    fn clip(&self, library: &str, name: &str, scale: [f64; 2]) -> Result<u16> {
        self.index
            .get(&(library.to_owned(), name.to_owned(), scale[0].abs().to_bits(), scale[1].abs().to_bits()))
            .map(|&c| c as u16)
            .ok_or_else(|| format!("no cooked clip {name:?} of {library} at scale {scale:?}"))
    }
}

/// An object to cook art for: its animator's library, scale and the clips it plays.
struct Wanted {
    library: Obj,
    scale: [f64; 2],
    clips: Vec<String>,
}

fn wanted(source: &Source, sc: &Scene, gid: i64, scale: [f64; 2], clips: Vec<String>) -> Result<Wanted> {
    let (_, animator) = art_parts(sc, gid)?;
    let library = source.deref(&sc.base, k(animator, "library")?).map_err(|e| e.to_string())?;
    Ok(Wanted { library, scale, clips })
}

fn objects_of(source: &Source, sc: &Scene, set: &SceneSet) -> Result<Vec<Wanted>> {
    let mut out = Vec::new();
    for s in &set.switches {
        out.push(wanted(source, sc, s.gid, s.scale, vec![s.idle.clone(), s.hit.clone(), s.activated.clone()])?);
    }
    for t in &set.targets {
        let mut clips = vec![t.idle.clone()];
        clips.extend(t.clips.iter().map(|c| c.1.clone()));
        out.push(wanted(source, sc, t.gid, t.scale, clips)?);
    }
    Ok(out)
}

/// Cut a scene's interactables out of the tk2d collections and pack them into sheets.
pub fn scene_art(source: &Source, sc: &Scene, set: &SceneSet) -> Result<SceneArt> {
    use crate::battle_gates::pack_images;
    let scale = focal() / -CAM_Z;
    let mut textures: HashMap<String, Arc<Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    // (collection sid, sprite id, |sx| bits, |sy| bits) to the sprite record.
    let mut variants: HashMap<(String, i64, u64, u64), usize> = HashMap::new();
    let mut variant_order: Vec<(Obj, i64, [f64; 2])> = Vec::new();
    let mut clips: Vec<ClipRec> = Vec::new();
    let mut index = HashMap::new();
    for w in objects_of(source, sc, set)? {
        let library = source.read(&w.library).map_err(|e| e.to_string())?;
        let lib_sid = w.library.sid();
        for name in &w.clips {
            let key = (lib_sid.clone(), name.clone(), w.scale[0].abs().to_bits(), w.scale[1].abs().to_bits());
            if index.contains_key(&key) {
                continue;
            }
            let clip = kl(&library, "clips")?
                .iter()
                .rev()
                .find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name.as_str()))
                .ok_or_else(|| format!("{lib_sid} has no clip {name:?}"))?;
            let wrap = clip.get("wrapMode").and_then(Value::int).unwrap_or(-1);
            let fps = clip.get("fps").and_then(Value::float).unwrap_or(0.0);
            if fps != fps.trunc() || !(0.0..256.0).contains(&fps) || !matches!(wrap, 0 | 2 | 6) {
                return err(format!("interactable clip {name:?} is not a whole-rate loop or play-once clip"));
            }
            let mut sprites = Vec::new();
            for frame in kl(clip, "frames")? {
                let co = source.deref(&w.library.file, k(frame, "spriteCollection")?).map_err(|e| e.to_string())?;
                let id = ki(frame, "spriteId")?;
                let vk = (co.sid(), id, w.scale[0].abs().to_bits(), w.scale[1].abs().to_bits());
                let at = match variants.get(&vk) {
                    Some(&at) => at,
                    None => {
                        variant_order.push((co.clone(), id, w.scale));
                        variants.insert(vk, variant_order.len() - 1);
                        variant_order.len() - 1
                    }
                };
                sprites.push(at);
            }
            index.insert(key, clips.len());
            clips.push(ClipRec { fps, looping: wrap == 0, sprites });
        }
    }
    // Cut every variant into pieces of at most one slot.
    let mut images: Vec<Image> = Vec::new();
    let mut pieces: Vec<PieceRec> = Vec::new();
    let mut sprites: Vec<SpriteRec> = Vec::new();
    for (co, id, s) in &variant_order {
        let sid = co.sid();
        if !collections.contains_key(&sid) {
            collections.insert(sid.clone(), source.read(co).map_err(|e| e.to_string())?);
        }
        let (image, b) = tk_sprite(source, &co.file, &collections[&sid], *id as usize, &mut textures)?;
        let (kx, ky) = (s[0].abs(), s[1].abs());
        let b = [b[0] * kx, b[1] * ky, b[2] * kx, b[3] * ky];
        let w = (((b[2] - b[0]) * scale).ceil() as usize).max(1);
        let h = (((b[3] - b[1]) * scale).ceil() as usize).max(1);
        let image = image.to_rgba().resize(w, h, Filter::Lanczos);
        let first = pieces.len();
        let slot = crate::atlas::SLOT_PIXELS;
        let mut py = 0;
        while py < h {
            let ph = slot.min(h - py);
            let mut px = 0;
            while px < w {
                let pw = slot.min(w - px);
                let x0 = b[0] + (b[2] - b[0]) * px as f64 / w as f64;
                let x1 = b[0] + (b[2] - b[0]) * (px + pw) as f64 / w as f64;
                let y1 = b[3] - (b[3] - b[1]) * py as f64 / h as f64;
                let y0 = b[3] - (b[3] - b[1]) * (py + ph) as f64 / h as f64;
                images.push(image.crop_int(px as i64, py as i64, (px + pw) as i64, (py + ph) as i64));
                pieces.push(PieceRec { sheet: 0, rect: [0; 4], box_: [x0, y0, x1, y1] });
                px += pw;
            }
            py += ph;
        }
        sprites.push(SpriteRec { first, count: pieces.len() - first });
    }
    let refs: Vec<&Image> = images.iter().collect();
    let (sheets, rects) = pack_images(&refs);
    for (p, r) in pieces.iter_mut().zip(&rects) {
        p.sheet = r[0];
        p.rect = [r[1], r[2], r[3], r[4]];
    }
    if sheets.len() > 32 {
        return err(format!("{} interactable sheets in one scene exceed the guest's 32", sheets.len()));
    }
    Ok(SceneArt { sheets, clips, sprites, pieces, index })
}

/// The sheets of every scene, for `hk-cook sheet-art`'s index: scene file to the sheets' files and sizes.
pub fn cook_sheets(source: &Source, dir: &Path) -> Result<Vec<(String, Vec<(usize, usize, String)>)>> {
    let mut out = Vec::new();
    for set in survey(source)? {
        let sc = Scene::new(source, &set.file).map_err(|e| e.to_string())?;
        let art = scene_art(source, &sc, &set)?;
        let mut sheets = Vec::new();
        for (i, sheet) in art.sheets.iter().enumerate() {
            let file = format!("{}-interact-{i}.rgba", set.file);
            std::fs::write(dir.join(&file), &sheet.data).map_err(|e| e.to_string())?;
            sheets.push((sheet.width, sheet.height, file));
        }
        out.push((set.file.clone(), sheets));
    }
    Ok(out)
}

// ------------------------------------------------------------------ tables

const MACHINE_TOLL_SWITCH: u8 = 0;
const MACHINE_SWITCH_CONTROL: u8 = 1;
const MACHINE_TOLL_GATE: u8 = 2;
const MACHINE_PILLAR: u8 = 3;
const MACHINE_METAL_GATE: u8 = 4;

fn q16(v: f64) -> i64 {
    crate::common::py_round(v * 65536.0)
}

/// The edge exclusions every other generated table already spends per catalogue slot.
fn other_edges(root: &Path) -> HashMap<usize, usize> {
    let mut spent = neighbour_edges(root);
    for (slot, n) in region_edge_counts(root) {
        *spent.entry(slot).or_default() += n;
    }
    for (slot, n) in crate::battle_gates::edge_counts_in(root, "data/blocker_terrain.rs") {
        *spent.entry(slot).or_default() += n;
    }
    spent
}

/// Generate `data/interactables.rs` and `.hkpsx/interactables.json` from the source scenes and the region report.
pub fn main(root: &Path, source: Option<&Path>) -> Result<()> {
    use std::fmt::Write;
    let source = match source {
        Some(p) => Source::new(p),
        None => Source::from_doctor(root),
    }
    .map_err(|e| e.to_string())?;
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("data/regions.json")).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let regions = report["regions"].as_array().ok_or("regions")?;
    let sets = survey(&source)?;
    // Global tables.
    let mut switch_rows = String::new();
    let mut target_rows = String::new();
    let mut links: Vec<u8> = Vec::new();
    let mut clip_rows = String::new();
    let mut clip_sprites: Vec<usize> = Vec::new();
    let mut sprite_rows = String::new();
    let mut piece_rows = String::new();
    let (mut n_switch, mut n_target, mut n_clip, mut n_sprite, mut n_piece) = (0usize, 0usize, 0usize, 0usize, 0usize);
    // (scene, first global target, collider source) for the edge join.
    let mut colliders: Vec<(usize, usize, String)> = Vec::new();
    let mut art_rows: Vec<(usize, usize)> = Vec::new();
    let mut summary = Vec::new();
    for set in &sets {
        let sc = Scene::new(&source, &set.file).map_err(|e| e.to_string())?;
        let art = scene_art(&source, &sc, set)?;
        let (clip_base, sprite_base, piece_base) = (n_clip, n_sprite, n_piece);
        for p in &art.pieces {
            let r = p.rect;
            write!(piece_rows, "({},[{},{},{},{}],[{},{},{},{}]),", p.sheet, r[0], r[1], r[2], r[3], q16(p.box_[0]), q16(p.box_[1]), q16(p.box_[2]), q16(p.box_[3])).unwrap();
        }
        n_piece += art.pieces.len();
        for s in &art.sprites {
            write!(sprite_rows, "({},{}),", piece_base + s.first, s.count).unwrap();
        }
        n_sprite += art.sprites.len();
        for c in &art.clips {
            write!(clip_rows, "Clip{{first:{},count:{},fps:{},looping:{}}},", clip_sprites.len(), c.sprites.len(), c.fps as i64, c.looping).unwrap();
            clip_sprites.extend(c.sprites.iter().map(|s| sprite_base + s));
        }
        n_clip += art.clips.len();
        let target_base = n_target;
        let scene_target = |gid: i64| set.targets.iter().position(|t| t.gid == gid).unwrap();
        for (si, s) in set.switches.iter().enumerate() {
            let lib = switch_library(&source, &sc, s.gid)?;
            let mut clips = [NO_CLIP; ROLES];
            clips[0] = art.clip(&lib, &s.idle, s.scale)?;
            clips[1] = art.clip(&lib, &s.hit, s.scale)?;
            clips[2] = art.clip(&lib, &s.activated, s.scale)?;
            let machine = if s.kind == SwitchKind::Toll { MACHINE_TOLL_SWITCH } else { MACHINE_SWITCH_CONTROL };
            let link_first = links.len();
            links.extend(s.targets.iter().map(|g| scene_target(*g) as u8));
            let flip = (s.scale[0] < 0.0) as u8 | ((s.scale[1] < 0.0) as u8) << 1;
            write!(
                switch_rows,
                "Switch{{scene:{},local:{},machine:{machine},flip:{flip},pos:[{},{}],hit:[{},{},{},{}],clips:[{}],frame:{},links:{link_first},link_count:{}}},",
                set.guest,
                si,
                q16(s.position[0]),
                q16(s.position[1]),
                q16(s.hit_box[0]),
                q16(s.hit_box[1]),
                q16(s.hit_box[2]),
                q16(s.hit_box[3]),
                clips.iter().map(|c| if *c == NO_CLIP { "NO_CLIP".to_owned() } else { (clip_base + *c as usize).to_string() }).collect::<Vec<_>>().join(","),
                s.activated_frame.unwrap_or(0),
                s.targets.len()
            )
            .unwrap();
            n_switch += 1;
        }
        for (ti, t) in set.targets.iter().enumerate() {
            let lib = target_library(&source, &sc, t.gid)?;
            let mut clips = [NO_CLIP; ROLES];
            clips[0] = art.clip(&lib, &t.idle, t.scale)?;
            let role_of = |name: &str| match name {
                "Opening" => 3,
                "Shake" => 4,
                "Finish" => 5,
                _ => 6,
            };
            for (role, clip) in &t.clips {
                clips[role_of(role)] = art.clip(&lib, clip, t.scale)?;
            }
            let (machine, local) = match t.kind {
                TargetKind::TollGate => (MACHINE_TOLL_GATE, 255usize),
                TargetKind::Pillar => (MACHINE_PILLAR, 8 + ti),
                TargetKind::MetalGate => (MACHINE_METAL_GATE, 255),
            };
            let flip = (t.scale[0] < 0.0) as u8 | ((t.scale[1] < 0.0) as u8) << 1;
            write!(
                target_rows,
                "Target{{scene:{},local:{local},machine:{machine},flip:{flip},pos:[{},{}],clips:[{}]}},",
                set.guest,
                q16(t.position[0]),
                q16(t.position[1]),
                clips.iter().map(|c| if *c == NO_CLIP { "NO_CLIP".to_owned() } else { (clip_base + *c as usize).to_string() }).collect::<Vec<_>>().join(",")
            )
            .unwrap();
            colliders.push((set.guest, target_base + ti, t.collider_source.clone()));
            n_target += 1;
        }
        // The view frames of this scene's sheets.
        let mut frames = 0;
        for (slot, region) in regions.iter().enumerate() {
            if region["scene_id"].as_u64() != Some(set.guest as u64) {
                continue;
            }
            let Some(a) = region.get("interact_art").filter(|a| !a.is_null()) else {
                return err(format!("view {slot} of {} carries no interactable art: run the region cook", set.name));
            };
            if a["sheets"].as_u64() != Some(art.sheets.len() as u64) {
                return err(format!("view {slot} of {} disagrees about the interactable sheets", set.name));
            }
            art_rows.push((slot, a["frame_base"].as_u64().ok_or("frame_base")? as usize));
            frames += 1;
        }
        summary.push(format!(
            "{}: {} switches, {} targets, {} sheets, {} pieces, {} clips, {frames} views",
            set.name,
            set.switches.len(),
            set.targets.len(),
            art.sheets.len(),
            art.pieces.len(),
            art.clips.len()
        ));
    }
    // The edge join: the cook's own `edge_sources` name every cooked edge's collider.
    let budget = edge_scratch_slots(root)?;
    let others = other_edges(root);
    let owner: HashMap<&str, (usize, usize)> = colliders.iter().map(|(scene, t, c)| (c.as_str(), (*scene, *t))).collect();
    let mut edge_rows = String::new();
    let mut edge_list: Vec<usize> = Vec::new();
    let mut bound = std::collections::BTreeSet::new();
    for (slot, region) in regions.iter().enumerate() {
        let mut found: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
        for (edge, src) in region["edge_sources"].as_array().ok_or("edge_sources")?.iter().enumerate() {
            let Some(&(scene, target)) = src.as_str().and_then(|s| owner.get(s)) else { continue };
            if region["scene_id"].as_u64() != Some(scene as u64) {
                return err("an interactable's terrain edge outside its own scene");
            }
            found.entry(target).or_default().push(edge);
        }
        let spent = found.values().map(Vec::len).sum::<usize>() + others.get(&slot).copied().unwrap_or(0);
        if !found.is_empty() && spent > budget {
            return err(format!(
                "catalogue slot {slot} ({}) needs {spent} scripted edge exclusions against {budget} in game/src/world.rs",
                region["scene_name"].as_str().unwrap_or("")
            ));
        }
        for (target, edges) in found {
            bound.insert(target);
            write!(edge_rows, "({slot},{target},{},{}),", edge_list.len(), edges.len()).unwrap();
            edge_list.extend(edges);
        }
    }
    let unbound: Vec<usize> = (0..n_target).filter(|t| !bound.contains(t)).collect();
    let mut lines = vec![
        "// Generated from the source switch, gate and pillar FSMs and the cooked region edge sources.".to_owned(),
        "// host/hk-cook/src/interactables.rs is the generator.".to_owned(),
        "pub const NO_CLIP:u16=65535;".to_owned(),
        "/// The most switches and targets one scene holds: the guest sizes its scene state by them.".to_owned(),
        format!("pub const SCENE_SWITCHES:usize={};", sets.iter().map(|s| s.switches.len()).max().unwrap_or(1).max(1)),
        format!("pub const SCENE_TARGETS:usize={};", sets.iter().map(|s| s.targets.len()).max().unwrap_or(1).max(1)),
        format!("pub static SWITCHES:[Switch;{n_switch}]=[{switch_rows}];"),
        format!("pub static TARGETS:[Target;{n_target}]=[{target_rows}];"),
        "/// Per switch: the scene-local indices (into its scene's targets, in table order) it sends to.".to_owned(),
        format!("pub static LINKS:[u8;{}]=[{}];", links.len(), links.iter().map(u8::to_string).collect::<Vec<_>>().join(",")),
        format!("pub static CLIPS:[Clip;{n_clip}]=[{clip_rows}];"),
        format!("pub static CLIP_SPRITES:[u16;{}]=[{}];", clip_sprites.len(), clip_sprites.iter().map(usize::to_string).collect::<Vec<_>>().join(",")),
        "/// Per sprite: its first piece and how many.".to_owned(),
        format!("pub static SPRITES:[(u16,u8);{n_sprite}]=[{sprite_rows}];"),
        "/// Per piece: its sheet, its texel rectangle [x, y, w, h] and its box around the object origin (q16).".to_owned(),
        format!("pub static PIECES:[(u8,[u8;4],[i32;4]);{n_piece}]=[{piece_rows}];"),
        "/// (catalogue slot, first sheet frame of that view's actor bank).".to_owned(),
        format!("pub static ART:&[(u16,u16)]=&[{}];", art_rows.iter().map(|(s, f)| format!("({s},{f}),")).collect::<String>()),
        "/// (catalogue slot, target, first, count into EDGE_LIST): the terrain edges the target stands in.".to_owned(),
        format!("pub static EDGES:&[(u16,u8,u16,u8)]=&[{edge_rows}];"),
        format!("pub static EDGE_LIST:&[u16]=&[{}];", edge_list.iter().map(usize::to_string).collect::<Vec<_>>().join(",")),
    ];
    lines.push(String::new());
    std::fs::write(root.join("data/interactables.rs"), lines.join("\n")).map_err(|e| e.to_string())?;
    let mut text = summary.join("\n");
    text.push('\n');
    std::fs::write(root.join(".hkpsx/interactables.txt"), &text).map_err(|e| e.to_string())?;
    print!("{text}");
    if !unbound.is_empty() {
        println!("  note: {} targets have no cooked terrain in any region: {unbound:?}", unbound.len());
    }
    Ok(())
}

fn switch_library(source: &Source, sc: &Scene, gid: i64) -> Result<String> {
    let (_, animator) = art_parts(sc, gid)?;
    Ok(source.deref(&sc.base, k(animator, "library")?).map_err(|e| e.to_string())?.sid())
}

fn target_library(source: &Source, sc: &Scene, gid: i64) -> Result<String> {
    switch_library(source, sc, gid)
}

/// Print what the scenes hold (development report; the tables come from `main`).
pub fn report(root: &Path, scenes: &[String]) -> Result<()> {
    let source = Source::from_doctor(root).map_err(|e| e.to_string())?;
    for name in scenes {
        let file = crate::quality::SCENE_TABLE
            .iter()
            .find(|r| r.scene_name == name.as_str())
            .map(|r| r.file.to_owned())
            .unwrap_or_else(|| name.clone());
        let sc = Scene::new(&source, &file).map_err(|e| e.to_string())?;
        for s in switches(&source, &sc)? {
            let digest = fsm_digest(
                k(&sc.object(s.source.rsplit(':').next().unwrap().parse().unwrap()).ok_or("fsm")?.tree, "fsm")?,
                &["Activated", "SetPlayerData", "Player Data"],
            )?;
            println!("{name}: switch {:?} {:?} at {:?} scale {:?} hit {:?} wait {} idle {:?} hit {:?} activated {:?}/{:?} targets {:?} pd {:?} digest {digest}",
                s.kind, s.name, s.position, s.scale, s.hit_box, s.wait, s.idle, s.hit, s.activated, s.activated_frame, s.targets, s.player_data);
        }
        for t in targets(&source, &sc)? {
            let digest = fsm_digest(k(&sc.object(t.source.rsplit(':').next().unwrap().parse().unwrap()).ok_or("fsm")?.tree, "fsm")?, &[])?;
            println!("{name}: target {:?} {:?} gid {} at {:?} scale {:?} box {:?} idle {:?} clips {:?} active {} collider {} digest {digest}",
                t.kind, t.name, t.gid, t.position, t.scale, t.box_, t.idle, t.clips, t.active, t.collider_source);
        }
    }
    for set in survey(&source)? {
        if !scenes.is_empty() && !scenes.contains(&set.name) {
            continue;
        }
        let sc = Scene::new(&source, &set.file).map_err(|e| e.to_string())?;
        let art = scene_art(&source, &sc, &set)?;
        let texels: usize = art.pieces.iter().map(|p| p.rect[2] * p.rect[3]).sum();
        let sheet_texels: usize = art.sheets.iter().map(|i| i.width * i.height).sum();
        println!("{}: art {} sheets ({} texels of {} on sheets), {} pieces, {} sprites, {} clips", set.name, art.sheets.len(), texels, sheet_texels, art.pieces.len(), art.sprites.len(), art.clips.len());
        for (i, c) in art.clips.iter().enumerate() {
            println!("   clip {i}: fps {} loop {} sprites {:?}", c.fps, c.looping, c.sprites);
        }
        for (i, sp) in art.sprites.iter().enumerate() {
            let w: usize = art.pieces[sp.first..sp.first + sp.count].iter().map(|p| p.rect[2]).max().unwrap_or(0);
            println!("   sprite {i}: {} pieces first {} (piece0 {:?})", sp.count, sp.first, art.pieces[sp.first].rect);
            let _ = w;
        }
    }
    Ok(())
}
