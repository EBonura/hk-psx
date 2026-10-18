//! Boss tier, family 1: variables and arithmetic.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use hk_format::fsm3::ops::math;
use psx_math::int32::{mul_div_i32, mul_shr_i32};

#[cfg_attr(target_arch = "mips", optimize(size))]
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
    } else if op == math::SET_INT_VALUE {
        // SetIntValue [variable, value]
        let v = c.int(1);
        c.set(0, v);
    } else if op == math::SET_GAME_OBJECT {
        // SetGameObject [variable, gameObject]
        let h = c.obj(1);
        c.set_obj(0, h);
    } else if op == math::RANDOM_FLOAT {
        // RandomFloat [min, max, store]: min + (draw / 2^24) * (max - min), the draw being 24 random bits.
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
    } else if op == math::FLOAT_MULTIPLY {
        // FloatMultiply [variable, multiplyBy]
        let v = mul_shr_i32(c.fixed(0), c.fixed(1), 16);
        c.set(0, v);
    } else if op == math::FLOAT_CLAMP {
        // FloatClamp [variable, minValue, maxValue]
        let (lo, hi) = (c.fixed(1), c.fixed(2));
        let v = c.fixed(0).clamp(lo, hi.max(lo));
        c.set(0, v);
    } else if op == math::FLOAT_SUBTRACT {
        // FloatSubtract [variable, subtract, perSecond]: per second scales by the frame time.
        let mut d = c.fixed(1);
        if c.bit(2) {
            d = mul_div_i32(d, c.env().dt_scaled, 60 << 16);
        }
        let v = c.fixed(0) - d;
        c.set(0, v);
    } else if op == math::SET_VECTOR3_VALUE {
        // SetVector3Value [variable, value]
        for k in 0..3u8 {
            let v = c.comp(1, k);
            c.set_comp(0, k, v);
        }
    } else if op == math::INT_ADD || op == math::INT_ADD_V2 {
        // IntAdd [variable, add]
        let v = c.int(0).wrapping_add(c.int(1));
        c.set(0, v);
    } else if op == math::FLOAT_OPERATOR {
        // FloatOperator [float1, float2, operation, store]: add, subtract, multiply, divide, min, max.
        let (x, y) = (c.fixed(0), c.fixed(1));
        let o = c.int(2);
        let r = if o == 0 {
            x.wrapping_add(y)
        } else if o == 1 {
            x.wrapping_sub(y)
        } else if o == 2 {
            mul_shr_i32(x, y, 16)
        } else if o == 3 {
            if y == 0 {
                0
            } else {
                mul_div_i32(x, 1 << 16, y)
            }
        } else if o == 4 {
            x.min(y)
        } else {
            x.max(y)
        };
        c.set(3, r);
    } else if op == math::INT_OPERATOR {
        // IntOperator [integer1, integer2, operation, store]
        let (x, y) = (c.int(0), c.int(1));
        let o = c.int(2);
        let r = if o == 0 {
            x.wrapping_add(y)
        } else if o == 1 {
            x.wrapping_sub(y)
        } else if o == 2 {
            x.wrapping_mul(y)
        } else if o == 3 {
            if y == 0 {
                0
            } else {
                x / y
            }
        } else if o == 4 {
            x.min(y)
        } else {
            x.max(y)
        };
        c.set(3, r);
    } else if op == math::VECTOR3_ADD_XYZ {
        // Vector3AddXYZ [vector, addX, addY, addZ, perSecond]
        let per = c.bit(4);
        for k in 0..3u8 {
            let mut d = c.fixed(1 + k);
            if per {
                d = mul_div_i32(d, c.env().dt_scaled, 60 << 16);
            }
            let v = c.comp(0, k) + d;
            c.set_comp(0, k, v);
        }
    } else {
        return super::unknown(c, 1, op);
    }
    r_stateless(c.flags)
}
