//! Exact bounded rectangle subtraction for source-preserving GPU occlusion.
use core::mem::MaybeUninit;
#[path = "tile_runs.rs"]
pub mod tile_runs;
pub const MAX_PIECES: usize = 32;
// Rectangle subtraction keeps its established CPU bound. Larger terminal tile
// output is emitted directly and never feeds back into rectangle subtraction.
pub const MAX_SUBTRACT_PIECES: usize = 8;
// Small cuts cost more CPU partition work than their saved GPU raster time.
pub const MIN_SAVED_PIXELS: i32 = 1024;
/// Only the initialized prefix is exposed. Empty scratch has no initialization
/// stores, important because this is constructed for every scenery draw.
#[repr(C)]
pub struct Pieces {
    rects: [MaybeUninit<[i16; 4]>; MAX_PIECES],
    count: usize,
}
impl Pieces {
    pub const fn empty() -> Self {
        Self {
            rects: [MaybeUninit::uninit(); MAX_PIECES],
            count: 0,
        }
    }
    pub fn from_slice(rects: &[[i16; 4]]) -> Self {
        let mut out = Self::empty();
        out.set_slice(rects);
        out
    }
    pub fn clear(&mut self) {
        self.count = 0;
    }
    /// Initialize an exclusive, aligned storage slot without reading or
    /// clearing its uninitialized rectangle backing. No references may be live.
    pub unsafe fn initialize_at(slot: *mut Self) {
        (&raw mut (*slot).count).write(0);
    }
    pub fn set_slice(&mut self, rects: &[[i16; 4]]) {
        self.clear();
        for &rect in rects {
            self.push(rect);
        }
    }
    pub fn push(&mut self, rect: [i16; 4]) {
        assert!(self.count < MAX_PIECES);
        self.rects[self.count].write(rect);
        self.count += 1;
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn as_slice(&self) -> &[[i16; 4]] {
        // push writes each element before increasing count; count is private.
        unsafe { core::slice::from_raw_parts(self.rects.as_ptr().cast(), self.count) }
    }
}
pub fn rect_area(r: &[i16; 4]) -> i32 {
    ((r[2] - r[0]) as i32 * (r[3] - r[1]) as i32).max(0)
}
pub fn overlap(a: &[i16; 4], b: &[i16; 4]) -> i32 {
    let l = a[0].max(b[0]);
    let t = a[1].max(b[1]);
    let r = a[2].min(b[2]);
    let bt = a[3].min(b[3]);
    if l < r && t < bt {
        (r - l) as i32 * (bt - t) as i32
    } else {
        0
    }
}
/// The current disjoint pieces are subsets of bounds. Their total overlap
/// cannot exceed this box overlap, even after a flat core is removed.
#[inline]
pub fn can_save_minimum(bounds: &[i16; 4], hole: &[i16; 4]) -> bool {
    overlap(bounds, hole) >= MIN_SAVED_PIXELS
}
/// Subtract in place only after proving the final partition fits. Remove
/// fully covered entries first, so later splits cannot temporarily exceed the
/// final size. Failed preflight leaves every original rectangle unchanged.
pub fn subtract(p: &mut Pieces, hole: &[i16; 4]) -> bool {
    let mut needed = 0;
    let mut changed = false;
    for piece in p.as_slice() {
        let l = piece[0].max(hole[0]);
        let t = piece[1].max(hole[1]);
        let r = piece[2].min(hole[2]);
        let b = piece[3].min(hole[3]);
        if l >= r || t >= b {
            needed += 1;
        } else {
            changed = true;
            needed += usize::from(piece[1] < t)
                + usize::from(b < piece[3])
                + usize::from(piece[0] < l)
                + usize::from(r < piece[2]);
        }
        if needed > MAX_SUBTRACT_PIECES {
            return false;
        }
    }
    if !changed {
        return false;
    }
    let mut retained = 0;
    for i in 0..p.count {
        let q = unsafe { p.rects[i].assume_init() };
        if hole[0] <= q[0] && hole[1] <= q[1] && hole[2] >= q[2] && hole[3] >= q[3] {
            continue;
        }
        p.rects[retained].write(q);
        retained += 1;
    }
    p.count = retained;
    for i in (0..retained).rev() {
        let q = unsafe { p.rects[i].assume_init() };
        let l = q[0].max(hole[0]);
        let t = q[1].max(hole[1]);
        let r = q[2].min(hole[2]);
        let b = q[3].min(hole[3]);
        if l >= r || t >= b {
            continue;
        }
        let mut first = true;
        for part in [
            [q[0], q[1], q[2], t],
            [q[0], b, q[2], q[3]],
            [q[0], t, l, b],
            [r, t, q[2], b],
        ] {
            if part[0] < part[2] && part[1] < part[3] {
                if first {
                    p.rects[i].write(part);
                    first = false;
                } else {
                    p.push(part);
                }
            }
        }
        debug_assert!(!first);
    }
    debug_assert_eq!(p.count, needed);
    true
}

/// Apply more than one occluder without increasing the bounded rectangle packet
/// ceiling. Rank by their original overlap, then visit each at most once. A
/// failed subtraction leaves the exact previous partition intact.
pub fn subtract_many(pieces: &mut Pieces, holes: &[[i16; 4]]) -> i32 {
    debug_assert!(holes.len() <= 8);
    let mut ranked = [MaybeUninit::<(i32, usize)>::uninit(); 8];
    for (k, hole) in holes.iter().enumerate() {
        let score = pieces.as_slice().iter().map(|p| overlap(p, hole)).sum();
        ranked[k].write((score, k));
    }
    // Every rank in this prefix was initialized above; unused slots are never read.
    let ranked = unsafe {
        core::slice::from_raw_parts_mut(ranked.as_mut_ptr().cast::<(i32, usize)>(), holes.len())
    };
    let mut saved = 0;
    for _ in 0..holes.len() {
        let mut best = 0;
        for k in 1..holes.len() {
            if ranked[k].0 > ranked[best].0 {
                best = k;
            }
        }
        let (score, k) = ranked[best];
        if score < MIN_SAVED_PIXELS {
            break;
        }
        ranked[best].0 = 0;
        // Later holes may overlap pixels already removed by an earlier one.
        let gain = pieces
            .as_slice()
            .iter()
            .map(|p| overlap(p, &holes[k]))
            .sum::<i32>();
        if gain < MIN_SAVED_PIXELS {
            continue;
        }
        if subtract(pieces, &holes[k]) {
            saved += gain;
        }
        if pieces.len() == 0 {
            break;
        }
    }
    saved
}
