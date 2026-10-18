//! Boss tier, family 4: transforms, movement and tweens, on the tree's scene objects.
use crate::fsm3::{r_stateless, r_with, Ctx, PH_ENTER, PH_FIXED, R_CONT};
use crate::fsm3_world::{div, flag, mul, sincos};
use hk_format::fsm3::{ops::transform, NONE8};
use psx_math::int32::mul_div_i32;

const DEG360: i32 = 360 << 16;

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == transform::SET_VELOCITY2D {
        // SetVelocity2d [gameObject, vector, x, y]: a component left at None keeps its value.
        let b = c.skip();
        let t = c.otarget();
        let (nx, ny) = (c.is_none(b + 1), c.is_none(b + 2));
        let v = [if nx { c.comp(b, 0) } else { c.fixed(b + 1) }, if ny { c.comp(b, 1) } else { c.fixed(b + 2) }];
        let vector = !c.is_none(b);
        if let Some(e) = c.world().get_mut(t) {
            if vector || !nx {
                e.vel[0] = v[0];
            }
            if vector || !ny {
                e.vel[1] = v[1];
            }
        }
        r_stateless(c.flags)
    } else if op == transform::GET_VELOCITY2D {
        // GetVelocity2d [gameObject, vector, x, y, space]
        let b = c.skip();
        let t = c.otarget();
        let v = c.world().get(t).map_or([0; 2], |e| e.vel);
        c.set_comp(b, 0, v[0]);
        c.set_comp(b, 1, v[1]);
        if !c.is_none(b + 1) {
            c.set(b + 1, v[0]);
        }
        if !c.is_none(b + 2) {
            c.set(b + 2, v[1]);
        }
        r_stateless(c.flags)
    } else if op == transform::GET_SPEED2D {
        // GetSpeed2d [gameObject, store]: |v|
        let t = c.otarget();
        let v = c.world().get(t).map_or([0; 2], |e| e.vel);
        let sq = psx_math::WideU::mul(v[0].unsigned_abs(), v[0].unsigned_abs())
            .add(psx_math::WideU::mul(v[1].unsigned_abs(), v[1].unsigned_abs()));
        c.set(c.skip(), sq.isqrt() as i32);
        r_stateless(c.flags)
    } else if op == transform::SET_VELOCITY_AS_ANGLE {
        // SetVelocityAsAngle [gameObject, angle, speed]: the heading in degrees.
        let b = c.skip();
        let (t, angle, speed) = (c.otarget(), c.fixed(b), c.fixed(b + 1));
        let (sin, cos) = sincos(angle);
        if let Some(e) = c.world().get_mut(t) {
            e.vel = [mul(speed, cos), mul(speed, sin)];
        }
        r_stateless(c.flags)
    } else if op == transform::SET_GRAVITY2D_SCALE {
        // SetGravity2dScale [gameObject, gravityScale]
        let (t, g) = (c.otarget(), c.fixed(c.skip()));
        if let Some(e) = c.world().get_mut(t) {
            e.gravity = g;
        }
        r_stateless(c.flags)
    } else if op == transform::SET_IS_KINEMATIC2D {
        // SetIsKinematic2d [gameObject, isKinematic]
        let (t, on) = (c.otarget(), c.bit(c.skip()));
        if let Some(e) = c.world().get_mut(t) {
            e.set(flag::KINEMATIC, on);
        }
        r_stateless(c.flags)
    } else if op == transform::DECELERATE_V2 || op == transform::DECELERATE_XY {
        // DecelerateV2 [gameObject, deceleration] / DecelerateXY [gameObject, decelerationX, decelerationY]: on entry and
        // every fixed step, each velocity component is multiplied by the factor, and one that would flip sign stops instead.
        if c.phase == PH_ENTER || c.phase == PH_FIXED {
            let b = c.skip();
            let t = c.otarget();
            let (kx, ky) = if op == transform::DECELERATE_V2 {
                let k = c.fixed(b);
                (Some(k), Some(k))
            } else {
                (if c.is_none(b) { None } else { Some(c.fixed(b)) }, if c.is_none(b + 1) { None } else { Some(c.fixed(b + 1)) })
            };
            let snap = op == transform::DECELERATE_XY;
            if let Some(e) = c.world().get_mut(t) {
                for (axis, factor) in [(0usize, kx), (1usize, ky)] {
                    if let Some(k) = factor {
                        let before = e.vel[axis];
                        let mut v = mul(before, k);
                        if (before < 0 && v > 0) || (before > 0 && v < 0) {
                            v = 0;
                        }
                        if snap && v.abs() < 66 {
                            v = 0;
                        }
                        e.vel[axis] = v;
                    }
                }
            }
        }
        R_CONT
    } else if op == transform::FLIP_SCALE {
        // FlipScale [gameObject, flipHorizontally, flipVertically]
        let b = c.skip();
        let (t, fx, fy) = (c.otarget(), c.bit(b), c.bit(b + 1));
        if let Some(e) = c.world().get_mut(t) {
            if fx {
                e.scale[0] = -e.scale[0];
            }
            if fy {
                e.scale[1] = -e.scale[1];
            }
        }
        r_stateless(c.flags)
    } else if op == transform::GET_POSITION || op == transform::SET_POSITION {
        position(c, op == transform::SET_POSITION)
    } else if op == transform::GET_SCALE || op == transform::SET_SCALE {
        scale(c, op == transform::SET_SCALE)
    } else if op == transform::GET_ROTATION || op == transform::SET_ROTATION {
        rotation(c, op == transform::SET_ROTATION)
    } else if op == transform::TRANSLATE {
        translate(c)
    } else if op == transform::I_TWEEN_MOVE_TO || op == transform::I_TWEEN_SCALE_TO || op == transform::I_TWEEN_MOVE_BY {
        tween(c, op)
    } else if op == transform::ROTATE {
        // Rotate [gameObject, vector, xAngle, yAngle, zAngle, space, perSecond]: spin about z.
        let b = c.skip();
        let t = c.otarget();
        let mut dz = if !c.is_none(b + 3) { c.fixed(b + 3) } else if !c.is_none(b) { c.comp(b, 2) } else { 0 };
        if c.bit(b + 5) {
            dz = mul_div_i32(dz, c.env().dt_scaled, 60 << 16);
        }
        if let Some(e) = c.world().get_mut(t) {
            e.rot = e.rot.wrapping_add(dz);
        }
        r_stateless(c.flags)
    } else {
        super::unknown(c, 4, op)
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// GetPosition / SetPosition [gameObject, vector, x, y, z, space]: world (0) or the parent's frame (1). A missing component
/// keeps the current value.
#[inline(never)]
fn position(c: &mut Ctx<'_, '_>, set: bool) -> u16 {
    let b = c.skip();
    let t = c.otarget();
    let local = c.int(b + 4) == 1;
    let (wp, _, _) = c.world().world_of(t);
    let lp = c.world().get(t).map_or([0; 3], |e| e.pos);
    let cur = if local { lp } else { wp };
    if set {
        let mut p = cur;
        if !c.is_none(b) {
            p = [c.comp(b, 0), c.comp(b, 1), c.comp(b, 2)];
        }
        for k in 0..3u8 {
            if !c.is_none(b + 1 + k) {
                p[k as usize] = c.fixed(b + 1 + k);
            }
        }
        let local_p = if local { p } else { c.world().local_for(t, p) };
        if let Some(e) = c.world().get_mut(t) {
            e.pos = local_p;
        }
    } else {
        for k in 0..3u8 {
            c.set_comp(b, k, cur[k as usize]);
            if !c.is_none(b + 1 + k) {
                c.set(b + 1 + k, cur[k as usize]);
            }
        }
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// GetScale / SetScale [gameObject, vector, x, y, z]: the local scale (Get reports the local scale too).
#[inline(never)]
fn scale(c: &mut Ctx<'_, '_>, set: bool) -> u16 {
    let b = c.skip();
    let t = c.otarget();
    let cur = c.world().get(t).map_or([crate::ONE; 2], |e| e.scale);
    if set {
        let mut s = cur;
        if !c.is_none(b) {
            s = [c.comp(b, 0), c.comp(b, 1)];
        }
        for k in 0..2u8 {
            if !c.is_none(b + 1 + k) {
                s[k as usize] = c.fixed(b + 1 + k);
            }
        }
        if let Some(e) = c.world().get_mut(t) {
            e.scale = s;
        }
    } else {
        c.set_comp(b, 0, cur[0]);
        c.set_comp(b, 1, cur[1]);
        for k in 0..2u8 {
            if !c.is_none(b + 1 + k) {
                c.set(b + 1 + k, cur[k as usize]);
            }
        }
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// GetRotation / SetRotation [gameObject, quaternion, vector, xAngle, yAngle, zAngle, space]: the 2D rotation is the z Euler
/// angle in degrees; world space adds the parents' rotations.
#[inline(never)]
fn rotation(c: &mut Ctx<'_, '_>, set: bool) -> u16 {
    let b = c.skip();
    let t = c.otarget();
    let world_space = c.int(b + 5) == 0;
    let (_, _, wr) = c.world().world_of(t);
    let lr = c.world().get(t).map_or(0, |e| e.rot);
    let cur = (if world_space { wr } else { lr }).rem_euclid(DEG360);
    if set {
        let mut z = cur;
        if !c.is_none(b + 1) {
            z = c.comp(b + 1, 2);
        }
        if !c.is_none(b + 4) {
            z = c.fixed(b + 4);
        }
        let parent_rot = wr.wrapping_sub(lr);
        let local = if world_space { z.wrapping_sub(parent_rot) } else { z };
        if let Some(e) = c.world().get_mut(t) {
            e.rot = local;
        }
    } else {
        c.set_comp(b + 1, 2, cur);
        if !c.is_none(b + 4) {
            c.set(b + 4, cur);
        }
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Translate [gameObject, vector, x, y, z, space, perSecond]: a move by the offset, in world (0) or the object's own frame (1),
/// every update if it runs every frame.
#[inline(never)]
fn translate(c: &mut Ctx<'_, '_>) -> u16 {
    let b = c.skip();
    let t = c.otarget();
    let mut d = [0i32; 3];
    for k in 0..3u8 {
        d[k as usize] = if !c.is_none(b + 1 + k) { c.fixed(b + 1 + k) } else if !c.is_none(b) { c.comp(b, k) } else { 0 };
    }
    if c.bit(b + 5) {
        let dt = c.env().dt_scaled;
        for v in &mut d {
            *v = mul_div_i32(*v, dt, 60 << 16);
        }
    }
    let selfspace = c.int(b + 4) == 1;
    let (wp, _, wr) = c.world().world_of(t);
    let (sin, cos) = sincos(wr);
    let delta = if selfspace { [mul(d[0], cos) - mul(d[1], sin), mul(d[0], sin) + mul(d[1], cos), d[2]] } else { d };
    let target = [wp[0] + delta[0], wp[1] + delta[1], wp[2] + delta[2]];
    let local = c.world().local_for(t, target);
    if let Some(e) = c.world().get_mut(t) {
        e.pos = local;
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// iTween MoveTo / MoveBy / ScaleTo: eased motion, then the finish event. Layouts (b is the owner skip):
/// MoveTo [gameObject, id, transformPosition, vectorPosition, time, delay, speed, space, easeType, loopType, orientToPath,
/// lookAtObject, lookAtVector, lookTime, axis, moveToPath, lookAhead, transforms[], vectors[], reverse, startEvent, finishEvent,
/// realTime, stopOnExit, loopDontFinish, scratch];
/// MoveBy [gameObject, id, vector, time, delay, speed, easeType, loopType, space, orientToPath, lookAtObject, lookAtVector,
/// lookTime, axis, startEvent, finishEvent, realTime, stopOnExit, loopDontFinish, scratch];
/// ScaleTo [gameObject, id, transformScale, vectorScale, time, delay, speed, easeType, loopType, startEvent, finishEvent,
/// realTime, stopOnExit, loopDontFinish, scratch]. A speed, when given, replaces the time (the distance over the speed).
/// The scratch bytes hold the elapsed ticks, the total ticks and the start x, y.
#[inline(never)]
fn tween(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    let t = c.otarget();
    let (by, mover) = (op == transform::I_TWEEN_MOVE_BY, op != transform::I_TWEEN_SCALE_TO);
    // Operand indices: the vector, the time, the speed, the ease, the finish event, the scratch slot, the space.
    let (vec_at, time_at, speed_at, ease_at, finish_at, slot_at, space_at) = if by {
        (b + 1, b + 2, b + 4, b + 5, b + 14, b + 18, b + 7)
    } else if mover {
        (b + 2, b + 3, b + 5, b + 7, b + 20, b + 24, b + 6)
    } else {
        (b + 2, b + 3, b + 5, b + 6, b + 9, b + 13, 0)
    };
    let slot = c.int(slot_at) as u8;
    let mut aim = if !by && mover && !c.is_none(b + 1) && c.obj(b + 1) != NONE8 {
        let h = c.obj(b + 1);
        super::position_of(c, h)
    } else {
        [c.comp(vec_at, 0), c.comp(vec_at, 1), c.comp(vec_at, 2)]
    };
    if c.phase == PH_ENTER {
        let start = if mover {
            c.world().get(t).map_or([0; 3], |e| e.pos)
        } else {
            let s = c.world().get(t).map_or([crate::ONE; 2], |e| e.scale);
            [s[0], s[1], 0]
        };
        // MoveTo's space 1 is the parent's frame (iTween's islocal), 0 the world; MoveBy and ScaleTo offsets are local.
        if mover && !by && c.int(space_at) == 0 {
            aim = c.world().local_for(t, aim);
        }
        if by {
            aim = [start[0] + aim[0], start[1] + aim[1], start[2] + aim[2]];
        }
        let mut total = super::ticks_of(c.fixed(time_at)) as i32;
        if !c.is_none(speed_at) {
            let speed = c.fixed(speed_at).max(1);
            let (dx, dy) = ((aim[0] - start[0]).unsigned_abs(), (aim[1] - start[1]).unsigned_abs());
            let dist = psx_math::WideU::mul(dx, dx).add(psx_math::WideU::mul(dy, dy)).isqrt().min(i32::MAX as u32) as i32;
            total = mul_div_i32(dist, 60, speed);
        }
        c.set_scratch32(slot, 0);
        c.set_scratch32(slot + 4, total.max(1));
        c.set_scratch32(slot + 8, start[0]);
        c.set_scratch32(slot + 12, start[1]);
        c.set_scratch32(slot + 16, aim[0]);
        c.set_scratch32(slot + 20, aim[1]);
        return R_CONT;
    }
    let total = c.scratch32(slot + 4).max(1);
    let at = (c.scratch32(slot) + (c.env().dt_scaled >> 16).max(1)).min(total);
    c.set_scratch32(slot, at);
    let u = div(at << 16, total << 16);
    let s = ease_curve(c.int(ease_at), u);
    let (x0, y0) = (c.scratch32(slot + 8), c.scratch32(slot + 12));
    let (x1, y1) = (c.scratch32(slot + 16), c.scratch32(slot + 20));
    let pos = [x0 + mul(x1 - x0, s), y0 + mul(y1 - y0, s)];
    if let Some(e) = c.world().get_mut(t) {
        if mover {
            e.pos[0] = pos[0];
            e.pos[1] = pos[1];
        } else {
            e.scale = pos;
        }
    }
    if at >= total {
        r_with(true, c.event(finish_at))
    } else {
        R_CONT
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// iTween's `EaseType` curves the original's trees use: quad (0 to 2), cubic (3 to 5), sine (12 to 14) and linear (21), by `u`
/// in [0, 1]; the cooker refuses a type not listed.
pub fn ease_curve(kind: i32, u: i32) -> i32 {
    let one = crate::ONE;
    let half = one / 2;
    match kind {
        0 => mul(u, u),
        1 => (u << 1) - mul(u, u),
        2 => {
            if u < half {
                mul(u, u) << 1
            } else {
                one - (mul(one - u, one - u) << 1)
            }
        }
        3 => mul(mul(u, u), u),
        4 => {
            let v = one - u;
            one - mul(mul(v, v), v)
        }
        5 => {
            if u < half {
                mul(mul(u, u), u) << 2
            } else {
                let v = one - u;
                one - (mul(mul(v, v), v) << 2)
            }
        }
        12 => one - sincos(mul(90 << 16, u)).1,
        13 => sincos(mul(90 << 16, u)).0,
        14 => (one - sincos(mul(180 << 16, u)).1) / 2,
        _ => u,
    }
}
