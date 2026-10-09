//! The cooked world a scene is run in: its regions, rooms and enemy placements, read the way the
//! guest reads them (world-metadata bank for placements and bounds, `chunk_N.hk` for terrain).
use super::*;
use hk_format::world_meta::{WorldMeta, KIND_ACTOR};
use std::path::{Path, PathBuf};

pub struct RegionData {
    pub global_id: u32,
    pub scene: usize,
    pub bounds: [i32; 4],
    pub collision_bounds: [i32; 4],
    pub room_bytes: &'static [u8],
    pub actors: &'static [(ActorPlacement, &'static ActorSpec)],
}
impl RegionData {
    pub fn room(&self) -> hk_format::Room<'static> {
        hk_format::Room::parse(self.room_bytes).expect("cooked room parses")
    }
    pub fn region(&self) -> world::Region {
        world::Region { scene: self.scene, bounds: self.bounds, collision_bounds: self.collision_bounds, actors: self.actors }
    }
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("HKBP_DATA").unwrap_or_else(|_| "data".into()))
}
pub fn banks_dir() -> PathBuf {
    PathBuf::from(std::env::var("HKBP_BANKS").unwrap_or_else(|_| ".hkpsx/world-metadata-packed".into()))
}

/// `game/src/world.rs::actor_placement`: the placement words of one `KIND_ACTOR` object.
fn placement(object: &hk_format::world_meta::Object<'_>) -> ActorPlacement {
    let flags = object.flags();
    ActorPlacement {
        source_id: object.state_id(),
        x: object.extra(1),
        y: object.extra(2),
        initial_direction: if flags & 1 != 0 { 1 } else { -1 },
        random_start_direction: flags & 2 != 0,
        start_alert: flags & 4 != 0,
        start_right: flags & 8 != 0,
        rotation_q16: ((flags >> 8) & 3) as i32 * 90 * ONE,
    }
}

fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

/// Every region of `scene`, in bank order.
pub fn load_scene(scene: usize) -> Vec<RegionData> {
    let bank_path = banks_dir().join(format!("scene_{scene}.hkwm"));
    let bank_bytes = leak(std::fs::read(&bank_path).unwrap_or_else(|e| panic!("{}: {e}", bank_path.display())));
    let bank = WorldMeta::parse(bank_bytes).expect("scene bank parses");
    let specs: &'static [ActorSpec] = catalogue::SCENE_ACTORS[scene];
    let mut out = Vec::new();
    for region in bank.regions() {
        let global_id = region.global_id();
        let chunk = data_dir().join(format!("regions/chunk_{}.hk", global_id));
        let room_bytes = leak(std::fs::read(&chunk).unwrap_or_else(|e| panic!("{}: {e}", chunk.display())));
        let mut actors = Vec::new();
        for i in 0..region.object_count() {
            let Some(object) = region.object(i) else { continue };
            if object.kind() == KIND_ACTOR && object.flags() & 64 != 0 {
                actors.push((placement(&object), &specs[object.extra(0) as usize]));
            }
        }
        out.push(RegionData {
            global_id,
            scene,
            bounds: region.bounds(),
            collision_bounds: region.collision_bounds(),
            room_bytes,
            actors: Box::leak(actors.into_boxed_slice()),
        });
    }
    out
}
