//! The Knight's hard landing, presented. The rule and the lock are the simulation's
//! (`hk_sim::Player::hard_landing_tick`, shared/hk-sim/src/hard_landing.rs); this is what the
//! landing looks like: `DoHardLanding` spawns `HardLandEffect`, which sends the camera `AverageShake`
//! and raises dust, and `HeroAnimationController` plays the `HardLand` clip for the whole lock.
//! The landing sound is the audio module's (`audio::movement_tick`).
//!
//! The dust is a stand-in: `HardLandEffect`'s own `Knight Hard Land Dust B/F` particle systems, its
//! impact lines and its flung rocks are not cooked yet, so the Knight's Focus dust (`Dust L` and
//! `Dust R`, already cooked into every scene) is raised at his feet instead.
//!
//! King's Pass's first fall is not an ordinary hard landing. Tutorial_01's `Initial Fall Impact`
//! FSM (`Control`) catches the new game's drop as the Knight passes y 11.56: it spawns the same
//! `HardLandEffect`, takes his control (`RelinquishControl`, `StopAnimationControl`), sends
//! `BigShake`, plays `Super Hard Land`, waits 0.75 s, lets the crows go (`Flock`) for 3.25 s, then
//! plays `Collect Normal 3` to get him up and hands control back. The crows, their calls and the
//! grass shake (`SHAKE ALL GRASS`) are not ported.
use crate::{ability_art, audio, camera, focus_fx, world};
use hk_sim::{HardLandingParams, Player};

/// The hard landing's two times, resampled from `HeroController` (data/sfx.rs).
pub const RULE: HardLandingParams = HardLandingParams {
    fall_ticks: audio::HARD_FALL_MIN_TICKS,
    land_ticks: audio::HARD_LANDING_TICKS,
};

/// `Land` waits 0.75 s and `Flock` 3.25 s (`Wait` in `Initial Fall Impact`'s `Control`, 60 Hz ticks).
const CRASH_FLOCK_AT: u16 = 45;
const CRASH_GET_UP_AT: u16 = CRASH_FLOCK_AT + 195;
/// Tutorial_01's scene index in the cooked world.
const KINGS_PASS: usize = 0;

/// Hard landings so far, and ticks spent locked in one, for the route pins.
#[no_mangle]
pub static mut HK_HARD_LANDINGS: u32 = 0;
#[no_mangle]
pub static mut HK_HARD_LOCK_TICKS: u32 = 0;
/// Ticks of the lock still to run, after this tick: a route that cuts its tape at a landing waits on
/// this reaching zero (tools/tapes/*.cuts), which is the tick control returns.
#[no_mangle]
pub static mut HK_HARD_LAND_LEFT: u32 = 0;
/// Opening crashes played (zero or one a new game).
#[no_mangle]
pub static mut HK_OPENING_CRASHES: u32 = 0;

/// The new game's drop has not landed yet.
static mut CRASH_PENDING: bool = false;
/// The lock now running is the opening crash, not an ordinary hard landing.
static mut CRASH_RUNNING: bool = false;

/// A new game begins at the top of King's Pass: its first landing is the opening crash.
pub fn arm_opening_crash() {
    unsafe {
        CRASH_PENDING = true;
        CRASH_RUNNING = false;
    }
}

/// Ticks the whole opening crash locks the Knight: the two waits and the get-up clip.
fn crash_ticks() -> u16 {
    CRASH_GET_UP_AT + ability_art::clip_ticks(ability_art::GET_UP) as u16
}

/// What `DoHardLanding` and `HardLandEffect.OnEnable` do on the landing tick (the simulation has
/// already locked the Knight for `RULE.land_ticks`).
pub fn landed(scene: usize, player: &mut Player) {
    let crash = unsafe { core::mem::take(&mut CRASH_PENDING) } && scene == KINGS_PASS;
    unsafe {
        HK_HARD_LANDINGS = HK_HARD_LANDINGS.wrapping_add(1);
        CRASH_RUNNING = crash;
        if crash {
            HK_OPENING_CRASHES = HK_OPENING_CRASHES.wrapping_add(1);
        }
    }
    if crash {
        player.start_hard_landing(crash_ticks());
    }
    camera::request(if crash {
        camera::Shake::Big
    } else {
        camera::Shake::Average
    });
    world::particles::pool().spawn_break_at(scene, focus_fx::DUST_OWNER, [player.x, player.y]);
}

/// Call once per consumed tick, after the Knight's lock was stepped.
pub fn tick(player: &Player) {
    unsafe {
        HK_HARD_LAND_LEFT = u32::from(player.hard_land);
    }
    if player.hard_landing() {
        unsafe {
            HK_HARD_LOCK_TICKS = HK_HARD_LOCK_TICKS.wrapping_add(1);
        }
    } else {
        unsafe {
            CRASH_RUNNING = false;
        }
    }
}

/// The clip that owns the Knight's body while he is locked, and its age, else none.
///
/// A hard landing plays `HardLand`. The clip runs at 12.5 fps and the ability art stores whole
/// frames per second (12), so its age is stretched by 25/24: `frame_index` then lands on the frame
/// the 12.5 fps clip shows at every tick. The opening crash lies on the last frame of `Super Hard
/// Land` until the crows have gone, then plays `Collect Normal 3`.
pub fn pose(player: &Player) -> Option<(usize, u32)> {
    if !player.hard_landing() {
        return None;
    }
    if unsafe { CRASH_RUNNING } {
        let age = player.hard_landing_age(crash_ticks());
        return Some(match u16::try_from(age) {
            Ok(age) if age >= CRASH_GET_UP_AT => {
                (ability_art::GET_UP, u32::from(age - CRASH_GET_UP_AT))
            }
            _ => (ability_art::SUPER_HARD_LAND, age),
        });
    }
    let age = player.hard_landing_age(RULE.land_ticks);
    Some((ability_art::HARD_LAND, age * 25 / 24))
}
