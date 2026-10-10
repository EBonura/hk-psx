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
//! (`data/actor_persistence.rs`: scene, source id, state group). This module
//! keeps the deaths in `persist` (kind `Enemy`, above the Blockers' own ids)
//! so they ride the bench save with the rest of the world, and `enemies.rs`
//! seats a placement dead when its state says so, the way it already does for
//! a Blocker.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/actor_persistence.rs"
));
use crate::persist::{self, Kind};

/// First `Kind::Enemy` local id this module uses. The Blockers keep the ids
/// below it (their index in `blocker_terrain::SOURCES`).
pub const LOCAL_BASE: usize = persist::ENEMY_STATE_BASE;
const _: () = assert!(
    crate::blocker_terrain::BLOCKERS <= LOCAL_BASE,
    "the Blockers' ids would reach into the state groups"
);
const _: () = assert!(
    LOCAL_BASE + GROUPS <= persist::MAX_LOCAL,
    "a state group does not fit a persist id"
);
/// Stored values: dead for good, and dead until the next reset.
const DEAD: u8 = 1;
const DEAD_UNTIL_RESET: u8 = persist::ENEMY_DEAD_UNTIL_RESET;

#[inline(never)]
#[optimize(size)]
fn key(scene: usize, source_id: u32) -> Option<(usize, bool)> {
    let (group, semi) = hk_sim::persistent_actor(PERSISTENT_ACTORS, scene, source_id)?;
    Some((LOCAL_BASE + group as usize, semi))
}

/// Whether the store holds this placement's state as dead, so seating it would
/// bring back what the source took out of play.
#[optimize(size)]
pub fn dead(scene: usize, source_id: u32) -> bool {
    key(scene, source_id)
        .is_some_and(|(local, _)| persist::get(Kind::Enemy, scene, local).is_some())
}

/// A kill: the source's `PersistentBoolItem` going true.
#[optimize(size)]
pub fn killed(scene: usize, source_id: u32) {
    if let Some((local, semi)) = key(scene, source_id) {
        persist::set(
            Kind::Enemy,
            scene,
            local,
            if semi { DEAD_UNTIL_RESET } else { DEAD },
        );
    }
}
