// Native geometry diagnostics. This deliberately reports contacts without
// claiming guest CD/input timing, enemy or health-system equivalence.
//
// The guest modules compile where they live, so rustc resolves the submodules
// world.rs declares. tools/route_probe.py builds this with `--cfg test`, which
// is the same shape tests/world_runtime.rs uses: the PS1 presentation paths
// drop out and world.rs reads its bank from TEST_BANK instead of the disc.
#![allow(dead_code)]
use hk_sim::{Nail, Player, ONE};
include!("params.rs");
include!("paths.rs");
mod render {
    pub const SOURCE_ALPHA_COVERAGE: u8 = 1;
    pub fn reset_visibility() {}
    pub fn set_visible(_: usize, _: bool) {}
    pub fn set_gain(_: usize, _: u8) {}
    pub fn set_opacity(_: usize, _: u8) {}
    pub fn texture(_: usize, _: [(i16, i16); 4], _: (u8, u8, u8)) {}
    pub fn texture_material_alpha(_: usize, _: [(i16, i16); 4], _: (u8, u8, u8), _: u8) {}
}
mod impact;
mod reveal_masks;
mod world;

/// Admit the cooked bank for this scene, as a scene gate does on the guest.
/// Leaked because world.rs holds the bank for 'static, exactly as the disc
/// arena does; the probe admits at most one bank per scene it visits.
fn admit_scene(scene: usize) -> hk_format::WorldMeta<'static> {
    let path = BANKS.iter().find(|(id, _)| *id == scene)
        .unwrap_or_else(|| panic!("no cooked world bank for scene {scene}")).1;
    let bytes: &'static [u8] = Box::leak(std::fs::read(path).unwrap().into_boxed_slice());
    world::TEST_BANK.with(|bank| bank.set(bytes));
    hk_format::WorldMeta::parse(bytes).expect("cooked world bank")
}
/// The spatial lookup `disc::Cache::locate` performs, against the admitted bank.
fn locate(bank: &hk_format::WorldMeta<'static>, scene: usize, x: i32, y: i32) -> Option<usize> {
    if bank.scene_id() as usize != scene {
        return None;
    }
    for index in 0..bank.region_count() {
        let region = bank.region(index)?;
        let b = region.bounds();
        if x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3] {
            let global = region.global_id() as usize;
            if global >= 1 && world::scene_of(global - 1) == scene {
                return Some(global - 1);
            }
        }
    }
    None
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let events: Vec<(usize, &str, usize)> = args[1].split(',').map(|event| {
        let fields: Vec<_> = event.split(':').collect();
        (fields[0].parse().unwrap(), fields[1], fields[2].parse().unwrap())
    }).collect();
    let ticks: usize = args[2].parse().unwrap();
    let raw: Vec<_> = PATHS.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let rooms: Vec<_> = raw.iter().map(|p| hk_format::Room::parse(p).unwrap()).collect();
    assert_eq!(rooms.len(), world::REGION_SCENES.len(), "selected/generated region count mismatch");
    let mut index = 0;
    let mut scene = world::scene_of(index);
    let mut bank = admit_scene(scene);
    let mut player = Player::spawn(SPAWN.0, SPAWN.1);
    let mut nail = Nail::new();
    let mut state = world::State::new();
    let (mut broken, mut hazard_ticks) = (0, 0);
    let mut checkpoint = ([SPAWN.0, SPAWN.1], 1);
    println!("tick,x,y,vy,ground,region,broken,hazard,checkpoint_x,checkpoint_y,facing");
    for tick in 0..ticks {
        let held = |button: &str| events.iter().any(|(start, name, hold)| *name == button && tick >= *start && tick < start + hold);
        let room = &rooms[index];
        let region = world::resident(index).expect("selected region resident in its admitted bank");
        player.step(PARAMS, held("right") as i32 - held("left") as i32, held("cross"), room.counts[5],
            state.edge_reader(&region, room));
        nail.tick(ATTACK_PARAMS, held("square"), held("up") as i32 - held("down") as i32, &mut player);
        let strike = state.strike(&region, &nail, ATTACK_PARAMS, NAIL_POLYGONS, &player);
        broken += strike.broken;
        let previous_checkpoint = checkpoint;
        // The bank Region carries the hazard and checkpoint tables the guest reads.
        let bank_region = world::bank_region(&region).expect("admitted bank covers this region");
        if let Some(next) = world::checkpoint(bank_region, &player, PARAMS) { checkpoint = next; }
        let hazard = world::hazard_contact(bank_region, &player, PARAMS).is_some();
        hazard_ticks += usize::from(hazard);
        if tick % 10 == 0 || strike.broken > 0 || hazard || previous_checkpoint != checkpoint {
            println!("{},{:.5},{:.5},{:.5},{},{},{},{},{:.5},{:.5},{}", tick,
                player.x as f64 / ONE as f64, player.y as f64 / ONE as f64,
                player.vy as f64 / ONE as f64, player.grounded, index + 1, broken, hazard,
                checkpoint.0[0] as f64 / ONE as f64, checkpoint.0[1] as f64 / ONE as f64, checkpoint.1);
        }
        if !world::contains(region.bounds, player.x, player.y) {
            if let Some(next) = locate(&bank, region.scene, player.x, player.y) {
                index = next;
                if world::scene_of(index) != scene {
                    scene = world::scene_of(index);
                    bank = admit_scene(scene);
                }
            } else {
                eprintln!("left cooked bounds at tick {tick}");
                break;
            }
        }
    }
    eprintln!("final {:.5},{:.5}; broken={broken}; hazard_contact_ticks={hazard_ticks}",
        player.x as f64 / ONE as f64, player.y as f64 / ONE as f64);
}
