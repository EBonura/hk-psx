//! Greenpath's Hornet in the guest (docs/FSM-V2-DESIGN.md 2.4): the fight of `hk_sim::hornet_fight` run once a tick in
//! Fungus1_04, and drawn from the sprite clips the Hornet postpass appended to that scene's banks (host/hk-cook
//! hornet_scene.rs).
//!
//! All of it lives in the room's `hornet` module and its zeroed state block (`art_hornet`): the fight's two trees and their
//! scene objects are ten kilobytes the resident image has no room for. The resident code only asks the module to tick and
//! to draw, while the module is resident.
use super::Draws;
use crate::debris::Rotation;
use hk_format::{i32_at, u32_at, Room};
use hk_sim::fsm3_world::{flag, MAX_ENT, NO_CLIP};
use hk_sim::hornet_fight::{self as hf, Effect, Fight, Input, Phase, Strike};
use hk_sim::Player;

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/hornet_art.rs"));

const MAGIC: u32 = 0x484f_524e;

/// The room's state block: the fight and what the guest keeps beside it.
#[repr(C)]
pub struct State {
    fight: Fight,
    ready: u32,
    /// Per tree and object, the cooked clip name id last looked up and the room clip it is (u16::MAX: none).
    art: [[(u32, u16); MAX_ENT]; 2],
    /// Ticks until the conversation the Encounter started has been read (the dialogue box is not wired yet).
    convo: u16,
    done: bool,
}
const _: () = assert!(core::mem::size_of::<State>() <= hf::STATE_BYTES);

/// What the module hands back to the tick: the numbers `Events` and the Knight's vitals are updated from.
pub struct Out {
    pub hits: u16,
    pub kills: u16,
    /// The slash reached her (landed, blocked or in her evasion window): the Knight takes his recoil and pogo.
    pub reached: bool,
    pub hurt: Option<(u8, i32)>,
}

fn state() -> Option<&'static mut State> {
    let p = crate::modules::data_mut(crate::modules::ART_HORNET)?;
    debug_assert!(p as usize % 4 == 0);
    // SAFETY: the room's zeroed state block, aligned and `STATE_BYTES` long; nothing else touches it.
    Some(unsafe { &mut *(p as *mut State) })
}

/// The fight's saved PlayerData reserve, in `hk-fsm boss::HORNET_FIELDS` order.
fn saved() -> [i32; 8] {
    let mut pd = [0; 8];
    pd[hf::field::HORNET1_DEFEATED as usize] = crate::persist::player(crate::persist::HORNET_1_DEFEATED) as i32;
    pd[hf::field::HAS_DASH as usize] = crate::persist::player(crate::persist::HAS_DASH) as i32;
    pd
}

/// One 60 Hz tick of the fight in `scene`, if it is Fungus1_04 with the module resident.
#[inline(never)]
pub fn tick(
    player: &Player,
    body: [i32; 4],
    polygon: &[[i32; 2]],
    kind: u8,
    damage: i32,
    count: usize,
    edge: &dyn Fn(usize) -> [i32; 4],
    camera: [i32; 3],
    out: &mut Out,
) {
    let Some(s) = state() else { return };
    if s.ready != MAGIC {
        let defeated = crate::persist::player(crate::persist::HORNET_1_DEFEATED);
        s.fight.begin(0x484f_524e, defeated, saved());
        s.ready = MAGIC;
        for tree in s.art.iter_mut() {
            *tree = [(0, u16::MAX); MAX_ENT];
        }
        s.convo = 0;
        s.done = false;
    }
    if s.fight.phase == Phase::Gone {
        return;
    }
    if s.convo != 0 {
        s.convo -= 1;
        s.done = s.convo == 0;
    }
    let strike = (!polygon.is_empty()).then_some(Strike { polygon, kind, damage, facing: player.facing });
    let input = Input {
        hero: [player.x, player.y],
        hero_body: body,
        camera,
        dt_scaled: crate::ONE,
        dt_real: crate::ONE,
        strike,
        conversation_done: core::mem::take(&mut s.done),
    };
    let mut convo = 0u16;
    s.fight.step(&input, count, edge, &mut |e| {
        if let Effect::Persist { field, value } = e {
            if field == hf::field::HORNET1_DEFEATED && value != 0 {
                crate::persist::set_player(crate::persist::HORNET_1_DEFEATED);
            }
        } else if let Effect::Struck { .. } = e {
            out.hits += 1;
        } else if let Effect::Died = e {
            out.kills += 1;
        } else if let Effect::HeroHurt { damage, from_x } = e {
            out.hurt = Some((damage, from_x));
        } else if let Effect::Conversation { .. } = e {
            // Her greeting is read in a few seconds; the box itself is not drawn yet.
            convo = 60 * 4;
        }
    });
    out.reached |= s.fight.reached;
    if convo != 0 {
        s.convo = convo;
    }
}

/// Everything of the fight on screen: each visible object of the live tree, from its current clip.
#[inline(never)]
pub fn draw(draws: &mut Draws, room: &Room, camera: (i32, i32)) {
    let Some(s) = state() else { return };
    if s.ready != MAGIC {
        return;
    }
    let flash = s.fight.health.flash;
    let (tree, world) = match s.fight.phase {
        Phase::Asleep | Phase::Fighting => (0u8, &s.fight.boss.world),
        Phase::Dying => (1u8, s.fight.corpse_world()),
        Phase::Gone => return,
    };
    for i in 0..world.n {
        let e = &world.ent[i as usize];
        if !e.on(flag::MESH) || e.clip == NO_CLIP || !world.active_in_hierarchy(i) {
            continue;
        }
        let Some(&(_, _, _, clip)) = HORNET_ART_CLIPS.iter().find(|r| r.0 == tree && r.1 == i && r.2 == e.clip) else {
            continue;
        };
        if clip as usize >= room.counts[4] {
            continue;
        }
        let c = room.clip(clip as usize);
        let frame = if e.frame >= 0 {
            (e.frame as u32).min(c[1] - 1)
        } else {
            let elapsed = hk_sim::fixed::mul_div_floor(e.clip_from as u32, c[2], 60 * 65536);
            let mode = c[3] & 65535;
            let start = c[3] >> 16;
            if mode == 0 {
                elapsed % c[1]
            } else if mode == 1 && elapsed >= c[1] {
                start + (elapsed - start) % (c[1] - start)
            } else {
                elapsed.min(c[1] - 1)
            }
        };
        let frame_index = (c[0] + frame) as usize;
        let (pos, scale, rot) = world.world_of(i);
        let authored = HORNET_ART_SCALES.iter().find(|r| r.0 == tree && r.1 == i).map_or((crate::ONE, crate::ONE), |r| (r.2, r.3));
        let ratio = (
            if authored.0 != 0 { hk_sim::fixed::mul_div_floor(scale[0].unsigned_abs(), crate::ONE as u32, authored.0 as u32) as i32 } else { crate::ONE },
            if authored.1 != 0 { hk_sim::fixed::mul_div_floor(scale[1].unsigned_abs(), crate::ONE as u32, authored.1 as u32) as i32 } else { crate::ONE },
        );
        // The art faces left at scale +1: a mirrored object takes the other facing.
        let facing = if scale[0] < 0 { 1 } else { -1 };
        let turn = (rot != 0).then(|| Rotation::new(-rot));
        let (cols, rows) = room.texture(u32_at(room.frame(frame_index), 0) as usize).tile_grid();
        for tile in 0..cols * rows {
            let (texture, b) = if cols * rows == 1 {
                let f = room.frame(frame_index);
                (u32_at(f, 0) as usize, core::array::from_fn::<_, 4, _>(|k| i32_at(f, 4 + k * 4)))
            } else {
                room.frame_tile(frame_index, tile)
            };
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
            if !draws.can_push(texture as u16) {
                return;
            }
            draws.push(texture as u16, verts, if i == 0 { flash } else { 0 });
        }
    }
}
