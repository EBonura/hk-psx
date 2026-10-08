//! Map validated texel covers to scissors without changing the original quad's
//! vertices or UV interpolation. All screen rectangles are half-open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scissors {
    pub rects: [[i16; 4]; 4],
    pub count: usize,
    pub saved_pixels: u32,
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

/// `cover` is a validated 20-byte record: count, three zero bytes, then up to
/// four disjoint [x,y,width-1,height-1] texel rectangles. None retains the
/// original draw; Some with count zero omits a sufficiently large empty draw.
// Keep optional cover processing out of the common scenery loop's instruction
// cache footprint and stack frame on the R3000's 4 KiB instruction cache.
#[inline(never)]
pub fn map(verts: [(i16, i16); 4], width: u16, height: u16, cover: &[u8]) -> Option<Scissors> {
    if cover.len() != 20
        || cover[0] > 4
        || cover[1..4] != [0; 3]
        || width == 0
        || width > 256
        || height == 0
        || height > 256
    {
        return None;
    }
    if cover[0] == 1
        && cover[4] == 0
        && cover[5] == 0
        && cover[6] as u16 + 1 == width
        && cover[7] as u16 + 1 == height
    {
        return None; // A full texture cover cannot save any screen pixels.
    }
    let [(x0, y0), (x1, y1), (x2, y2), (x3, y3)] = verts;
    if y0 != y1 || x0 != x2 || x1 != x3 || y2 != y3 {
        return None;
    }
    let (span_x, span_y) = ((x1 as i32 - x0 as i32).abs(), (y2 as i32 - y0 as i32).abs());
    // The original GPU rejects oversized triangles. Never turn such a draw
    // into differently accepted work, or divide by a degenerate screen span.
    if span_x == 0 || span_x > 1023 || span_y == 0 || span_y > 511 {
        return None;
    }
    if x0.max(x1) <= 0 || x0.min(x1) >= 320 || y0.max(y2) <= 0 || y0.min(y2) >= 240 {
        return None;
    }
    let full_area = ((x0.max(x1).min(320) as i32 - x0.min(x1).max(0) as i32)
        * (y0.max(y2).min(240) as i32 - y0.min(y2).max(0) as i32)) as u32;
    if full_area <= 128 {
        return None;
    }
    let mut out = Scissors {
        rects: [[0; 4]; 4],
        count: 0,
        saved_pixels: full_area,
    };
    if cover[0] == 0 {
        return Some(out); // Empty textures require no UV interpolation math.
    }
    let x = Axis::new(x0, x1, width, 320);
    let y = Axis::new(y0, y2, height, 240);
    for index in 0..cover[0] as usize {
        let p = 4 + index * 4;
        let (left, top) = (cover[p] as i32, cover[p + 1] as i32);
        let (right, bottom) = (
            left + cover[p + 2] as i32 + 1,
            top + cover[p + 3] as i32 + 1,
        );
        if right > width as i32 || bottom > height as i32 {
            return None;
        }
        let (left, right) = if left == 0 && right == width as i32 {
            (x.start, x.end)
        } else {
            x.interval(left, right)
        };
        let (top, bottom) = if top == 0 && bottom == height as i32 {
            (y.start, y.end)
        } else {
            y.interval(top, bottom)
        };
        if left < right && top < bottom {
            let area = ((right - left) * (bottom - top)) as u32;
            out.saved_pixels = out.saved_pixels.checked_sub(area)?;
            out.rects[out.count] = [left as i16, top as i16, right as i16, bottom as i16];
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
