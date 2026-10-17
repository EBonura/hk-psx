//! The boss tier of the HKFSM03 action families (docs/FSM-V2-DESIGN.md 2.3, 2.4).
//!
//! A bank whose header says `boss` runs these handlers instead of the light tier's (fsm3_flow.rs and the rest). They are
//! the same PlayMaker actions, but they act on the tree's scene objects (`fsm3_world::World`): a boss is a graph of
//! GameObjects (the body, its hit boxes, its needle, its effect sprites, its sensing ranges), not one body. Where an
//! action reaches the engine (a sound, a spawn, a camera shake) it appends a request the actor adapter drains after
//! the pass.
//!
//! Operand layouts are the cooker's: the original action's parameters in declaration order, minus `everyFrame`,
//! `lateUpdate` and `fixedUpdate` (they become flags) and minus the first FsmOwnerDefault when it means the owner
//! (`F_OWNER`). `host/hk-fsm` `fsm3-dump TREE` prints them per action type ("params" lines). Each handler says its layout.
//!
//! Module rules (host/code_modules.py): no `match` that becomes a jump table; handlers are tried in an if chain.
pub mod ai;
pub mod animation;
pub mod audio;
pub mod collider;
pub mod events;
pub mod flow;
pub mod fx;
pub mod math;
pub mod object;
pub mod playerdata;
pub mod timers;
pub mod transform;

use crate::fsm3::{Ctx, R_CONT};
use hk_format::fsm3::{ENT_CAMERA, ENT_HERO};

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, family: u8, op: u8) -> u16 {
    if family == 0 {
        flow::entry(c, op)
    } else if family == 1 {
        math::entry(c, op)
    } else if family == 2 {
        events::entry(c, op)
    } else if family == 3 {
        timers::entry(c, op)
    } else if family == 4 {
        transform::entry(c, op)
    } else if family == 5 {
        ai::entry(c, op)
    } else if family == 7 {
        animation::entry(c, op)
    } else if family == 8 {
        object::entry(c, op)
    } else if family == 9 {
        audio::entry(c, op)
    } else if family == 10 {
        fx::entry(c, op)
    } else if family == 11 {
        collider::entry(c, op)
    } else if family == 12 {
        collider::combat_entry(c, op)
    } else if family == 13 {
        playerdata::entry(c, op)
    } else {
        unknown(c, family, op)
    }
}

/// An action type no handler of its family takes: it would wait for ever as if it were stateful, so the world says which one it was
/// (`World::unhandled`, family << 8 | op) for the tests to refuse.
#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn unknown(c: &mut Ctx<'_, '_>, family: u8, op: u8) -> u16 {
    c.world().unhandled = (family as u16) << 8 | op as u16;
    R_CONT
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Where entity `h` stands in the world: the hero and the camera from the environment, anything else from the table.
#[inline(never)]
pub fn position_of(c: &mut Ctx<'_, '_>, h: u8) -> [i32; 3] {
    if h == ENT_HERO {
        c.env().hero
    } else if h == ENT_CAMERA {
        c.env().camera
    } else {
        c.world().world_of(h).0
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// A Q16.16 duration in seconds as 60 Hz ticks, rounded to nearest.
#[inline(always)]
pub fn ticks_of(seconds: i32) -> u16 {
    psx_math::int32::mul_shr_i32(seconds.max(0) + 546, 60, 16).clamp(0, 0xffff) as u16
}
