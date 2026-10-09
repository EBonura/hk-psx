//! Four independently prepared scenery banks. The active bank is immutable.
pub const BANKS: usize = 4;
pub const PAGES: usize = 5;
pub const CLUTS: usize = 416;
// Current resident scenes consume1206 CLUTs. Keep their admitted namespace
// below the last legacy strip, whose space now holds independent Geo art.
pub const SCENE_CLUTS: usize = 3 * CLUTS;
/// Geo's independent 4bpp art/CLUT allocation in previously unused gaps.
/// Each tuple is x,y,width,height in VRAM halfwords, not texture pixels.
pub const GEO_RECTS: [(usize, usize, usize, usize); 5] = [
    (320, 464, 64, 16),
    (352, 240, 32, 16),
    (352, 485, 32, 27),
    (320, 496, 32, 16),
    (352, 32, 8, 32),
];
/// Lifeblood and independent break-effect atlases, including their CLUTs.
pub const LIFE_RECTS: [(usize, usize, usize, usize); 1] = [(352, 96, 32, 80)];
/// Static scenery pages this build reserves, out of the `BANKS*PAGES` the
/// layout can address. The twentieth belongs to the animation cache instead.
/// Measured over all 45 cooked scenes in `.hkpsx/packed-scenes.json`: the
/// largest, Tutorial_01, is 18 pages and the next largest, Crossroads_ShamanTemple,
/// is 9, so pages 18 and 19 were reserved and no scene has ever uploaded
/// either. Page 18 is what remains of that spare, and it is shared: see
/// `MAP_PAGE`.
pub const STATIC_PAGES: usize = BANKS * PAGES - 1;
/// The quick map's art (`data/game-map.bin`, `game/src/game_map.rs`), with the
/// Snail Shaman's and its own palettes in the page's last row: static page 18,
/// which only a scene that needs all nineteen pages uploads (the False
/// Knight's arena). The map is uploaded before the title, lost while such a
/// scene is admitted, and read back at the next gate into a scene that leaves
/// page 18 alone. x,y,width,height in halfwords.
pub const MAP_PAGE: (usize, usize, usize, usize) = (384 + (18 % 10) * 64, (18 / 10) * 256, 64, 256);
/// A 64x64 4bpp slot: 16 halfwords wide and 64 rows tall.
pub const SLOT_HALFWORDS: (usize, usize) = (16, 64);
/// The animation cache's VRAM, as x,y,width,height in halfwords. A region
/// holds `width/16` slots across and `height/64` down, and slot indices walk
/// the regions in order, row-major inside each.
///
/// The first is the original 32x256 strip at x320, two slots across and four
/// down, boxed in by CLUT bank 1 below it and the SOUL, Geo, Lifeblood and
/// break-effect atlases at x352. It cannot grow: `SPARE_HALFWORDS` measures
/// 304 free halfwords in two fragments 16 halfwords wide, against a slot's 16
/// by 64. The second is static scenery page 19's texture page, four slots
/// across and four down, which is where the other sixteen came from.
///
/// Every region starts on a texture page origin, so a slot never straddles two
/// texture pages and its pixel UV always fits the u8 a primitive carries.
pub const ANIMATION_REGIONS: [(usize, usize, usize, usize); 2] =
    [(320, 0, 32, 256), (960, 256, 64, 256)];
/// Slots the regions physically hold, which is what `crate::SLOTS` must equal.
pub const ANIMATION_SLOTS: usize = {
    let (mut total, mut i) = (0, 0);
    while i < ANIMATION_REGIONS.len() {
        let (_, _, w, h) = ANIMATION_REGIONS[i];
        total += (w / SLOT_HALFWORDS.0) * (h / SLOT_HALFWORDS.1);
        i += 1;
    }
    total
};
/// Texture page origin and pixel UV of one slot: `(tpage_x, tpage_y, u, v)`,
/// the four numbers a draw needs and, as `(tpage_x+u/4, tpage_y+v)`, the
/// halfword origin its upload needs.
pub const fn slot_placement(slot: usize) -> (u16, u16, u16, u16) {
    let (mut slot, mut i) = (slot, 0);
    while i < ANIMATION_REGIONS.len() {
        let (x, y, w, h) = ANIMATION_REGIONS[i];
        let (cols, rows) = (w / SLOT_HALFWORDS.0, h / SLOT_HALFWORDS.1);
        if slot < cols * rows {
            return (
                x as u16,
                y as u16,
                ((slot % cols) * 64) as u16,
                ((slot / cols) * 64) as u16,
            );
        }
        slot -= cols * rows;
        i += 1;
    }
    panic!("animation slot beyond ANIMATION_REGIONS")
}
/// Halfwords `all_vram_allocations_are_disjoint` leaves unclaimed. Measured,
/// not budgeted: the two free fragments are 16x14 and 16x5 halfwords, both in
/// the CLUT rows below y480, and one animation slot is 16x64. So no further
/// slot can come from the gaps; the next one costs another scenery page.
pub const SPARE_HALFWORDS: usize = 304;
pub const BREAK_RECTS: [(usize, usize, usize, usize); 3] =
    [(352, 176, 32, 64), (360, 32, 24, 32), (352, 64, 32, 32)];
#[derive(Clone, Copy)]
struct Entry {
    region: Option<usize>,
    ready: bool,
    stamp: u32,
}
pub struct Banks {
    entries: [Entry; BANKS],
    active: Option<usize>,
    pending: Option<usize>,
    clock: u32,
}
impl Banks {
    pub const fn new() -> Self {
        Self {
            entries: [Entry {
                region: None,
                ready: false,
                stamp: 0,
            }; BANKS],
            active: None,
            pending: None,
            clock: 0,
        }
    }
    pub fn find(&self, region: usize) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.region == Some(region) && e.ready)
    }
    pub fn active(&self) -> Option<usize> {
        self.active
    }
    pub fn pending(&self) -> Option<(usize, usize)> {
        self.pending
            .map(|bank| (bank, self.entries[bank].region.unwrap()))
    }
    /// Retire an unrelated partial bank at a GPU-frame boundary. It was never
    /// ready or active; discarded contents cannot become selectable afterward.
    pub fn cancel_for_demand(&mut self, region: usize) -> bool {
        let Some((bank, pending)) = self.pending() else {
            return false;
        };
        if pending == region {
            return false;
        }
        assert_ne!(self.active, Some(bank));
        assert!(!self.entries[bank].ready);
        self.entries[bank] = Entry {
            region: None,
            ready: false,
            stamp: 0,
        };
        self.pending = None;
        true
    }
    /// Allocate only an unprotected bank; never cancel a partially uploaded one.
    pub fn begin(&mut self, region: usize, keep: &[usize]) -> Option<usize> {
        if self.pending.is_some() || self.find(region).is_some() {
            return None;
        }
        let bank = (0..BANKS)
            .filter(|&i| self.active != Some(i))
            .filter(|&i| self.entries[i].region.is_none_or(|r| !keep.contains(&r)))
            .max_by_key(|&i| {
                if self.entries[i].region.is_none() {
                    u32::MAX
                } else {
                    self.clock.wrapping_sub(self.entries[i].stamp)
                }
            })?;
        self.clock = self.clock.wrapping_add(1);
        self.entries[bank] = Entry {
            region: Some(region),
            ready: false,
            stamp: self.clock,
        };
        self.pending = Some(bank);
        Some(bank)
    }
    pub fn finish(&mut self, bank: usize) {
        assert_eq!(self.pending, Some(bank));
        self.entries[bank].ready = true;
        self.pending = None;
    }
    pub fn activate(&mut self, region: usize) -> Option<usize> {
        let bank = self.find(region)?;
        self.clock = self.clock.wrapping_add(1);
        self.entries[bank].stamp = self.clock;
        self.active = Some(bank);
        Some(bank)
    }
}
impl Default for Banks {
    fn default() -> Self {
        Self::new()
    }
}
/// Static texture page origin in VRAM halfwords, by linear page index. The
/// pages tile the right of VRAM ten across and two down; the twentieth is the
/// animation cache's second region, so `STATIC_PAGES` of them are reachable.
pub fn page_xy(page: usize) -> (u16, u16) {
    assert!(page < STATIC_PAGES);
    (384 + (page % 10) as u16 * 64, (page / 10) as u16 * 256)
}
pub fn clut_xy(bank: usize, palette: usize) -> (u16, u16) {
    assert!(bank < BANKS && palette < CLUTS);
    match bank {
        // The font starts at row500. Overflow uses the free16-word strip
        // between its CLUT and the HUD palettes, never a framebuffer row.
        0 if palette < 400 => ((palette % 20) as u16 * 16, 480 + (palette / 20) as u16),
        0 => (336, 480 + (palette - 400) as u16),
        1 => (320 + (palette % 4) as u16 * 16, 256 + (palette / 4) as u16),
        2 => (320 + (palette % 4) as u16 * 16, 360 + (palette / 4) as u16),
        _ => (352 + (palette % 2) as u16 * 16, 32 + (palette / 2) as u16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_and_requested_banks_survive_replacement() {
        let mut b = Banks::new();
        let a = b.begin(10, &[]).unwrap();
        assert_eq!(b.activate(10), None);
        b.finish(a);
        assert_eq!(b.activate(10), Some(a));
        let n = b.begin(11, &[]).unwrap();
        assert_eq!(b.begin(12, &[]), None);
        b.finish(n);
        let c = b.begin(12, &[11]).unwrap();
        b.finish(c);
        let d = b.begin(14, &[11, 12]).unwrap();
        b.finish(d);
        assert_eq!(b.begin(13, &[11, 12, 14]), None);
        let replacement = b.begin(13, &[11, 14]).unwrap();
        assert_eq!(replacement, c);
        assert_eq!(b.find(10), Some(a));
        assert_eq!(b.find(11), Some(n));
        assert_eq!(b.find(13), None);
        b.finish(replacement);
        assert_eq!(b.activate(13), Some(replacement));
    }
    fn loaded(b: &mut Banks, region: usize) -> usize {
        let bank = b.begin(region, &[]).unwrap();
        b.finish(bank);
        bank
    }
    #[test]
    fn unfinished_upload_is_never_visible_or_active_and_cannot_be_replaced() {
        let mut b = Banks::new();
        let current = loaded(&mut b, 10);
        b.activate(10);
        let next = b.begin(11, &[]).unwrap();
        assert_eq!(b.find(11), None);
        assert_eq!(b.activate(11), None);
        assert_eq!(b.active(), Some(current));
        assert_eq!(b.pending(), Some((next, 11)));
        assert_eq!(b.begin(12, &[]), None);
        assert_eq!(b.begin(11, &[]), None);
        assert_eq!(b.pending(), Some((next, 11)));
        assert_eq!(b.find(10), Some(current));
        b.finish(next);
        assert_eq!(b.find(11), Some(next));
        assert_eq!(b.pending(), None);
        assert_eq!(b.activate(11), Some(next));
        assert_eq!(b.active(), Some(next));
    }
    #[test]
    fn ready_reentry_preserves_pixels_while_another_bank_uploads() {
        let mut b = Banks::new();
        let first = loaded(&mut b, 10);
        b.activate(10);
        let second = loaded(&mut b, 11);
        b.activate(11);
        let uploading = b.begin(12, &[10]).unwrap();
        assert_eq!(b.activate(10), Some(first));
        assert_eq!(b.find(11), Some(second));
        assert_eq!(b.pending(), Some((uploading, 12)));
        assert_eq!(b.begin(10, &[]), None);
        assert_eq!(b.begin(13, &[]), None);
        b.finish(uploading);
        assert_eq!(b.active(), Some(first));
        assert_eq!(b.activate(11), Some(second));
        assert_eq!(b.pending(), None);
    }
    #[test]
    fn reentry_refreshes_recency_and_eviction_invalidates_old_region_immediately() {
        let mut b = Banks::new();
        let first = loaded(&mut b, 10);
        b.activate(10);
        let recent = loaded(&mut b, 11);
        let old = loaded(&mut b, 12);
        loaded(&mut b, 13);
        b.activate(11);
        b.activate(10);
        let replacement = b.begin(14, &[]).unwrap();
        assert_eq!(replacement, old);
        assert_eq!(b.find(12), None);
        assert_eq!(b.activate(12), None);
        assert_eq!(b.active(), Some(first));
        assert_eq!(b.find(11), Some(recent));
        assert_eq!(b.find(14), None);
        b.finish(replacement);
        assert_eq!(b.activate(14), Some(replacement));
    }
    #[test]
    fn exhausted_protected_banks_leave_all_readiness_and_active_state_unchanged() {
        let mut b = Banks::new();
        let first = loaded(&mut b, 10);
        b.activate(10);
        let a = loaded(&mut b, 11);
        let c = loaded(&mut b, 12);
        let d = loaded(&mut b, 13);
        for _ in 0..10 {
            assert_eq!(b.begin(14, &[11, 12, 13]), None);
        }
        assert_eq!(b.active(), Some(first));
        assert_eq!(b.pending(), None);
        assert_eq!(
            [b.find(10), b.find(11), b.find(12), b.find(13)],
            [Some(first), Some(a), Some(c), Some(d)]
        );
        let replacement = b.begin(14, &[11, 13]).unwrap();
        assert_eq!(replacement, c);
    }
    #[test]
    fn urgent_cancellation_invalidates_only_unrelated_inactive_partial_bank() {
        let mut b = Banks::new();
        let current = loaded(&mut b, 10);
        b.activate(10);
        let ready = loaded(&mut b, 11);
        let partial = b.begin(12, &[11]).unwrap();
        assert!(!b.cancel_for_demand(12));
        assert_eq!(b.pending(), Some((partial, 12)));
        assert!(b.cancel_for_demand(13));
        assert_eq!(b.pending(), None);
        assert_eq!(b.find(12), None);
        assert_eq!(b.activate(12), None);
        assert_eq!(b.active(), Some(current));
        assert_eq!(b.find(10), Some(current));
        assert_eq!(b.find(11), Some(ready));
        assert!(!b.cancel_for_demand(13));
        let target = b.begin(13, &[11]).unwrap();
        assert_eq!(b.find(13), None);
        b.finish(target);
        assert_eq!(b.activate(13), Some(target));
        // A ready old room can later return, but canceled12 must upload anew.
        b.activate(10);
        let retry = b.begin(12, &[11, 13]).unwrap();
        assert_eq!(b.activate(12), None);
        b.finish(retry);
        assert_eq!(b.activate(12), Some(retry));
    }
    #[test]
    fn urgent_ready_reentry_can_release_unrelated_upload_without_invalidating_either_ready_bank() {
        let mut b = Banks::new();
        let a = loaded(&mut b, 1);
        b.activate(1);
        let dest = loaded(&mut b, 2);
        b.begin(3, &[2]).unwrap();
        assert!(b.cancel_for_demand(2));
        assert_eq!(b.find(1), Some(a));
        assert_eq!(b.find(2), Some(dest));
        assert_eq!(b.activate(2), Some(dest));
        assert_eq!(b.find(3), None);
    }
    #[test]
    fn all_texture_and_palette_origins_are_ps1_encodable() {
        for page in 0..STATIC_PAGES {
            let (x, y) = page_xy(page);
            assert_eq!(x % 64, 0);
            assert!(y == 0 || y == 256);
            let tpage = (x / 64) | ((y / 256) << 4);
            assert_eq!(((tpage & 15) * 64, ((tpage >> 4) & 1) * 256), (x, y));
        }
        for bank in 0..BANKS {
            for palette in 0..CLUTS {
                let (x, y) = clut_xy(bank, palette);
                assert_eq!(x % 16, 0);
                let clut = (x / 16) | (y << 6);
                assert_eq!(((clut & 63) * 16, clut >> 6), (x, y));
            }
        }
    }
    /// Every VRAM claim in the build, as the disjointness and spare-space
    /// tests both need the same map.
    fn vram_map() -> std::vec::Vec<bool> {
        let mut used = std::vec![false;1024*512];
        {
            let used = &mut used;
            let mut claim = move |x: usize, y: usize, w: usize, h: usize| {
                assert!(x + w <= 1024 && y + h <= 512);
                for yy in y..y + h {
                    for xx in x..x + w {
                        assert!(!used[yy * 1024 + xx], "overlap at {xx},{yy}");
                        used[yy * 1024 + xx] = true;
                    }
                }
            };
            claim(0, 0, 320, 480);
            for (x, y, w, h) in ANIMATION_REGIONS {
                claim(x, y, w, h);
            }
            claim(352, 0, 4, 32);
            claim(352, 480, 16, 2);
            // Source SOUL frame, masked fill and eyes; their three CLUTs remain
            // below bank 0 and outside the vertical scenery-palette strips.
            claim(356, 0, 28, 32);
            claim(352, 482, 16, 3);
            // Shared subtractive black-mask fade CLUT.
            claim(320, 480, 16, 1);
            // Original dialogue font strips and separate grayscale CLUT.
            claim(0, 500, 320, 12);
            claim(320, 481, 16, 1);
            for (x, y, w, h) in GEO_RECTS {
                claim(x, y, w, h);
            }
            for (x, y, w, h) in LIFE_RECTS {
                claim(x, y, w, h);
            }
            for (x, y, w, h) in BREAK_RECTS {
                claim(x, y, w, h);
            }
            for page in 0..STATIC_PAGES {
                let (x, y) = page_xy(page);
                claim(x as usize, y as usize, 64, 256);
            }
            for bank in 0..BANKS {
                for palette in 0..CLUTS {
                    if bank * CLUTS + palette < SCENE_CLUTS {
                        let (x, y) = clut_xy(bank, palette);
                        claim(x as usize, y as usize, 16, 1);
                    }
                }
            }
        }
        used
    }
    #[test]
    fn all_vram_allocations_are_disjoint() {
        vram_map();
    }
    /// The regions hold exactly the slots the cache declares, each slot sits
    /// inside one texture page, and what is left over still cannot hold
    /// another. This is the measurement behind `crate::MAX_FRAME_TILES`.
    #[test]
    fn the_animation_regions_are_full_and_nothing_spare_can_hold_another_slot() {
        // `crate::SLOTS` is checked against this in lib.rs, which is the only
        // place both are in scope: test_bootstrap_atlas compiles this file on
        // its own as a crate root.
        assert_eq!(ANIMATION_SLOTS, 24);
        // Sixteen of those slots are one static scenery page: the reservation
        // is one page shorter and the region sits exactly where the linear
        // page map used to put index 19.
        assert_eq!(STATIC_PAGES, BANKS * PAGES - 1);
        assert_eq!((MAP_PAGE.0 as u16, MAP_PAGE.1 as u16), page_xy(18));
        assert_eq!(
            (ANIMATION_REGIONS[1].0, ANIMATION_REGIONS[1].1),
            (384 + (19 % 10) * 64, (19 / 10) * 256)
        );
        assert_eq!(ANIMATION_REGIONS[1].2 * ANIMATION_REGIONS[1].3, 64 * 256);
        let mut origins = std::vec::Vec::new();
        for slot in 0..ANIMATION_SLOTS {
            let (tx, ty, u, v) = slot_placement(slot);
            assert_eq!(tx % 64, 0, "slot {slot} is not on a texture page");
            assert!(ty == 0 || ty == 256, "slot {slot} is not on a texture page");
            // A textured primitive carries u and v as one byte each, so the
            // far corner of a 64x64 slot has to stay inside 255.
            assert!(
                u + 63 <= 255 && v + 63 <= 255,
                "slot {slot} leaves its texture page"
            );
            let (x, y) = (tx as usize + u as usize / 4, ty as usize + v as usize);
            assert!(
                ANIMATION_REGIONS.iter().any(|&(rx, ry, w, h)| x >= rx
                    && y >= ry
                    && x + SLOT_HALFWORDS.0 <= rx + w
                    && y + SLOT_HALFWORDS.1 <= ry + h),
                "slot {slot} leaves every animation region"
            );
            assert!(
                !origins.contains(&(x, y)),
                "slot {slot} shares another slot's texels"
            );
            origins.push((x, y));
        }
        // The eight slots that were always there keep the strip's own layout.
        for slot in 0..8 {
            assert_eq!(
                slot_placement(slot),
                (320, 0, (slot as u16 % 2) * 64, (slot as u16 / 2) * 64)
            );
        }
        let used = vram_map();
        assert_eq!(used.iter().filter(|&&u| !u).count(), SPARE_HALFWORDS);
        // Widest free run on any row, which bounds every free rectangle.
        let widest = (0..512)
            .map(|y| {
                let (mut best, mut run) = (0, 0);
                for x in 0..1024 {
                    if used[y * 1024 + x] {
                        run = 0
                    } else {
                        run += 1;
                        if run > best {
                            best = run
                        }
                    }
                }
                best
            })
            .max()
            .unwrap();
        assert_eq!(widest, 16);
        // Tallest free run in any column, likewise.
        let tallest = (0..1024)
            .map(|x| {
                let (mut best, mut run) = (0, 0);
                for y in 0..512 {
                    if used[y * 1024 + x] {
                        run = 0
                    } else {
                        run += 1;
                        if run > best {
                            best = run
                        }
                    }
                }
                best
            })
            .max()
            .unwrap();
        assert_eq!(tallest, 14);
        assert!(tallest < 64, "a ninth slot would need 64 free rows");
    }
    #[test]
    fn soul_hud_slots_fit_reserved_strip_and_encodable_uvs() {
        let mut used = [false; 28 * 32];
        // Max pixel widths admitted by cook_hud: frame40, fill24, eyes16.
        for (x, w, h) in [(356, 40, 24), (366, 24, 32), (372, 16, 32)] {
            let words = (w + 3) / 4;
            assert!(x >= 356 && x + words <= 384);
            let u = (x - 320) * 4;
            assert!(u + w <= 256 && h <= 32);
            for y in 0..h {
                for xx in x..x + words {
                    let p = y * 28 + xx - 356;
                    assert!(!used[p]);
                    used[p] = true;
                }
            }
        }
        for y in 482..=484 {
            let clut = (352 / 16) | (y << 6);
            assert_eq!(((clut & 63) * 16, clut >> 6), (352, y));
        }
    }
}
