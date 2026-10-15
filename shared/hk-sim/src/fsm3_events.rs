//! HKFSM03 family 2: events and messaging.
use crate::fsm3::{r_event, r_stateless, Ctx, R_CONT};
use hk_format::fsm3::{ops::events, NONE8};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == events::SEND_EVENT_BY_NAME {
        // SendEventByName [target, gameObject, fsmName, event, delay]: to the actor's FSMs, now or after
        // `delay` seconds. The cook admits only the owner-and-children target.
        let delay = c.fixed(4);
        let ticks = ((delay * 60 + 32768) >> 16) as u16;
        let ev = c.event(3);
        c.send(ev, NONE8, ticks);
        r_stateless(c.flags)
    } else if op == events::TRIGGER2D_EVENT {
        // Trigger2dEvent [triggerType, gameObject, collideTag, sendEvent, store]: the hero entering (0),
        // inside (1) or leaving (2) the owner's trigger volume, as the actor says this tick. It stays active.
        let kind = c.int(0) as u8 & 3;
        if c.env().trigger & (1 << kind) != 0 {
            return r_event(c.event(3));
        }
        R_CONT
    } else {
        R_CONT
    }
}
