//! HKFSM03 family 3: timers.
use crate::fsm3::{r_with, Ctx, PH_ENTER, R_CONT};
use hk_format::fsm3::ops::timers;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == timers::WAIT {
        wait(c)
    } else {
        R_CONT
    }
}

/// Wait [time, finishEvent, realTime, scratch slot]. The scratch word counts the remaining time in
/// Q16.16 simulation ticks; a realTime wait also counts ticks the game was paused for.
#[inline(never)]
fn wait(c: &mut Ctx<'_, '_>) -> u16 {
    let slot = c.int(3) as u8;
    let remaining = if c.phase == PH_ENTER {
        c.fixed(0).max(0) * 60
    } else {
        let elapsed = if c.bit(2) { c.env().dt_real } else { c.env().dt_scaled };
        c.scratch32(slot).saturating_sub(elapsed).max(0)
    };
    c.set_scratch32(slot, remaining);
    let done = remaining == 0;
    r_with(done, if done { c.event(1) } else { hk_format::fsm3::NONE8 })
}
