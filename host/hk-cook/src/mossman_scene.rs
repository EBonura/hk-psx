//! Add the selected Mossman family to complete existing scene banks.
//! Existing scenery, actors, pickups and their record indices stay intact.
use crate::{
    actor_art::{ArtActor, ArtBank},
    actor_specs::{generated_actor_records, SpecActor},
    actors,
    atlas::{Atlas, MAX_ROOM_TEXTURES},
    common::py_round,
    effects_art,
    pyjson::dumps,
    quality, quantize, region_delta,
};
use hk_unity::{scene::Scene, Source};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const TARGETS: [(&str, u64, &[i64]); 4] = [
    ("level128", 46, &[736]),
    ("level134", 49, &[110]),
    ("level135", 50, &[808]),
    ("level136", 51, &[510, 1082]),
];
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn emit(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}

/// Append complete art without moving any existing palette, texel or record index.
fn append(raw: &[u8], bank: &ArtBank, clip_base: usize) -> Result<Vec<u8>> {
    let layout = region_delta::layout(raw)?;
    let [np, nt, nd, nf, nc, ne] = layout.counts.map(|x| x as usize);
    let ns = word(raw, 32) as usize;
    let flags = word(raw, 36);
    let pad = clip_base
        .checked_sub(nc)
        .ok_or("clip base before room clips")?;
    if !bank.atlas.pages.is_empty()
        || nt + bank.atlas.entries.len() > MAX_ROOM_TEXTURES
        || nf + bank.frames.len() > 2048
        || clip_base + bank.clips.len() > 128
        || (pad > 0 && nf == 0)
        || bank.atlas.palettes.len() != bank.atlas.entries.len()
    {
        return Err("Mossman bank exceeds unchanged room format budgets".into());
    }
    let mut blocks = Vec::new();
    let mut at = 40;
    for len in [
        nt * 16,
        nd * 44,
        nf * 20,
        nc * 16,
        ne * 16,
        nt * 32,
        np * 32768,
        ns,
    ] {
        blocks.push(&raw[at..at + len]);
        at += len;
    }
    let mut out = b"HKROOM02".to_vec();
    for n in [
        np,
        nt + bank.atlas.entries.len(),
        nd,
        nf + bank.frames.len(),
        clip_base + bank.clips.len(),
        ne,
        ns + bank.atlas.stream.len(),
        flags as usize,
    ] {
        emit(&mut out, u32::try_from(n)?);
    }
    out.extend(blocks[0]);
    for entry in &bank.atlas.entries {
        let [page, u, v, w, h, palette, offset] = entry.ok_or("Mossman texture was not packed")?;
        if page != 65535 {
            return Err("Mossman requires streamed art".into());
        }
        for n in [page, u, v, w, h, palette + nt as i64] {
            out.extend(u16::try_from(n)?.to_le_bytes());
        }
        emit(&mut out, u32::try_from(offset + ns as i64)?);
    }
    out.extend(blocks[1]);
    out.extend(blocks[2]);
    for f in &bank.frames {
        emit(&mut out, u32::try_from(f.texture + nt)?);
        for x in f.box_ {
            out.extend(i32::try_from(py_round(x * 65536.0))?.to_le_bytes());
        }
    }
    out.extend(blocks[3]);
    for _ in 0..pad {
        for n in [0, 1, 65536, 0] {
            emit(&mut out, n);
        }
    }
    for c in &bank.clips {
        for n in [
            u32::try_from(c.start + nf)?,
            u32::try_from(c.count)?,
            u32::try_from(py_round(c.fps * 65536.0))?,
            u32::try_from(c.wrap | (c.loop_start << 16))?,
        ] {
            emit(&mut out, n);
        }
    }
    out.extend(blocks[4]);
    out.extend(blocks[5]);
    for p in &bank.atlas.palettes {
        if p.len() != 32 {
            return Err("palette size".into());
        }
        out.extend(p);
    }
    out.extend(blocks[6]);
    out.extend(blocks[7]);
    out.extend(&bank.atlas.stream);
    let result = region_delta::layout(&out)?;
    if out.len() - np * 32768 > quality::ROOM_BYTE_BUDGET {
        return Err("Mossman exceeds unchanged room staging budget".into());
    }
    if result.counts[1] as usize > quality::TEXTURE_BUDGET {
        return Err("Mossman exceeds unchanged per-view texture budget".into());
    }
    Ok(out)
}
fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let next = path.with_extension("mossman-next");
    std::fs::write(&next, bytes)?;
    std::fs::rename(next, path)?;
    Ok(())
}

pub fn cook(root: &Path) -> Result<()> {
    let source = Source::from_doctor(root)?;
    let path = root.join("data/regions.json");
    let mut report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    if report.get("mossman_bank_adopted").is_some() {
        return Err(
            "Mossman postpass needs the unchanged legacy cook output, not a second append".into(),
        );
    }
    let regions = report["regions"].as_array_mut().ok_or("regions absent")?;
    let mut outputs = Vec::new();
    let mut spec_sets = Vec::new();
    for (file, scene_id, selected) in TARGETS {
        let scene = Scene::new(&source, file)?;
        let all = actors::scan(&scene, &source, &[])?;
        let actors: Vec<_> = all
            .into_iter()
            .filter(|a| selected.contains(&a.game_object))
            .collect();
        if actors.len() != selected.len() || actors.iter().any(|a| !a.supported) {
            return Err("selected Mossman source refused".into());
        }
        let mut art: Vec<_> = actors.iter().map(ArtActor::new).collect();
        let mut bank = ArtBank::new(Atlas::standard());
        crate::actor_art::append_actor_art_bodies(
            &scene,
            &source,
            &mut art,
            &mut bank,
            &quantize::atlas_quantizer,
        )?;
        effects_art::append_corpse_art(
            &source,
            &scene,
            &mut art,
            &mut bank,
            &quantize::atlas_quantizer,
        )?;
        if art.iter().any(|a| !a.supported || a.corpse.is_none()) {
            return Err("Mossman art refused".into());
        }
        bank.atlas.pack()?;
        let indices: Vec<_> = regions
            .iter()
            .enumerate()
            .filter_map(|(i, r)| (r["scene_id"].as_u64() == Some(scene_id)).then_some(i))
            .collect();
        if indices.is_empty() {
            return Err("Mossman scene is not admitted".into());
        }
        let mut clip_base = 0;
        for &i in &indices {
            let raw = std::fs::read(root.join(regions[i]["path"].as_str().ok_or("room path")?))?;
            clip_base = clip_base.max(region_delta::layout(&raw)?.counts[4] as usize);
        }
        let mut templates = Vec::new();
        for actor in &actors {
            let template = indices
                .iter()
                .flat_map(|&i| regions[i]["actors"].as_array().into_iter().flatten())
                .find(|a| a["source"].as_str() == Some(actor.source.as_str()))
                .ok_or("original Mossman metadata absent from current admitted scene")?
                .clone();
            if template["spec_source_id"].as_i64() != Some(actor.spec_source_id) {
                return Err("Mossman canonical source key drift".into());
            }
            templates.push(template);
        }
        for a in &mut art {
            for (_, c) in &mut a.clips {
                *c += clip_base as i64;
            }
            if let Some(crate::pyjson::Json::Obj(corpse)) = &mut a.corpse {
                for (k, v) in corpse {
                    if k == "air_clip" || k == "land_clip" {
                        if let crate::pyjson::Json::Int(n) = v {
                            *n += clip_base as i64;
                        } else {
                            return Err("corpse clip type".into());
                        }
                    }
                }
            }
        }
        let specs: Vec<_> = art
            .iter()
            .map(|a| SpecActor {
                row: a.row,
                clips: &a.clips,
                corpse: a.corpse.as_ref(),
            })
            .collect();
        let generated = generated_actor_records(&specs)?;
        let mut unique = Vec::new();
        for (spec, _) in &generated {
            if !unique.contains(spec) {
                unique.push(spec.clone());
            }
        }
        spec_sets.push(json!({"scene_id":scene_id,"specs":unique}));
        for &i in &indices {
            let r = &mut regions[i];
            let room_path = root.join(r["path"].as_str().ok_or("room path")?);
            let raw = std::fs::read(&room_path)?;
            let pack = append(&raw, &bank, clip_base)?;
            let layout = region_delta::layout(&pack)?;
            let list = r["actors"].as_array_mut().ok_or("actor list")?;
            list.retain(|a| {
                !actors
                    .iter()
                    .any(|row| a["source"].as_str() == Some(row.source.as_str()))
            });
            for (a, template) in art.iter().zip(&templates) {
                let mut value = template.clone();
                value["movement_supported"] = json!(true);
                value["movement_control"] =
                    serde_json::from_str(&dumps(a.control().ok_or("controller absent")?))?;
                value["pending_movement_error"] = Value::Null;
                for (k, c) in &a.clips {
                    value[k] = json!(c);
                }
                value["corpse"] =
                    serde_json::from_str(&dumps(a.corpse.as_ref().ok_or("corpse absent")?))?;
                list.push(value);
            }
            if list
                .iter()
                .filter(|a| a["movement_supported"] == json!(true))
                .count()
                > 32
            {
                return Err("Mossman exceeds unchanged 32-actor scene pool".into());
            }
            r["bytes"] = json!(pack.len());
            r["sha256"] = json!(format!("{:x}", Sha256::digest(&pack)));
            r["textures"] = json!(layout.counts[1]);
            r["pages"] = json!(layout.counts[0]);
            r["stream_bytes"] = json!(word(&pack, 32));
            let actor_bank_bytes = r["actor_bank_bytes"]
                .as_u64()
                .ok_or("old actor bank bytes")?
                + u64::try_from(pack.len() - raw.len())?;
            if actor_bank_bytes > 256 * 1024 {
                return Err("Mossman exceeds unchanged 256KiB actor-bank admission".into());
            }
            r["actor_bank_bytes"] = json!(actor_bank_bytes);
            r["mossman_bank_growth_bytes"] = json!(pack.len() - raw.len());
            outputs.push((room_path, pack));
        }
    }
    let total = regions
        .iter()
        .map(|r| r["bytes"].as_u64().unwrap())
        .sum::<u64>();
    report["total_pack_bytes"] = json!(total);
    report["mossman_bank_adopted"] = json!(true);
    report["mossman_actor_specs"] = json!(spec_sets);
    // All four scenes must fit before any output replaces the old cook.
    for (path, bytes) in outputs {
        publish(&path, &bytes)?;
    }
    publish(&path, &serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

/// Existing metadata generation retains indices; bind its selected type to SwipeCalm.
/// The complete expected Rust spec must match, allowing only that attack variant.
pub fn bind_generated_specs(root: &Path) -> Result<()> {
    let report: Value = serde_json::from_slice(&std::fs::read(root.join("data/regions.json"))?)?;
    let specs = report["mossman_actor_specs"]
        .as_array()
        .ok_or("Mossman specs absent")?;
    let path = root.join("data/regions.rs");
    let mut text = std::fs::read_to_string(&path)?;
    for scene in specs {
        let id = scene["scene_id"].as_u64().ok_or("scene id")?;
        let marker = format!("static SCENE_ACTORS_{id}: &[hk_sim::ActorSpec] = &[");
        let start = text
            .find(&marker)
            .ok_or("generated scene spec table absent")?
            + marker.len();
        let end = start
            + text[start..]
                .find("];\n")
                .ok_or("generated scene spec table end")?;
        let mut section = text[start..end].to_string();
        for expected in scene["specs"].as_array().ok_or("scene specs")? {
            let expected = expected.as_str().ok_or("spec text")?;
            let legacy = expected.replace("Attack::SwipeCalm", "Attack::Swipe");
            if section.matches(expected).count() == 1 {
                continue;
            }
            if section.matches(&legacy).count() != 1 {
                return Err(format!("scene{id}: exact generated Mossman spec differs from source-derived Rust contract").into());
            }
            section = section.replacen(&legacy, expected, 1);
        }
        text.replace_range(start..end, &section);
    }
    publish(&path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Vec<u8>, ArtBank) {
        let mut raw = b"HKROOM02".to_vec();
        for n in [0, 1, 1, 1, 1, 1, 16, 1] {
            emit(&mut raw, n);
        }
        raw.extend([7; 16]);
        raw.extend([11; 44]);
        raw.extend([13; 20]);
        for n in [0, 1, 65536, 0] {
            emit(&mut raw, n);
        }
        raw.extend([17; 16]);
        raw.extend([19; 32]);
        raw.extend([23; 16]);
        let mut bank = ArtBank::new(Atlas::standard());
        bank.atlas.entries.push(Some([65535, 0, 0, 2, 2, 0, 0]));
        bank.atlas.palettes.push(vec![29; 32]);
        bank.atlas.stream = vec![31; 16];
        bank.frames.push(crate::actor_art::Frame {
            texture: 0,
            box_: [0.0, 0.0, 1.0, 1.0],
            sprite: "fixture".into(),
            box_q16: None,
            event: hk_unity::Value::Map(vec![]),
        });
        bank.clips.push(crate::actor_art::Clip {
            name: "fixture".into(),
            start: 0,
            count: 1,
            fps: 12.0,
            wrap: 2,
            loop_start: 0,
        });
        (raw, bank)
    }
    #[test]
    fn appended_art_preserves_old_tables_palette_and_stream_bytes() {
        let (raw, bank) = fixture();
        let out = append(&raw, &bank, 1).unwrap();
        assert_eq!(&out[40..56], &raw[40..56]);
        assert_eq!(&out[72..116], &raw[56..100]);
        assert_eq!(&out[116..136], &raw[100..120]);
        assert_eq!(&out[156..172], &raw[120..136]);
        assert_eq!(&out[188..204], &raw[136..152]);
        assert_eq!(&out[204..236], &raw[152..184]);
        let layout = region_delta::layout(&out).unwrap();
        assert_eq!(&out[layout.stream..layout.stream + 16], &raw[184..]);
        assert_eq!(layout.counts, [0, 2, 1, 2, 2, 1]);
        assert_eq!(word(&out, 172), 1, "new clip starts after the old frame");
    }
    #[test]
    fn incompatible_clip_base_and_budget_growth_fail_without_touching_input() {
        let (raw, mut bank) = fixture();
        let before = raw.clone();
        assert!(append(&raw, &bank, 0).is_err());
        bank.atlas.stream.resize(quality::ROOM_BYTE_BUDGET, 0);
        assert!(append(&raw, &bank, 1).is_err());
        assert_eq!(raw, before);
    }
}
