//! HKFSM03 family 0: flow and comparison actions.
use crate::fsm3::{r_event, r_stateless, r_with, Ctx, PH_UPDATE, R_CONT, R_FIN};
use hk_format::fsm3::{ops::flow, NONE8};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == flow::BOOL_TEST {
        bool_test(c)
    } else if op == flow::FLOAT_COMPARE {
        float_compare(c)
    } else if op == flow::FLOAT_SWITCH {
        float_switch(c)
    } else if op == flow::PLAYER_DATA_BOOL_TEST {
        // PlayerDataBoolTest [field slot, isTrue, isFalse]
        let e = if c.pd_read() != 0 { c.event(1) } else { c.event(2) };
        r_with(r_stateless(c.flags) & R_FIN != 0, e)
    } else if op == flow::BOOL_ALL_TRUE {
        bool_all_true(c)
    } else if op == flow::NEXT_FRAME_EVENT {
        // NextFrameEvent [event]: finishes on its first update, not on enter, raising the event then.
        if c.phase == PH_UPDATE {
            r_with(true, c.event(0))
        } else {
            R_CONT
        }
    } else {
        R_CONT
    }
}

/// BoolTest [variable, isTrue, isFalse]: raise one by the variable. Finished unless it runs every frame.
#[inline(never)]
fn bool_test(c: &mut Ctx<'_, '_>) -> u16 {
    let e = if c.bit(0) { c.event(1) } else { c.event(2) };
    r_with(r_stateless(c.flags) & R_FIN != 0, e)
}

/// FloatCompare [float1, float2, tolerance, equal, lessThan, greaterThan].
#[inline(never)]
fn float_compare(c: &mut Ctx<'_, '_>) -> u16 {
    let (x, y, tolerance) = (c.fixed(0), c.fixed(1), c.fixed(2));
    let e = if x.abs_diff(y) <= tolerance as u32 {
        c.event(3)
    } else if x < y {
        c.event(4)
    } else {
        c.event(5)
    };
    r_with(r_stateless(c.flags) & R_FIN != 0, e)
}

/// FloatSwitch [value, lessThan[], sendEvent[]]: the first threshold above the value picks the event.
#[inline(never)]
fn float_switch(c: &mut Ctx<'_, '_>) -> u16 {
    let v = c.fixed(0);
    let (n, limits) = c.array(1);
    let (_, events) = c.array(2);
    let mut e = NONE8;
    for i in 0..n {
        let w = c.array_word(limits, i);
        if v < c.value_of(w, true) {
            let w = c.array_word(events, i);
            e = c.event_of(w);
            break;
        }
    }
    r_with(r_stateless(c.flags) & R_FIN != 0, e)
}

/// BoolAllTrue [variables[], trueEvent, falseEvent]: an empty list raises nothing.
#[inline(never)]
fn bool_all_true(c: &mut Ctx<'_, '_>) -> u16 {
    let (n, first) = c.array(0);
    let mut all = n != 0;
    for i in 0..n {
        let w = c.array_word(first, i);
        if !c.bit_of(w) {
            all = false;
            break;
        }
    }
    let e = if n == 0 {
        NONE8
    } else if all {
        c.event(1)
    } else {
        c.event(2)
    };
    let _ = r_event;
    r_with(r_stateless(c.flags) & R_FIN != 0, e)
}
