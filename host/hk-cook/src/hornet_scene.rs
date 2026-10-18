//! Greenpath's Hornet: her sprite clips, and her corpse's, appended to the banks of Fungus1_04 (the scene that loads her arena).
//!
//! The boss trees (host/hk-fsm `cook3::cook_hornet3`) say which animated objects there are and which clips of which tk2d
//! library each plays (`data/hornet3_art.json`, `data/hornet_corpse3_art.json`). This pass cooks those clips the way an actor's
//! are (streamed animation slots, no scene pages), appends them to every region of the scene after the legacy cook, as the
//! Mossman family's pass does (mossman_scene.rs), and writes the table the guest finds them by (`data/hornet_art.rs`):
//! the bank clip of each (tree, object, clip name id) and the scale each object was authored at.
use crate::actor_art::{scale_alpha, ArtBank};
use crate::common::{get, py_round};
use crate::atlas::SLOT_PIXELS;
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
pub(crate) struct Ent {
    pub(crate) tree: u8,
    pub(crate) ent: u8,
    pub(crate) library: String,
    pub(crate) sprite_scale: [f64; 2],
    pub(crate) world_scale: [f64; 2],
    pub(crate) alpha: f64,
    pub(crate) clips: Vec<(String, u32)>,
}

pub(crate) fn manifest(root: &Path, label: &str, tree: u8) -> Result<Vec<Ent>> {
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
pub(crate) type Group = (String, u64, u64, i64);

pub(crate) fn group_of(e: &Ent) -> Group {
    let sx = (e.sprite_scale[0] * e.world_scale[0]).abs();
    let sy = (e.sprite_scale[1] * e.world_scale[1]).abs();
    (e.library.clone(), sx.to_bits(), sy.to_bits(), py_round(e.alpha * 255.0))
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


/// What her art would cost in the standard boss bank (host/hk-cook fk_bank.rs `plan`), with room for everything: the measurement
/// that says whether the bank has the space (pages and stream bytes in `docs/HORNET.md`). Prints the plan's totals.
pub fn measure(root: &Path) -> Result<()> {
    use crate::fk_bank::{plan, ArtClipRec, SourceArt, SpriteKey, SpriteRec};
    let source = Source::from_doctor(root)?;
    let mut ents = manifest(root, "hornet3", 0)?;
    ents.extend(manifest(root, "hornet_corpse3", 1)?);
    let scale = focal() / -CAM_Z;
    let mut textures: HashMap<String, Arc<hk_pil::Image>> = HashMap::new();
    let mut collections: HashMap<String, Value> = HashMap::new();
    let mut art = SourceArt { sprites: vec![], clips: vec![], floor_frames: vec![], objects: crate::pyjson::Json::Null };
    let mut names: Vec<String> = vec![];
    let mut wanted: BTreeMap<Group, (&Ent, BTreeSet<String>)> = BTreeMap::new();
    for e in &ents {
        let slot = wanted.entry(group_of(e)).or_insert_with(|| (e, BTreeSet::new()));
        slot.1.extend(e.clips.iter().map(|c| c.0.clone()));
    }
    for (group, (e, clip_names)) in &wanted {
        let (file, path) = e.library.rsplit_once(':').ok_or("library id")?;
        let library_file = source.file(file)?;
        let library_o = source.object(&library_file, path.parse()?)?;
        let library = source.read(&library_o)?;
        let (sx, sy) = (f64::from_bits(group.1), f64::from_bits(group.2));
        for name in clip_names {
            let clip = library
                .get("clips")
                .and_then(Value::list)
                .and_then(|l| l.iter().find(|c| c.get("name").and_then(Value::str).as_deref() == Some(name.as_str())))
                .ok_or_else(|| format!("{} has no clip {name}", e.library))?;
            let mut keys = vec![];
            for frame in clip.get("frames").and_then(Value::list).unwrap_or(&[]) {
                let co = source.deref(&library_o.file, get(frame, "spriteCollection")?)?;
                let sid = co.sid();
                if !collections.contains_key(&sid) {
                    collections.insert(sid.clone(), source.read(&co)?);
                }
                let index = get(frame, "spriteId")?.int().unwrap_or(0);
                let key = SpriteKey::tk(&sid, index, sx, sy, e.alpha);
                if !art.sprites.iter().any(|s| s.0 == key) {
                    let (mut image, b) = tk_sprite(&source, &co.file, &collections[&sid], index as usize, &mut textures)?;
                    scale_alpha(&mut image, e.alpha);
                    let b = [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy];
                    let (w, h) = (((b[2] - b[0]) * scale).ceil() as usize, ((b[3] - b[1]) * scale).ceil() as usize);
                    art.sprites.push((key.clone(), SpriteRec { image, box_: b, w: w.max(1), h: h.max(1) }));
                }
                keys.push(key);
            }
            let label = format!("{}#{}/{name}", e.library, art.clips.len());
            names.push(label.clone());
            art.clips.push((label, ArtClipRec { record: clip.clone(), keys }));
        }
    }
    // Per group: how many sprites and pixels its clips bring.
    for (group, (e, clip_names)) in &wanted {
        let mut seen: Vec<&SpriteKey> = vec![];
        for (label, rec) in &art.clips {
            if label.starts_with(&format!("{}#", e.library)) && clip_names.iter().any(|n| label.ends_with(&format!("/{n}"))) {
                for k in &rec.keys {
                    if !seen.contains(&k) {
                        seen.push(k);
                    }
                }
            }
        }
        let px: usize = seen.iter().map(|k| art.sprite(k).w * art.sprite(k).h).sum();
        println!("  group {} scale {:.3}x{:.3}: {} clips, {} sprites, {} px (ents {:?})", e.library, f64::from_bits(group.1), f64::from_bits(group.2), clip_names.len(), seen.len(), px,
            ents.iter().filter(|x| group_of(x) == *group).map(|x| (x.tree, x.ent)).collect::<Vec<_>>());
    }
    let priority: Vec<&str> = names.iter().map(String::as_str).collect();
    let quantize = &quantize::atlas_quantizer;
    let planned = plan(&art, &[], quantize, &priority, 100, 1 << 40, "Hornet")?;
    let pixels: usize = art.sprites.iter().map(|s| s.1.w * s.1.h).sum();
    println!("hornet art: {} sprites, {} clips, {} px; plan {}", art.sprites.len(), art.clips.len(), pixels, crate::pyjson::dumps(&planned.totals));
    Ok(())
}

/// The clips each animated object plays, from the simulated fights (`shared/hk-sim/tests/hornet_clip_trace.rs`, collected by
/// `hk-cook boss-measure --write-used`): one `tree<TAB>object<TAB>clip` row each. The tree cooker names every clip any object of a
/// library could play on every object of it, which is five times what the fight shows; objects the fights never reach are added
/// by `effect_like` below, and a clip the bank lacks draws nothing and is counted by the guest (`HK_BOSS_CLIP_MISSES`).
const PLAYED: &str = include_str!("hornet_clips.tsv");

fn effect_like(name: &str) -> bool {
    ["Effect", "Thread", "Flash", "Needle", "Ball"].iter().any(|k| name.contains(k))
}

pub(crate) fn played(ents: &[Ent]) -> crate::boss_art::Used {
    let mut used = crate::boss_art::Used::new();
    for line in PLAYED.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        if let (Ok(tree), Ok(ent)) = (f[0].parse::<u8>(), f[1].parse::<u8>()) {
            used.entry((tree, ent)).or_default().insert(f[2].to_owned());
        }
    }
    // Her own body plays every clip her trees name.
    for e in ents.iter().filter(|e| e.tree == 0 && e.ent == 0) {
        used.entry((0, 0)).or_default().extend(e.clips.iter().map(|c| c.0.clone()));
    }
    // An object the simulated fights never played, on her own library: its effect clips.
    for e in ents {
        if e.library.starts_with("sharedassets105.assets") && !used.contains_key(&(e.tree, e.ent)) {
            used.entry((e.tree, e.ent)).or_default().extend(e.clips.iter().map(|c| c.0.clone()).filter(|n| effect_like(n)));
        }
    }
    used.retain(|_, v| !v.is_empty());
    used
}

/// Shared palettes of the Hornet bank (docs/BOSS-ART.md: 16 add 4.3 RMS per channel over the room quantizer on 218 sprites).
const PALETTES: usize = 16;
const BANK_ID: u16 = 1;

/// What the Hornet bank would be (`hk-cook hornet-bank`): the stats `cook` prints, with no regions touched.
pub fn bank_report(root: &Path, sheet: Option<&Path>) -> Result<()> {
    let source = Source::from_doctor(root)?;
    let mut ents = manifest(root, "hornet3", 0)?;
    ents.extend(manifest(root, "hornet_corpse3", 1)?);
    let used = played(&ents);
    for ((tree, ent), names) in &used {
        println!("  tree {tree} object {ent}: {} clips", names.len());
    }
    let lib = crate::boss_art::load(&source, &ents, Some(&used))?;
    let built = crate::boss_bank::build(&lib, BANK_ID, PALETTES)?;
    println!("hornet bank: {:#?}", built.stats);
    if let Some(dir) = sheet {
        // Original art next to what the guest will decode from the chunks, one row per clip of her body (middle frame).
        use hk_pil::{Image, Mode};
        let blob = crate::boss_bank::blob_of(&built);
        let bg = [70.0f64, 72.0, 84.0];
        let rows: Vec<usize> = lib.clips.iter().filter(|c| !c.frames.is_empty()).map(|c| c.frames[c.frames.len() / 2]).collect::<BTreeSet<_>>().into_iter().filter(|&i| lib.sprites[i].w <= 220).take(24).collect();
        let cell = (rows.iter().map(|&i| lib.sprites[i].w).max().unwrap_or(64) + 4, rows.iter().map(|&i| lib.sprites[i].h).max().unwrap_or(64) + 4);
        let mut out = Image::new(Mode::Rgba, cell.0 * 2, cell.1 * rows.len());
        for (r, &i) in rows.iter().enumerate() {
            let s = &lib.sprites[i];
            let (plane, pal, w, h) = crate::boss_bank::decode_sprite(&blob, &built, i).ok_or("bank does not open")?;
            let mut left = Image::new(Mode::Rgba, w, h);
            let mut right = Image::new(Mode::Rgba, w, h);
            for k in 0..w * h {
                let a = s.image.data[k * 4 + 3] as f64 / 255.0;
                let src = [0, 1, 2].map(|c| s.image.data[k * 4 + c] as f64 * a + bg[c] * (1.0 - a));
                let dec = if plane[k] == 0 {
                    bg
                } else {
                    let d = crate::boss_art::display(pal[plane[k] as usize] & 0x7fff);
                    if pal[plane[k] as usize] & 0x8000 != 0 { [0, 1, 2].map(|c| (bg[c] + d[c]).min(255.0)) } else { d }
                };
                for c in 0..3 {
                    left.data[k * 4 + c] = src[c] as u8;
                    right.data[k * 4 + c] = dec[c] as u8;
                }
                left.data[k * 4 + 3] = 255;
                right.data[k * 4 + 3] = 255;
            }
            out.paste(&left, 0, (r * cell.1) as i64, None);
            out.paste(&right, cell.0 as i64, (r * cell.1) as i64, None);
        }
        std::fs::create_dir_all(dir)?;
        crate::png::save_rgba(&dir.join("hornet-bank-sheet.png"), &out)?;
        println!("sheet: {}/hornet-bank-sheet.png (left: original art at its on-screen size, right: decoded from the bank chunks)", dir.display());
    }
    Ok(())
}

/// The pass: after the legacy region cook, before anything reads the rooms. Her art is a boss bank (`boss_bank.rs`) carried by
/// every region of her scene as streamed chunk textures. `HK_HORNET_ART=off` leaves her out of the build, `=empty` cooks the
/// guest tables with no art (a smoke build for the module).
pub fn cook(root: &Path) -> Result<()> {
    let mode = std::env::var("HK_HORNET_ART").unwrap_or_default();
    if mode == "off" {
        println!("  hornet art: skipped (HK_HORNET_ART=off)");
        let _ = std::fs::remove_file(root.join("data/hornet_art.rs"));
        return Ok(());
    }
    // The scene is not admitted in every build: without it the boss has no room to appear in.
    let Some(row) = quality::SCENE_TABLE.iter().find(|r| r.scene_name == SCENE) else {
        println!("  hornet art: {SCENE} is not admitted, skipped");
        let _ = std::fs::remove_file(root.join("data/hornet_art.rs"));
        return Ok(());
    };
    let scene_id = row.scene_id as u64;
    if mode == "empty" {
        let table = format!(
            "// Generated by host/hk-cook hornet_scene.rs (HK_HORNET_ART=empty: no art); do not edit.\npub const HORNET_ART_SCENE:usize={scene_id};\npub const HORNET_BANK_ID:u16={BANK_ID};\npub static HORNET_ART_CLIPS: [(u8, u8, u32, u16); 0] = [];\npub static HORNET_ART_ENTS: [(u8, u8, u8, i32, i32); 0] = [];\n"
        );
        publish(&root.join("data/hornet_art.rs"), table.as_bytes())?;
        println!("  hornet art: none appended (HK_HORNET_ART=empty)");
        return Ok(());
    }
    let source = Source::from_doctor(root)?;
    let mut ents = manifest(root, "hornet3", 0)?;
    ents.extend(manifest(root, "hornet_corpse3", 1)?);
    let path = root.join("data/regions.json");
    let mut report: J = serde_json::from_slice(&std::fs::read(&path)?)?;
    if report.get(MARKER).is_some() {
        return Err("the Hornet postpass needs the unchanged legacy cook output, not a second append".into());
    }
    let lib = crate::boss_art::load(&source, &ents, Some(&played(&ents)))?;
    let built = crate::boss_bank::build(&lib, BANK_ID, PALETTES)?;
    let atlas = crate::boss_bank::atlas_of(&built)?;
    println!(
        "  hornet art: {:?}",
        built.stats
    );
    let bank = ArtBank { atlas, frames: vec![], clips: vec![] };
    let regions = report["regions"].as_array_mut().ok_or("regions absent")?;
    let indices: Vec<usize> =
        regions.iter().enumerate().filter_map(|(i, r)| (r["scene_id"].as_u64() == Some(scene_id)).then_some(i)).collect();
    if indices.is_empty() {
        return Err("Fungus1_04 is not admitted".into());
    }
    let mut outputs = vec![];
    for &i in &indices {
        let r = &mut regions[i];
        let room_path = root.join(r["path"].as_str().ok_or("room path")?);
        let raw = std::fs::read(&room_path)?;
        let clip_base = region_delta::layout(&raw)?.counts[4] as usize;
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
    // The guest's table: the bank clip of every (tree, group, clip name id) and, per animated object, its group (objects of one
    // library at one scale share their clips) and the world scale it was cooked at (Q16.16), which the bank's boxes carry.
    let mut groups: Vec<Group> = vec![];
    for e in &ents {
        let g = group_of(e);
        if !groups.contains(&g) {
            groups.push(g);
        }
    }
    let mut rows: Vec<(u8, usize, u32, usize)> = vec![];
    for e in &ents {
        let group = group_of(e);
        let at = groups.iter().position(|g| *g == group).unwrap();
        for (name, id) in &e.clips {
            if let Some(&clip) = built.clip_of.get(&(group.clone(), name.clone())) {
                rows.push((e.tree, at, *id, clip));
            }
        }
    }
    rows.sort();
    rows.dedup();
    let mut table = String::new();
    table.push_str("// Generated by host/hk-cook hornet_scene.rs; do not edit.\n");
    table.push_str(&format!("pub const HORNET_ART_SCENE:usize={scene_id};\npub const HORNET_BANK_ID:u16={BANK_ID};\n"));
    table.push_str("/// Per tree (0 the fight, 1 the corpse), group and clip name id: the bank clip that plays it.\n");
    table.push_str(&format!("pub static HORNET_ART_CLIPS: [(u8, u8, u32, u16); {}] = [\n", rows.len()));
    for (tree, group, id, clip) in &rows {
        table.push_str(&format!("    ({tree}, {group}, {id}, {clip}),\n"));
    }
    table.push_str("];\n");
    let q = |f: f64| py_round(f * 65536.0);
    table.push_str("/// Per animated object: its group and the world scale it was cooked at (Q16.16).\n");
    table.push_str(&format!("pub static HORNET_ART_ENTS: [(u8, u8, u8, i32, i32); {}] = [\n", ents.len()));
    for e in &ents {
        let group = group_of(e);
        let at = groups.iter().position(|g| *g == group).unwrap();
        table.push_str(&format!(
            "    ({}, {}, {at}, {}, {}),\n",
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
