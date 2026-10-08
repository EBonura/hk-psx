//! host/scene_certificates.py in Rust: relocate the exact coverage proofs into
//! independently resident per-scene HKOCSC01 bundles.
//!
//! No rasterization, new group selection, geometry changes or disc writes. The
//! generated tables (data/opaque_tiles.rs, data/opaque_groups.rs) stay a
//! hash-bound oracle: they are parsed back and checked against their reports
//! before any proof moves. The outputs match the Python's byte for byte except
//! the report's `code_sha256`, which hashed the Python.

use crate::break_effects::{compressed, fnv};
use crate::common::{err, Result};
use crate::opaque_tiles::{file_sha, int, list, read, read_json, room_desc, section, sha, text, u16_at};
use crate::pyjson::{dumps, Json};
use serde_json::Value as J;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"HKOCSC01";
/// What a proof may not outgrow. This is the cook's refusal, not the guest's
/// reservation: `reserved_arena` sizes the linked buffer from the proofs the
/// cook actually wrote, so slack under the ceiling costs no RAM.
pub const ARENA_CEILING: usize = 83_700;
const HEADER_BYTES: usize = 80;
const NO_CERT: usize = 65535;
const STRIDES: [usize; 6] = [12, 4, 2, 12, 4, 10];

fn aligned(n: usize) -> usize {
    (n + 3) & !3
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Desc {
    gx: i64,
    gy: i64,
    width: i64,
    height: i64,
    offset: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    certificate: usize,
    members: Vec<usize>,
}
/// One generated table, read back: its descriptors, bitmap words, and per bank
/// either the pool map (tiles) or the selected groups.
struct Legacy {
    certificates: Vec<Desc>,
    bits: Vec<u32>,
    maps: Vec<Vec<usize>>,
    groups: Vec<Vec<Group>>,
}

/// `_numbers`: only decimal or hex integers, commas and whitespace.
fn numbers(text: &str) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for token in text.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()) {
        let v = match token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
            Some(hex) if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) => i64::from_str_radix(hex, 16).ok(),
            None if token.chars().all(|c| c.is_ascii_digit()) => token.parse().ok(),
            _ => None,
        };
        out.push(v.ok_or("Invalid generated integer table")?);
    }
    Ok(out)
}
/// `_array`: the body of `pub static NAME:<type>=&[\n<body>\n];`.
fn array<'a>(text: &'a str, name: &str) -> Result<&'a str> {
    let missing = || format!("Missing generated table {name}");
    let start = text.find(&format!("pub static {name}:")).ok_or_else(missing)?;
    let rest = &text[start..];
    let open = rest.find('=').ok_or_else(missing)?;
    let rest = rest[open..].strip_prefix("=&[\n").ok_or_else(missing)?;
    let end = rest.find("\n];").ok_or_else(missing)?;
    Ok(&rest[..end])
}
/// One `Kind{gx:..,gy:..,width:..,height:..,offset:..},` row.
fn desc_row(line: &str, kind: &str) -> Option<Desc> {
    let inner = line.strip_prefix(kind)?.strip_prefix('{')?.strip_suffix("},")?;
    let mut values = [0i64; 5];
    for (v, (field, part)) in values.iter_mut().zip(["gx", "gy", "width", "height", "offset"].iter().zip(inner.split(','))) {
        let n = part.strip_prefix(field)?.strip_prefix(':')?;
        let negative_ok = *field == "gx" || *field == "gy";
        let digits = if negative_ok { n.strip_prefix('-').unwrap_or(n) } else { n };
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        *v = n.parse().ok()?;
    }
    (inner.split(',').count() == 5).then_some(Desc { gx: values[0], gy: values[1], width: values[2], height: values[3], offset: values[4] })
}
fn report_desc(v: &J) -> Result<Desc> {
    Ok(Desc { gx: int(v, "gx")?, gy: int(v, "gy")?, width: int(v, "width")?, height: int(v, "height")?, offset: int(v, "offset")? })
}

/// `read_legacy`: the report's metadata against the generated Rust, then its bits.
fn read_legacy(report: &J, text: &str, groups: bool) -> Result<Legacy> {
    let (prefix, name, kind) = if groups { ("GROUP", "GROUP_CERTS", "GroupCert") } else { ("TILE_CERT", "TILE_CERTS", "TileCert") };
    let body = array(text, name)?;
    let certificates = body.split('\n').map(|line| desc_row(line, kind)).collect::<Option<Vec<_>>>();
    let reported = list(report, "certificates")?.iter().map(report_desc).collect::<Result<Vec<_>>>()?;
    match certificates {
        Some(c) if c == reported => {}
        _ => return err("Legacy descriptor report differs from Rust"),
    }
    let bits: Vec<u32> = numbers(array(text, &format!("{prefix}_BITS"))?)?.into_iter().map(|v| v as u32).collect();
    if (bits.len() * 4) as i64 != int(report, "bitmap_bytes")? {
        return err("Legacy bitmap count differs");
    }
    let mut legacy = Legacy { certificates: reported, bits, maps: Vec::new(), groups: Vec::new() };
    if groups {
        let body = array(text, "SCENE_GROUPS")?;
        // Each bank is `&[\n<rows>],` with rows `Group{certificate:N,members:[a,b,c,d]},\n`.
        let mut rest = body;
        while !rest.trim().is_empty() {
            let open = rest.find("&[\n").filter(|&i| rest[..i].trim().is_empty()).ok_or("Invalid legacy group records")?;
            let after = &rest[open + 3..];
            let close = after.find("],").ok_or("Invalid legacy group records")?;
            // `members:[..]` holds a `]` too: the bank ends at the first `],`
            // that closes a line of its own.
            let close = after.match_indices("],").map(|(i, _)| i).find(|&i| i == 0 || after[..i].ends_with('\n')).unwrap_or(close);
            let mut row = Vec::new();
            for line in after[..close].lines().filter(|l| !l.is_empty()) {
                let inner = line.strip_prefix("Group{certificate:").and_then(|l| l.strip_suffix("]},")).ok_or("Invalid legacy group records")?;
                let (c, members) = inner.split_once(",members:[").ok_or("Invalid legacy group records")?;
                if !c.chars().all(|ch| ch.is_ascii_digit()) || c.is_empty() || !members.chars().all(|ch| ch.is_ascii_digit() || ch == ',') {
                    return err("Invalid legacy group records");
                }
                row.push(Group { certificate: c.parse().map_err(|_| "Invalid legacy group records")?, members: numbers(members)?.into_iter().map(|v| v as usize).collect() });
            }
            legacy.groups.push(row);
            rest = &after[close + 2..];
        }
        let reported: Vec<Vec<Group>> = list(report, "scene_groups")?.iter().map(|row| {
            row.as_array().ok_or("scene_groups row").map(|r| r.iter().map(|g| Group {
                certificate: g["certificate"].as_u64().unwrap_or(u64::MAX) as usize,
                members: g["members"].as_array().map(|m| m.iter().map(|x| x.as_u64().unwrap_or(u64::MAX) as usize).collect()).unwrap_or_default(),
            }).collect())
        }).collect::<std::result::Result<_, _>>()?;
        if legacy.groups != reported {
            return err("Legacy group memberships differ");
        }
    } else {
        let body = array(text, "SCENE_DRAW_CERTS")?;
        for line in body.split('\n') {
            let inner = line.strip_prefix("&[").and_then(|l| l.strip_suffix("],")).ok_or("Legacy pool maps differ")?;
            legacy.maps.push(numbers(inner)?.into_iter().map(|v| v as usize).collect());
        }
        let reported: Vec<Vec<usize>> = list(report, "bank_bindings")?.iter()
            .map(|row| row.as_array().map(|r| r.iter().map(|x| x.as_u64().unwrap_or(u64::MAX) as usize).collect()).unwrap_or_default())
            .collect();
        if legacy.maps != reported {
            return err("Legacy pool maps differ");
        }
        let shift = text.find("pub const TILE_CERT_SHIFT:u32=").and_then(|i| {
            let v = &text[i + "pub const TILE_CERT_SHIFT:u32=".len()..];
            v[..v.find(';')?].parse::<i64>().ok()
        });
        if shift.is_none() || shift != Some(int(report, "grid_shift")?) || int(report, "grid_shift")? != 2 {
            return err("Unsupported or stale certificate grid");
        }
    }
    Ok(legacy)
}

type Shape = ((i64, i64, i64, i64), Vec<u32>);
fn shape(cert: &Desc, bits: &[u32]) -> Result<Shape> {
    let Desc { gx, gy, width: w, height: h, offset } = *cert;
    if !((-32768..=32767).contains(&gx) && (-32768..=32767).contains(&gy) && (1..=65535).contains(&w) && (1..=65535).contains(&h)) || offset % 32 != 0 {
        return err("Invalid certificate descriptor");
    }
    let n = ((w * h + 31) / 32) as usize;
    let start = (offset / 32) as usize;
    if start + n > bits.len() {
        return err("Certificate bitmap outside table");
    }
    let words = bits[start..start + n].to_vec();
    if words.iter().all(|&v| v == 0) {
        return err("Empty/invalid certificate bitmap");
    }
    if (w * h) % 32 != 0 && words[n - 1] >> ((w * h) % 32) != 0 {
        return err("Nonzero unused certificate bits");
    }
    Ok(((gx, gy, w, h), words))
}
fn relocate(certificates: &[Desc], bits: &[u32], ids: impl IntoIterator<Item = usize>) -> Result<(Vec<Desc>, Vec<u32>, HashMap<usize, usize>)> {
    let ids: Vec<usize> = ids.into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    if ids.len() > NO_CERT || ids.iter().any(|&c| c >= certificates.len()) {
        return err("Invalid legacy certificate ID");
    }
    let mut descriptors = Vec::new();
    let mut out = Vec::new();
    for &old in &ids {
        let ((gx, gy, width, height), words) = shape(&certificates[old], bits)?;
        descriptors.push(Desc { gx, gy, width, height, offset: (out.len() * 32) as i64 });
        out.extend(words);
    }
    Ok((descriptors, out, ids.into_iter().enumerate().map(|(new, old)| (old, new)).collect()))
}

/// The proofs one scene needs, in its own pool's terms.
struct Owner<'a> {
    certificates: &'a [Desc],
    bits: &'a [u32],
    map: &'a [usize],
    group_certificates: &'a [Desc],
    group_bits: &'a [u32],
    groups: &'a [Group],
}
fn pack_descs(descs: &[Desc]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in descs {
        out.extend((c.gx as i16).to_le_bytes());
        out.extend((c.gy as i16).to_le_bytes());
        out.extend((c.width as u16).to_le_bytes());
        out.extend((c.height as u16).to_le_bytes());
        out.extend((c.offset as u32).to_le_bytes());
    }
    out
}
fn put_u32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn build_bundle(scene_id: u32, scene_raw_fnv: u32, atlas_fnv: u32, pool_count: usize, owner: &Owner) -> Result<(Vec<u8>, Json)> {
    if !(1..=4096).contains(&pool_count) || owner.map.len() != pool_count {
        return err("Scene draw pool size mismatch");
    }
    let (tc, tb, tm) = relocate(owner.certificates, owner.bits, owner.map.iter().copied().filter(|&i| i != NO_CERT))?;
    let (gc, gb, gm) = relocate(owner.group_certificates, owner.group_bits, owner.groups.iter().map(|g| g.certificate))?;
    let tile_map: Vec<u16> = owner.map.iter().map(|&old| if old == NO_CERT { NO_CERT as u16 } else { tm[&old] as u16 }).collect();
    let mut groups = Vec::new();
    for g in owner.groups {
        groups.extend((gm[&g.certificate] as u16).to_le_bytes());
        for &m in &g.members {
            groups.extend((m as u16).to_le_bytes());
        }
    }
    let sections: [Vec<u8>; 6] = [
        pack_descs(&tc),
        tb.iter().flat_map(|w| w.to_le_bytes()).collect(),
        tile_map.iter().flat_map(|w| w.to_le_bytes()).collect(),
        pack_descs(&gc),
        gb.iter().flat_map(|w| w.to_le_bytes()).collect(),
        groups,
    ];
    let mut out = vec![0u8; HEADER_BYTES];
    out[..8].copy_from_slice(MAGIC);
    for (k, v) in [scene_id, scene_raw_fnv, atlas_fnv, pool_count as u32, 2, 0].into_iter().enumerate() {
        put_u32(&mut out, 8 + 4 * k, v);
    }
    for (index, (payload, stride)) in sections.iter().zip(STRIDES).enumerate() {
        out.resize(aligned(out.len()), 0);
        let (at, count) = (out.len() as u32, (payload.len() / stride) as u32);
        put_u32(&mut out, 32 + index * 8, at);
        put_u32(&mut out, 36 + index * 8, count);
        out.extend(payload);
    }
    out.resize(aligned(out.len()), 0);
    if out.len() > ARENA_CEILING {
        return err("Per-scene coverage exceeds fixed auxiliary arena");
    }
    let proof = verify_equivalence(&out, owner)?;
    Ok((out, proof))
}

struct Parsed {
    tile_certs: Vec<Desc>,
    tile_bits: Vec<u32>,
    map: Vec<usize>,
    group_certs: Vec<Desc>,
    group_bits: Vec<u32>,
    groups: Vec<Group>,
}
fn u32s(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}
fn parse_bundle(raw: &[u8]) -> Result<Parsed> {
    if raw.len() < HEADER_BYTES || raw.len() > ARENA_CEILING || &raw[..8] != MAGIC {
        return err("Invalid coverage header");
    }
    let header = u32s(&raw[8..32]);
    let (pools, shift, reserved) = (header[3] as usize, header[4], header[5]);
    if !(1..=4096).contains(&pools) || shift != 2 || reserved != 0 {
        return err("Invalid coverage configuration");
    }
    let mut sections = Vec::new();
    let mut end = HEADER_BYTES;
    for (index, stride) in STRIDES.into_iter().enumerate() {
        let entry = u32s(&raw[32 + index * 8..40 + index * 8]);
        let (offset, count) = (entry[0] as usize, entry[1] as usize);
        if offset != aligned(end) || offset > raw.len() || raw[end..offset].iter().any(|&b| b != 0) || count > (raw.len() - offset) / stride {
            return err("Invalid coverage section extent");
        }
        end = offset + count * stride;
        sections.push(&raw[offset..end]);
    }
    if raw.len() != aligned(end) || raw[end..].iter().any(|&b| b != 0) {
        return err("Nonzero padding/trailing coverage data");
    }
    let certs = |data: &[u8]| -> Vec<Desc> {
        data.chunks_exact(12).map(|r| Desc {
            gx: i16::from_le_bytes([r[0], r[1]]).into(),
            gy: i16::from_le_bytes([r[2], r[3]]).into(),
            width: u16::from_le_bytes([r[4], r[5]]).into(),
            height: u16::from_le_bytes([r[6], r[7]]).into(),
            offset: u32::from_le_bytes([r[8], r[9], r[10], r[11]]).into(),
        }).collect()
    };
    let p = Parsed {
        tile_certs: certs(sections[0]),
        tile_bits: u32s(sections[1]),
        map: sections[2].chunks_exact(2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]]))).collect(),
        group_certs: certs(sections[3]),
        group_bits: u32s(sections[4]),
        groups: sections[5].chunks_exact(10).map(|r| {
            let v: Vec<usize> = r.chunks_exact(2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]]))).collect();
            Group { certificate: v[0], members: v[1..].to_vec() }
        }).collect(),
    };
    if p.map.len() != pools || p.tile_certs.len() > NO_CERT || p.group_certs.len() > NO_CERT {
        return err("Invalid coverage map/ID count");
    }
    for (descriptors, bits) in [(&p.tile_certs, &p.tile_bits), (&p.group_certs, &p.group_bits)] {
        let mut next_bit = 0;
        for c in descriptors {
            let (_, words) = shape(c, bits)?;
            if c.offset != next_bit {
                return err("Noncontiguous certificate bitmap records");
            }
            next_bit += (words.len() * 32) as i64;
        }
        if next_bit != (bits.len() * 32) as i64 {
            return err("Unused certificate bitmap records");
        }
    }
    if p.map.iter().any(|&c| c != NO_CERT && c >= p.tile_certs.len()) {
        return err("Invalid mapped certificate");
    }
    let mut previous: Option<Vec<usize>> = None;
    for g in &p.groups {
        let live: Vec<usize> = g.members.iter().copied().filter(|&i| i != NO_CERT).collect();
        let mut padded = live.clone();
        padded.resize(4, NO_CERT);
        let mut sorted = live.clone();
        sorted.sort();
        sorted.dedup();
        if g.certificate >= p.group_certs.len() || !(2..=4).contains(&live.len()) || g.members != padded || live != sorted || live.iter().any(|&m| m >= pools) {
            return err("Invalid coverage group binding");
        }
        if previous.as_ref().is_some_and(|prev| live <= *prev) {
            return err("Unsorted/duplicate coverage groups");
        }
        previous = Some(live);
    }
    Ok(p)
}

/// Resolve every original pool binding and member against the relocated words.
fn verify_equivalence(raw: &[u8], owner: &Owner) -> Result<Json> {
    let parsed = parse_bundle(raw)?;
    if owner.map.len() != parsed.map.len() {
        return err("Relocated map length changed");
    }
    let mut bits_compared = 0i64;
    for (&old, &new) in owner.map.iter().zip(&parsed.map) {
        if (old == NO_CERT) != (new == NO_CERT) {
            return err("Relocation changed certificate eligibility");
        }
        if old == NO_CERT {
            continue;
        }
        let a = shape(&owner.certificates[old], owner.bits)?;
        if a != shape(&parsed.tile_certs[new], &parsed.tile_bits)? {
            return err("Relocation changed tile coverage");
        }
        bits_compared += a.0 .2 * a.0 .3;
    }
    if owner.groups.len() != parsed.groups.len() {
        return err("Relocation changed group selection");
    }
    for (a, b) in owner.groups.iter().zip(&parsed.groups) {
        if a.members != b.members {
            return err("Relocation changed group members/order");
        }
        let old = shape(&owner.group_certificates[a.certificate], owner.group_bits)?;
        if old != shape(&parsed.group_certs[b.certificate], &parsed.group_bits)? {
            return err("Relocation changed seam coverage");
        }
        bits_compared += old.0 .2 * old.0 .3;
    }
    Ok(Json::Obj(vec![
        ("status".into(), Json::Str("PASS".into())),
        ("pool_bindings".into(), Json::Int(owner.map.len() as i64)),
        ("groups".into(), Json::Int(owner.groups.len() as i64)),
        ("resolved_bits_compared".into(), Json::Int(bits_compared)),
    ]))
}

/// Check the exact serialized pool references, repeated local pool IDs
/// included, against the tile report's region bindings.
fn verify_region_bindings(raw: &[u8], scene: &J, scene_bytes: &[u8], owner: &Owner, expected: &[J]) -> Result<Json> {
    let new = parse_bundle(raw)?;
    let rooms = section(scene, "rooms")?;
    let mut rows = Vec::new();
    for (local, source) in list(scene, "source_rooms")?.iter().enumerate() {
        let desc = room_desc(scene_bytes, rooms, local)?;
        let (chunk, count) = (desc[0] as usize, desc[1] as usize);
        let counts = list(source, "counts")?;
        if chunk as i64 != int(source, "chunk_id")? || Some(count as i64) != counts.get(2).and_then(J::as_i64) {
            return err("Scene region draw identity differs");
        }
        let pools = (0..count).map(|d| u16_at(scene_bytes, desc[5] as usize + 2 * d).map(usize::from)).collect::<Result<Vec<_>>>()?;
        if pools.iter().any(|&p| p >= owner.map.len()) {
            return err("Region draw reference exceeds coverage map");
        }
        let old: BTreeMap<usize, usize> = pools.iter().enumerate().filter(|(_, &p)| owner.map[p] != NO_CERT).map(|(d, &p)| (d, owner.map[p])).collect();
        let want: BTreeMap<usize, usize> = expected.get(chunk - 1).and_then(J::as_array).ok_or("Legacy region certificate resolution differs")?.iter()
            .map(|pair| (pair[0].as_u64().unwrap_or(u64::MAX) as usize, pair[1].as_u64().unwrap_or(u64::MAX) as usize)).collect();
        if old != want {
            return err("Legacy region certificate resolution differs");
        }
        for &p in &pools {
            let (before, after) = (owner.map[p], new.map[p]);
            if before == NO_CERT {
                if after != NO_CERT {
                    return err("Region certificate eligibility changed");
                }
            } else if shape(&owner.certificates[before], owner.bits)? != shape(&new.tile_certs[after], &new.tile_bits)? {
                return err("Region certificate resolution changed");
            }
        }
        let mut inverse: HashMap<usize, usize> = HashMap::new();
        for (d, &p) in pools.iter().enumerate() {
            inverse.entry(p).or_insert(d);
        }
        let resolve = |groups: &[Group]| -> Vec<Vec<usize>> {
            groups.iter().filter_map(|g| {
                let members: Vec<usize> = g.members.iter().copied().filter(|&p| p != NO_CERT).collect();
                members.iter().all(|p| inverse.contains_key(p)).then(|| members.iter().map(|p| inverse[p]).collect())
            }).collect()
        };
        let old_groups = resolve(owner.groups);
        if old_groups != resolve(&new.groups) {
            return err("Region group local resolution changed");
        }
        rows.push(Json::Obj(vec![
            ("chunk_id".into(), Json::Int(chunk as i64)),
            ("local_draws".into(), Json::Int(count as i64)),
            ("certified_draws".into(), Json::Int(old.len() as i64)),
            ("resolved_groups".into(), Json::Int(old_groups.len() as i64)),
        ]));
    }
    Ok(Json::List(rows))
}

fn read_bound(root: &Path, path: &str, digest: &str) -> Result<Vec<u8>> {
    let data = read(&root.join(path))?;
    if sha(&data) != digest {
        return err(format!("Stale coverage input: {}", root.join(path).display()));
    }
    Ok(data)
}
fn atlas_fingerprint(atlases: &[&J]) -> Result<u32> {
    let mut data = Vec::new();
    for a in atlases {
        for k in ["kind", "first", "count", "raw_len", "raw_fnv"] {
            data.extend((int(a, k)? as u32).to_le_bytes());
        }
    }
    Ok(fnv(&data))
}
/// Checkout-relative when inside it (host/source.py `rel`).
fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()
}

struct Bundle {
    scene_id: i64,
    scene_raw_fnv: i64,
    atlas_fnv: u32,
    draw_pool_count: usize,
    chunk_id: usize,
    raw_len: usize,
    raw_fnv: u32,
    stored_len: usize,
    stored_fnv: u32,
}
/// Linked bytes the guest reserves for the admitted scene's proof: the
/// largest proof the cook wrote, four-byte aligned, rather than the ceiling.
fn reserved_arena(bundles: &[Bundle]) -> usize {
    bundles.iter().map(|b| aligned(b.raw_len)).max().unwrap_or(0)
}
fn rust_manifest(bundles: &[Bundle], pool_capacity: i64) -> String {
    let fields = [
        ("scene_id", "u32"), ("scene_raw_fnv", "u32"), ("atlas_fnv", "u32"), ("draw_pool_count", "u32"), ("chunk_id", "usize"),
        ("raw_len", "usize"), ("raw_fnv", "u32"), ("stored_len", "usize"), ("stored_fnv", "u32"),
    ];
    let mut text = String::from("// Exact scene-local coverage relocation; generated from licensed local inputs.\n");
    text += &format!("#[derive(Clone,Copy,Debug)]\npub struct CoverageDesc{{{}}}\n", fields.iter().map(|(n, t)| format!("pub {n}:{t}")).collect::<Vec<_>>().join(","));
    text += &format!("pub const COVERAGE_ARENA_BYTES:usize={};\npub const COVERAGE_GRID_SHIFT:u32=2;\npub const COVERAGE_GROUP_POOL_CAPACITY:usize={pool_capacity};\n", reserved_arena(bundles));
    text += "pub const COVERAGE_MANIFEST:&[CoverageDesc]=&[\n";
    for b in bundles {
        let values = [
            b.scene_id.to_string(), b.scene_raw_fnv.to_string(), b.atlas_fnv.to_string(), b.draw_pool_count.to_string(), b.chunk_id.to_string(),
            b.raw_len.to_string(), b.raw_fnv.to_string(), b.stored_len.to_string(), b.stored_fnv.to_string(),
        ];
        text += &format!("CoverageDesc{{{}}},\n", fields.iter().zip(values).map(|((n, _), v)| format!("{n}:{v}")).collect::<Vec<_>>().join(","));
    }
    text += "];\n";
    text
}

/// Run what `python3 host/scene_certificates.py [--output DIR] [--report PATH] [--manifest PATH]` runs.
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let option = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let (arena, bundles) = cook(root, option("--output"), option("--report"), option("--manifest"))?;
    println!("Scene certificates: {bundles} bundles, arena {arena} of {ARENA_CEILING} bytes");
    Ok(())
}

pub fn cook(root: &Path, output_dir: Option<PathBuf>, report_path: Option<PathBuf>, manifest_path: Option<PathBuf>) -> Result<(usize, usize)> {
    let output_dir = output_dir.unwrap_or_else(|| root.join("data/scene-coverage"));
    let report_path = report_path.unwrap_or_else(|| root.join(".hkpsx/scene-certificates.json"));
    let manifest_path = manifest_path.unwrap_or_else(|| root.join("data/scene_coverage_manifest.rs"));
    let paths = [".hkpsx/packed-scenes.json", ".hkpsx/opaque-tiles/report.json", ".hkpsx/opaque-groups/report.json", ".hkpsx/ambience.json"].map(|p| root.join(p));
    let [packed, tr, gr, ambience] = [&paths[0], &paths[1], &paths[2], &paths[3]].map(|p| read_json(p));
    let (packed, tr, gr, ambience) = (packed?, tr?, gr?, ambience?);
    if text(&packed, "residency")? != "scene_gate" {
        return err("Scene-local coverage currently requires exclusive scene residency");
    }
    // Proof provenance includes original bank and region identities, source
    // mutation exclusions and the exact generated code being replaced.
    for report in [&tr, &gr] {
        for key in ["input_sha256", "outputs"] {
            for (name, digest) in report[key].as_object().ok_or("report without hashes")? {
                read_bound(root, name, digest.as_str().unwrap_or(""))?;
            }
        }
    }
    let tiles = read_legacy(&tr, &String::from_utf8_lossy(&read(&root.join("data/opaque_tiles.rs"))?), false)?;
    let groups = read_legacy(&gr, &String::from_utf8_lossy(&read(&root.join("data/opaque_groups.rs"))?), true)?;
    let scenes = list(&packed, "scenes")?;
    if tiles.maps.len() != scenes.len() || groups.groups.len() != scenes.len() {
        return err("Coverage owner count mismatch");
    }
    let atlases = list(&packed, "atlases")?;
    let first_chunk = scenes.len() + list(&ambience, "clips")?.len() + atlases.len() + 2;
    let region_bindings = list(&tr, "region_bindings")?;
    let arena_bytes = int(&packed, "arena_bytes")? as usize;
    let mut bundles = Vec::new();
    let mut reports = Vec::new();
    let mut bodies = Vec::new();
    for (index, scene) in scenes.iter().enumerate() {
        read_bound(root, &text(scene, "resident_raw_path")?, &text(scene, "resident_raw_sha256")?)?;
        let owners: Vec<&J> = atlases.iter().filter(|a| a["scene_index"].as_u64() == Some(index as u64)).collect();
        if owners.is_empty() {
            return err("Missing atlas owner");
        }
        let owner = Owner {
            certificates: &tiles.certificates,
            bits: &tiles.bits,
            map: &tiles.maps[index],
            group_certificates: &groups.certificates,
            group_bits: &groups.bits,
            groups: &groups.groups[index],
        };
        let scene_id = int(scene, "scene_id")?;
        let scene_raw_fnv = int(scene, "resident_raw_fnv")?;
        let atlas_fnv = atlas_fingerprint(&owners)?;
        let pools = scene["pools"]["draws"].as_u64().ok_or("scene without a draw pool")? as usize;
        let (raw, proof) = build_bundle(scene_id as u32, scene_raw_fnv as u32, atlas_fnv, pools, &owner)?;
        let full_scene = read_bound(root, &text(scene, "raw_path")?, &text(scene, "raw_sha256")?)?;
        let regions = verify_region_bindings(&raw, scene, &full_scene, &owner, region_bindings)?;
        let Json::Obj(mut proof) = proof else { unreachable!() };
        proof.push(("regions".into(), regions));
        let stored = compressed(&raw);
        if aligned(raw.len()) > ARENA_CEILING || stored.len().div_ceil(2048) * 2048 > arena_bytes {
            return err("Coverage staging exceeds arena");
        }
        let path = output_dir.join(format!("scene_{scene_id}"));
        let base = rel(root, &path);
        let bundle = Bundle {
            scene_id, scene_raw_fnv, atlas_fnv, draw_pool_count: pools, chunk_id: first_chunk + index,
            raw_len: raw.len(), raw_fnv: fnv(&raw), stored_len: stored.len(), stored_fnv: fnv(&stored),
        };
        reports.push(Json::Obj(vec![
            ("scene_index".into(), Json::Int(index as i64)),
            ("scene_id".into(), Json::Int(scene_id)),
            ("scene_raw_fnv".into(), Json::Int(scene_raw_fnv)),
            ("atlas_fnv".into(), Json::Int(atlas_fnv.into())),
            ("draw_pool_count".into(), Json::Int(pools as i64)),
            ("chunk_id".into(), Json::Int(bundle.chunk_id as i64)),
            ("raw_len".into(), Json::Int(raw.len() as i64)),
            ("raw_fnv".into(), Json::Int(bundle.raw_fnv.into())),
            ("raw_sha256".into(), Json::Str(sha(&raw))),
            ("stored_len".into(), Json::Int(stored.len() as i64)),
            ("stored_fnv".into(), Json::Int(bundle.stored_fnv.into())),
            ("stored_sha256".into(), Json::Str(sha(&stored))),
            ("raw_path".into(), Json::Str(format!("{base}.hk"))),
            ("stored_path".into(), Json::Str(format!("{base}.hlzc"))),
            ("path".into(), Json::Str(format!("{base}.hlzc"))),
            ("equivalence".into(), Json::Obj(proof)),
        ]));
        bundles.push(bundle);
        bodies.push((base, raw, stored));
    }
    let pool_capacity = scenes.iter().map(|s| s["pools"]["draws"].as_i64().unwrap_or(0)).max().unwrap_or(0);
    let rust = rust_manifest(&bundles, pool_capacity);
    let report = Json::Obj(vec![
        ("format".into(), Json::Str("HKOCSC01".into())),
        ("arena_bytes".into(), Json::Int(reserved_arena(&bundles) as i64)),
        ("arena_ceiling".into(), Json::Int(ARENA_CEILING as i64)),
        ("header_bytes".into(), Json::Int(HEADER_BYTES as i64)),
        ("grid_shift".into(), Json::Int(2)),
        ("source_sha256".into(), Json::Obj(paths.iter().map(|p| Ok((p.to_string_lossy().into_owned(), Json::Str(file_sha(p)?)))).collect::<Result<Vec<_>>>()?)),
        ("code_sha256".into(), Json::Str(sha(include_bytes!("scene_certificates.rs")))),
        ("bundles".into(), Json::List(reports)),
        ("manifest_path".into(), Json::Str(manifest_path.to_string_lossy().into_owned())),
        ("manifest_sha256".into(), Json::Str(sha(rust.as_bytes()))),
        ("proof".into(), Json::Str("Every scene pool binding and selected ordered seam group resolves to identical legacy geometry, dimensions and all bitmap words; only certificate IDs and offsets relocate.".into())),
    ]);
    // Every owner is validated before any output is published.
    for dir in [Some(output_dir.as_path()), report_path.parent(), manifest_path.parent()].into_iter().flatten() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    for (base, raw, stored) in &bodies {
        for (suffix, data) in [(".hk", raw), (".hlzc", stored)] {
            let path = root.join(format!("{base}{suffix}"));
            std::fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    std::fs::write(&manifest_path, &rust).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    std::fs::write(&report_path, dumps(&report) + "\n").map_err(|e| format!("{}: {e}", report_path.display()))?;
    Ok((reserved_arena(&bundles), bundles.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_parts() -> (Vec<Desc>, Vec<u32>, Vec<usize>, Vec<Desc>, Vec<u32>, Vec<Group>) {
        let tile = vec![Desc { gx: 0, gy: 0, width: 4, height: 1, offset: 0 }, Desc { gx: 1, gy: 2, width: 2, height: 2, offset: 32 }];
        let bits = vec![0b1111, 0b0110];
        let map = vec![1, NO_CERT, 1, 0];
        let group = vec![Desc { gx: -3, gy: 4, width: 3, height: 1, offset: 0 }];
        let group_bits = vec![0b101];
        let groups = vec![Group { certificate: 0, members: vec![0, 2, NO_CERT, NO_CERT] }];
        (tile, bits, map, group, group_bits, groups)
    }
    #[test]
    fn a_bundle_relocates_and_parses_back_to_the_same_proofs() {
        let (tile, bits, map, group, group_bits, groups) = owner_parts();
        let owner = Owner { certificates: &tile, bits: &bits, map: &map, group_certificates: &group, group_bits: &group_bits, groups: &groups };
        let (raw, proof) = build_bundle(7, 1, 2, 4, &owner).unwrap();
        assert_eq!(&raw[..8], MAGIC);
        assert_eq!(raw.len() % 4, 0);
        let parsed = parse_bundle(&raw).unwrap();
        // Certificate 1 comes first after relocation only if used first; IDs sort.
        assert_eq!(parsed.map, vec![1, NO_CERT, 1, 0]);
        assert_eq!(parsed.groups, groups);
        let Json::Obj(fields) = proof else { panic!() };
        assert_eq!(fields[3], ("resolved_bits_compared".into(), Json::Int(4 + 4 + 4 + 3)));
    }
    #[test]
    fn the_generated_tables_parse_strictly() {
        let text = "pub const TILE_CERT_SHIFT:u32=2;\npub static TILE_CERTS:&[TileCert]=&[\nTileCert{gx:-1,gy:2,width:3,height:1,offset:0},\n];\npub static TILE_CERT_BITS:&[u32]=&[\n0x00000005,\n];\npub static SCENE_DRAW_CERTS:&[&[u16]]=&[\n&[0,65535],\n];\n";
        let report: J = serde_json::json!({
            "certificates": [{"gx": -1, "gy": 2, "width": 3, "height": 1, "offset": 0}],
            "bitmap_bytes": 4, "bank_bindings": [[0, 65535]], "grid_shift": 2,
        });
        let legacy = read_legacy(&report, text, false).unwrap();
        assert_eq!((legacy.bits, legacy.maps), (vec![5], vec![vec![0, 65535]]));
        assert!(read_legacy(&report, &text.replace("gx:-1", "gx:-1 "), false).is_err());
        let groups = "pub static GROUP_CERTS:&[GroupCert]=&[\nGroupCert{gx:0,gy:0,width:1,height:1,offset:0},\n];\npub static GROUP_BITS:&[u32]=&[\n0x00000001,\n];\npub static SCENE_GROUPS:&[&[Group]]=&[\n&[\nGroup{certificate:0,members:[1,2,65535,65535]},\n],\n&[\n],\n];\n";
        let report: J = serde_json::json!({
            "certificates": [{"gx": 0, "gy": 0, "width": 1, "height": 1, "offset": 0}],
            "bitmap_bytes": 4, "scene_groups": [[{"certificate": 0, "members": [1, 2, 65535, 65535]}], []],
        });
        let legacy = read_legacy(&report, groups, true).unwrap();
        assert_eq!(legacy.groups, vec![vec![Group { certificate: 0, members: vec![1, 2, 65535, 65535] }], vec![]]);
    }
    #[test]
    fn numbers_take_hex_and_decimal_only() {
        assert_eq!(numbers("0x0000000a, 7,\n").unwrap(), vec![10, 7]);
        assert!(numbers("7a").is_err());
    }

    // The cases of tests/test_scene_certificates.py, with its fixtures.
    fn d(gx: i64, gy: i64, width: i64, height: i64, offset: i64) -> Desc {
        Desc { gx, gy, width, height, offset }
    }
    struct Fixture {
        certs: Vec<Desc>,
        bits: Vec<u32>,
        map: Vec<usize>,
        group_certs: Vec<Desc>,
        group_bits: Vec<u32>,
        groups: Vec<Group>,
    }
    impl Fixture {
        fn new() -> Fixture {
            let certs = vec![d(-3, 4, 2, 2, 0), d(-2, 1, 3, 2, 32), d(7, -9, 1, 1, 64)];
            Fixture {
                group_certs: certs[..2].to_vec(),
                certs,
                bits: vec![0b1011, 0b101101, 1],
                map: vec![2, NO_CERT, 1, 2, 1],
                group_bits: vec![0b1011, 0b101101],
                groups: vec![
                    Group { certificate: 1, members: vec![0, 1, NO_CERT, NO_CERT] },
                    Group { certificate: 0, members: vec![0, 1, 3, NO_CERT] },
                ],
            }
        }
        fn owner(&self) -> Owner<'_> {
            Owner { certificates: &self.certs, bits: &self.bits, map: &self.map, group_certificates: &self.group_certs, group_bits: &self.group_bits, groups: &self.groups }
        }
        fn blob(&self) -> Vec<u8> {
            build_bundle(3, 123, 456, 5, &self.owner()).unwrap().0
        }
    }
    fn field(raw: &[u8], at: usize) -> usize {
        u32_at_test(raw, at) as usize
    }
    fn u32_at_test(raw: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
    }
    fn poke(raw: &[u8], at: usize, bytes: &[u8]) -> Vec<u8> {
        let mut bad = raw.to_vec();
        bad[at..at + bytes.len()].copy_from_slice(bytes);
        bad
    }

    #[test]
    fn relocation_keeps_shared_pool_references_and_short_prefix_group_order() {
        let f = Fixture::new();
        let (raw, proof) = build_bundle(3, 123, 456, 5, &f.owner()).unwrap();
        assert_eq!(raw, f.blob());
        let p = parse_bundle(&raw).unwrap();
        assert_eq!(p.map, vec![1, NO_CERT, 0, 1, 0]);
        assert_eq!(p.tile_certs.len(), 2);
        assert_eq!(p.tile_certs.iter().map(|c| c.offset).collect::<Vec<_>>(), vec![0, 32]);
        assert_eq!(p.tile_bits, vec![0b101101, 1]);
        assert_eq!(p.groups, f.groups);
        assert_eq!((field(&raw, 8), field(&raw, 12), field(&raw, 16), field(&raw, 20)), (3, 123, 456, 5));
        let Json::Obj(fields) = proof else { panic!() };
        assert_eq!(fields[0], ("status".into(), Json::Str("PASS".into())));
        assert_eq!(fields[1], ("pool_bindings".into(), Json::Int(5)));
        assert!(raw.len() <= ARENA_CEILING);
    }
    #[test]
    fn empty_certificates_and_groups_still_preserve_complete_pool_map() {
        let map = vec![NO_CERT; 4];
        let owner = Owner { certificates: &[], bits: &[], map: &map, group_certificates: &[], group_bits: &[], groups: &[] };
        let (raw, _) = build_bundle(0, 1, 2, 4, &owner).unwrap();
        let p = parse_bundle(&raw).unwrap();
        assert_eq!(p.map, map);
        assert!(p.groups.is_empty());
    }
    #[test]
    fn atlas_fingerprint_has_independent_little_endian_ordered_contract() {
        let a: J = serde_json::json!({"kind": 0, "first": 2, "count": 1, "raw_len": 32768, "raw_fnv": 0xabcdef01u32});
        let b: J = serde_json::json!({"kind": 1, "first": 9, "count": 3, "raw_len": 96, "raw_fnv": 0x98765432u32});
        let bytes = [0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0x80, 0, 0, 0x01, 0xef, 0xcd, 0xab, 1, 0, 0, 0, 9, 0, 0, 0, 3, 0, 0, 0, 0x60, 0, 0, 0, 0x32, 0x54, 0x76, 0x98];
        assert_eq!(atlas_fingerprint(&[&a, &b]).unwrap(), fnv(&bytes));
        assert_ne!(atlas_fingerprint(&[&a, &b]).unwrap(), atlas_fingerprint(&[&b, &a]).unwrap());
    }
    #[test]
    fn rejects_malformed_sections_configuration_and_padding() {
        let original = Fixture::new().blob();
        let mut cases = vec![original[..79].to_vec(), [original.clone(), vec![0; 4]].concat(), poke(&original, 0, b"BADMAGIC")];
        for (at, value) in [(20, 6u32), (24, 3), (28, 1), (32, 84), (36, 0xffffffff), (40, 80), (44, 0xffffffff)] {
            cases.push(poke(&original, at, &value.to_le_bytes()));
        }
        // Five u16 pool entries leave two alignment bytes before group certs.
        let (off, count) = (field(&original, 48), field(&original, 52));
        cases.push(poke(&original, off + count * 2, &[1]));
        for bad in cases {
            assert!(parse_bundle(&bad).is_err(), "accepted a bundle of {} bytes", bad.len());
        }
    }
    #[test]
    fn rejects_out_of_bounds_ids_bitmap_words_and_group_bindings() {
        let original = Fixture::new().blob();
        let [tc, tb, mp, _gc, _gb, gr] = core::array::from_fn(|i| field(&original, 32 + i * 8));
        let word = |v: u32| v.to_le_bytes().to_vec();
        let half = |v: u16| v.to_le_bytes().to_vec();
        let mutations = [
            (tc + 8, word(1)), (tc + 4, half(0)), (tb, word(0)), (tb, word(0xffffffff)), (mp, half(99)),
            (gr, half(99)), (gr + 2, half(1)), (gr + 4, half(4)), (gr + 6, half(0)), (gr + 4, half(0)),
            (gr + 12, half(1)), (gr + 14, half(0)), (gr + 16, half(NO_CERT as u16)),
        ];
        for (at, bytes) in mutations {
            assert!(parse_bundle(&poke(&original, at, &bytes)).is_err(), "accepted a change at {at}");
        }
    }
    #[test]
    fn exhaustive_comparison_rejects_valid_but_different_bits_and_eligibility() {
        let f = Fixture::new();
        let raw = f.blob();
        let (tilebits, poolmap) = (field(&raw, 40), field(&raw, 48));
        for bad in [poke(&raw, tilebits, &0b101111u32.to_le_bytes()), poke(&raw, poolmap, &(NO_CERT as u16).to_le_bytes())] {
            parse_bundle(&bad).unwrap();
            assert!(verify_equivalence(&bad, &f.owner()).is_err());
        }
    }
    #[test]
    fn regional_first_occurrence_mapping_is_exact() {
        let f = Fixture::new();
        let raw = f.blob();
        let mut bank = vec![0u8; 50];
        for (k, v) in [1u32, 5, 0, 0, 0, 40, 0, 0, 0, 0].into_iter().enumerate() {
            bank[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        for (k, v) in [3u16, 1, 0, 3, 4].into_iter().enumerate() {
            bank[40 + 2 * k..42 + 2 * k].copy_from_slice(&v.to_le_bytes());
        }
        let scene: J = serde_json::json!({"sections": {"rooms": {"offset": 0}}, "source_rooms": [{"chunk_id": 1, "counts": [0, 0, 5]}]});
        let expected: Vec<J> = vec![serde_json::json!([[0, 2], [2, 2], [3, 2], [4, 1]])];
        let proof = verify_region_bindings(&raw, &scene, &bank, &f.owner(), &expected).unwrap();
        let Json::List(rows) = proof else { panic!() };
        assert_eq!(rows, vec![Json::Obj(vec![
            ("chunk_id".into(), Json::Int(1)), ("local_draws".into(), Json::Int(5)),
            ("certified_draws".into(), Json::Int(4)), ("resolved_groups".into(), Json::Int(2)),
        ])]);
        let wrong: Vec<J> = vec![serde_json::json!([[0, 1], [2, 2], [3, 2], [4, 1]])];
        assert!(verify_region_bindings(&raw, &scene, &bank, &f.owner(), &wrong).is_err());
    }
    #[test]
    fn format_capacity_is_fixed_and_does_not_truncate_proofs() {
        let mut f = Fixture::new();
        f.certs[1] = d(-2, 1, 1024, 1024, 0);
        f.bits = vec![0xffffffff; 32768];
        f.map = vec![1; 5];
        let owner = Owner { group_certificates: &[], group_bits: &[], groups: &[], ..f.owner() };
        assert!(build_bundle(3, 1, 2, 5, &owner).unwrap_err().contains("arena"));
        let mut f = Fixture::new();
        f.map.pop();
        assert!(build_bundle(3, 1, 2, 5, &f.owner()).is_err());
    }
    #[test]
    fn manifest_carries_predecode_inventory_and_pack_indices() {
        let entry = |scene_id, chunk_id, raw_len| Bundle {
            scene_id, scene_raw_fnv: 2, atlas_fnv: 3, draw_pool_count: 17, chunk_id, raw_len, raw_fnv: 4, stored_len: 55, stored_fnv: 6,
        };
        let text = rust_manifest(&[entry(1, 37, 88), entry(2, 38, 40)], 17);
        // The reservation is the largest proof, four-byte aligned, not the
        // ceiling: slack under ARENA_CEILING must cost the guest no BSS.
        assert!(text.contains("COVERAGE_ARENA_BYTES:usize=88;"));
        assert!(rust_manifest(&[entry(1, 37, 89)], 17).contains("COVERAGE_ARENA_BYTES:usize=92;"));
        assert!(text.contains("COVERAGE_GROUP_POOL_CAPACITY:usize=17;"));
        assert!(text.contains("scene_id:1,scene_raw_fnv:2,atlas_fnv:3,draw_pool_count:17,chunk_id:37"));
    }
    #[test]
    fn generated_rust_is_checked_against_report_not_trusted_as_arbitrary_code() {
        let report: J = serde_json::json!({
            "certificates": [{"gx": -2, "gy": 1, "width": 3, "height": 2, "offset": 0}],
            "bitmap_bytes": 4, "grid_shift": 2, "bank_bindings": [[0, 65535]],
        });
        let text = "pub const TILE_CERT_SHIFT:u32=2;\npub static TILE_CERTS:&[TileCert]=&[\nTileCert{gx:-2,gy:1,width:3,height:2,offset:0},\n];\npub static TILE_CERT_BITS:&[u32]=&[\n0x0000002d,\n];\npub static SCENE_DRAW_CERTS:&[&[u16]]=&[\n&[0,65535],\n];";
        assert_eq!(read_legacy(&report, text, false).unwrap().bits, vec![45]);
        for bad in [text.replace("width:3", "width:4"), text.replace("&[0,65535]", "&[1,65535]"), text.replace("0x0000002d", "include_bytes!(\"unknown\")")] {
            assert!(read_legacy(&report, &bad, false).is_err());
        }
    }
    #[test]
    fn stale_source_cannot_publish_bundle_or_manifest() {
        let root = std::env::temp_dir().join(format!("hk-cert-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [".hkpsx/opaque-tiles", ".hkpsx/opaque-groups", "data"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let reports = [
            (".hkpsx/packed-scenes.json", r#"{"residency": "scene_gate"}"#),
            (".hkpsx/opaque-tiles/report.json", r#"{"input_sha256": {"data/stale.rs": "bad"}, "outputs": {}}"#),
            (".hkpsx/opaque-groups/report.json", "{}"),
            (".hkpsx/ambience.json", "{}"),
            ("data/stale.rs", "changed"),
        ];
        for (path, value) in reports {
            std::fs::write(root.join(path), value).unwrap();
        }
        let message = cook(&root, None, None, None).unwrap_err();
        assert!(message.contains("Stale"), "{message}");
        assert!(!root.join("data/scene-coverage").exists() && !root.join("data/scene_coverage_manifest.rs").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
