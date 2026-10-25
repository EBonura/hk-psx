//! The resident side of a boss's drawing module: three small calls the frame and the animation cache make into `hornet_art`
//! (game/src/boss_bank.rs) while it is resident. Each is its own function so the hot callers hold a plain call, not the
//! module's import site: the same hooks written inline cost the frame loop's functions 4 KB of code between them.
use hk_format::Room;

/// The animation-cache key range of the boss bank's parts.
#[cfg(feature = "hornet")]
pub const KEY_BASE: u16 = crate::boss_bank::KEY_BASE;
#[cfg(not(feature = "hornet"))]
pub const KEY_BASE: u16 = u16::MAX;

/// Decode bank part `key - KEY_BASE` into cache slot `slot`; the bytes uploaded.
#[cfg(feature = "hornet")]
#[inline(never)]
pub fn upload(room: &Room, key: u16, slot: usize) -> Option<u32> {
    crate::boss_bank::upload(room, (key - KEY_BASE) as usize, slot)
}
#[cfg(not(feature = "hornet"))]
pub fn upload(_room: &Room, _key: u16, _slot: usize) -> Option<u32> {
    None
}

/// Add this frame's queued parts to the working set.
#[cfg(feature = "hornet")]
#[inline(never)]
pub fn append_needed(needed: &mut [u16], len: &mut usize) {
    if crate::modules::loaded(crate::modules::HORNET_ART) {
        crate::boss_bank::append_needed(needed, len);
    }
}
#[cfg(not(feature = "hornet"))]
pub fn append_needed(_needed: &mut [u16], _len: &mut usize) {}

/// Draw the queued parts; the packets added.
#[cfg(feature = "hornet")]
#[inline(never)]
pub fn draw() -> u32 {
    if crate::modules::loaded(crate::modules::HORNET_ART) {
        crate::boss_bank::draw()
    } else {
        0
    }
}
#[cfg(not(feature = "hornet"))]
pub fn draw() -> u32 {
    0
}
