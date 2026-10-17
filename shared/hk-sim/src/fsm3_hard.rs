//! HKFSM03 measurement handlers for the families the truth pass left unmeasured: object and spawn, animation,
//! ai, collider, combat, particles, audio, events and the rest of transform, written for the Hornet 1 type set.
//!
//! Engine effects are requests: a handler decodes its operands and appends one record (`kind | obj << 8 |
//! flag << 16`, three values) to `Env::req`; the actor adapter executes the records after the pass, as the
//! native service layer (DESIGN decision 6). Compiled only with the `hard-ops` feature: no shipped bank uses
//! these types yet, so the production modules do not carry them. Operand orders follow the PlayMaker
//! actions' public fields; they are checked against the cooked Hornet bank when a boss lands (M3).
use crate::fsm3::{r_stateless, r_with, Ctx, PH_ENTER, PH_UPDATE, R_CONT, R_FIN};
use hk_format::fsm3::{ops, ENT_SELF, NONE8};

pub const K_ACTIVATE: u8 = 1;
pub const K_ACTIVATE_CHILDREN: u8 = 2;
pub const K_DESTROY: u8 = 3;
pub const K_PARENT: u8 = 4;
pub const K_SPAWN: u8 = 5;
pub const K_MESH: u8 = 6;
pub const K_COLLIDER: u8 = 7;
pub const K_BOX_SIZE: u8 = 8;
pub const K_TRIGGER: u8 = 9;
pub const K_DAMAGE: u8 = 10;
pub const K_INVINCIBLE: u8 = 11;
pub const K_RECOIL: u8 = 12;
pub const K_PARTICLES: u8 = 13;
pub const K_AUDIO: u8 = 14;
pub const K_AUDIO_STOP: u8 = 15;
pub const K_MUSIC: u8 = 16;
pub const K_SNAPSHOT: u8 = 17;
pub const K_GRAVITY: u8 = 18;
pub const K_KINEMATIC: u8 = 19;
pub const K_POSITION: u8 = 20;
pub const K_SCALE: u8 = 21;
pub const K_ROTATION: u8 = 22;
pub const K_FLIP: u8 = 23;

#[inline(never)]
fn req(c: &mut Ctx<'_, '_>, kind: u8, obj: u8, flag: u8, a: [i32; 3]) {
    let e = c.env();
    let n = e.req_n as usize;
    if n < e.req.len() {
        e.req[n] = [kind as i32 | (obj as i32) << 8 | (flag as i32) << 16, a[0], a[1], a[2]];
        e.req_n += 1;
    }
}

// ---- object and spawn (family 8) -------------------------------------------------------------------

#[inline(never)]
pub fn object(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    if op == ops::object::ACTIVATE_GAME_OBJECT {
        // ActivateGameObject [gameObject, activate, recursive, resetOnExit, everyFrame]
        let on = c.bit(b) as u8;
        let t = c.target();
        req(c, K_ACTIVATE, t, on | (c.bit(b + 1) as u8) << 1, [0; 3]);
    } else if op == ops::object::ACTIVATE_ALL_CHILDREN {
        // ActivateAllChildren [gameObject, activate]
        let on = c.bit(b) as u8;
        let t = c.target();
        req(c, K_ACTIVATE_CHILDREN, t, on, [0; 3]);
    } else if op == ops::object::DESTROY_SELF {
        req(c, K_DESTROY, ENT_SELF, 0, [0; 3]);
        return R_FIN;
    } else if op == ops::object::FIND_CHILD {
        // FindChild [gameObject, childName, store]: the cook resolved the name to an entity handle.
        let h = c.int(b + 1) as u8;
        c.set_obj(b + 2, h);
    } else if op == ops::object::GET_PARENT {
        // GetParent [gameObject, store]: resolved by the cook as well.
        let h = c.int(b) as u8;
        c.set_obj(b + 1, h);
    } else if op == ops::object::GET_TAG {
        // GetTag [gameObject, store]: a tag id.
        let h = c.int(b);
        c.set(b + 1, h);
    } else if op == ops::object::SET_PARENT {
        // SetParent [gameObject, parent, resetLocalPosition, resetLocalRotation]
        let (t, p) = (c.target(), c.obj(b));
        let flags = c.bit(b + 1) as u8 | (c.bit(b + 2) as u8) << 1;
        req(c, K_PARENT, t, flags, [p as i32, 0, 0]);
    } else if op == ops::object::SPAWN_OBJECT_FROM_GLOBAL_POOL {
        // SpawnObjectFromGlobalPool [gameObject, spawnPoint, position, rotation, store]: pool object `prefab` at
        // the spawn point's position plus the offset; the handle the adapter gives it is stored.
        let prefab = c.int(b);
        let from = c.obj(b + 1);
        let at = c.env().position_of(from);
        let (dx, dy) = (c.fixed(b + 2), c.fixed(b + 3));
        req(c, K_SPAWN, NONE8, 0, [prefab, at[0] + dx, at[1] + dy]);
        let h = c.env().spawned;
        c.set_obj(b + 4, h);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

// ---- animation (family 7) -------------------------------------------------------------------------

#[inline(never)]
pub fn animation(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    if op == ops::animation::TK2D_PLAY_ANIMATION_WITH_EVENTS {
        // Tk2dPlayAnimationWithEvents [gameObject, library, clip, animationTriggerEvent, animationCompleteEvent]:
        // starts the clip; raises the trigger event on a frame trigger and the complete event at the end.
        if c.phase == PH_ENTER {
            let id = c.int(b + 1) as u32;
            c.play(id, 0);
            c.set_scratch8(0, 0);
            return R_CONT;
        }
        let e = c.env();
        if e.anim_trigger {
            e.anim_trigger = false;
            return r_with(false, c.event(b + 2));
        }
        if c.env().anim_left == 0 {
            return r_with(true, c.event(b + 3));
        }
        R_CONT
    } else if op == ops::animation::SET_MESH_RENDERER {
        // SetMeshRenderer [gameObject, active]
        let (t, on) = (c.target(), c.bit(b) as u8);
        req(c, K_MESH, t, on, [0; 3]);
        r_stateless(c.flags)
    } else {
        R_CONT
    }
}

// ---- ai, sense, chase (family 5) ------------------------------------------------------------------

/// The sign of `v` as the facing the sprite takes: right is +1.
#[inline(always)]
fn side(v: i32) -> i8 {
    if v > 0 {
        1
    } else {
        -1
    }
}

#[inline(never)]
pub fn ai(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    if op == ops::ai::FACE_ANGLE || op == ops::ai::FACE_ANGLE_V2 {
        // FaceAngle [gameObject, angle, spriteFacesRight...]: face right when the heading points right.
        let angle = c.fixed(b);
        let right = c.bit(b + 1);
        let a = angle.rem_euclid(360 << 16);
        let toward = !(90 << 16..=270 << 16).contains(&a);
        let e = c.env();
        e.facing = if toward == right { 1 } else { -1 };
        e.dirty |= 2;
        r_stateless(c.flags)
    } else if op == ops::ai::CHECK_TARGET_DIRECTION {
        // CheckTargetDirection [gameObject, target, offset, ..., aboveEvent, belowEvent, leftEvent, rightEvent, ...]
        let t = c.target();
        let other = c.obj(b);
        let (me, you) = (c.env().position_of(t), c.env().position_of(other));
        let (dx, dy) = (you[0] - me[0], you[1] - me[1]);
        let pick = if dx.abs() > dy.abs() { if dx < 0 { 2 } else { 3 } } else if dy < 0 { 1 } else { 0 };
        let e = c.event(b + 5 + pick);
        r_with(r_stateless(c.flags) & R_FIN != 0, e)
    } else if op == ops::ai::GET_ANGLE_TO_TARGET2_D {
        // GetAngleToTarget2D [gameObject, target, offsetX, offsetY, store]: degrees, Q16.16.
        let t = c.target();
        let other = c.obj(b);
        let (me, you) = (c.env().position_of(t), c.env().position_of(other));
        let (ox, oy) = (c.fixed(b + 1), c.fixed(b + 2));
        let q12 = psx_math::sincos::atan2_q12((you[1] + oy - me[1]) >> 8, (you[0] + ox - me[0]) >> 8) as i32;
        c.set(b + 3, q12 * 360 << 4);
        r_stateless(c.flags)
    } else {
        let _ = side(0);
        R_CONT
    }
}

// ---- colliders, combat, particles (families 11, 12, 10) -------------------------------------------

#[inline(never)]
pub fn collider(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    let t = c.target();
    if op == ops::collider::SET_BOX_COLLIDER2_DSIZE_VECTOR {
        // SetBoxCollider2DSizeVector [gameObject, vector]
        let (x, y) = (c.fixed(b), c.fixed(b + 1));
        req(c, K_BOX_SIZE, t, 0, [x, y, 0]);
    } else if op == ops::collider::SET_BOX_COLLIDER_TRIGGER {
        let on = c.bit(b) as u8;
        req(c, K_TRIGGER, t, on, [0; 3]);
    } else if op == ops::collider::SET_COLLIDER {
        let on = c.bit(b) as u8;
        req(c, K_COLLIDER, t, on, [0; 3]);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

#[inline(never)]
pub fn combat(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    let t = c.target();
    if op == ops::combat::SET_DAMAGE_HERO_AMOUNT {
        let n = c.int(b);
        req(c, K_DAMAGE, t, 0, [n, 0, 0]);
    } else if op == ops::combat::SET_INVINCIBLE {
        let on = c.bit(b) as u8;
        req(c, K_INVINCIBLE, t, on, [0; 3]);
    } else if op == ops::combat::SET_RECOIL_SPEED {
        let s = c.fixed(b);
        req(c, K_RECOIL, t, 0, [s, 0, 0]);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

#[inline(never)]
pub fn particles(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == ops::fx::PLAY_PARTICLE_EMITTER {
        // PlayParticleEmitter [gameObject, emit]
        let (t, n) = (c.target(), c.int(c.skip()));
        req(c, K_PARTICLES, t, 1, [n, 0, 0]);
        r_stateless(c.flags)
    } else {
        R_CONT
    }
}

// ---- audio (family 9) -----------------------------------------------------------------------------

#[inline(never)]
pub fn audio(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    if op == ops::audio::AUDIO_PLAY_SIMPLE {
        // AudioPlaySimple [gameObject, oneShotClip, volume]
        let (clip, vol, t) = (c.int(b), c.fixed(b + 1), c.target());
        req(c, K_AUDIO, t, 0, [clip, vol, 1 << 16]);
    } else if op == ops::audio::AUDIO_PLAYER_ONE_SHOT_SINGLE {
        // AudioPlayerOneShotSingle [spawnPoint, audioClip, spawnPosition, pitchMin, pitchMax, volume, delay, store]
        let clip = c.int(b + 1);
        let (lo, hi, vol) = (c.fixed(b + 3), c.fixed(b + 4), c.fixed(b + 5));
        let r = (c.rand() >> 8) as i32;
        let pitch = lo + psx_math::int32::mul_shr_i32(hi - lo, r, 24);
        let t = c.obj(b);
        req(c, K_AUDIO, t, 1, [clip, vol, pitch]);
    } else if op == ops::audio::AUDIO_PLAY_RANDOM {
        // AudioPlayRandom [gameObject, clips[], weights[], volume, pitch]
        let (n, first) = c.array(b);
        let pick = if n == 0 { 0 } else { (c.rand() >> 8) as usize % n };
        let w = c.array_word(first, pick);
        let clip = c.value_of(w, false);
        let vol = c.fixed(b + 3);
        let t = c.target();
        req(c, K_AUDIO, t, 0, [clip, vol, 1 << 16]);
    } else if op == ops::audio::AUDIO_STOP {
        let t = c.target();
        req(c, K_AUDIO_STOP, t, 0, [0; 3]);
    } else if op == ops::audio::AUDIO_PLAY_IN_STATE {
        // AudioPlayInState [gameObject, clip]: plays on enter, stops on exit (the adapter tracks the voice).
        let clip = c.int(b);
        let t = c.target();
        req(c, K_AUDIO, t, 2, [clip, 1 << 16, 1 << 16]);
    } else if op == ops::audio::APPLY_MUSIC_CUE {
        // ApplyMusicCue [musicCue, delay, transitionTime]
        let (cue, delay, t) = (c.int(0), c.fixed(1), c.fixed(2));
        req(c, K_MUSIC, NONE8, 0, [cue, delay, t]);
    } else if op == ops::audio::TRANSITION_TO_AUDIO_SNAPSHOT {
        let (snap, t) = (c.int(0), c.fixed(1));
        req(c, K_SNAPSHOT, NONE8, 0, [snap, t, 0]);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

// ---- events (family 2): variables of other FSMs of the tree ---------------------------------------

#[inline(never)]
pub fn events(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    // SetFsm*/GetFsm* [gameObject, fsmName (cooked to the FSM index), variable (cooked to its slot), value]
    let b = c.skip();
    let (f, slot) = (c.int(b) as u8, c.int(b + 1) as usize);
    if op == ops::events::SET_FSM_FLOAT {
        let v = c.fixed(b + 2);
        c.t.set_cell(f, slot, v);
    } else if op == ops::events::SET_FSM_BOOL {
        let v = c.bit(b + 2) as i32;
        c.t.set_cell(f, slot, v);
    } else if op == ops::events::SET_FSM_STRING || op == ops::events::SET_FSM_GAME_OBJECT {
        let v = c.int(b + 2);
        c.t.set_cell(f, slot, v);
    } else if op == ops::events::GET_FSM_INT || op == ops::events::GET_FSM_GAME_OBJECT {
        let v = c.t.cell(f, slot);
        c.set(b + 2, v);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

// ---- transform (family 4) -------------------------------------------------------------------------

/// One axis of Q16.16 deceleration: v * k, k in Q16.16.
#[inline(always)]
fn damp(v: i32, k: i32) -> i32 {
    psx_math::int32::mul_shr_i32(v, k, 16)
}

#[inline(never)]
pub fn transform(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let b = c.skip();
    let t = c.target();
    if op == ops::transform::DECELERATE_V2 {
        // DecelerateV2 [gameObject, deceleration, brake]: every tick, v = v * deceleration.
        let k = c.fixed(b);
        let v = c.env().vel;
        c.env().vel = [damp(v[0], k), damp(v[1], k)];
        c.env().dirty |= 1;
        R_CONT
    } else if op == ops::transform::DECELERATE_XY {
        // DecelerateXY [gameObject, decelerationX, decelerationY]
        let (kx, ky) = (c.fixed(b), c.fixed(b + 1));
        let v = c.env().vel;
        c.env().vel = [damp(v[0], kx), damp(v[1], ky)];
        c.env().dirty |= 1;
        R_CONT
    } else if op == ops::transform::FLIP_SCALE {
        // FlipScale [gameObject, flipX, flipY, flipZ]
        let f = c.bit(b) as u8 | (c.bit(b + 1) as u8) << 1 | (c.bit(b + 2) as u8) << 2;
        req(c, K_FLIP, t, f, [0; 3]);
        r_stateless(c.flags)
    } else if op == ops::transform::GET_POSITION || op == ops::transform::GET_SCALE || op == ops::transform::GET_ROTATION {
        // GetPosition [gameObject, vector, x, y, z, space, everyFrame]
        let p = c.env().position_of(t);
        c.set(b + 1, p[0]);
        c.set(b + 2, p[1]);
        c.set(b + 3, p[2]);
        r_stateless(c.flags)
    } else if op == ops::transform::GET_SPEED2D {
        // GetSpeed2d [gameObject, store]: |v| in Q16.16
        let v = c.env().vel;
        let sq = psx_math::WideU::mul(v[0].unsigned_abs(), v[0].unsigned_abs())
            .add(psx_math::WideU::mul(v[1].unsigned_abs(), v[1].unsigned_abs()));
        c.set(b + 1, sq.isqrt() as i32);
        r_stateless(c.flags)
    } else if op == ops::transform::SET_GRAVITY2D_SCALE {
        let g = c.fixed(b);
        req(c, K_GRAVITY, t, 0, [g, 0, 0]);
        r_stateless(c.flags)
    } else if op == ops::transform::SET_IS_KINEMATIC2D {
        let k = c.bit(b) as u8;
        req(c, K_KINEMATIC, t, k, [0; 3]);
        r_stateless(c.flags)
    } else if op == ops::transform::SET_POSITION || op == ops::transform::SET_SCALE {
        // SetPosition [gameObject, vector, x, y, z, space, everyFrame]: a missing axis keeps its value.
        let (x, y, z) = (c.fixed(b + 1), c.fixed(b + 2), c.fixed(b + 3));
        let mask = (!c.is_none(b + 1) as u8) | (!c.is_none(b + 2) as u8) << 1 | (!c.is_none(b + 3) as u8) << 2;
        req(c, if op == ops::transform::SET_POSITION { K_POSITION } else { K_SCALE }, t, mask, [x, y, z]);
        r_stateless(c.flags)
    } else if op == ops::transform::I_TWEEN_MOVE_TO || op == ops::transform::I_TWEEN_SCALE_TO {
        tween(c, op, t)
    } else {
        R_CONT
    }
}

/// iTween MoveTo / ScaleTo [gameObject, target, x, y, z, time, delay, easeType, ..., eventOnComplete...]: eased
/// motion to a point over `time` seconds. Scratch holds the elapsed ticks (word 0) and the start (words 1, 2).
#[inline(never)]
fn tween(c: &mut Ctx<'_, '_>, op: u8, t: u8) -> u16 {
    let b = c.skip();
    let (x, y) = (c.fixed(b + 1), c.fixed(b + 2));
    let total = (c.fixed(b + 4).max(1) >> 8) * 60 >> 8;
    let ease = c.int(b + 6);
    let kind = if op == ops::transform::I_TWEEN_MOVE_TO { K_POSITION } else { K_SCALE };
    if c.phase == PH_ENTER {
        let p = c.env().position_of(t);
        c.set_scratch32(0, 0);
        c.set_scratch32(4, p[0]);
        c.set_scratch32(8, p[1]);
        return R_CONT;
    }
    let dt = c.env().dt_scaled >> 16;
    let at = (c.scratch32(0) + dt.max(1)).min(total);
    c.set_scratch32(0, at);
    // t in Q16.16, shaped by the ease type: linear, ease-in (t^2), ease-out, ease-in-out.
    let u = ((at << 16) / total.max(1)).min(1 << 16);
    let s = match ease {
        1 => damp(u, u),
        2 => (u << 1) - damp(u, u),
        3 => {
            if u < 1 << 15 {
                damp(u, u) << 1
            } else {
                (1 << 16) - (damp((1 << 16) - u, (1 << 16) - u) << 1)
            }
        }
        _ => u,
    };
    let (x0, y0) = (c.scratch32(4), c.scratch32(8));
    let px = x0 + damp(x - x0, s);
    let py = y0 + damp(y - y0, s);
    req(c, kind, t, 3, [px, py, 0]);
    if at >= total {
        r_with(true, c.event(b + 8))
    } else {
        R_CONT
    }
}

// ---- flow, math (families 0, 1) -------------------------------------------------------------------

#[inline(never)]
pub fn flow(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let fin = r_stateless(c.flags) & R_FIN != 0;
    if op == ops::flow::FLOAT_IN_RANGE {
        // FloatInRange [float, lowerValue, upperValue, boolVariable, isInRangeEvent, outOfRangeEvent]
        let (v, lo, hi) = (c.fixed(0), c.fixed(1), c.fixed(2));
        let inside = v >= lo && v <= hi;
        c.set_bit(3, inside);
        r_with(fin, if inside { c.event(4) } else { c.event(5) })
    } else if op == ops::flow::FLOAT_TEST_TO_BOOL {
        // FloatTestToBool [float, tolerance, ..., trueEvent, falseEvent]
        let (v, w) = (c.fixed(0), c.fixed(1));
        let yes = v.abs_diff(w) <= c.fixed(2) as u32;
        c.set_bit(3, yes);
        r_with(fin, if yes { c.event(4) } else { c.event(5) })
    } else if op == ops::flow::BOOL_TEST_MULTI {
        // BoolTestMulti [bool1, bool2, bool3, ..., event]: raise when all given are true.
        let mut all = true;
        for i in 0..3 {
            all &= c.is_none(i) || c.bit(i);
        }
        r_with(fin, if all { c.event(3) } else { c.event(4) })
    } else if op == ops::flow::SEND_RANDOM_EVENT_V2 || op == ops::flow::SEND_RANDOM_EVENT_V3 {
        // SendRandomEventV2 [events[], weights[], trackingInts, ...]: weighted pick.
        let (n, evs) = c.array(0);
        let (_, ws) = c.array(1);
        let mut total = 0;
        for i in 0..n {
            total += c.value_of(c.t.bank.word(ws + 2 * i), true);
        }
        let mut r = ((c.rand() >> 8) as i32 >> 1).rem_euclid(total.max(1));
        let mut e = NONE8;
        for i in 0..n {
            r -= c.value_of(c.t.bank.word(ws + 2 * i), true);
            if r < 0 {
                e = c.event_of(c.t.bank.word(evs + 2 * i));
                break;
            }
        }
        r_with(fin, e)
    } else {
        R_CONT
    }
}

#[inline(never)]
pub fn math(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == ops::math::FLOAT_OPERATOR || op == ops::math::INT_OPERATOR {
        // FloatOperator [float1, float2, operation, storeResult]: add, subtract, multiply, divide, min, max.
        let q = op == ops::math::FLOAT_OPERATOR;
        let (x, y, o) = (c.val(0, q), c.val(1, q), c.int(2));
        let r = match o {
            0 => x.wrapping_add(y),
            1 => x.wrapping_sub(y),
            2 => {
                if q {
                    psx_math::int32::mul_shr_i32(x, y, 16)
                } else {
                    x.wrapping_mul(y)
                }
            }
            3 => {
                if y == 0 {
                    0
                } else if q {
                    psx_math::int32::mul_div_i32(x, 1 << 16, y)
                } else {
                    x / y
                }
            }
            4 => x.min(y),
            _ => x.max(y),
        };
        c.set(3, r);
    } else if op == ops::math::INT_ADD || op == ops::math::INT_ADD_V2 || op == ops::math::FLOAT_SUBTRACT {
        // IntAdd [variable, add, everyFrame] / FloatSubtract [variable, subtract, everyFrame, perSecond]
        let (v, d) = (c.val(0, op == ops::math::FLOAT_SUBTRACT), c.val(1, op == ops::math::FLOAT_SUBTRACT));
        c.set(0, if op == ops::math::FLOAT_SUBTRACT { v.wrapping_sub(d) } else { v.wrapping_add(d) });
    } else if op == ops::math::VECTOR3_ADD_XYZ {
        // Vector3AddXYZ [vector (3 cells), x, y, z]
        for i in 0..3 {
            let v = c.fixed(i) + c.fixed(3 + i);
            c.set(i, v);
        }
    } else if op == ops::math::SET_GAME_OBJECT {
        let h = c.obj(1);
        c.set_obj(0, h);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

#[allow(dead_code)]
fn _keep(c: &mut Ctx<'_, '_>) -> u16 {
    let _ = PH_UPDATE;
    animation(c, 0) + ai(c, 0)
}
