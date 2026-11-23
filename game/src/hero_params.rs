//! The one cached `HeroParams`: the cooked no-charm constants, then the shop's
//! PlayerData, then the equipped charms, then the cheats, composed only when
//! one of those changes.
//!
//! Composing at every read is what moved kings-return once (see the note over
//! `charms::compose`): the parameters are read several times a tick, so a read
//! here is a flag test and a byte compare, and the walk over the board runs
//! only after `invalidate`. `charms::publish` and `shop::publish` call that on
//! every change to their state, and the two cheats that change a value (Max
//! Nail and Max Masks) are the key the cache was composed for, so a cheat
//! toggle needs no call of its own.
use crate::cheats::Settings;
use hk_sim::HeroParams;

/// The cooked no-charm values, the bottom of the stack.
pub const BASE: HeroParams = HeroParams {
    vitals: crate::VITAL_PARAMS,
    attack: crate::ATTACK_PARAMS,
    fireball: crate::FIREBALL_PARAMS,
    focus: crate::FOCUS_PARAMS,
    nail_response: crate::NAIL_RESPONSE_PARAMS,
    ground_speed: crate::PARAMS.speed,
    dash_cooldown_ticks: crate::PARAMS.dash_cooldown_ticks,
    greed: false,
    reserve: hk_sim::SoulReserve {
        max: 0,
        per_hit: crate::charms::RESERVE_SOUL_PER_HIT,
    },
    slash_scale: hk_sim::ONE,
};
/// `StartSoulLimiter` sets `maxMP` to 66 while a Shade is owed, and `EndSoulLimiter` back to 99.
const SOUL_LIMITED: u16 = 66;
static mut LIMITED: bool = false;
/// The Shade's soul limiter, which the caller reports each tick; a change composes again.
pub fn set_soul_limited(on: bool) {
    // SAFETY: as in `invalidate`.
    unsafe {
        if LIMITED != on {
            LIMITED = on;
            FRESH = false;
        }
    }
}
/// `HeroController::SoulGain` for a nail hit that landed, with the live vessel and reserve.
#[inline(never)]
pub fn soul_gain(vitals: &mut hk_sim::Vitals, cheats: Settings) {
    let p = get(cheats);
    vitals.soul_gain(p.vitals, p.reserve);
}
/// The cheat key the cache was composed for, and whether it is still current.
static mut KEY: u8 = 0;
static mut FRESH: bool = false;
static mut CACHE: HeroParams = BASE;

/// The board or the shop changed: the next read composes again.
pub fn invalidate() {
    // SAFETY: the guest is single-threaded and no interrupt handler reads the cache.
    unsafe { FRESH = false };
}

/// The live parameters for `cheats`.
#[inline]
pub fn get(cheats: Settings) -> &'static HeroParams {
    let key = cheats.param_key();
    // SAFETY: as in `invalidate`.
    unsafe {
        if !FRESH || KEY != key {
            compose(key);
        }
        &*core::ptr::addr_of!(CACHE)
    }
}

/// The live parameters under the cheats of the last `get`, for a reader that
/// has no `Settings` to hand. Only the vitals depend on the cheats, so this is
/// exact for every other part of the record.
#[inline]
pub fn current() -> &'static HeroParams {
    // SAFETY: as in `invalidate`.
    unsafe {
        if !FRESH {
            compose(KEY);
        }
        &*core::ptr::addr_of!(CACHE)
    }
}

/// The live vitals for `cheats`: what `cheats::Settings::params` reads.
#[inline]
pub fn vitals(cheats: Settings) -> hk_sim::VitalParams {
    get(cheats).vitals
}

/// The live nail recoil and pogo, for the enemy receivers.
#[inline]
pub fn nail_response() -> hk_sim::NailResponseParams {
    current().nail_response
}

#[inline(never)]
fn compose(key: u8) {
    let mut p = BASE;
    // The source's order: `PlayerData::MaxHealth` is `maxHealthBase` plus the
    // charm bonuses, so the shop's masks come first and a charm adds to them.
    p.vitals = crate::shop::vitals(p.vitals);
    p.reserve.max = crate::shop::soul_reserve();
    p = crate::charms::hero(p);
    Settings::from_param_key(key).override_vitals(&mut p.vitals);
    // SAFETY: as in `invalidate`.
    if unsafe { LIMITED } {
        p.vitals.max_soul = p.vitals.max_soul.min(SOUL_LIMITED);
    }
    // SAFETY: as in `invalidate`.
    unsafe {
        CACHE = p;
        KEY = key;
        FRESH = true;
    }
}
