//! Greenpath's Hornet: her sprite clips, and her corpse's, appended to the banks of Fungus1_04 (the scene that loads her arena).
//!
//! The boss trees (host/hk-fsm `cook3::cook_hornet3`) say which animated objects there are and which clips of which tk2d
//! library each plays (`data/hornet3_art.json`, `data/hornet_corpse3_art.json`). This pass cooks those clips the way an actor's
//! are (streamed animation slots, no scene pages), appends them to every region of the scene after the legacy cook, as the
//! Mossman family's pass does (mossman_scene.rs), and writes the table the guest finds them by (`data/hornet_art.rs`):
//! the bank clip of each (tree, object, clip name id) and the scale each object was authored at.
use crate::actor_art::{guest_wrap_of, scale_alpha, ArtBank, Clip, Frame};
use crate::atlas::{Atlas, MAX_FRAME_TILES, MAX_TEXTURE_AXIS, SLOT_PIXELS};
use crate::common::{get, py_round};
use crate::cook::{focal, tk_sprite, CAM_Z};
use crate::{quality, quantize, region_delta};
use hk_unity::{Source, Value};
use serde_json::{json, Value as J};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The scene whose banks carry the art.
const SCENE: &str = "Fungus1_04";
const MARKER: &str = "hornet_bank_adopted";

/// One animated object of a boss tree, from the tree cooker's manifest.
struct Ent {
    tree: u8,
    ent: u8,
    library: String,
    sprite_scale: [f64; 2],
    world_scale: [f64; 2],
    alpha: f64,
    clips: Vec<(String, u32)>,
}

fn manifest(root: &Path, label: &str, tree: u8) -> Result<Vec<Ent>> {
    let v: J = serde_json::from_slice(&std::fs::read(root.join(format!("data/{label}_art.json")))?)?;
    let mut out = vec![];
    for e in v["ents"].as_array().ok_or("manifest ents")? {
        let pair = |k: &str| -> Result<[f64; 2]> {
            let a = e[k].as_array().ok_or("manifest scale")?;
            Ok([a[0].as_f64().ok_or("scale")?, a[1].as_f64().ok_or("scale")?])
        };
        out.push(Ent {
            tree,
            ent: e["ent"].as_u64().ok_or("ent")? as u8,
            library: e["library"].as_str().ok_or("library")?.to_owned(),
            sprite_scale: pair("sprite_scale")?,
            world_scale: pair("world_scale")?,
            alpha: e["alpha"].as_f64().unwrap_or(1.0),
            clips: e["clips"]
                .as_array()
                .ok_or("clips")?
                .iter()
                .map(|c| Ok((c["name"].as_str().ok_or("clip name")?.to_owned(), c["id"].as_u64().ok_or("clip id")? as u32)))
                .collect::<Result<_>>()?,
        });
    }
    Ok(out)
}

fn frame_tiles(w: i64, h: i64) -> i64 {
    (-(-w).div_euclid(SLOT_PIXELS as i64)) * (-(-h).div_euclid(SLOT_PIXELS as i64))
}

/// What a bank group is cooked at: the sprite library and the sprite's scale as it stands in the world.
type Group = (String, u64, u64, i64);

fn group_of(e: &Ent) -> Group {
    let sx = (e.sprite_scale[0] * e.world_scale[0]).abs();
    let sy = (e.sprite_scale[1] * e.world_scale[1]).abs();
    (e.library.clone(), sx.to_bits(), sy.to_bits(), py_round(e.alpha * 255.0))
}

/// Cook every clip the manifests name into one bank. Returns the bank and, per group, the bank clip of each clip name.
fn cook_bank(source: &Source, ents: &[Ent]) -> Result<(ArtBank, BTreeMap<(Group, String), usize>)> {
    let mut bank = ArtBank::new(Atlas::standard());
    let mut clip_of: BTreeMap<(Group, String), usize> = BTreeMap::new();
    let scale = focal() / -CAM_Z;
    let mut textures: HashMap<String, Arc<hk_pil::Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut cache: HashMap<(String, i64, u64, u64, i64), (usize, [f64; 4])> = HashMap::new();
    // The clips each group needs, in a fixed order.
    let mut wanted: BTreeMap<Group, (&Ent, BTreeSet<String>)> = BTreeMap::new();
    for e in ents {
        let slot = wanted.entry(group_of(e)).or_insert_with(|| (e, BTreeSet::new()));
        slot.1.extend(e.clips.iter().map(|c| c.0.clone()));
    }
    for (group, (e, names)) in &wanted {
        let (file, path) = e.library.rsplit_once(':').ok_or("library id")?;
        let library_file = source.file(file)?;
        let library_o = source.object(&library_file, path.parse()?)?;
        let library = source.read(&library_o)?;
        let (sx, sy) = (f64::from_bits(group.1), f64::from_bits(group.2));
        for name in names {
            let clip = library
                .get("clips")
                .and_then(Value::list)
                .and_then(|l| l.iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name.as_str())))
                .ok_or_else(|| format!("{} has no clip {name}", e.library))?;
            let frames = clip.get("frames").and_then(Value::list).unwrap_or(&[]);
            let start = bank.frames.len();
            for frame in frames {
                let co = source.deref(&library_o.file, get(frame, "spriteCollection")?)?;
                let sid = co.sid();
                if !collections.contains_key(&sid) {
                    collections.insert(sid.clone(), source.read(&co)?);
                }
                let index = get(frame, "spriteId")?.int().unwrap_or(0);
                let key = (sid.clone(), index, group.1, group.2, group.3);
                if !cache.contains_key(&key) {
                    let (mut image, b) = tk_sprite(source, &co.file, &collections[&sid], index as usize, &mut textures)?;
                    scale_alpha(&mut image, e.alpha);
                    let b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                    let (w, h) = (((b[2] - b[0]) * scale).ceil() as i64, ((b[3] - b[1]) * scale).ceil() as i64);
                    if w > MAX_TEXTURE_AXIS as i64 || h > MAX_TEXTURE_AXIS as i64 || frame_tiles(w, h) > MAX_FRAME_TILES as i64 {
                        return Err(format!("Hornet frame {sid}:{index} is {w}x{h}, past the {MAX_FRAME_TILES} animation slots a frame may hold").into());
                    }
                    let texture = bank.atlas.add_tiled(&image, (b[2] - b[0]) * scale, (b[3] - b[1]) * scale, &quantize::atlas_quantizer)?;
                    cache.insert(key.clone(), (texture, b));
                }
                let (texture, b) = cache[&key];
                bank.frames.push(Frame { texture, box_: b, sprite: format!("{sid}:{index}"), box_q16: None, event: frame.clone() });
            }
            clip_of.insert((group.clone(), name.clone()), bank.clips.len());
            bank.clips.push(Clip {
                name: format!("{}/{name}", e.library),
                start,
                count: frames.len(),
                fps: clip.get("fps").and_then(Value::float).unwrap_or(12.0),
                wrap: guest_wrap_of(clip, frames.len())?,
                loop_start: clip.get("loopStart").and_then(Value::int).unwrap_or(0),
            });
        }
    }
    bank.atlas.pack()?;
    Ok((bank, clip_of))
}

fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}

fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let next = path.with_extension("hornet-next");
    std::fs::write(&next, bytes)?;
    std::fs::rename(next, path)?;
    Ok(())
}

/// The pass: after the legacy region cook, before anything reads the rooms.
pub fn cook(root: &Path) -> Result<()> {
    // The scene is not admitted in every build: without it the boss has no room to appear in.
    let Some(row) = quality::SCENE_TABLE.iter().find(|r| r.scene_name == SCENE) else {
        println!("  hornet art: {SCENE} is not admitted, skipped");
        return Ok(());
    };
    let scene_id = row.scene_id as u64;
    let source = Source::from_doctor(root)?;
    let mut ents = manifest(root, "hornet3", 0)?;
    ents.extend(manifest(root, "hornet_corpse3", 1)?);
    let path = root.join("data/regions.json");
    let mut report: J = serde_json::from_slice(&std::fs::read(&path)?)?;
    if report.get(MARKER).is_some() {
        return Err("the Hornet postpass needs the unchanged legacy cook output, not a second append".into());
    }
    let (bank, clip_of) = cook_bank(&source, &ents)?;
    let regions = report["regions"].as_array_mut().ok_or("regions absent")?;
    let indices: Vec<usize> =
        regions.iter().enumerate().filter_map(|(i, r)| (r["scene_id"].as_u64() == Some(scene_id)).then_some(i)).collect();
    if indices.is_empty() {
        return Err("Fungus1_04 is not admitted".into());
    }
    let mut clip_base = 0;
    for &i in &indices {
        let raw = std::fs::read(root.join(regions[i]["path"].as_str().ok_or("room path")?))?;
        clip_base = clip_base.max(region_delta::layout(&raw)?.counts[4] as usize);
    }
    let mut outputs = vec![];
    for &i in &indices {
        let r = &mut regions[i];
        let room_path = root.join(r["path"].as_str().ok_or("room path")?);
        let raw = std::fs::read(&room_path)?;
        let pack = crate::mossman_scene::append(&raw, &bank, clip_base)?;
        let layout = region_delta::layout(&pack)?;
        r["bytes"] = json!(pack.len());
        r["sha256"] = json!(format!("{:x}", Sha256::digest(&pack)));
        r["textures"] = json!(layout.counts[1]);
        r["pages"] = json!(layout.counts[0]);
        r["stream_bytes"] = json!(word(&pack, 32));
        let grown = r["actor_bank_bytes"].as_u64().ok_or("old actor bank bytes")? + u64::try_from(pack.len() - raw.len())?;
        if grown > 256 * 1024 {
            return Err("Hornet exceeds the 256KiB actor-bank admission".into());
        }
        r["actor_bank_bytes"] = json!(grown);
        r["hornet_growth_bytes"] = json!(pack.len() - raw.len());
        outputs.push((room_path, pack));
    }
    let total = regions.iter().map(|r| r["bytes"].as_u64().unwrap()).sum::<u64>();
    report["total_pack_bytes"] = json!(total);
    report[MARKER] = json!(true);
    // The guest's table: the bank clip of every (tree, object, clip name) and the world scale each object was cooked at.
    let mut table = String::new();
    table.push_str("// Generated by host/hk-cook hornet_scene.rs; do not edit.\n");
    table.push_str(&format!("pub const HORNET_ART_SCENE:usize={scene_id};\n"));
    let mut rows: Vec<(u8, u8, u32, usize)> = vec![];
    for e in &ents {
        for (name, id) in &e.clips {
            let clip = clip_of[&(group_of(e), name.clone())];
            rows.push((e.tree, e.ent, *id, clip + clip_base));
        }
    }
    rows.sort();
    rows.dedup();
    table.push_str("/// Per tree (0 the fight, 1 the corpse), object and clip name id: the room clip that plays it.\n");
    table.push_str(&format!("pub static HORNET_ART_CLIPS: [(u8, u8, u32, u16); {}] = [\n", rows.len()));
    for (tree, ent, id, clip) in &rows {
        table.push_str(&format!("    ({tree}, {ent}, {id}, {clip}),\n"));
    }
    table.push_str("];\n");
    let q = |f: f64| py_round(f * 65536.0);
    table.push_str("/// Per animated object: the world scale it was cooked at (Q16.16), which the clips' boxes carry.\n");
    table.push_str(&format!("pub static HORNET_ART_SCALES: [(u8, u8, i32, i32); {}] = [\n", ents.len()));
    for e in &ents {
        table.push_str(&format!(
            "    ({}, {}, {}, {}),\n",
            e.tree,
            e.ent,
            q(e.sprite_scale[0].abs() * e.world_scale[0].abs()),
            q(e.sprite_scale[1].abs() * e.world_scale[1].abs())
        ));
    }
    table.push_str("];\n");
    // All of it fits before any output replaces the old cook.
    for (path, bytes) in outputs {
        publish(&path, &bytes)?;
    }
    publish(&path, &serde_json::to_vec_pretty(&report)?)?;
    publish(&root.join("data/hornet_art.rs"), table.as_bytes())?;
    Ok(())
}
