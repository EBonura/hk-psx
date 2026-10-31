//! Serialise a boss's art (`boss_art::Library`) as an HKBOSS bank (shared/hk-format `boss.rs`) and cut it into the chunk
//! textures the boss's scene carries. docs/BOSS-ART.md has the format and the measurements behind its choices.
use crate::atlas::Atlas;
use crate::boss_art::{self as ba, Library};
use crate::hornet_scene::Group;
use crate::common::py_round;
use crate::boss_codec as codec;
use hk_format::boss as fmt;
use std::collections::BTreeMap;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Built {
    /// The chunk textures, 2,048 bytes each, header included.
    pub chunks: Vec<Vec<u8>>,
    /// The shared palettes, 16 words each (index 0 transparent).
    pub palettes: Vec<[u16; 16]>,
    /// The bank clip of each (group, clip name).
    pub clip_of: BTreeMap<(Group, String), usize>,
    pub stats: Stats,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub sprites: usize,
    pub parts: usize,
    pub clips: usize,
    pub raw_parts: usize,
    pub texel_bytes: usize,
    pub coded_bytes: usize,
    pub table_bytes: usize,
    /// Parts coded with the bank's Huffman table, the bytes of that table, and what the parts' texels would take as packbits
    /// alone (the coder before the table).
    pub huffman_parts: usize,
    pub code_table_bytes: usize,
    pub packbits_bytes: usize,
    pub chunks: usize,
    pub palettes: usize,
    /// Per-channel RMS the shared palettes add to what the room quantizer decided, over the opaque texels.
    pub added_rms: f64,
}

fn put16(v: &mut Vec<u8>, n: usize) -> Result<()> {
    v.extend(u16::try_from(n)?.to_le_bytes());
    Ok(())
}
fn put32(v: &mut Vec<u8>, n: u32) {
    v.extend(n.to_le_bytes());
}

/// How each part is stored, and the code table the `CODED` ones share (empty if none is). A part takes the smallest of its
/// texels as they are (`RAW`), packbits and the table's coder; the table is built from the parts that end up using it, so
/// it is rebuilt until the set is stable. A table that does not pay for itself is not written at all, which leaves a bank
/// every old reader can read.
fn store_parts(parts: &[&ba::Part], stats: &mut Stats) -> (Vec<u8>, Vec<(Vec<u8>, u16)>) {
    let alternative: Vec<(Vec<u8>, u16)> = parts
        .iter()
        .map(|p| {
            let packed = ba::packbits(&p.texels);
            if packed.len() < p.texels.len() {
                (packed, 0)
            } else {
                (p.texels.clone(), fmt::RAW)
            }
        })
        .collect();
    stats.packbits_bytes = alternative.iter().map(|(b, _)| b.len()).sum();
    stats.texel_bytes = parts.iter().map(|p| p.texels.len()).sum();
    let planes: Vec<codec::Plane> = parts.iter().map(|p| codec::Plane { width: p.w, height: p.h, texels: &p.texels }).collect();
    let mut kept: Vec<usize> = (0..parts.len()).collect();
    let mut best: Option<(Vec<u8>, Vec<(usize, Vec<u8>)>)> = None;
    for _ in 0..6 {
        let chosen: Vec<codec::Plane> = kept.iter().map(|&i| codec::Plane { width: planes[i].width, height: planes[i].height, texels: planes[i].texels }).collect();
        let coded = codec::encode(&chosen, codec::Options::default());
        let streams: Vec<(usize, Vec<u8>)> = kept.iter().copied().zip(coded.streams).collect();
        let still: Vec<usize> = streams.iter().filter(|(i, s)| s.len() < alternative[*i].0.len()).map(|(i, _)| *i).collect();
        let stable = still.len() == kept.len();
        best = Some((coded.table, streams.into_iter().filter(|(i, _)| still.contains(i)).collect()));
        if stable || still.is_empty() {
            break;
        }
        kept = still;
    }
    let (table, streams) = best.expect("the loop runs at least once");
    let saved: usize = streams.iter().map(|(i, s)| alternative[*i].0.len() - s.len()).sum();
    let mut out = alternative;
    let mut table = table;
    if streams.is_empty() || saved <= table.len() {
        table.clear();
    } else {
        for (i, s) in streams {
            out[i] = (s, fmt::CODED);
            stats.huffman_parts += 1;
        }
    }
    stats.raw_parts = out.iter().filter(|(_, f)| *f == fmt::RAW).count();
    (table, out)
}

/// Cook `lib` onto `palettes` shared palettes.
pub fn build(lib: &Library, id: u16, palettes: usize) -> Result<Built> {
    let n = lib.sprites.len();
    if n == 0 || palettes == 0 || palettes > 255 {
        return Err("a bank needs sprites and between 1 and 255 palettes".into());
    }
    let refs: Vec<ba::Reference> = lib.sprites.iter().map(ba::reference).collect::<ba::Result<_>>()?;
    let hists: Vec<ba::Hist> = refs.iter().map(ba::histogram).collect();
    let (merged, _) = ba::merge(&hists, palettes.min(n), f64::INFINITY, &[], &mut |_, _, _| {});
    let mut shared: Vec<[u16; 16]> = vec![];
    let mut group_of = vec![0usize; n];
    for (g, members) in merged.members.iter().enumerate() {
        // Entries in the order `recolour` indexes them: opaque ones, then semi ones.
        let w = &merged.words[g];
        let mut ordered: Vec<u16> = w.iter().copied().filter(|x| x & 0x8000 == 0).collect();
        ordered.extend(w.iter().copied().filter(|x| x & 0x8000 != 0));
        let mut p = [0u16; 16];
        for (i, x) in ordered.iter().take(15).enumerate() {
            p[i + 1] = *x;
        }
        shared.push(p);
        for &m in members {
            group_of[m] = g;
        }
    }
    // The tables. Every sprite's parts are cut first; the coder then sees them all, because one code table serves the bank.
    struct Cut {
        part: ba::Part,
        sprite: usize,
    }
    let mut cuts: Vec<Cut> = vec![];
    let mut sprite_rows: Vec<fmt::Sprite> = vec![];
    let (mut err, mut px) = (0.0, 0usize);
    let mut stats = Stats::default();
    for (i, s) in lib.sprites.iter().enumerate() {
        let (plane, e, _, _) = ba::recolour(s, &refs[i], &merged.words[group_of[i]], true);
        err += e;
        px += plane.iter().filter(|&&p| p != 0).count();
        let first = cuts.len();
        cuts.extend(ba::parts(&plane, s.w, s.h, 64).into_iter().map(|part| Cut { part, sprite: i }));
        let q = |v: f64| py_round(v * 65536.0) as i32;
        sprite_rows.push(fmt::Sprite {
            width: s.w as u16,
            height: s.h as u16,
            bounds: [q(s.box_[0]), q(s.box_[1]), q(s.box_[2]), q(s.box_[3])],
            first_part: first as u16,
            parts: (cuts.len() - first) as u16,
            palette: group_of[i] as u8,
        });
    }
    stats.added_rms = (err / (3.0 * px.max(1) as f64)).sqrt();
    let (table, stored) = store_parts(&cuts.iter().map(|c| &c.part).collect::<Vec<_>>(), &mut stats);
    let mut parts_out: Vec<(fmt::Part, Vec<u8>)> = vec![];
    for (c, (bytes, flag)) in cuts.iter().zip(stored) {
        parts_out.push((
            fmt::Part {
                x: c.part.x as u16,
                y: c.part.y as u16,
                width: c.part.w as u8,
                height: c.part.h as u8,
                coded: bytes.len() as u16 | flag,
                sprite: c.sprite as u16,
                offset: 0,
            },
            bytes,
        ));
    }
    // The clips.
    let mut clip_rows: Vec<(u16, u16, u32, u8, u8)> = vec![];
    let mut seq: Vec<u16> = vec![];
    let mut clip_of: BTreeMap<(Group, String), usize> = BTreeMap::new();
    for c in &lib.clips {
        clip_of.insert((c.group.clone(), c.name.clone()), clip_rows.len());
        clip_rows.push((seq.len() as u16, c.frames.len() as u16, py_round(c.fps * 65536.0) as u32, c.wrap as u8, c.loop_start as u8));
        seq.extend(c.frames.iter().map(|&f| f as u16));
    }
    if seq.len() > u16::MAX as usize || parts_out.len() > u16::MAX as usize {
        return Err("bank tables exceed 16-bit indices".into());
    }
    let off_clip = fmt::HEADER_BYTES;
    let off_seq = off_clip + clip_rows.len() * fmt::CLIP_BYTES;
    let off_sprite = off_seq + seq.len() * 2;
    let off_part = off_sprite + sprite_rows.len() * fmt::SPRITE_BYTES;
    let table_at = off_part + parts_out.len() * fmt::PART_BYTES;
    let data = table_at + table.len();
    let mut at = data;
    for (p, bytes) in parts_out.iter_mut() {
        p.offset = at as u32;
        at += bytes.len();
    }
    let mut blob: Vec<u8> = vec![];
    put32(&mut blob, fmt::BANK_MAGIC);
    put32(&mut blob, id as u32 | (shared.len() as u32) << 16);
    for v in [sprite_rows.len(), parts_out.len(), clip_rows.len(), seq.len(), off_clip, off_seq, off_sprite, off_part] {
        put32(&mut blob, v as u32);
    }
    assert_eq!(blob.len(), fmt::HEADER_BYTES);
    for &(first, count, fps, wrap, loop_start) in &clip_rows {
        put16(&mut blob, first as usize)?;
        put16(&mut blob, count as usize)?;
        put32(&mut blob, fps);
        blob.extend([wrap, loop_start, 0, 0]);
    }
    for &s in &seq {
        put16(&mut blob, s as usize)?;
    }
    for s in &sprite_rows {
        put16(&mut blob, s.width as usize)?;
        put16(&mut blob, s.height as usize)?;
        for b in s.bounds {
            put32(&mut blob, b as u32);
        }
        put16(&mut blob, s.first_part as usize)?;
        put16(&mut blob, s.parts as usize)?;
        blob.extend([s.palette, 0, 0, 0]);
    }
    for (p, _) in &parts_out {
        put16(&mut blob, p.x as usize)?;
        put16(&mut blob, p.y as usize)?;
        blob.extend([p.width, p.height]);
        put16(&mut blob, p.coded as usize)?;
        put16(&mut blob, p.sprite as usize)?;
        put32(&mut blob, p.offset);
    }
    assert_eq!(blob.len(), table_at);
    blob.extend(&table);
    for (_, bytes) in &parts_out {
        blob.extend(bytes);
    }
    stats.table_bytes = table_at;
    stats.code_table_bytes = table.len();
    stats.coded_bytes = blob.len() - data;
    stats.sprites = n;
    stats.parts = parts_out.len();
    stats.clips = clip_rows.len();
    stats.palettes = shared.len();
    // Chunks.
    let count = blob.len().div_ceil(fmt::CHUNK_PAYLOAD).max(shared.len());
    if count > fmt::MAX_CHUNKS {
        return Err(format!("bank needs {count} chunks, the guest indexes {}", fmt::MAX_CHUNKS).into());
    }
    let mut chunks = vec![];
    for i in 0..count {
        let mut c = Vec::with_capacity(fmt::CHUNK_BYTES);
        put32(&mut c, fmt::CHUNK_MAGIC);
        put16(&mut c, id as usize)?;
        put16(&mut c, i)?;
        let from = (i * fmt::CHUNK_PAYLOAD).min(blob.len());
        let to = ((i + 1) * fmt::CHUNK_PAYLOAD).min(blob.len());
        c.extend(&blob[from..to]);
        c.resize(fmt::CHUNK_BYTES, 0);
        chunks.push(c);
    }
    stats.chunks = chunks.len();
    Ok(Built { chunks, palettes: shared, clip_of, stats })
}

/// The logical blob back from the chunks, as the guest addresses it.
pub fn blob_of(built: &Built) -> Vec<u8> {
    built.chunks.iter().flat_map(|c| c[fmt::CHUNK_HEADER..].iter().copied()).collect()
}

/// Sprite `i` of a built bank, decoded the way the guest does: its index plane (0 transparent), its shared palette's words
/// (index 0 first) and its size. Reads only the bytes the chunks hold.
pub fn decode_sprite(blob: &[u8], built: &Built, i: usize) -> Option<(Vec<u8>, [u16; 16], usize, usize)> {
    let bank = fmt::Bank::open(blob)?;
    let s = bank.sprite(i);
    let (w, h) = (s.width as usize, s.height as usize);
    let mut plane = vec![0u8; w * h];
    for p in 0..s.parts as usize {
        let part = bank.part(s.first_part as usize + p);
        let stride = fmt::Part::stride(part.width as usize);
        let mut bytes = vec![0u8; part.texel_bytes()];
        bank.decode_into(&part, &mut bytes)?;
        for y in 0..part.height as usize {
            for x in 0..part.width as usize {
                plane[(part.y as usize + y) * w + part.x as usize + x] = (bytes[y * stride + x / 2] >> ((x & 1) * 4)) & 15;
            }
        }
    }
    Some((plane, built.palettes[s.palette as usize], w, h))
}

/// The chunks as an atlas of streamed 64x64 textures, in order. Chunk `i` names palette `i % palettes` of the bank, so the first
/// `palettes` chunks carry the palettes into the scene's CLUT block.
pub fn atlas_of(built: &Built) -> Result<Atlas> {
    let mut atlas = Atlas::standard();
    for (i, c) in built.chunks.iter().enumerate() {
        let p = &built.palettes[i % built.palettes.len()];
        let palette: Vec<u8> = p.iter().flat_map(|w| w.to_le_bytes()).collect();
        atlas.add_quantized(64, 64, &palette, c, true, true)?;
    }
    atlas.pack()?;
    Ok(atlas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hk_pil::{Image, Mode};

    fn sprite(id: &str, w: usize, h: usize, seed: u8) -> ba::Sprite {
        let mut image = Image::new(Mode::Rgba, w, h);
        for y in 0..h {
            for x in 0..w {
                // A diagonal band of two flat colours with a transparent corner.
                let on = (x + y) % 17 < 9 && !(x < 8 && y < 8);
                if on {
                    let c = if x < w / 2 { [200u8, 40 + seed, 40] } else { [230, 230, 235] };
                    image.data[(y * w + x) * 4..][..4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                }
            }
        }
        ba::Sprite { id: id.into(), box_: [0.0, 0.0, 2.0, 1.0], w, h, image }
    }

    #[test]
    fn a_built_bank_decodes_to_the_planes_it_was_cut_from() {
        let group: Group = ("lib".into(), 0, 0, 0);
        let lib = Library {
            sprites: vec![sprite("a", 90, 70, 0), sprite("b", 40, 30, 20), sprite("c", 130, 20, 40)],
            clips: vec![ba::ClipRec { group: group.clone(), name: "Idle".into(), library: "lib".into(), fps: 12.0, wrap: 0, loop_start: 0, frames: vec![0, 1, 2, 1] }],
        };
        let built = build(&lib, 7, 2).unwrap();
        assert_eq!(built.palettes.len(), 2);
        assert!(built.chunks.iter().all(|c| c.len() == fmt::CHUNK_BYTES));
        // Reassemble the logical blob from the chunks and read it back through the shared reader.
        let mut blob = vec![];
        for (i, c) in built.chunks.iter().enumerate() {
            assert_eq!(u32::from_le_bytes(c[..4].try_into().unwrap()), fmt::CHUNK_MAGIC);
            assert_eq!(u16::from_le_bytes(c[6..8].try_into().unwrap()) as usize, i);
            blob.extend(&c[fmt::CHUNK_HEADER..]);
        }
        let bank = fmt::Bank::open(blob.as_slice()).unwrap();
        assert_eq!((bank.sprites, bank.clips, bank.id), (3, 1, 7));
        let clip = bank.clip(0);
        assert_eq!((clip.count, clip.fps), (4, 12 << 16));
        assert_eq!(bank.seq_sprite(clip.first as usize + 2), 2);
        let refs: Vec<ba::Reference> = lib.sprites.iter().map(|s| ba::reference(s).unwrap()).collect();
        for i in 0..3 {
            let s = bank.sprite(i);
            assert_eq!((s.width as usize, s.height as usize), (lib.sprites[i].w, lib.sprites[i].h));
            // Every opaque texel of the sprite is in exactly one part, with the index the shared palette gave it.
            let mut covered = 0;
            for p in 0..s.parts as usize {
                let part = bank.part(s.first_part as usize + p);
                assert_eq!(part.sprite as usize, i);
                let mut bytes = vec![0u8; part.texel_bytes()];
                bank.decode_into(&part, &mut bytes).unwrap();
                let stride = fmt::Part::stride(part.width as usize);
                for y in 0..part.height as usize {
                    for x in 0..part.width as usize {
                        covered += ((bytes[y * stride + x / 2] >> ((x & 1) * 4)) & 15 != 0) as usize;
                    }
                }
            }
            assert_eq!(covered, refs[i].plane.iter().filter(|&&p| p != 0).count());
        }
        let atlas = atlas_of(&built).unwrap();
        assert_eq!(atlas.entries.len(), built.chunks.len());
        assert_eq!(atlas.stream.len(), built.chunks.len() * 2048);
    }

    /// Recode the banks named by `HK_RECODE_IN` (paths joined by `+`, read as one blob, as the guest does) with the current
    /// coder and print what it saves; with `HK_RECODE_OUT` set, write the recoded bank there. Not a test of anything but the
    /// cooker's own measurements, so it is ignored by default: `HK_RECODE_IN=a.bin cargo test -p hk-cook recode -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn recode_a_cooked_bank() {
        let Ok(paths) = std::env::var("HK_RECODE_IN") else { return };
        let old: Vec<u8> = paths.split('+').flat_map(|p| std::fs::read(p).unwrap()).collect();
        let bank = fmt::Bank::open(old.as_slice()).unwrap();
        let mut texels = vec![];
        for i in 0..bank.parts as usize {
            let part = bank.part(i);
            let mut bytes = vec![0u8; part.texel_bytes()];
            bank.decode_into(&part, &mut bytes).unwrap();
            texels.push((part, bytes));
        }
        let cuts: Vec<ba::Part> =
            texels.iter().map(|(p, t)| ba::Part { x: p.x as usize, y: p.y as usize, w: p.width as usize, h: p.height as usize, texels: t.clone() }).collect();
        let mut stats = Stats::default();
        let (table, stored) = store_parts(&cuts.iter().collect::<Vec<_>>(), &mut stats);
        let part_at = bank.table_at() - bank.parts as usize * fmt::PART_BYTES;
        let old_data: usize = (0..bank.parts as usize).map(|i| (bank.part(i).coded & fmt::LEN_MASK) as usize).sum();
        let mut out = old[..part_at].to_vec();
        let mut at = part_at + stored.len() * fmt::PART_BYTES + table.len();
        for (i, (bytes, flag)) in stored.iter().enumerate() {
            let p = bank.part(i);
            out.extend(p.x.to_le_bytes());
            out.extend(p.y.to_le_bytes());
            out.extend([p.width, p.height]);
            out.extend((bytes.len() as u16 | flag).to_le_bytes());
            out.extend(p.sprite.to_le_bytes());
            out.extend((at as u32).to_le_bytes());
            at += bytes.len();
        }
        out.extend(&table);
        for (bytes, _) in &stored {
            out.extend(bytes);
        }
        let new_data: usize = stored.iter().map(|(b, _)| b.len()).sum::<usize>() + table.len();
        println!(
            "recode: {} parts, {} coded with the table; texel data {old_data} -> {new_data} bytes ({:+.1} percent) of which table {}; blob {} -> {}",
            stored.len(),
            stats.huffman_parts,
            (new_data as f64 / old_data as f64 - 1.0) * 100.0,
            table.len(),
            old.len(),
            out.len()
        );
        // The new blob decodes to the same texels.
        let new = fmt::Bank::open(out.as_slice()).unwrap();
        if std::env::var("HK_RECODE_PARTS").is_ok() {
            for (i, (p, t)) in texels.iter().enumerate() {
                let (tokens, literals) = codec::token_counts(&codec::Plane { width: p.width as usize, height: p.height as usize, texels: t }, codec::Options::default());
                println!("part {i} {}x{} stream {} tokens {tokens} literals {literals}", p.width, p.height, stored[i].0.len());
            }
        }
        for (i, (_, want)) in texels.iter().enumerate() {
            let part = new.part(i);
            let mut got = vec![0u8; part.texel_bytes()];
            new.decode_into(&part, &mut got).unwrap();
            assert_eq!(&got, want, "part {i}");
        }
        if let Ok(path) = std::env::var("HK_RECODE_OUT") {
            std::fs::write(path, &out).unwrap();
        }
    }
}
