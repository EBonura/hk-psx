//! HKFSM03 family 5: senses, facing, chasing and walking.
use crate::fsm3::{
    r_event, r_stateless, Ctx, PH_COLLISION_ENTER, PH_COLLISION_STAY, PH_ENTER, PH_FIXED, PH_UPDATE, R_CONT,
};
use crate::{WalkState, ONE};
use hk_format::fsm3::{ops::ai, ENT_SELF, NONE8};
use psx_math::int32::{mul_div_i32, mul_shr_i32};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == ai::WALK_LEFT_RIGHT {
        walk_left_right(c)
    } else if op == ai::GET_DISTANCE {
        get_distance(c)
    } else if op == ai::FACE_DIRECTION {
        face_direction(c)
    } else if op == ai::CHECK_COLLISION_SIDE {
        check_collision_side(c, PH_COLLISION_STAY)
    } else if op == ai::CHECK_COLLISION_SIDE_ENTER {
        check_collision_side(c, PH_COLLISION_ENTER)
    } else if op == ai::IDLE_BUZZ {
        idle_buzz(c)
    } else if op == ai::CHASE_OBJECT {
        chase_object(c)
    } else if op == ai::FACE_OBJECT {
        face_object(c)
    } else if op == ai::CHECK_CAN_SEE_HERO {
        // CheckCanSeeHero [store]
        let v = c.env().can_see_hero;
        c.set_bit(0, v);
        r_stateless(c.flags)
    } else if op == ai::CHECK_ALERT_RANGE {
        // CheckAlertRange [range, store]
        let v = c.env().in_alert_range;
        c.set_bit(1, v);
        r_stateless(c.flags)
    } else if op == ai::FIND_ALERT_RANGE {
        // FindAlertRange [store, childName]: the radius of the actor's alert circle.
        let r = c.env().alert_range;
        c.set(0, r);
        r_stateless(c.flags)
    } else {
        R_CONT
    }
}

/// WalkLeftRight of the owner [walkSpeed, spriteFacesLeft, groundLayer, turnDelay, walkAnim, turnAnim,
/// startLeft, startRight, keepDirection]: the Walker's heading, turns and speed. It never finishes.
/// The speed and sprite contract are the cook's to check; the body's own parameters carry them.
#[inline(never)]
fn walk_left_right(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let (left, right, keep) = (c.bit(b + 6), c.bit(b + 7), c.bit(b + 8));
    if c.phase == PH_ENTER {
        if !keep && (left || right) {
            c.env().walk = WalkState::new(if left { -1 } else { 1 }, false);
        }
    } else if c.phase == PH_UPDATE {
        let delay = c.fixed(b + 3);
        let e = c.env();
        let mut p = e.walk_params;
        p.turn_cooldown_ticks = ((delay * 60 + 65535) >> 16) as u16;
        let (g, w, l) = e.senses;
        e.velocity = e.walk.tick(p, g, w, l);
    }
    R_CONT
}

/// Euclidean distance of two points, exact to the Q16.16 floor of the root of the exact squared sum.
#[inline(never)]
pub fn distance(a: [i32; 3], b: [i32; 3]) -> i32 {
    let mut squared = psx_math::WideU::ZERO;
    for i in 0..3 {
        let delta = a[i].abs_diff(b[i]);
        squared = squared.add(psx_math::WideU::mul(delta, delta));
    }
    squared.isqrt().min(i32::MAX as u32) as i32
}

/// GetDistance [gameObject, target, storeResult].
#[inline(never)]
fn get_distance(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let (a, t) = (c.target(), c.obj(b));
    let d = distance(c.env().position_of(a), c.env().position_of(t));
    c.set(b + 1, d);
    r_stateless(c.flags)
}

/// FaceDirection [gameObject, spriteFacesRight, playNewAnimation, clip, pauseBetweenTurns, pauseTime, slot]:
/// face the way the body is moving (left when it is not moving), and after a turn hold the new facing for
/// `pauseTime`. The pause timer is the action's own and survives leaving the state.
#[inline(never)]
fn face_direction(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let slot = c.int(b + 5) as u8;
    let pause = c.scratch8(slot);
    let pausing = c.bit(b + 3);
    if pause > 0 && pausing {
        c.set_scratch8(slot, pause - 1);
        return r_stateless(c.flags);
    }
    let e = c.env();
    let want: i8 = if e.vel[0] > 0 { 1 } else { -1 };
    if want != e.facing {
        e.facing = want;
        e.dirty |= crate::fsm3::DIRTY_FACING;
        let clip = c.int(b + 2) as u32;
        let play = c.bit(b + 1);
        // The cook bounds the pause to 4 seconds, so the tick count fits a byte.
        let hold = ((c.fixed(b + 4).max(0) * 60 + 32768) >> 16) as u8;
        c.set_scratch8(slot, hold);
        if play {
            c.play(clip, 0);
        }
    } else if pause > 0 {
        c.set_scratch8(slot, pause - 1);
    }
    r_stateless(c.flags)
}

/// FaceObject [gameObject, target, spriteFacesRight, playNewAnimation, clip, resetFrame]: turn to the target.
#[inline(never)]
fn face_object(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let target = c.obj(b);
    let at = c.env().position_of(target)[0];
    let e = c.env();
    e.facing = if at > e.pos[0] { 1 } else { -1 };
    e.dirty |= crate::fsm3::DIRTY_FACING;
    r_stateless(c.flags)
}

/// CheckCollisionSide and its Enter form [up, right, down, left outputs, up, right, down, left events, ...]:
/// the first contacted side, in that order, raises its event. They never finish.
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

/// A Random.Range over Q16.16 values from the tree's generator.
#[inline(never)]
fn range(c: &mut Ctx<'_, '_>, low: i32, high: i32) -> i32 {
    let draw = (c.rand() >> 8) as i32;
    low + mul_shr_i32(draw, high - low, 24)
}

/// IdleBuzz [gameObject, waitMin, waitMax, speedMax, accelerationMax, roamingRange, slot]: roam around the
/// point it was entered at. The scratch bytes hold start x, start y (32 bits each), acceleration x, y (16 bits) and the wait left
/// (32 bits); they are the action's own across visits. One call is one fixed step; entering the state re-samples the origin
/// and runs one step.
#[inline(never)]
fn idle_buzz(c: &mut Ctx<'_, '_>) -> u16 {
    if c.phase != PH_ENTER && c.phase != PH_FIXED {
        return R_CONT;
    }
    let b = c.skip();
    let slot = c.int(b + 5) as u8;
    let (wait_min, wait_max) = (c.fixed(b), c.fixed(b + 1));
    let (speed_max, accel_max, roaming) = (c.fixed(b + 2), c.fixed(b + 3), c.fixed(b + 4));
    let pos = c.env().pos;
    if c.phase == PH_ENTER {
        c.set_scratch32(slot, pos[0]);
        c.set_scratch32(slot + 4, pos[1]);
    }
    let mut v = c.env().vel;
    let start = [c.scratch32(slot), c.scratch32(slot + 4)];
    let mut accel = [c.scratch16(slot + 8) as i32, c.scratch16(slot + 10) as i32];
    let mut wait = c.scratch32(slot + 12);
    for axis in 0..2 {
        let low = pos[axis] < start[axis] - roaming;
        let high = pos[axis] > start[axis] + roaming;
        if (low && v[axis] < 0) || (high && v[axis] > 0) {
            accel[axis] = if low { accel_max } else { -accel_max } / 2000;
            v[axis] = mul_div_i32(v[axis], 8, 9);
            wait = range(c, wait_min, wait_max);
        }
    }
    if wait <= 0 {
        for axis in 0..2 {
            let (lo, hi) = if pos[axis] < start[axis] - roaming {
                (0, accel_max)
            } else if pos[axis] > start[axis] + roaming {
                (-accel_max, 0)
            } else {
                (-accel_max, accel_max)
            };
            accel[axis] = range(c, lo, hi) / 2000;
        }
        wait = range(c, wait_min, wait_max);
    }
    if wait > 0 {
        wait -= ONE / 50;
    }
    for axis in 0..2 {
        v[axis] = (v[axis] + accel[axis]).clamp(-speed_max, speed_max);
    }
    c.env().vel = v;
    c.env().dirty |= crate::fsm3::DIRTY_VEL;
    c.set_scratch16(slot + 8, accel[0] as i16);
    c.set_scratch16(slot + 10, accel[1] as i16);
    c.set_scratch32(slot + 12, wait);
    R_CONT
}

/// ChaseObject [gameObject, target, speedMax, acceleration, ...]: every fixed step, and on entry, push the
/// velocity toward the target on each axis and clamp it. It never finishes.
#[inline(never)]
fn chase_object(c: &mut Ctx<'_, '_>) -> u16 {
    if c.phase != PH_ENTER && c.phase != PH_FIXED {
        return R_CONT;
    }
    let b = c.skip();
    let target = c.obj(b);
    let (speed_max, accel) = (c.fixed(b + 1), c.fixed(b + 2));
    let to = c.env().position_of(target);
    let _ = ENT_SELF;
    let e = c.env();
    e.dirty |= crate::fsm3::DIRTY_VEL;
    for axis in 0..2 {
        let step = if to[axis] > e.pos[axis] { accel } else { -accel };
        e.vel[axis] = (e.vel[axis] + step).clamp(-speed_max, speed_max);
    }
    R_CONT
}
