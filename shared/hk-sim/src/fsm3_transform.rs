//! HKFSM03 family 4: transform and movement actions.
use crate::fsm3::{r_stateless, Ctx, PH_FIXED, R_CONT};
use hk_format::fsm3::{ops::transform, ENT_SELF};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == transform::TRANSLATE {
        translate(c)
    } else if op == transform::SET_VELOCITY_AS_ANGLE {
        // SetVelocityAsAngle [gameObject, angle, speed]: the actor's heading and speed. Only the sign of
        // the horizontal velocity is kept, which is all FaceDirection reads.
        let b = c.skip();
        if c.target() == ENT_SELF {
            let (angle, speed) = (c.fixed(b), c.fixed(b + 1));
            let e = c.env();
            e.angle = angle;
            e.speed = speed;
            e.vel[0] = crate::fsm3::heading_sign(angle, speed);
            e.dirty |= crate::fsm3::DIRTY_VEL;
        }
        r_stateless(c.flags)
    } else if op == transform::SET_VELOCITY2D {
        // SetVelocity2d [gameObject, vector, x, y]: a component left at None keeps its value.
        let b = c.skip();
        if c.target() == ENT_SELF {
            let (nx, ny) = (c.is_none(b + 1), c.is_none(b + 2));
            let (x, y) = (c.fixed(b + 1), c.fixed(b + 2));
            let e = c.env();
            e.dirty |= crate::fsm3::DIRTY_VEL;
            if !nx {
                e.vel[0] = x;
            }
            if !ny {
                e.vel[1] = y;
            }
        }
        r_stateless(c.flags)
    } else if op == transform::GET_VELOCITY2D {
        // GetVelocity2d [gameObject, vector, x, y, space]: store the components that name a variable.
        let b = c.skip();
        let v = c.env().vel;
        if !c.is_none(b + 1) {
            c.set(b + 1, v[0]);
        }
        if !c.is_none(b + 2) {
            c.set(b + 2, v[1]);
        }
        r_stateless(c.flags)
    } else if op == transform::SET_ROTATION {
        // SetRotation [gameObject, quaternion, x, y, z, space...]: the sprite body is not rotated in the port.
        r_stateless(c.flags)
    } else if op == transform::ADD_FORCE2D_V2 {
        add_force(c)
    } else if op == transform::DECELERATE {
        // Decelerate [gameObject, deceleration]: every fixed step, each axis toward zero.
        if c.phase == PH_FIXED {
            let rate = c.fixed(c.skip());
            c.env().dirty |= crate::fsm3::DIRTY_VEL;
            let v = &mut c.env().vel;
            for a in v.iter_mut() {
                *a = a.signum() * (a.abs() - rate).max(0);
            }
        }
        R_CONT
    } else {
        R_CONT
    }
}

/// Translate of the owner [vector, x, y, z, space, perSecond]. A NONE component adds nothing. In Self
/// space the walker's local x runs against its heading (the sprite faces left natively).
#[inline(never)]
fn translate(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let mut dx = if c.is_none(b + 1) { 0 } else { c.fixed(b + 1) };
    let mut dy = if c.is_none(b + 2) { 0 } else { c.fixed(b + 2) };
    if c.int(b + 4) != 0 {
        dx *= -c.env().walk.direction;
    }
    if c.bit(b + 5) {
        dx /= 60;
        dy /= 60;
    }
    let e = c.env();
    e.pos[0] += dx;
    e.pos[1] += dy;
    r_stateless(c.flags)
}

/// AddForce2dV2 [gameObject, atPosition, vector, x, y, vector3, maxSpeed, maxSpeedX, maxSpeedY] (parameters from b): a force on the
/// unit-mass body, applied each fixed step (the action also acts on entry), then the speed caps that are set.
/// A component or cap left at None is not applied. The trace does not carry velocities, so this is checked by
/// its events (the state it lives in), not numerically.
#[inline(never)]
fn add_force(c: &mut Ctx<'_, '_>) -> u16 {
    if c.phase != PH_FIXED && c.phase != crate::fsm3::PH_ENTER {
        return R_CONT;
    }
    let b = c.skip();
    let comp = |c: &Ctx<'_, '_>, n: u8| if c.is_none(n) { 0 } else { c.fixed(n) };
    let (fx, fy) = (comp(c, b + 2), comp(c, b + 3));
    let (cap, cap_x, cap_y) = (c.is_none(b + 5), c.is_none(b + 6), c.is_none(b + 7));
    let (max, max_x, max_y) = (comp(c, b + 5), comp(c, b + 6), comp(c, b + 7));
    let e = c.env();
    e.dirty |= crate::fsm3::DIRTY_VEL;
    // One 50 Hz fixed step is 0.02 s.
    e.vel[0] += psx_math::int32::mul_shr_i32(fx, 1311, 16);
    e.vel[1] += psx_math::int32::mul_shr_i32(fy, 1311, 16);
    if !cap_x {
        e.vel[0] = e.vel[0].clamp(-max_x, max_x);
    }
    if !cap_y {
        e.vel[1] = e.vel[1].clamp(-max_y, max_y);
    }
    let _ = (cap, max);
    R_CONT
}
