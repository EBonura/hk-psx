//! Boss tier, family 2: events and messaging.
//!
//! Cross-FSM variable access is resolved by the cooker: `SetFsm*` / `GetFsm*` carry the FSM index and the variable slot of
//! the (FSM name, variable name) pair, which must be unique in the tree; a pair the tree does not have is the engine's
//! (the `Recoil` component's "recoil" FSM, say) and is sent as a request.
use crate::fsm3::{r_event, r_stateless, Ctx, R_CONT};
use crate::fsm3_world::k;
use hk_format::fsm3::{ops::events, NONE8};

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == events::SEND_EVENT_BY_NAME {
        send_event_by_name(c)
    } else if op == events::TRIGGER2D_EVENT {
        // Trigger2dEvent [triggerType, collideTag, collideLayer, sendEvent, store]: the hero entering (0), inside (1) or
        // leaving (2) the trigger volume of the object the FSM is on, as the adapter found this step. It stays active.
        let kind = c.int(0) as u8 & 3;
        let owner = c.fsm_owner(c.f);
        let trig = c.world().get(owner).map_or(0, |e| e.trig);
        if trig & (1 << kind) != 0 {
            return r_event(c.event(3));
        }
        R_CONT
    } else if op == events::SET_FSM_BOOL || op == events::SET_FSM_FLOAT || op == events::SET_FSM_STRING || op == events::SET_FSM_GAME_OBJECT {
        set_fsm(c, op)
    } else if op == events::GET_FSM_INT || op == events::GET_FSM_GAME_OBJECT {
        get_fsm(c, op)
    } else if op == events::CALL_METHOD_PROPER {
        // CallMethodProper [gameObject, behaviour, methodName, parameters[], storeResult]: a call into a native component.
        let (behaviour, method) = (c.int(1), c.int(2));
        let (n, first) = c.array(3);
        let arg = if n > 0 { c.value_of(c.array_word(first, 0), false) } else { 0 };
        let object = c.obj(0);
        c.world().request(k::CALL, object, 0, [behaviour, method, arg]);
        r_stateless(c.flags)
    } else if op == events::SEND_MESSAGE {
        // SendMessage [gameObject, delivery, options, method, argument]: the same, to an object's scripts.
        let (method, arg) = (c.int(3), c.int(4));
        let object = c.obj(0);
        c.world().request(k::CALL, object, 1, [-1, method, arg]);
        r_stateless(c.flags)
    } else {
        super::unknown(c, 2, op)
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// SendEventByName [target mode | flags, target object, fsm name, event, delay]. The mode is the low nibble of the first
/// operand (0 self, 1 game object, 2 game object FSM, 3 FSM component, 4 broadcast all, 5 host FSM, 6 sub FSMs), bit 4 excludes
/// the sender and bit 5 includes the object's children. Recipients inside the tree get the event after the delay; the rest of
/// the scene (a camera shaker, the gates, the music region) is told through a request.
#[inline(never)]
fn send_event_by_name(c: &mut Ctx<'_, '_>) -> u16 {
    let word = c.int(0);
    let (mode, exclude, children) = (word & 15, word & 16 != 0, word & 32 != 0);
    let object = c.obj(1);
    let ev = c.event(3);
    let ticks = super::ticks_of(c.fixed(4));
    if ev == NONE8 {
        return r_stateless(c.flags);
    }
    let me = c.f;
    let n = c.t.bank.fsm_count();
    // The object the event is addressed to: the sender's own when none is named.
    // "None" is the sender's own object; an object variable that holds nothing names something the tree never found (outside it).
    let aim = if mode == 1 || mode == 2 || mode == 3 {
        if c.is_none(1) {
            c.fsm_owner(me)
        } else {
            object
        }
    } else {
        NONE8
    };
    let mut inside = false;
    for g in 0..n {
        if exclude && g == me {
            continue;
        }
        let owner = c.fsm_owner(g);
        let hit = if mode == 0 || mode == 5 {
            g == me
        } else if mode == 4 {
            true
        } else if mode == 1 || mode == 2 || mode == 3 {
            owner == aim || (children && is_within(c, owner, aim))
        } else {
            false
        };
        if hit {
            inside = true;
            c.send(ev, g, ticks);
        }
    }
    // Anything addressed beyond the tree, and every broadcast, is also the engine's to hear.
    let outside = mode == 4
        || ((mode == 1 || mode == 2) && !inside)
        || aim == hk_format::fsm3::ENT_HERO
        || aim == hk_format::fsm3::ENT_CAMERA
        || (aim == NONE8 && mode != 0 && mode != 5);
    if outside {
        c.world().request(k::MESSAGE, aim, mode as u8, [ev as i32, ticks as i32, 0]);
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Whether object `h` is a descendant of `top`.
#[inline(never)]
fn is_within(c: &mut Ctx<'_, '_>, h: u8, top: u8) -> bool {
    let mut at = h;
    for _ in 0..6 {
        match c.world().get(at) {
            Some(e) => {
                if e.parent == top {
                    return true;
                }
                at = e.parent;
            }
            None => return false,
        }
    }
    false
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// SetFsmBool / Float / String / GameObject [gameObject?, fsm index, variable slot, value] (the object operand is absent for
/// the owner-default forms). An FSM index of 255 means the engine owns the variable: the names ride out as a request.
#[inline(never)]
fn set_fsm(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    let (f, slot) = (c.int(b), c.int(b + 1));
    let value = if op == events::SET_FSM_BOOL {
        c.bit(b + 2) as i32
    } else if op == events::SET_FSM_GAME_OBJECT {
        c.obj(b + 2) as i32
    } else if op == events::SET_FSM_FLOAT {
        c.fixed(b + 2)
    } else {
        c.int(b + 2)
    };
    if f >= 0 && (f as u8) < c.t.bank.fsm_count() {
        let (f, slot) = (f as u8, slot as usize);
        if op == events::SET_FSM_BOOL {
            c.t.set_bit(f, slot, value != 0);
        } else if op == events::SET_FSM_GAME_OBJECT {
            c.t.set_obj(f, slot, value as u8);
        } else {
            c.t.set_cell(f, slot, value);
        }
    } else {
        let object = if b == 0 { c.fsm_owner(c.f) } else { c.obj(0) };
        c.world().request(k::SETVAR, object, op, [f, slot, value]);
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// GetFsmInt / GetFsmGameObject [gameObject?, fsm index, variable slot, store].
#[inline(never)]
fn get_fsm(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    let (f, slot) = (c.int(b), c.int(b + 1));
    if f >= 0 && (f as u8) < c.t.bank.fsm_count() {
        if op == events::GET_FSM_GAME_OBJECT {
            let h = c.t.obj(f as u8, slot as usize);
            c.set_obj(b + 2, h);
        } else {
            let v = c.t.cell(f as u8, slot as usize);
            c.set(b + 2, v);
        }
    }
    r_stateless(c.flags)
}
