//! Exact 64-bit division and remainder without a compiler-builtins call.
//!
//! The R3000 has no 64-bit divide, so `a / b` on `u64` or `i64` links
//! `__udivdi3` or `__divdi3` and `u64_div_rem`, 1.4 KB of branchy code that costs
//! hundreds of cycles a call, and the guest's symbol gate refuses it. These
//! give the same quotient and remainder as the native operators for every input
//! (the tests sweep the edges and a pseudo-random spread against them on the
//! host). A divisor that fits 32 bits, which is what the guest passes, takes the
//! SDK's two-word divide (`psx_math::int32::div_u64_by_u32`) plus one native
//! divide per word; a wider divisor falls back to a 64-step restoring division
//! built from constant shifts, add and compare only, which nothing in the
//! shipped routes reaches.
use psx_math::int32::div_u64_by_u32;

/// `(n / d, n % d)`. Panics when `d` is zero, like the native operators.
#[inline(never)]
pub fn div_rem_u64(n: u64, d: u64) -> (u64, u64) {
    assert!(d != 0, "attempt to divide by zero");
    let (high, low) = ((n >> 32) as u32, n as u32);
    if d >> 32 == 0 {
        let d = d as u32;
        let (q_high, carry) = (high / d, high % d);
        let q_low = div_u64_by_u32(carry, low, d);
        // The remainder is below `d`, so it fits a word.
        let rest = ((carry as u64) << 32 | low as u64) - q_low as u64 * d as u64;
        return ((q_high as u64) << 32 | q_low as u64, rest);
    }
    // psx-numeric-allow-next-line: restoring division, shifts, add and compare only
    let (mut quotient, mut remainder, mut bits) = (0u64, 0u64, n);
    for _ in 0..64 {
        let overflow = remainder >> 63 != 0;
        remainder = remainder << 1 | bits >> 63;
        bits <<= 1;
        quotient <<= 1;
        if overflow || remainder >= d {
            remainder = remainder.wrapping_sub(d);
            quotient |= 1;
        }
    }
    (quotient, remainder)
}

/// `n / d`.
#[inline(never)]
pub fn div_u64(n: u64, d: u64) -> u64 {
    div_rem_u64(n, d).0
}

/// `n % d`.
#[inline(never)]
pub fn rem_u64(n: u64, d: u64) -> u64 {
    div_rem_u64(n, d).1
}

/// `n / d`, truncated toward zero like the native operator. `i64::MIN / -1`
/// wraps where the native operator panics.
#[inline(never)]
pub fn div_i64(n: i64, d: i64) -> i64 {
    let magnitude = div_u64(n.unsigned_abs(), d.unsigned_abs());
    if (n < 0) != (d < 0) {
        magnitude.wrapping_neg() as i64
    } else {
        magnitude as i64
    }
}

/// `n % d`, with the sign of `n` like the native operator.
#[inline(never)]
pub fn rem_i64(n: i64, d: i64) -> i64 {
    let magnitude = rem_u64(n.unsigned_abs(), d.unsigned_abs());
    if n < 0 {
        magnitude.wrapping_neg() as i64
    } else {
        magnitude as i64
    }
}

/// `n.div_euclid(d)`: the quotient whose remainder is never negative.
#[inline(never)]
pub fn div_euclid_i64(n: i64, d: i64) -> i64 {
    let q = div_i64(n, d);
    if rem_i64(n, d) < 0 {
        if d > 0 {
            q - 1
        } else {
            q + 1
        }
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn spread() -> Vec<u64> {
        let mut values = Vec::new();
        for shift in 0..64 {
            let base = 1u64 << shift;
            for delta in [0u64, 1, 2, 3, 255, 65535] {
                values.push(base.wrapping_add(delta));
                values.push(base.wrapping_sub(delta));
            }
        }
        values.extend([0, 1, u64::MAX, u64::MAX - 1, u32::MAX as u64, u32::MAX as u64 + 1]);
        // xorshift: deterministic, every magnitude.
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..4000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            values.push(state >> (state & 63));
        }
        values
    }

    #[test]
    fn unsigned_division_matches_the_native_operators() {
        let values = spread();
        for &n in &values {
            for &d in values.iter().filter(|&&d| d != 0) {
                assert_eq!(div_rem_u64(n, d), (n / d, n % d), "{n} / {d}");
            }
        }
    }

    #[test]
    fn signed_division_matches_the_native_operators() {
        let values = spread();
        for &u in &values {
            for &v in values.iter().step_by(7) {
                for (n, d) in [(u as i64, v as i64), ((u as i64).wrapping_neg(), v as i64), (u as i64, (v as i64).wrapping_neg()), ((u as i64).wrapping_neg(), (v as i64).wrapping_neg())] {
                    if d == 0 || (n == i64::MIN && d == -1) {
                        continue;
                    }
                    assert_eq!(div_i64(n, d), n / d, "{n} / {d}");
                    assert_eq!(rem_i64(n, d), n % d, "{n} % {d}");
                    assert_eq!(div_euclid_i64(n, d), n.div_euclid(d), "{n}.div_euclid({d})");
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "divide by zero")]
    fn a_zero_divisor_panics() {
        div_u64(1, 0);
    }
}
