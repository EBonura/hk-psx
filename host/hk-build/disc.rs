//! The disc: WORLD.PAK's chunks staged and put in read order, then mkisopsx.
//! Ported from the packaging half of `build()` in host/build_guest.py and its
//! `disc_order`, which this replaces; the chunk ids, their order and the
//! mkisopsx command line are the same, plus the XA song file where the Red Book
//! tracks were.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The ADPCM banks the guest streams from the disc, in the order it uploads
/// them (the world bank is read before the title, the rest at bootstrap), and
/// the quick map's room art, which rides the same staged read.
const AUDIO_BANKS: [&str; 5] = [
    "sfx.adpcm",
    "geo-audio.adpcm",
    "runner-audio.adpcm",
    "world-sfx.adpcm",
    "game-map.bin",
];

fn field<'a>(v: &'a Value, key: &str) -> Result<&'a Value> {
    v.get(key)
        .ok_or_else(|| format!("missing field {key}").into())
}
fn int(v: &Value, key: &str) -> Result<usize> {
    field(v, key)?
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| format!("{key} is not a number").into())
}
fn path_of(v: &Value, key: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(
        field(v, key)?
            .as_str()
            .ok_or_else(|| format!("{key} is not a path"))?,
    ))
}
fn list<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    field(v, key)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{key} is not a list").into())
}
fn json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(
        &std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
    )?)
}
fn sha256(path: &Path) -> Result<String> {
    Ok(Sha256::digest(std::fs::read(path)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The chunk ids of each kind, numbered by kind as the guest's manifests index them.
struct Ids {
    rooms: usize,
    clips: usize,
    atlases: usize,
}

/// The chunks of WORLD.PAK: the cooked inventories and the id each kind starts at.
pub struct Plan {
    pub rooms: Vec<Value>,
    pub clips: usize,
    pub atlases: Vec<Value>,
    pub bundles: Vec<Value>,
    pub world_banks: Vec<Value>,
    pub effect_base: usize,
    pub bank_ids: Vec<usize>,
    pub music_ids: Vec<usize>,
    pub scene_sfx_ids: Vec<usize>,
    pub menu_id: usize,
    /// Code and art chunks by id, with the scene index each one heads or ends.
    pub code_ids: Vec<(usize, usize)>,
    pub art_ids: Vec<(usize, usize)>,
    pub data_ids: Vec<usize>,
}

/// WORLD.PAK chunk ids in disc order, which is read order rather than kind.
///
/// Chunk ids keep numbering the payloads by kind, as the guest's manifests
/// index them; only where each one sits changes. Boot's reads come first in
/// the order it makes them: the world bank before the title, the quick map's
/// art and the title art, then the SFX bank, Focus, Geo and Runner; then the
/// ambience clips, which a scene gate reads when its area first needs one. Then
/// each manifest scene gets one contiguous group with everything a scene gate
/// reads for it, in the order game/src/disc.rs `admit_scenes` reads it: its
/// code chunk, one-shot bank, coverage, effect art, atlases, the scene, its
/// world metadata and its art chunk. Last, the area music premixes, which only
/// the refill reads, and the carried data packages.
pub fn disc_order(p: &Plan) -> Result<Vec<usize>> {
    let ids = Ids {
        rooms: p.rooms.len(),
        clips: p.clips,
        atlases: p.atlases.len(),
    };
    let (clip_base, atlas_base) = (ids.rooms, ids.rooms + ids.clips);
    let focus = atlas_base + ids.atlases + 1;
    if p.scene_sfx_ids.len() != p.rooms.len() {
        return Err("one scene sound bank per manifest scene".into());
    }
    let (boot, world, game_map) = (&p.bank_ids[..3], p.bank_ids[3], p.bank_ids[4]);
    let mut order = vec![world, game_map, p.menu_id, boot[0], focus, boot[1], boot[2]];
    order.extend(clip_base + 1..=clip_base + ids.clips);
    for (index, room) in p.rooms.iter().enumerate() {
        if int(room, "chunk_id")? != index + 1
            || room
                .get("scene_index")
                .map_or(Ok(index), |_| int(room, "scene_index"))?
                != index
        {
            return Err("scene chunk ids are not manifest order".into());
        }
        let mut cover = Vec::new();
        for b in &p.bundles {
            if int(b, "scene_index")? == index {
                cover.push(int(b, "chunk_id")?);
            }
        }
        let bank = p
            .world_banks
            .get(index)
            .ok_or("missing world metadata bank")?;
        if cover.len() != 1 || int(bank, "scene_id")? != int(room, "scene_id")? {
            return Err(format!("scene group does not line up: {index}").into());
        }
        order.extend(p.code_ids.iter().filter(|c| c.1 == index).map(|c| c.0));
        order.extend([
            p.scene_sfx_ids[index],
            cover[0],
            p.effect_base + int(room, "scene_id")? + 1,
        ]);
        for (i, a) in p.atlases.iter().enumerate() {
            if int(a, "scene_index")? == index {
                order.push(atlas_base + i + 1);
            }
        }
        order.extend([int(room, "chunk_id")?, int(bank, "chunk_id")?]);
        order.extend(p.art_ids.iter().filter(|c| c.1 == index).map(|c| c.0));
    }
    order.extend(&p.music_ids);
    order.extend(&p.data_ids);
    let mut seen = order.clone();
    seen.sort_unstable();
    seen.dedup();
    if seen.len() != order.len() {
        return Err("disc order repeats a chunk".into());
    }
    Ok(order)
}

/// What goes into the image besides the pack.
pub struct Songs {
    /// Raw 2336-byte-sector XA files (`mkisopsx --xa-file`).
    pub xa: Vec<PathBuf>,
    /// Red Book tracks (`--cdda-track`), for comparing against an older disc.
    pub cdda: Vec<PathBuf>,
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
    Ok(())
}

/// Stage every chunk of WORLD.PAK in `chunks` as `chunk_<id>.<kind>` and
/// return the plan's chunk ids. `modules` is the guest build's modules.json.
fn stage(root: &Path, chunks: &Path, modules: &Value) -> Result<Plan> {
    let packed = json(&root.join(".hkpsx/packed-scenes.json"))?;
    let rooms = list(&packed, "scenes")?.to_vec();
    let hk = root.join(".hkpsx");
    let arena = int(&packed, "scene_arena_bytes")?;
    let clips = list(&json(&hk.join("ambience.json"))?, "clips")?.to_vec();
    let atlases = list(&packed, "atlases")?.to_vec();
    let coverage = json(&hk.join("scene-certificates.json"))?;
    let bundles = list(&coverage, "bundles")?.to_vec();
    let focus = json(&hk.join("focus-audio.json"))?;
    let world_banks = packed
        .get("world_metadata")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (nr, nc) = (rooms.len(), clips.len());
    for room in &rooms {
        copy(
            &root.join(path_of(room, "path")?),
            &chunks.join(format!("chunk_{}.hk", int(room, "chunk_id")?)),
        )?;
    }
    for (i, clip) in clips.iter().enumerate() {
        copy(
            &root.join(path_of(clip, "path")?),
            &chunks.join(format!("chunk_{}.adpcm", nr + i + 1)),
        )?;
    }
    for (i, atlas) in atlases.iter().enumerate() {
        copy(
            &root.join(path_of(atlas, "path")?),
            &chunks.join(format!("chunk_{}.atlas", nr + nc + i + 1)),
        )?;
    }
    copy(
        &root.join(path_of(&focus, "path")?),
        &chunks.join(format!("chunk_{}.focus", nr + nc + atlases.len() + 1)),
    )?;
    for bundle in &bundles {
        copy(
            &root.join(path_of(bundle, "path")?),
            &chunks.join(format!("chunk_{}.coverage", int(bundle, "chunk_id")?)),
        )?;
    }
    for bank in &world_banks {
        copy(
            &root.join(path_of(bank, "path")?),
            &chunks.join(format!("chunk_{}.worldmeta", int(bank, "chunk_id")?)),
        )?;
    }
    // One effect-art chunk per catalogue scene id, after the world metadata chunks.
    let effect_base = nr + nc + atlases.len() + 1 + bundles.len() + world_banks.len();
    let effects = json(&hk.join("break-effects/report.json"))?;
    let effect_chunks = list(&effects, "chunks")?;
    for entry in effect_chunks {
        copy(
            &root.join(path_of(entry, "path")?),
            &chunks.join(format!(
                "chunk_{}.effectart",
                effect_base + int(entry, "scene_id")? + 1
            )),
        )?;
    }
    // The SFX, Geo, Runner and world ADPCM banks and the map art, last by kind
    // and in the guest's AUDIO_BANKS order.
    let mut bank_ids = Vec::new();
    for (i, name) in AUDIO_BANKS.iter().enumerate() {
        let payload = root.join("data").join(name);
        if (std::fs::metadata(&payload)?.len() as usize).div_ceil(2048) * 2048 > arena {
            return Err(format!(
                "Audio bank does not fit the startup room arena: {}",
                payload.display()
            )
            .into());
        }
        bank_ids.push(effect_base + effect_chunks.len() + i + 1);
        copy(
            &payload,
            &chunks.join(format!("chunk_{}.bank", bank_ids[i])),
        )?;
    }
    // The area music premixes, after the banks: streamed, never staged.
    let area = json(&hk.join("area-music.json"))?;
    let mut music_ids = Vec::new();
    for (i, track) in list(&area, "tracks")?.iter().enumerate() {
        music_ids.push(bank_ids[bank_ids.len() - 1] + i + 1);
        copy(
            &root.join(path_of(track, "path")?),
            &chunks.join(format!("chunk_{}.music", music_ids[i])),
        )?;
    }
    // One one-shot bank per manifest scene, numbered after the music.
    let sfx = json(&hk.join("scene-sfx.json"))?;
    let mut scene_sfx_ids = Vec::new();
    let sfx_base = music_ids
        .last()
        .copied()
        .unwrap_or(bank_ids[bank_ids.len() - 1]);
    for (i, room) in rooms.iter().enumerate() {
        let id = int(room, "scene_id")?;
        let payload = root
            .join("data/scene-sfx")
            .join(format!("scene_{id}.adpcm"));
        let expected = field(
            field(field(&sfx, "scenes")?, &id.to_string())?,
            "chunk_sha256",
        )?
        .as_str()
        .unwrap_or("");
        if sha256(&payload)? != expected {
            return Err(format!(
                "Scene sound bank changed after cooking: {}",
                payload.display()
            )
            .into());
        }
        scene_sfx_ids.push(sfx_base + i + 1);
        copy(
            &payload,
            &chunks.join(format!("chunk_{}.scenesfx", scene_sfx_ids[i])),
        )?;
    }
    // The title art, numbered last so no older chunk id moved, then the rooms'
    // code chunks in manifest scene order.
    let menu_id = scene_sfx_ids[scene_sfx_ids.len() - 1] + 1;
    copy(
        &root.join("data/boot-art.hk"),
        &chunks.join(format!("chunk_{menu_id}.menu")),
    )?;
    let (mut code_ids, mut art_ids, mut data_ids) = (Vec::new(), Vec::new(), Vec::new());
    for (c, chunk) in list(modules, "chunks")?.iter().enumerate() {
        let id = menu_id + 1 + c;
        match field(chunk, "kind")?.as_str() {
            Some("data") => data_ids.push(id),
            Some("code") => code_ids.push((id, int(chunk, "scene_index")?)),
            _ => art_ids.push((id, int(chunk, "scene_index")?)),
        }
        copy(
            &path_of(chunk, "path")?,
            &chunks.join(format!("chunk_{id}.hkmd")),
        )?;
    }
    Ok(Plan {
        rooms,
        clips: nc,
        atlases,
        bundles,
        world_banks,
        effect_base,
        bank_ids,
        music_ids,
        scene_sfx_ids,
        menu_id,
        code_ids,
        art_ids,
        data_ids,
    })
}

/// Build the disc image from the staged guest: the pack chunks, the order they
/// are read in and the songs, into `library/hk-psx.bin` and `.cue`, replacing
/// the previous pair only once the new one is complete. `work` holds the chunk
/// staging and the order file (the guest build's output directory).
pub fn package(root: &Path, exe: &Path, work: &Path, library: &Path, songs: &Songs) -> Result<()> {
    let modules = json(&work.join("modules.json"))?;
    let chunks = work.join("disc-chunks");
    let _ = std::fs::remove_dir_all(&chunks);
    std::fs::create_dir_all(&chunks)?;
    let plan = stage(root, &chunks, &modules)?;
    let order = work.join("world-pack-order.txt");
    std::fs::write(
        &order,
        disc_order(&plan)?
            .iter()
            .map(|i| format!("{i}\n"))
            .collect::<String>(),
    )?;
    std::fs::create_dir_all(library)?;
    let temp = library.join(format!(".hk-psx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp)?;
    let staged = temp.join("hk-psx.bin");
    let mut command = Command::new("cargo");
    command
        .args(["run", "--locked", "--release", "--manifest-path"])
        .arg(root.join(".psoxide/tools/mkisopsx/Cargo.toml"))
        .arg("--")
        .arg("--exe")
        .arg(exe)
        .arg("--out")
        .arg(&staged)
        .args(["--volume", "HKPSX", "--world-pack-extra-dir"])
        .arg(&chunks)
        .arg("--world-pack-order-file")
        .arg(&order);
    for track in &songs.cdda {
        command.arg("--cdda-track").arg(track);
    }
    for file in &songs.xa {
        command.arg("--xa-file").arg(file);
    }
    println!("+ {command:?}");
    let status = command.current_dir(root).status()?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("mkisopsx failed with {status}").into());
    }
    std::fs::rename(&staged, library.join("hk-psx.bin"))?;
    std::fs::rename(staged.with_extension("cue"), library.join("hk-psx.cue"))?;
    std::fs::remove_dir_all(&temp)?;
    std::fs::remove_dir_all(&chunks)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan() -> Plan {
        Plan {
            rooms: vec![
                json!({"chunk_id": 1, "scene_id": 5}),
                json!({"chunk_id": 2, "scene_id": 6}),
            ],
            clips: 0,
            atlases: Vec::new(),
            bundles: vec![
                json!({"scene_index": 0, "chunk_id": 10}),
                json!({"scene_index": 1, "chunk_id": 11}),
            ],
            world_banks: vec![
                json!({"scene_id": 5, "chunk_id": 20}),
                json!({"scene_id": 6, "chunk_id": 21}),
            ],
            effect_base: 30,
            bank_ids: vec![40, 41, 42, 43, 44],
            music_ids: Vec::new(),
            scene_sfx_ids: vec![50, 51],
            menu_id: 52,
            code_ids: vec![(53, 1)],
            art_ids: Vec::new(),
            data_ids: Vec::new(),
        }
    }

    #[test]
    fn boot_reads_come_first_in_the_order_boot_makes_them() {
        let order = disc_order(&plan()).unwrap();
        // The world bank, the map art, the title art, the SFX bank, Focus, Geo, Runner.
        assert_eq!(order[..7], [43, 44, 52, 40, 3, 41, 42]);
    }

    #[test]
    fn a_code_chunk_heads_its_scene_group() {
        let order = disc_order(&plan()).unwrap();
        let at = order.iter().position(|&c| c == 51).unwrap();
        assert_eq!(order[at - 1], 53);
        assert_eq!(order.iter().filter(|&&c| c == 53).count(), 1);
    }

    #[test]
    fn a_scene_group_is_sfx_coverage_effects_scene_metadata() {
        let order = disc_order(&plan()).unwrap();
        let at = order.iter().position(|&c| c == 50).unwrap();
        assert_eq!(order[at..at + 5], [50, 10, 36, 1, 20]);
    }

    #[test]
    fn music_then_data_packages_come_last() {
        let mut p = plan();
        p.music_ids = vec![60, 61];
        p.data_ids = vec![70];
        let order = disc_order(&p).unwrap();
        assert_eq!(order[order.len() - 3..], [60, 61, 70]);
    }

    #[test]
    fn a_misnumbered_scene_or_a_repeated_chunk_is_refused() {
        let mut p = plan();
        p.rooms[1] = json!({"chunk_id": 3, "scene_id": 6});
        assert!(disc_order(&p).is_err());
        let mut p = plan();
        p.music_ids = vec![40];
        assert!(disc_order(&p).is_err());
        let mut p = plan();
        p.scene_sfx_ids.pop();
        assert!(disc_order(&p).is_err());
    }
}
