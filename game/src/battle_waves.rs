//! Waves arenas: the cooked table `host/hk-cook/src/battle_waves.rs` writes.
//!
//! Crossroads_22's `Battle Control` summons four waves of ordinary enemies one
//! at a time and, when the fight starts, kills what stood in the room first.
//! `shared/hk-sim/src/waves.rs` is the order of that fight; `enemies.rs` seats
//! the members, so this module only answers which enemy belongs to which part
//! of it. A summoned member is not seated with the scene (the 32 actor slots
//! could not hold every wave at once), so `enemies.rs` asks here to leave it out
//! of `sync_region` and to find it when its wave comes.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/battle_waves.rs"
));

/// The arena of `scene`, if it has one.
pub fn arena(scene: usize) -> Option<&'static Arena> {
    ARENAS.iter().find(|a| a.scene as usize == scene)
}

/// The members of `scene`, sorted by source id.
pub fn members(scene: usize) -> &'static [(u16, u32, u8)] {
    let first = MEMBERS.partition_point(|m| (m.0 as usize) < scene);
    let len = MEMBERS[first..]
        .iter()
        .take_while(|m| m.0 as usize == scene)
        .count();
    &MEMBERS[first..first + len]
}

fn word(scene: usize, source_id: u32) -> u8 {
    members(scene)
        .iter()
        .find(|m| m.1 == source_id)
        .map_or(0, |m| m.2)
}

/// The 1 based wave an enemy is summoned in, or 0.
pub fn wave_of(scene: usize, source_id: u32) -> u8 {
    word(scene, source_id) & 7
}

/// Whether `Remove on battle start` kills this enemy when the fight begins.
pub fn removed(scene: usize, source_id: u32) -> bool {
    word(scene, source_id) & REMOVED != 0
}
