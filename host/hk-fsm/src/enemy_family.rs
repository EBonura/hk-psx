//! The enemy-family cooker: host/enemy_families.json rows to HKFSM03 banks and the tables the guest's
//! `hk_sim::fsm_actor` reads (docs/ENEMY-FAMILIES.md).
//!
//! For each row it finds every placement of the family in the admitted scenes (the scene files of
//! `data/regions.json`), takes the FSMs the row names (the inert ones dropped, each logged), cooks them as one
//! tree, requires every placement to cook to the same bank, and writes
//!
//! * `data/<name>3.hkf3`, `data/<name>3_names.txt`, `data/<name>3_families.txt` (the action families, which
//!   `host/code_modules.py` turns into the modules a room carries), and
//! * one `data/enemy_families.rs` for all rows: the bank, its runtime size, the clips (frames, rate, wrap),
//!   the clip a string of the tree names, the clip lengths the animation actions need, the hurt box each
//!   frame of a clip puts on the sprite (tk2d writes a Box sprite's collider into the object each time the
//!   frame changes). The trigger box of the child the tree listens on is authored per placement, so it rides in the
//!   control record and the actor spec (host/enemy_family.py).
//!
//! Python twin of the admission rules: host/enemy_family.py. Rust twin: host/hk-cook/src/enemy_family.rs.
use crate::cook3::{cook_tree, write_families_pub, write_names_pub, Cooked};
use hk_format::fsm3::Bank;
use hk_unity::scene::Scene;
use hk_unity::{Source, Value};
use std::collections::BTreeSet;
use std::fmt::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// One FSM a row names.
struct FsmRow {
    on: String,
    name: String,
    inert: bool,
}

/// One manifest row.
struct Row {
    name: String,
    id: u8,
    prefixes: Vec<String>,
    fsms: Vec<FsmRow>,
    rest: String,
    clips: Vec<String>,
}

fn manifest(root: &std::path::Path) -> Result<Vec<Row>> {
    let text = std::fs::read(root.join("host/enemy_families.json"))?;
    let json: serde_json::Value = serde_json::from_slice(&text)?;
    let mut rows = Vec::new();
    for row in json["families"].as_array().ok_or("families")? {
        let strings = |key: &str| -> Result<Vec<String>> {
            row[key]
                .as_array()
                .ok_or(format!("manifest row without {key}"))?
                .iter()
                .map(|s| s.as_str().map(str::to_owned).ok_or_else(|| "manifest string".into()))
                .collect()
        };
        let mut fsms = Vec::new();
        for f in row["fsms"].as_array().ok_or("fsms")? {
            fsms.push(FsmRow {
                on: f["on"].as_str().ok_or("fsm.on")?.to_owned(),
                name: f["name"].as_str().ok_or("fsm.name")?.to_owned(),
                inert: f.get("inert").is_some(),
            });
        }
        rows.push(Row {
            name: row["name"].as_str().ok_or("name")?.to_owned(),
            id: u8::try_from(row["id"].as_u64().ok_or("id")?)?,
            prefixes: strings("name_prefixes")?,
            fsms,
            rest: row["rest"].as_str().ok_or("rest")?.to_owned(),
            clips: strings("clips")?,
        });
    }
    for (i, r) in rows.iter().enumerate() {
        if usize::from(r.id) != i {
            return Err(format!("enemy family {} has id {}, expected {i}", r.name, r.id).into());
        }
    }
    Ok(rows)
}

fn go_of(tree: &Value) -> Option<i64> {
    tree.get("m_GameObject").and_then(|g| g.get("m_PathID")).and_then(Value::int)
}

/// The `(path id, type, tree)` of every object of the scene on GameObject `gid`.
fn records<'a>(sc: &'a Scene, gid: i64) -> Vec<(i64, &'a str, &'a Value)> {
    sc.objects.iter().filter(|o| go_of(&o.tree) == Some(gid)).map(|o| (o.id, o.typename.as_str(), &o.tree)).collect()
}

/// Child GameObjects of `gid` by name.
fn children(sc: &Scene, gid: i64) -> Result<Vec<(String, i64)>> {
    let tid = *sc.go_transform.get(&gid).ok_or("actor has no transform")?;
    let mut out = Vec::new();
    for c in sc.transform(tid).and_then(|t| t.get("m_Children")).and_then(Value::list).unwrap_or(&[]) {
        let child_t = c.get("m_PathID").and_then(Value::int).ok_or("child")?;
        let kid = sc.transform(child_t).and_then(go_of).ok_or("child object")?;
        let name = sc.go(kid).and_then(|g| g.get("m_Name")).and_then(Value::str).unwrap_or_default();
        out.push((name, kid));
    }
    Ok(out)
}

fn q16(v: f64) -> i32 {
    (v * 65536.0).round_ties_even() as i32
}

/// A placement of a family: the FSMs of the tree in manifest order, with the object each one owns.
struct Placement {
    scene: String,
    source_id: i64,
    fsms: Vec<(i64, Value)>,
    library: String,
}

/// Every placement of `row` in one scene.
fn scan(sc: &Scene, row: &Row, out: &mut Vec<Placement>) -> Result<()> {
    for o in &sc.objects {
        if o.typename != "HealthManager" || !o.tree.get("m_Enabled").is_some_and(Value::truthy) {
            continue;
        }
        let Some(gid) = go_of(&o.tree) else { continue };
        let name = sc.go(gid).and_then(|g| g.get("m_Name")).and_then(Value::str).unwrap_or_default();
        if !row.prefixes.iter().any(|p| name.starts_with(p.as_str())) || !sc.active(gid) {
            continue;
        }
        let own = records(sc, gid);
        let kids = children(sc, gid)?;
        let mut fsms = Vec::new();
        for want in &row.fsms {
            let owner = if want.on.is_empty() {
                gid
            } else {
                kids.iter().find(|k| k.0 == want.on).ok_or(format!("{name}: no child {}", want.on))?.1
            };
            let hits: Vec<&Value> = records(sc, owner)
                .into_iter()
                .filter(|r| r.1 == "PlayMakerFSM")
                .filter_map(|r| r.2.get("fsm"))
                .filter(|f| f.get("name").and_then(Value::str).as_deref() == Some(want.name.as_str()))
                .collect();
            if hits.len() != 1 {
                return Err(format!("{name}: expected one FSM {} on {:?}", want.name, want.on).into());
            }
            if !want.inert {
                fsms.push((owner, hits[0].clone()));
            }
        }
        let animator = own
            .iter()
            .find(|r| r.1 == "tk2dSpriteAnimator")
            .ok_or(format!("{name}: no animator"))?
            .2;
        let library = sc.deref(animator.get("library").ok_or("library")?)?.sid();
        out.push(Placement { scene: sc.base.name.clone(), source_id: o.id, fsms, library });
    }
    Ok(())
}

/// A clip as the guest reads it.
struct ClipRow {
    frames: u8,
    fps: u8,
    wrap: u8,
    loop_start: u8,
}

/// The cooked clips of the row's library and the box each of their frames puts on the sprite.
struct Library {
    clips: Vec<ClipRow>,
    boxes: Vec<(u8, u8, [i32; 4])>,
}

fn library(sc: &Scene, row: &Row) -> Result<Library> {
    let source = sc.source;
    // The first placement's animator names the library; every placement's library id was checked equal.
    let hm = sc
        .objects
        .iter()
        .find(|o| {
            o.typename == "HealthManager"
                && go_of(&o.tree)
                    .and_then(|g| sc.go(g))
                    .and_then(|g| g.get("m_Name"))
                    .and_then(Value::str)
                    .is_some_and(|n| row.prefixes.iter().any(|p| n.starts_with(p.as_str())))
        })
        .ok_or("no placement")?;
    let gid = go_of(&hm.tree).ok_or("object")?;
    let animator = records(sc, gid).into_iter().find(|r| r.1 == "tk2dSpriteAnimator").ok_or("animator")?.2;
    let lib_obj = sc.deref(animator.get("library").ok_or("library")?)?;
    let lib = source.read(&lib_obj)?;
    let mut clips = Vec::new();
    let mut boxes = Vec::new();
    for (slot, name) in row.clips.iter().enumerate() {
        let clip = lib
            .get("clips")
            .and_then(Value::list)
            .unwrap_or(&[])
            .iter()
            .find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name.as_str()))
            .ok_or(format!("library lacks clip {name}"))?;
        let frames = clip.get("frames").and_then(Value::list).ok_or("frames")?;
        let fps = clip.get("fps").and_then(Value::float).ok_or("fps")?;
        if fps.fract() != 0.0 || !(1.0..=60.0).contains(&fps) || frames.is_empty() || frames.len() > 255 {
            return Err(format!("clip {name}: unsupported rate or length").into());
        }
        let wrap = clip.get("wrapMode").and_then(Value::int).ok_or("wrapMode")?;
        let wrap = match (wrap, frames.len()) {
            (0..=2, _) => wrap as u8,
            // Single holds one frame, which is Once for a one-frame clip.
            (6, 1) => 2,
            _ => return Err(format!("clip {name}: unsupported wrap mode {wrap}").into()),
        };
        let loop_start = clip.get("loopStart").and_then(Value::int).unwrap_or(0) as u8;
        clips.push(ClipRow { frames: frames.len() as u8, fps: fps as u8, wrap, loop_start });
        let mut held: Option<[i32; 4]> = None;
        for (index, frame) in frames.iter().enumerate() {
            let coll = source.deref(&lib_obj.file, frame.get("spriteCollection").ok_or("spriteCollection")?)?;
            let collection = source.read(&coll)?;
            let id = frame.get("spriteId").and_then(Value::int).ok_or("spriteId")? as usize;
            let def = collection
                .get("spriteDefinitions")
                .and_then(Value::list)
                .and_then(|d| d.get(id))
                .ok_or("sprite definition")?;
            // Box (2): the centre and half extents of the frame. None (1): no collider. Unset (0): the last box stays.
            let now = match def.get("colliderType").and_then(Value::int) {
                Some(2) => {
                    let v = def.get("colliderVertices").and_then(Value::list).ok_or("colliderVertices")?;
                    let n = |i: usize, k: &str| v.get(i).and_then(|p| p.get(k)).and_then(Value::float).ok_or("vertex");
                    let (cx, cy, hx, hy) = (n(0, "x")?, n(0, "y")?, n(1, "x")?, n(1, "y")?);
                    Some([q16(cx - hx), q16(cy - hy), q16(cx + hx), q16(cy + hy)])
                }
                Some(1) => None,
                Some(0) => held,
                other => return Err(format!("clip {name} frame {index}: collider type {other:?}").into()),
            };
            held = now;
            // A dead actor takes no hit, so the death clip's frames need no box.
            if let Some(b) = now.filter(|_| name != "Death") {
                boxes.push((slot as u8, index as u8, b));
            }
        }
    }
    Ok(Library { clips, boxes })
}

fn rust_bytes(name: &str) -> String {
    name.to_ascii_uppercase()
}

/// Cook every row of the manifest. `data/regions.json` names the scenes the build admits.
pub fn cook_families(root: &std::path::Path) -> Result<()> {
    let rows = manifest(root)?;
    let source = Source::from_doctor(root)?;
    let regions: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("data/regions.json"))?)?;
    let mut files = BTreeSet::new();
    for region in regions["regions"].as_array().ok_or("regions")? {
        files.insert(region["scene_file"].as_str().ok_or("scene_file")?.to_owned());
    }
    let mut scenes = Vec::new();
    for file in &files {
        scenes.push(Scene::new(&source, file).map_err(|e| e.to_string())?);
    }
    let mut rs = String::from("// Generated by host/hk-fsm enemy_family.rs; do not edit.\n");
    let mut families = String::new();
    let mut runtimes = Vec::new();
    for row in &rows {
        let mut placements = Vec::new();
        for sc in &scenes {
            scan(sc, row, &mut placements)?;
        }
        if placements.is_empty() {
            return Err(format!("enemy family {}: no placement in the admitted scenes", row.name).into());
        }
        let mut cooked: Option<Cooked> = None;
        let mut dropped = 0usize;
        for p in &placements {
            let trees: Vec<(i64, &Value)> = p.fsms.iter().map(|(g, f)| (*g, f)).collect();
            let c = cook_tree(&trees).map_err(|e| format!("{} {}:{}: {e}", row.name, p.scene, p.source_id))?;
            match &cooked {
                None => cooked = Some(c),
                Some(first) if first.bank != c.bank => {
                    return Err(format!("{}: placements cook to different banks ({}:{})", row.name, p.scene, p.source_id).into())
                }
                _ => {}
            }
            dropped += row.fsms.iter().filter(|f| f.inert).count();
        }
        let bank = cooked.ok_or("bank")?;
        if let Some(p) = placements.iter().find(|p| p.library != placements[0].library) {
            return Err(format!("{}: {}:{} has another animation library than {}:{}", row.name, p.scene, p.source_id, placements[0].scene, placements[0].source_id).into());
        }
        // The library is read once, from the first scene that holds a placement.
        let lib_scene = scenes
            .iter()
            .find(|sc| sc.base.name == placements[0].scene)
            .ok_or("scene of the first placement")?;
        let lib = library(lib_scene, row)?;
        let runtime = Bank::parse(&bank.bank).map_err(|e| format!("{e:?}"))?.runtime_bytes();
        // Which clip a string of the tree names, and how long the finite ones the animation actions wait for are.
        let mut slots = Vec::new();
        let mut env_clips = Vec::new();
        for (slot, clip) in row.clips.iter().enumerate() {
            let Some(id) = bank.string_id(clip) else { continue };
            slots.push((id, slot as u8));
            let c = &lib.clips[slot];
            if c.wrap == 2 {
                let ticks = u32::from(c.frames) * 60;
                if ticks % u32::from(c.fps) != 0 {
                    return Err(format!("{}: clip {clip} does not last a whole number of ticks", row.name).into());
                }
                env_clips.push((id, ticks / u32::from(c.fps)));
            }
        }
        if env_clips.len() > 4 {
            return Err(format!("{}: {} finite clips named by the tree, the kernel keeps four", row.name, env_clips.len()).into());
        }
        let label = format!("{}3", row.name);
        write_names_pub(root, &label, &bank).map_err(|e| e.to_string())?;
        write_families_pub(root, &label, &[&bank]).map_err(|e| e.to_string())?;
        std::fs::write(root.join(format!("data/{label}.hkf3")), &bank.bank)?;
        let up = rust_bytes(&row.name);
        writeln!(rs, "pub static BANK_{up}: [u8; {}] = *include_bytes!(\"{label}.hkf3\");", bank.bank.len())?;
        writeln!(rs, "pub const RUNTIME_{up}: usize = {runtime};")?;
        let mut env = env_clips.clone();
        env.resize(4, (u32::MAX, 0));
        writeln!(
            rs,
            "static CLIPS_{up}: [ClipInfo; {}] = [{}];",
            lib.clips.len(),
            lib.clips
                .iter()
                .map(|c| format!("ClipInfo {{ frames: {}, fps: {}, wrap: {}, loop_start: {} }}", c.frames, c.fps, c.wrap, c.loop_start))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        writeln!(
            rs,
            "static BOXES_{up}: [(u8, u8, [i32; 4]); {}] = [{}];",
            lib.boxes.len(),
            lib.boxes.iter().map(|(c, f, b)| format!("({c}, {f}, {b:?})")).collect::<Vec<_>>().join(", ")
        )?;
        writeln!(
            rs,
            "static SLOTS_{up}: [(u32, u8); {}] = [{}];",
            slots.len(),
            slots.iter().map(|(i, s)| format!("({i}, {s})")).collect::<Vec<_>>().join(", ")
        )?;
        let death = row.clips.iter().position(|c| c == "Death").map_or(255, |i| i);
        let rest = row.clips.iter().position(|c| *c == row.rest).ok_or("rest clip is not a bound clip")?;
        writeln!(
            families,
            "    Family {{ name: \"{}\", bank: &BANK_{up}, clips: &CLIPS_{up}, boxes: &BOXES_{up}, slots: &SLOTS_{up}, \
             env_clips: {env:?}, death_slot: {death}, rest_slot: {rest} }},",
            row.name
        )?;
        runtimes.push(runtime);
        println!(
            "Enemy family {}: {} placements in {} scenes, {} FSMs dropped as inert, {} bank bytes, {runtime} runtime bytes, {} boxed frames",
            row.name,
            placements.len(),
            placements.iter().map(|p| p.scene.as_str()).collect::<BTreeSet<_>>().len(),
            dropped,
            bank.bank.len(),
            lib.boxes.len()
        );
    }
    writeln!(rs, "pub static FAMILIES: [Family; {}] = [\n{families}];", rows.len())?;
    writeln!(rs, "/// The widest runtime block of any family: the size of an actor's tree memory.")?;
    writeln!(rs, "pub const MAX_RUNTIME: usize = {};", runtimes.iter().max().copied().unwrap_or(0))?;
    std::fs::write(root.join("data/enemy_families.rs"), rs)?;
    Ok(())
}
