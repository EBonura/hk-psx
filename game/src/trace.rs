//! `HK_TRACE`: the Knight's and the nearest enemies' state at the end of the
//! last simulation tick, as one block of words at a fixed symbol. The replay
//! harness (tools/hkref) reads it through the emulator's RAM watch to compare
//! the port with the original per tick: the same quantities the original's
//! reference mod exports (hero state, clip and frame, positions, hit points).
//!
//! The block costs `WORDS * 4` bytes of RAM, 1,260 of code and a few hundred cycles a tick, so it
//! is built only with the `trace` feature (`HK_GUEST_FEATURES=trace` for host/build_guest.py).
//! Layout (all words little-endian, positions and velocities Q16 world units):
//!
//! | words | meaning |
//! |---|---|
//! | 0 | magic `HKTR` |
//! | 1 | layout version |
//! | 2 | simulation tick (`HK_SIM_TICKS`) |
//! | 3, 4, 5 | scene, region, drawn view |
//! | 6 | flags: bit 0 paused, 1 dead, 2 dialogue or shop open, 3 door pending |
//! | 7 | live enemies in the scene pool |
//! | 8, 9 | camera x, y |
//! | 10 | Geo wallet |
//! | 11 | enemy slots written below |
//! | 12..16 | reserved, zero |
//! | 16 | hero x |
//! | 17 | hero y |
//! | 18 | hero vertical velocity |
//! | 19 | hero facing, +1 or -1 |
//! | 20 | hero state, see `hero_state` |
//! | 21 | hero flags: bit 0 grounded, 1 jumping, 2 dashing, 3 wall sliding, 4 double jumping, 5 nail active, 6 focusing, 7 invulnerable, 8 wall locked |
//! | 22 | health (low half) and Lifeblood masks (high half) |
//! | 23 | SOUL |
//! | 24 | room clip the Knight plays |
//! | 25 | that clip's age in 60 Hz ticks (frame = age * rate) |
//! | 26 | nail kind (0 side, 1 alternate, 2 up, 3 down) in the low half, age in the high half |
//! | 27 | dash ticks left |
//! | 28 | pad the Knight read this tick |
//! | 29 | nail attacks begun so far |
//! | 30, 31 | Vengeful Spirit x and y, zero when none is flying |
//! | 32.. | enemy slots of `ENEMY_WORDS` words, see `enemies::EnemyWorld::trace` |
use crate::frame::Game;
use hk_sim::Player;

pub const MAGIC: u32 = 0x5254_4b48;
pub const VERSION: u32 = 1;
pub const HEADER: usize = 32;
pub const ENEMY_SLOTS: usize = 12;
pub const ENEMY_WORDS: usize = 8;
pub const WORDS: usize = HEADER + ENEMY_SLOTS * ENEMY_WORDS;
const _: () = assert!(WORDS * 4 <= 1024, "HK_TRACE must stay within a kilobyte");
#[no_mangle]
pub static mut HK_TRACE: [u32; WORDS] = [0; WORDS];

/// A coarse, comparable hero state: 0 idle, 1 running, 2 rising, 3 falling,
/// 4 dashing, 5 wall sliding, 6 striking, 7 focusing, 8 hurt, 9 dead.
#[optimize(size)]
fn hero_state(game: &Game) -> u32 {
    let p: &Player = &game.player;
    if game.vitals.dead { 9 }
    else if game.vitals.recoil_ticks > 0 { 8 }
    else if p.dash_left > 0 { 4 }
    else if p.wall_sliding { 5 }
    else if game.nail.active { 6 }
    else if game.focus.locks_control() { 7 }
    else if !p.grounded { if p.vy > 0 { 2 } else { 3 } }
    else if unsafe { crate::input::CONSUMED_PAD } & (psx_pad::button::LEFT | psx_pad::button::RIGHT) as u32 != 0 { 1 }
    else { 0 }
}

/// Publish this tick's state. Called once per simulation tick, after it.
#[inline(never)]
#[optimize(size)]
pub fn publish(game: &Game, scene: usize, region: usize) {
    let p = &game.player;
    let (cx, cy) = game.camera.position();
    let mut flags = 0u32;
    flags |= (game.paused as u32) | (game.vitals.dead as u32) << 1;
    flags |= ((crate::dialogue::open() || game.shop_screen.open) as u32) << 2 | (game.door.pending() as u32) << 3;
    let mut hero = 0u32;
    hero |= p.grounded as u32 | (p.jumping as u32) << 1 | ((p.dash_left > 0) as u32) << 2 | (p.wall_sliding as u32) << 3;
    hero |= (p.double_jumping as u32) << 4 | (game.nail.active as u32) << 5 | (game.focus.locks_control() as u32) << 6;
    hero |= ((game.vitals.invulnerable_ticks > 0) as u32) << 7 | (p.wall_locked as u32) << 8;
    let ball = game.cast.ball_bounds(crate::FIREBALL_PARAMS).map(|_| (game.cast.ball.x, game.cast.ball.y));
    unsafe {
        let t = &mut *(&raw mut HK_TRACE);
        t[0] = MAGIC;
        t[1] = VERSION;
        t[2] = crate::input::CONSUMED;
        t[3] = scene as u32;
        t[4] = region as u32;
        t[5] = game.view as u32;
        t[6] = flags;
        t[8] = cx as u32;
        t[9] = cy as u32;
        t[10] = game.geo.wallet();
        t[16] = p.x as u32;
        t[17] = p.y as u32;
        t[18] = p.vy as u32;
        t[19] = p.facing as u32;
        t[20] = hero_state(game);
        t[21] = hero;
        t[22] = game.vitals.health as u32 | (game.vitals.blue_health as u32) << 16;
        t[23] = game.vitals.soul as u32;
        t[24] = p.animation as u32;
        t[25] = p.animation_tick;
        t[26] = game.nail.kind as u32 | (game.nail.age as u32) << 16;
        t[27] = p.dash_left as u32;
        t[28] = crate::input::CONSUMED_PAD;
        t[29] = game.attacks;
        let (bx, by) = ball.unwrap_or((0, 0));
        t[30] = bx as u32;
        t[31] = by as u32;
        let (live, written) = game.enemies.trace(&mut t[HEADER..], ENEMY_SLOTS);
        t[7] = live;
        t[11] = written;
    }
}
