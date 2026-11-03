//! The guest's atlas inventory in 16 bytes a row instead of 32.
//!
//! `data/scene_manifest.rs` (written by host/pack_scenes.py) lists every atlas chunk as an
//! `AtlasDesc` of eight machine words, 22,656 bytes of `.data` for 708 rows. Four of the eight are
//! facts the rows already imply: the owning scene (the row's place in `SCENE_ATLAS_RANGES`), the raw
//! length (count times the page or palette size) and, with the kind and count, the next row's
//! `first`. This step runs on every build, after the asset stage, like `scene_specs::pack` (so it
//! touches no cook key): it checks those facts for every row, then writes
//! `data/scene_manifest_guest.rs`, which is the manifest with the `ATLASES` block replaced by
//! `ATLAS_ROWS` of `AtlasRow`. The guest expands a row to the full `AtlasDesc` on use
//! (game/src/disc.rs `atlas`), so every reader sees the same values as before.
use super::Result;
use std::path::Path;

const PAGE_BYTES: u64 = 32768;
const PALETTE_BYTES: u64 = 32;

fn field(row: &str, name: &str) -> Result<u64> {
    let key = format!("{name}:");
    let at = row.find(&key).ok_or_else(|| format!("atlas row without {name}: {row}"))? + key.len();
    let digits: String = row[at..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().map_err(|_| format!("atlas row {name} is not a number: {row}").into())
}

/// The `(first, end)` atlas ranges of `SCENE_ATLAS_RANGES`.
fn ranges(text: &str) -> Result<Vec<(u64, u64)>> {
    let line = text
        .lines()
        .find(|l| l.starts_with("pub const SCENE_ATLAS_RANGES"))
        .ok_or("scene_manifest.rs has no SCENE_ATLAS_RANGES")?;
    let body = &line[line.find("=&[(").ok_or("SCENE_ATLAS_RANGES shape")? + 3..];
    let mut out = Vec::new();
    for pair in body.split(')').filter(|p| p.contains('(')) {
        let nums: Vec<u64> = pair
            .trim_start_matches([',', '[', ' '])
            .trim_start_matches('(')
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        if nums.len() != 2 {
            return Err(format!("SCENE_ATLAS_RANGES entry {pair:?}").into());
        }
        out.push((nums[0], nums[1]));
    }
    Ok(out)
}

/// The manifest with its atlas rows compacted; fails when a row does not carry the facts the guest derives.
pub fn transform(text: &str) -> Result<String> {
    let ranges = ranges(text)?;
    let mut out = String::with_capacity(text.len());
    let mut rows = 0u64;
    let (mut in_rows, mut next_first) = (false, [0u64; 2]);
    let mut scene_of_row = Vec::new();
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if bare.starts_with("pub const ATLASES:&[AtlasDesc]=&[") {
            in_rows = true;
            out.push_str(
                "/// 16 bytes a row (game/src/disc.rs `atlas` expands it): the facts `AtlasDesc` repeats are derived and\n\
                 /// checked by host/hk-build/guest_manifest.rs.\n\
                 #[derive(Clone,Copy,Debug)]\n\
                 pub struct AtlasRow{pub stored_len:u32,pub stored_fnv:u32,pub raw_fnv:u32,pub first:u16,pub count_kind:u16}\n\
                 pub const ATLAS_ROWS:&[AtlasRow]=&[\n",
            );
            continue;
        }
        if in_rows {
            if bare == "];" {
                in_rows = false;
                out.push_str(line);
                continue;
            }
            let index = rows;
            let scene = ranges
                .iter()
                .position(|&(a, b)| a <= index && index < b)
                .ok_or_else(|| format!("atlas row {index} is in no scene range"))? as u64;
            if field(bare, "scene_index")? != scene {
                return Err(format!("atlas row {index}: scene_index differs from its range").into());
            }
            if scene_of_row.last() != Some(&scene) {
                next_first = [0, 0];
            }
            scene_of_row.push(scene);
            let (kind, first, count) = (field(bare, "kind")?, field(bare, "first")?, field(bare, "count")?);
            let unit = match kind {
                0 => PAGE_BYTES,
                1 => PALETTE_BYTES,
                _ => return Err(format!("atlas row {index}: kind {kind}").into()),
            };
            if first != next_first[kind as usize] || count == 0 || count >= 1 << 15 || first >= 1 << 16 {
                return Err(format!("atlas row {index}: first/count not contiguous or too wide").into());
            }
            if field(bare, "raw_len")? != count * unit {
                return Err(format!("atlas row {index}: raw_len is not count x unit").into());
            }
            next_first[kind as usize] += count;
            out.push_str(&format!(
                "AtlasRow{{stored_len:{},stored_fnv:{},raw_fnv:{},first:{first},count_kind:{}}},\n",
                field(bare, "stored_len")?,
                field(bare, "stored_fnv")?,
                field(bare, "raw_fnv")?,
                count | (kind << 15)
            ));
            rows += 1;
            continue;
        }
        out.push_str(line);
    }
    if rows == 0 || in_rows {
        return Err("scene_manifest.rs: the ATLASES block was not found or not closed".into());
    }
    if ranges.last().map(|r| r.1) != Some(rows) {
        return Err("SCENE_ATLAS_RANGES does not end at the last atlas row".into());
    }
    Ok(out)
}

/// Write `data/scene_manifest_guest.rs` from `data/scene_manifest.rs`.
pub fn pack(root: &Path) -> Result<()> {
    let text = std::fs::read_to_string(root.join("data/scene_manifest.rs"))?;
    let guest = transform(&text)?;
    let path = root.join("data/scene_manifest_guest.rs");
    if !std::fs::read_to_string(&path).is_ok_and(|old| old == guest) {
        std::fs::write(&path, guest)?;
    }
    Ok(())
}

/// The guest manifest on disk is today's compaction of today's manifest.
pub fn verify(root: &Path) -> Result<()> {
    let text = std::fs::read_to_string(root.join("data/scene_manifest.rs"))?;
    if std::fs::read_to_string(root.join("data/scene_manifest_guest.rs"))? != transform(&text)? {
        return Err("data/scene_manifest_guest.rs is not data/scene_manifest.rs compacted: rerun the build".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "pub struct AtlasDesc{x:u8}\npub const ATLASES:&[AtlasDesc]=&[\n\
AtlasDesc{scene_index:0,stored_len:10,stored_fnv:11,raw_len:65536,raw_fnv:12,kind:0,first:0,count:2},\n\
AtlasDesc{scene_index:0,stored_len:20,stored_fnv:21,raw_len:96,raw_fnv:22,kind:1,first:0,count:3},\n\
AtlasDesc{scene_index:1,stored_len:30,stored_fnv:31,raw_len:32768,raw_fnv:32,kind:0,first:0,count:1},\n\
];\npub const SCENE_ATLAS_RANGES:&[(u16,u16)]=&[(0,2),(2,3)];\npub const TAIL:u8=1;\n";

    #[test]
    fn rows_are_compacted_and_the_rest_kept() {
        let out = transform(SAMPLE).unwrap();
        assert!(out.contains("AtlasRow{stored_len:20,stored_fnv:21,raw_fnv:22,first:0,count_kind:32771}"));
        assert!(out.contains("pub struct AtlasDesc{x:u8}") && out.contains("pub const TAIL:u8=1;"));
        assert!(!out.contains("scene_index:0,stored_len"));
    }

    #[test]
    fn a_row_that_breaks_a_derived_fact_is_refused() {
        assert!(transform(&SAMPLE.replace("raw_len:96", "raw_len:97")).is_err());
        assert!(transform(&SAMPLE.replace("first:0,count:3", "first:1,count:3")).is_err());
        assert!(transform(&SAMPLE.replace("scene_index:1", "scene_index:0")).is_err());
        assert!(transform(&SAMPLE.replace("(0,2),(2,3)", "(0,2),(2,4)")).is_err());
    }
}
