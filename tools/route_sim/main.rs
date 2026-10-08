//! Offline knight movement simulator over cooked room edges, for authoring
//! route tapes without emulator round trips. Same hk_sim step as the guest;
//! the union of the given rooms' edges stands in for per-region residency.
//!
//! usage: route_sim X Y "ticks:buttons,ticks:buttons,..." room.hk...
//! buttons: r l j (right, left, jump) or - for none. Prints x y grounded
//! after every segment, in world units.
extern crate hk_format;
extern crate hk_sim;
use hk_sim::{Player, ONE};
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/params.rs"));

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let x: f64 = args[1].parse().unwrap();
    let y: f64 = args[2].parse().unwrap();
    let script = &args[3];
    let mut edges: Vec<[i32; 4]> = Vec::new();
    for path in &args[4..] {
        let bytes = std::fs::read(path).unwrap();
        let room = hk_format::Room::parse(&bytes).unwrap();
        for i in 0..room.counts[5] {
            let edge = room.edge(i);
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    let mut player = Player::spawn((x * ONE as f64) as i32, (y * ONE as f64) as i32);
    player.facing = 1;
    let mut response = hk_sim::NailResponse::new();
    let mut tick = 0;
    for segment in script.split(',') {
        let (ticks, buttons) = segment.split_once(':').unwrap();
        let ticks: usize = ticks.parse().unwrap();
        let dir = i32::from(buttons.contains('r')) - i32::from(buttons.contains('l'));
        let jump = buttons.contains('j');
        for _ in 0..ticks {
            response.step(NAIL_RESPONSE_PARAMS, PARAMS, &mut player, dir, jump, edges.len(), |i| edges[i]);
            tick += 1;
        }
        println!("tick {tick:5} {segment:>8} x {:8.3} y {:8.3} vy {:7.2} grounded {}", player.x as f64 / ONE as f64,
            player.y as f64 / ONE as f64, player.vy as f64 / ONE as f64, player.grounded);
    }
}
