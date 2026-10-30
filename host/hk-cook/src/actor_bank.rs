//! `hk-cook actor-bank --ents FILE --out DIR [--id N] [--palettes N]`: an actor family's art as a compact bank
//! (docs/BOSS-ART.md, game/src/actor_bank.rs).
//!
//! The ents file lists the animated objects whose art goes in the bank, each with its sprite library, its scale and the clips to
//! cook (`host/actor_bank.py` writes it from the controllers' clip bindings). The cooker cooks those clips into sprites at the
//! size they have on screen, merges them onto `--palettes` shared palettes (default 8) and writes
//!
//!   DIR/bank.bin   the bank's bytes: header, tables and coded texels, one contiguous string (the guest's `Bank::open(&[u8])`);
//!   DIR/bank.json  what the host needs to bind the actors to it: the shared palettes, every sprite's world box, every bank clip
//!                  (rate, wrap, loop start and the sprite of each frame) and, per ent, the bank clip of each clip it named.
use crate::boss_art::{self as ba};
use crate::boss_bank;
use crate::hornet_scene::{group_of, Ent};
use hk_unity::Source;
use serde_json::{json, Value as J};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let arg = |name: &str| args.iter().position(|a| a == name).map(|i| args[i + 1].clone());
    let ents_path = arg("--ents").ok_or("--ents FILE")?;
    let out = arg("--out").ok_or("--out DIR")?;
    let id: u16 = arg("--id").map(|v| v.parse()).transpose()?.unwrap_or(1);
    let palettes: usize = arg("--palettes").map(|v| v.parse()).transpose()?.unwrap_or(8);
    let source = Source::from_doctor(root)?;
    let v: J = serde_json::from_slice(&std::fs::read(&ents_path)?)?;
    let mut ents = vec![];
    for (i, e) in v["ents"].as_array().ok_or("ents")?.iter().enumerate() {
        let pair = |k: &str| -> Result<[f64; 2]> {
            let a = e[k].as_array().ok_or("scale")?;
            Ok([a[0].as_f64().ok_or("scale")?, a[1].as_f64().ok_or("scale")?])
        };
        ents.push(Ent {
            tree: 0,
            ent: i as u8,
            library: e["library"].as_str().ok_or("library")?.to_owned(),
            sprite_scale: pair("sprite_scale")?,
            world_scale: pair("world_scale")?,
            alpha: e["alpha"].as_f64().unwrap_or(1.0),
            clips: e["clips"]
                .as_array()
                .ok_or("clips")?
                .iter()
                .enumerate()
                .map(|(n, c)| Ok((c.as_str().or_else(|| c["name"].as_str()).ok_or("clip name")?.to_owned(), n as u32)))
                .collect::<Result<_>>()?,
        });
    }
    let lib = ba::load(&source, &ents, None)?;
    let built = boss_bank::build(&lib, id, palettes)?;
    let s = &built.stats;
    let exact = s.table_bytes + s.coded_bytes;
    let mut blob = boss_bank::blob_of(&built);
    blob.truncate(exact);
    std::fs::create_dir_all(&out)?;
    std::fs::write(Path::new(&out).join("bank.bin"), &blob)?;
    let sprites: Vec<J> = lib.sprites.iter().map(|s| json!({"w": s.w, "h": s.h, "box": s.box_})).collect();
    let clips: Vec<J> = lib
        .clips
        .iter()
        .map(|c| json!({"name": c.name, "library": c.library, "fps": c.fps, "wrap": c.wrap, "loop_start": c.loop_start, "frames": c.frames}))
        .collect();
    let mut ent_clips = vec![];
    for e in &ents {
        let mut m = serde_json::Map::new();
        for (name, _) in &e.clips {
            let at = built.clip_of.get(&(group_of(e), name.clone())).ok_or_else(|| format!("no bank clip for {name}"))?;
            m.insert(name.clone(), json!(at));
        }
        ent_clips.push(J::Object(m));
    }
    let doc = json!({
        "id": id,
        "blob_bytes": blob.len(),
        "palettes": built.palettes.iter().map(|p| p.to_vec()).collect::<Vec<_>>(),
        "sprites": sprites,
        "clips": clips,
        "ent_clips": ent_clips,
        "stats": {"sprites": s.sprites, "parts": s.parts, "clips": s.clips, "texel_bytes": s.texel_bytes, "coded_bytes": s.coded_bytes, "table_bytes": s.table_bytes, "added_rms": s.added_rms},
    });
    std::fs::write(Path::new(&out).join("bank.json"), serde_json::to_vec_pretty(&doc)?)?;
    println!(
        "actor bank {id}: {} sprites, {} parts, {} clips, {} palettes, {} bytes (tables {}, coded {}), added RMS {:.2}",
        s.sprites, s.parts, s.clips, built.palettes.len(), blob.len(), s.table_bytes, s.coded_bytes, s.added_rms
    );
    Ok(())
}
