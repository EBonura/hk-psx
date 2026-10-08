//! host/opaque_groups.py in Rust: optional bounded seam certificates for
//! immutable flat Tilemap groups.
//!
//! Every retained group is atomic. Budget admission affects only optional
//! culling; this is deliberately not a promise to cover every scene seam. The
//! proof is `coverage::certify_group`. The outputs match the Python's byte for
//! byte except the report's `helper` and the source hashes that named the
//! Python and its helper binary.

use crate::common::{err, Result};
use crate::coverage;
use crate::opaque_tiles::{
    bit_rows, cert_rows, draw_record, file_sha, helper, int, list, mutable_draws, read, read_certificates, read_json, room_desc, section, sha, text,
    u16_at, write_changed, Cert,
};
use crate::pyjson::{dumps, dumps_sorted_compact, Json};
use serde_json::Value as J;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

pub const MAX_TABLE_BYTES: usize = 8 * 1024;
pub const MAX_REGION_GROUPS: usize = 32;
const NO_MEMBER: usize = 65535;

/// Every integer projection shape of a two to four quad group, relative to its
/// first vertex.
pub fn group_phases(xy: &[i64]) -> Result<Vec<Vec<(i64, i64)>>> {
    if ![16, 24, 32].contains(&xy.len()) {
        return err("Opaque groups: expected 2-4 quads");
    }
    let mut xs = BTreeSet::new();
    let mut ys = BTreeSet::new();
    for p in 0..256 {
        xs.insert((0..xy.len()).step_by(2).map(|k| ((xy[k] - p) >> 8) - ((xy[0] - p) >> 8)).collect::<Vec<_>>());
        ys.insert((1..xy.len()).step_by(2).map(|k| -((xy[k] - p) >> 8) + ((xy[1] - p) >> 8)).collect::<Vec<_>>());
    }
    let out: Vec<Vec<(i64, i64)>> = xs.iter().flat_map(|x| ys.iter().map(move |y| x.iter().copied().zip(y.iter().copied()).collect())).collect();
    if out.len() > 256 {
        return err("Opaque groups: phase bound exceeded");
    }
    Ok(out)
}

pub fn axis(xy: &[i64; 8]) -> bool {
    xy[1] == xy[3] && xy[0] == xy[4] && xy[2] == xy[6] && xy[5] == xy[7]
}

/// Whether one cooked draw is an immutable flat opaque Tilemap quad: a tilemap
/// rect, a front draw at a positive scale carrying the BLACK_AVERAGE material,
/// an axis-aligned quad, and nothing mutates its source at runtime. The
/// seventh test, a solid word-1 palette, needs the pack's pixels
/// (`solid_word_one`).
pub fn flat_opaque_record(src: &J, front: u16, scale: i32, xy: &[i64; 8], black_average: u8, mutated_sources: &HashSet<String>) -> bool {
    src.get("tilemap_rect").is_some() && front == 1 && black_average == 1 && scale > 0 && !mutated_sources.contains(&src["source"].to_string()) && axis(xy)
}

#[derive(Clone, Debug, PartialEq)]
struct Record {
    xy: [i64; 8],
    scale: i32,
    texture: u16,
}

/// Stable two to four member groups with a common scale and touching source
/// bounds, in the Python generator's order.
fn connected_groups(records: &BTreeMap<usize, Record>) -> Vec<Vec<usize>> {
    let ids: Vec<usize> = records.keys().copied().collect();
    let boxes: HashMap<usize, [i64; 4]> = ids.iter().map(|&i| {
        let xy = &records[&i].xy;
        let even = || (0..4).map(|k| xy[2 * k]);
        let odd = || (0..4).map(|k| xy[2 * k + 1]);
        (i, [even().min().unwrap(), odd().min().unwrap(), even().max().unwrap(), odd().max().unwrap()])
    }).collect();
    let adjacent = |a: usize, b: usize| {
        let (x, y) = (boxes[&a], boxes[&b]);
        records[&a].scale == records[&b].scale && x[2].min(y[2]) >= x[0].max(y[0]) && x[3].min(y[3]) >= x[1].max(y[1])
    };
    let edges: HashMap<usize, Vec<usize>> = ids.iter().map(|&i| (i, ids.iter().copied().filter(|&j| i != j && adjacent(i, j)).collect())).collect();
    let mut current: BTreeSet<Vec<usize>> = ids.iter().flat_map(|&i| edges[&i].iter().map(move |&j| { let mut g = vec![i, j]; g.sort(); g })).collect();
    let mut out = Vec::new();
    for size in [2, 3, 4] {
        if size > 2 {
            current = current.iter().flat_map(|g| g.iter().flat_map(|i| edges[i].iter()).filter(|j| !g.contains(j)).map(|&j| {
                let mut n = g.clone();
                n.push(j);
                n.sort();
                n
            }).collect::<Vec<_>>()).collect();
        }
        for group in &current {
            let xy: Vec<i64> = group.iter().flat_map(|i| records[i].xy).collect();
            let span = |o: usize| xy.iter().skip(o).step_by(2).max().unwrap() - xy.iter().skip(o).step_by(2).min().unwrap();
            if span(0) <= 703 * 256 && span(1) <= 511 * 256 {
                out.push(group.clone());
            }
        }
    }
    out
}

/// Whether every texel of `texture` reads palette word 1.
pub fn solid_word_one(raw: &[u8], bank: &J, texture: usize) -> Result<bool> {
    let at = section(bank, "textures")? + texture * 16;
    let [page, u, v, w, h, pal] = core::array::from_fn(|k| u16_at(raw, at + 2 * k));
    let (page, u, v, w, h, pal) = (page?, u? as usize, v? as usize, w? as usize, h? as usize, pal? as usize);
    if page == 65535 || w == 0 || h == 0 {
        return Ok(false);
    }
    let (palettes, pages) = (section(bank, "palettes")?, section(bank, "pages")?);
    let palette = (0..16).map(|k| u16_at(raw, palettes + pal * 32 + 2 * k)).collect::<Result<Vec<_>>>()?;
    let base = pages + usize::from(page) * 32768;
    for y in 0..h {
        for x in 0..w {
            let byte = *raw.get(base + (v + y) * 128 + (u + x) / 2).ok_or("bank read outside its bytes")?;
            if palette[usize::from((byte >> (4 * ((u + x) & 1))) & 15)] != 1 {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[derive(Clone)]
struct Candidate {
    scene: usize,
    members: Vec<usize>,
    pose: usize,
    regions: Vec<i64>,
}
type CertKey = (i16, i16, u16, u16, Vec<u32>);
fn cert_key(c: &Cert) -> CertKey {
    (c.gx, c.gy, c.width, c.height, c.words.clone())
}

/// Greedy stable area per byte admission with whole memberships and hard
/// bounds. Marginal bytes are recomputed after a shared bitmap is admitted;
/// region multiplicity weights usefulness across spatial views; ties go to the
/// lower bank and member IDs, then the later candidate (Python's `max`).
fn select_groups(candidates: Vec<Candidate>, certificates: &[Cert], bank_count: usize, budget: usize, max_region: usize) -> (Vec<Candidate>, usize, Vec<(i64, usize)>) {
    let mut used = 24 + 8 * bank_count;
    let mut selected = Vec::new();
    let mut admitted: HashSet<CertKey> = HashSet::new();
    let mut per_region: Vec<(i64, usize)> = Vec::new();
    let count = |per_region: &[(i64, usize)], r: i64| per_region.iter().find(|(k, _)| *k == r).map_or(0, |e| e.1);
    let keys: Vec<CertKey> = certificates.iter().map(cert_key).collect();
    let mut pending = candidates;
    loop {
        // (area, cost, negated identity, index, cost)
        let mut best: Option<(u128, u128, Vec<i64>, usize, usize)> = None;
        for (index, g) in pending.iter().enumerate() {
            let c = &certificates[g.pose];
            if c.width == 0 || g.regions.iter().any(|&r| count(&per_region, r) >= max_region) {
                continue;
            }
            let cost = 10 + if admitted.contains(&keys[g.pose]) { 0 } else { 12 + c.words.len() * 4 };
            if used + cost > budget {
                continue;
            }
            let area = c.words.iter().map(|w| w.count_ones() as u128).sum::<u128>() * g.regions.len() as u128;
            let tie: Vec<i64> = std::iter::once(g.scene).chain(g.members.iter().copied()).map(|x| -(x as i64)).collect();
            let better = match &best {
                None => true,
                Some((a, k, t, _, _)) => match (area * *k).cmp(&(*a * cost as u128)) {
                    Ordering::Greater => true,
                    Ordering::Less => false,
                    // Equal scores: the larger negated identity, then the later index.
                    Ordering::Equal => tie >= *t,
                },
            };
            if better {
                best = Some((area, cost as u128, tie, index, cost));
            }
        }
        let Some((_, _, _, index, cost)) = best else { break };
        let g = pending.remove(index);
        used += cost;
        admitted.insert(keys[g.pose].clone());
        for &r in &g.regions {
            match per_region.iter_mut().find(|(k, _)| *k == r) {
                Some(e) => e.1 += 1,
                None => per_region.push((r, 1)),
            }
        }
        selected.push(g);
    }
    (selected, used, per_region)
}

fn int_list(v: impl IntoIterator<Item = i64>) -> Json {
    Json::List(v.into_iter().map(Json::Int).collect())
}

/// Run what `python3 host/opaque_groups.py` runs.
pub fn main(root: &Path) -> Result<()> {
    cook(root)
}

pub fn cook(root: &Path) -> Result<()> {
    let cache = root.join(".hkpsx/opaque-groups");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let packed = read_json(&root.join(".hkpsx/packed-scenes.json"))?;
    let metadata = read_json(&root.join("data/regions.json"))?;
    let mh = file_sha(&root.join("data/regions.json"))?;
    if text(&packed, "source_metadata_sha256")? != mh {
        return err("Opaque groups: stale packed scene metadata");
    }
    let geo = read_json(&root.join(".hkpsx/geo-provenance.json"))?;
    let life = read_json(&root.join(".hkpsx/lifeblood-provenance.json"))?;
    for (report, path) in [(&geo, "data/geo.rs"), (&life, "data/lifeblood.rs")] {
        if text(report, "rust_sha256")? != file_sha(&root.join(path))? || text(report, "region_metadata_sha256")? != mh {
            return err("Opaque groups: stale mutable bindings");
        }
    }
    let exclusions = mutable_draws(&metadata, &geo, &life)?;
    let rows = list(&metadata, "regions")?;
    let mut sources: HashMap<i64, Vec<J>> = HashMap::new();
    let mut inputs: Vec<(String, String)> = Vec::new();
    for row in rows {
        let chunk = int(row, "chunk_id")?;
        let name = format!("data/regions/region-{chunk:03}/scene.json");
        let draws = list(&read_json(&root.join(&name))?, "draws")?.clone();
        inputs.push((name.clone(), file_sha(&root.join(&name))?));
        if draws.len() as i64 != int(row, "draws")? {
            return err("Opaque groups: source identity count differs");
        }
        sources.insert(chunk, draws);
    }
    for name in [
        "data/regions.json", "data/geo.rs", "data/lifeblood.rs", "host/hk-cook/src/opaque_groups.rs", "host/hk-cook/src/opaque_tiles.rs",
        "host/hk-cook/src/coverage.rs", "host/coverage_raster.rs", "game/src/world.rs", "game/src/reveal_masks.rs", "game/src/geo_render.rs",
        "game/src/lifeblood.rs", "game/src/great_door.rs",
    ] {
        inputs.push((name.into(), file_sha(&root.join(name))?));
    }
    let banks = list(&packed, "scenes")?;
    for bank in banks {
        let (raw_path, raw_sha) = (text(bank, "raw_path")?, text(bank, "raw_sha256")?);
        if file_sha(&root.join(&raw_path))? != raw_sha {
            return err("Opaque groups: final bank payload changed");
        }
        inputs.push((raw_path, raw_sha));
        for sr in list(bank, "source_rooms")? {
            let row = &rows[(int(sr, "chunk_id")? - 1) as usize];
            let raw = read(&root.join(text(row, "path")?))?;
            let digest = sha(&raw);
            if digest != text(row, "sha256")? || digest != text(sr, "sha256")?
                || crate::opaque_tiles::u32_at(&raw, 12)? as usize != list(sr, "texture_map")?.len()
            {
                return err("Opaque groups: final room mapping changed");
            }
        }
    }
    let helper = helper(&["host/hk-cook/src/coverage.rs", "host/coverage_raster.rs"], root)?;
    let key = sha(dumps_sorted_compact(&Json::Obj(vec![
        ("inputs".into(), Json::Obj(inputs.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect())),
        ("scene_order".into(), int_list(banks.iter().map(|b| b["scene_id"].as_i64().unwrap_or(-1)))),
        ("helper".into(), helper.clone()),
        ("format".into(), Json::Str("HKOPAQUEGROUPS01".into())),
        ("budget".into(), Json::Int(MAX_TABLE_BYTES as i64)),
        ("region_limit".into(), Json::Int(MAX_REGION_GROUPS as i64)),
    ]))
    .as_bytes());
    let report_path = cache.join("report.json");
    if let Ok(old) = read_json(&report_path) {
        let outputs = old.get("outputs").and_then(J::as_object);
        let current = outputs.is_some_and(|o| !o.is_empty() && o.iter().all(|(p, h)| file_sha(&root.join(p)).ok().as_deref() == h.as_str()));
        if old.get("input_key").and_then(J::as_str) == Some(key.as_str()) && current {
            println!("Opaque groups: cached {} groups, {} bytes", old["selected_groups"], old["table_bytes"]);
            return Ok(());
        }
    }
    let mutated_sources: HashSet<String> = exclusions.iter().enumerate()
        .flat_map(|(i, ids)| ids.iter().map(move |&d| (i, d)))
        .map(|(i, d)| sources[&(i as i64 + 1)][d as usize]["source"].to_string())
        .collect();
    let mut mutated_records: HashSet<(usize, usize)> = HashSet::new();
    let mut records: HashMap<(usize, usize), Record> = HashMap::new();
    let mut regions: Vec<(usize, i64, BTreeMap<usize, usize>)> = Vec::new();
    for (bank_index, bank) in banks.iter().enumerate() {
        let raw = read(&root.join(text(bank, "raw_path")?))?;
        let (rooms, draws) = (section(bank, "rooms")?, section(bank, "draws")?);
        let mut solid: HashMap<u16, bool> = HashMap::new();
        for (local, sr) in list(bank, "source_rooms")?.iter().enumerate() {
            let region = int(sr, "chunk_id")?;
            let desc = room_desc(&raw, rooms, local)?;
            let source = &sources[&region];
            let mut row = BTreeMap::new();
            if i64::from(desc[0]) != region || desc[1] as usize != source.len() {
                return err("Opaque groups: room reference mismatch");
            }
            for (draw, src) in source.iter().enumerate() {
                let gid = usize::from(u16_at(&raw, desc[5] as usize + draw * 2)?);
                let (tex, front, scale, xy, flags) = draw_record(&raw, draws, gid)?;
                if exclusions[(region - 1) as usize].contains(&(draw as i64)) {
                    mutated_records.insert((bank_index, gid));
                }
                if !flat_opaque_record(src, front, scale, &xy, flags[3], &mutated_sources) {
                    continue;
                }
                let is_solid = match solid.get(&tex) {
                    Some(&s) => s,
                    None => {
                        let s = solid_word_one(&raw, bank, usize::from(tex))?;
                        solid.insert(tex, s);
                        s
                    }
                };
                if !is_solid {
                    continue;
                }
                let record = Record { xy, scale, texture: tex };
                let existing = records.entry((bank_index, gid)).or_insert_with(|| record.clone());
                if *existing != record {
                    return err("Opaque groups: pooled source mismatch");
                }
                row.entry(gid).or_insert(draw);
            }
            regions.push((bank_index, region, row));
        }
    }
    for ident in &mutated_records {
        records.remove(ident);
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut candidate_index: HashMap<(usize, Vec<usize>), usize> = HashMap::new();
    let mut poses: Vec<(Vec<i64>, Vec<Vec<(i64, i64)>>, usize)> = Vec::new();
    let mut pose_lookup: HashMap<Vec<i64>, usize> = HashMap::new();
    for (bank, region, row) in &regions {
        let local: BTreeMap<usize, Record> = row.keys().filter_map(|gid| records.get(&(*bank, *gid)).map(|r| (*gid, r.clone()))).collect();
        for members in connected_groups(&local) {
            let identity = (*bank, members.clone());
            if !candidate_index.contains_key(&identity) {
                let xy: Vec<i64> = members.iter().flat_map(|i| local[i].xy).collect();
                let relative: Vec<i64> = xy.iter().enumerate().map(|(i, v)| v - xy[i % 2]).collect();
                let pose = match pose_lookup.get(&relative) {
                    Some(&p) => p,
                    None => {
                        let phases = group_phases(&xy)?;
                        let too_wide = phases.iter().any(|p| {
                            let span = |f: fn(&(i64, i64)) -> i64| p.iter().map(f).max().unwrap() - p.iter().map(f).min().unwrap();
                            span(|q| q.0) > 703 || span(|q| q.1) > 511
                        });
                        if too_wide {
                            continue;
                        }
                        let p = poses.len();
                        pose_lookup.insert(relative.clone(), p);
                        poses.push((relative, phases, members.len()));
                        p
                    }
                };
                candidate_index.insert(identity.clone(), candidates.len());
                candidates.push(Candidate { scene: *bank, members, pose, regions: Vec::new() });
            }
            candidates[candidate_index[&identity]].regions.push(*region);
        }
    }
    let mut payload = b"HKGPIN01".to_vec();
    payload.extend((poses.len() as u32).to_le_bytes());
    for (i, (_, phases, members)) in poses.iter().enumerate() {
        for v in [i, phases.len(), *members] {
            payload.extend((v as u32).to_le_bytes());
        }
        for v in phases {
            for &(x, y) in v {
                payload.extend((x as i32).to_le_bytes());
                payload.extend((y as i32).to_le_bytes());
            }
        }
    }
    write_changed(&cache.join("input.bin"), &payload)?;
    let output = coverage::group_proofs(&payload);
    write_changed(&cache.join("output.bin"), &output)?;
    let certificates = read_certificates(&output, poses.len())?;
    let candidate_count = candidates.len();
    let (mut selected, total, counts) = select_groups(candidates, &certificates, banks.len(), MAX_TABLE_BYTES, MAX_REGION_GROUPS);
    selected.sort_by(|a, b| (a.scene, &a.members).cmp(&(b.scene, &b.members)));
    let mut descriptors: Vec<(Cert, usize)> = Vec::new();
    let mut bits: Vec<u32> = Vec::new();
    let mut lookup: HashMap<CertKey, usize> = HashMap::new();
    let mut scene_groups: Vec<Vec<(usize, Vec<usize>)>> = vec![Vec::new(); banks.len()];
    let mut selected_report = Vec::new();
    for g in &selected {
        let c = &certificates[g.pose];
        let ident = *lookup.entry(cert_key(c)).or_insert_with(|| {
            descriptors.push((c.clone(), bits.len() * 32));
            bits.extend(&c.words);
            descriptors.len() - 1
        });
        let mut members = g.members.clone();
        members.resize(4, NO_MEMBER);
        scene_groups[g.scene].push((ident, members));
        selected_report.push(Json::Obj(vec![
            ("scene".into(), Json::Int(g.scene as i64)),
            ("members".into(), int_list(g.members.iter().map(|&m| m as i64))),
            ("pose".into(), Json::Int(g.pose as i64)),
            ("regions".into(), int_list(g.regions.iter().copied())),
            ("certificate".into(), Json::Int(ident as i64)),
        ]));
    }
    let actual = 24 + 8 * scene_groups.len() + 10 * selected.len() + 12 * descriptors.len() + 4 * bits.len();
    if actual != total || actual > MAX_TABLE_BYTES {
        return err("Opaque groups: budget accounting differs");
    }
    let capacities: Vec<i64> = banks.iter().map(|b| b["pools"]["draws"].as_i64().unwrap_or(0)).collect();
    let pool_capacity = capacities.iter().copied().max().unwrap_or(0);
    let mut rust = String::from("// Generated optional4px/19x19 seam proofs. Local licensed data.\n");
    rust += &format!("pub const GROUP_POOL_CAPACITY:usize={pool_capacity};\n");
    rust += &format!("pub static GROUP_CERTS:&[GroupCert]=&[\n{}];\n", cert_rows("GroupCert", &descriptors));
    rust += &format!("pub static GROUP_BITS:&[u32]=&[\n{}];\n", bit_rows(&bits));
    rust += "pub static SCENE_GROUPS:&[&[Group]]=&[\n";
    for row in &scene_groups {
        rust += "&[\n";
        for (c, members) in row {
            rust += &format!("Group{{certificate:{c},members:[{}]}},\n", members.iter().map(usize::to_string).collect::<Vec<_>>().join(","));
        }
        rust += "],\n";
    }
    rust += "];\n";
    let destination = root.join("data/opaque_groups.rs");
    write_changed(&destination, rust.as_bytes())?;
    let maximum = counts.iter().map(|c| c.1).max().unwrap_or(0);
    let cert_json = |(c, offset): &(Cert, usize)| Json::Obj(vec![
        ("gx".into(), Json::Int(c.gx.into())),
        ("gy".into(), Json::Int(c.gy.into())),
        ("width".into(), Json::Int(c.width.into())),
        ("height".into(), Json::Int(c.height.into())),
        ("offset".into(), Json::Int(*offset as i64)),
    ]);
    let report = Json::Obj(vec![
        ("format".into(), Json::Str("HKOPAQUEGROUPS01".into())),
        ("input_key".into(), Json::Str(key)),
        ("input_sha256".into(), Json::Obj(inputs.into_iter().map(|(k, v)| (k, Json::Str(v))).collect())),
        ("helper".into(), helper),
        ("native_input_sha256".into(), Json::Str(sha(&payload))),
        ("native_output_sha256".into(), Json::Str(sha(&output))),
        ("candidate_groups".into(), Json::Int(candidate_count as i64)),
        ("pose_count".into(), Json::Int(poses.len() as i64)),
        ("nonempty_poses".into(), Json::Int(certificates.iter().filter(|c| c.width != 0).count() as i64)),
        ("selected_groups".into(), Json::Int(selected.len() as i64)),
        ("pool_capacity".into(), Json::Int(pool_capacity)),
        ("bank_pool_capacities".into(), int_list(capacities)),
        ("certificate_count".into(), Json::Int(descriptors.len() as i64)),
        ("table_bytes".into(), Json::Int(actual as i64)),
        ("bitmap_bytes".into(), Json::Int(4 * bits.len() as i64)),
        ("descriptor_bytes".into(), Json::Int(12 * descriptors.len() as i64)),
        ("membership_bytes".into(), Json::Int(10 * selected.len() as i64)),
        ("budget".into(), Json::Int(MAX_TABLE_BYTES as i64)),
        ("maximum_region_groups".into(), Json::Int(maximum as i64)),
        ("region_counts".into(), Json::Obj(counts.iter().map(|&(r, n)| (r.to_string(), Json::Int(n as i64))).collect())),
        ("selection".into(), Json::Str("Stable greedy new seam-bit count times resident region count per marginal table byte; whole groups, shared identical bitmaps,8KiB and32 groups/region caps. Optional incomplete culling coverage.".into())),
        ("certificates".into(), Json::List(descriptors.iter().map(cert_json).collect())),
        ("selected".into(), Json::List(selected_report)),
        ("scene_groups".into(), Json::List(scene_groups.iter().map(|row| Json::List(row.iter().map(|(c, m)| Json::Obj(vec![
            ("certificate".into(), Json::Int(*c as i64)),
            ("members".into(), int_list(m.iter().map(|&x| x as i64))),
        ])).collect())).collect())),
        ("poses".into(), Json::List(poses.iter().map(|(relative, phases, members)| Json::Obj(vec![
            ("relative_q8".into(), int_list(relative.iter().copied())),
            ("members".into(), Json::Int(*members as i64)),
            ("phase_count".into(), Json::Int(phases.len() as i64)),
        ])).collect())),
        ("proof".into(), Json::Str("Union original flat quad raster for each exact common camera phase, intersect phases, erode19x19 on4px grid; remove windows covered by any single member in every phase. All members static immutable opaque Tilemaps with identical scale and legal combined span.".into())),
        ("outputs".into(), Json::Obj(vec![("data/opaque_groups.rs".into(), Json::Str(sha(rust.as_bytes())))])),
    ]);
    write_changed(&report_path, (dumps(&report) + "\n").as_bytes())?;
    println!("Opaque groups: {}/{candidate_count} groups, {actual}/{MAX_TABLE_BYTES} bytes, {maximum} maximum region groups", selected.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(x0: i64, x1: i64, scale: i32) -> Record {
        Record { xy: [x0, 0, x1, 0, x0, 4096, x1, 4096], scale, texture: 0 }
    }
    #[test]
    fn touching_quads_of_one_scale_group_and_others_do_not() {
        let records: BTreeMap<usize, Record> = [(1, record(0, 4096, 1)), (2, record(4096, 8192, 1)), (3, record(8192, 12288, 2)), (4, record(8192, 8500, 1))].into();
        // 1 touches 2, 2 touches 4 (same scale); 3 is another scale.
        assert_eq!(connected_groups(&records), vec![vec![1, 2], vec![2, 4], vec![1, 2, 4]]);
    }
    #[test]
    fn an_axis_quad_is_recognised() {
        assert!(axis(&[0, 0, 10, 0, 0, 5, 10, 5]));
        assert!(!axis(&[0, 0, 10, 1, 0, 5, 10, 5]));
    }
    #[test]
    fn group_phases_refuse_a_lone_quad() {
        assert!(group_phases(&[0; 8]).is_err());
        assert_eq!(group_phases(&[0; 16]).unwrap().len(), 1);
    }
    #[test]
    fn selection_prefers_area_per_byte_then_lower_ids_and_shares_bitmaps() {
        let cert = |words: Vec<u32>| Cert { gx: 0, gy: 0, width: 32, height: words.len() as u16, words };
        let certificates = vec![cert(vec![u32::MAX]), cert(vec![1]), cert(vec![u32::MAX])];
        let candidates = vec![
            Candidate { scene: 0, members: vec![5, 6], pose: 1, regions: vec![1] },
            Candidate { scene: 0, members: vec![3, 4], pose: 0, regions: vec![1] },
            Candidate { scene: 1, members: vec![1, 2], pose: 0, regions: vec![2] },
        ];
        let (selected, used, counts) = select_groups(candidates, &certificates, 2, MAX_TABLE_BYTES, MAX_REGION_GROUPS);
        // Pose 0's bitmap is admitted once and reused at 10 bytes.
        assert_eq!(selected.iter().map(|g| (g.scene, g.members.clone())).collect::<Vec<_>>(), vec![(0, vec![3, 4]), (1, vec![1, 2]), (0, vec![5, 6])]);
        assert_eq!(used, 24 + 16 + (10 + 16) + 10 + (10 + 16));
        assert_eq!(counts, vec![(1, 2), (2, 1)]);
    }

    // The cases of tests/test_opaque_groups.py.
    struct Rng(u64);
    impl Rng {
        fn range(&mut self, lo: i64, hi: i64) -> i64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            lo + (self.0 % (hi - lo) as u64) as i64
        }
    }
    #[test]
    fn joint_camera_phases_cover_fractional_member_offsets() {
        let mut rng = Rng(918);
        for count in [2, 3, 4] {
            for _ in 0..10 {
                let mut xy = Vec::new();
                for _ in 0..count {
                    let (x, y, w, h) = (rng.range(-30000, 30000), rng.range(-30000, 30000), rng.range(1, 10000), rng.range(1, 10000));
                    xy.extend([x, y, x + w, y, x, y + h, x + w, y + h]);
                }
                let variants: HashSet<Vec<(i64, i64)>> = group_phases(&xy).unwrap().into_iter().collect();
                assert!(variants.len() <= 256);
                for _ in 0..100 {
                    let (cx, cy) = (rng.range(-1000000, 1000000), rng.range(-1000000, 1000000));
                    let v: Vec<(i64, i64)> = (0..xy.len()).step_by(2).map(|i| ((xy[i] - cx) >> 8, -((xy[i + 1] - cy) >> 8))).collect();
                    let relative: Vec<(i64, i64)> = v.iter().map(|&(x, y)| (x - v[0].0, y - v[0].1)).collect();
                    assert!(variants.contains(&relative));
                }
            }
        }
        assert!(group_phases(&[0; 8]).is_err());
    }
    #[test]
    fn connected_members_require_common_scale_and_bounded_extent() {
        let boxed = |x: i64, w: i64, scale: i32| Record { xy: [x * 256, 0, (x + w) * 256, 0, x * 256, 48 * 256, (x + w) * 256, 48 * 256], scale, texture: 0 };
        let group = |records: Vec<(usize, Record)>| connected_groups(&records.into_iter().collect());
        assert_eq!(group(vec![(3, boxed(0, 16, 60693)), (7, boxed(16, 16, 60693)), (9, boxed(100, 16, 2))]), vec![vec![3, 7]]);
        assert!(group(vec![(3, boxed(0, 704, 60693)), (7, boxed(16, 16, 60693))]).is_empty());
        assert!(group(vec![(3, boxed(0, 16, 60693)), (7, boxed(17, 16, 60693))]).is_empty());
    }
    #[test]
    fn only_exact_resident_sentinel_qualifies() {
        let mut raw = vec![0u8; 32832];
        let bank = serde_json::json!({"sections": {"textures": {"offset": 0}, "palettes": {"offset": 16}, "pages": {"offset": 64}}});
        for (k, v) in [0u16, 0, 0, 2, 2, 0].into_iter().enumerate() {
            raw[2 * k..2 * k + 2].copy_from_slice(&v.to_le_bytes());
        }
        raw[64] = 0x11;
        raw[64 + 128] = 0x11;
        for (word, want) in [(1u16, true), (0, false), (0x8001, false), (0x8000, false), (2, false)] {
            raw[18..20].copy_from_slice(&word.to_le_bytes());
            assert_eq!(solid_word_one(&raw, &bank, 0).unwrap(), want, "word {word:#x}");
        }
        raw[0..2].copy_from_slice(&65535u16.to_le_bytes());
        assert!(!solid_word_one(&raw, &bank, 0).unwrap());
    }
    #[test]
    fn budget_admits_atomic_groups_and_shares_identical_proofs() {
        let c = Cert { gx: 0, gy: 0, width: 1, height: 1, words: vec![1] };
        let groups = || -> Vec<Candidate> { (0..2).map(|i| Candidate { scene: 0, members: vec![i * 2, i * 2 + 1], pose: 0, regions: vec![i as i64 + 1] }).collect() };
        let certs = [c];
        let (selected, size, _) = select_groups(groups(), &certs, 1, 68, MAX_REGION_GROUPS);
        assert_eq!((selected.len(), size), (2, 68));
        let (selected, size, _) = select_groups(groups(), &certs, 1, 58, MAX_REGION_GROUPS);
        assert_eq!((selected.len(), size), (1, 58));
        let same: Vec<Candidate> = groups().into_iter().map(|g| Candidate { regions: vec![1], ..g }).collect();
        assert_eq!(select_groups(same, &certs, 1, 100, 1).0.len(), 1);
        assert!(select_groups(groups(), &certs, 1, 57, MAX_REGION_GROUPS).0.is_empty());
    }
}

// tests/test_opaque_groups.py also built the guest's own membership guards
// (game/src/opaque_groups.rs) and ran their tests; this keeps them running.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../game/src/opaque_groups.rs"]
mod guest_membership;
