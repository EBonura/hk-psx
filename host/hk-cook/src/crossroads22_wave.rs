//! Full-room admission for the one authored Crossroads_22 arena.
//! The legacy prepass retains its ordinary actor limit. This postpass replaces
//! only this scene's actor metadata with the forty source-bound placements.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or("truncated arena bank")?
            .try_into()
            .map_err(|_| "arena word")?,
    ))
}
fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2166136261, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(16777619)
    })
}
fn load(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(
        &std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
    )?)
}
fn save(path: &Path, value: &Value) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn artifact(root: &Path) -> Result<PathBuf> {
    let report = load(&root.join("data/regions.json"))?;
    Ok(root.join(
        report["crossroads22_wave_artifact"]
            .as_str()
            .ok_or("Crossroads_22 full-room preparation missing")?,
    ))
}

/// Update an already cooked arena's summon coordinates without appending its
/// identical art again. The old receipt and room files remain intact.
pub fn bind_arrivals(root: &Path) -> Result<()> {
    use hk_unity::{scene::Scene, Source};
    let old = artifact(root)?;
    let mut bridge = load(&old.join("report.json"))?;
    let source = Source::from_doctor(root)?;
    let scene = Scene::new(&source, "level59")?;
    let catalogue: Vec<_> = crate::quality::SCENE_TABLE
        .iter()
        .map(|s| {
            (
                s.file.to_string(),
                s.runtime_bounds.map(|n| n.f64()),
                s.scene_name.to_string(),
            )
        })
        .collect();
    let rows = crate::actors::scan_crossroads22(&scene, &source, &catalogue)?;
    let placements = bridge["placements"]
        .as_array_mut()
        .filter(|p| p.len() == 40)
        .ok_or("arrival binding needs all forty original placements")?;
    for placement in placements {
        let id = placement["placement"]["source_id"]
            .as_i64()
            .ok_or("original source id")?;
        let row = rows
            .iter()
            .find(|r| r.spec_source_id == id && r.supported)
            .ok_or("arena source actor no longer admitted")?;
        if placement["game_object"].as_i64() != Some(row.game_object) {
            return Err("summon actor owner drift".into());
        }
        let member = crate::battle::membership(
            &scene,
            row.game_object,
            &crate::common::component_records(&scene, row.game_object),
        )?;
        if placement["wave"].as_u64() != Some(u64::from(member.0))
            || placement["removed_on_start"].as_bool() != Some(member.1)
        {
            return Err("arena member lifetime changed".into());
        }
        placement["arrival"] = if member.0 != 0 {
            { let contract = crate::battle_waves::arrival_position(&scene, row)?;
                placement["authored_depth"] = json!(contract.1);
                json!(contract.0) }
        } else {
            Value::Null
        };
    }
    let report_path = root.join("data/regions.json");
    let mut report = load(&report_path)?;
    let relative = format!(
        "{}-arrival-bound",
        report["crossroads22_wave_artifact"]
            .as_str()
            .ok_or("arena artifact pointer")?
    );
    let out = root.join(&relative);
    if out.exists() {
        return Err("arrival-bound output already exists".into());
    }
    let rooms = bridge["rooms"].as_array().ok_or("arena rooms")?;
    for room in rooms {
        let raw = std::fs::read(old.join(room["path"].as_str().ok_or("room path")?))?;
        if format!("{:x}", Sha256::digest(&raw)) != room["sha256"].as_str().ok_or("room digest")? {
            return Err("arrival binding found changed arena art".into());
        }
    }
    std::fs::create_dir(&out)?;
    for room in rooms {
        let name = room["path"].as_str().ok_or("room path")?;
        std::fs::copy(old.join(name), out.join(name))?;
    }
    std::fs::copy(old.join("actors.bin"), out.join("actors.bin"))?;
    save(&out.join("report.json"), &bridge)?;
    for region in report["regions"].as_array_mut().ok_or("region list")? {
        if region["scene_id"].as_u64() == Some(21) {
            let room = rooms
                .iter()
                .find(|r| r["chunk_id"] == region["chunk_id"])
                .ok_or("arena chunk lost")?;
            if region["sha256"] != room["sha256"] {
                return Err("arena admission differs from retained art".into());
            }
            region["path"] = json!(format!(
                "{relative}/{}",
                room["path"].as_str().ok_or("room name")?
            ));
        }
    }
    report["crossroads22_wave_artifact"] = json!(relative);
    save(&report_path, &report)?;
    println!("Bound twelve original summon destinations; all forty identities and complete art unchanged");
    finish(root)
}

fn render_budget(raw: &[u8], bridge: &Value) -> Result<()> {
    let room = hk_format::Room::parse(raw).map_err(|e| format!("arena room: {e:?}"))?;
    let mut babies_and_shots = HashSet::new();
    for actor in bridge["placements"]
        .as_array()
        .ok_or("arena placement list")?
    {
        for pair in actor["clips"].as_array().ok_or("arena clip bindings")? {
            let slot = pair[0].as_str().ok_or("arena clip slot")?;
            let clip = usize::try_from(pair[1].as_u64().ok_or("arena clip id")?)?;
            let [first, count, _, _] = room.clip(clip);
            for frame in first..first.checked_add(count).ok_or("arena clip overflow")? {
                let (texture, columns, rows) = room.frame_grid(frame as usize);
                if columns != 1 || rows != 1 {
                    return Err("arena actor frame exceeds the one-tile working-set proof".into());
                }
                if actor["controller"].as_str() == Some("HatcherBaby")
                    || slot == "shot_clip"
                    || slot == "impact_clip"
                {
                    babies_and_shots.insert(texture);
                }
            }
        }
        if let Some(corpse) = actor["corpse"].as_str() {
            let corpse: Value = serde_json::from_str(corpse)?;
            for key in ["air_clip", "land_clip"] {
                let [first, count, _, _] =
                    room.clip(corpse[key].as_u64().ok_or("corpse clip")? as usize);
                for frame in first..first + count {
                    let (_, columns, rows) = room.frame_grid(frame as usize);
                    if columns != 1 || rows != 1 {
                        return Err("arena corpse exceeds one tile".into());
                    }
                }
            }
        }
    }
    // Before activation: four Spitters and one Hatcher. Later waves never
    // exceed five such bodies (dead members still occupy their own slot).
    // All Baby and projectile animation phases are covered simultaneously.
    if babies_and_shots.len() + 5 > 20 {
        return Err("arena actors and damaging shots exceed twenty resident texture tiles".into());
    }
    println!("arena render bound: {} shared Baby/projectile tiles plus five actor bodies; sixty actor/projectile quads", babies_and_shots.len());
    Ok(())
}

/// Run after ordinary room generation and before the unchanged legacy prepass.
pub fn prepare(root: &Path) -> Result<()> {
    let path = root.join("data/regions.json");
    let bytes = std::fs::read(&path)?;
    let mut regions: Value = serde_json::from_slice(&bytes)?;
    if regions.get("crossroads22_wave_artifact").is_some() {
        return Err("arena preparation requires freshly generated base regions".into());
    }
    let suffix = format!("{:x}", Sha256::digest(&bytes));
    let relative = format!(".hkpsx/crossroads22-wave-arrivals-{suffix}");
    let out = root.join(&relative);
    if !out.exists() {
        crate::crossroads22_bridge::main(
            root,
            None,
            &["--out".into(), out.to_string_lossy().into()],
        )
        .map_err(|e| e.to_string())?;
    }
    let bridge = load(&out.join("report.json"))?;
    if bridge["base_regions_sha256"].as_str() != Some(suffix.as_str())
        || bridge["placements"].as_array().map(Vec::len) != Some(40)
        || bridge["placements"][0].get("bounds").is_none()
    {
        return Err("stale arena art or missing typed body bounds".into());
    }
    let rooms = bridge["rooms"].as_array().ok_or("arena rooms absent")?;
    let mut replaced = 0;
    for region in regions["regions"].as_array_mut().ok_or("regions absent")? {
        if region["scene_id"].as_u64() != Some(21) {
            continue;
        }
        let room = rooms
            .iter()
            .find(|r| r["chunk_id"] == region["chunk_id"])
            .ok_or("missing full arena room")?;
        if room["base_sha256"] != region["sha256"] {
            return Err("arena base room drift".into());
        }
        let rel = format!(
            "{relative}/{}",
            room["path"].as_str().ok_or("arena room path")?
        );
        let raw = std::fs::read(root.join(&rel))?;
        if format!("{:x}", Sha256::digest(&raw)) != room["sha256"].as_str().ok_or("room SHA")? {
            return Err("arena complete room hash drift".into());
        }
        render_budget(&raw, &bridge)?;
        region["path"] = json!(rel);
        for key in ["sha256", "bytes", "actor_bank_bytes", "textures"] {
            region[key] = room[key].clone();
        }
        replaced += 1;
    }
    if replaced != rooms.len() || replaced != 15 {
        return Err("arena room membership changed".into());
    }
    regions["crossroads22_wave_artifact"] = json!(relative);
    save(&path, &regions)?;
    Ok(crate::battle_waves::main(root, None)?)
}

fn actor_records(bridge: &Value) -> Result<Vec<[u8; 48]>> {
    let placements = bridge["placements"]
        .as_array()
        .ok_or("arena placements absent")?;
    if placements.len() != 40 {
        return Err("arena must admit all forty placements".into());
    }
    let mut seen = HashSet::new();
    placements
        .iter()
        .map(|row| {
            let p = &row["placement"];
            let id = u32::try_from(p["source_id"].as_i64().ok_or("actor identity")?)?;
            if !seen.insert(id) {
                return Err("duplicate arena actor identity".into());
            }
            let (x, y) = if row["wave"].as_u64().ok_or("actor wave")? != 0 {
                let arrival = row["arrival"]
                    .as_array()
                    .filter(|v| v.len() == 2)
                    .ok_or("summon arrival absent")?;
                (
                    arrival[0].as_i64().ok_or("arrival X")?,
                    arrival[1].as_i64().ok_or("arrival Y")?,
                )
            } else {
                (
                    p["x"].as_i64().ok_or("actor X")?,
                    p["y"].as_i64().ok_or("actor Y")?,
                )
            };
            let bounds = row["bounds"]
                .as_array()
                .filter(|b| b.len() == 4)
                .ok_or("typed actor bounds")?;
            let mut out = [0; 48];
            put(&mut out, 0, id);
            put(&mut out, 4, id);
            out[8..10].copy_from_slice(&5u16.to_le_bytes());
            let flags = 32u16
                | 64
                | u16::from(p["initial_direction"].as_i64().ok_or("actor direction")? > 0)
                | (u16::from(
                    p["random_start_direction"]
                        .as_bool()
                        .ok_or("actor random direction")?,
                ) << 1)
                | (u16::from(p["start_alert"].as_bool().ok_or("actor alert")?) << 2)
                | (u16::from(p["start_right"].as_bool().ok_or("actor orientation")?) << 3)
                | (u16::try_from(
                    p["rotation_quarter"]
                        .as_i64()
                        .filter(|n| (0..4).contains(n))
                        .ok_or("actor rotation")?,
                )? << 8);
            out[10..12].copy_from_slice(&flags.to_le_bytes());
            for i in 0..4 {
                let coordinate = bounds[i]
                    .as_i64()
                    .ok_or("actor body bound")?
                    .checked_add(if i % 2 == 0 { x } else { y })
                    .ok_or("actor bound overflow")?;
                put(&mut out, 12 + i * 4, i32::try_from(coordinate)? as u32);
            }
            put(
                &mut out,
                36,
                u32::try_from(row["spec_index"].as_u64().ok_or("actor spec index")?)?,
            );
            put(&mut out, 40, i32::try_from(x)? as u32);
            put(&mut out, 44, i32::try_from(y)? as u32);
            Ok(out)
        })
        .collect()
}

/// Preserve every non-actor record and every polygon, point and neighbor word.
fn replace_actors(raw: &[u8], actors: &[[u8; 48]], binding: [u8; 32]) -> Result<Vec<u8>> {
    if raw.get(..8) != Some(b"HKWMTA01") || word(raw, 8)? != 21 || actors.len() != 40 {
        return Err("wrong arena metadata identity".into());
    }
    let strides = [76usize, 48, 8, 8, 2];
    let mut sections = Vec::new();
    for (i, stride) in strides.iter().enumerate() {
        let offset = word(raw, 64 + i * 12)? as usize;
        let count = word(raw, 68 + i * 12)? as usize;
        if word(raw, 72 + i * 12)? as usize != *stride {
            return Err("metadata stride drift".into());
        }
        let end = offset
            .checked_add(count.checked_mul(*stride).ok_or("metadata size overflow")?)
            .ok_or("metadata offset overflow")?;
        sections.push(
            raw.get(offset..end)
                .ok_or("truncated metadata section")?
                .to_vec(),
        );
    }
    if sections[0].len() / 76 != word(raw, 48)? as usize {
        return Err("region count mismatch".into());
    }
    let old_objects = std::mem::take(&mut sections[1]);
    let mut objects = Vec::new();
    for region in sections[0].chunks_exact_mut(76) {
        let first = word(region, 60)? as usize;
        let count = word(region, 64)? as usize;
        let end = first.checked_add(count).ok_or("object count overflow")?;
        let original = old_objects
            .get(
                first.checked_mul(48).ok_or("object offset")?
                    ..end.checked_mul(48).ok_or("object end")?,
            )
            .ok_or("region object span invalid")?;
        let start = objects.len() / 48;
        let mut present = HashSet::new();
        for object in original.chunks_exact(48) {
            if u16::from_le_bytes([object[8], object[9]]) == 5 {
                let id = word(object, 0)?;
                let replacement = actors
                    .iter()
                    .find(|a| word(&a[..], 0).ok() == Some(id))
                    .ok_or("ordinary metadata contains an unbound arena actor")?;
                if !present.insert(id) {
                    return Err("duplicate actor in region".into());
                }
                objects.extend(replacement);
            } else {
                objects.extend(object);
            }
        }
        for actor in actors {
            if present.insert(word(&actor[..], 0)?) {
                objects.extend(actor);
            }
        }
        if present.len() != 40 {
            return Err("arena region lost actors".into());
        }
        put(region, 60, u32::try_from(start)?);
        put(region, 64, u32::try_from(objects.len() / 48 - start)?);
    }
    sections[1] = objects;
    let mut output = raw.get(..160).ok_or("metadata header truncated")?.to_vec();
    output[16..48].copy_from_slice(&binding);
    for (i, section) in sections.iter().enumerate() {
        while output.len() % 4 != 0 {
            output.push(0);
        }
        let offset = u32::try_from(output.len())?;
        put(&mut output, 64 + i * 12, offset);
        put(
            &mut output,
            68 + i * 12,
            u32::try_from(section.len() / strides[i])?,
        );
        output.extend(section);
    }
    let len = u32::try_from(output.len())?;
    put(&mut output, 52, len);
    Ok(output)
}

/// Run immediately after the legacy prepass, before code-module selection.
pub fn finish(root: &Path) -> Result<()> {
    let bridge = load(&artifact(root)?.join("report.json"))?;
    let actor_bytes = actor_records(&bridge)?;
    let mut packed = load(&root.join(".hkpsx/packed-scenes.json"))?;
    let descriptor = packed["world_metadata"]
        .as_array_mut()
        .ok_or("world metadata descriptors absent")?
        .iter_mut()
        .find(|d| d["scene_id"].as_u64() == Some(21))
        .ok_or("arena world metadata absent")?;
    let raw_path = root.join(descriptor["raw_path"].as_str().ok_or("metadata raw path")?);
    let original = std::fs::read(&raw_path)?;
    let binding: [u8; 32] = Sha256::digest(serde_json::to_vec(&bridge)?).into();
    let raw = replace_actors(&original, &actor_bytes, binding)?;
    hk_format::world_meta::WorldMeta::parse(&raw)
        .map_err(|e| format!("arena metadata validation: {e:?}"))?;
    let block = hk_lz4::compress_hc(&raw);
    if hk_lz4::decompress(&block, raw.len()).as_deref() != Some(raw.as_slice()) {
        return Err("arena metadata compression roundtrip failed".into());
    }
    let mut compressed = b"HLZC".to_vec();
    compressed.extend(u32::try_from(raw.len())?.to_le_bytes());
    compressed.extend(block);
    if compressed.len() >= raw.len() {
        compressed = raw.clone();
    }
    std::fs::write(&raw_path, &raw)?;
    let stored_path = root.join(
        descriptor["stored_path"]
            .as_str()
            .ok_or("metadata stored path")?,
    );
    std::fs::write(stored_path, &compressed)?;
    descriptor["bytes"] = json!(raw.len());
    descriptor["raw_len"] = json!(raw.len());
    descriptor["object_count"] = json!(word(&raw, 80)?);
    descriptor["bank_fnv"] = json!(fnv(&raw));
    descriptor["raw_sha256"] = json!(format!("{:x}", Sha256::digest(&raw)));
    descriptor["stored_len"] = json!(compressed.len());
    descriptor["stored_fnv"] = json!(fnv(&compressed));
    descriptor["stored_sha256"] = json!(format!("{:x}", Sha256::digest(&compressed)));
    descriptor["fingerprint"] = json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&bridge)?)
    ));
    descriptor["section_bytes"] = json!((0..5)
        .map(|i| word(&raw, 68 + i * 12).unwrap() * word(&raw, 72 + i * 12).unwrap())
        .collect::<Vec<_>>());
    let row = format!("WorldMetaDesc{{scene_id:21,raw_len:{},raw_fnv:{},bank_fnv:{},stored_len:{},stored_fnv:{},chunk_id:{},fingerprint:[{}]}},",
        raw.len(), word(&raw, 12)?, fnv(&raw), compressed.len(), fnv(&compressed),
        descriptor["chunk_id"].as_u64().ok_or("metadata chunk id")?,
        binding.iter().map(u8::to_string).collect::<Vec<_>>().join(","));
    let manifest_path = root.join("data/scene_manifest.rs");
    let manifest = std::fs::read_to_string(&manifest_path)?;
    let mut replaced = 0;
    let new_manifest = manifest
        .lines()
        .map(|line| {
            if line.starts_with("WorldMetaDesc{scene_id:21,") {
                replaced += 1;
                row.clone()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let arena_len = manifest
        .lines()
        .find_map(|l| l.strip_prefix("pub const WORLD_META_ARENA_BYTES:usize="))
        .and_then(|l| l.strip_suffix(';'))
        .and_then(|l| l.parse::<usize>().ok())
        .ok_or("metadata arena bound absent")?;
    if replaced != 1 || raw.len() > arena_len {
        return Err("arena metadata exceeds existing bank allocation".into());
    }
    std::fs::write(manifest_path, new_manifest)?;
    // Arrival rebinding changes only the paths of hash-verified identical
    // room banks. Publish their current metadata binding after the cold-bank
    // replacement has succeeded, so coverage validates the actual report.
    packed["source_metadata_sha256"] = json!(format!(
        "{:x}", Sha256::digest(std::fs::read(root.join("data/regions.json"))?)
    ));
    save(&root.join(".hkpsx/packed-scenes.json"), &packed)?;
    let path = root.join("data/regions.rs");
    let text = std::fs::read_to_string(&path)?;
    let begin = text
        .find("SCENE_ACTORS_21:")
        .ok_or("scene actor catalogue absent")?;
    let first = text[begin..]
        .find("&[")
        .ok_or("scene actor catalogue type")?
        + begin;
    let first = text[first + 2..]
        .find("&[")
        .ok_or("scene actor catalogue value")?
        + first
        + 2;
    let end = text[first..]
        .find("];")
        .ok_or("scene actor catalogue end")?
        + first;
    let specs = bridge["specs"]
        .as_array()
        .ok_or("typed arena catalogue absent")?;
    let mut new_text = text[..first + 2].to_string();
    new_text.push_str(
        &specs
            .iter()
            .map(|s| s.as_str().ok_or("actor spec not text"))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join(",\n"),
    );
    new_text.push_str(&text[end..]);
    // The unchanged legacy formatter emits uncapped ordinary Hatchers. Add
    // the new typed field without changing their behavior or the actor ABI.
    let mut at = 0;
    while let Some(offset) = new_text[at..].find("ActorController::Hatcher {") {
        let start = at + offset;
        let close = new_text[start..]
            .find('}')
            .ok_or("Hatcher spec closing brace")?
            + start;
        if !new_text[start..close].contains("max_hatched") {
            new_text.insert_str(close, ",max_hatched:0");
            at = close + 15;
        } else {
            at = close + 1;
        }
    }
    std::fs::write(path, new_text)?;
    println!("Crossroads_22 admitted: forty actors in fifteen full rooms, four source waves, metadata {} bytes", raw.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<u8>, Vec<[u8; 48]>) {
        let mut raw = vec![0; 160 + 76 + 96];
        raw[..8].copy_from_slice(b"HKWMTA01");
        put(&mut raw, 8, 21);
        put(&mut raw, 48, 1);
        put(&mut raw, 60, 5);
        let mut offset = 160;
        for (i, (count, stride)) in [(1, 76), (2, 48), (0, 8), (0, 8), (0, 2)]
            .into_iter()
            .enumerate()
        {
            put(&mut raw, 64 + i * 12, offset);
            put(&mut raw, 68 + i * 12, count);
            put(&mut raw, 72 + i * 12, stride);
            offset += count * stride;
        }
        let len = raw.len() as u32;
        put(&mut raw, 52, len);
        put(&mut raw, 160, 123);
        put(&mut raw, 224, 2);
        // A scenery record stays byte-for-byte intact before the old actor.
        put(&mut raw, 236, 17);
        raw[244..246].copy_from_slice(&1u16.to_le_bytes());
        put(&mut raw, 284, 100);
        raw[292..294].copy_from_slice(&5u16.to_le_bytes());
        let actors = (100..140)
            .map(|id| {
                let mut a = [0; 48];
                put(&mut a, 0, id);
                put(&mut a, 4, id);
                a[8..10].copy_from_slice(&5u16.to_le_bytes());
                a
            })
            .collect();
        (raw, actors)
    }

    #[test]
    fn every_room_keeps_scenery_and_all_forty_actor_identities() {
        let (raw, actors) = fixture();
        let out = replace_actors(&raw, &actors, [42; 32]).unwrap();
        assert_eq!(&out[16..48], &[42; 32]);
        assert_eq!(word(&out, 80).unwrap(), 41);
        let first = word(&out, 76).unwrap() as usize;
        assert_eq!(&out[first..first + 48], &raw[236..284]);
        assert_eq!(word(&out, 160 + 64).unwrap(), 41);
        let ids = out[first + 48..first + 41 * 48]
            .chunks_exact(48)
            .map(|a| word(a, 0).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, (100..140).collect::<Vec<_>>());
    }

    #[test]
    fn unbound_old_actor_and_partial_placement_sets_are_refused() {
        let (mut raw, actors) = fixture();
        assert!(replace_actors(&raw, &actors[..39], [0; 32]).is_err());
        put(&mut raw, 284, 999);
        assert!(replace_actors(&raw, &actors, [0; 32]).is_err());
    }

    #[test]
    fn a_wave_member_uses_its_arrival_but_retains_its_source_body_and_identity() {
        let placements = (0..40).map(|i| json!({"wave":u8::from(i==0),"arrival":[100,200],"spec_index":0,"bounds":[-1,-2,1,2],
            "placement":{"source_id":100+i,"x":6000000,"y":6000000,"initial_direction":-1,
            "random_start_direction":false,"start_alert":false,"start_right":false,"rotation_quarter":0}})).collect::<Vec<_>>();
        let records = actor_records(&json!({"placements":placements})).unwrap();
        assert_eq!(word(&records[0], 0).unwrap(), 100);
        assert_eq!(
            [
                word(&records[0], 40).unwrap(),
                word(&records[0], 44).unwrap()
            ],
            [100, 200]
        );
        assert_eq!(
            [
                word(&records[0], 12).unwrap(),
                word(&records[0], 16).unwrap()
            ],
            [99, 198]
        );
        assert_eq!(word(&records[1], 40).unwrap(), 6000000);
    }
}
