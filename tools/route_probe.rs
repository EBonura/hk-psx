// Native geometry diagnostics. This deliberately reports contacts without
// claiming guest CD/input timing, enemy or health-system equivalence.
//
// The guest modules compile where they live, so rustc resolves the submodules
// world.rs declares. tools/route_probe.py builds this with `--cfg test`, which
// is the same shape tests/world_runtime.rs uses: the PS1 presentation paths
// drop out and world.rs reads its bank from TEST_BANK instead of the disc.
#![allow(dead_code)]
// world.rs marks its cold paths #[optimize(size)], as the guest crate allows.
#![feature(optimize_attribute)]
use hk_sim::{DreamNail, Nail, Player, SuperDash, ONE};
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
    pub fn black_mask(_: usize) -> bool { false }
    pub fn draw_is_front(_: usize) -> bool { false }
    pub fn draw_scenery_offset(_: usize, _: [i32; 2], _: (i32, i32)) -> u32 { 0 }
    pub fn draw_scenery_quad(_: usize, _: [[i32; 2]; 4], _: (i32, i32)) -> u32 { 0 }
}
mod impact;
mod reveal_masks;
mod secret_breaks;
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
    // Only the abilities that need a button are granted, so a route that never
    // presses L1, R1 or Triangle behaves exactly as the guest does. The Mantis
    // Claw is deliberately withheld: a wall slide and a wall jump need no
    // button at all, so granting it would make any route that presses into a
    // wall in mid-air slide or kick off here and not on the disc. That cost a
    // route once already. `--claw` grants it for deliberate wall authoring.
    let claw = std::env::args().any(|a| a == "--claw");
    player.has_dash = true;
    player.has_walljump = claw;
    player.has_double_jump = true;
    player.has_super_dash = true;
    player.has_shade_cloak = true;
    let mut nail = Nail::new();
    let mut dream = DreamNail::new();
    dream.has_dream_nail = true;
    let mut state = world::State::new();
    let (mut broken, mut hazard_ticks) = (0, 0);
    let mut swings = 0u32;
    let mut checkpoint = ([SPAWN.0, SPAWN.1], 1);
    println!("tick,x,y,vy,ground,region,broken,hazard,checkpoint_x,checkpoint_y,facing,\
dash,wall,superdash,dream");
    for tick in 0..ticks {
        let held = |button: &str| events.iter().any(|(start, name, hold)| *name == button && tick >= *start && tick < start + hold);
        let room = &rooms[index];
        let region = world::resident(index).expect("selected region resident in its admitted bank");
        dream.tick(DREAM_NAIL_PARAMS, held("triangle"), &player, false);
        player.super_dash_input(PARAMS, held("r1"));
        player.dash_input(PARAMS, held("l1"));
        let locked = dream.locks_control();
        player.step(PARAMS,
            if locked { 0 } else { held("right") as i32 - held("left") as i32 },
            held("cross") && !locked, room.counts[5],
            state.edge_reader(&region, room));
        if nail.tick(ATTACK_PARAMS, held("square"), held("up") as i32 - held("down") as i32, &mut player) { swings += 1; }
        let body = [player.x - PARAMS.half_width, player.y + PARAMS.bottom, player.x + PARAMS.half_width, player.y + PARAMS.top];
        let strike = state.strike(&region, &nail, ATTACK_PARAMS, NAIL_POLYGONS, &player, swings, body);
        state.tick();
        broken += strike.broken;
        let previous_checkpoint = checkpoint;
        // The bank Region carries the hazard and checkpoint tables the guest reads.
        let bank_region = world::bank_region(&region).expect("admitted bank covers this region");
        if let Some(next) = world::checkpoint(bank_region, &player, PARAMS) { checkpoint = next; }
        let hazard = world::hazard_contact(bank_region, &player, PARAMS).is_some();
        hazard_ticks += usize::from(hazard);
        // Every tick: route authoring needs to see the exact frame a landing,
        // a buffered jump or a ledge contact happens, not a 10-tick average.
        {
            let wall = if player.wall_locked { "locked" } else if player.wall_sliding { "sliding" }
                else if player.touching_wall != 0 { "touching" } else { "-" };
            let superdash = match player.super_dash {
                SuperDash::Off => "-", SuperDash::Charging(_) => "charging",
                SuperDash::Ready => "ready", SuperDash::Travelling(_) => "travelling",
                SuperDash::Recovering(_) => "recovering",
            };
            println!("{},{:.5},{:.5},{:.5},{},{},{},{},{:.5},{:.5},{},{},{},{},{:?}", tick,
                player.x as f64 / ONE as f64, player.y as f64 / ONE as f64,
                player.vy as f64 / ONE as f64, player.grounded, index + 1, broken, hazard,
                checkpoint.0[0] as f64 / ONE as f64, checkpoint.0[1] as f64 / ONE as f64, checkpoint.1,
                player.dash_left, wall, superdash, dream.phase);
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
