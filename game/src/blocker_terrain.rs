//! The Blockers' `Terrain Block` children, lifted when their Blocker dies.
//!
//! The world cook bakes each block as ordinary terrain; in the source the
//! Blocker GameObject is destroyed on death with the block under it, and its
//! `PersistentBoolItem` keeps it destroyed across reloads and the card. So the
//! state lives in `persist` (kind `Enemy`, local id the Blocker's index here)
//! and `apply` drops a dead Blocker's block edges from the view's terrain, the
//! way `battle_gates::apply` drops an open gate's. `host/blocker_terrain.py`
//! is the cooked join.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/blocker_terrain.rs"));
use crate::persist::{self, Kind};

const _: () = assert!(BLOCKERS <= 8, "the dead set is one u8");

/// Blockers the store holds as dead, one bit each; the evidence a replay reads.
#[no_mangle] pub static mut HK_BLOCKERS_DEAD: u32 = 0;

fn index(scene: usize, source_id: u32) -> Option<usize> {
    SOURCES.iter().position(|&(s, id)| s as usize == scene && id == source_id)
}
fn dead_index(i: usize) -> bool {
    persist::get(Kind::Enemy, SOURCES[i].0 as usize, i).is_some()
}
/// Whether this placement is a Blocker the store already holds as dead, so
/// seating it would bring back what the source destroyed.
pub fn dead(scene: usize, source_id: u32) -> bool {
    index(scene, source_id).is_some_and(dead_index)
}
/// A kill: record it, the source's `PersistentBoolItem` going true.
pub fn killed(scene: usize, source_id: u32) {
    if let Some(i) = index(scene, source_id) {
        persist::set(Kind::Enemy, scene, i, 1);
        unsafe { HK_BLOCKERS_DEAD |= 1 << i; }
        crate::world::scripted_terrain_changed();
    }
}
/// Drop every dead Blocker's block edges from this view's terrain, after the
/// Lifeblood refresh and beside the gates' exclusions.
pub fn apply(state: &mut crate::world::State, region: usize) {
    let slot = region as u16;
    let first = REGIONS.partition_point(|&(row, _, _)| row < slot);
    for &(row, blocker, edges) in &REGIONS[first..] {
        if row != slot { break; }
        if dead_index(blocker as usize) {
            state.append_script_edges(edges);
            unsafe { HK_BLOCKERS_DEAD |= 1 << blocker; }
        }
    }
}
