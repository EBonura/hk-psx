//! HKFSM03 family 7: sprite animation.
use crate::fsm3::{r_stateless, r_with, Ctx, PH_COLLISION_ENTER, PH_ENTER, R_CONT};
use hk_format::fsm3::{ops::animation, NONE8};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == animation::TK2D_PLAY_ANIMATION {
        // Tk2dPlayAnimation [gameObject, library, clip]: start the clip; its length is the actor's to say.
        let id = c.int(c.skip() + 1) as u32;
        start(c, id);
        r_stateless(c.flags)
    } else if op == animation::TK2D_PLAY_ANIMATION_WITH_EVENTS {
        // Tk2dPlayAnimationWithEvents [gameObject, clip, triggerEvent, completeEvent]: starts the clip, then stays active
        // and raises the complete event once it is over. The light tier has no frame triggers.
        let b = c.skip();
        if c.phase == PH_ENTER {
            let id = c.int(b) as u32;
            start(c, id);
            return R_CONT;
        }
        let complete = c.event(b + 2);
        if c.env().anim_left == 0 && complete != NONE8 {
            r_with(false, complete)
        } else {
            R_CONT
        }
    } else if op == animation::TK2D_PLAY_FRAME {
        // Tk2dPlayFrame [gameObject, frame]: the clip runs at 12 fps, five 60 Hz ticks a frame.
        let frame = c.int(c.skip());
        let e = c.env();
        let last = (e.play_count as usize).saturating_sub(1);
        e.plays[last].1 = (frame * 5) as u16;
        r_stateless(c.flags)
    } else if op == animation::TK2D_WATCH_ANIMATION_EVENTS {
        // Tk2dWatchAnimationEvents [gameObject, triggerEvent, completeEvent]: stays active, raising the
        // complete event once the clip has finished.
        let done = c.env().anim_left == 0;
        let e = c.event(c.skip() + 1);
        if done && e != NONE8 {
            r_with(false, e)
        } else {
            R_CONT
        }
    } else {
        let _ = PH_COLLISION_ENTER;
        R_CONT
    }
}

/// Play clip `id` from its first frame: the clip's length, if the actor gave one, is how long it runs.
#[inline(never)]
fn start(c: &mut Ctx<'_, '_>, id: u32) {
    c.play(id, 0);
    let e = c.env();
    e.anim_left = 0xffff;
    for (clip, ticks) in e.clips {
        if clip == id {
            e.anim_left = ticks;
        }
    }
}
