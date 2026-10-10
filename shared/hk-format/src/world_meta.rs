//! Checked, allocation-free HKWMTA01 per-scene world metadata.
//!
//! The payload is deliberately a wire format.  It never casts a Rust struct,
//! slice, `usize`, `Option`, or pointer out of the bank.  All offsets are
//! absolute within the immutable bank and all element references are checked
//! before a view is returned.

use crate::{i32_at, u16_at, u32_at, Error};

const HEADER: usize = 160;
const SECTIONS: usize = 5;
const REGION_STRIDE: usize = 76;
const OBJECT_STRIDE: usize = 48;
const POLYGON_STRIDE: usize = 8;
const POINT_STRIDE: usize = 8;
const INDEX_STRIDE: usize = 2;
const MAX_REGIONS: usize = 4096;
const MAX_OBJECTS: usize = 65_535;
const MAX_POLYGONS: usize = 65_535;
const MAX_POINTS: usize = 262_140;
const MAX_INDICES: usize = 65_535;

/// Object kinds shared with tools/world_metadata.py KINDS.
pub const KIND_BREAKABLE: u16 = 1;
pub const KIND_GRASS: u16 = 2;
pub const KIND_HAZARD: u16 = 3;
pub const KIND_CHECKPOINT: u16 = 4;
/// One enemy placement. Flag 32 marks any recorded actor; flag 64 means the
/// guest has a controller for it, and then the object carries the whole
/// placement, because `SCENE_ACTORS[scene]` is a catalogue of enemy *types*
/// shared by every placement of one: payload word 0 is the index into it,
/// words 1 and 2 are the world x and y, and `state` is the scene-unique source
/// id, which for an object merged in from an additive scene is its shifted id
/// rather than the object's own. The remaining flag bits are the authored
/// switches: 1 initial facing right, 2 random start direction, 4 `startAlert`,
/// 8 the Climber's `start_right`, 128 `FSMActivator` (the enemy waits for the camera's ActiveRegion),
/// and bits 8..9 its rotation in quarter turns.
pub const KIND_ACTOR: u16 = 5;
/// A breakable's own mask fade; follows its breakable in the region's objects.
pub const KIND_MASK_FADE: u16 = 6;
/// A mask in this region faded by a breakable owned elsewhere (state = owner).
pub const KIND_REMOTE_MASK: u16 = 7;
/// One per region: interleaved (controller, draw) reveal binding pairs.
pub const KIND_REVEAL_BINDINGS: u16 = 8;
/// One per region: variant catalogue indices (debris, particle bank, impact).
pub const KIND_REGION_STATICS: u16 = 9;
/// Static NailSlash target: polygons, flag 1 = horizontal and up slashes too,
/// state = owning breakable id or 0xFFFFFFFF.
pub const KIND_POGO: u16 = 10;
/// CameraLockArea trigger; its one two-point polygon holds the camera-centre limits.
pub const KIND_CAMERA_LOCK: u16 = 11;
/// RestBench trigger; payload: seat x, seat y, Knight sit clip base.
pub const KIND_BENCH: u16 = 12;
/// BounceShroom trigger. The cooker admits only axis-aligned box colliders,
/// so the object's bounds are the exact shape the down slash has to meet and
/// no polygon or payload is carried.
pub const KIND_SHROOM: u16 = 13;
/// Talkable NPC: the object's bounds are its npc_control talk trigger and the
/// payload is its world x, y and the view's clip base for Idle, Talk Left and
/// Talk Right. Its conversation pages live in the guest's own cooked table,
/// found by this object's source id.
pub const KIND_NPC: u16 = 14;
/// A scene's TransitionPoints, all of them in the bank's first region because
/// a gate belongs to the scene rather than to one view. The object's bounds are
/// the gate's trigger, `flags` is the `Delay Collider` tick count, `state` packs
/// the destination as region | scene << 16 | side << 24, and the payload is the
/// destination spawn x, y and the entry velocity.
pub const KIND_GATE: u16 = 15;
/// A scene's enemy Geo payouts, beside the gates in the first region because
/// they are keyed by enemy source id across the whole scene. `state` packs the
/// small and medium drop counts, the payload is the effect origin x, y and the
/// large drop count, flag 1 is `megaFlingGeo`, and the bounds are unread.
pub const KIND_GEO_ENEMY: u16 = 16;
/// A scene's reveal-mask controllers, in the order the per-region
/// `KIND_REVEAL_BINDINGS` pairs index them, in the first region. `state` is
/// that controller index, the bounds are the trigger's AABB, the one polygon
/// is the exact trigger, the payload word is the fade duration in ticks, and
/// flag 1 is one-way while flag 2 means the mask starts covered.
pub const KIND_REVEAL_MASK: u16 = 17;
/// A hidden wall's or cracked floor's hit counter, right after its breakable
/// (which carries flag 8). `state` is the breakable's id; flags hold the family
/// (bits 0..2), the wall facing (3..4), the hits (5..8), whether a spell breaks
/// it (9) and whether the bounds are a hero range the Knight must stand in
/// (10); the polygons are the sagging planks' stage quads; payload word 0 is an
/// index list of (moving part, draw) pairs and words 1 and 2 the world point
/// the strike effect spawns at. host/secret_breaks.py, tools/world_metadata.py.
pub const KIND_SECRET: u16 = 18;

#[derive(Clone, Copy)]
pub struct WorldMeta<'a> {
    bytes: &'a [u8],
    offsets: [usize; SECTIONS],
    counts: [usize; SECTIONS],
    scene_id: u32,
    raw_fnv: u32,
}

impl<'a> WorldMeta<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        Self::parse_with(bytes, || {})
    }

    /// `parse` that calls `step` after each region's references are checked,
    /// so a guest can service its cooperative checkpoints inside a validation
    /// that grows with the scene's object count.
    pub fn parse_with(bytes: &'a [u8], mut step: impl FnMut()) -> Result<Self, Error> {
        let view = Self::header(bytes)?;
        let mut previous_global = 0;
        for index in 0..view.counts[0] {
            view.validate_region(index, &mut previous_global)?;
            step();
        }
        Ok(view)
    }

    /// # Safety
    /// `bytes` must be unchanged after `WorldMeta::parse` or a completed
    /// admission validation, for the complete lifetime of the returned view.
    pub unsafe fn validated_view(bytes: &'a [u8]) -> Self {
        let mut offsets = [0; SECTIONS];
        let mut counts = [0; SECTIONS];
        for section in 0..SECTIONS {
            offsets[section] = u32_at(bytes, 64 + section * 12) as usize;
            counts[section] = u32_at(bytes, 68 + section * 12) as usize;
        }
        Self {
            bytes,
            offsets,
            counts,
            scene_id: u32_at(bytes, 8),
            raw_fnv: u32_at(bytes, 12),
        }
    }

    fn header(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER || &bytes[..8] != b"HKWMTA01" {
            return Err(Error::Header);
        }
        if u32_at(bytes, 52) as usize != bytes.len()
            || u32_at(bytes, 56) != 0
            || u32_at(bytes, 60) as usize != SECTIONS
            || bytes[64 + SECTIONS * 12..HEADER]
                .iter()
                .any(|&byte| byte != 0)
        {
            return Err(Error::Header);
        }
        let mut offsets = [0; SECTIONS];
        let mut counts = [0; SECTIONS];
        let strides = [
            REGION_STRIDE,
            OBJECT_STRIDE,
            POLYGON_STRIDE,
            POINT_STRIDE,
            INDEX_STRIDE,
        ];
        let limits = [
            MAX_REGIONS,
            MAX_OBJECTS,
            MAX_POLYGONS,
            MAX_POINTS,
            MAX_INDICES,
        ];
        let mut next = HEADER;
        for section in 0..SECTIONS {
            let offset = u32_at(bytes, 64 + section * 12) as usize;
            let count = u32_at(bytes, 68 + section * 12) as usize;
            let stride = u32_at(bytes, 72 + section * 12) as usize;
            let aligned = next.checked_add(3).ok_or(Error::Truncated)? & !3;
            if offset != aligned
                || stride != strides[section]
                || count > limits[section]
                || offset > bytes.len()
                || bytes[next..offset].iter().any(|&byte| byte != 0)
            {
                return Err(Error::Header);
            }
            let end = offset
                .checked_add(count.checked_mul(stride).ok_or(Error::Truncated)?)
                .ok_or(Error::Truncated)?;
            if end > bytes.len() {
                return Err(Error::Truncated);
            }
            offsets[section] = offset;
            counts[section] = count;
            next = end;
        }
        let aligned = next.checked_add(3).ok_or(Error::Truncated)? & !3;
        if aligned != bytes.len() || bytes[next..].iter().any(|&byte| byte != 0) {
            return Err(Error::Truncated);
        }
        Ok(Self {
            bytes,
            offsets,
            counts,
            scene_id: u32_at(bytes, 8),
            raw_fnv: u32_at(bytes, 12),
        })
    }

    fn span(&self, section: usize, first: usize, count: usize) -> Result<(usize, usize), Error> {
        if section >= SECTIONS
            || first > self.counts[section]
            || count > self.counts[section] - first
        {
            return Err(Error::Reference);
        }
        let stride = [
            REGION_STRIDE,
            OBJECT_STRIDE,
            POLYGON_STRIDE,
            POINT_STRIDE,
            INDEX_STRIDE,
        ][section];
        let start = self.offsets[section]
            .checked_add(first.checked_mul(stride).ok_or(Error::Reference)?)
            .ok_or(Error::Reference)?;
        let end = start
            .checked_add(count.checked_mul(stride).ok_or(Error::Reference)?)
            .ok_or(Error::Reference)?;
        Ok((start, end))
    }

    fn validate_region(&self, index: usize, previous_global: &mut u32) -> Result<(), Error> {
        // The cooker emits the canonical global chunk order. Keeping that
        // order in the wire contract makes duplicate or missing region IDs
        // fail admission before any scene-owner mapping can consume them.
        let region = self.region(index).ok_or(Error::Reference)?;
        let global = region.global_id();
        if global == 0 || global <= *previous_global {
            return Err(Error::Reference);
        }
        *previous_global = global;
        self.span(1, region.object_first(), region.object_count())?;
        self.span(2, region.polygon_first(), region.polygon_count())?;
        self.span(3, region.neighbour_first(), region.neighbour_count())?;
        for object in region.objects() {
            let object = object?;
            self.span(2, object.polygon_first(), object.polygon_count())?;
            if object.polygon_count() > 16 {
                return Err(Error::Limit);
            }
            // Index-list spans live in the payload words of list-bearing kinds.
            let lists: &[usize] = match object.kind() {
                KIND_BREAKABLE => &[0, 1, 2],
                KIND_MASK_FADE | KIND_REMOTE_MASK | KIND_REVEAL_BINDINGS | KIND_SECRET => &[0],
                _ => &[],
            };
            for &word in lists {
                let (first, count) = object.index_span(word);
                self.span(4, first, count)?;
            }
            // A camera lock's single "polygon" is its two limit points.
            let minimum = if object.kind() == KIND_CAMERA_LOCK {
                2
            } else {
                3
            };
            for polygon in object.polygons() {
                let polygon = polygon?;
                if polygon.count() < minimum || polygon.count() > 16 {
                    return Err(Error::Geometry);
                }
                self.span(3, polygon.first(), polygon.count())?;
            }
        }
        for neighbour in region.neighbours() {
            // Neighbours are global region IDs.  A scene bank cannot
            // compare them with its local region count (Town, for
            // example, points into Tutorial), but zero is never a valid
            // global catalogue owner.
            if neighbour? == 0 {
                return Err(Error::Reference);
            }
        }
        Ok(())
    }

    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }
    pub fn raw_fnv(&self) -> u32 {
        self.raw_fnv
    }
    pub fn region_count(&self) -> usize {
        self.counts[0]
    }
    pub fn object_count(&self) -> usize {
        self.counts[1]
    }
    pub fn fingerprint(&self) -> &'a [u8] {
        &self.bytes[16..48]
    }
    /// Bind a decoded bank to the manifest entry that requested it before any
    /// borrowed view is handed to gameplay or rendering.
    pub fn check_identity(
        &self,
        scene_id: u32,
        raw_fnv: u32,
        fingerprint: &[u8; 32],
    ) -> Result<(), Error> {
        if self.scene_id != scene_id || self.raw_fnv != raw_fnv || self.fingerprint() != fingerprint
        {
            return Err(Error::Reference);
        }
        Ok(())
    }
    pub fn region(&self, index: usize) -> Option<Region<'a>> {
        if index >= self.counts[0] {
            return None;
        }
        Some(Region { meta: *self, index })
    }
    /// Resolve a global chunk identity without assuming scene-local IDs are
    /// contiguous or start at zero.
    pub fn region_by_global_id(&self, global_id: u32) -> Option<Region<'a>> {
        (0..self.counts[0]).find_map(|index| {
            let region = self.region(index)?;
            (region.global_id() == global_id).then_some(region)
        })
    }
    pub fn regions(&self) -> Regions<'a> {
        Regions {
            meta: *self,
            index: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Regions<'a> {
    meta: WorldMeta<'a>,
    index: usize,
}
impl<'a> Iterator for Regions<'a> {
    type Item = Region<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let value = self.meta.region(self.index);
        self.index += usize::from(value.is_some());
        value
    }
}

#[derive(Clone, Copy)]
pub struct Region<'a> {
    meta: WorldMeta<'a>,
    index: usize,
}
impl<'a> Region<'a> {
    fn at(&self) -> usize {
        self.meta.offsets[0] + self.index * REGION_STRIDE
    }
    pub fn global_id(&self) -> u32 {
        u32_at(self.meta.bytes, self.at())
    }
    pub fn bounds(&self) -> [i32; 4] {
        quad(self.meta.bytes, self.at() + 4)
    }
    pub fn collision_bounds(&self) -> [i32; 4] {
        quad(self.meta.bytes, self.at() + 20)
    }
    pub fn camera(&self) -> [i32; 4] {
        quad(self.meta.bytes, self.at() + 36)
    }
    pub fn neighbour_first(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 52) as usize
    }
    pub fn neighbour_count(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 56) as usize
    }
    pub fn object_first(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 60) as usize
    }
    pub fn object_count(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 64) as usize
    }
    pub fn polygon_first(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 68) as usize
    }
    pub fn polygon_count(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 72) as usize
    }
    pub fn neighbours(&self) -> Neighbours<'a> {
        Neighbours {
            meta: self.meta,
            first: self.neighbour_first(),
            index: 0,
            count: self.neighbour_count(),
        }
    }
    pub fn objects(&self) -> Objects<'a> {
        Objects {
            meta: self.meta,
            first: self.object_first(),
            index: 0,
            count: self.object_count(),
        }
    }
    /// The region's `local`th object; admission checked the span.
    pub fn object(&self, local: usize) -> Option<Object<'a>> {
        if local >= self.object_count() {
            return None;
        }
        self.meta.object(self.object_first() + local)
    }
}

pub struct Neighbours<'a> {
    meta: WorldMeta<'a>,
    first: usize,
    index: usize,
    count: usize,
}
impl<'a> Iterator for Neighbours<'a> {
    type Item = Result<u32, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let at = self.meta.offsets[3] + (self.first + self.index) * POINT_STRIDE;
        self.index += 1;
        Some(Ok(u32_at(self.meta.bytes, at)))
    }
}

pub struct Objects<'a> {
    meta: WorldMeta<'a>,
    first: usize,
    index: usize,
    count: usize,
}
impl<'a> Iterator for Objects<'a> {
    type Item = Result<Object<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let index = self.first + self.index;
        self.index += 1;
        Some(self.meta.object(index).ok_or(Error::Reference))
    }
}

impl<'a> WorldMeta<'a> {
    fn object(&self, index: usize) -> Option<Object<'a>> {
        if index >= self.counts[1] {
            return None;
        }
        Some(Object { meta: *self, index })
    }
    fn polygon(&self, index: usize) -> Option<Polygon<'a>> {
        if index >= self.counts[2] {
            return None;
        }
        Some(Polygon { meta: *self, index })
    }
}

#[derive(Clone, Copy)]
pub struct Object<'a> {
    meta: WorldMeta<'a>,
    index: usize,
}
impl<'a> Object<'a> {
    fn at(&self) -> usize {
        self.meta.offsets[1] + self.index * OBJECT_STRIDE
    }
    pub fn source_id(&self) -> u32 {
        u32_at(self.meta.bytes, self.at())
    }
    pub fn state_id(&self) -> u32 {
        u32_at(self.meta.bytes, self.at() + 4)
    }
    pub fn kind(&self) -> u16 {
        u16_at(self.meta.bytes, self.at() + 8)
    }
    pub fn flags(&self) -> u16 {
        u16_at(self.meta.bytes, self.at() + 10)
    }
    pub fn bounds(&self) -> [i32; 4] {
        quad(self.meta.bytes, self.at() + 12)
    }
    pub fn polygon_first(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 28) as usize
    }
    pub fn polygon_count(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 32) as usize
    }
    /// Kind-specific payload words. Hazards: origin x, damage | respawn << 16.
    /// Checkpoints: spawn x, spawn y, facing.
    pub fn extra(&self, index: usize) -> i32 {
        i32_at(self.meta.bytes, self.at() + 36 + (index % 3) * 4)
    }
    pub fn polygons(&self) -> Polygons<'a> {
        Polygons {
            meta: self.meta,
            first: self.polygon_first(),
            index: 0,
            count: self.polygon_count(),
        }
    }
    /// Payload word `word` as an index-list span: first in the low half,
    /// count in the high half.
    pub fn index_span(&self, word: usize) -> (usize, usize) {
        let value = self.extra(word) as u32;
        ((value & 0xFFFF) as usize, (value >> 16) as usize)
    }
    /// The u16 list a payload word spans; admission checked the span.
    pub fn indices(&self, word: usize) -> Indices<'a> {
        let (first, count) = self.index_span(word);
        Indices {
            meta: self.meta,
            first,
            index: 0,
            count,
        }
    }
}

pub struct Indices<'a> {
    meta: WorldMeta<'a>,
    first: usize,
    index: usize,
    count: usize,
}
impl<'a> Iterator for Indices<'a> {
    type Item = u16;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let index = self.first + self.index;
        self.index += 1;
        if index >= self.meta.counts[4] {
            return None;
        }
        Some(u16_at(
            self.meta.bytes,
            self.meta.offsets[4] + index * INDEX_STRIDE,
        ))
    }
}

pub struct Polygons<'a> {
    meta: WorldMeta<'a>,
    first: usize,
    index: usize,
    count: usize,
}
impl<'a> Iterator for Polygons<'a> {
    type Item = Result<Polygon<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let index = self.first + self.index;
        self.index += 1;
        Some(self.meta.polygon(index).ok_or(Error::Reference))
    }
}

#[derive(Clone, Copy)]
pub struct Polygon<'a> {
    meta: WorldMeta<'a>,
    index: usize,
}
impl<'a> Polygon<'a> {
    fn at(&self) -> usize {
        self.meta.offsets[2] + self.index * POLYGON_STRIDE
    }
    pub fn first(&self) -> usize {
        u32_at(self.meta.bytes, self.at()) as usize
    }
    pub fn count(&self) -> usize {
        u32_at(self.meta.bytes, self.at() + 4) as usize
    }
    pub fn points(&self) -> Points<'a> {
        Points {
            meta: self.meta,
            first: self.first(),
            index: 0,
            count: self.count(),
        }
    }
}

pub struct Points<'a> {
    meta: WorldMeta<'a>,
    first: usize,
    index: usize,
    count: usize,
}
impl<'a> Iterator for Points<'a> {
    type Item = Result<[i32; 2], Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let index = self.first + self.index;
        self.index += 1;
        if index >= self.meta.counts[3] {
            return Some(Err(Error::Reference));
        }
        let at = self.meta.offsets[3] + index * POINT_STRIDE;
        Some(Ok([
            i32_at(self.meta.bytes, at),
            i32_at(self.meta.bytes, at + 4),
        ]))
    }
}

fn quad(bytes: &[u8], at: usize) -> [i32; 4] {
    [
        i32_at(bytes, at),
        i32_at(bytes, at + 4),
        i32_at(bytes, at + 8),
        i32_at(bytes, at + 12),
    ]
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{vec, vec::Vec};
    fn put(b: &mut [u8], at: usize, value: u32) {
        b[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture() -> Vec<u8> {
        let counts = [1, 1, 1, 3, 2];
        let strides = [
            REGION_STRIDE,
            OBJECT_STRIDE,
            POLYGON_STRIDE,
            POINT_STRIDE,
            INDEX_STRIDE,
        ];
        let mut offsets = [0; SECTIONS];
        let mut end = HEADER;
        for i in 0..SECTIONS {
            offsets[i] = end;
            end += counts[i] * strides[i];
        }
        let total = (end + 3) & !3;
        let mut bytes = vec![0; total];
        bytes[..8].copy_from_slice(b"HKWMTA01");
        put(&mut bytes, 8, 4);
        put(&mut bytes, 12, 9);
        put(&mut bytes, 52, total as u32);
        put(&mut bytes, 60, SECTIONS as u32);
        for i in 0..SECTIONS {
            put(&mut bytes, 64 + i * 12, offsets[i] as u32);
            put(&mut bytes, 68 + i * 12, counts[i] as u32);
            put(&mut bytes, 72 + i * 12, strides[i] as u32);
        }
        put(&mut bytes, offsets[0], 77);
        put(&mut bytes, offsets[0] + 56, 0);
        put(&mut bytes, offsets[0] + 60, 0);
        put(&mut bytes, offsets[0] + 64, 1);
        put(&mut bytes, offsets[0] + 68, 0);
        put(&mut bytes, offsets[0] + 72, 1);
        put(&mut bytes, offsets[1], 123);
        put(&mut bytes, offsets[1] + 4, 456);
        bytes[offsets[1] + 8..offsets[1] + 10].copy_from_slice(&KIND_MASK_FADE.to_le_bytes());
        put(&mut bytes, offsets[1] + 28, 0);
        put(&mut bytes, offsets[1] + 32, 1);
        put(&mut bytes, offsets[1] + 36, 2 << 16); // two indices from 0
        put(&mut bytes, offsets[1] + 44, 3);
        bytes[offsets[4]..offsets[4] + 2].copy_from_slice(&40u16.to_le_bytes());
        bytes[offsets[4] + 2..offsets[4] + 4].copy_from_slice(&41u16.to_le_bytes());
        put(&mut bytes, offsets[2], 0);
        put(&mut bytes, offsets[2] + 4, 3);
        for i in 0..3 {
            put(&mut bytes, offsets[3] + i * 8, (i * 65536) as u32);
            put(&mut bytes, offsets[3] + i * 8 + 4, (i * 2 * 65536) as u32);
        }
        bytes
    }
    #[test]
    fn checked_views_decode_wire_scalars() {
        let bytes = fixture();
        let bank = WorldMeta::parse(&bytes).unwrap();
        let r = bank.region(0).unwrap();
        assert_eq!(r.global_id(), 77);
        assert_eq!(bank.region_by_global_id(77).unwrap().global_id(), 77);
        assert!(bank.check_identity(5, 9, &[0; 32]).is_err());
        let o = r.objects().next().unwrap().unwrap();
        assert_eq!(o.source_id(), 123);
        assert_eq!((o.extra(1), o.extra(2)), (0, 3));
        assert_eq!(o.indices(0).collect::<Vec<u16>>(), vec![40, 41]);
        assert_eq!(o.polygons().next().unwrap().unwrap().points().count(), 3);
    }
    #[test]
    fn rejects_wrong_stride_and_trailing_bytes() {
        let mut b = fixture();
        put(&mut b, 72, 99);
        assert_eq!(WorldMeta::parse(&b).err(), Some(Error::Header));
        let mut b = fixture();
        b.push(1);
        assert_eq!(WorldMeta::parse(&b).err(), Some(Error::Header));
    }
    #[test]
    fn rejects_out_of_range_polygon_points() {
        let mut b = fixture();
        let at = offset(&b, 1) + 28;
        put(&mut b, at, 99);
        assert_eq!(WorldMeta::parse(&b).err(), Some(Error::Reference));
    }
    #[test]
    fn rejects_index_list_span_outside_the_index_section() {
        let mut b = fixture();
        let at = offset(&b, 1) + 36;
        put(&mut b, at, 3 << 16);
        assert_eq!(WorldMeta::parse(&b).err(), Some(Error::Reference));
    }
    #[test]
    fn rejects_zero_global_region_id() {
        let mut b = fixture();
        let at = offset(&b, 0);
        put(&mut b, at, 0);
        assert_eq!(WorldMeta::parse(&b).err(), Some(Error::Reference));
    }
    fn offset(b: &[u8], section: usize) -> usize {
        u32_at(b, 64 + section * 12) as usize
    }
}
