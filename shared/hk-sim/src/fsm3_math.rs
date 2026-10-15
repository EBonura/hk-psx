//! HKFSM03 family 1: variables and arithmetic.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use hk_format::fsm3::ops::math;
use psx_math::int32::mul_shr_i32;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == math::SET_BOOL_VALUE {
        // SetBoolValue [variable, value]
        let v = c.bit(1);
        c.set_bit(0, v);
    } else if op == math::SET_FLOAT_VALUE {
        // SetFloatValue [variable, value]
        let v = c.fixed(1);
        c.set(0, v);
    } else if op == math::RANDOM_FLOAT {
        random_float(c);
    } else if op == math::FLOAT_CLAMP {
        // FloatClamp [variable, min, max]
        let (lo, hi) = (c.fixed(1), c.fixed(2));
        let v = c.fixed(0).clamp(lo.min(hi), hi.max(lo));
        c.set(0, v);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

/// RandomFloat [min, max, store]: min + (draw / 2^24) * (max - min), the draw being 24 random bits.
/// min <= max is the cook's to guarantee for literals.
#[inline(never)]
fn random_float(c: &mut Ctx<'_, '_>) {
    let (low, high) = (c.fixed(0), c.fixed(1));
    let draw = (c.rand() >> 8) as i32;
    let v = low + mul_shr_i32(draw, high - low, 24);
    #[cfg(feature = "trace")]
    let v = {
        let e = c.env();
        if e.forced_n > 0 {
            let f = e.forced_random[0];
            e.forced_random.copy_within(1.., 0);
            e.forced_n -= 1;
            f
        } else {
            v
        }
    };
    c.set(2, v);
}
