//! Bounded animation working set on the pinned SDK's slot cache.
#![no_std]
#[cfg(test)] extern crate std;
pub mod residency;
use psx_cache::SlotCache;

/// Eight slots in the 32x256 strip at x320 and sixteen in the texture page a
/// static scenery page gave up; `residency::ANIMATION_REGIONS` holds the
/// measurement and `residency::ANIMATION_SLOTS` must agree with this.
pub const SLOTS: usize = 24;
pub const MAX_REQUESTS: usize = 24;
/// Global texture IDs in an admitted resident scene.
pub const MAX_KEYS: usize = 2048;
/// The whole cache, which is what one frame may replace: a cap, not a target.
/// A frame only uploads its misses.
pub const MAX_UPLOAD_BYTES: u32 = 49152;

/// One slot is 64x64 4bpp. A frame larger than that binds a rectangle of slots
/// and the draw submits one quad per tile, because no slot can be bigger than
/// a texture page's own 64x64 grid step.
pub const TILE: usize = 64;
/// Slots the rest of the frame always keeps: the Knight's body or ability pose,
/// the nail effect, the Hollow Shade and the Vengeful Spirit ball are the four
/// keys `main.rs` can request beside the view's own actor or NPC.
pub const RESERVED_SLOTS: usize = 4;
/// The largest tile rectangle one frame may bind. Growing this needs more
/// slots, and the next sixteen slots cost another static scenery page:
/// `residency::SPARE_HALFWORDS` says the gaps hold none.
pub const MAX_FRAME_TILES: usize = SLOTS - RESERVED_SLOTS;
/// Tile columns and rows a frame of this pixel size binds.
pub const fn tile_grid(width: usize, height: usize) -> (usize, usize) {
    (width.div_ceil(TILE), height.div_ceil(TILE))
}
/// Slots a frame of this pixel size occupies while it is drawn.
pub const fn frame_tiles(width: usize, height: usize) -> usize {
    let (cols, rows) = tile_grid(width, height);
    cols * rows
}
/// Pixel origin and size of one tile of a `width` by `height` frame, in the
/// row-major order the cooker emits and the consecutive texture IDs follow.
pub const fn tile_rect(
    width: usize,
    height: usize,
    col: usize,
    row: usize,
) -> (usize, usize, usize, usize) {
    let (x, y) = (col * TILE, row * TILE);
    let w = if width - x < TILE { width - x } else { TILE };
    let h = if height - y < TILE { height - y } else { TILE };
    (x, y, w, h)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub hits: u32,
    pub misses: u32,
    pub upload_bytes: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Phase,
    WorkingSet,
    Upload,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Prepared,
    InFlight,
}
pub struct Cache {
    slots: SlotCache<u8, SLOTS, MAX_KEYS>,
    keys: [u16; MAX_REQUESTS],
    key_count: usize,
    phase: Phase,
}
impl Cache {
    pub const fn new() -> Self {
        Self {
            slots: SlotCache::new(),
            keys: [0; MAX_REQUESTS],
            key_count: 0,
            phase: Phase::Idle,
        }
    }
    /// Upload closure receives key and physical slot. Its successful byte count
    /// must be 1..=2048. Caller validates dimensions before issuing GPU writes.
    /// Failure leaves no usable reference to a partially uploaded slot.
    pub fn prepare(
        &mut self,
        keys: &[u16],
        mut upload: impl FnMut(u16, usize) -> Option<u32>,
    ) -> Result<Stats, Error> {
        if self.phase != Phase::Idle {
            return Err(Error::Phase);
        }
        let mut unique = [0; MAX_REQUESTS];
        let mut count = 0;
        for &key in keys {
            if key as usize >= MAX_KEYS {
                return Err(Error::WorkingSet);
            }
            if !unique[..count].contains(&key) {
                if count == MAX_REQUESTS {
                    return Err(Error::WorkingSet);
                }
                unique[count] = key;
                count += 1;
            }
        }
        self.slots.bump_epoch();
        // Pin every resident request BEFORE the first miss can evict anything.
        self.slots.set_pinned(&unique[..count]);
        let mut stats = Stats::default();
        for &key in &unique[..count] {
            if self.slots.get(key).is_some() {
                stats.hits += 1;
                continue;
            }
            let slot = self.slots.reserve(key).ok_or(Error::WorkingSet)?;
            match upload(key, slot) {
                Some(bytes) if (1..=2048).contains(&bytes) => {
                    self.slots.mark_ready(slot, slot as u8);
                    self.slots.pin(key);
                    stats.misses += 1;
                    stats.upload_bytes += bytes;
                }
                _ => {
                    self.slots.evict(key);
                    self.slots.unpin_all();
                    return Err(Error::Upload);
                }
            }
        }
        self.keys = unique;
        self.key_count = count;
        self.phase = Phase::Prepared;
        Ok(stats)
    }
    /// Only a member of the declared frame working set may be drawn.
    pub fn slot(&self, key: u16) -> Option<usize> {
        if self.phase == Phase::Idle || !self.keys[..self.key_count].contains(&key) {
            return None;
        }
        self.slots.peek(key).map(|&slot| slot as usize)
    }
    pub fn submit(&mut self) -> Result<(), Error> {
        if self.phase != Phase::Prepared {
            return Err(Error::Phase);
        }
        self.phase = Phase::InFlight;
        Ok(())
    }
    /// Caller must observe DMA completion AND GPU raster completion first.
    pub fn complete(&mut self) -> Result<(), Error> {
        if self.phase != Phase::InFlight {
            return Err(Error::Phase);
        }
        self.slots.unpin_all();
        self.phase = Phase::Idle;
        self.key_count = 0;
        Ok(())
    }
}
impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(c: &mut Cache, keys: &[u16]) -> Stats {
        let stats = c.prepare(keys, |_, _| Some(2048)).unwrap();
        c.submit().unwrap();
        c.complete().unwrap();
        stats
    }
    #[test]
    fn repeated_frames_and_duplicate_requests_do_not_upload() {
        let mut c = Cache::new();
        assert_eq!(frame(&mut c, &[1, 1]).misses, 1);
        assert_eq!(frame(&mut c, &[1]).hits, 1);
        assert_eq!(frame(&mut c, &[1]).upload_bytes, 0);
    }
    #[test]
    fn global_scene_keys_remain_distinct_and_reusable_across_frame_sets() {
        let mut c=Cache::new();
        assert_eq!(frame(&mut c,&[0,640,1114,2047]).misses,4);
        let stats=c.prepare(&[2047,640,0,1114],|_,_|panic!("resident global key uploaded again")).unwrap();
        assert_eq!(stats.hits,4);assert_eq!(stats.upload_bytes,0);
        let slots=[c.slot(0),c.slot(640),c.slot(1114),c.slot(2047)];
        for i in 0..slots.len(){for j in 0..i{assert_ne!(slots[i],slots[j]);}}
        c.submit().unwrap();c.complete().unwrap();
        assert_eq!(c.prepare(&[u16::MAX],|_,_|panic!()),Err(Error::WorkingSet));
        assert_eq!(frame(&mut c,&[2047]).hits,1);
        // A real scene replacement clears global IDs before new bytes reuse them.
        c=Cache::new();assert_eq!(frame(&mut c,&[2047]).misses,1);
    }
    #[test]
    fn incoming_hit_is_pinned_before_lru_miss() {
        let mut c = Cache::new();
        // Keys 1 and 2 land first, so they are the least recently used once
        // the rest of the slots fill behind them.
        frame(&mut c, &[1, 2]);
        let rest: std::vec::Vec<u16> = (3..=SLOTS as u16).collect();
        for chunk in rest.chunks(MAX_REQUESTS) {
            frame(&mut c, chunk);
        }
        // 1 is an oldest slot, and appears AFTER the miss in this request.
        let incoming = SLOTS as u16 + 1;
        let stats = c.prepare(&[incoming, 1], |_, _| Some(2048)).unwrap();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_ne!(c.slot(incoming), c.slot(1));
        assert_eq!(c.slot(2), None); // undeclared texture cannot be drawn
        c.submit().unwrap();
        c.complete().unwrap();
        assert_eq!(frame(&mut c, &[2]).misses, 1); // 2 was the LRU victim
    }
    #[test]
    fn no_inflight_upload_or_double_prepare() {
        let mut c = Cache::new();
        c.prepare(&[1], |_, _| Some(2048)).unwrap();
        assert_eq!(c.prepare(&[2], |_, _| panic!()), Err(Error::Phase));
        c.submit().unwrap();
        assert_eq!(c.prepare(&[2], |_, _| panic!()), Err(Error::Phase));
        c.complete().unwrap();
        assert_eq!(c.complete(), Err(Error::Phase));
        assert_eq!(c.slot(1), None);
    }
    #[test]
    fn invalid_working_set_rejected_before_any_upload() {
        let mut c = Cache::new();
        let over: std::vec::Vec<u16> = (1..=MAX_REQUESTS as u16 + 1).collect();
        assert_eq!(c.prepare(&over, |_, _| panic!()), Err(Error::WorkingSet));
        assert_eq!(
            c.prepare(&[MAX_KEYS as u16], |_, _| panic!()),
            Err(Error::WorkingSet)
        );
        let full: std::vec::Vec<u16> = (1..=MAX_REQUESTS as u16).collect();
        assert_eq!(frame(&mut c, &full).upload_bytes, MAX_UPLOAD_BYTES);
    }
    #[test]
    fn a_frame_binds_a_rectangle_of_slots_and_covers_every_texel_once() {
        assert_eq!(frame_tiles(64, 64), 1);
        assert_eq!(tile_grid(64, 64), (1, 1));
        // Crossroads_47's Stag, measured through host/cook.py's own art path:
        // its Idle largest frame is 91x89 at the placement's scale.
        assert_eq!(tile_grid(91, 89), (2, 2));
        assert_eq!(frame_tiles(91, 89), 4);
        // Every tile is inside the frame, no two overlap and together they are
        // the whole frame. A 65-pixel edge must give a 1-pixel tile, not 64.
        for (w, h) in [(91, 89), (65, 1), (1, 65), (198, 169), (256, 256)] {
            let (cols, rows) = tile_grid(w, h);
            let mut covered = 0;
            for row in 0..rows {
                for col in 0..cols {
                    let (x, y, tw, th) = tile_rect(w, h, col, row);
                    assert!(tw >= 1 && tw <= TILE && th >= 1 && th <= TILE);
                    assert!(x + tw <= w && y + th <= h);
                    assert_eq!((x, y), (col * TILE, row * TILE));
                    covered += tw * th;
                }
            }
            assert_eq!(covered, w * h);
        }
    }
    #[test]
    fn the_frame_tile_budget_is_what_the_slots_leave() {
        // The slot count is the regions', and the upload cap is the whole
        // cache. Both have to hold somewhere else too, or they drift.
        assert_eq!(SLOTS, residency::ANIMATION_SLOTS);
        assert_eq!(MAX_UPLOAD_BYTES as usize, SLOTS * TILE * TILE / 2);
        assert_eq!(MAX_REQUESTS, SLOTS);
        assert_eq!(MAX_FRAME_TILES, 20);
        assert!(MAX_FRAME_TILES + RESERVED_SLOTS <= SLOTS);
        assert!(frame_tiles(91, 89) <= MAX_FRAME_TILES);
        // The False Knight's largest frame, 198x169 through host/cook.py's own
        // actor art path. Twelve tiles beside the four reserved keys is 16 of
        // the 24 slots, 32,768 bytes of the 49,152 the cache now holds.
        assert_eq!(frame_tiles(198, 169), 12);
        assert!(frame_tiles(198, 169) <= MAX_FRAME_TILES);
        assert_eq!((frame_tiles(198, 169) + RESERVED_SLOTS) * 2048, 32768);
        assert!(frame_tiles(198, 169) + RESERVED_SLOTS <= SLOTS);
        // No frame of the fight is wider than 204 or taller than 188, so four
        // by three tiles bounds all 110 of its sprites, not just that one.
        assert_eq!(frame_tiles(204, 188), 12);
        assert!(frame_tiles(204, 188) + RESERVED_SLOTS <= SLOTS);
    }
    #[test]
    fn every_slot_of_a_full_frame_fits_one_prepare(){
        let mut c=Cache::new();
        // The largest tile rectangle plus the four reserved keys is the whole
        // cache, and the tile keys are consecutive because the cooker emits
        // them that way.
        let mut keys=[0u16;SLOTS];
        for (i,k) in keys.iter_mut().enumerate(){*k=i as u16;}
        let stats=frame(&mut c,&keys);
        assert_eq!(stats.misses,SLOTS as u32);
        assert_eq!(stats.upload_bytes,MAX_UPLOAD_BYTES);
        let mut over=[0u16;SLOTS+1];
        for (i,k) in over.iter_mut().enumerate(){*k=i as u16;}
        assert_eq!(c.prepare(&over,|_,_|panic!()),Err(Error::WorkingSet));
    }
    #[test]
    fn failed_upload_is_not_resident_and_can_be_retried() {
        let mut c = Cache::new();
        assert_eq!(c.prepare(&[1], |_, _| None), Err(Error::Upload));
        assert_eq!(c.slot(1), None);
        assert_eq!(c.prepare(&[1], |_, _| Some(2049)), Err(Error::Upload));
        assert_eq!(frame(&mut c, &[1]).misses, 1);
    }
}
