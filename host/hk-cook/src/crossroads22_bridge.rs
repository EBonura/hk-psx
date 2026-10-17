//! Explicit, uninstalled full-room bridge for Crossroads_22.
//! The ordinary cooker and runtime continue to use their existing 32-slot policy.
use crate::{
    actor_art::{ArtActor, ArtBank},
    actor_specs::{self, Placement, SpecActor},
    actors,
    atlas::Atlas,
    battle,
    common::{component_records, Result},
    effects_art, quality, quantize, region_delta,
};
use hk_unity::{scene::Scene, Source};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, path::Path};
const SCENE: u32 = 21;
const COUNT: usize = 40;
const PEAK: usize = 28;

fn validate(members: &[(u8, bool)]) -> Result<()> {
    let mut waves = [0; 5];
    let mut removed = 0;
    for &(wave, remove) in members {
        if wave > 4 || (wave != 0 && remove) {
            return Err("invalid Crossroads_22 member lifetime".into());
        }
        waves[wave as usize] += 1;
        removed += usize::from(remove);
    }
    if members.len() != COUNT
        || waves != [28, 2, 3, 3, 4]
        || removed != 5
        || battle::pool_peak(members) != PEAK
    {
        return Err(
            "Crossroads_22 must retain all 40 placements and the authored 28-slot peak".into(),
        );
    }
    Ok(())
}

fn metadata(records: &[(usize, Placement, (u8, bool), u16)], types: usize) -> Result<Vec<u8>> {
    validate(&records.iter().map(|r| r.2).collect::<Vec<_>>())?;
    if types == 0 || types > 32 {
        return Err("Crossroads_22 spec catalogue exceeds existing index range".into());
    }
    let mut ids = HashSet::new();
    let mut out = b"HKARNA01".to_vec();
    for word in [SCENE, COUNT as u32, PEAK as u32, 24] {
        out.extend(word.to_le_bytes());
    }
    for (spec, p, (wave, remove), cap) in records {
        let sid = u32::try_from(p.source_id).map_err(|_| "actor source id exceeds u32")?;
        if !ids.insert(sid) || *spec >= types || !(0..4).contains(&p.rotation_quarter) {
            return Err("duplicate source id or invalid actor spec/orientation".into());
        }
        let flags = 32u16
            | 64
            | u16::from(p.initial_direction > 0)
            | (u16::from(p.random_start_direction) << 1)
            | (u16::from(p.start_alert) << 2)
            | (u16::from(p.start_right) << 3)
            | ((p.rotation_quarter as u16) << 8);
        out.extend(sid.to_le_bytes());
        out.extend((*spec as u16).to_le_bytes());
        out.extend(flags.to_le_bytes());
        out.extend(
            i32::try_from(p.x)
                .map_err(|_| "actor X exceeds Q16 range")?
                .to_le_bytes(),
        );
        out.extend(
            i32::try_from(p.y)
                .map_err(|_| "actor Y exceeds Q16 range")?
                .to_le_bytes(),
        );
        out.extend([*wave, u8::from(*remove)]);
        out.extend(cap.to_le_bytes());
        out.extend(0u32.to_le_bytes());
    }
    Ok(out)
}

pub fn main(
    root: &Path,
    source_dir: Option<&Path>,
    args: &[String],
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let source = match source_dir {
        Some(p) => Source::new(p).map_err(|e| e.to_string())?,
        None => Source::from_doctor(root).map_err(|e| e.to_string())?,
    };
    let scene = Scene::new(&source, "level59").map_err(|e| e.to_string())?;
    let catalogue: Vec<_> = quality::SCENE_TABLE
        .iter()
        .map(|s| {
            (
                s.file.to_string(),
                s.runtime_bounds.map(|n| n.f64()),
                s.scene_name.to_string(),
            )
        })
        .collect();
    let rows: Vec<_> = actors::scan_crossroads22(&scene, &source, &catalogue)?
        .into_iter()
        .filter(|r| r.supported)
        .collect();
    let members = rows
        .iter()
        .map(|r| {
            battle::membership(
                &scene,
                r.game_object,
                &component_records(&scene, r.game_object),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    validate(&members)?;
    let mut art: Vec<_> = rows.iter().map(ArtActor::new).collect();
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
    if art.iter().any(|a| !a.supported) {
        return Err("complete Crossroads_22 art refused".into());
    }
    bank.atlas.pack()?;
    let report_bytes = std::fs::read(root.join("data/regions.json"))?;
    let report: Value = serde_json::from_slice(&report_bytes)?;
    let regions: Vec<_> = report["regions"]
        .as_array()
        .ok_or("regions absent")?
        .iter()
        .filter(|r| r["scene_id"] == SCENE)
        .collect();
    if regions.is_empty()
        || regions
            .iter()
            .any(|r| r["scene_file"] != "level59" || r["scene_name"] != "Crossroads_22")
    {
        return Err("Crossroads_22 scene indices/name differ".into());
    }
    let mut clip_base = 0;
    for r in &regions {
        let raw = std::fs::read(root.join(r["path"].as_str().ok_or("room path")?))?;
        if format!("{:x}", Sha256::digest(&raw)) != r["sha256"].as_str().ok_or("room SHA absent")? {
            return Err("stale complete source room".into());
        }
        clip_base = clip_base.max(region_delta::layout(&raw)?.counts[4] as usize);
    }
    for a in &mut art {
        for (_, c) in &mut a.clips {
            *c += clip_base as i64;
        }
        if let Some(crate::pyjson::Json::Obj(corpse)) = &mut a.corpse {
            for (key, value) in corpse {
                if key == "air_clip" || key == "land_clip" {
                    if let crate::pyjson::Json::Int(n) = value {
                        *n += clip_base as i64;
                    } else {
                        return Err("corpse clip type".into());
                    }
                }
            }
        }
    }
    let input: Vec<_> = art
        .iter()
        .map(|a| SpecActor {
            row: a.row,
            clips: &a.clips,
            corpse: a.corpse.as_ref(),
        })
        .collect();
    let generated = actor_specs::generated_actor_records_with_bounds(&input, COUNT)?;
    if generated.len() != rows.len() || input.len() != COUNT || art.len() != COUNT {
        return Err("source, cooked art and generated placement lengths differ".into());
    }
    let mut specs = Vec::new();
    let mut records = Vec::new();
    let mut placements = Vec::new();
    for (row_index, ((spec, p, bounds), (row, member))) in generated.into_iter().zip(rows.iter().zip(&members)).enumerate() {
        if p.source_id != row.spec_source_id {
            return Err("generated placement does not match its ordered original source id".into());
        }
        let index = match specs.iter().position(|s| *s == spec) {
            Some(i) => i,
            None => {
                specs.push(spec);
                specs.len() - 1
            }
        };
        let control: Value = serde_json::from_str(&crate::pyjson::dumps(
            &row.control.as_ref().ok_or("controller absent")?.1,
        ))?;
        let cap = control
            .get("hatched_max")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let contract = if member.0 != 0 { Some(crate::battle_waves::arrival_position(&scene, row)?) } else { None };
        let arrival = contract.map(|c| c.0);
        let authored_depth = contract.map(|c| c.1);
        records.push((
            index,
            p.clone(),
            *member,
            u16::try_from(cap).map_err(|_| "Hatcher cap exceeds wire range")?,
        ));
        placements.push(json!({"source":row.source,"game_object":row.game_object,"spec_index":index,"placement":serde_json::from_str::<Value>(&crate::pyjson::dumps(&p.to_json()))?,"arrival":arrival,"authored_depth":authored_depth,"bounds":bounds,"wave":member.0,"removed_on_start":member.1,"hatcher_cap":cap,"controller":control["kind"],"clips":art[row_index].clips,"corpse":art[row_index].corpse.as_ref().map(crate::pyjson::dumps)}));
    }
    let meta = metadata(&records, specs.len())?;
    let out = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join(".hkpsx/crossroads22-bridge"));
    if out.exists() {
        return Err("bridge output already exists; select a fresh output directory".into());
    }
    let mut outputs = Vec::new();
    let mut room_report = Vec::new();
    for r in &regions {
        let raw = std::fs::read(root.join(r["path"].as_str().ok_or("room path")?))?;
        let packed =
            crate::mossman_scene::append(&raw, &bank, clip_base).map_err(|e| e.to_string())?;
        let name = format!("region-{}.hk", r["chunk_id"].as_u64().ok_or("chunk id")?);
        let layout = region_delta::layout(&packed)?;
        let growth = packed
            .len()
            .checked_sub(raw.len())
            .ok_or("bank append shrank the source room")?;
        let actor_bytes = r["actor_bank_bytes"]
            .as_u64()
            .ok_or("measured base actor bank absent")?
            .checked_add(growth as u64)
            .ok_or("actor bank byte count overflow")?;
        if actor_bytes > 256 * 1024 {
            return Err("complete 40-actor bank exceeds unchanged 256 KiB bank bound".into());
        }
        room_report.push(json!({"scene_id":SCENE,"chunk_id":r["chunk_id"],"base_path":r["path"],"base_sha256":r["sha256"],"path":name,"sha256":format!("{:x}",Sha256::digest(&packed)),"bytes":packed.len(),"growth_bytes":packed.len()-raw.len(),"actor_bank_bytes":actor_bytes,"textures":layout.counts[1]}));
        outputs.push((name, packed));
    }
    std::fs::create_dir_all(&out)?;
    for (name, bytes) in outputs {
        std::fs::write(out.join(name), bytes)?;
    }
    std::fs::write(out.join("actors.bin"), &meta)?;
    let receipt = json!({"scene_id":SCENE,"scene_name":"Crossroads_22","source_file":"level59","placements":placements,"specs":specs,"peak_actor_slots":PEAK,"actor_count":COUNT,"metadata_sha256":format!("{:x}",Sha256::digest(&meta)),"base_regions_sha256":format!("{:x}",Sha256::digest(&report_bytes)),"rooms":room_report,"runtime_admitted":false,"wave_runtime_pending":true,"render_peak_pending":true});
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "Crossroads_22 bridge: 40 placements, 28 peak, {} complete rooms; runtime admission pending",
        regions.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<(usize, Placement, (u8, bool), u16)> {
        let mut members = vec![(0, false); 23];
        members.extend([(0, true); 5]);
        for (wave, n) in [(1, 2), (2, 3), (3, 3), (4, 4)] {
            members.extend(vec![(wave, false); n]);
        }
        members
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                (
                    i % 4,
                    Placement {
                        source_id: 100 + i as i64,
                        x: i as i64 * 65536,
                        y: 7 * 65536,
                        initial_direction: -1,
                        random_start_direction: false,
                        start_alert: m.0 != 0,
                        start_right: false,
                        rotation_quarter: 0,
                    },
                    m,
                    if m.0 > 0 { 3 } else { 0 },
                )
            })
            .collect()
    }
    #[test]
    fn retains_all_40_members_and_the_28_peak() {
        let f = fixture();
        let b = metadata(&f, 4).unwrap();
        assert_eq!(b.len(), 24 + 40 * 24);
        assert_eq!(u32::from_le_bytes(b[12..16].try_into().unwrap()), 40);
        assert_eq!(u32::from_le_bytes(b[16..20].try_into().unwrap()), 28);
        for (i, r) in f.iter().enumerate() {
            let at = 24 + i * 24;
            assert_eq!(
                u32::from_le_bytes(b[at..at + 4].try_into().unwrap()),
                r.1.source_id as u32
            );
            assert_eq!(b[at + 16], r.2 .0);
            assert_eq!(b[at + 17], u8::from(r.2 .1));
        }
    }
    #[test]
    fn rejects_partial_duplicate_and_changed_wave_shapes() {
        let mut f = fixture();
        assert!(metadata(&f[..39], 4).is_err());
        f[1].1.source_id = f[0].1.source_id;
        assert!(metadata(&f, 4).is_err());
        let mut f = fixture();
        f[0].2 = (4, false);
        assert!(metadata(&f, 4).is_err());
        let mut f = fixture();
        f[28].2 .1 = true;
        assert!(metadata(&f, 4).is_err());
    }
    #[test]
    fn rejects_out_of_range_spec_orientation_and_coordinates() {
        let mut f = fixture();
        f[0].0 = 4;
        assert!(metadata(&f, 4).is_err());
        let mut f = fixture();
        f[0].1.x = i64::MAX;
        assert!(metadata(&f, 4).is_err());
        let mut f = fixture();
        f[0].1.rotation_quarter = 4;
        assert!(metadata(&f, 4).is_err());
    }
}
