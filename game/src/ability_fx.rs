//! The effects of the Knight's movement abilities, drawn at him as the original's children of the
//! Knight prefab: the Dash Burst of an ordinary dash, the wings and flash of a double jump and the puff
//! of a wall jump (`HeroController.HeroDash`, `DoDoubleJump`, `DoWallJump`), and Vengeful Spirit's cast
//! blast and the end of its flight (`Fireball Top`, the ball's `Wall Impact`). The clips are cooked into
//! the `art_fx` and `art_fx_spell` packs (host/ability_art.py `FX_CLIPS`), each frame cut to parts of one
//! animation slot. Each plays once from the tick of the state edge that raised it; the Knight's follow
//! him and turn with his facing, the spell's keep the place and the way they were spawned with. The double
//! jump's `White Flash DJ` is a small round glow of blue light (one frame of `art_fx`).
//!
//! Dash Ash (the dust a dash drags behind the Knight) and Wallslide Dust are particle systems, cooked as
//! owners of the Knight in every scene (host/hk-cook break_effects.rs `HERO_*_OWNER`) and raised here
//! every sixth tick of the dash and of the slide, as the Focus dust is.
//!
//! A pack is carried (`wants_move`, `wants_spell`; frame.rs `carry_ability_art`) into the scenes of a
//! Knight who owns one of its abilities, where the pool has room for it; an effect whose pack is not
//! resident is not raised (`HK_FX_LATE` counts them), so a gate that has not read it costs one missing
//! puff, never a stall.
use crate::ability_art::{
    self, BALL_END, DASH_EFFECT, FIREBALL_BLAST, FLASH_EFFECT, WALL_IMPACT, WALL_PUFF, WINGS_EFFECT,
};
use crate::world;
use hk_sim::Player;

/// The effects, one slot each: a repeat replaces the earlier play of its own kind.
const CLIPS: [usize; 7] = [
    DASH_EFFECT,
    WINGS_EFFECT,
    WALL_PUFF,
    FLASH_EFFECT,
    FIREBALL_BLAST,
    WALL_IMPACT,
    BALL_END,
];
/// Ticks each clip plays once (the flash holds and fades instead): the clips are all play-once.
const TICKS: [u32; 7] = {
    let mut t = [0; 7];
    let mut i = 0;
    while i < 7 {
        let c = ability_art::ABILITY_CLIPS[CLIPS[i]];
        t[i] = (c.count as u32 * 60).div_ceil(c.fps);
        i += 1;
    }
    t[FLASH] = FLASH_HOLD + FLASH_FADE;
    t
};
/// The frame of slot `i` at its age, as a number in its clip: the 60 Hz tick count times the clip's
/// fps over 60, the divide done as a multiply (exact for these small ages), held on the last frame.
fn frame_at(i: usize, age: u32) -> usize {
    let c = ability_art::ABILITY_CLIPS[CLIPS[i]];
    ((age * c.fps * 17_477) >> 20).min(c.count as u32 - 1) as usize
}
/// The parts of slot `i`'s frame, as indices of the cooked frames: an effect frame is cut to parts of
/// one animation slot each, the ball's end is one.
fn parts_at(i: usize, frame: usize) -> core::ops::Range<usize> {
    if CLIPS[i] == BALL_END {
        let first = ability_art::ABILITY_CLIPS[BALL_END].start + frame;
        first..first + 1
    } else {
        ability_art::fx_parts(CLIPS[i], frame)
    }
}
/// Slots from here on are spawned in the world (they keep `AT` and `FACING`).
const FIRST_WORLD: usize = 4;
const FLASH: usize = 3;
/// `White Flash DJ` (resources.assets:4989) waits 0.125 s at full alpha, then `EaseColor` takes it to
/// nothing in 0.4 s; the cooked `Double Jump Flash` frame is a small round glow of blue light.
const FLASH_HOLD: u32 = 8;
const FLASH_FADE: u32 = 24;
/// The level the cooked effect frames are drawn at (128 is the texel's own).
const LEVEL: u8 = 112;
/// The Knight's own particle owners (host/hk-cook break_effects.rs): the dash's dust offset to the
/// right of him (the original's local +x, behind a Knight who faces left) and to the left, the wall
/// slide's.
const DASH_ASH_RIGHT: usize = 0xFFFD;
const DASH_ASH_LEFT: usize = 0xFFFC;
const SLIDE_DUST: usize = 0xFFFB;
/// Ticks between bursts: each cooked burst is a tenth of a second of the system's own emission.
const BURST_PERIOD: u16 = 6;
/// An idle slot's age, and a frame whose art is not here.
const IDLE: u8 = u8::MAX;
const NO_FRAME: u16 = u16::MAX;

static mut AGE: [u8; 7] = [IDLE; 7];
/// The parts slot `i` shows this frame: first and one past the last (`NO_FRAME` first when it shows none).
static mut PARTS: [(u16, u16); 7] = [(NO_FRAME, 0); 7];
static mut AT: [[i32; 2]; 3] = [[0; 2]; 3];
static mut FACING: [i32; 3] = [0; 3];
static mut WAS_ALIVE: bool = false;
/// Effects raised so far, for the route pins.
#[no_mangle]
pub static mut HK_FX_RAISED: u32 = 0;
/// Effects that were due and found no art in the pool.
#[no_mangle]
pub static mut HK_FX_LATE: u32 = 0;

/// Start slot `slot` (a place and a direction for the world ones).
#[inline(never)]
fn raise(slot: usize, at: [i32; 2], facing: i32) {
    if !ability_art::ready(CLIPS[slot]) {
        unsafe {
            HK_FX_LATE = HK_FX_LATE.saturating_add(1);
        }
        return;
    }
    unsafe {
        AGE[slot] = 0;
        if slot >= FIRST_WORLD {
            AT[slot - FIRST_WORLD] = at;
            FACING[slot - FIRST_WORLD] = facing;
        }
        HK_FX_RAISED = HK_FX_RAISED.wrapping_add(1);
    }
}
/// What a tick's state edges raise: `before` and `after` are the Knight around one `step`.
#[inline(never)]
pub fn on_edges(before: &Player, after: &Player) {
    if before.dash_left == 0 && after.dash_left > 0 && !after.shadow_dashing {
        raise(0, [0; 2], 0);
    }
    if !before.double_jumping && after.double_jumping {
        raise(1, [0; 2], 0);
        raise(FLASH, [0; 2], 0);
    }
    if !before.wall_locked && after.wall_locked {
        raise(2, [0; 2], 0);
    }
}
/// Vengeful Spirit leaves the Knight: `Fireball Top`'s `Fireball Blast`, in front of him.
#[inline(never)]
pub fn cast(player: &Player) {
    raise(FIRST_WORLD, [player.x, player.y], player.facing);
}
/// One burst of the Knight's owner `owner` at `at`.
#[inline(never)]
fn burst(scene: usize, owner: usize, at: [i32; 2]) {
    world::particles::pool().spawn_break_at(scene, owner, at);
}
/// The particles of this tick: a burst every sixth tick of an ordinary dash (behind the Knight) and of
/// a wall slide; and the end of the ball, a wall's impact or the end of its life.
#[inline(never)]
pub fn emit(scene: usize, player: &Player, ball: &hk_sim::Fireball) {
    let at = [player.x, player.y];
    if player.dash_left > 0
        && !player.shadow_dashing
        && (crate::PARAMS.dash_ticks - player.dash_left) % BURST_PERIOD == 0
    {
        burst(scene, if player.facing < 0 { DASH_ASH_RIGHT } else { DASH_ASH_LEFT }, at);
    }
    if player.wall_sliding && player.animation_tick % u32::from(BURST_PERIOD) == 1 {
        burst(scene, SLIDE_DUST, at);
    }
    if unsafe { WAS_ALIVE } && !ball.alive {
        // Left mid-flight: a wall. Out of life: the ball's own end.
        let slot = if ball.life > 0 { FIRST_WORLD + 1 } else { FIRST_WORLD + 2 };
        raise(slot, [ball.x, ball.y], ball.facing);
    }
    unsafe {
        WAS_ALIVE = ball.alive;
    }
}
/// One 60 Hz simulation tick: every effect ages, and ends with its clip.
#[inline(never)]
pub fn tick() {
    for i in 0..CLIPS.len() {
        unsafe {
            if AGE[i] == IDLE {
                continue;
            }
            AGE[i] += 1;
            if u32::from(AGE[i]) >= TICKS[i] {
                AGE[i] = IDLE;
            }
        }
    }
}
/// A scene left or a respawn: nothing outlives it.
#[inline(never)]
pub fn clear() {
    unsafe {
        AGE = [IDLE; 7];
        WAS_ALIVE = false;
    }
}
/// The parts slot `i` shows now: none when it is idle or its art is not here.
#[inline(never)]
fn parts_of(i: usize) -> (u16, u16) {
    unsafe {
        if AGE[i] != IDLE && ability_art::ready(CLIPS[i]) {
            let parts = parts_at(i, frame_at(i, u32::from(AGE[i])));
            (parts.start as u16, parts.end as u16)
        } else {
            (NO_FRAME, 0)
        }
    }
}
/// Ask for this frame's animation keys, one for each part of each live frame. False, and nothing
/// asked for, when the cache's slots are not left for them: an effect is the one thing in a frame that
/// may simply not draw.
#[inline(never)]
pub fn request(needed: &mut [u16], needed_len: &mut usize) -> bool {
    let (start, room) = (*needed_len, hk_cache::SLOTS.min(needed.len()));
    for i in 0..CLIPS.len() {
        let p = parts_of(i);
        unsafe {
            PARTS[i] = p;
        }
        if p.0 == NO_FRAME {
            continue;
        }
        for part in p.0..p.1 {
            if *needed_len >= room {
                *needed_len = start;
                return false;
            }
            needed[*needed_len] = ability_art::KEY_BASE + part;
            *needed_len += 1;
        }
    }
    *needed_len > start
}
/// Draw slot `i`, at the Knight or where it was spawned. The cooked frames carry their own colour
/// (the bright palette): 128 is the texel's own level.
#[cfg(not(test))]
#[inline(never)]
fn draw_one(i: usize, player: &Player, camera: (i32, i32)) -> u32 {
    let (first, end) = unsafe { PARTS[i] };
    if first == NO_FRAME {
        return 0;
    }
    let (at, facing) = if i >= FIRST_WORLD {
        // The Knight's atlas faces left and the spell art right (`draw` mirrors for +1).
        unsafe { (AT[i - FIRST_WORLD], -FACING[i - FIRST_WORLD]) }
    } else {
        ([player.x, player.y], player.facing)
    };
    // The flash holds, then fades (`EaseColor`); the rest are at their own level.
    let level = if i == FLASH {
        let age = u32::from(unsafe { AGE[i] });
        (u32::from(LEVEL) * (FLASH_FADE - age.saturating_sub(FLASH_HOLD).min(FLASH_FADE)) / FLASH_FADE) as u8
    } else {
        LEVEL
    };
    let mut drawn = 0;
    for part in first..end {
        drawn += ability_art::draw_light(usize::from(part), at[0], at[1], facing, camera, (level, level, level));
    }
    drawn
}
/// Draw the live effects. Only after `request` said yes this frame.
#[cfg(not(test))]
#[inline(never)]
pub fn draw(player: &Player, camera: (i32, i32)) -> u32 {
    let mut drawn = 0;
    for i in 0..CLIPS.len() {
        drawn += draw_one(i, player, camera);
    }
    drawn
}
/// Whether this Knight owns an ability whose effects are in `art_fx`.
pub fn wants_move(player: &Player) -> bool {
    player.has_dash || player.has_walljump || player.has_double_jump
}
/// Whether this Knight owns Vengeful Spirit, whose effects are in `art_fx_spell`.
pub fn wants_spell(spell: bool) -> bool {
    spell
}
