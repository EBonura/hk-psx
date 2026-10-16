//! Exact arithmetic on 64-bit operands, built from 32-bit steps only.
//!
//! The R3000 multiplies 32 by 32 into 64 bits in one instruction and divides 32 by 32
//! natively; it has no 64-bit divide, and a divide written with `u64` links a compiler
//! routine the guest's symbol gate refuses. Everything here is `psx_math::WideU`
//! products, adds and compares plus `psx_math::int32` divides. Where a divisor is wider
//! than 32 bits the quotient is estimated from its top 32 bits, then corrected against
//! the exact product, so the result is the true floor quotient.
use core::cmp::Ordering;
use psx_math::{Wide, WideU};

/// `floor((hi * 2^32 + lo) / d)` for `hi < d`: the SDK's two-word divide, as the one copy in
/// the image (an inlined copy is nearly 500 bytes and the guest has no room for several).
#[inline(never)]
pub fn div_wide(hi: u32, lo: u32, d: u32) -> u32 {
    psx_math::int32::div_u64_by_u32(hi, lo, d)
}

/// `floor(a * b / c)` when it fits `u32`, like `psx_math::int32::mul_div_u32` over the shared
/// divider.
#[inline(never)]
pub fn mul_div_u32(a: u32, b: u32, c: u32) -> u32 {
    let product = WideU::mul(a, b);
    if product.high() == 0 {
        product.low() / c
    } else {
        div_wide(product.high(), product.low(), c)
    }
}

/// The 128-bit product of two 64-bit values, as (high, low) 64-bit words.
#[inline(never)]
pub fn mul_u64(a: WideU, b: WideU) -> (WideU, WideU) {
    let low = WideU::mul(a.low(), b.low());
    let cross_a = WideU::mul(a.low(), b.high());
    let cross_b = WideU::mul(a.high(), b.low());
    let high = WideU::mul(a.high(), b.high());
    // Neither sum below can wrap: each stays under 2^64 until the second, whose carry is
    // read off the compare.
    let first = cross_a + WideU::of(low.high());
    let middle = first + cross_b;
    let carry = u32::from(middle < first);
    (
        high + WideU::from_parts(carry, middle.high()),
        WideU::from_parts(middle.low(), low.low()),
    )
}

/// Orders `a / b` against `c / d` (`b` and `d` nonzero) without dividing: `a * d`
/// against `c * b`, each a 128-bit product.
#[inline(never)]
pub fn cmp_fractions(a: WideU, b: WideU, c: WideU, d: WideU) -> Ordering {
    mul_u64(a, d).cmp(&mul_u64(c, b))
}

/// `(n / d, n % d)` for a 32-bit divisor, with the full 64-bit quotient.
#[inline(never)]
pub fn div_rem_u32(n: WideU, d: u32) -> (WideU, u32) {
    assert!(d != 0, "attempt to divide by zero");
    let carry = n.high() % d;
    let q_low = div_wide(carry, n.low(), d);
    let rest = WideU::from_parts(carry, n.low()) - WideU::mul(q_low, d);
    (WideU::from_parts(n.high() / d, q_low), rest.low())
}

/// The top 32 bits of a divisor wider than 32, and the shift that produced them.
fn narrowed(d: WideU) -> (u32, u32) {
    let shift = 32 - d.high().leading_zeros();
    (shift, (d >> shift).low())
}

/// Moves `q` to the floor of `n / d` given that it is within a few of it. `n` is a
/// 96-bit value as (high word, low 64 bits).
#[inline(never)]
fn settle(mut q: u32, n: (u32, WideU), d: WideU, limit: u32) -> u32 {
    let exact = (WideU::of(n.0), n.1);
    while mul_u64(WideU::of(q), d) > exact {
        q -= 1;
    }
    while q < limit && mul_u64(WideU::of(q + 1), d) <= exact {
        q += 1;
    }
    q
}

/// `floor(n / d)` for a nonzero `d` when the quotient fits `u32`.
#[inline(never)]
pub fn quotient(n: WideU, d: WideU) -> u32 {
    assert!(!d.is_zero(), "attempt to divide by zero");
    if d.high() == 0 {
        return div_rem_u32(n, d.low()).0.low();
    }
    let (shift, narrow) = narrowed(d);
    let n_narrow = n >> shift;
    let estimate = if n_narrow.high() < narrow {
        div_wide(n_narrow.high(), n_narrow.low(), narrow)
    } else {
        u32::MAX
    };
    settle(estimate, (0, n), d, u32::MAX)
}

/// `floor(a * t / den)` for `t <= den` and nonzero `den`; the result is at most `a`.
#[inline(never)]
pub fn scale_by_fraction(a: u32, t: WideU, den: WideU) -> u32 {
    assert!(!den.is_zero() && t <= den, "fraction outside [0, 1]");
    if den.high() == 0 {
        return mul_div_u32(a, t.low(), den.low());
    }
    let (shift, narrow) = narrowed(den);
    let estimate = mul_div_u32(a, (t >> shift).low(), narrow);
    let (hi, lo) = mul_u64(WideU::of(a), t);
    settle(estimate.min(a), (hi.low(), lo), den, a)
}

/// A `u64` as a `WideU`.
pub fn from_u64(x: u64) -> WideU {
    WideU::from_parts((x >> 32) as u32, x as u32)
}

/// A nonnegative `i64` as a `WideU`.
pub fn wide_u(x: i64) -> WideU {
    debug_assert!(x >= 0);
    WideU::from_parts((x >> 32) as u32, x as u32)
}

/// A `WideU` below 2^63 as an `i64`.
pub fn to_i64(x: WideU) -> i64 {
    (i64::from(x.high()) << 32) | i64::from(x.low())
}

/// The low 32 bits of `floor(n / d)`, like `n.div_euclid(d) as i32` for a positive `d`.
#[inline(never)]
pub fn floor_div_u32(n: Wide, d: u32) -> i32 {
    let magnitude = n.abs();
    let (q, r) = div_rem_u32(WideU::from_parts(magnitude.high() as u32, magnitude.low_u32()), d);
    let q = q.low();
    if n.is_negative() {
        q.wrapping_add(u32::from(r != 0)).wrapping_neg() as i32
    } else {
        q as i32
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    fn w(x: u64) -> WideU {
        WideU::from_parts((x >> 32) as u32, x as u32)
    }
    fn u(x: WideU) -> u64 {
        (x.high() as u64) << 32 | x.low() as u64
    }
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn magnitude(&mut self) -> u64 {
            let v = self.next();
            v >> (self.next() % 64)
        }
    }

    #[test]
    fn products_match_u128() {
        let mut r = Rng(0x9e3779b97f4a7c15);
        let edges = [0, 1, u32::MAX as u64, 1 << 32, u64::MAX, u64::MAX - 1, 1 << 63];
        let mut vals: std::vec::Vec<u64> = edges.to_vec();
        for _ in 0..3000 {
            vals.push(r.magnitude());
        }
        for &a in &vals {
            for &b in vals.iter().step_by(13) {
                let (h, l) = mul_u64(w(a), w(b));
                let p = a as u128 * b as u128;
                assert_eq!((u(h) as u128) << 64 | u(l) as u128, p, "{a} {b}");
            }
        }
    }

    #[test]
    fn fractions_order_like_u128() {
        let mut r = Rng(12345);
        for _ in 0..200_000 {
            let (a, c) = (r.magnitude(), r.magnitude());
            let (b, d) = (r.magnitude().max(1), r.magnitude().max(1));
            let want = (a as u128 * d as u128).cmp(&(c as u128 * b as u128));
            assert_eq!(cmp_fractions(w(a), w(b), w(c), w(d)), want);
            assert_eq!(cmp_fractions(w(a), w(b), w(a), w(b)), Ordering::Equal);
        }
    }

    #[test]
    fn division_by_a_word_matches() {
        let mut r = Rng(777);
        for _ in 0..200_000 {
            let n = r.magnitude();
            let d = (r.magnitude() as u32).max(1);
            let (q, m) = div_rem_u32(w(n), d);
            assert_eq!((u(q), m as u64), (n / d as u64, n % d as u64), "{n} / {d}");
        }
    }

    #[test]
    fn quotient_matches_when_it_fits() {
        let mut r = Rng(4242);
        for _ in 0..300_000 {
            let d = r.magnitude().max(1);
            let q = r.next() as u32 >> (r.next() % 32);
            let n = (q as u128 * d as u128 + (r.next() as u128 % d as u128)).min(u64::MAX as u128) as u64;
            if n / d > u32::MAX as u64 {
                continue;
            }
            assert_eq!(quotient(w(n), w(d)) as u64, n / d, "{n} / {d}");
        }
    }

    #[test]
    fn fraction_scaling_matches() {
        let mut r = Rng(99);
        for _ in 0..300_000 {
            let den = r.magnitude().max(1);
            let t = if r.next() % 4 == 0 { den } else { (r.next() as u128 % (den as u128 + 1)) as u64 };
            let a = r.next() as u32 >> (r.next() % 32);
            let want = (a as u128 * t as u128 / den as u128) as u32;
            assert_eq!(scale_by_fraction(a, w(t), w(den)), want, "{a} * {t} / {den}");
        }
    }

    #[test]
    fn floor_division_matches_div_euclid() {
        let mut r = Rng(31337);
        for _ in 0..200_000 {
            let n = (r.magnitude() as i64) * if r.next() % 2 == 0 { 1 } else { -1 };
            let d = (r.magnitude() as u32).max(1);
            let want = (n as i128).div_euclid(d as i128) as i64 as i32;
            assert_eq!(floor_div_u32(Wide::from_parts((n >> 32) as i32, n as u32), d), want, "{n} / {d}");
        }
    }
}
