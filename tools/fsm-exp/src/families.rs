//! Handlers used by the experiment: a stub for every family (the dispatch benchmark and
//! the bank runs) and thirteen real handlers whose byte counts replace the 90 to 260 B
//! per type DESIGN.md assumed. Operand orders are the ones `fsm3-sizes` encodes.
use crate::kernel::*;

#[inline(always)]
fn fin_or_cont(c: &Ctx) -> u16 {
    if c.flags & F_UPDATE != 0 {
        R_CONT
    } else {
        R_FIN
    }
}

/// Stub for a family not written: finishes on entry, continues on update.
pub unsafe extern "C" fn stub(c: &mut Ctx, op: u8) -> u16 {
    let _ = op;
    fin_or_cont(c)
}

/// Native service shims: stand-ins for the entity table, so the handlers that call one are
/// measured with a real `jal` out of the module, as the guest does.
pub static mut SERVICE_LOG: [i32; 4] = [0; 4];

#[inline(never)]
pub unsafe extern "C" fn svc_set_velocity(obj: u8, x: i32, y: i32) {
    core::ptr::write_volatile(&raw mut SERVICE_LOG[0], obj as i32 ^ x ^ y);
}

// ---- flow/compare (family 0) -----------------------------------------------------------

#[inline(never)]
pub unsafe extern "C" fn bool_test(c: &mut Ctx, _op: u8) -> u16 {
    let e = k_event(c, if k_bit(c, 0) { 1 } else { 2 });
    if e != 0xff {
        return r_event(e);
    }
    fin_or_cont(c)
}

#[inline(never)]
pub unsafe extern "C" fn float_compare(c: &mut Ctx, _op: u8) -> u16 {
    let a = k_val(c, 0, true);
    let b = k_val(c, 1, true);
    let tol = k_val(c, 2, true);
    let d = a.wrapping_sub(b);
    let which = if d.wrapping_abs() <= tol {
        3
    } else if d < 0 {
        4
    } else {
        5
    };
    let e = k_event(c, which);
    if e != 0xff {
        return r_event(e);
    }
    fin_or_cont(c)
}

#[inline(never)]
pub unsafe extern "C" fn int_compare(c: &mut Ctx, _op: u8) -> u16 {
    let a = k_val(c, 0, false);
    let b = k_val(c, 1, false);
    let which = if a == b {
        2
    } else if a < b {
        3
    } else {
        4
    };
    let e = k_event(c, which);
    if e != 0xff {
        return r_event(e);
    }
    fin_or_cont(c)
}

/// events[], weights[], delay: arrays are not decoded here, the first two events stand in.
#[inline(never)]
pub unsafe extern "C" fn send_random_event(c: &mut Ctx, _op: u8) -> u16 {
    let r = k_rand(c);
    let e = k_event(c, (r & 1) as u8);
    k_send(c, e, (k_val(c, 2, false) as u16).min(60));
    R_FIN
}

#[inline(never)]
pub unsafe extern "C" fn next_frame_event(c: &mut Ctx, _op: u8) -> u16 {
    let e = k_event(c, 0);
    k_send(c, e, 1);
    R_FIN
}

pub unsafe extern "C" fn flow_entry(c: &mut Ctx, op: u8) -> u16 {
    match op {
        3 => bool_test(c, op),
        9 => float_compare(c, op),
        17 => int_compare(c, op),
        20 => next_frame_event(c, op),
        24 => send_random_event(c, op),
        _ => fin_or_cont(c),
    }
}

// ---- math/vars (family 1) ---------------------------------------------------------------

#[inline(never)]
pub unsafe extern "C" fn set_float(c: &mut Ctx, _op: u8) -> u16 {
    let v = k_val(c, 1, true);
    k_set(c, 0, v);
    fin_or_cont(c)
}

#[inline(never)]
pub unsafe extern "C" fn set_int(c: &mut Ctx, _op: u8) -> u16 {
    let v = k_val(c, 1, false);
    k_set(c, 0, v);
    fin_or_cont(c)
}

#[inline(never)]
pub unsafe extern "C" fn set_bool(c: &mut Ctx, _op: u8) -> u16 {
    let v = k_bit(c, 1);
    k_set_bit(c, 0, v);
    fin_or_cont(c)
}

#[inline(never)]
pub unsafe extern "C" fn float_multiply(c: &mut Ctx, _op: u8) -> u16 {
    let a = k_val(c, 0, true);
    let b = k_val(c, 1, true);
    // Q16.16 multiply without a 64-bit intermediate: split the multiplier.
    let hi = (b >> 16).wrapping_mul(a);
    let lo = ((b & 0xffff) as u32 as i32).wrapping_mul(a >> 8) >> 8;
    k_set(c, 0, hi.wrapping_add(lo));
    fin_or_cont(c)
}

#[inline(never)]
pub unsafe extern "C" fn random_float(c: &mut Ctx, _op: u8) -> u16 {
    let lo = k_val(c, 0, true);
    let hi = k_val(c, 1, true);
    let span = hi.wrapping_sub(lo) as u32;
    let r = (k_rand(c) >> 16) as u32;
    let v = lo.wrapping_add((((span >> 8).wrapping_mul(r)) >> 8) as i32);
    k_set(c, 2, v);
    fin_or_cont(c)
}

pub unsafe extern "C" fn math_entry(c: &mut Ctx, op: u8) -> u16 {
    match op {
        13 => float_multiply(c, op),
        25 => random_float(c, op),
        32 => set_bool(c, op),
        36 => set_float(c, op),
        39 => set_int(c, op),
        _ => fin_or_cont(c),
    }
}

// ---- timers (family 3) ------------------------------------------------------------------

/// Wait: time, finishEvent, realTime. Time in ticks of 1/60 s from Q16.16 seconds.
#[inline(never)]
pub unsafe extern "C" fn wait(c: &mut Ctx, _op: u8) -> u16 {
    let s = k_scratch(c, 0);
    if c.enter {
        let secs = k_val(c, 0, true);
        *s = (((secs as u32) >> 8).wrapping_mul(60) >> 8) + 1;
    }
    if *s > 1 {
        *s -= 1;
        return R_CONT;
    }
    let e = k_event(c, 1);
    if e != 0xff {
        return r_event(e);
    }
    R_FIN
}

#[inline(never)]
pub unsafe extern "C" fn wait_random(c: &mut Ctx, _op: u8) -> u16 {
    let s = k_scratch(c, 0);
    if c.enter {
        let lo = k_val(c, 0, true) as u32;
        let hi = k_val(c, 1, true) as u32;
        let span = hi.wrapping_sub(lo) >> 8;
        let secs = lo.wrapping_add((span.wrapping_mul(k_rand(c) >> 16)) >> 8);
        *s = ((secs >> 8).wrapping_mul(60) >> 8) + 1;
    }
    if *s > 1 {
        *s -= 1;
        return R_CONT;
    }
    let e = k_event(c, 2);
    if e != 0xff {
        return r_event(e);
    }
    R_FIN
}

pub unsafe extern "C" fn timers_entry(c: &mut Ctx, op: u8) -> u16 {
    match op {
        1 => wait(c, op),
        4 => wait_random(c, op),
        _ => fin_or_cont(c),
    }
}

// ---- events/messaging (family 2) --------------------------------------------------------

/// SendEventByName: eventTarget (3 operands), sendEvent, delay.
#[inline(never)]
pub unsafe extern "C" fn send_event_by_name(c: &mut Ctx, _op: u8) -> u16 {
    let e = k_event(c, 3);
    let delay = k_val(c, 4, true);
    k_send(c, e, (((delay as u32) >> 8).wrapping_mul(60) >> 8) as u16);
    R_FIN
}

pub unsafe extern "C" fn events_entry(c: &mut Ctx, op: u8) -> u16 {
    match op {
        16 => send_event_by_name(c, op),
        _ => fin_or_cont(c),
    }
}

// ---- transform/move (family 4): a handler that calls a native service --------------------

/// SetVelocity2d: owner flag decides whether operand 0 is the object.
#[inline(never)]
pub unsafe extern "C" fn set_velocity_2d(c: &mut Ctx, _op: u8) -> u16 {
    let s = (c.flags & F_OWNER == 0) as u8;
    let obj = if s != 0 { k_obj(c, 0) } else { 0 };
    let x = k_val(c, s + 1, true);
    let y = k_val(c, s + 2, true);
    svc_set_velocity(obj, x, y);
    fin_or_cont(c)
}

pub unsafe extern "C" fn move_entry(c: &mut Ctx, op: u8) -> u16 {
    match op {
        34 => set_velocity_2d(c, op),
        _ => fin_or_cont(c),
    }
}

/// Bind the real family entries; the rest keep the stub.
pub unsafe fn bind() {
    FAMILY_ENTRY[0] = flow_entry;
    FAMILY_ENTRY[1] = math_entry;
    FAMILY_ENTRY[2] = events_entry;
    FAMILY_ENTRY[3] = timers_entry;
    FAMILY_ENTRY[4] = move_entry;
}
