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
use hk_sim::{ActorSpec, CorpseSpec};

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scene_specs.rs"));

const EMPTY_SPEC: ActorSpec = ActorSpec {
    controller: hk_sim::ActorController::Gruzzer,
    bounds: [0; 4],
    health: hk_sim::EnemyParams {
        health: 0,
        contact_damage: 0,
        evasion_ticks: 0,
        invincible: false,
        damage_override: false,
    },
    walk: hk_sim::WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0,
    turn_clip: 0,
    corpse: None,
    recoil_speed: 0,
    recoil_ticks: 0,
    dream_soul: 0,
};
const EMPTY_CORPSE: CorpseSpec = CorpseSpec {
    air_clip: 0,
    land_clip: 0,
    bounds: [0; 4],
    spawn_offset: [0; 2],
    bounce_factor: 0,
    fling_speed: 0,
    gravity: 0,
    breaker: false,
    smash_bounces: 0,
    remove_after_land: 0,
    hold_ticks: 0,
};

static mut SPECS: [ActorSpec; MAX_SCENE_SPECS] = [EMPTY_SPEC; MAX_SCENE_SPECS];
static mut CORPSES: [CorpseSpec; MAX_SCENE_CORPSES] = [EMPTY_CORPSE; MAX_SCENE_CORPSES];
/// Catalogue scene whose specs the pool holds, and how many.
static mut LOADED: (usize, usize) = (usize::MAX, 0);

/// Bytes a spec chunk occupies on the disc.
pub const CHUNK_BYTES: usize = SPEC_SECTORS * 2048;

/// Decode `scene_id`'s chunk into the pool. False (and an empty pool) when the chunk is malformed,
/// is another scene's, or does not fit the pool.
pub fn load(scene_id: usize, chunk: &[u8]) -> bool {
    // SAFETY: the pool is touched on the main thread only, by this function (an exclusive
    // admission) and by `specs`; the references it handed out for the previous scene are dead by
    // the module comment's argument.
    unsafe {
        LOADED = (usize::MAX, 0);
        let corpses: &'static mut [CorpseSpec] = &mut *(&raw mut CORPSES);
        let specs: &mut [ActorSpec] = &mut *(&raw mut SPECS);
        match hk_sim::wire::decode_chunk(chunk, corpses, specs) {
            Some(head) if head.scene_id as usize == scene_id => {
                LOADED = (scene_id, head.specs);
                true
            }
            _ => false,
        }
    }
}

/// The admitted scene's enemy types, in the order its placements index them; empty for any other
/// scene.
pub fn specs(scene_id: usize) -> &'static [ActorSpec] {
    // SAFETY: see `load`.
    unsafe {
        let (loaded, count) = LOADED;
        if loaded == scene_id {
            &(&*(&raw const SPECS))[..count]
        } else {
            &[]
        }
    }
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
