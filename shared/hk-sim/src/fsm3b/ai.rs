//! Boss tier, family 5: senses and facing, on the tree's scene objects.
use crate::fsm3::{r_event, r_stateless, Ctx, PH_COLLISION_ENTER, PH_COLLISION_STAY, PH_ENTER, PH_FIXED, R_CONT};
use crate::fsm3_world::flag;
use hk_format::fsm3::{ops::ai, NONE8};

/// Degrees in a Q12 turn step, as Q16.16: 360 degrees over 4096 steps.
const DEG_PER_Q12: i32 = 5760;

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == ai::CHECK_COLLISION_SIDE {
        check_collision_side(c, PH_COLLISION_STAY)
    } else if op == ai::CHECK_COLLISION_SIDE_ENTER {
        check_collision_side(c, PH_COLLISION_ENTER)
    } else if op == ai::FACE_OBJECT {
        face_object(c)
    } else if op == ai::FACE_ANGLE || op == ai::FACE_ANGLE_V2 {
        face_angle(c, op == ai::FACE_ANGLE_V2)
    } else if op == ai::GET_ANGLE_TO_TARGET2_D {
        // GetAngleToTarget2D [gameObject, target, offsetX, offsetY, storeAngle]: degrees from the object to the target plus the
        // offset, in [0, 360).
        let b = c.skip();
        let (me, you) = (c.otarget(), c.obj(b));
        let (ox, oy) = (if c.is_none(b + 1) { 0 } else { c.fixed(b + 1) }, if c.is_none(b + 2) { 0 } else { c.fixed(b + 2) });
        let (a, t) = (super::position_of(c, me), super::position_of(c, you));
        let deg = angle_deg(t[1] + oy - a[1], t[0] + ox - a[0]);
        c.set(b + 3, deg);
        r_stateless(c.flags)
    } else if op == ai::CHECK_TARGET_DIRECTION {
        check_target_direction(c)
    } else {
        R_CONT
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// atan2(y, x) in degrees, Q16.16 in [0, 360).
#[inline(never)]
pub fn angle_deg(y: i32, x: i32) -> i32 {
    let q = psx_math::sincos::atan2_q12(y >> 8, x >> 8) as i32;
    q * DEG_PER_Q12
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// CheckCollisionSide and its Enter form [up, right, down, left outputs, up, right, down, left events, ...]: the first
/// contacted side, in that order, raises its event. They never finish.
#[inline(never)]
fn check_collision_side(c: &mut Ctx<'_, '_>, listening: u8) -> u16 {
    if c.phase == listening {
        let bits = c.env().contacts;
        for side in 0..4u8 {
            if bits & (1 << side) != 0 {
                let e = c.event(4 + side);
                return if e == NONE8 { R_CONT } else { r_event(e) };
            }
        }
    }
    R_CONT
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// FaceObject [objectA, objectB, spriteFacesRight, playNewAnimation, newAnimationClip, resetFrame]: A's local x scale takes the
/// sign that makes it face B, magnitude kept; a change may restart or replace the clip.
#[inline(never)]
fn face_object(c: &mut Ctx<'_, '_>) -> u16 {
    let (a, b) = (c.obj(0), c.obj(1));
    let (faces_right, play, clip, reset) = (c.bit(2), c.bit(3), c.int(4) as u32, c.bit(5));
    if c.world().get(a).is_none() || (b != hk_format::fsm3::ENT_HERO && b != hk_format::fsm3::ENT_CAMERA && c.world().get(b).is_none()) {
        return r_stateless(c.flags);
    }
    let (pa, pb) = (super::position_of(c, a), super::position_of(c, b));
    let left_of = pa[0] < pb[0];
    let w = c.world();
    let e = &mut w.ent[a as usize];
    let magnitude = e.scale[0].abs();
    let want = if left_of == faces_right { magnitude } else { -magnitude };
    if want != e.scale[0] {
        e.scale[0] = want;
        if reset || play {
            e.frame = -1;
            e.clip_from = 0;
            e.set(flag::PLAYED, true);
        }
        if play {
            e.clip = clip;
        }
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// FaceAngle [gameObject, angleOffset] / FaceAngleV2 [gameObject, angleOffset, worldSpace]: on entry and every fixed step,
/// the z rotation is the heading of the body's velocity plus the offset.
#[inline(never)]
fn face_angle(c: &mut Ctx<'_, '_>, v2: bool) -> u16 {
    if c.phase == PH_ENTER || c.phase == PH_FIXED {
        let b = c.skip();
        let t = c.otarget();
        let offset = c.fixed(b);
        let world_space = v2 && c.bit(b + 1);
        let v = c.world().get(t).map_or([0; 2], |e| e.vel);
        let z = angle_deg(v[1], v[0]).wrapping_add(offset);
        let (_, _, wr) = c.world().world_of(t);
        let lr = c.world().get(t).map_or(0, |e| e.rot);
        let parent = wr.wrapping_sub(lr);
        if let Some(e) = c.world().get_mut(t) {
            e.rot = if world_space { z.wrapping_sub(parent) } else { z };
        }
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// CheckTargetDirection [gameObject, target, aboveEvent, belowEvent, rightEvent, leftEvent, aboveBool, belowBool, rightBool,
/// leftBool]: where the target is relative to the object, on the bools and as events (the first now, the rest queued).
#[inline(never)]
fn check_target_direction(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let (me, you) = (c.otarget(), c.obj(b));
    let (a, t) = (super::position_of(c, me), super::position_of(c, you));
    let dirs = [t[1] > a[1], t[1] < a[1], t[0] > a[0], t[0] < a[0]];
    let mut first = NONE8;
    for (i, on) in dirs.into_iter().enumerate() {
        c.set_bit(b + 5 + i as u8, on);
        let e = c.event(b + 1 + i as u8);
        if on && e != NONE8 {
            if first == NONE8 {
                first = e;
            } else {
                let f = c.f;
                c.send(e, f, 0);
            }
        }
    }
    crate::fsm3::r_with(r_stateless(c.flags) & crate::fsm3::R_FIN != 0, first)
}
