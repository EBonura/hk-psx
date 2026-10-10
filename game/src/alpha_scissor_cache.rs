//! Map validated texel covers to scissors without changing the original quad's
//! vertices or UV interpolation. All screen rectangles are half-open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scissors {
    pub rects: [[i16; 4]; 4],
    pub count: usize,
    pub saved_pixels: u32,
}

/// Caller-owned mapping result. Only the counted rectangle prefix is readable;
/// unsuccessful mappings expose no rectangles, even when reusing old storage.
#[repr(C)]
pub struct MappedScissors {
    count: usize,
    saved_pixels: u32,
    rects: [core::mem::MaybeUninit<[i16; 4]>; 4],
}
impl MappedScissors {
    /// Initialize scalar validity fields in place. The MaybeUninit rectangle
    /// fields accept arbitrary backing bytes and need no clearing or copying.
    pub fn initialize(storage: &mut core::mem::MaybeUninit<Self>) -> &mut Self {
        unsafe {
            let p = storage.as_mut_ptr();
            (&raw mut (*p).count).write(0);
            (&raw mut (*p).saved_pixels).write(0);
            &mut *p
        }
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn saved_pixels(&self) -> u32 {
        self.saved_pixels
    }
    pub fn as_slice(&self) -> &[[i16; 4]] {
        unsafe { core::slice::from_raw_parts(self.rects.as_ptr().cast(), self.count) }
    }
}

struct Axis {
    start: i32,
    end: i32,
    seed: i32,
    step: i32,
}
impl Axis {
    fn new(a: i16, b: i16, texels: u16, screen: i32) -> Self {
        let (a, b) = (a as i32, b as i32);
        let minimum = a.min(b);
        let span = (b - a).abs();
        let reverse = b < a;
        // Rust signed division truncates toward zero, matching the GPU DDA.
        let step = (if reverse { -1 } else { 1 }) * (texels as i32 - 1) * 4096 / span;
        let start = minimum.max(0);
        let seed =
            (if reverse { texels as i32 - 1 } else { 0 }) * 4096 + 2048 + (start - minimum) * step;
        Self {
            start,
            end: a.max(b).min(screen),
            seed,
            step,
        }
    }
    /// First pixel whose increasing fixed-point value reaches the threshold.
    /// The exact inverse replaces a binary search's repeated MIPS multiplies.
    #[inline]
    fn lower_bound(&self, threshold: i32) -> i32 {
        let (seed, step) = if self.step < 0 {
            (-self.seed, -self.step)
        } else {
            (self.seed, self.step)
        };
        if threshold <= seed {
            return self.start;
        }
        let length = self.end - self.start;
        if threshold > seed + (length - 1) * step {
            return self.end;
        }
        // Here step>0 and delta>0. ceil(delta/step)=(delta-1)/step+1.
        // Legal spans and 256-texel dimensions bound all values below 2^22.
        self.start + ((threshold - seed - 1) as u32 / step as u32 + 1) as i32
    }
    fn interval(&self, low: i32, high: i32) -> (i32, i32) {
        if self.step == 0 {
            let sample = self.seed >> 12;
            return if low <= sample && sample < high {
                (self.start, self.end)
            } else {
                (self.start, self.start)
            };
        }
        let (low, high) = (low << 12, high << 12);
        if self.step < 0 {
            // low <= seed+n*step < high becomes 1-high <= -(seed+n*step)
            // < 1-low. The +1 retains exact inclusive texel boundaries.
            (self.lower_bound(1 - high), self.lower_bound(1 - low))
        } else {
            (self.lower_bound(low), self.lower_bound(high))
        }
    }
}

/// Screen pixels (half-open, clipped) whose sampled texel lies inside the
/// texel rectangle `rect` = [x,y,w,h] of an axis-aligned quad. Same DDA as the
/// covers, so a flat quad over this rectangle and the textured quad clipped to
/// its complement partition the original pixels exactly.
pub fn texel_rect_to_screen(
    verts: [(i16, i16); 4],
    width: u16,
    height: u16,
    rect: [u8; 4],
) -> Option<[i16; 4]> {
    let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = verts;
    if y0 != y1 || x0 != x2 || x1 != x3 || y2 != y3 || rect[2] == 0 || rect[3] == 0 {
        return None;
    }
    let dx = x1 as i32 - x0 as i32;
    let dy = y2 as i32 - y0 as i32;
    if dx == 0
        || dx.abs() > 1023
        || dy == 0
        || dy.abs() > 511
        || width == 0
        || width > 256
        || height == 0
        || height > 256
    {
        return None;
    }
    let (l, t) = (rect[0] as i32, rect[1] as i32);
    let (r, b) = (l + rect[2] as i32, t + rect[3] as i32);
    if r > width as i32 || b > height as i32 {
        return None;
    }
    let sx = dx.abs();
    let sy = dy.abs();
    let x = if dx < 0 {
        Axis::new(sx as i16, 0, width, sx)
    } else {
        Axis::new(0, sx as i16, width, sx)
    };
    let y = if dy < 0 {
        Axis::new(sy as i16, 0, height, sy)
    } else {
        Axis::new(0, sy as i16, height, sy)
    };
    let (l, r) = if l == 0 && r == width as i32 {
        (x.start, x.end)
    } else {
        x.interval(l, r)
    };
    let (t, b) = if t == 0 && b == height as i32 {
        (y.start, y.end)
    } else {
        y.interval(t, b)
    };
    let (left, top) = (x0.min(x1) as i32, y0.min(y2) as i32);
    let l = (left + l).max(0);
    let r = (left + r).min(320);
    let t = (top + t).max(0);
    let b = (top + b).min(240);
    (l < r && t < b).then_some([l as i16, t as i16, r as i16, b as i16])
}

/// Fixed-texture room lease contract: cover/dimensions of a texture ID do not
/// change until all entries are reset. Cache keys include exact signed spans.
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub struct Entry {
    rects: [[i16; 4]; 4],
    spans: [i16; 2],
    texture: u16,
    count: u8,
    reserved: u8,
}
// Keep40-byte secondary copies aligned for native PS1 LW/SW transfers.
const _: () = {
    assert!(core::mem::size_of::<Entry>() == 40);
    assert!(core::mem::align_of::<Entry>() == 4);
};
const UNPREPARED: u8 = 0;
const FALLBACK: u8 = 255;
// Cumulative counts cover eligible draws above the area gate, plus immutable
// cached-fallback hits (which return before geometry, clipping, and area work).
// Guest rendering is single-threaded. Native parity tests do not mutate these.
#[no_mangle]
pub static mut HK_SCISSOR_CACHE_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_SCISSOR_CACHE_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_SCISSOR_SECONDARY_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_SCISSOR_SECONDARY_MISSES: u32 = 0;

/// Reuse full-local preparations across draws and camera rounding phases.
/// Texture dimensions/covers must remain immutable until `invalidate`, under
/// exactly the same scene-bank lease as the primary per-draw entries.
pub struct Secondary {
    entries: [[Entry; 2]; 64],
    victim: [u8; 64],
}
const _: () = {
    assert!(core::mem::size_of::<Secondary>() == 5184);
    assert!(core::mem::align_of::<Secondary>() == 4);
};
impl Secondary {
    pub const EMPTY: Self = Self {
        entries: [[Entry::EMPTY; 2]; 64],
        victim: [0; 64],
    };
    pub fn invalidate(&mut self) {
        for bucket in &mut self.entries {
            for entry in bucket {
                entry.invalidate();
            }
        }
        self.victim = [0; 64];
    }
    #[inline]
    fn bucket(texture: u16, spans: [i16; 2]) -> usize {
        let t = texture as u32;
        let x = spans[0] as i32;
        let y = spans[1] as i32;
        ((t ^ (t >> 6)
            ^ (x.wrapping_shl(3) as u32)
            ^ ((x >> 4) as u32)
            ^ (y.wrapping_shl(1) as u32)
            ^ ((y >> 5) as u32))
            & 63) as usize
    }
    #[inline(never)]
    fn prepare(
        &mut self,
        out: &mut Entry,
        spans: [i16; 2],
        texture: u16,
        width: u16,
        height: u16,
        cover: &[u8],
    ) {
        let index = Self::bucket(texture, spans);
        for entry in &self.entries[index] {
            if entry.count != UNPREPARED && entry.texture == texture && entry.spans == spans {
                *out = *entry;
                #[cfg(target_arch = "mips")]
                unsafe {
                    HK_SCISSOR_SECONDARY_HITS = HK_SCISSOR_SECONDARY_HITS.wrapping_add(1);
                }
                return;
            }
        }
        #[cfg(target_arch = "mips")]
        unsafe {
            HK_SCISSOR_SECONDARY_MISSES = HK_SCISSOR_SECONDARY_MISSES.wrapping_add(1);
        }
        out.prepare(spans, texture, width, height, cover);
        // FIFO within two ways: hits need no entry moves or victim update.
        self.entries[index][self.victim[index] as usize] = *out;
        self.victim[index] ^= 1;
    }
}
impl Entry {
    pub const EMPTY: Self = Self {
        rects: [[0; 4]; 4],
        spans: [0; 2],
        texture: 0,
        count: UNPREPARED,
        reserved: 0,
    };
    /// Invalidate only this draw's state when a new room changes texture IDs.
    /// No rectangle data needs clearing and EMPTY is entirely zero-filled BSS.
    pub fn invalidate(&mut self) {
        self.count = UNPREPARED;
    }
    #[inline(never)]
    fn prepare(&mut self, spans: [i16; 2], texture: u16, width: u16, height: u16, cover: &[u8]) {
        self.spans = spans;
        self.texture = texture;
        self.count = FALLBACK;
        if cover.len() != 20
            || cover[0] > 4
            || cover[1..4] != [0; 3]
            || width == 0
            || width > 256
            || height == 0
            || height > 256
        {
            return;
        }
        if cover[0] == 1
            && cover[4] == 0
            && cover[5] == 0
            && cover[6] as u16 + 1 == width
            && cover[7] as u16 + 1 == height
        {
            return;
        }
        if cover[0] == 0 {
            self.count = 1;
            return;
        }
        // Full domains are critical: screen-clipped preparation would drop
        // texels that become visible after a later camera translation.
        let sx = spans[0].abs();
        let sy = spans[1].abs();
        let x = if spans[0] < 0 {
            Axis::new(sx, 0, width, sx as i32)
        } else {
            Axis::new(0, sx, width, sx as i32)
        };
        let y = if spans[1] < 0 {
            Axis::new(sy, 0, height, sy as i32)
        } else {
            Axis::new(0, sy, height, sy as i32)
        };
        let mut count = 0;
        for index in 0..cover[0] as usize {
            let p = 4 + index * 4;
            let (l, t) = (cover[p] as i32, cover[p + 1] as i32);
            let (r, b) = (l + cover[p + 2] as i32 + 1, t + cover[p + 3] as i32 + 1);
            if r > width as i32 || b > height as i32 {
                return;
            }
            let (l, r) = if l == 0 && r == width as i32 {
                (x.start, x.end)
            } else {
                x.interval(l, r)
            };
            let (t, b) = if t == 0 && b == height as i32 {
                (y.start, y.end)
            } else {
                y.interval(t, b)
            };
            if l < r && t < b {
                self.rects[count] = [l as i16, t as i16, r as i16, b as i16];
                count += 1;
            }
        }
        self.count = count as u8 + 1;
    }
    /// Unshared path retained for native differential tests and one-off users.
    pub fn map(
        &mut self,
        verts: [(i16, i16); 4],
        texture: u16,
        width: u16,
        height: u16,
        cover: &[u8],
    ) -> Option<Scissors> {
        self.map_inner(verts, texture, width, height, cover, None)
    }
    #[inline(never)]
    pub fn map_cached(
        &mut self,
        verts: [(i16, i16); 4],
        texture: u16,
        width: u16,
        height: u16,
        cover: &[u8],
        secondary: &mut Secondary,
    ) -> Option<Scissors> {
        self.map_inner(verts, texture, width, height, cover, Some(secondary))
    }
    /// Map directly into caller-owned storage; no initialized tail or aggregate
    /// result copy is required. Cache preparation and acceptance match map().
    #[inline(never)]
    pub fn map_cached_into(
        &mut self,
        verts: [(i16, i16); 4],
        texture: u16,
        width: u16,
        height: u16,
        cover: &[u8],
        secondary: &mut Secondary,
        out: &mut MappedScissors,
    ) -> bool {
        out.count = 0;
        out.saved_pixels = 0;
        // Every prepare fallback is immutable texture metadata: invalid cover
        // header/dimensions, full cover, or an out-of-bounds texel rectangle.
        // Geometry rejection and insufficient savings never set FALLBACK.
        // The room-lease contract therefore permits this before span checks.
        if self.count == FALLBACK && self.texture == texture {
            #[cfg(target_arch = "mips")]
            unsafe {
                HK_SCISSOR_CACHE_HITS = HK_SCISSOR_CACHE_HITS.wrapping_add(1);
            }
            return false;
        }
        let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = verts;
        if y0 != y1 || x0 != x2 || x1 != x3 || y2 != y3 {
            return false;
        }
        let dx = x1 as i32 - x0 as i32;
        let dy = y2 as i32 - y0 as i32;
        if dx == 0 || dx.abs() > 1023 || dy == 0 || dy.abs() > 511 {
            return false;
        }
        let (left, top) = (x0.min(x1) as i32, y0.min(y2) as i32);
        let (right, bottom) = (x0.max(x1) as i32, y0.max(y2) as i32);
        if right <= 0 || left >= 320 || bottom <= 0 || top >= 240 {
            return false;
        }
        let full_area = ((right.min(320) - left.max(0)) * (bottom.min(240) - top.max(0))) as u32;
        if full_area <= 128 {
            return false;
        }
        let spans = [dx as i16, dy as i16];
        if self.count == UNPREPARED || self.spans != spans || self.texture != texture {
            #[cfg(target_arch = "mips")]
            unsafe {
                HK_SCISSOR_CACHE_MISSES = HK_SCISSOR_CACHE_MISSES.wrapping_add(1);
            }
            secondary.prepare(self, spans, texture, width, height, cover);
        } else {
            #[cfg(target_arch = "mips")]
            unsafe {
                HK_SCISSOR_CACHE_HITS = HK_SCISSOR_CACHE_HITS.wrapping_add(1);
            }
        }
        if self.count == FALLBACK {
            return false;
        }
        let mut count = 0;
        let mut saved_pixels = full_area;
        for rect in &self.rects[..(self.count - 1) as usize] {
            let l = (left + rect[0] as i32).max(0);
            let t = (top + rect[1] as i32).max(0);
            let r = (left + rect[2] as i32).min(320);
            let b = (top + rect[3] as i32).min(240);
            if l < r && t < b {
                let Some(remaining) = saved_pixels.checked_sub(((r - l) * (b - t)) as u32) else {
                    return false;
                };
                saved_pixels = remaining;
                out.rects[count].write([l as i16, t as i16, r as i16, b as i16]);
                count += 1;
            }
        }
        let threshold = 128u32.max(128 * count.saturating_sub(1) as u32);
        if saved_pixels <= threshold {
            return false;
        }
        out.count = count;
        out.saved_pixels = saved_pixels;
        true
    }
    #[inline(always)]
    fn map_inner(
        &mut self,
        verts: [(i16, i16); 4],
        texture: u16,
        width: u16,
        height: u16,
        cover: &[u8],
        secondary: Option<&mut Secondary>,
    ) -> Option<Scissors> {
        // Every prepare fallback is immutable texture metadata: invalid cover
        // header/dimensions, full cover, or an out-of-bounds texel rectangle.
        // Geometry rejection and insufficient savings never set FALLBACK.
        // The room-lease contract therefore permits this before span checks.
        if self.count == FALLBACK && self.texture == texture {
            #[cfg(target_arch = "mips")]
            unsafe {
                HK_SCISSOR_CACHE_HITS = HK_SCISSOR_CACHE_HITS.wrapping_add(1);
            }
            return None;
        }
        let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = verts;
        if y0 != y1 || x0 != x2 || x1 != x3 || y2 != y3 {
            return None;
        }
        let dx = x1 as i32 - x0 as i32;
        let dy = y2 as i32 - y0 as i32;
        if dx == 0 || dx.abs() > 1023 || dy == 0 || dy.abs() > 511 {
            return None;
        }
        let (left, top) = (x0.min(x1) as i32, y0.min(y2) as i32);
        let (right, bottom) = (x0.max(x1) as i32, y0.max(y2) as i32);
        if right <= 0 || left >= 320 || bottom <= 0 || top >= 240 {
            return None;
        }
        let full_area = ((right.min(320) - left.max(0)) * (bottom.min(240) - top.max(0))) as u32;
        if full_area <= 128 {
            return None;
        }
        let spans = [dx as i16, dy as i16];
        if self.count == UNPREPARED || self.spans != spans || self.texture != texture {
            #[cfg(target_arch = "mips")]
            unsafe {
                HK_SCISSOR_CACHE_MISSES = HK_SCISSOR_CACHE_MISSES.wrapping_add(1);
            }
            if let Some(cache) = secondary {
                cache.prepare(self, spans, texture, width, height, cover);
            } else {
                self.prepare(spans, texture, width, height, cover);
            }
        } else {
            #[cfg(target_arch = "mips")]
            unsafe {
                HK_SCISSOR_CACHE_HITS = HK_SCISSOR_CACHE_HITS.wrapping_add(1);
            }
        }
        if self.count == FALLBACK {
            return None;
        }
        let mut out = Scissors {
            rects: [[0; 4]; 4],
            count: 0,
            saved_pixels: full_area,
        };
        for rect in &self.rects[..(self.count - 1) as usize] {
            let l = (left + rect[0] as i32).max(0);
            let t = (top + rect[1] as i32).max(0);
            let r = (left + rect[2] as i32).min(320);
            let b = (top + rect[3] as i32).min(240);
            if l < r && t < b {
                out.saved_pixels = out.saved_pixels.checked_sub(((r - l) * (b - t)) as u32)?;
                out.rects[out.count] = [l as i16, t as i16, r as i16, b as i16];
                out.count += 1;
            }
        }
        let threshold = 128u32.max(128 * out.count.saturating_sub(1) as u32);
        if out.saved_pixels > threshold {
            Some(out)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "../../tests/alpha_scissor_cache_runtime.rs"]
mod tests;
