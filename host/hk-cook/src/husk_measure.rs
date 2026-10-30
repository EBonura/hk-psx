//! `hk-cook husk-measure`: what the Crossroads_10_boss husks' art costs as a compact bank (docs/BOSS-ART.md), so the
//! numbers are the cooker's own and not an estimate. Reads `.hkpsx/husk/husk_ents.json` (written by the husk script from the
//! controllers' clip lists: one object per husk, its sprite library and scale) and the Steam sprite libraries, cooks every
//! clip the controllers admit (or, with `--core`, without the Death and Fall clips) and builds the bank on 4, 8 and 16
//! shared palettes.
use crate::boss_art::{self as ba, Used};
use crate::boss_bank;
use crate::hornet_scene::Ent;
use hk_unity::Source;
use serde_json::Value as J;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let core = args.iter().any(|a| a == "--core");
    let source = Source::from_doctor(root)?;
    let v: J = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/husk/husk_ents.json"))?)?;
    let mut ents = vec![];
    let mut used = Used::new();
    for e in v["ents"].as_array().ok_or("ents")? {
        let pair = |k: &str| -> Result<[f64; 2]> {
            let a = e[k].as_array().ok_or("scale")?;
            Ok([a[0].as_f64().ok_or("scale")?, a[1].as_f64().ok_or("scale")?])
        };
        let ent = e["ent"].as_u64().ok_or("ent")? as u8;
        let clips: Vec<(String, u32)> = e["clips"]
            .as_array()
            .ok_or("clips")?
            .iter()
            .map(|c| Ok((c["name"].as_str().ok_or("clip name")?.to_owned(), c["id"].as_u64().ok_or("clip id")? as u32)))
            .collect::<Result<_>>()?;
        let keep: Vec<String> = clips
            .iter()
            .map(|c| c.0.clone())
            .filter(|n| !core || !(n.starts_with("Death") || n == "Fall"))
            .collect();
        used.entry((0, ent)).or_default().extend(keep);
        ents.push(Ent {
            tree: 0,
            ent,
            library: e["library"].as_str().ok_or("library")?.to_owned(),
            sprite_scale: pair("sprite_scale")?,
            world_scale: pair("world_scale")?,
            alpha: e["alpha"].as_f64().unwrap_or(1.0),
            clips,
        });
    }
    let lib = ba::load(&source, &ents, Some(&used))?;
    println!("{} husks, {} clips, {} sprites{}", ents.len(), lib.clips.len(), lib.sprites.len(), if core { " (core: no Death/Fall)" } else { "" });
    let padded: usize = lib.sprites.iter().map(|s| ((s.w + 3) & !3) / 2 * s.h).sum();
    println!("padded stream bytes the room path would add (frame_bytes): {padded}");
    for palettes in [4usize, 8, 16] {
        let b = boss_bank::build(&lib, 1, palettes)?;
        let s = &b.stats;
        println!(
            "palettes {palettes:2}: parts {} raw parts {} texel bytes {} coded {} tables {} -> blob {} bytes, {} chunks (2,048 B each = {} stream bytes), {} palettes, added RMS {:.2}",
            s.parts, s.raw_parts, s.texel_bytes, s.coded_bytes, s.table_bytes, s.coded_bytes + s.table_bytes, s.chunks, s.chunks * 2048, s.palettes, s.added_rms
        );
    }
    Ok(())
}
