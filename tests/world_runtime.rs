//! Host-only world integration tests; compile with the shared host rlibs.
extern crate hk_format;
extern crate hk_sim;
const KNIGHT_SCALE:i32=60693;
#[path="../game/src/impact.rs"]
mod impact;
mod render {
    pub const SOURCE_ALPHA_COVERAGE:u8=1;
    pub fn texture_material_alpha(_:usize,_:[(i16,i16);4],_:(u8,u8,u8),_:u8){}
    pub fn texture(_:usize,_:[(i16,i16);4],_:(u8,u8,u8)) {}
    use std::sync::Mutex;
    pub static GAINS: Mutex<Vec<(usize, u8)>> = Mutex::new(Vec::new());
    pub static TEST_LOCK: Mutex<()> = Mutex::new(());
    pub fn reset_visibility() {
        GAINS.lock().unwrap().clear();
    }
    pub fn set_visible(_: usize, _: bool) {}
    pub fn set_gain(draw: usize, gain: u8) {
        GAINS.lock().unwrap().push((draw, gain));
    }
    pub fn set_opacity(_: usize, _: u8) {}
}
#[path = "../game/src/reveal_masks.rs"]
mod reveal_masks;
#[path = "../game/src/world.rs"]
mod world;
use hk_sim::{AttackParams, Nail, Player};
use world::{Breakable, MaskFade, Region, State};
const PARAMS: AttackParams = AttackParams {
    duration: 21,
    cooldown: 25,
    alternate_reset: 30,
    hit_start: 2,
    hit_end: 6,
};
const TRIANGLE: &[[i32; 2]] = &[[100, 0], [200, 0], [100, 100]];
const SQUARE: &[[i32; 2]] = &[[100, 0], [200, 0], [200, 100], [100, 100]];
const FADES: &[MaskFade] = &[MaskFade {
    draws: &[5],
    ticks: 60,
}];
const B: Breakable = Breakable {
    id: 0,
    bounds: [100, 0, 200, 100],
    polygons: &[TRIANGLE],
    off: &[1],
    on: &[2],
    edges: &[1],
    fades: FADES,
    fade_ticks: 60,
    persistent: true,
    door_sound: true,
};
static TARGETS: &[Breakable] = &[
    B,
    Breakable {
        id: 1,
        polygons: &[SQUARE],
        ..B
    },
];
fn region() -> Region {
    Region {
        flat_floor_heights: &[],
        scene: 0,
        bounds: [0, 0, 1000, 1000],
        collision_bounds: [0, 0, 1000, 1000],
        camera: [0, 0, 1000, 1000],
        grass: &[],
        grass_impact:None,
    door_debris:&[],
    particle_bank:None,
    grass_emitters:&[],
        breakables: TARGETS,
        remote_masks: &[],
        reveal_masks: &[],
        reveal_bindings: &[],
        hazards: &[],
        actors: &[],
        checkpoints: &[],
        neighbours: &[],
    }
}
#[test]
fn exact_source_polygon_rejects_box_false_positive_and_breaks_once() {
    let mut state = State::new();
    let mut nail = Nail::new();
    nail.active = true;
    nail.age = 2;
    let player = Player::spawn(0, 0);
    let attack = &[[-185, 80], [-180, 80], [-180, 85], [-185, 85]][..];
    let hit = state.strike(&region(), &nail, PARAMS, [attack; 4], &player);
    assert_eq!((hit.broken, hit.door_sounds), (1, 1));
    assert!(!state.broken(0));
    assert!(state.broken(1));
    assert_eq!(
        state
            .strike(&region(), &nail, PARAMS, [attack; 4], &player)
            .broken,
        0
    );
    nail.age = 6;
    assert_eq!(
        state
            .strike(&region(), &nail, PARAMS, [SQUARE; 4], &player)
            .broken,
        0
    );
}
#[test]
fn source_fade_advances_once_without_restarting_on_repeat_hit() {
    let _lock = render::TEST_LOCK.lock().unwrap();
    let mut state = State::new();
    assert!(state.break_object(B));
    for _ in 0..30 {
        state.tick();
    }
    assert!(!state.break_object(B));
    assert_eq!(state.fade_left[0], 30);
    let mut r = region();
    r.breakables = &[B];
    state.apply(&r);
    assert_eq!(*render::GAINS.lock().unwrap(), vec![(5, 64)]);
    for _ in 0..30 {
        state.tick();
    }
    state.apply(&r);
    assert_eq!(*render::GAINS.lock().unwrap(), vec![(5, 0)]);
}
#[test]
fn actual_generated_persistence_survives_scene_reset_and_grass_is_scene_local() {
    let persistent = world::REGIONS
        .iter()
        .flat_map(|r| r.breakables)
        .find(|b| b.persistent)
        .unwrap();
    let temporary = world::REGIONS
        .iter()
        .flat_map(|r| r.breakables)
        .find(|b| !b.persistent && b.id / 128 == persistent.id / 128)
        .unwrap();
    let scene = persistent.id / 128;
    let mut state = State::new();
    state.break_object(*persistent);
    state.break_object(*temporary);
    state.cut_grass(scene * 1024 + 1023);
    state.cut_grass((1 - scene) * 1024 + 1023);
    state.reset_scene(scene);
    assert!(state.broken(persistent.id));
    assert!(!state.broken(temporary.id));
    assert!(!state.cut(scene * 1024 + 1023));
    assert!(state.cut((1 - scene) * 1024 + 1023));
}
#[test]
fn only_owned_edges_disappear() {
    let mut bytes = vec![0u8; 40 + 16 + 2 * 16 + 32 + 32768];
    bytes[..8].copy_from_slice(b"HKROOM02");
    bytes[8] = 1;
    bytes[12] = 1;
    bytes[28] = 2;
    bytes[46] = 1;
    bytes[48] = 1;
    for (i, e) in [[1i32, 2, 3, 2], [4, 5, 4, 6]].iter().enumerate() {
        for (j, v) in e.iter().enumerate() {
            let p = 56 + i * 16 + j * 4;
            bytes[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    let room = hk_format::Room::parse(&bytes).unwrap();
    let mut state = State::new();
    state.break_object(B);
    assert_eq!(state.edge(&region(), &room, 0), [1, 2, 3, 2]);
    assert_eq!(state.edge(&region(), &room, 1), [0; 4]);
}

#[test]
fn hazards_and_checkpoints_use_polygons_and_preserve_distant_spawn() {
    const P: hk_sim::Params = hk_sim::Params {
        speed: 0,
        jump: 0,
        gravity: 0,
        fall: 0,
        hold_ticks: 0,
        min_ticks: 0,
        half_width: 2,
        bottom: -2,
        top: 2,
    };
    static H: &[world::Hazard] = &[world::Hazard {
        bounds: [100, 0, 200, 100],
        polygons: &[TRIANGLE],
        origin_x: 150,
        damage: 1,
        hazard: true,
    }];
    static C: &[world::Checkpoint] = &[world::Checkpoint {
        bounds: [100, 0, 200, 100],
        polygons: &[TRIANGLE],
        spawn: [9000, 3000],
        facing: -1,
    }];
    let mut r = region();
    r.hazards = H;
    r.checkpoints = C;
    let missed = Player::spawn(185, 85);
    assert_eq!(world::hazard_contact(&r, &missed, P), None);
    assert_eq!(world::checkpoint(&r, &missed, P), None);
    let hit = Player::spawn(115, 20);
    assert_eq!(world::hazard_contact(&r, &hit, P), Some((1, true, -1)));
    assert_eq!(world::checkpoint(&r, &hit, P), Some((9000, 3000)));
    assert_eq!(world::checkpoint_record(&r, &hit, P).unwrap().facing, -1);
}

fn movement_params() -> hk_sim::Params {
    hk_sim::Params {
        speed: 8 * hk_sim::ONE,
        jump: 16 * hk_sim::ONE,
        gravity: 48 * hk_sim::ONE,
        fall: 20 * hk_sim::ONE,
        hold_ticks: 12,
        min_ticks: 5,
        half_width: hk_sim::ONE / 4,
        bottom: -hk_sim::ONE,
        top: 0,
    }
}
fn room_with_edges(edges: &[[i32; 4]]) -> Vec<u8> {
    let mut bytes = vec![0u8; 40 + 16 + edges.len() * 16 + 32 + 32768];
    bytes[..8].copy_from_slice(b"HKROOM02");
    bytes[8] = 1;
    bytes[12] = 1;
    bytes[28..32].copy_from_slice(&(edges.len() as u32).to_le_bytes());
    bytes[46] = 1;
    bytes[48] = 1;
    for (i, edge) in edges.iter().enumerate() {
        for (j, value) in edge.iter().enumerate() {
            let p = 56 + i * 16 + j * 4;
            bytes[p..p + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}
#[test]
fn vertical_prefetch_rejects_solid_floor_and_allows_real_downward_exit() {
    use hk_sim::ONE;
    let bounds = [-5 * ONE, 0, 5 * ONE, 10 * ONE];
    let mut player = Player::spawn(0, 2 * ONE);
    player.vy = -4 * ONE;
    let before = player;
    let floor = [-5 * ONE, ONE, 5 * ONE, ONE];
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 1, |_| floor),
        None
    );
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 0, |_| [0; 4]),
        Some(-ONE / 16)
    );
    assert_eq!(player, before, "look-ahead must not modify gameplay");
}
#[test]
fn vertical_prefetch_rejects_ceiling_and_short_jump_apex() {
    use hk_sim::ONE;
    let bounds = [-5 * ONE, 0, 5 * ONE, 10 * ONE];
    let mut player = Player::spawn(0, 9 * ONE);
    player.vy = 12 * ONE;
    let ceiling = [-5 * ONE, 9 * ONE + ONE / 2, 5 * ONE, 9 * ONE + ONE / 2];
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 1, |_| ceiling),
        None
    );
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 0, |_| [0; 4]),
        Some(10 * ONE + ONE / 16)
    );
    player.y = 8 * ONE;
    player.vy = 6 * ONE;
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 0, |_| [0; 4]),
        None
    );
}
#[test]
fn vertical_prefetch_respects_removed_owned_floor_and_thirty_tick_bound() {
    use hk_sim::ONE;
    let bounds = [-5 * ONE, 0, 5 * ONE, 10 * ONE];
    let mut player = Player::spawn(0, 2 * ONE);
    player.vy = -4 * ONE;
    let bytes = room_with_edges(&[
        [4 * ONE, 0, 4 * ONE, 4 * ONE],
        [-5 * ONE, ONE, 5 * ONE, ONE],
    ]);
    let room = hk_format::Room::parse(&bytes).unwrap();
    let mut state = State::new();
    let r = region();
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 2, |i| state
            .edge(&r, &room, i)),
        None
    );
    state.break_object(B);
    assert_eq!(
        world::vertical_exit(bounds, &player, movement_params(), 2, |i| state
            .edge(&r, &room, i)),
        Some(-ONE / 16)
    );
    let mut params = movement_params();
    params.gravity = 0;
    player.vy = -ONE;
    let calls = std::cell::Cell::new(0);
    assert_eq!(
        world::vertical_exit(bounds, &player, params, 1, |_| {
            calls.set(calls.get() + 1);
            [0; 4]
        }),
        None
    );
    assert_eq!(
        calls.get(),
        60,
        "two terrain sweeps per step, at most thirty steps"
    );
}
#[test]
fn generated_region_prefetch_falls_back_horizontally_when_floor_blocks_drop() {
    use hk_sim::ONE;
    let candidate =
        world::REGIONS
            .iter()
            .enumerate()
            .find_map(|(index, r)| {
                let x = r.bounds[0] + (r.bounds[2] - r.bounds[0]) / 2;
                let y = r.bounds[1] + 2 * ONE;
                let horizontal = r.neighbours.iter().copied().find(|&n| {
                    world::contains(world::REGIONS[n].bounds, r.bounds[2] + ONE / 16, y)
                });
                let downward = r.neighbours.iter().copied().find(|&n| {
                    world::contains(world::REGIONS[n].bounds, x, r.bounds[1] - ONE / 16)
                });
                match (horizontal, downward) {
                    (Some(h), Some(d)) if h != d => Some((index, x, y, h, d)),
                    _ => None,
                }
            })
            .expect("generated room table must contain horizontal/downward alternatives");
    let (index, x, y, horizontal, downward) = candidate;
    let r = &world::REGIONS[index];
    let mut player = Player::spawn(x, y);
    player.vy = -4 * ONE;
    let bytes = room_with_edges(&[[
        r.bounds[0] - ONE,
        r.bounds[1] + ONE,
        r.bounds[2] + ONE,
        r.bounds[1] + ONE,
    ]]);
    let room = hk_format::Room::parse(&bytes).unwrap();
    let state = State::new();
    assert_eq!(
        world::upcoming(index, &player, &room, &state, movement_params()),
        Some(horizontal)
    );
    let bytes = room_with_edges(&[]);
    let empty = hk_format::Room::parse(&bytes).unwrap();
    assert_eq!(
        world::upcoming(index, &player, &empty, &state, movement_params()),
        Some(downward)
    );
}

#[test]
fn door1_mask_survives_region_entry_outside_its_collider() {
    let _lock = render::TEST_LOCK.lock().unwrap();
    // Original level6:12793 state38 owns FSM12055 -> renderer11301.
    // Region14 includes the mask but not Door1's x59.59 collision/attack shape.
    let owner = world::REGIONS.iter().flat_map(|r| r.breakables)
        .find(|b| b.id == 38 && b.fade_ticks == 60).unwrap();
    let distant = world::REGIONS.iter().find(|r| r.scene == 0 &&
        world::contains(r.bounds, 84 * hk_sim::ONE + hk_sim::ONE/32, 11 * hk_sim::ONE + hk_sim::ONE/2))
        .unwrap();
    assert!(!distant.breakables.iter().any(|b| b.id == owner.id));
    let bindings: Vec<_> = distant.remote_masks.iter().filter(|m| m.owner == owner.id).collect();
    assert!(!bindings.is_empty());
    let mut state = State::new();
    state.break_object(*owner);
    for _ in 0..30 { state.tick(); }
    for binding in &bindings {
        // Isolate a single application to avoid sharing the rendering stub
        // with unrelated fade tests run by the native parallel test harness.
        assert_eq!(state.fade_left[binding.owner],30);
    }
    state.apply(distant);
    let gains = render::GAINS.lock().unwrap().clone();
    for binding in &bindings { for draw in binding.fade.draws {
        assert!(gains.contains(&(*draw as usize,64)));
    }}
    for _ in 0..30 { state.tick(); }
    state.apply(distant);
    let gains = render::GAINS.lock().unwrap().clone();
    for binding in &bindings { for draw in binding.fade.draws {
        assert!(gains.contains(&(*draw as usize,0)));
    }}
    assert!(state.broken(owner.id));
}

#[test]
fn actual_crawler_spawn_resolves_against_complete_cooked_terrain() {
    use hk_sim::{resolve_actor_spawn, Params, ONE};
    for source_id in [12546,12548] {
        let (index,region,spec)=world::REGIONS.iter().enumerate().find_map(|(index,r)| {
            r.actors.iter().find(|s| s.source_id==source_id &&
                world::contains(r.bounds,s.x,s.y)).map(|s|(index,r,s))
        }).expect("authored Tutorial crawler activation region");
        let path=format!("{}/../data/regions/chunk_{}.hk",env!("CARGO_MANIFEST_DIR"),index+1);
        let bytes=std::fs::read(path).unwrap();let room=hk_format::Room::parse(&bytes).unwrap();
        let state=State::new();let b=spec.bounds;
        let p=Params {speed:spec.walk.speed,jump:0,gravity:60*ONE,fall:100*ONE,
            hold_ticks:0,min_ticks:0,half_width:(b[2]-b[0])/2,bottom:b[1],top:b[3]};
        let mut body=Player::spawn(spec.x+b[0]+p.half_width,spec.y);
        assert!(resolve_actor_spawn(&mut body,p,room.counts[5],|i|state.edge(region,&room,i)));
        assert_eq!(body.y,if source_id==12548 {331776} else {spec.y});
        for _ in 0..60 {body.step(p,0,false,room.counts[5],|i|state.edge(region,&room,i));}
        assert!(body.grounded);
        if source_id==12548 {assert_eq!(body.y,331776);}
    }
}

#[test]
fn authored_inverse_remask_bindings_preserve_one_controller_across_regions() {
    use hk_sim::{Params, ONE};
    let first=world::REGIONS.iter().find(|r|r.scene==0).unwrap();
    let index=first.reveal_masks.iter().position(|s|s.source_id==12038).expect("source inverse_remask_right FSM");
    let spec=first.reveal_masks[index];
    assert_eq!(spec.fade_ticks,30);
    assert_eq!(spec.initial_opacity,0);
    assert_eq!(spec.trigger,&[[13202367,2153911],[10462305,2153911],[10462305,822671],[13202367,822671]]);
    let p=Params {speed:0,jump:0,gravity:0,fall:0,hold_ticks:0,min_ticks:0,half_width:ONE/4,bottom:-ONE,top:0};
    let inside=Player::spawn(180*ONE,20*ONE);
    let outside=Player::spawn(154*ONE,20*ONE);
    let mut controller=reveal_masks::State::new();controller.scene_ready(0,first.reveal_masks);
    for _ in 0..30 {controller.tick(first.reveal_masks,&outside,p);}
    assert_eq!(controller.opacity(index),0);
    for _ in 0..15 {controller.tick(first.reveal_masks,&inside,p);}
    assert_eq!(controller.opacity(index),64);
    let mut binding_count=0;
    for (id,region) in world::REGIONS.iter().enumerate().filter(|(_,r)|r.scene==0) {
        assert_eq!(region.reveal_masks.len(),first.reveal_masks.len());
        for (a,b) in region.reveal_masks.iter().zip(first.reveal_masks) {
            assert_eq!(a.source_id,b.source_id);assert_eq!(a.trigger,b.trigger);assert_eq!(a.fade_ticks,b.fade_ticks);
        }
        controller.scene_ready(0,region.reveal_masks);assert_eq!(controller.opacity(index),64);
        let path=format!("{}/../data/regions/chunk_{}.hk",env!("CARGO_MANIFEST_DIR"),id+1);
        let bytes=std::fs::read(path).unwrap();let room=hk_format::Room::parse(&bytes).unwrap();
        for binding in region.reveal_bindings {
            assert!((binding.controller as usize)<region.reveal_masks.len());
            assert!((binding.draw as usize)<room.counts[2]);
            if binding.controller as usize==index {binding_count+=1;}
        }
    }
    assert!(binding_count>1,"revealed source sprite spans several residency regions");
    for _ in 0..30 {controller.tick(first.reveal_masks,&outside,p);}
    assert_eq!(controller.opacity(index),0);
}
