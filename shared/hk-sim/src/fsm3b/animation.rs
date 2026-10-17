//! Boss tier, family 7: sprite animation. Every sprite animator of the tree is its object's `clip` in the table; the
//! adapter advances it, counts `anim_left` down and raises `anim_event` for frame triggers.
use crate::fsm3::{r_stateless, r_with, Ctx, PH_ENTER, R_CONT};
use crate::fsm3_world::{flag, World};
use hk_format::fsm3::ops::animation;

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == animation::TK2D_PLAY_ANIMATION {
        // Tk2dPlayAnimation [gameObject, library, clip]
        let b = c.skip();
        let (t, id) = (c.otarget(), c.int(b + 1) as u32);
        play(c.world(), t, id);
        r_stateless(c.flags)
    } else if op == animation::TK2D_PLAY_ANIMATION_WITH_EVENTS {
        // Tk2dPlayAnimationWithEvents [gameObject, clip, triggerEvent, completeEvent]: plays the clip, then stays active
        // raising the trigger event on every frame trigger and the complete event once the clip is over.
        let b = c.skip();
        let t = c.otarget();
        if c.phase == PH_ENTER {
            let id = c.int(b) as u32;
            play(c.world(), t, id);
            return R_CONT;
        }
        let (trigger, complete) = (c.event(b + 1), c.event(b + 2));
        let (event, over) = c.world().get(t).map_or((false, false), |e| (e.anim_event, e.anim_left == 0));
        if event && trigger != hk_format::fsm3::NONE8 {
            if let Some(e) = c.world().get_mut(t) {
                e.anim_event = false;
            }
            return r_with(false, trigger);
        }
        if over && complete != hk_format::fsm3::NONE8 {
            return r_with(false, complete);
        }
        R_CONT
    } else if op == animation::TK2D_PLAY_FRAME {
        // Tk2dPlayFrame [gameObject, frame]: stop the clip on one frame.
        let b = c.skip();
        let (t, frame) = (c.otarget(), c.int(b));
        if let Some(e) = c.world().get_mut(t) {
            e.frame = frame as i16;
            e.anim_left = 0xffff;
        }
        r_stateless(c.flags)
    } else if op == animation::TK2D_WATCH_ANIMATION_EVENTS {
        // Tk2dWatchAnimationEvents [gameObject, triggerEvent, completeEvent]: watches a clip another action started and
        // finishes with the complete event once it is over.
        let t = c.otarget();
        let b = c.skip();
        let (trigger, complete) = (c.event(b), c.event(b + 1));
        let (event, over) = c.world().get(t).map_or((false, false), |e| (e.anim_event, e.anim_left == 0));
        if event && trigger != hk_format::fsm3::NONE8 {
            if let Some(e) = c.world().get_mut(t) {
                e.anim_event = false;
            }
            return r_with(false, trigger);
        }
        if over {
            return r_with(true, complete);
        }
        R_CONT
    } else if op == animation::SET_MESH_RENDERER {
        // SetMeshRenderer [gameObject, active]
        let (t, on) = (c.otarget(), c.bit(c.skip()));
        if let Some(e) = c.world().get_mut(t) {
            e.set(flag::MESH, on);
        }
        r_stateless(c.flags)
    } else {
        super::unknown(c, 7, op)
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// `tk2dSpriteAnimator.Play(clip)`: starts the clip unless it is already the one playing, then marks the object for the
/// adapter to apply.
#[inline(never)]
fn play(w: &mut World, t: u8, id: u32) {
    let ticks = w.clip_ticks(t, id);
    if let Some(e) = w.get_mut(t) {
        if e.clip == id && e.anim_left != 0 && e.frame < 0 {
            return;
        }
        e.clip = id;
        e.clip_from = 0;
        e.frame = -1;
        e.anim_left = ticks;
        e.anim_event = false;
        e.set(flag::PLAYED, true);
    }
}
