//! Boss tier, family 0: flow and comparison actions.
use crate::fsm3::{r_stateless, r_with, Ctx, PH_UPDATE, R_CONT, R_FIN};
use hk_format::fsm3::{ops::flow, NONE8};

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == flow::BOOL_TEST {
        // BoolTest [boolVariable, isTrue, isFalse]
        let e = if c.bit(0) { c.event(1) } else { c.event(2) };
        r_with(fin(c), e)
    } else if op == flow::BOOL_TEST_MULTI {
        bool_test_multi(c)
    } else if op == flow::FLOAT_COMPARE {
        // FloatCompare [float1, float2, tolerance, equal, lessThan, greaterThan]
        let (x, y, tolerance) = (c.fixed(0), c.fixed(1), c.fixed(2));
        let e = if x.abs_diff(y) <= tolerance as u32 {
            c.event(3)
        } else if x < y {
            c.event(4)
        } else {
            c.event(5)
        };
        r_with(fin(c), e)
    } else if op == flow::FLOAT_IN_RANGE {
        // FloatInRange [floatVariable, lowerValue, upperValue, boolVariable, trueEvent, falseEvent]
        let v = c.fixed(0);
        let inside = v >= c.fixed(1) && v <= c.fixed(2);
        c.set_bit(3, inside);
        r_with(fin(c), if inside { c.event(4) } else { c.event(5) })
    } else if op == flow::FLOAT_TEST_TO_BOOL {
        // FloatTestToBool [float1, float2, tolerance, equalBool, lessThanBool, greaterThanBool]
        let (x, y, tolerance) = (c.fixed(0), c.fixed(1), c.fixed(2));
        c.set_bit(3, x.abs_diff(y) <= tolerance as u32);
        c.set_bit(4, x < y);
        c.set_bit(5, x > y);
        r_stateless(c.flags)
    } else if op == flow::INT_COMPARE {
        // IntCompare [integer1, integer2, equal, lessThan, greaterThan]
        let (x, y) = (c.int(0), c.int(1));
        let e = if x == y {
            c.event(2)
        } else if x < y {
            c.event(3)
        } else {
            c.event(4)
        };
        r_with(fin(c), e)
    } else if op == flow::NEXT_FRAME_EVENT {
        // NextFrameEvent [event]: finishes on its first update, not on enter, raising the event then.
        if c.phase == PH_UPDATE {
            r_with(true, c.event(0))
        } else {
            R_CONT
        }
    } else if op == flow::PLAYER_DATA_BOOL_TEST {
        // PlayerDataBoolTest [gameObject?, field slot, isTrue, isFalse]
        let b = c.skip();
        let e = if c.pd_read_at(b) != 0 { c.event(b + 1) } else { c.event(b + 2) };
        r_with(fin(c), e)
    } else if op == flow::SEND_RANDOM_EVENT {
        send_random(c)
    } else if op == flow::SEND_RANDOM_EVENT_V2 {
        send_random_v2(c)
    } else if op == flow::SEND_RANDOM_EVENT_V3 {
        send_random_v3(c)
    } else {
        R_CONT
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Finished unless the action runs every frame.
#[inline(always)]
fn fin(c: &Ctx<'_, '_>) -> bool {
    r_stateless(c.flags) & R_FIN != 0
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// BoolTestMulti [boolVariables[], boolStates[], trueEvent, falseEvent, storeResult]: every variable equals its state.
#[inline(never)]
fn bool_test_multi(c: &mut Ctx<'_, '_>) -> u16 {
    let (n, vars) = c.array(0);
    let (m, states) = c.array(1);
    if n == 0 || n != m {
        return r_stateless(c.flags);
    }
    let mut all = true;
    for i in 0..n {
        let (a, b) = (c.array_word(vars, i), c.array_word(states, i));
        if c.bit_of(a) != c.bit_of(b) {
            all = false;
            break;
        }
    }
    c.set_bit(4, all);
    r_with(fin(c), if all { c.event(2) } else { c.event(3) })
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Oracle replay: the index of the event the original drew, when the replay says which.
#[cfg(feature = "trace")]
fn forced_index(c: &mut Ctx<'_, '_>, events: usize, n: usize) -> Option<usize> {
    if c.env().forced_events_n == 0 {
        return None;
    }
    // 0xfe: the original drew an entry with no event.
    let want = c.env().forced_events[0];
    let e = c.env();
    e.forced_events.copy_within(1.., 0);
    e.forced_events_n -= 1;
    for i in 0..n {
        let ev = c.event_of(c.array_word(events, i));
        if (want == 0xfe && ev == NONE8) || ev == want {
            return Some(i);
        }
    }
    None
}
#[cfg_attr(target_arch = "mips", optimize(size))]
#[cfg(not(feature = "trace"))]
#[inline(always)]
fn forced_index(_: &mut Ctx<'_, '_>, _: usize, _: usize) -> Option<usize> {
    None
}

/// `GetRandomWeightedIndex`: a draw over the sum of the weights; None when no weight is positive.
#[inline(never)]
pub(crate) fn weighted_index(c: &mut Ctx<'_, '_>, weights: usize, n: usize) -> Option<usize> {
    let mut total = 0i32;
    for i in 0..n {
        let w = c.array_word(weights, i);
        total = total.saturating_add(c.value_of(w, true).max(0));
    }
    if total <= 0 {
        return None;
    }
    let draw = (c.rand() >> 8) as i32;
    let mut r = psx_math::int32::mul_shr_i32(draw, total, 24);
    for i in 0..n {
        let w = c.array_word(weights, i);
        let weight = c.value_of(w, true).max(0);
        if r < weight {
            return Some(i);
        }
        r -= weight;
    }
    Some(n - 1)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// SendRandomEvent [events[], weights[], delay]: one weighted event, now or after `delay` seconds.
#[inline(never)]
fn send_random(c: &mut Ctx<'_, '_>) -> u16 {
    let (n, events) = c.array(0);
    let (_, weights) = c.array(1);
    if n == 0 {
        return r_stateless(c.flags);
    }
    let Some(i) = forced_index(c, events, n).or_else(|| weighted_index(c, weights, n)) else { return r_stateless(c.flags) };
    let ev = c.event_of(c.array_word(events, i));
    let delay = c.fixed(2);
    if delay < 66 {
        // Under a millisecond the event is raised at once, as the original does.
        r_with(true, ev)
    } else {
        let f = c.f;
        c.send(ev, f, super::ticks_of(delay));
        R_FIN
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// SendRandomEventV2 [events[], weights[], trackingInts[], eventMax[]]: as above, but an event that has been sent
/// `eventMax` times in a row is not sent again; the counts of the others reset.
#[inline(never)]
fn send_random_v2(c: &mut Ctx<'_, '_>) -> u16 {
    let (n, events) = c.array(0);
    let (_, weights) = c.array(1);
    let (_, tracking) = c.array(2);
    let (_, max) = c.array(3);
    // The original redraws until an event is accepted; a bank that can never accept one is stopped after a bounded
    // number of draws instead of hanging the frame.
    for _ in 0..64 {
        let Some(i) = forced_index(c, events, n).or_else(|| weighted_index(c, weights, n)) else { break };
        let (tw, mw) = (c.array_word(tracking, i), c.array_word(max, i));
        let count = c.value_of(tw, false);
        if count < c.value_of(mw, false) {
            for j in 0..n {
                let w = c.array_word(tracking, j);
                c.set_of(w, 0);
            }
            c.set_of(tw, count + 1);
            return r_with(true, c.event_of(c.array_word(events, i)));
        }
    }
    R_FIN
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// SendRandomEventV3 [events[], weights[], trackingInts[], eventMax[], trackingIntsMissed[], missedMax[]]: V2, and an
/// event that has gone unsent `missedMax` times is sent next whatever the draw.
#[inline(never)]
fn send_random_v3(c: &mut Ctx<'_, '_>) -> u16 {
    let (n, events) = c.array(0);
    let (_, weights) = c.array(1);
    let (_, tracking) = c.array(2);
    let (_, max) = c.array(3);
    let (_, missed) = c.array(4);
    let (_, missed_max) = c.array(5);
    for _ in 0..64 {
        let Some(i) = forced_index(c, events, n).or_else(|| weighted_index(c, weights, n)) else { break };
        // An event overdue by the miss counter wins (the last one that is).
        let mut forced = NONE8 as usize;
        for j in 0..n {
            let (mw, xw) = (c.array_word(missed, j), c.array_word(missed_max, j));
            if c.value_of(mw, false) >= c.value_of(xw, false) {
                forced = j;
            }
        }
        if forced != NONE8 as usize {
            for j in 0..n {
                let (tw, mw) = (c.array_word(tracking, j), c.array_word(missed, j));
                c.set_of(tw, 0);
                let m = c.value_of(mw, false);
                c.set_of(mw, m + 1);
            }
            let (tw, mw) = (c.array_word(tracking, forced), c.array_word(missed, forced));
            c.set_of(mw, 0);
            c.set_of(tw, 1);
            return r_with(true, c.event_of(c.array_word(events, forced)));
        }
        let (tw, xw) = (c.array_word(tracking, i), c.array_word(max, i));
        let count = c.value_of(tw, false);
        if count < c.value_of(xw, false) {
            for j in 0..n {
                let (w, mw) = (c.array_word(tracking, j), c.array_word(missed, j));
                c.set_of(w, 0);
                let m = c.value_of(mw, false);
                c.set_of(mw, m + 1);
            }
            let mw = c.array_word(missed, i);
            c.set_of(tw, count + 1);
            c.set_of(mw, 0);
            return r_with(true, c.event_of(c.array_word(events, i)));
        }
    }
    R_FIN
}
