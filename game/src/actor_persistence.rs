//! Enemies the source keeps dead.
//!
//! An enemy that carries a `PersistentBoolItem` writes it when it dies
//! (`EnemyDeathEffects` calls its `SaveState`), and the item puts it back out
//! of play whenever its scene next loads: `HealthManager` answers the saved
//! state by setting `isDead` and deactivating the object. The item is keyed by
//! its owner's name and scene, so placements that share a name share one
//! state, and `semiPersistent` ones are forgotten when the Knight rests or dies.
//!
//! `host/hk-cook/src/actor_persistence.rs` cooks which placements those are
//! (`data/actor_persistence.rs`: scene, source id, stable state id). This module
//! keeps the deaths in `persist` (kind `Enemy`, above the Blockers' own ids)
//! so they ride the bench save with the rest of the world, and `enemies.rs`
//! seats a placement dead when its state says so, the way it already does for
//! a Blocker.
#[cfg(not(test))]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/actor_persistence.rs"));
#[cfg(test)]
use crate::actor_persistence_table::PERSISTENT_ACTORS;
use crate::persist::{self, Kind};

/// First `Kind::Enemy` local id this module uses. The Blockers keep the ids
/// below it (their index in `blocker_terrain::SOURCES`).
pub const LOCAL_BASE: usize = 16;
const _: () = assert!(crate::blocker_terrain::BLOCKERS <= LOCAL_BASE, "the Blockers' ids would reach into the stable state ids");
const _: () = { let mut i = 0; while i < PERSISTENT_ACTORS.len() { let (scene, _, word) = PERSISTENT_ACTORS[i]; let local = (word & !hk_sim::SEMI_PERSISTENT) as usize; assert!((scene as usize) < persist::MAX_SCENE); assert!(local >= LOCAL_BASE && local < persist::MAX_LOCAL); i += 1; } };
/// Stored values: dead for good, and dead until a bench rest.
const DEAD: u8 = 1;
const DEAD_UNTIL_REST: u8 = 2;

#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
fn key(scene: usize, source_id: u32) -> Option<(usize, bool)> {
    let (group, semi) = hk_sim::persistent_actor(PERSISTENT_ACTORS, scene, source_id)?;
    Some((group as usize, semi))
}

/// Whether the store holds this placement's state as dead, so seating it would
/// bring back what the source took out of play.
#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn dead(scene: usize, source_id: u32) -> bool {
    key(scene, source_id).is_some_and(|(local, semi)|
        persist::get(Kind::Enemy, scene, local) == Some(if semi { DEAD_UNTIL_REST } else { DEAD }))
}

/// A kill: the source's `PersistentBoolItem` going true.
#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn killed(scene: usize, source_id: u32) {
    if let Some((local, semi)) = key(scene, source_id) {
        persist::set(Kind::Enemy, scene, local, if semi { DEAD_UNTIL_REST } else { DEAD });
    }
}
