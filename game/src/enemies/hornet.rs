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
use hk_sim::fsm3_world::{flag, MAX_ENT, NO_CLIP};
use hk_sim::hornet_fight::{self as hf, Effect, Fight, Input, Phase, Strike};
use hk_sim::Player;

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/hornet_art.rs"));
// Her greeting, `HORNET_GREENPATH` of the Hornet sheet, as the panel's pages (host/npc_dialogue.py `cook_hornet`).
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/hornet_convo.rs"));

const MAGIC: u32 = 0x484f_524e;

/// The room's state block: the fight and what the guest keeps beside it.
#[repr(C)]
pub struct State {
    fight: Fight,
    /// Where each object of the live tree stands this tick, composed through its parents: the fight's code works it out and the
    /// drawing module reads it (the two modules do not call each other).
    xf: [Xf; MAX_ENT],
    ready: u32,
    /// Her greeting is on the panel (`HORNET_CONVO`), at `convo_page`.
    convo_open: bool,
    convo_page: u8,
    /// The pad of the last tick, for the panel's presses.
    convo_prev: u16,
    /// The Cloak was already the Knight's when the room began (no item get), and the ticks its item-get panel has left.
    cloak_seen: bool,
    cloak_ticks: u16,
    /// The boss bank reader's index and draw queue (boss_bank.rs).
    boss: crate::boss_bank::Mem,
    /// The panel closed since the fight last looked: `CONVO_FINISH`.
    done: bool,
}
/// An object's world transform and whether it and every ancestor are active.
#[derive(Clone, Copy)]
#[repr(C)]
struct Xf {
    pos: [i32; 3],
    scale: [i32; 2],
    rot: i32,
    on: bool,
}
const _: () = assert!(core::mem::size_of::<State>() <= hf::STATE_BYTES);

/// The room's data package (`art_hornet`): the state block, then the fight's cooked tree and the corpse's, each padded to a word
/// (host/build_guest.py `module_tables`). The trees ride here rather than in the `hornet` module's read-only data so the module
/// stays under the pool's first window.
const fn word_up(n: usize) -> usize {
    (n + 3) & !3
}
const FIGHT_BANK: usize = hk_sim::hornet::fight::BANK.len();
const CORPSE_BANK: usize = hk_sim::hornet::corpse::BANK.len();

/// Both modules (`hornet` ticks, `hornet_art` draws) read the state block the same way, so each keeps its own copy of this.
#[inline(always)]
fn state_block() -> Option<&'static mut State> {
    let p = crate::modules::data_mut(crate::modules::ART_HORNET)?;
    debug_assert!(p as usize % 4 == 0);
    // SAFETY: the room's state block, aligned and `STATE_BYTES` long; nothing else touches it.
    Some(unsafe { &mut *(p as *mut State) })
}

/// The bank reader's memory, in the room's state block.
#[inline(always)]
pub fn boss_mem() -> &'static mut crate::boss_bank::Mem {
    match state_block() {
        Some(s) => &mut s.boss,
        // SAFETY: the reader runs only while the drawing module is resident, and that module is carried with the data package.
        None => unsafe { core::hint::unreachable_unchecked() },
    }
}

/// Point the trees at their cooked bytes in the data package: the host cooker's order and padding.
fn place_trees() {
    let Some(p) = crate::modules::data_mut(crate::modules::ART_HORNET) else { return };
    let fight = p as usize + hf::STATE_BYTES;
    // SAFETY: single-threaded guest; the package outlives the fight and stays where it is while the room is resident.
    unsafe {
        let banks = &mut *(&raw mut hk_sim::hornet::POOL_BANKS);
        banks[0] = (fight, FIGHT_BANK);
        banks[1] = (fight + word_up(FIGHT_BANK), CORPSE_BANK);
    }
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
    place_trees();
    if s.ready != MAGIC {
        let defeated = crate::persist::player(crate::persist::HORNET_1_DEFEATED);
        s.fight.begin(0x484f_524e, defeated, saved());
        unsafe { super::HK_HERO_LOCK = false };
        // A Hornet already beaten has left: the shiny is out (`Check PD`).
        unsafe { super::HK_HORNET_CLOAK_WAIT = 0 };
        s.ready = MAGIC;
        s.convo_open = false;
        s.done = false;
        s.cloak_seen = crate::persist::player(crate::persist::HAS_DASH);
        s.cloak_ticks = 0;
    }
    // `Shiny Control`'s big item: the Cloak taken in this room is announced.
    if !s.cloak_seen && crate::persist::player(crate::persist::HAS_DASH) {
        s.cloak_seen = true;
        s.cloak_ticks = 60 * 8;
    }
    s.cloak_ticks = s.cloak_ticks.saturating_sub(1);
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
        let at = s.fight.boss.world.world_of(0).0;
        super::HK_HORNET_X = at[0];
        super::HK_HORNET_Y = at[1];
        let state = s.fight.boss.tree().state(0) as u16;
        super::HK_HORNET_AGE = if state == super::HK_HORNET_STATE { super::HK_HORNET_AGE.saturating_add(1) } else { 0 };
        super::HK_HORNET_STATE = state;
        super::HK_HORNET_INVULN = vitals.invulnerable_ticks;
        let w = &s.fight.boss.world;
        // Object 7 is the Needle: out while it and every ancestor are active.
        super::HK_HORNET_NEEDLE = if w.active_in_hierarchy(7) {
            let n = w.world_of(7).0;
            [n[0], n[1]]
        } else {
            [i32::MAX, 0]
        };
    }
    if s.fight.phase == Phase::Gone {
        return;
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
    let (mut hits, mut kills, mut hurt, mut convo) = (0u16, 0u16, None, false);
    let scene = region.scene;
    // The terrain as the actors read it (one copy per room and epoch, `actor_edges`): `State::edge` is used by nothing else, and
    // keeping it linked for her cost 1,768 bytes of resident code.
    let (edges, _) = super::actor_edges(world, region, room, region, room);
    let edge = |i: usize| edges.get(i).copied().unwrap_or([0; 4]);
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
            convo = true;
        }
    });
    if convo {
        if HORNET_CONVO.is_empty() {
            s.done = true;
        } else {
            s.convo_open = true;
            s.convo_page = 0;
        }
    }
    // The drawing module's view of the tree, from the objects the pass left.
    let world = &s.fight.boss.world;
    for i in 0..world.n {
        let e = &world.ent[i as usize];
        s.xf[i as usize].on = false;
        if e.on(flag::MESH) && e.clip != NO_CLIP && world.active_in_hierarchy(i) {
            let (pos, scale, rot) = world.world_of(i);
            s.xf[i as usize] = Xf { pos, scale, rot, on: true };
        }
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
            let x = s.xf[i as usize];
            if !x.on {
                continue;
            }
            let Some(&(_, _, group, sx, sy)) = HORNET_ART_ENTS.iter().find(|r| r.0 == tree && r.1 == i) else {
                continue;
            };
            let Some(&(_, _, _, clip)) = HORNET_ART_CLIPS.iter().find(|r| r.0 == tree && r.1 == group && r.2 == e.clip) else {
                super::note_clip_miss();
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
            let (pos, scale, rot) = (x.pos, x.scale, x.rot);
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

/// One tick of the greeting's panel: Cross turns the page and closes after the last, Circle closes (the NPC panel's keys, npc_state.rs).
/// Runs every tick the room's module is resident, so the pad's edges are never missed.
#[inline(never)]
pub fn convo_tick(bits: u16) {
    let Some(s) = state_block() else { return };
    let pressed = bits & !s.convo_prev;
    s.convo_prev = bits;
    let (cross, circle) = (0x4000, 0x2000);
    if s.cloak_ticks != 0 && pressed & (cross | circle) != 0 {
        s.cloak_ticks = 0;
    }
    if !s.convo_open {
        return;
    }
    if pressed & circle != 0 || (pressed & cross != 0 && s.convo_page as usize + 1 >= HORNET_CONVO.len()) {
        s.convo_open = false;
        s.done = true;
    } else if pressed & cross != 0 {
        s.convo_page += 1;
    }
}

/// Her greeting's panel for this frame, composed after the HUD's (`dialogue::prepare`). True while it is up.
#[inline(never)]
pub fn convo_prepare() -> bool {
    use crate::dialogue::{panel_box, panel_text, panel_width};
    let Some(s) = state_block() else { return false };
    if s.cloak_ticks != 0 && !s.convo_open {
        // The conversation box's line pitch.
        const ROW: i16 = 16;
        let y = panel_box(HORNET_ITEM.len() as i16);
        for (i, line) in HORNET_ITEM.iter().enumerate() {
            panel_text(160 - panel_width(line) / 2, y + i as i16 * ROW, line);
        }
        return true;
    }
    if !s.convo_open {
        return false;
    }
    crate::dialogue::panel_pages(HORNET_CONVO, s.convo_page as usize);
    true
}
