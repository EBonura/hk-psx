//! Greenpath's Hornet in the guest (docs/FSM-V2-DESIGN.md 2.4): the fight of `hk_sim::hornet_fight` run once a tick in
//! Fungus1_04, and drawn from the sprite clips the Hornet postpass appended to that scene's banks (host/hk-cook
//! hornet_scene.rs).
//!
//! All of it lives in the room's `hornet` module and its zeroed state block (`art_hornet`): the fight's two trees and their
//! scene objects are ten kilobytes the resident image has no room for. The resident code only asks the module to tick and
//! to draw, while the module is resident.
use super::{Draws, Events, Nail, Vitals};
use crate::world::{Region, State as World};
use crate::world::debris::Rotation;
use hk_format::Room;
use hk_sim::fsm3_world::{flag, NO_CLIP};
use hk_sim::hornet_fight::{self as hf, Effect, Fight, Input, Phase, Strike};
use hk_sim::Player;

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/hornet_art.rs"));

const MAGIC: u32 = 0x484f_524e;

/// The room's state block: the fight and what the guest keeps beside it.
#[repr(C)]
pub struct State {
    fight: Fight,
    ready: u32,
    /// Ticks until the conversation the Encounter started has been read (the dialogue box is not wired yet).
    convo: u16,
    done: bool,
}
const _: () = assert!(core::mem::size_of::<State>() <= hf::STATE_BYTES);

#[cfg_attr(target_arch = "mips", optimize(size))]
fn state_block() -> Option<&'static mut State> {
    let p = crate::modules::data_mut(crate::modules::ART_HORNET)?;
    debug_assert!(p as usize % 4 == 0);
    // SAFETY: the room's zeroed state block, aligned and `STATE_BYTES` long; nothing else touches it.
    Some(unsafe { &mut *(p as *mut State) })
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// The fight's saved PlayerData reserve, in `hk-fsm boss::HORNET_FIELDS` order.
fn saved() -> [i32; 8] {
    let mut pd = [0; 8];
    pd[hf::field::HORNET1_DEFEATED as usize] = crate::persist::player(crate::persist::HORNET_1_DEFEATED) as i32;
    pd[hf::field::HAS_DASH as usize] = crate::persist::player(crate::persist::HAS_DASH) as i32;
    pd
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// One 60 Hz tick of the fight: the Knight against her, and her against the Knight. The resident tick only calls this while
/// Fungus1_04's room module is resident.
#[inline(never)]
pub fn tick(
    events: &mut Events,
    player: &mut Player,
    vitals: &mut Vitals,
    response: &mut hk_sim::NailResponse,
    nail: &Nail,
    cheats: crate::cheats::Settings,
    body: [i32; 4],
    polygon: &[[i32; 2]],
    camera: [i32; 3],
    world: &World,
    region: &Region,
    room: &Room,
) {
    let Some(s) = state_block() else { return };
    if s.ready != MAGIC {
        let defeated = crate::persist::player(crate::persist::HORNET_1_DEFEATED);
        s.fight.begin(0x484f_524e, defeated, saved());
        unsafe { super::HK_HERO_LOCK = false };
        // A Hornet already beaten has left: the shiny is out (`Check PD`).
        unsafe { super::HK_HORNET_CLOAK_WAIT = 0 };
        s.ready = MAGIC;
        s.convo = 0;
        s.done = false;
    }
    unsafe {
        let wait = super::HK_HORNET_CLOAK_WAIT;
        if wait != 0 && wait != u16::MAX {
            super::HK_HORNET_CLOAK_WAIT = wait - 1;
        }
    }
    unsafe {
        super::HK_HORNET_PHASE = s.fight.phase as u32;
        super::HK_HORNET_HP = s.fight.health.hp.max(0) as u32;
        super::HK_HORNET_TICKS += 1;
    }
    if s.fight.phase == Phase::Gone {
        return;
    }
    if s.convo != 0 {
        s.convo -= 1;
        s.done = s.convo == 0;
    }
    let damage = cheats.params(crate::VITAL_PARAMS).nail_damage as i32;
    let strike = (!polygon.is_empty()).then_some(Strike { polygon, kind: nail.kind as u8, damage, facing: player.facing });
    let input = Input {
        hero: [player.x, player.y],
        hero_body: body,
        camera,
        dt_scaled: crate::ONE,
        dt_real: crate::ONE,
        strike,
        conversation_done: core::mem::take(&mut s.done),
    };
    let (mut hits, mut kills, mut hurt, mut convo) = (0u16, 0u16, None, 0u16);
    let scene = region.scene;
    let edge = |i: usize| world.edge(region, room, i);
    s.fight.step(&input, room.counts[5], &edge, &mut |e| {
        if let Effect::Persist { field, value } = e {
            if field == hf::field::HORNET1_DEFEATED && value != 0 {
                crate::persist::set_player(crate::persist::HORNET_1_DEFEATED);
            }
        } else if let Effect::Struck { .. } = e {
            hits += 1;
        } else if let Effect::Died = e {
            kills += 1;
            unsafe { super::HK_HORNET_CLOAK_WAIT = u16::MAX };
        } else if let Effect::CloakLeaves = e {
            // `Fall Pause` 0.5 s, `Fall` 0.4 s.
            unsafe { super::HK_HORNET_CLOAK_WAIT = 54 };
        } else if let Effect::HeroHurt { damage, from_x } = e {
            hurt = Some((damage, from_x));
            unsafe { super::HK_HORNET_HURT += 1 };
        } else if let Effect::GatesClose = e {
            crate::battle_gates::close(scene);
            crate::music::boss(true);
        } else if let Effect::GatesOpen = e {
            crate::battle_gates::open(scene);
            crate::music::boss(false);
        } else if let Effect::Shake { average } = e {
            crate::camera::request(if average { crate::camera::Shake::Average } else { crate::camera::Shake::Kill });
        } else if let Effect::HeroControl { regain } = e {
            unsafe { super::HK_HERO_LOCK = !regain };
        } else if let Effect::Conversation { .. } = e {
            unsafe { super::HK_HORNET_ENCOUNTERS += 1 };
            // Her greeting is read in a few seconds; the box itself is not drawn yet.
            convo = 60 * 4;
        }
    });
    if convo != 0 {
        s.convo = convo;
    }
    events.hits += hits;
    events.kills += kills;
    if hits != 0 {
        vitals.gain_soul_on_nail_hit(cheats.params(crate::VITAL_PARAMS));
    }
    if s.fight.reached {
        super::nail_answer(vitals, response, nail, player);
    }
    if let Some((damage, x)) = hurt {
        events.damage_hero(vitals, cheats, player, x, damage as u16);
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Everything of the fight on screen: each visible object of the live tree, from its current clip, as parts of the Hornet bank
/// (boss_bank.rs) queued for this frame's pass.
#[inline(never)]
pub fn draw(_draws: &mut Draws, room: &Room, camera: (i32, i32)) {
    crate::boss_bank::begin();
    let Some(s) = state_block() else { return };
    if s.ready != MAGIC {
        return;
    }
    if !crate::boss_bank::attach(room, HORNET_BANK_ID) {
        return;
    }
    let flash = s.fight.health.flash;
    let (tree, world) = match s.fight.phase {
        Phase::Asleep | Phase::Fighting => (0u8, &s.fight.boss.world),
        Phase::Dying => (1u8, s.fight.corpse_world()),
        Phase::Gone => return,
    };
    crate::boss_bank::with(room, |bank| {
        for i in 0..world.n {
            let e = &world.ent[i as usize];
            if !e.on(flag::MESH) || e.clip == NO_CLIP || !world.active_in_hierarchy(i) {
                continue;
            }
            let Some(&(_, _, group, sx, sy)) = HORNET_ART_ENTS.iter().find(|r| r.0 == tree && r.1 == i) else {
                continue;
            };
            let Some(&(_, _, _, clip)) = HORNET_ART_CLIPS.iter().find(|r| r.0 == tree && r.1 == group && r.2 == e.clip) else {
                unsafe { crate::boss_bank::HK_BOSS_CLIP_MISSES += 1 };
                continue;
            };
            if clip as u32 >= bank.clips {
                continue;
            }
            let c = bank.clip(clip as usize);
            if c.count == 0 {
                continue;
            }
            let frame = if e.frame >= 0 { (e.frame as usize).min(c.count as usize - 1) } else { c.frame(e.clip_from as u32) };
            let sprite = bank.sprite(bank.seq_sprite(c.first as usize + frame));
            let (pos, scale, rot) = world.world_of(i);
            let authored = (sx, sy);
            let ratio = (
                if authored.0 != 0 { hk_sim::fixed::mul_div_floor(scale[0].unsigned_abs(), crate::ONE as u32, authored.0 as u32) as i32 } else { crate::ONE },
                if authored.1 != 0 { hk_sim::fixed::mul_div_floor(scale[1].unsigned_abs(), crate::ONE as u32, authored.1 as u32) as i32 } else { crate::ONE },
            );
            // The art faces left at scale +1: a mirrored object takes the other facing.
            let facing = if scale[0] < 0 { 1 } else { -1 };
            let turn = (rot != 0).then(|| Rotation::new(-rot));
            let clut = crate::boss_bank::clut(room, sprite.palette);
            for p in 0..sprite.parts as usize {
                let index = sprite.first_part as usize + p;
                let part = bank.part(index);
                let b = hk_format::boss::Bank::<&[u8]>::part_bounds(&sprite, &part);
                let b = [
                    psx_math::int32::mul_shr_i32(b[0], ratio.0, 16),
                    psx_math::int32::mul_shr_i32(b[1], ratio.1, 16),
                    psx_math::int32::mul_shr_i32(b[2], ratio.0, 16),
                    psx_math::int32::mul_shr_i32(b[3], ratio.1, 16),
                ];
                let verts = super::project_box_turned(b, (pos[0], pos[1]), facing, camera, pos[2], turn);
                if super::offscreen(&verts) {
                    continue;
                }
                if !crate::boss_bank::queue(index, clut, part.width, part.height, verts, if i == 0 { flash } else { 0 }) {
                    return;
                }
            }
        }
    });
}
