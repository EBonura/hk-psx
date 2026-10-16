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
