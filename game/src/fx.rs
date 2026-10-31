//! Effects of objects the Breakable cook does not own, raised through the break-effect pool.
//!
//! The owners are the cooker's (host/hk-cook/src/break_effects/fx.rs): emitters cooked in place
//! (the Great Door's dust) take no offset; the others are cooked at their anchor's origin and
//! raised where the anchor is now. A scene whose effect art did not fit has no emitter for an
//! owner, and raising it spawns nothing.

/// The nail's hit dust, the one owner `world.rs` raises on its own.
pub const HIT_DUST: usize = crate::world::particles::HIT_DUST_OWNER;
pub const DOOR_DUST: usize = 0xD010;
pub const DOOR_RUBBLE: usize = 0xD011;
pub const DOOR_CHIPS: usize = 0xD012;
pub const HIT_CHIPS: usize = 0xD013;
pub const HIT_FLASH: usize = 0xD014;
pub const DOOR_PULSE: usize = 0xD015;
pub const COCOON: usize = 0xD020;
pub const COCOON_CAP_RIGHT: usize = 0xD021;
pub const COCOON_CAP_LEFT: usize = 0xD022;
pub const JAR_GLASS: usize = 0xD030;
pub const SCUTTLER_BLOOD: usize = 0xD023;
pub const FK_DUST: usize = 0xD040;
pub const FK_MIST: usize = 0xD041;
pub const FK_FIRE: usize = 0xD042;
/// Every boss's death (game/src/boss_fx.rs): each boss scene cooks its own emitters under these two.
pub const BOSS_BLEED: usize = 0xD043;
pub const BOSS_BURST: usize = 0xD044;
pub const BOSS_HIT: usize = 0xD045;

/// Raise `owner`'s emitters, moved by `at` (world, Q16).
#[cfg(not(test))]
#[inline(never)]
pub fn raise(scene: usize, owner: usize, at: [i32; 2]) {
    crate::world::particles::pool().spawn_break_salted(scene, owner, at);
}
