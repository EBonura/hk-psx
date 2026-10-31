//! The resident side of a boss's drawing module: three small calls the frame and the animation cache make into `hornet_art`
//! (game/src/boss_bank.rs) while it is resident. Each is its own function so the hot callers hold a plain call, not the
//! module's import site: the same hooks written inline cost the frame loop's functions 4 KB of code between them.
use hk_format::Room;

/// The animation-cache key range of the boss bank's parts, which an actor bank (game/src/actor_bank.rs) shares: a scene
/// carries one or the other.
pub const KEY_BASE: u16 = crate::actor_bank::KEY_BASE;

/// Bind the actor bank of manifest scene `scene` (if it has one) to the view `room`, once per view bind (`render::init`). The
/// reader lives in the `bank_reader` module, which the scenes that have a bank carry; everywhere else the bank is just taken
/// away.
#[inline(never)]
pub fn bind_bank(room: &Room, scene: usize) {
    crate::actor_bank::clear();
    if crate::modules::loaded(crate::modules::BANK_READER) {
        crate::actor_bank::attach(room, scene);
    }
}

/// The bytes of resident data package `m` (`modules::bytes`), out of line: inlined into the bank reader's module it would
/// reference the pool's base, a linker constant that only resident code may name.
#[inline(never)]
pub fn package(m: usize) -> Option<&'static [u8]> {
    crate::modules::bytes(m)
}

/// Upload decoded texels into animation-cache slot `slot` (a part `width` by `height` texels). The bank reader's module calls this
/// instead of inlining `animation_cache::dest`, whose power-of-two arithmetic reads a lookup table that only the resident image
/// owns.
#[inline(never)]
pub fn upload_texels(slot: usize, width: u8, height: u8, texels: &[u8]) {
    crate::texture_upload::upload(crate::animation_cache::dest(slot, width as u16, height as u16), texels);
}

/// Decode bank part `key - KEY_BASE` into cache slot `slot`; the bytes uploaded.
#[inline(never)]
pub fn upload(room: &Room, key: u16, slot: usize) -> Option<u32> {
    if crate::actor_bank::active() && crate::modules::loaded(crate::modules::BANK_READER) {
        return crate::actor_bank::upload((key - KEY_BASE) as usize, slot);
    }
    #[cfg(feature = "hornet")]
    return crate::boss_bank::upload(room, (key - KEY_BASE) as usize, slot);
    #[cfg(not(feature = "hornet"))]
    {
        let _ = room;
        None
    }
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
