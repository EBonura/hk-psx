//! Wide-integer idioms the simulation shares, on `psx_math::Wide`.
//!
//! The guest has no 64-bit integer maths. A quantity wider than a word (a squared
//! distance, a cross product, a scaled random draw) is a [`Wide`]: one `mult` for each
//! product, no helper call. These are the few shapes that recur across the actors; each
//! is `#[inline(always)]`, so a call compiles to the instructions of the bare expression.
use psx_math::Wide;

/// `(a - b) >> 8`, the difference taken exactly (it can need 33 bits) and then scaled
/// down so its square fits a [`Wide`] with room to add two of them.
#[inline(always)]
pub fn diff8(a: i32, b: i32) -> i32 {
    Wide::of(a).sub_w(Wide::of(b)).shr_i32(8)
}

/// `dx * dx + dy * dy`.
#[inline(always)]
pub fn sum_sq(dx: i32, dy: i32) -> Wide {
    Wide::mul(dx, dx).mul_add(dy, dy)
}

/// `value * value`.
#[inline(always)]
pub fn square(value: i32) -> Wide {
    Wide::mul(value, value)
}

/// `low + ((random * (high - low)) >> 24)`: a draw of the 24-bit `random` over
/// `low..high`, rounding toward negative infinity.
#[inline(always)]
pub fn range24(random: u32, low: i32, high: i32) -> i32 {
    low + Wide::mul(random as i32, high - low).shr_i32(24)
}

/// The cross product `(b - a) x (p - a)` of two Q16 edge vectors. Cooked local
/// polygons fit +/-16 units and room bounds +/-512, so each difference fits `i32` and the
/// exact value fits a [`Wide`].
#[inline(always)]
pub fn cross(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> Wide {
    Wide::mul(b[0] - a[0], p[1] - a[1]).mul_sub(b[1] - a[1], p[0] - a[0])
}

/// `((a - b) * k) >> shift`, the difference taken exactly: the projection of a Q16
/// world offset onto the screen by a fixed-point scale. The result is the low 32 bits
/// of the shifted value (a screen offset, always small).
#[inline(always)]
pub fn project(a: i32, b: i32, k: i32, shift: u32) -> i32 {
    Wide::of(a).sub_w(Wide::of(b)).times(k).shr_i32(shift)
}

/// `dx * dx + dy * dy <= radius * radius`: is the offset inside the circle?
#[inline(always)]
pub fn within(dx: i32, dy: i32, radius: i32) -> bool {
    sum_sq(dx, dy) <= square(radius)
}

/// `floor(a * b / d)` for a divisor that is a nonzero word, as the low 32 bits: the
/// clip-time conversions (`tick * fps / 60`). One shared divide, no 64-bit helper.
#[inline(always)]
pub fn mul_div_floor(a: u32, b: u32, d: u32) -> u32 {
    psx_math::WideU::mul(a, b).div_rem_u32(d).0.low()
}

/// The sign (-1, 0 or 1) of [`cross`]: what the segment and polygon tests compare against zero.
#[inline(always)]
pub fn cross_sign(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> i32 {
    let c = cross(a, b, p);
    i32::from(c.is_positive()) - i32::from(c.is_negative())
}

/// `floor(sqrt(dx * dx + dy * dy))`, exact for every pair of words (the squares are summed
/// unsigned, so even two `i32::MIN` offsets cannot overflow).
#[inline(always)]
pub fn hypot(dx: i32, dy: i32) -> u32 {
    let (x, y) = (dx.unsigned_abs(), dy.unsigned_abs());
    psx_math::WideU::mul(x, x).mul_add(y, y).isqrt()
}

/// What Unity's AudioSource rolloff leaves of a source `dx`, `dy` (Q16 units) from the
/// listener, in 4096ths. The listener is the camera, 38 units in front of the play plane
/// (CAM_Z), so the source is `sqrt(dx^2 + dy^2 + 38^2)` away: full volume to `min` (39 units),
/// then the source's custom curve (1 at 0.693 of `max`, 0 at `max` 56.3, slopes -5.15 and
/// -1.63) down to silence. Distances are taken in 1/16 units so the squares stay in 32 bits.
pub fn source_rolloff(dx: i32, dy: i32, min: i32, max: i32) -> i32 {
    let (x, y) = ((dx >> 12).abs().min(1000), (dy >> 12).abs().min(1000));
    let d2 = x * x + y * y + (38 * 16) * (38 * 16);
    // Integer square root by Newton steps.
    let mut d = 38 * 16 + x.max(y);
    for _ in 0..6 {
        d = (d + d2 / d) / 2;
    }
    let (lo, hi) = (min * 16, max * 16);
    if d <= lo {
        return 4096;
    }
    if d >= hi {
        return 0;
    }
    // Hermite segment on u in 0..1 over lo..hi, slopes in value per unit of u.
    let u = (d - lo) * 4096 / (hi - lo);
    let (u2, u3) = (u * u / 4096, u * u / 4096 * u / 4096);
    const M0: i32 = -1583; // -5.1528 * 0.3073, in 1/1000
    const M1: i32 = -501; // -1.6285 * 0.3073
    let h00 = 2 * u3 - 3 * u2 + 4096;
    let h10 = u3 - 2 * u2 + u;
    let h11 = u3 - u2;
    (h00 + h10 * M0 / 1000 + h11 * M1 / 1000).clamp(0, 4096)
}


#[cfg(test)]
mod rolloff_tests {
    use super::source_rolloff;
    const ONE: i32 = 65536;
    #[test]
    fn full_volume_inside_the_minimum_distance_silent_beyond_the_maximum() {
        // The listener sits 38 units off the play plane, so 39 units is 8.8 units across it.
        assert_eq!(source_rolloff(0, 0, 39, 56), 4096);
        assert_eq!(source_rolloff(8 * ONE, 0, 39, 56), 4096);
        assert_eq!(source_rolloff(50 * ONE, 0, 39, 56), 0);
        assert_eq!(source_rolloff(-60 * ONE, 30 * ONE, 39, 56), 0);
    }
    #[test]
    fn falls_monotonically_between_them() {
        let mut last = 4096;
        for x in 9..=42 {
            let level = source_rolloff(x * ONE, 0, 39, 56);
            assert!(level <= last, "x={x}: {level} > {last}");
            last = level;
        }
        assert!(source_rolloff(20 * ONE, 0, 39, 56) > 0);
        assert!(source_rolloff(20 * ONE, 0, 39, 56) < 4096);
    }
}
