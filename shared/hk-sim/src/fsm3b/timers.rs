//! Boss tier, family 3: timers.
use crate::fsm3::{r_with, Ctx, PH_ENTER, R_CONT};
use hk_format::fsm3::{ops::timers, NONE8};

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == timers::WAIT {
        // Wait [time, finishEvent, realTime, scratch slot]
        count(c, 3, None)
    } else if op == timers::WAIT_RANDOM {
        // WaitRandom [timeMin, timeMax, finishEvent, realTime, scratch slot]: the time is drawn on entry.
        let t = if c.phase == PH_ENTER {
            let (low, high) = (c.fixed(0), c.fixed(1));
            let draw = (c.rand() >> 8) as i32;
            #[cfg(feature = "trace")]
            let draw_time = {
                let at = (c.f as usize).min(23);
                let f = c.env().forced_wait[at];
                c.env().forced_wait[at] = -1;
                if f >= 0 {
                    f
                } else {
                    low + psx_math::int32::mul_shr_i32(draw, high - low, 24)
                }
            };
            #[cfg(not(feature = "trace"))]
            let draw_time = low + psx_math::int32::mul_shr_i32(draw, high - low, 24);
            draw_time
        } else {
            0
        };
        count(c, 4, Some(t))
    } else {
        R_CONT
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// The scratch word counts the time left in Q16.16 simulation ticks; a realTime wait also counts ticks the game was paused for.
/// `drawn` is the time WaitRandom chose on entry (used only on entry).
#[inline(never)]
fn count(c: &mut Ctx<'_, '_>, slot_at: u8, drawn: Option<i32>) -> u16 {
    let slot = c.int(slot_at) as u8;
    let (event, real) = if drawn.is_some() { (c.event(2), c.bit(3)) } else { (c.event(1), c.bit(2)) };
    let remaining = if c.phase == PH_ENTER {
        drawn.unwrap_or_else(|| c.fixed(0)).max(0).saturating_mul(60)
    } else {
        let elapsed = if real { c.env().dt_real } else { c.env().dt_scaled };
        c.scratch32(slot).saturating_sub(elapsed).max(0)
    };
    c.set_scratch32(slot, remaining);
    let done = remaining == 0;
    r_with(done, if done { event } else { NONE8 })
}
