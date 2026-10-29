//! Desolate Dive and Howling Wraiths in the frame loop (docs/SPELLS.md): what a tap of the cast button
//! with Down or Up held starts, the Knight's body while it runs, and the shapes that hurt.
//!
//! This file and `hk_sim`'s `quake` and `scream` are the code module `spells` (host/code_modules.py): it is
//! carried in once the Knight owns either spell and the pool has room (`frame.rs` `carry_ability_art`),
//! and the frame loop calls into it only while `modules::loaded(modules::SPELLS)`. The state, which the
//! loop reads every tick, is the two small structs of `Game`.
use crate::frame::Game;
use crate::{camera, world};
use hk_format::Room;
use hk_sim::{SpellKind, Strike};

/// Desolate Dives and Howling Wraiths begun, Dives landed (the route pins read them).
#[no_mangle]
pub static mut HK_QUAKES: u32 = 0;
#[no_mangle]
pub static mut HK_QUAKE_LANDINGS: u32 = 0;
#[no_mangle]
pub static mut HK_SCREAMS: u32 = 0;

/// Start the spell the tap chose (the soul is paid already, in `Cast`).
#[inline(never)]
pub fn begin(game: &mut Game, kind: SpellKind) {
    if kind == SpellKind::Quake {
        unsafe {
            HK_QUAKES += 1;
        }
        game.quake.begin(&crate::QUAKE_PARAMS, game.cast.quake_level, game.player.grounded);
    } else {
        unsafe {
            HK_SCREAMS += 1;
        }
        game.scream.begin(game.cast.scream_level);
    }
}

/// One tick of the Knight while a spell owns him: velocity set, gravity and steering off, one terrain step.
#[inline(never)]
pub fn drive(game: &mut Game, r: &world::Region, room: &Room) {
    let mut p = crate::PARAMS;
    p.speed = 0;
    p.gravity = 0;
    game.player.end_hard_landing();
    game.player.jumping = false;
    game.player.double_jumping = false;
    game.player.vy = game.quake.velocity(&crate::QUAKE_PARAMS).unwrap_or(0);
    game.player.step(p, 0, false, room.counts[5], &game.state.edge_reader(r, room));
    if game.quake.tick(&crate::QUAKE_PARAMS, &game.player) {
        // `Quake1 Land`: `BigShake`.
        camera::request(camera::Shake::Big);
        unsafe {
            HK_QUAKE_LANDINGS += 1;
        }
    }
    if game.scream.tick(&crate::SCREAM_PARAMS) {
        // `Scream Burst`: `BigShake` as the roar begins.
        camera::request(camera::Shake::Big);
    }
}

/// The shapes that hurt this tick, written to `out`: how many.
#[inline(never)]
pub fn strikes(game: &Game, out: &mut [Strike; 6]) -> usize {
    let n = game.quake.strikes(&crate::QUAKE_PARAMS, &game.player, out);
    n + game.scream.strikes(&crate::SCREAM_PARAMS, &game.player, &mut out[n..])
}
