//! Running a scene's enemies natively, tick by tick.
use super::*;
use crate::world_data::{load_scene, RegionData};

/// The swing lasts one tick and always reaches: the polygon decides what a strike meets.
fn no_attack() -> AttackParams {
    AttackParams { duration: 20, cooldown: 24, alternate_reset: 30, hit_start: 0, hit_end: 1, ..AttackParams::ZERO }
}

/// A box swung at the right of the Knight (he faces right), large enough to reach the actor beside him.
const STRIKE: &[[i32; 2]] = &[[-4 * ONE, -2 * ONE], [0, -2 * ONE], [0, 2 * ONE], [-4 * ONE, 2 * ONE]];

/// One simulation step with the Knight idle.
pub fn step(
    world: &mut enemies::EnemyWorld,
    here: &RegionData,
    all: &[RegionData],
    player: &mut Player,
    vitals: &mut Vitals,
    camera: [i32; 3],
) -> enemies::Events {
    step_with(world, here, all, player, vitals, camera, &Nail::new(), false)
}

/// One simulation step; `strike` makes the nail hit this tick (the box above, at the Knight).
pub fn step_with(
    world: &mut enemies::EnemyWorld,
    here: &RegionData,
    all: &[RegionData],
    player: &mut Player,
    vitals: &mut Vitals,
    camera: [i32; 3],
    nail: &Nail,
    strike: bool,
) -> enemies::Events {
    let region = here.region();
    let room = here.room();
    let rooms: Vec<(world::Region, hk_format::Room<'static>)> = all.iter().map(|r| (r.region(), r.room())).collect();
    world.tick(
        &region,
        &room,
        &mut world::State,
        player,
        vitals,
        nail,
        &hk_sim::DreamNail::new(),
        None,
        &mut NailResponse::new(),
        no_attack(),
        [if strike { STRIKE } else { &[[-ONE, -2 * ONE], [ONE, -2 * ONE], [ONE, ONE], [-ONE, ONE]] }; 4],
        cheats::Settings::new(),
        camera,
        |_| {},
        |scene, x, y, _| {
            rooms.iter().find(|(r, _)| r.scene == scene && world::contains(r.collision_bounds, x, y)).map(|(r, room)| (*r, *room))
        },
    )
}

/// `edges SCENE X0 Y0 X1 Y1`: the terrain edges of every region of the scene that touch the box.
pub fn edges(args: &[String]) {
    let scene: usize = args[0].parse().unwrap();
    let b: Vec<f64> = args[1..5].iter().map(|v| v.parse().unwrap()).collect();
    for here in load_scene(scene) {
        let room = here.room();
        println!("region {} bounds {:?} collision {:?} edges {}", here.global_id, here.bounds.map(|v| v / 65536), here.collision_bounds.map(|v| v / 65536), room.counts[5]);
        for i in 0..room.counts[5] {
            let e = room.edge(i).map(|v| v as f64 / 65536.0);
            let (lo_x, hi_x) = (e[0].min(e[2]), e[0].max(e[2]));
            let (lo_y, hi_y) = (e[1].min(e[3]), e[1].max(e[3]));
            if hi_x >= b[0] && lo_x <= b[2] && hi_y >= b[1] && lo_y <= b[3] {
                println!("   edge {i:4} ({:8.3},{:8.3}) -> ({:8.3},{:8.3})", e[0], e[1], e[2], e[3]);
            }
        }
    }
}

pub fn probe(args: &[String]) {
    let scene: usize = args[0].parse().expect("scene id");
    let ticks: usize = args.get(1).map_or(1800, |t| t.parse().unwrap());
    let regions = load_scene(scene);
    println!("scene {scene}: {} regions", regions.len());
    for here in &regions {
        if here.actors.is_empty() {
            continue;
        }
        let mut w = enemies::EnemyWorld::new();
        let mut player = Player::spawn(-150 * ONE, -150 * ONE);
        let mut vitals = Vitals::new(VITAL_PARAMS);
        let ids: Vec<u32> = here.actors.iter().map(|(p, _)| p.source_id).collect();
        let mut lo = vec![(i32::MAX, i32::MAX); ids.len()];
        let mut hi = vec![(i32::MIN, i32::MIN); ids.len()];
        let camera = match std::env::var("HKBP_CAMERA").ok() {
            Some(v) => { let c: Vec<f64> = v.split(',').map(|n| n.parse().unwrap()).collect(); [(c[0] * 65536.0) as i32, (c[1] * 65536.0) as i32, (c[2] * 65536.0) as i32] }
            None => [(here.bounds[0] + here.bounds[2]) / 2, (here.bounds[1] + here.bounds[3]) / 2, -38 * ONE],
        };
        let watch: Option<u32> = std::env::var("HKBP_WATCH").ok().and_then(|v| v.parse().ok());
        for t in 0..ticks {
            step(&mut w, here, &regions, &mut player, &mut vitals, camera);
            for (k, id) in ids.iter().enumerate() {
                if let Some((x, y, _)) = w.actor_state(scene, *id) {
                    if watch == Some(*id) && (t % 15 == 0 || t < 6) && world::contains(here.bounds, here.actors[k].0.x, here.actors[k].0.y) {
                        println!("  t={t:5} x={:9.4} y={:9.4}", x as f64 / 65536.0, y as f64 / 65536.0);
                    }
                    lo[k] = (lo[k].0.min(x), lo[k].1.min(y));
                    hi[k] = (hi[k].0.max(x), hi[k].1.max(y));
                }
            }
        }
        println!("region {} bounds {:?}", here.global_id, here.bounds.map(|v| v as f64 / 65536.0));
        for (k, (p, spec)) in here.actors.iter().enumerate() {
            if !world::contains(here.bounds, p.x, p.y) {
                continue;
            }
            let q = |v: i32| v as f64 / 65536.0;
            println!(
                "  src {:>6} rot {:3} sr {} dir {:2} start ({:8.3},{:8.3}) x [{:8.3},{:8.3}] y [{:8.3},{:8.3}] {:?}",
                p.source_id, p.rotation_q16 / 65536, p.start_right as u8, p.initial_direction, q(p.x), q(p.y), q(lo[k].0), q(hi[k].0), q(lo[k].1), q(hi[k].1),
                format!("{:?}", spec.controller).split(|c: char| !c.is_alphanumeric()).next().unwrap_or("")
            );
        }
    }
}
