//! The enemy types of the admitted scene (`ActorSpec`, `CorpseSpec`), decoded from the scene's spec
//! chunk at its gate (host/hk-specpack, hk-sim `wire`). They used to be linked statics for all 65
//! scenes (15.7 KB of `.data`); now the image holds one scene's worth, sized by the cook
//! (`MAX_SCENE_SPECS`, `MAX_SCENE_CORPSES`: a scene past them fails the cook, not the boot).
//!
//! The pool is overwritten only by an admission, which is exclusive: it runs at a drained,
//! blacked-out gate, after the outgoing scene's metadata bank was revoked, so nothing resolves a
//! placement of the old scene any more. An `Actor` keeps no spec; the world's `placed_spec` table
//! holds references into this pool and `sync_region` rebuilds it whenever the admitted region
//! changes (`world::actors_key`), before any tick of the new scene.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scene_specs.rs"));

static mut POOL: hk_sim::wire::SpecPool<MAX_SCENE_SPECS, MAX_SCENE_CORPSES> = hk_sim::wire::SpecPool::new();

/// Bytes a spec chunk occupies on the disc.
pub const CHUNK_BYTES: usize = SPEC_SECTORS * 2048;

/// Decode `scene_id`'s chunk into the pool. False (and an empty pool) when the chunk is malformed,
/// is another scene's, or does not fit the pool.
pub fn load(scene_id: usize, chunk: &[u8]) -> bool {
    // SAFETY: the pool is touched on the main thread only, by this function (an exclusive
    // admission) and by `specs`; the references it handed out for the previous scene are dead by
    // the module comment's argument.
    unsafe { (*(&raw mut POOL)).load(scene_id, chunk) }
}

/// The admitted scene's enemy types, in the order its placements index them; empty for any other
/// scene.
pub fn specs(scene_id: usize) -> &'static [hk_sim::ActorSpec] {
    // SAFETY: see `load`.
    unsafe { (*(&raw const POOL)).specs(scene_id) }
}

/// The runner audio family scene `scene_id` plays, from the cook (the next scene's bank is wanted
/// before its specs are read): None when it places no Runner and no Zombie Shield, Some(true) for
/// Mossman.
pub fn runner_family(scene_id: usize) -> Option<bool> {
    match SCENE_RUNNER_FAMILY.get(scene_id) {
        Some(2) => Some(false),
        Some(3) => Some(true),
        _ => None,
    }
}

/// The scene that places the False Knight, if the build has one.
pub fn false_knight_scene() -> Option<usize> {
    (FALSE_KNIGHT_SCENE != usize::MAX).then_some(FALSE_KNIGHT_SCENE)
}
