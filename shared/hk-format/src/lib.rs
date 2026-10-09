//! HKROOM02: checked, allocation-free little-endian room and animation reader.
#![no_std]
pub mod coverage;
mod scene;
pub use scene::{Scene, SceneValidation};
pub mod world_meta;
pub use world_meta::{
    Object as WorldObject, Polygon as WorldPolygon, Region as WorldRegion, WorldMeta,
};
#[derive(Debug, PartialEq)]
pub enum Error {
    Header,
    Limit,
    Truncated,
    Reference,
    Geometry,
}
pub const STREAMED_PAGE: u16 = u16::MAX;
pub const HAS_ALPHA_COVERS: u32 = 1;
pub const ALPHA_COVER_BYTES: usize = 20;
pub const MAX_STREAM_BYTES: usize = 256 * 1024;
/// A streamed texture's VRAM origin is the animation slot it lands in, so its
/// `u`/`v` never describe a placement. The first tile of a frame too large for
/// one slot carries the frame's tile grid there instead: `u` columns by `v`
/// rows of consecutive texture IDs in row-major order. Every other streamed
/// texture keeps 0,0 and is a frame on its own.
pub const MAX_TILE_GRID: u16 = 8;
/// Texture records one region may address. The palette block is sized to match,
/// which is a storage rule and not a VRAM one: see `Texture::palette`.
pub const MAX_TEXTURES: usize = 640;
#[derive(Clone, Copy, Debug)]
pub struct Texture {
    pub page: u16,
    pub u: u16,
    pub v: u16,
    pub width: u16,
    pub height: u16,
    /// Index into the region's palette block, which holds one 32-byte entry per
    /// texture. Nothing requires those entries to differ and nothing requires a
    /// texture to own one: any number of textures may name the same index, and
    /// the tiles of one tiled frame always do, because the frame is quantized
    /// once. A region's VRAM CLUT cost is therefore its distinct palette words
    /// rather than its texture count, and the scene bank pools those words by
    /// value before a scene uploads them.
    pub palette: u16,
    pub stream_offset: u32,
}
impl Texture {
    pub fn is_streamed(self) -> bool {
        self.page == STREAMED_PAGE
    }
    pub fn stream_stride(self) -> usize {
        ((self.width as usize + 3) & !3) / 2
    }
    /// Tile columns and rows this texture's frame binds. One tile unless the
    /// cooker had to split a frame across several 64x64 slots.
    pub fn tile_grid(self) -> (usize, usize) {
        if !self.is_streamed() || self.u == 0 {
            return (1, 1);
        }
        (self.u as usize, self.v as usize)
    }
}
impl core::ops::Index<usize> for Texture {
    type Output = u16;
    fn index(&self, i: usize) -> &u16 {
        match i {
            0 => &self.page,
            1 => &self.u,
            2 => &self.v,
            3 => &self.width,
            4 => &self.height,
            5 => &self.palette,
            _ => panic!("texture field"),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Room<'a> {
    pub bytes: &'a [u8],
    pub counts: [usize; 6],
    offsets: [usize; 8],
    stream_bytes: usize,
    // Resident scene rooms keep local record order through reference arrays.
    // Textures and their VRAM placements are shared across every scene region.
    references: Option<[usize; 4]>,
}
/// Bounded metadata-check cursor. It owns no arena borrow and never publishes a
/// `Room`. Each step checks at most its record budget; header/layout validation
/// happens once in `new`. Length is checked again before every slice access.
///
/// Completion covers the bytes observed across calls. A caller using
/// `Room::validated_view` must keep the entire payload unchanged from `new`
/// through completion and while any resulting room view exists. A decoder can
/// enforce this by keeping its inactive arena private until completion.
pub struct Validation {
    counts: [usize; 6],
    offsets: [usize; 8],
    stream_bytes: usize,
    byte_len: usize,
    section: usize,
    index: usize,
}
impl Validation {
    pub fn new(bytes: &[u8]) -> Result<Self, Error> {
        let room = Room::header(bytes)?;
        Ok(Self {
            counts: room.counts,
            offsets: room.offsets,
            stream_bytes: room.stream_bytes,
            byte_len: bytes.len(),
            section: 0,
            index: 0,
        })
    }
    fn view<'a>(&self, bytes: &'a [u8]) -> Room<'a> {
        Room {
            bytes,
            counts: self.counts,
            offsets: self.offsets,
            stream_bytes: self.stream_bytes,
            references: None,
        }
    }
    pub fn step(&mut self, bytes: &[u8], mut records: usize) -> Result<bool, Error> {
        if bytes.len() != self.byte_len {
            return Err(Error::Truncated);
        }
        let room = self.view(bytes);
        while self.section < 5 {
            if self.index == self.counts[self.section + 1] {
                self.section += 1;
                self.index = 0;
                continue;
            }
            if records == 0 {
                return Ok(false);
            }
            room.validate_record(self.section, self.index)?;
            self.index += 1;
            records -= 1;
        }
        Ok(true)
    }
}
// Bounded fixed-width slices let the compiler combine byte reads without any
// pointer alignment requirement. The suffix slice also avoids offset addition
// overflow; invalid or truncated fields still panic before accessing bytes.
#[inline]
pub fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..][..2].try_into().unwrap())
}
#[inline]
pub fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..][..4].try_into().unwrap())
}
#[inline]
pub fn i32_at(b: &[u8], i: usize) -> i32 {
    u32_at(b, i) as i32
}
impl<'a> Room<'a> {
    /// Borrow an immutable payload that has already passed `parse`.
    ///
    /// # Safety
    /// The same bytes must have passed `Room::parse`, or every step of a
    /// completed `Validation`, and remained unchanged throughout and since validation.
    pub unsafe fn validated_view(b: &'a [u8]) -> Self {
        let counts = core::array::from_fn(|i| u32_at(b, 8 + i * 4) as usize);
        let mut offsets = [0; 8];
        let mut p = 40;
        for (i, (n, size)) in [
            (counts[1], 16),
            (counts[2], 44),
            (counts[3], 20),
            (counts[4], 16),
            (counts[5], 16),
            (counts[1], 32),
            (counts[0], 32768),
        ]
        .iter()
        .enumerate()
        {
            offsets[i] = p;
            p += n * size;
        }
        offsets[7] = p;
        Self {
            bytes: b,
            counts,
            offsets,
            stream_bytes: u32_at(b, 32) as usize,
            references: None,
        }
    }
    fn header(b: &'a [u8]) -> Result<Self, Error> {
        if b.len() < 40 || &b[..8] != b"HKROOM02" || u32_at(b, 36) & !HAS_ALPHA_COVERS != 0 {
            return Err(Error::Header);
        }
        let mut counts = [0; 6];
        let limits = [20, MAX_TEXTURES, 1024, 2048, 128, 1024];
        for i in 0..6 {
            counts[i] = u32_at(b, 8 + i * 4) as usize;
            if counts[i] > limits[i] {
                return Err(Error::Limit);
            }
        }
        let stream_bytes = u32_at(b, 32) as usize;
        if counts[1] == 0 || stream_bytes > MAX_STREAM_BYTES {
            return Err(Error::Limit);
        }
        let mut offsets = [0; 8];
        let mut p = 40;
        for (i, (n, size)) in [
            (counts[1], 16),
            (counts[2], 44),
            (counts[3], 20),
            (counts[4], 16),
            (counts[5], 16),
            (counts[1], 32),
            (counts[0], 32768),
        ]
        .iter()
        .enumerate()
        {
            offsets[i] = p;
            p += n * size;
        }
        offsets[7] = p;
        if p.checked_add(stream_bytes) != Some(b.len()) {
            return Err(Error::Truncated);
        }
        let room = Self {
            bytes: b,
            counts,
            offsets,
            stream_bytes,
            references: None,
        };
        Ok(room)
    }
    pub fn parse(b: &'a [u8]) -> Result<Self, Error> {
        let mut validation = Validation::new(b)?;
        validation.step(b, usize::MAX)?;
        Ok(validation.view(b))
    }
    fn validate_record(&self, section: usize, i: usize) -> Result<(), Error> {
        let room = self;
        let counts = &self.counts;
        match section {
            0 => {
                let t = room.texture(i);
                if t.palette as usize >= counts[1] || t.width == 0 || t.height == 0 {
                    return Err(Error::Reference);
                }
                if t.is_streamed() {
                    if t.width > 64
                        || t.height > 64
                        || t.stream_offset & 3 != 0
                        || room.stream_pixels(t).is_none()
                    {
                        return Err(Error::Reference);
                    }
                    // A tile grid is the frame's, so it must name at least two
                    // tiles, stay inside the slot cache and have every tile
                    // present as a streamed texture of its own.
                    let (cols, rows) = (t.u, t.v);
                    if cols != 0 || rows != 0 {
                        if cols == 0
                            || rows == 0
                            || cols > MAX_TILE_GRID
                            || rows > MAX_TILE_GRID
                            || cols as usize * rows as usize <= 1
                        {
                            return Err(Error::Reference);
                        }
                        let end = i.checked_add(cols as usize * rows as usize);
                        if end.is_none_or(|end| end > counts[1]) {
                            return Err(Error::Reference);
                        }
                        for tile in 1..cols as usize * rows as usize {
                            let t = room.texture(i + tile);
                            if !t.is_streamed() || t.u != 0 || t.v != 0 {
                                return Err(Error::Reference);
                            }
                        }
                    }
                } else if t.page as usize >= counts[0]
                    || t.u as u32 + t.width as u32 > 256
                    || t.v as u32 + t.height as u32 > 256
                {
                    return Err(Error::Reference);
                } else if room.has_alpha_covers() {
                    let cover = room.alpha_cover(t).ok_or(Error::Reference)?;
                    let count = cover[0] as usize;
                    if count > 4
                        || cover[1..4].iter().any(|&x| x != 0)
                        || cover[4 + count * 4..].iter().any(|&x| x != 0)
                    {
                        return Err(Error::Reference);
                    }
                    for a in 0..count {
                        let r = &cover[4 + a * 4..8 + a * 4];
                        let right = r[0] as u16 + r[2] as u16 + 1;
                        let bottom = r[1] as u16 + r[3] as u16 + 1;
                        if right > t.width || bottom > t.height {
                            return Err(Error::Geometry);
                        }
                        for b in 0..a {
                            let q = &cover[4 + b * 4..8 + b * 4];
                            if (r[0] as u16) < q[0] as u16 + q[2] as u16 + 1
                                && (q[0] as u16) < right
                                && (r[1] as u16) < q[1] as u16 + q[3] as u16 + 1
                                && (q[1] as u16) < bottom
                            {
                                return Err(Error::Geometry);
                            }
                        }
                    }
                } else if t.stream_offset != 0 {
                    return Err(Error::Reference);
                }
            }
            1 => {
                let d = room.draw(i);
                if u16_at(d, 0) as usize >= counts[1]
                    || u16_at(d, 2) > 1
                    || d[43] > 1
                    || room.texture(u16_at(d, 0) as usize).is_streamed()
                {
                    return Err(Error::Reference);
                }
                if !(1..=262144).contains(&u32_at(d, 4)) {
                    return Err(Error::Geometry);
                }
                for k in 0..8 {
                    if i32_at(d, 8 + 4 * k).unsigned_abs() > 8_000_000 {
                        return Err(Error::Geometry);
                    }
                }
            }
            2 => {
                let f = room.frame(i);
                if u32_at(f, 0) as usize >= counts[1] {
                    return Err(Error::Reference);
                }
                for k in 0..4 {
                    if i32_at(f, 4 + k * 4).unsigned_abs() > 16 * 65536 {
                        return Err(Error::Geometry);
                    }
                }
            }
            3 => {
                let c = room.clip(i);
                if c[1] == 0
                    || c[0] as usize + c[1] as usize > counts[3]
                    || c[2] == 0
                    || c[2] > 120 * 65536
                    || (c[3] & 65535) > 2
                    || (c[3] >> 16) >= c[1]
                {
                    return Err(Error::Reference);
                }
            }
            4 => {
                let e = room.edge(i);
                if e.iter().any(|x| *x < -512 * 65536 || *x > 512 * 65536) {
                    return Err(Error::Geometry);
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
    pub fn scene_resident(&self) -> bool {
        self.references.is_some()
    }
    fn has_alpha_covers(&self) -> bool {
        u32_at(self.bytes, if self.scene_resident() { 52 } else { 36 }) & HAS_ALPHA_COVERS != 0
    }
    fn record(&self, section: usize, index: usize, size: usize) -> &'a [u8] {
        let index = match self.references {
            Some(refs) => u16_at(self.bytes, refs[section - 1] + index * 2) as usize,
            None => index,
        };
        let p = self.offsets[section] + index * size;
        &self.bytes[p..p + size]
    }
    pub fn stream_byte_count(&self) -> usize {
        self.stream_bytes
    }
    pub fn stream_pixels(&self, t: Texture) -> Option<&'a [u8]> {
        if !t.is_streamed()
            || t.width == 0
            || t.width > 64
            || t.height == 0
            || t.height > 64
            // u/v carry the frame's tile grid, never a VRAM origin; a
            // malformed one means the record is not a texture this reader
            // understands, so refuse rather than stream half a frame.
            || (t.u == 0) != (t.v == 0)
            || t.u > MAX_TILE_GRID
            || t.v > MAX_TILE_GRID
            || t.stream_offset & 3 != 0
        {
            return None;
        }
        let start = t.stream_offset as usize;
        let end = start.checked_add(t.stream_stride() * t.height as usize)?;
        if end > self.stream_bytes {
            return None;
        }
        self.bytes
            .get(self.offsets[7] + start..self.offsets[7] + end)
    }
    /// Cooked disjoint visible-texel cover. Legacy rooms and streamed animation
    /// textures have no cover. Records are validated with their texture once,
    /// without rescanning pixel data on the guest.
    pub fn alpha_cover(&self, t: Texture) -> Option<&'a [u8]> {
        if t.is_streamed() || !self.has_alpha_covers() || t.stream_offset & 3 != 0 {
            return None;
        }
        let start = t.stream_offset as usize;
        let end = start.checked_add(ALPHA_COVER_BYTES)?;
        if end > self.stream_bytes {
            return None;
        }
        self.bytes
            .get(self.offsets[7] + start..self.offsets[7] + end)
    }
    pub fn texture(&self, i: usize) -> Texture {
        let p = self.offsets[0] + i * 16;
        Texture {
            page: u16_at(self.bytes, p),
            u: u16_at(self.bytes, p + 2),
            v: u16_at(self.bytes, p + 4),
            width: u16_at(self.bytes, p + 6),
            height: u16_at(self.bytes, p + 8),
            palette: u16_at(self.bytes, p + 10),
            stream_offset: u32_at(self.bytes, p + 12),
        }
    }
    pub fn draw(&self, i: usize) -> &'a [u8] {
        self.record(1, i, 44)
    }
    /// How many draws the shared pool behind `draw_pool_index` holds: the scene
    /// bank's draw section for a scene-resident view, else this room's own.
    pub fn pool_draw_count(&self) -> usize {
        if self.references.is_some() {
            (self.offsets[2] - self.offsets[1]) / 44
        } else {
            self.counts[2]
        }
    }
    /// Pool draw `p` (`draw(i)` is `pool_draw(draw_pool_index(i))` for a scene view).
    pub fn pool_draw(&self, p: usize) -> &'a [u8] {
        let start = self.offsets[1] + p * 44;
        &self.bytes[start..start + 44]
    }
    /// Shared scene draw identity for immutable cooked side tables. Standalone
    /// legacy rooms have no scene pool and cannot use those certificates.
    pub fn draw_pool_index(&self, i: usize) -> Option<usize> {
        self.references
            .map(|refs| u16_at(self.bytes, refs[0] + i * 2) as usize)
    }
    pub fn frame(&self, i: usize) -> &'a [u8] {
        self.record(2, i, 20)
    }
    /// A frame's first texture ID and the tile grid it binds. The remaining
    /// tiles are the consecutive IDs after it, row-major, left to right and
    /// top to bottom, which is the order `host/cook.py` emits them in.
    pub fn frame_grid(&self, i: usize) -> (usize, usize, usize) {
        let base = u32_at(self.frame(i), 0) as usize;
        let (cols, rows) = self.texture(base).tile_grid();
        (base, cols, rows)
    }
    /// Whole-frame pixel size recovered from the tiles: every tile but the last
    /// in its row or column is a full 64, so only the far edges are measured.
    pub fn frame_pixels(&self, i: usize) -> (usize, usize) {
        let (base, cols, rows) = self.frame_grid(i);
        let right = self.texture(base + cols - 1);
        let bottom = self.texture(base + (rows - 1) * cols);
        (
            (cols - 1) * 64 + right.width as usize,
            (rows - 1) * 64 + bottom.height as usize,
        )
    }
    /// One tile of a frame: its texture ID and the part of the frame's world
    /// box it covers, in the record's own `[x0, y0, x1, y1]` order. Tiles are
    /// cut on 64-pixel boundaries, so a tile's share of the box is its share of
    /// the pixels. Art row 0 is the top of the box, which is `b[3]`.
    pub fn frame_tile(&self, i: usize, tile: usize) -> (usize, [i32; 4]) {
        let (base, cols, rows) = self.frame_grid(i);
        let f = self.frame(i);
        let b: [i32; 4] = core::array::from_fn(|k| i32_at(f, 4 + k * 4));
        if cols * rows == 1 {
            return (base, b);
        }
        let (width, height) = self.frame_pixels(i);
        let t = self.texture(base + tile);
        let (px, py) = ((tile % cols) * 64, (tile / cols) * 64);
        let lerp = |from: i32, to: i32, at: usize, of: usize| {
            from + ((to - from) as i64 * at as i64 / of as i64) as i32
        };
        (
            base + tile,
            [
                lerp(b[0], b[2], px, width),
                lerp(b[3], b[1], py + t.height as usize, height),
                lerp(b[0], b[2], px + t.width as usize, width),
                lerp(b[3], b[1], py, height),
            ],
        )
    }
    pub fn clip(&self, i: usize) -> [u32; 4] {
        let r = self.record(3, i, 16);
        core::array::from_fn(|k| u32_at(r, k * 4))
    }
    pub fn edge(&self, i: usize) -> [i32; 4] {
        let r = self.record(4, i, 16);
        core::array::from_fn(|k| i32_at(r, k * 4))
    }
    /// Compact resident scenes keep logical atlas references; their payload
    /// was uploaded separately at bootstrap and is not part of this view.
    pub fn has_atlas_payload(&self) -> bool {
        !self.scene_resident() || &self.bytes[..8] != b"HKSCNE02"
    }
    pub fn palettes(&self) -> &'a [u8] {
        assert!(
            self.has_atlas_payload(),
            "compact scene has no palette payload"
        );
        &self.bytes[self.offsets[5]..self.offsets[6]]
    }
    pub fn page(&self, i: usize) -> &'a [u8] {
        assert!(
            self.has_atlas_payload(),
            "compact scene has no page payload"
        );
        let p = self.offsets[6] + i * 32768;
        &self.bytes[p..p + 32768]
    }
}
#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    fn pack(streamed: bool) -> std::vec::Vec<u8> {
        let mut b = vec![0; 40 + 16 + 32 + if streamed { 4 } else { 32768 }];
        b[..8].copy_from_slice(b"HKROOM02");
        b[8] = if streamed { 0 } else { 1 };
        b[12] = 1;
        b[46] = 1;
        b[48] = 1;
        if streamed {
            b[40..42].copy_from_slice(&STREAMED_PAGE.to_le_bytes());
            b[32] = 4;
        }
        b
    }
    /// One frame split across a 2x2 slot rectangle, sized like Crossroads_47's
    /// Stag: 91x89 pixels becomes 64+27 by 64+25.
    fn tiled_pack() -> std::vec::Vec<u8> {
        let tiles = [(64u16, 64u16), (27, 64), (64, 25), (27, 25)];
        let mut stream = 0usize;
        let mut b = vec![0u8; 40 + tiles.len() * 16 + 20 + tiles.len() * 32];
        b[..8].copy_from_slice(b"HKROOM02");
        b[12] = tiles.len() as u8;
        b[20] = 1;
        for (i, &(w, h)) in tiles.iter().enumerate() {
            let p = 40 + i * 16;
            b[p..p + 2].copy_from_slice(&STREAMED_PAGE.to_le_bytes());
            if i == 0 {
                b[p + 2..p + 4].copy_from_slice(&2u16.to_le_bytes());
                b[p + 4..p + 6].copy_from_slice(&2u16.to_le_bytes());
            }
            b[p + 6..p + 8].copy_from_slice(&w.to_le_bytes());
            b[p + 8..p + 10].copy_from_slice(&h.to_le_bytes());
            b[p + 10..p + 12].copy_from_slice(&(i as u16).to_le_bytes());
            b[p + 12..p + 16].copy_from_slice(&(stream as u32).to_le_bytes());
            stream += ((((w as usize + 3) & !3) / 2) * h as usize).next_multiple_of(4);
        }
        b[32..36].copy_from_slice(&(stream as u32).to_le_bytes());
        b.resize(b.len() + stream, 0);
        b
    }
    #[test]
    fn a_frame_may_bind_a_rectangle_of_slots() {
        let b = tiled_pack();
        let room = Room::parse(&b).unwrap();
        assert_eq!(room.frame_grid(0), (0, 2, 2));
        assert_eq!(room.frame_pixels(0), (91, 89));
        // Only the frame's first tile carries the grid; the rest are plain
        // streamed textures, so nothing double counts them.
        for i in 1..4 {
            assert_eq!(room.texture(i).tile_grid(), (1, 1));
        }
        assert!(room.stream_pixels(room.texture(3)).is_some());
    }
    #[test]
    fn tiles_partition_the_frames_world_box_without_gap_or_overlap() {
        let mut b = tiled_pack();
        // A 6x4 world box, the record's [x0, y0, x1, y1] in 16.16 units. Art
        // row 0 is the top, so y1 is where the first tile row starts.
        let at = 40 + 4 * 16;
        for (k, v) in [-3i32, -2, 3, 2].iter().enumerate() {
            b[at + 4 + k * 4..at + 8 + k * 4].copy_from_slice(&(v * 65536).to_le_bytes());
        }
        let room = Room::parse(&b).unwrap();
        let (_, cols, rows) = room.frame_grid(0);
        assert_eq!((cols, rows), (2, 2));
        let whole = core::array::from_fn::<_, 4, _>(|k| i32_at(room.frame(0), 4 + k * 4));
        let tiles: std::vec::Vec<_> = (0..4).map(|t| room.frame_tile(0, t).1).collect();
        // The union is the whole box.
        assert_eq!(tiles.iter().map(|t| t[0]).min(), Some(whole[0]));
        assert_eq!(tiles.iter().map(|t| t[1]).min(), Some(whole[1]));
        assert_eq!(tiles.iter().map(|t| t[2]).max(), Some(whole[2]));
        assert_eq!(tiles.iter().map(|t| t[3]).max(), Some(whole[3]));
        // Neighbours meet exactly: the 64-pixel cut is one world coordinate.
        assert_eq!(tiles[0][2], tiles[1][0]);
        assert_eq!(tiles[2][2], tiles[3][0]);
        assert_eq!(tiles[0][1], tiles[2][3]);
        assert_eq!(tiles[1][1], tiles[3][3]);
        // The split follows the pixels: 64 of 91 across, 64 of 89 down.
        assert_eq!(tiles[0][2] - tiles[0][0], (whole[2] - whole[0]) * 64 / 91);
        assert_eq!(tiles[0][3] - tiles[0][1], (whole[3] - whole[1]) * 64 / 89);
        // Areas sum to the whole, in the same proportion as the texels.
        let area = |t: [i32; 4]| (t[2] - t[0]) as i64 * (t[3] - t[1]) as i64;
        assert_eq!(tiles.iter().map(|&t| area(t)).sum::<i64>(), area(whole));
        // An untiled frame is one tile and the whole box.
        let untiled = pack(true);
        let plain = Room::parse(&untiled).unwrap();
        assert_eq!(plain.texture(0).tile_grid(), (1, 1));
    }
    /// A CLUT slot is a distinct palette, not a texture record. The host counts
    /// slots that way, so the reader has to keep accepting a region whose
    /// textures share one palette index; only an index outside the block is an
    /// error. The tiles of one frame are the case that matters: they are
    /// quantized together, so their entries are byte-identical.
    #[test]
    fn any_number_of_textures_may_name_one_palette() {
        let mut b = tiled_pack();
        for i in 0..4 {
            b[40 + i * 16 + 10..40 + i * 16 + 12].copy_from_slice(&0u16.to_le_bytes());
        }
        let room = Room::parse(&b).unwrap();
        assert!((0..4).all(|i| room.texture(i).palette == 0));
        // The block is still one entry per texture: sharing changes what the
        // records point at, never how the reader sizes the section.
        assert_eq!(room.palettes().len(), 4 * 32);
        // An index past the block is still refused, shared or not.
        b[50..52].copy_from_slice(&4u16.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
    }
    #[test]
    fn rejects_tile_grids_that_leave_the_slot_cache_or_the_texture_table() {
        // A grid naming more textures than the pack holds.
        let mut b = tiled_pack();
        b[42..44].copy_from_slice(&3u16.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        // A grid wider than the whole slot cache.
        let mut b = tiled_pack();
        b[42..44].copy_from_slice(&(MAX_TILE_GRID + 1).to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        // Half a grid: columns without rows names no tile at all.
        let mut b = tiled_pack();
        b[44..46].copy_from_slice(&0u16.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        // A one-tile grid, which is what an untiled texture already says with
        // 0,0; two spellings of the same frame would let a pack disagree.
        let mut b = tiled_pack();
        b[42..44].copy_from_slice(&1u16.to_le_bytes());
        b[44..46].copy_from_slice(&1u16.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        // A tile that carries a grid of its own.
        let mut b = tiled_pack();
        b[58..60].copy_from_slice(&2u16.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
    }
    #[test]
    fn rejects_truncation_and_bad_refs() {
        for streamed in [false, true] {
            let b = pack(streamed);
            let room = Room::parse(&b).unwrap();
            assert_eq!(room.byte_len(), b.len());
            for n in 0..b.len() {
                assert!(Room::parse(&b[..n]).is_err())
            }
        }
        let mut b = pack(false);
        b[40] = 1;
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
    }
    #[test]
    fn rejects_page_crossing_overflow_and_excess_counts() {
        let mut b = pack(false);
        b[42] = 255;
        b[46] = 2;
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        b[42..44].copy_from_slice(&65535u16.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        b[8] = 21;
        assert!(matches!(Room::parse(&b), Err(Error::Limit)));
    }
    #[test]
    fn streamed_rows_use_padded_stride_without_changing_width() {
        let mut b = pack(true);
        b[46] = 5;
        b[88..92].copy_from_slice(&[0x21, 0x43, 0x05, 0]);
        let room = Room::parse(&b).unwrap();
        let t = room.texture(0);
        assert!(t.is_streamed());
        assert_eq!(t.width, 5);
        assert_eq!(t.stream_stride(), 4);
        assert_eq!(room.stream_pixels(t), Some(&[0x21, 0x43, 0x05, 0][..]));
        assert_eq!(room.stream_byte_count(), 4);
        let b = pack(false);
        let room = Room::parse(&b).unwrap();
        assert!(room.stream_pixels(room.texture(0)).is_none());
    }
    #[test]
    fn rejects_malformed_stream_offsets_dimensions_and_header() {
        for (at, value) in [(52, 4u32), (52, u32::MAX), (44, 1), (46, 65), (48, 65)] {
            let mut b = pack(true);
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Room::parse(&b).is_err(), "offset {at}");
        }
        let mut b = pack(true);
        b[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Room::parse(&b), Err(Error::Limit)));
        let mut b = pack(true);
        b[36] = 2;
        assert!(matches!(Room::parse(&b), Err(Error::Header)));
        let mut b = pack(false);
        b[52] = 4;
        assert!(matches!(Room::parse(&b), Err(Error::Reference)));
        let mut b = pack(true);
        b.push(0);
        assert!(matches!(Room::parse(&b), Err(Error::Truncated)));
    }
    fn records_pack(counts: [usize; 6]) -> std::vec::Vec<u8> {
        let size = 40
            + counts[0] * 32768
            + counts[1] * 48
            + counts[2] * 44
            + counts[3] * 20
            + counts[4] * 16
            + counts[5] * 16;
        let mut b = vec![0; size];
        b[..8].copy_from_slice(b"HKROOM02");
        for (i, count) in counts.iter().enumerate() {
            b[8 + i * 4..12 + i * 4].copy_from_slice(&(*count as u32).to_le_bytes());
        }
        let mut at = 40;
        for _ in 0..counts[1] {
            b[at + 6] = 1;
            b[at + 8] = 1;
            at += 16;
        }
        for _ in 0..counts[2] {
            b[at + 4] = 1;
            at += 44;
        }
        at += counts[3] * 20;
        for _ in 0..counts[4] {
            b[at + 4] = 1;
            b[at + 8] = 1;
            at += 16;
        }
        b
    }
    fn incremental(b: &[u8], budget: usize) -> Result<(), Error> {
        let mut cursor = Validation::new(b)?;
        while !cursor.step(b, budget)? {}
        Ok(())
    }
    #[test]
    fn draw_material_legacy_and_black_average_are_bounded() {
        let mut b = records_pack([1, 1, 1, 0, 0, 0]);
        for mode in 0..=255 {
            b[40 + 16 + 43] = mode;
            let expected = if mode <= 1 {
                Ok(())
            } else {
                Err(Error::Reference)
            };
            assert_eq!(Room::parse(&b).map(|_| ()), expected);
            assert_eq!(incremental(&b, 1), expected);
        }
    }
    #[test]
    fn validation_resumes_across_every_table_without_exceeding_record_budget() {
        for counts in [[1, 10, 9, 11, 9, 9], [20, 640, 1024, 2048, 128, 1024]] {
            let b = records_pack(counts);
            assert!(Room::parse(&b).is_ok());
            let total: usize = counts[1..].iter().sum();
            for budget in [1, 2, 8, 31, usize::MAX] {
                let mut cursor = Validation::new(&b).unwrap();
                assert!(!cursor.step(&b, 0).unwrap());
                let mut checked = 0;
                loop {
                    let done = cursor.step(&b, budget).unwrap();
                    let now = counts[1..1 + cursor.section].iter().sum::<usize>() + cursor.index;
                    assert_eq!(now - checked, budget.min(total - checked));
                    checked = now;
                    assert_eq!(done, checked == total);
                    if done {
                        break;
                    }
                }
                assert!(cursor.step(&b, 0).unwrap());
                assert_eq!(cursor.view(&b).counts, counts);
            }
        }
    }
    #[test]
    fn incremental_preserves_exact_errors_and_table_order() {
        let valid = records_pack([1, 10, 9, 11, 9, 9]);
        let layout = Room::header(&valid).unwrap();
        let t = layout.offsets[0] + 9 * 16;
        let d = layout.offsets[1] + 8 * 44;
        let f = layout.offsets[2] + 10 * 20;
        let c = layout.offsets[3] + 8 * 16;
        let e = layout.offsets[4] + 8 * 16;
        let cases = [
            (36, 2, Error::Header),
            (8, 21, Error::Limit),
            (t, 1, Error::Reference),
            (t + 6, 0, Error::Reference),
            (t + 12, 4, Error::Reference),
            (d, 10, Error::Reference),
            (d + 2, 2, Error::Reference),
            (d + 4, 0, Error::Geometry),
            (d + 8, 8_000_001, Error::Geometry),
            (f, 10, Error::Reference),
            (f + 4, 16 * 65536 + 1, Error::Geometry),
            (c + 4, 0, Error::Reference),
            (c + 8, 120 * 65536 + 1, Error::Reference),
            (c + 12, 3, Error::Reference),
            (c + 12, 1 << 16, Error::Reference),
            (e, 512 * 65536 + 1, Error::Geometry),
        ];
        for (at, value, error) in cases {
            let mut b = valid.clone();
            b[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
            assert_eq!(Room::parse(&b).err(), Some(error), "offset {at}");
            for budget in [1, 8, 31] {
                assert_eq!(
                    incremental(&b, budget).err(),
                    Room::parse(&b).err(),
                    "offset {at}"
                );
            }
        }
        // A later reference error must not replace an earlier geometry error.
        let mut b = valid.clone();
        b[d + 4..d + 8].fill(0);
        b[f] = 10;
        assert_eq!(incremental(&b, 8), Err(Error::Geometry));
        b[t] = 1;
        assert_eq!(incremental(&b, 8), Err(Error::Reference));
        for n in [0, 7, 39, 40, valid.len() - 1] {
            assert_eq!(
                incremental(&valid[..n], 8).err(),
                Room::parse(&valid[..n]).err()
            );
        }
        // Layout never causes unchecked accesses if a caller substitutes a slice.
        let mut cursor = Validation::new(&valid).unwrap();
        assert_eq!(cursor.step(&valid[..40], 8), Err(Error::Truncated));
    }
    #[test]
    fn incremental_stream_checks_match_full_parser() {
        let valid = pack(true);
        for (at, value) in [(52, 4u32), (52, u32::MAX), (44, 1), (46, 65), (48, 65)] {
            let mut b = valid.clone();
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(incremental(&b, 1), Err(Error::Reference));
            assert_eq!(Room::parse(&b).err(), Some(Error::Reference));
        }
        assert_eq!(incremental(&valid, 1), Ok(()));
    }
    fn covered_pack() -> std::vec::Vec<u8> {
        let mut b = pack(false);
        b[36] = HAS_ALPHA_COVERS as u8;
        b[32] = ALPHA_COVER_BYTES as u8;
        b.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b
    }
    #[test]
    fn alpha_covers_accept_empty_full_256_and_touching_rectangles() {
        let mut b = covered_pack();
        let tail = b.len() - ALPHA_COVER_BYTES;
        assert_eq!(
            Room::parse(&b)
                .unwrap()
                .alpha_cover(Room::parse(&b).unwrap().texture(0)),
            Some(&b[tail..])
        );
        b[tail] = 0;
        assert!(Room::parse(&b).is_ok());
        b[46..48].copy_from_slice(&256u16.to_le_bytes());
        b[48..50].copy_from_slice(&256u16.to_le_bytes());
        b[tail] = 1;
        b[tail + 6] = 255;
        b[tail + 7] = 255;
        assert!(Room::parse(&b).is_ok());
        b[tail..].copy_from_slice(&[
            4, 0, 0, 0, 0, 0, 127, 127, 128, 0, 127, 127, 0, 128, 127, 127, 128, 128, 127, 127,
        ]);
        assert!(Room::parse(&b).is_ok());
        for budget in [1, 8, 31] {
            assert_eq!(incremental(&b, budget), Ok(()));
        }
        for streamed in [false, true] {
            let mut legacy = pack(streamed);
            let room = Room::parse(&legacy).unwrap();
            assert!(room.alpha_cover(room.texture(0)).is_none());
            if streamed {
                legacy[36] = 1;
                let room = Room::parse(&legacy).unwrap();
                assert!(room.alpha_cover(room.texture(0)).is_none());
            }
        }
    }
    #[test]
    fn alpha_cover_malformed_records_match_bounded_validation() {
        let valid = covered_pack();
        let tail = valid.len() - ALPHA_COVER_BYTES;
        for (at, value, error) in [
            (36, 2, Error::Header),
            (52, 1, Error::Reference),
            (52, 4, Error::Reference),
            (52, u32::MAX, Error::Reference),
            (tail, 5, Error::Reference),
            (tail + 1, 1, Error::Reference),
            (tail + 8, 1, Error::Reference),
            (tail + 4, 1, Error::Geometry),
            (tail + 6, 1, Error::Geometry),
        ] {
            let mut b = valid.clone();
            b[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
            assert_eq!(Room::parse(&b).err(), Some(error), "offset {at}");
            for budget in [1, 8] {
                assert_eq!(incremental(&b, budget).err(), Room::parse(&b).err());
            }
        }
        let mut overlap = valid.clone();
        overlap[tail] = 2;
        assert_eq!(Room::parse(&overlap).err(), Some(Error::Geometry));
        let mut truncated = valid.clone();
        truncated.pop();
        assert_eq!(Room::parse(&truncated).err(), Some(Error::Truncated));
        // A bad cover on a later texture is reached only on its budgeted step.
        let mut later = records_pack([1, 10, 0, 0, 0, 0]);
        later[36] = 1;
        later[32] = 200;
        for i in 0..10 {
            later[52 + i * 16..56 + i * 16].copy_from_slice(&((i * 20) as u32).to_le_bytes());
            later.extend_from_slice(&valid[tail..]);
        }
        let end = later.len();
        later[end - 20] = 5;
        let mut cursor = Validation::new(&later).unwrap();
        assert_eq!(cursor.step(&later, 8), Ok(false));
        assert_eq!(cursor.step(&later, 1), Ok(false));
        assert_eq!(cursor.step(&later, 1), Err(Error::Reference));
        assert_eq!(Room::parse(&later).err(), Some(Error::Reference));
    }
}
