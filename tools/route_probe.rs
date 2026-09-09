// Native geometry diagnostics. This deliberately reports contacts without
// claiming guest CD/input timing, enemy or health-system equivalence.
#![allow(dead_code)]
use hk_sim::{Nail, Player, ONE};
include!("params.rs");
include!("paths.rs");
mod render {
    pub fn reset_visibility() {}
    pub fn set_visible(_: usize, _: bool) {}
    pub fn set_gain(_: usize, _: u8) {}
}
mod world;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let events: Vec<(usize, &str, usize)> = args[1].split(',').map(|event| {
        let fields: Vec<_> = event.split(':').collect();
        (fields[0].parse().unwrap(), fields[1], fields[2].parse().unwrap())
    }).collect();
    let ticks: usize = args[2].parse().unwrap();
    let raw: Vec<_> = PATHS.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let rooms: Vec<_> = raw.iter().map(|p| hk_format::Room::parse(p).unwrap()).collect();
    assert_eq!(rooms.len(), world::REGIONS.len(), "selected/generated region count mismatch");
    let mut player = Player::spawn(SPAWN.0, SPAWN.1);
    let mut nail = Nail::new();
    let mut state = world::State::new();
    let (mut index, mut broken, mut hazard_ticks) = (0, 0, 0);
    let mut checkpoint = SPAWN;
    println!("tick,x,y,vy,ground,region,broken,hazard,checkpoint_x,checkpoint_y");
    for tick in 0..ticks {
        let held = |button: &str| events.iter().any(|(start, name, hold)| *name == button && tick >= *start && tick < start + hold);
        let (room, region) = (&rooms[index], &world::REGIONS[index]);
        player.step(PARAMS, held("right") as i32-held("left") as i32, held("cross"), room.counts[5], |i| state.edge(region, room, i));
        nail.tick(ATTACK_PARAMS, held("square"), held("up") as i32-held("down") as i32, &mut player);
        let strike = state.strike(region, &nail, ATTACK_PARAMS, NAIL_POLYGONS, &player);
        broken += strike.broken;
        let previous_checkpoint = checkpoint;
        if let Some(next) = world::checkpoint(region, &player, PARAMS) { checkpoint = next; }
        let hazard = world::hazard_contact(region, &player, PARAMS).is_some();
        hazard_ticks += usize::from(hazard);
        if tick % 10 == 0 || strike.broken > 0 || hazard || previous_checkpoint != checkpoint {
            println!("{},{:.5},{:.5},{:.5},{},{},{},{},{:.5},{:.5}", tick,
                player.x as f64/ONE as f64, player.y as f64/ONE as f64,
                player.vy as f64/ONE as f64, player.grounded, index+1, broken, hazard,
                checkpoint.0 as f64/ONE as f64, checkpoint.1 as f64/ONE as f64);
        }
        if !world::contains(region.bounds, player.x, player.y) {
            if let Some(next) = world::locate(region.scene, player.x, player.y) { index = next; }
            else { eprintln!("left cooked bounds at tick {tick}"); break; }
        }
    }
    eprintln!("final {:.5},{:.5}; broken={broken}; hazard_contact_ticks={hazard_ticks}", player.x as f64/ONE as f64, player.y as f64/ONE as f64);
}
