//! host/opaque_tiles.py in Rust: exact, phase-independent opaque tile
//! certificates from the finished scene banks.
//!
//! Only local final palette words and geometry are inputs. The raster proof is
//! `coverage::certify_tile` (the pinned GPL GPU raster equations), linked in
//! rather than compiled by rustc at cook time. Generated artwork and
//! provenance stay ignored; this never writes discs. The outputs match the
//! Python's byte for byte except the report's `helper` and the two source
//! hashes that named the Python and its helper binary.

use crate::common::{err, Result};
use crate::coverage;
use crate::pyjson::{dumps, dumps_sorted_compact, from_serde, Json};
use serde_json::Value as J;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

pub const MAX_TABLE_BYTES: usize = 66 * 1024;
/// Where the raster equations in host/coverage_raster.rs come from.
pub const RASTER_SOURCE: [(&str, &str); 5] = [
    ("repository", "https://github.com/EBonura/PSoXide-emulator"),
    ("revision", "38af605ac5a6961f3798d432bcfb7cceacece239"),
    ("path", "emu/crates/emulator-core/src/gpu/raster.rs"),
    (
        "sha256",
        "eb24fcaa17fb0a6f7f876b9c20466c9268d2c5c8bf35950fc39d5b91319ef6b6",
    ),
    ("license", "GPL-2.0-or-later"),
];

pub fn sha(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn file_sha(path: &Path) -> Result<String> {
    Ok(sha(&read(path)?))
}
pub fn read_json(path: &Path) -> Result<J> {
    serde_json::from_slice(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}
/// Write only when the bytes differ, through a temporary beside it.
pub fn write_changed(path: &Path, data: &[u8]) -> Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == data) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temp = path.with_file_name(name);
    std::fs::write(&temp, data).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}
/// The helper record a report carries: the proof sources and their upstream.
pub fn helper(sources: &[&str], root: &Path) -> Result<Json> {
    let mut hashes = Vec::new();
    for name in sources {
        hashes.push((name.to_string(), Json::Str(file_sha(&root.join(name))?)));
    }
    Ok(Json::Obj(vec![
        ("source_sha256".into(), Json::Obj(hashes)),
        (
            "upstream".into(),
            Json::Obj(
                RASTER_SOURCE
                    .iter()
                    .map(|(k, v)| (k.to_string(), Json::Str(v.to_string())))
                    .collect(),
            ),
        ),
    ]))
}

pub fn int(v: &J, key: &str) -> Result<i64> {
    v.get(key)
        .and_then(J::as_i64)
        .ok_or_else(|| format!("missing integer {key}"))
}
pub fn text(v: &J, key: &str) -> Result<String> {
    v.get(key)
        .and_then(J::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string {key}"))
}
pub fn list<'a>(v: &'a J, key: &str) -> Result<&'a Vec<J>> {
    v.get(key)
        .and_then(J::as_array)
        .ok_or_else(|| format!("missing list {key}"))
}
fn empty() -> &'static Vec<J> {
    static EMPTY: Vec<J> = Vec::new();
    &EMPTY
}
pub fn u16_at(raw: &[u8], at: usize) -> Result<u16> {
    raw.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "bank read outside its bytes".into())
}
pub fn u32_at(raw: &[u8], at: usize) -> Result<u32> {
    raw.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "bank read outside its bytes".into())
}
pub fn i32_at(raw: &[u8], at: usize) -> Result<i32> {
    Ok(u32_at(raw, at)? as i32)
}
/// A bank section's byte offset (`bank['sections'][name]['offset']`).
pub fn section(bank: &J, name: &str) -> Result<usize> {
    bank.get("sections")
        .and_then(|s| s.get(name))
        .and_then(|s| s.get("offset"))
        .and_then(J::as_u64)
        .map(|v| v as usize)
        .ok_or_else(|| format!("bank without section {name}"))
}
/// One pooled draw record: texture, front, scale, the eight Q8 coordinates and
/// the four flag bytes (`<HHi8i4B` at `draws + gid * 44`).
pub fn draw_record(
    raw: &[u8],
    draws: usize,
    gid: usize,
) -> Result<(u16, u16, i32, [i64; 8], [u8; 4])> {
    let at = draws + gid * 44;
    let mut xy = [0i64; 8];
    for (k, v) in xy.iter_mut().enumerate() {
        *v = i64::from(i32_at(raw, at + 8 + 4 * k)?);
    }
    let flags = raw
        .get(at + 40..at + 44)
        .ok_or("bank read outside its bytes")?;
    Ok((
        u16_at(raw, at)?,
        u16_at(raw, at + 2)?,
        i32_at(raw, at + 4)?,
        xy,
        [flags[0], flags[1], flags[2], flags[3]],
    ))
}
/// A room descriptor's ten words (`<10I` at `rooms + 40 * local`).
pub fn room_desc(raw: &[u8], rooms: usize, local: usize) -> Result<[u32; 10]> {
    let mut d = [0u32; 10];
    for (k, v) in d.iter_mut().enumerate() {
        *v = u32_at(raw, rooms + 40 * local + 4 * k)?;
    }
    Ok(d)
}

/// The complete static renderer mutation union per region (not local hit
/// shapes): every draw grass, breakables, mask fades, reveal masks, Geo rocks
/// and lifeblood cocoons can switch.
pub fn mutable_draws(metadata: &J, geo: &J, life: &J) -> Result<Vec<HashSet<i64>>> {
    let rows = list(metadata, "regions")?;
    let (geo_rows, life_rows) = (list(geo, "bindings")?, list(life, "bindings")?);
    if rows.len() != geo_rows.len() || rows.len() != life_rows.len() {
        return err("Opaque tiles: mutable binding region count differs");
    }
    let mut out = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let mut ids: Vec<&J> = Vec::new();
        for p in list(row, "grass")? {
            ids.extend([&p["off_draw"], &p["on_draw"]]);
        }
        for b in list(row, "breakables")? {
            ids.extend(list(b, "off_draws")?);
            ids.extend(list(b, "on_draws")?);
            for f in b.get("mask_fades").and_then(J::as_array).unwrap_or(empty()) {
                ids.extend(list(f, "draw_indices")?);
            }
        }
        for b in row
            .get("remote_mask_bindings")
            .and_then(J::as_array)
            .unwrap_or(empty())
        {
            ids.extend(list(&b["fade"], "draw_indices")?);
        }
        for b in row
            .get("reveal_mask_bindings")
            .and_then(J::as_array)
            .unwrap_or(empty())
        {
            ids.push(&b["draw"]);
        }
        for b in geo_rows[index]
            .as_array()
            .ok_or("Geo bindings row is not a list")?
        {
            ids.extend(list(b, "off")?);
        }
        ids.extend(list(&life_rows[index], "off")?);
        let draws = int(row, "draws")?;
        let mut set = HashSet::new();
        for i in ids {
            // Python's `type(i) is int`: no floats, no booleans.
            match i.as_i64() {
                Some(i) if (0..draws).contains(&i) => {
                    set.insert(i);
                }
                _ => return err("Opaque tiles: mutable draw index outside region"),
            }
        }
        out.push(set);
    }
    Ok(out)
}

/// Every integer projection shape relative to vertex 0, including reflection.
/// Camera and parallax subtract one Q8 integer per axis; its residue mod 256 is
/// exhaustive here, and absolute source origin and atlas UV translation cancel.
pub fn phase_variants(xy: &[i64; 8]) -> Result<Vec<[(i64, i64); 4]>> {
    let mut xs = BTreeSet::new();
    let mut ys = BTreeSet::new();
    for p in 0..256 {
        xs.insert([0, 2, 4, 6].map(|k| ((xy[k] - p) >> 8) - ((xy[0] - p) >> 8)));
        ys.insert([1, 3, 5, 7].map(|k| -(((xy[k] - p) >> 8) - ((xy[1] - p) >> 8))));
    }
    let variants: Vec<[(i64, i64); 4]> = xs
        .iter()
        .flat_map(|x| {
            ys.iter()
                .map(move |y| core::array::from_fn(|i| (x[i], y[i])))
        })
        .collect();
    if variants.len() > 16 {
        return err("Opaque tiles: phase bound exceeded");
    }
    Ok(variants)
}

/// The guest's source-only `legal_extent_q8`: after viewport rejection, every
/// original vertex and edge is legal in every phase.
pub fn legal_source(xy: &[i64; 8]) -> bool {
    let span = |o: usize| {
        (0..4).map(|k| xy[2 * k + o]).max().unwrap() - (0..4).map(|k| xy[2 * k + o]).min().unwrap()
    };
    span(0) <= 703 * 256 && span(1) <= 511 * 256
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cert {
    pub gx: i16,
    pub gy: i16,
    pub width: u16,
    pub height: u16,
    pub words: Vec<u32>,
}
/// The native proof output, checked as the Python checks it.
pub fn read_certificates(data: &[u8], count: usize) -> Result<Vec<Cert>> {
    if data.len() < 12 || &data[..8] != b"HKTCOT01" || u32_at(data, 8)? as usize != count {
        return err("Opaque tiles: native proof header mismatch");
    }
    let mut pos = 12;
    let mut out = Vec::new();
    for expected in 0..count {
        let ident = u32_at(data, pos)? as usize;
        let gx = u16_at(data, pos + 4)? as i16;
        let gy = u16_at(data, pos + 6)? as i16;
        let (w, h) = (u16_at(data, pos + 8)?, u16_at(data, pos + 10)?);
        let n = u32_at(data, pos + 12)? as usize;
        pos += 16;
        let cells = usize::from(w) * usize::from(h);
        if ident != expected || n != cells.div_ceil(32) || (w != 0) != (h != 0) {
            return err("Opaque tiles: native proof descriptor mismatch");
        }
        let words = (0..n)
            .map(|k| u32_at(data, pos + 4 * k))
            .collect::<Result<Vec<_>>>()?;
        pos += 4 * n;
        if n > 0 && cells % 32 != 0 && words[n - 1] >> (cells % 32) != 0 {
            return err("Opaque tiles: nonzero unused proof bits");
        }
        if n > 0 && words.iter().all(|&w| w == 0) {
            return err("Opaque tiles: all-zero mask was not rejected");
        }
        out.push(Cert {
            gx,
            gy,
            width: w,
            height: h,
            words,
        });
    }
    if pos != data.len() {
        return err("Opaque tiles: trailing native proof output");
    }
    Ok(out)
}

struct Instance {
    scene: i64,
    region: i64,
    draw: usize,
    bank_draw: usize,
    texture: u16,
    source: J,
    front: bool,
    xy: [i64; 8],
    pose: Option<usize>,
}
struct Pose {
    width: u16,
    height: u16,
    mask: Vec<u8>,
    variants: Vec<[(i64, i64); 4]>,
    relative: [i64; 8],
    digest: String,
}

fn int_list(v: impl IntoIterator<Item = i64>) -> Json {
    Json::List(v.into_iter().map(Json::Int).collect())
}
fn cert_json(c: &Cert, offset: usize) -> Json {
    Json::Obj(vec![
        ("gx".into(), Json::Int(c.gx.into())),
        ("gy".into(), Json::Int(c.gy.into())),
        ("width".into(), Json::Int(c.width.into())),
        ("height".into(), Json::Int(c.height.into())),
        ("offset".into(), Json::Int(offset as i64)),
    ])
}
/// `TileCert{gx:..,..,offset:..}` / `GroupCert{..}` rows and the `0x%08x`
/// bitmap words, eight to a line, as both generated tables write them.
pub fn cert_rows(name: &str, certs: &[(Cert, usize)]) -> String {
    certs
        .iter()
        .map(|(c, offset)| {
            format!(
                "{name}{{gx:{},gy:{},width:{},height:{},offset:{offset}}},\n",
                c.gx, c.gy, c.width, c.height
            )
        })
        .collect()
}
pub fn bit_rows(bits: &[u32]) -> String {
    bits.chunks(8)
        .map(|row| {
            row.iter()
                .map(|v| format!("0x{v:08x}"))
                .collect::<Vec<_>>()
                .join(",")
                + ",\n"
        })
        .collect()
}

/// Run what `python3 host/opaque_tiles.py [--grid-shift N] [--variant]` runs.
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let shift = args
        .iter()
        .position(|a| a == "--grid-shift")
        .map(|i| {
            args.get(i + 1)
                .and_then(|v| v.parse().ok())
                .ok_or("--grid-shift 2|3")
        })
        .transpose()?
        .unwrap_or(2);
    cook(root, shift, args.iter().any(|a| a == "--variant"))
}

pub fn cook(root: &Path, grid_shift: u32, variant: bool) -> Result<()> {
    if grid_shift != 2 && grid_shift != 3 {
        return err("Opaque tiles: supported grid shifts are2and3");
    }
    let cache = root.join(if variant {
        ".hkpsx/opaque-tiles-grid4"
    } else {
        ".hkpsx/opaque-tiles"
    });
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let packed = read_json(&root.join(".hkpsx/packed-scenes.json"))?;
    let metadata_path = root.join("data/regions.json");
    let metadata = read_json(&metadata_path)?;
    let metadata_hash = file_sha(&metadata_path)?;
    // The legacy table is a host intermediate: scene_certificates cuts it into
    // per-scene HKOCSC01 bundles that must each fit the streamed coverage arena,
    // so the whole-catalog bound scales with the scene count (quality.py's
    // SCENE_COUNT, the scene table regions.json carries).
    let scene_count = list(&metadata, "scenes")?.len();
    let window = 16 + (1i64 << grid_shift) - 1;
    let table_budget = scene_count
        * if grid_shift == 2 {
            104 * 1024
        } else {
            MAX_TABLE_BYTES
        };
    if text(&packed, "source_metadata_sha256")? != metadata_hash {
        return err("Opaque tiles: packed scene metadata is stale");
    }
    let geo = read_json(&root.join(".hkpsx/geo-provenance.json"))?;
    let life = read_json(&root.join(".hkpsx/lifeblood-provenance.json"))?;
    for (report, path) in [(&geo, "data/geo.rs"), (&life, "data/lifeblood.rs")] {
        if text(report, "rust_sha256")? != file_sha(&root.join(path))?
            || text(report, "region_metadata_sha256")? != metadata_hash
        {
            return err("Opaque tiles: generated mutable bindings are stale");
        }
    }
    let exclusions = mutable_draws(&metadata, &geo, &life)?;
    let regions = list(&metadata, "regions")?;
    let mut sources: HashMap<i64, Vec<J>> = HashMap::new();
    let mut inputs: Vec<(String, String)> = Vec::new();
    for row in regions {
        let chunk = int(row, "chunk_id")?;
        let name = format!("data/regions/region-{chunk:03}/scene.json");
        let source = list(&read_json(&root.join(&name))?, "draws")?.clone();
        if source.len() as i64 != int(row, "draws")? {
            return err("Opaque tiles: source draw identity count differs");
        }
        sources.insert(chunk, source);
        inputs.push((name.clone(), file_sha(&root.join(&name))?));
    }
    for path in [
        "data/regions.json",
        "data/regions.rs",
        "data/geo.rs",
        "data/lifeblood.rs",
        "host/hk-cook/src/opaque_tiles.rs",
        "host/hk-cook/src/coverage.rs",
        "host/coverage_raster.rs",
        "game/src/world.rs",
        "game/src/reveal_masks.rs",
        "game/src/geo_render.rs",
        "game/src/lifeblood.rs",
        "game/src/great_door.rs",
    ] {
        inputs.push((path.into(), file_sha(&root.join(path))?));
    }
    let banks = list(&packed, "scenes")?;
    for bank in banks {
        let raw_path = text(bank, "raw_path")?;
        let raw_sha = text(bank, "raw_sha256")?;
        if file_sha(&root.join(&raw_path))? != raw_sha {
            return err("Opaque tiles: final bank payload changed");
        }
        inputs.push((raw_path, raw_sha));
        for sr in list(bank, "source_rooms")? {
            let row = &regions[(int(sr, "chunk_id")? - 1) as usize];
            let raw = read(&root.join(text(row, "path")?))?;
            let digest = sha(&raw);
            if digest != text(row, "sha256")?
                || digest != text(sr, "sha256")?
                || u32_at(&raw, 12)? as usize != list(sr, "texture_map")?.len()
            {
                return err("Opaque tiles: final room texture map changed");
            }
        }
    }
    let helper = helper(
        &["host/hk-cook/src/coverage.rs", "host/coverage_raster.rs"],
        root,
    )?;
    let key = sha(dumps_sorted_compact(&Json::Obj(vec![
        (
            "inputs".into(),
            Json::Obj(
                inputs
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            ),
        ),
        (
            "scene_order".into(),
            int_list(banks.iter().map(|b| b["scene_id"].as_i64().unwrap_or(-1))),
        ),
        ("helper".into(), helper.clone()),
        ("format".into(), Json::Str("HKOPAQUETILES01".into())),
        ("grid_shift".into(), Json::Int(grid_shift.into())),
        ("table_budget".into(), Json::Int(table_budget as i64)),
    ]))
    .as_bytes());
    let report_path = cache.join("report.json");
    if let Ok(old) = read_json(&report_path) {
        let outputs = old.get("outputs").and_then(J::as_object);
        let current = outputs.is_some_and(|o| {
            !o.is_empty()
                && o.iter()
                    .all(|(p, h)| file_sha(&root.join(p)).ok().as_deref() == h.as_str())
        });
        if old.get("input_key").and_then(J::as_str) == Some(key.as_str()) && current {
            println!(
                "Opaque tiles: cached {} certificates, {} bytes",
                old["certificate_count"], old["table_bytes"]
            );
            return Ok(());
        }
    }
    let mutated_sources: HashSet<String> = exclusions
        .iter()
        .enumerate()
        .flat_map(|(i, ids)| ids.iter().map(move |&d| (i, d)))
        .map(|(i, d)| sources[&(i as i64 + 1)][d as usize]["source"].to_string())
        .collect();
    let mut mutated_records: HashSet<(i64, usize)> = HashSet::new();
    let mut instances: Vec<Instance> = Vec::new();
    let mut masks: HashMap<(i64, u16), (u16, u16, Vec<u8>, String)> = HashMap::new();
    for bank in banks {
        let sid = int(bank, "scene_id")?;
        let raw = read(&root.join(text(bank, "raw_path")?))?;
        let (textures, palettes, pages) = (
            section(bank, "textures")?,
            section(bank, "palettes")?,
            section(bank, "pages")?,
        );
        for t in 0..int(bank, "textures")? as usize {
            let at = textures + t * 16;
            let [page, u, v, w, h, pal] = core::array::from_fn(|k| u16_at(&raw, at + 2 * k));
            let (page, u, v, w, h, pal) = (page?, u? as usize, v? as usize, w?, h?, pal? as usize);
            if page == 65535 || !(1..=252).contains(&w) || !(1..=252).contains(&h) {
                continue;
            }
            let palette = (0..16)
                .map(|k| u16_at(&raw, palettes + pal * 32 + 2 * k))
                .collect::<Result<Vec<_>>>()?;
            let base = pages + usize::from(page) * 32768;
            let mut mask = Vec::with_capacity(usize::from(w) * usize::from(h));
            for y in 0..usize::from(h) {
                for x in 0..usize::from(w) {
                    let byte = *raw
                        .get(base + (v + y) * 128 + (u + x) / 2)
                        .ok_or("bank read outside its bytes")?;
                    let word = palette[usize::from((byte >> (4 * ((u + x) & 1))) & 15)];
                    mask.push(u8::from(word != 0 && word & 0x8000 == 0));
                }
            }
            if mask.iter().any(|&m| m != 0) {
                let mut hashed = Vec::with_capacity(4 + mask.len());
                hashed.extend(w.to_le_bytes());
                hashed.extend(h.to_le_bytes());
                hashed.extend(&mask);
                let digest = sha(&hashed);
                masks.insert((sid, t as u16), (w, h, mask, digest));
            }
        }
        let (rooms, draws) = (section(bank, "rooms")?, section(bank, "draws")?);
        for (local, sr) in list(bank, "source_rooms")?.iter().enumerate() {
            let region = int(sr, "chunk_id")?;
            let source = &sources[&region];
            let desc = room_desc(&raw, rooms, local)?;
            if i64::from(desc[0]) != region || desc[1] as usize != source.len() {
                return err("Opaque tiles: final bank room reference mismatch");
            }
            for (draw, d) in source.iter().enumerate() {
                let gid = usize::from(u16_at(&raw, desc[5] as usize + draw * 2)?);
                let (tex, front, _scale, xy, _flags) = draw_record(&raw, draws, gid)?;
                if exclusions[(region - 1) as usize].contains(&(draw as i64)) {
                    mutated_records.insert((sid, gid));
                }
                instances.push(Instance {
                    scene: sid,
                    region,
                    draw,
                    bank_draw: gid,
                    texture: tex,
                    source: d["source"].clone(),
                    front: front != 0,
                    xy,
                    pose: None,
                });
            }
        }
    }
    let mut poses: Vec<Pose> = Vec::new();
    let mut lookup: HashMap<(String, [i64; 8]), usize> = HashMap::new();
    for d in &mut instances {
        let Some((w, h, pixels, digest)) = masks.get(&(d.scene, d.texture)) else {
            continue;
        };
        if !legal_source(&d.xy)
            || mutated_sources.contains(&d.source.to_string())
            || mutated_records.contains(&(d.scene, d.bank_draw))
        {
            continue;
        }
        let relative: [i64; 8] = core::array::from_fn(|i| d.xy[i] - d.xy[i % 2]);
        let key = (digest.clone(), relative);
        if !lookup.contains_key(&key) {
            let variants = phase_variants(&d.xy)?;
            let points = || variants.iter().flatten();
            let bounds = [
                points().map(|p| p.0).min().unwrap(),
                points().map(|p| p.1).min().unwrap(),
                points().map(|p| p.0).max().unwrap() + 1,
                points().map(|p| p.1).max().unwrap() + 1,
            ];
            if bounds[2] - bounds[0] < window || bounds[3] - bounds[1] < window {
                continue;
            }
            lookup.insert(key.clone(), poses.len());
            poses.push(Pose {
                width: *w,
                height: *h,
                mask: pixels.clone(),
                variants,
                relative,
                digest: digest.clone(),
            });
        }
        d.pose = Some(lookup[&key]);
    }
    let mut data = b"HKTCIN01".to_vec();
    data.extend((poses.len() as u32).to_le_bytes());
    for (i, p) in poses.iter().enumerate() {
        data.extend((i as u32).to_le_bytes());
        data.extend((p.variants.len() as u32).to_le_bytes());
        data.extend(p.width.to_le_bytes());
        data.extend(p.height.to_le_bytes());
        for v in &p.variants {
            for &(x, y) in v {
                data.extend((x as i32).to_le_bytes());
                data.extend((y as i32).to_le_bytes());
            }
        }
        for &m in &p.mask {
            data.extend(u16::from(m).to_le_bytes());
        }
    }
    let native_input = cache.join("input.bin");
    let native_output = cache.join("output.bin");
    write_changed(&native_input, &data)?;
    let output = coverage::tile_proofs(&data, grid_shift);
    write_changed(&native_output, &output)?;
    let certs = read_certificates(&output, poses.len())?;
    let mut descriptors: Vec<(Cert, usize)> = Vec::new();
    let mut bits: Vec<u32> = Vec::new();
    let mut pose_to_cert: Vec<(usize, usize)> = Vec::new();
    for (i, c) in certs.iter().enumerate() {
        if c.width == 0 {
            continue;
        }
        pose_to_cert.push((i, descriptors.len()));
        descriptors.push((c.clone(), bits.len() * 32));
        bits.extend(&c.words);
    }
    let cert_of: HashMap<usize, usize> = pose_to_cert.iter().copied().collect();
    let certified = |d: &Instance| d.pose.and_then(|p| cert_of.get(&p).copied());
    let mut bindings: Vec<Vec<(usize, usize)>> = vec![Vec::new(); regions.len()];
    let mut order: Vec<&Instance> = instances.iter().collect();
    order.sort_by_key(|d| (d.region, d.front, d.draw));
    for d in order {
        if let Some(c) = certified(d) {
            bindings[(d.region - 1) as usize].push((d.draw, c));
        }
    }
    let mut bank_bindings: Vec<Vec<u16>> = banks
        .iter()
        .map(|b| {
            b["pools"]["draws"]
                .as_u64()
                .map(|n| vec![65535u16; n as usize])
                .ok_or("bank without a draw pool")
        })
        .collect::<std::result::Result<_, _>>()?;
    let bank_index: HashMap<i64, usize> = banks
        .iter()
        .enumerate()
        .map(|(i, b)| (b["scene_id"].as_i64().unwrap_or(-1), i))
        .collect();
    for d in &instances {
        if let Some(ident) = certified(d) {
            let slot = &mut bank_bindings[bank_index[&d.scene]][d.bank_draw];
            if *slot != 65535 && usize::from(*slot) != ident {
                return err("Opaque tiles: pooled draw certificate identity differs");
            }
            *slot = ident as u16;
        }
    }
    // Includes 32-bit target slice descriptors and generated arrays, not text size.
    let binding_bytes = 2 * bank_bindings.iter().map(Vec::len).sum::<usize>();
    let table_bytes =
        12 * descriptors.len() + 4 * bits.len() + binding_bytes + 8 * bank_bindings.len() + 24;
    if table_bytes > table_budget {
        return err(format!("Opaque tiles: generated tables need {table_bytes} bytes, budget {table_budget}; refusing to truncate"));
    }
    let mut rust = format!(
        "// Generated exact{}px-grid/{window}x{window} opaque certificates. Local licensed data.\n",
        1 << grid_shift
    );
    rust += &format!("pub const TILE_CERT_SHIFT:u32={grid_shift};\n");
    rust += &format!(
        "pub static TILE_CERTS:&[TileCert]=&[\n{}];\n",
        cert_rows("TileCert", &descriptors)
    );
    rust += &format!(
        "pub static TILE_CERT_BITS:&[u32]=&[\n{}];\n",
        bit_rows(&bits)
    );
    rust += "pub static SCENE_DRAW_CERTS:&[&[u16]]=&[\n";
    for row in &bank_bindings {
        rust += &format!(
            "&[{}],\n",
            row.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
        );
    }
    rust += "];\n";
    let destination = if variant {
        cache.join("opaque_tiles.rs")
    } else {
        root.join("data/opaque_tiles.rs")
    };
    write_changed(&destination, rust.as_bytes())?;
    let maximum = bindings.iter().map(Vec::len).max().unwrap_or(0);
    let destination_name = destination
        .strip_prefix(root)
        .unwrap_or(&destination)
        .to_string_lossy()
        .into_owned();
    let report = Json::Obj(vec![
        ("format".into(), Json::Str("HKOPAQUETILES01".into())),
        ("input_key".into(), Json::Str(key)),
        ("input_sha256".into(), Json::Obj(inputs.into_iter().map(|(k, v)| (k, Json::Str(v))).collect())),
        ("helper".into(), helper),
        ("native_input_sha256".into(), Json::Str(sha(&data))),
        ("native_output_sha256".into(), Json::Str(sha(&output))),
        ("pose_count".into(), Json::Int(poses.len() as i64)),
        ("certificate_count".into(), Json::Int(descriptors.len() as i64)),
        ("rejected_empty_poses".into(), Json::Int((poses.len() - descriptors.len()) as i64)),
        ("phase_count".into(), Json::Int(poses.iter().map(|p| p.variants.len()).sum::<usize>() as i64)),
        ("table_bytes".into(), Json::Int(table_bytes as i64)),
        ("table_budget".into(), Json::Int(table_budget as i64)),
        ("grid_shift".into(), Json::Int(grid_shift.into())),
        ("window".into(), Json::Int(window)),
        ("descriptor_bytes".into(), Json::Int(12 * descriptors.len() as i64)),
        ("bitmap_bytes".into(), Json::Int(4 * bits.len() as i64)),
        ("binding_bytes".into(), Json::Int(binding_bytes as i64)),
        ("binding_count".into(), Json::Int(bindings.iter().map(Vec::len).sum::<usize>() as i64)),
        ("maximum_region_candidates".into(), Json::Int(maximum as i64)),
        ("region_bindings".into(), Json::List(bindings.iter().map(|row| Json::List(row.iter().map(|&(d, c)| int_list([d as i64, c as i64])).collect())).collect())),
        ("bank_bindings".into(), Json::List(bank_bindings.iter().map(|row| int_list(row.iter().map(|&v| i64::from(v)))).collect())),
        ("certificates".into(), Json::List(descriptors.iter().map(|(c, o)| cert_json(c, *o)).collect())),
        ("pose_to_certificate".into(), Json::Obj(pose_to_cert.iter().map(|&(p, c)| (p.to_string(), Json::Int(c as i64))).collect())),
        ("poses".into(), Json::List(poses.iter().map(|p| Json::Obj(vec![
            ("width".into(), Json::Int(p.width.into())),
            ("height".into(), Json::Int(p.height.into())),
            ("relative_q8".into(), int_list(p.relative)),
            ("opaque_mask_sha256".into(), Json::Str(p.digest.clone())),
        ])).collect())),
        ("source_bindings".into(), Json::List(instances.iter().filter(|d| certified(d).is_some()).map(|d| Json::Obj(vec![
            ("scene".into(), Json::Int(d.scene)),
            ("region".into(), Json::Int(d.region)),
            ("draw".into(), Json::Int(d.draw as i64)),
            ("bank_draw".into(), Json::Int(d.bank_draw as i64)),
            ("texture".into(), Json::Int(d.texture.into())),
            ("source".into(), from_serde(&d.source)),
            ("front".into(), Json::Bool(d.front)),
            ("pose".into(), Json::Int(d.pose.unwrap_or(0) as i64)),
        ])).collect())),
        ("proof".into(), Json::Str(format!("Every bit certifies{window}x{window} nonzero STP-clear samples across every exact relativeQ8 projection shape. Original topology0-1-2/1-3-2 and pinned native PS1 raster equations. Unknown atlas neighbors never certify. All-zero masks and borders removed."))),
        ("outputs".into(), Json::Obj(vec![(destination_name, Json::Str(sha(rust.as_bytes())))])),
    ]);
    write_changed(&report_path, (dumps(&report) + "\n").as_bytes())?;
    println!("Opaque tiles: {}/{} poses, {table_bytes}/{table_budget} bytes, {maximum} maximum region candidates", descriptors.len(), poses.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pixel_aligned_quad_has_one_shape_per_axis() {
        // Every vertex on a whole pixel: no camera phase changes the shape.
        let xy = [0, 0, 64 * 256, 0, 0, 32 * 256, 64 * 256, 32 * 256];
        assert_eq!(
            phase_variants(&xy).unwrap(),
            vec![[(0, 0), (64, 0), (0, -32), (64, -32)]]
        );
    }
    #[test]
    fn half_pixel_offsets_give_two_shapes_per_axis() {
        let xy = [
            0,
            0,
            64 * 256 + 128,
            0,
            0,
            32 * 256,
            64 * 256 + 128,
            32 * 256,
        ];
        let v = phase_variants(&xy).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v.iter().map(|q| q[1].0).collect::<Vec<_>>(), [64, 65]);
    }
    #[test]
    fn legal_extent_matches_the_guest() {
        assert!(legal_source(&[
            0,
            0,
            703 * 256,
            0,
            0,
            511 * 256,
            703 * 256,
            511 * 256
        ]));
        assert!(!legal_source(&[0, 0, 703 * 256 + 1, 0, 0, 0, 0, 0]));
    }
    #[test]
    fn certificates_reject_a_bad_header_and_trailing_bytes() {
        let mut ok = b"HKTCOT01".to_vec();
        ok.extend(0u32.to_le_bytes());
        assert_eq!(read_certificates(&ok, 0).unwrap(), vec![]);
        assert!(read_certificates(&ok, 1).is_err());
        ok.push(0);
        assert!(read_certificates(&ok, 0).is_err());
    }
    #[test]
    fn generated_rows_keep_the_python_text() {
        let c = Cert {
            gx: -1,
            gy: 2,
            width: 3,
            height: 1,
            words: vec![5],
        };
        assert_eq!(
            cert_rows("TileCert", &[(c, 32)]),
            "TileCert{gx:-1,gy:2,width:3,height:1,offset:32},\n"
        );
        assert_eq!(bit_rows(&[1, 2, 3, 4, 5, 6, 7, 8, 9]), "0x00000001,0x00000002,0x00000003,0x00000004,0x00000005,0x00000006,0x00000007,0x00000008,\n0x00000009,\n");
    }

    // The cases of tests/test_opaque_tiles.py.
    #[test]
    fn all_mutation_bindings_are_excluded_including_remote_and_reveal() {
        let mut row = serde_json::json!({
            "draws": 12,
            "grass": [{"off_draw": 0, "on_draw": 1}],
            "breakables": [{"off_draws": [2], "on_draws": [3], "mask_fades": [{"draw_indices": [4]}]}],
            "remote_mask_bindings": [{"fade": {"draw_indices": [5]}}],
            "reveal_mask_bindings": [{"draw": 6}],
        });
        let geo = serde_json::json!({"bindings": [[{"off": [7]}]]});
        let life = serde_json::json!({"bindings": [{"off": [8]}]});
        let expected: HashSet<i64> = (0..9).collect();
        assert_eq!(
            mutable_draws(&serde_json::json!({"regions": [row.clone()]}), &geo, &life).unwrap(),
            vec![expected]
        );
        row["reveal_mask_bindings"][0]["draw"] = serde_json::json!(12);
        assert!(mutable_draws(&serde_json::json!({"regions": [row]}), &geo, &life).is_err());
    }
    #[test]
    fn phase_set_contains_projection_for_negative_coordinates_and_parallax() {
        let mut state = 6142u64;
        let mut next = |lo: i64, hi: i64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            lo + (state % (hi - lo) as u64) as i64
        };
        for _ in 0..30 {
            let xy: [i64; 8] = core::array::from_fn(|_| next(-200000, 200000));
            let actual: HashSet<Vec<(i64, i64)>> = phase_variants(&xy)
                .unwrap()
                .into_iter()
                .map(|v| v.to_vec())
                .collect();
            assert!(actual.len() <= 16);
            for _ in 0..200 {
                let (camera, scale) = (
                    [next(-10000000, 10000000), next(-10000000, 10000000)],
                    next(1, 200000),
                );
                let (cx, cy) = ((camera[0] * scale) >> 12, (camera[1] * scale) >> 12);
                let v: Vec<(i64, i64)> = (0..8)
                    .step_by(2)
                    .map(|i| ((xy[i] - cx) >> 8, -((xy[i + 1] - cy) >> 8)))
                    .collect();
                let relative: Vec<(i64, i64)> =
                    v.iter().map(|&(x, y)| (x - v[0].0, y - v[0].1)).collect();
                assert!(actual.contains(&relative));
            }
        }
    }
    #[test]
    fn output_rejects_padding_and_empty_payload_aliases() {
        let mut valid = b"HKTCOT01".to_vec();
        valid.extend(1u32.to_le_bytes());
        valid.extend(0u32.to_le_bytes());
        valid.extend((-2i16).to_le_bytes());
        valid.extend(3i16.to_le_bytes());
        valid.extend([1, 0, 1, 0]);
        valid.extend(1u32.to_le_bytes());
        valid.extend(1u32.to_le_bytes());
        assert_eq!(read_certificates(&valid, 1).unwrap()[0].words, vec![1]);
        let mut padded = valid.clone();
        padded.push(b'x');
        let mut two = valid.clone();
        let at = two.len() - 4;
        two[at..].copy_from_slice(&2u32.to_le_bytes());
        let mut empty = valid.clone();
        empty[at..].copy_from_slice(&0u32.to_le_bytes());
        for payload in [padded, two, empty] {
            assert!(read_certificates(&payload, 1).is_err());
        }
    }
    #[test]
    fn identical_generation_preserves_mtime() {
        let dir = std::env::temp_dir().join(format!("hk-opaque-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("generated.rs");
        assert!(write_changed(&path, b"abc").unwrap());
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!write_changed(&path, b"abc").unwrap());
        assert_eq!(stamp, std::fs::metadata(&path).unwrap().modified().unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
