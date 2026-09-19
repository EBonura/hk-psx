//! Native compile/execution of the guest enemy module with hardware-only drawing
//! stubbed. Fixtures exercise gameplay rather than mirroring GPU internals.
use hk_sim::*;
// cheats composes the equipped charms into the live parameters and reconciles
// the all-charms grant. The real module links the cooked catalogue this harness
// has no manifest directory for, and an empty board composes to the base
// anyway, so the contract is stubbed here; tests/charms_runtime.rs exercises
// the real one.
mod charms{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
 pub fn grant_all(_:bool){}
 pub fn on_damage(_:&mut hk_sim::Vitals,_:hk_sim::VitalParams){}
}
#[path="../../../game/src/cheats.rs"]mod cheats;
const NAIL_RESPONSE_PARAMS: NailResponseParams = NailResponseParams {
    recoil_ticks: 11,
    recoil_speed: 245760,
    bounce_ticks: 15,
    bounce_speed: 786432,
    down_speed: 0,
};
const KNIGHT_SCALE: i32 = 60693;
const DREAM_NAIL_POLYGON: &[[i32;2]] = &[[0,0],[0,0],[0,0]];
const PARAMS: Params = Params { speed: 8 * ONE, jump: 16 * ONE, gravity: 48 * ONE, fall: 20 * ONE, hold_ticks: 12, min_ticks: 5, half_width: ONE / 4, bottom: -ONE, ..Params::ZERO };
const VITAL_PARAMS: VitalParams = VitalParams {
    max_health: 5,
    max_soul: 99,
    nail_damage: 5,
    soul_per_hit: 11,
    invulnerable_ticks: 79,
    hazard_invulnerable_ticks: 40,
    recoil_ticks: 12,
    freeze_ticks: 19,
    death_ticks: 171,
    recoil_speed: 15 * ONE,
};
mod render {
    std::thread_local! {
        /// Submitted quads, so a frame drawn as several tiles can be checked
        /// tile by tile instead of only counted.
        pub static QUADS: std::cell::RefCell<Vec<(usize, [(i16, i16); 4])>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn texture(id: usize, verts: [(i16, i16); 4], _: (u8, u8, u8)) {
        QUADS.with(|q| q.borrow_mut().push((id, verts)));
    }
}
mod world {
    #[derive(Clone, Copy)]
    pub struct Region {
        pub scene: usize,
        pub bounds: [i32; 4],
        pub collision_bounds: [i32; 4],
        pub actors: &'static [&'static hk_sim::ActorSpec],
    }
    pub fn region_actors(region: &Region) -> impl Iterator<Item = &'static hk_sim::ActorSpec> {
        region.actors.iter().copied()
    }
    /// One fixture spike: a down-slash-only target owned by breakable 0.
    pub struct PogoTarget;
    impl PogoTarget {
        pub fn bounds(&self) -> [i32; 4] { [-hk_sim::ONE, -hk_sim::ONE, hk_sim::ONE, 0] }
        pub fn breakable(&self) -> Option<usize> { Some(0) }
        pub fn horizontal_and_up(&self) -> bool { false }
        pub fn hit_by(&self, polygon: &[[i32; 2]]) -> bool {
            let one = hk_sim::ONE;
            polygons_overlap(polygon, &[[-one, -one], [one, -one], [one, 0], [-one, 0]])
        }
    }
    pub fn pogo_targets(_: &Region) -> impl Iterator<Item = PogoTarget> { core::iter::once(PogoTarget) }
    pub mod debris {
        /// Cardinal-only rotation for the enemy draw test; the guest uses the debris sine table.
        pub fn rotate(p: [i32; 2], angle: i32) -> [i32; 2] {
            match (angle / (90 * hk_sim::ONE)).rem_euclid(4) { 0 => p, 1 => [-p[1], p[0]], 2 => [-p[0], -p[1]], _ => [p[1], -p[0]] }
        }
    }
    pub struct State;
    std::thread_local!{pub static DEATHS:std::cell::RefCell<Vec<(u32,[i32;3])>>=const{std::cell::RefCell::new(Vec::new())};}
    impl State {
        pub fn enemy_death_particles(&mut self,_:&Region,source:u32,position:[i32;3]) {DEATHS.with(|v|v.borrow_mut().push((source,position)));}
        pub fn broken(&self, _: usize) -> bool {
            false
        }
    }
    pub fn polygons_overlap(a: &[[i32; 2]], b: &[[i32; 2]]) -> bool {
        let bounds = [
            b.iter().map(|p| p[0]).min().unwrap(),
            b.iter().map(|p| p[1]).min().unwrap(),
            b.iter().map(|p| p[0]).max().unwrap(),
            b.iter().map(|p| p[1]).max().unwrap(),
        ];
        hk_sim::polygon_hits_box(a, bounds)
    }
    impl State {
        pub fn fill_edges_in_view(&self, _: &Region, _: &hk_format::Room, _: &Region, r: &hk_format::Room, edges:&mut [[i32;4]]) {
            for(i,edge)in edges.iter_mut().enumerate(){*edge=r.edge(i);}
        }
        pub fn edge_in_view(&self, _: &Region, _: &hk_format::Room, _: &Region, r: &hk_format::Room, i: usize) -> [i32; 4] {
            r.edge(i)
        }
        pub fn edge(&self, _: &Region, r: &hk_format::Room, i: usize) -> [i32; 4] {
            r.edge(i)
        }
    }
    pub fn contains(b: [i32; 4], x: i32, y: i32) -> bool {
        x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]
    }
}
/// The save record's PlayerData reserve, which the arena's `Activated` bool and
/// `falseKnightFirstPlop` ride. The real module links the cooked script bank
/// this harness has no manifest directory for; what the boss needs from it is
/// two plain slots, and a per-thread store keeps the parallel cases apart.
mod script {
    pub const FIELD_FALSE_KNIGHT_ARENA: usize = 15;
    pub const FIELD_FALSE_KNIGHT_FIRST_PLOP: usize = 14;
    std::thread_local! {
        pub static STORE: std::cell::RefCell<[i32; 16]> = const { std::cell::RefCell::new([0; 16]) };
    }
    pub fn player_data(slot: usize) -> i32 { STORE.with(|s| s.borrow()[slot]) }
    pub fn set_player_data(slot: usize, value: i32) { STORE.with(|s| s.borrow_mut()[slot] = value) }
}
/// The arena gates. The real module links the cooked `(slot, gate, edges)`
/// table, which `tests/battle_gates_runtime.rs` drives directly; what this
/// harness has to see is that every broadcast leaves the boss with the room it
/// names, so they are recorded in order.
mod battle_gates {
    std::thread_local! {
        pub static EVENTS: std::cell::RefCell<Vec<(&'static str, usize)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    fn record(event: &'static str, scene: usize) { EVENTS.with(|e| e.borrow_mut().push((event, scene))); }
    pub fn arena_entry(scene: usize, activated: bool) {
        record(if activated { "quick open" } else { "placement" }, scene);
    }
    pub fn close(scene: usize) { record("close", scene); }
    pub fn open(scene: usize) { record("open", scene); }
}
/// The camera and the SPU, which the boss reaches through two global modules
/// the real guest keeps in `static mut`. A shared static cannot be asserted
/// against while the other cases in this file run beside it, so both record
/// into one per-thread log here. One log rather than two, because the order is
/// the interesting part: `S Attack` swings and then `Slam` shakes, where the
/// jump attack's `JA Slam` shakes and then `JA Hit 2` swings.
mod boss_effects {
    std::thread_local! {
        pub static LOG: std::cell::RefCell<Vec<&'static str>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn record(what: &'static str) { LOG.with(|l| l.borrow_mut().push(what)); }
    pub fn take() -> Vec<&'static str> { LOG.with(|l| l.borrow_mut().drain(..).collect()) }
}
mod camera {
    /// `game/src/camera.rs` declares these; `tests/camera_shake_runtime.rs`
    /// drives the real module over the serialized `CameraShake` table.
    #[allow(dead_code)]
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Shake { Small, Kill, Average, Big }
    pub fn request(kind: Shake) {
        crate::boss_effects::record(match kind {
            Shake::Small => "shake small",
            Shake::Kill => "shake kill",
            Shake::Average => "shake average",
            Shake::Big => "shake big",
        });
    }
}
mod audio {
    pub fn boss_land() { crate::boss_effects::record("land voice"); }
    pub fn boss_swing() { crate::boss_effects::record("swing voice"); }
}
#[path = "../../../game/src/enemies.rs"]
mod enemies;
const SPEC: ActorSpec = ActorSpec {
    controller: ActorController::Crawler,
    source_id: 12546,
    x: 0,
    y: ONE,
    bounds: [-ONE / 2, -ONE, ONE / 2, 0],
    health: EnemyParams {
        health: 8,
        contact_damage: 1,
        evasion_ticks: 12,
        invincible: false,
        damage_override: false,
    },
    walk: WalkParams {
        speed: 4 * ONE,
        turn_ticks: 10,
        turn_cooldown_ticks: 60,
    },
    initial_direction: -1,
    random_start_direction: false,
    walk_clip: 0,
    turn_clip: 0,
    corpse: None,
    recoil_speed: 15 * ONE,
    recoil_ticks: 10, dream_soul: 0
};
fn region() -> world::Region {
    world::Region {
        scene: 0,
        bounds: [-20 * ONE, -5 * ONE, 20 * ONE, 20 * ONE],
        collision_bounds: [-23 * ONE, -10 * ONE, 23 * ONE, 25 * ONE],
        actors: &[&SPEC],
    }
}
fn room() -> Vec<u8> {
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 1, 0, 1, 1, 1, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u16, 0, 0, 4, 4, 0] {
        b.extend(n.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    for n in [0i32, -ONE / 2, -ONE, ONE / 2, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u32, 1, 12 * 65536, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [-20 * ONE, 0, 20 * ONE, 0] {
        b.extend(n.to_le_bytes());
    }
    b.resize(b.len() + 32 + 32768, 0);
    b
}
/// The same actor frame spread over a 2x2 rectangle of 64x64 animation slots,
/// sized like Crossroads_47's Stag: 91x89 pixels is 64+27 across by 64+25 down.
/// Its world box is the one `room()` gives the single-slot frame, so the two
/// fixtures have to project to the same rectangle.
fn tiled_room() -> Vec<u8> {
    let tiles = [(64u16, 64u16), (27, 64), (64, 25), (27, 25)];
    let mut offsets = Vec::new();
    let mut stream = 0usize;
    for &(w, h) in tiles.iter() {
        offsets.push(stream);
        // Atlas.pack pads each streamed row to a word and each texture to four.
        stream += ((((w as usize + 3) & !3) / 2) * h as usize).next_multiple_of(4);
    }
    let mut b = Vec::from(*b"HKROOM02");
    for n in [0u32, tiles.len() as u32, 0, 1, 1, 1, stream as u32, 0] {
        b.extend(n.to_le_bytes());
    }
    for (i, &(w, h)) in tiles.iter().enumerate() {
        // Only the frame's first tile carries the grid, in the two fields a
        // streamed texture never uses for a VRAM origin.
        let (u, v) = if i == 0 { (2u16, 2) } else { (0, 0) };
        for n in [hk_format::STREAMED_PAGE, u, v, w, h, i as u16] {
            b.extend(n.to_le_bytes());
        }
        b.extend((offsets[i] as u32).to_le_bytes());
    }
    for n in [0i32, -ONE / 2, -ONE, ONE / 2, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u32, 1, 12 * 65536, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [-20 * ONE, 0, 20 * ONE, 0] {
        b.extend(n.to_le_bytes());
    }
    b.resize(b.len() + 32 * tiles.len() + stream, 0);
    b
}
fn submitted(w: &enemies::EnemyWorld, r: &world::Region, room: &hk_format::Room)
    -> Vec<(usize, [(i16, i16); 4])> {
    render::QUADS.with(|q| q.borrow_mut().clear());
    w.prepare_draws(r, room, (0, 0)).draw();
    render::QUADS.with(|q| q.borrow().clone())
}
fn span(quad: &[(i16, i16); 4]) -> (i16, i16, i16, i16) {
    (
        quad.iter().map(|v| v.0).min().unwrap(),
        quad.iter().map(|v| v.1).min().unwrap(),
        quad.iter().map(|v| v.0).max().unwrap(),
        quad.iter().map(|v| v.1).max().unwrap(),
    )
}
#[test]
fn an_actor_frame_past_one_slot_draws_and_keys_every_tile() {
    let plain = room();
    let tiled = tiled_room();
    let one = hk_format::Room::parse(&plain).unwrap();
    let many = hk_format::Room::parse(&tiled).unwrap();
    let r = region();
    let mut w = enemies::EnemyWorld::new();
    w.sync_region(&r);
    // Every tile is its own animation key, in the consecutive order the cooker
    // emits, so the cache is asked for all four rather than the first.
    let mut needed = [0u16; 8];
    let mut len = 0;
    w.prepare_draws(&r, &many, (0, 0)).append_needed(&mut needed, &mut len);
    assert_eq!(len, 4);
    assert_eq!(&needed[..4], &[0, 1, 2, 3]);
    let whole = submitted(&w, &r, &one);
    let tiles = submitted(&w, &r, &many);
    assert_eq!(whole.len(), 1);
    assert_eq!(tiles.len(), 4);
    assert_eq!(tiles.iter().map(|t| t.0).collect::<Vec<_>>(), [0, 1, 2, 3]);
    let (x0, y0, x1, y1) = span(&whole[0].1);
    let spans: Vec<_> = tiles.iter().map(|t| span(&t.1)).collect();
    // The union is the quad the single-slot frame draws: a tiled actor covers
    // the same rectangle, not only its top-left corner.
    assert_eq!(spans.iter().map(|s| s.0).min(), Some(x0));
    assert_eq!(spans.iter().map(|s| s.1).min(), Some(y0));
    assert_eq!(spans.iter().map(|s| s.2).max(), Some(x1));
    assert_eq!(spans.iter().map(|s| s.3).max(), Some(y1));
    // Neighbours meet on one coordinate, so no seam gaps or overdraws. Art row
    // 0 is the top of the box, and the atlas faces left, so tile 0 is the
    // screen's top-left for this actor's facing.
    assert_eq!(spans[0].2, spans[1].0);
    assert_eq!(spans[2].2, spans[3].0);
    assert_eq!(spans[0].3, spans[2].1);
    assert_eq!(spans[1].3, spans[3].1);
    // The split follows the texels, 64 of 91 across and 64 of 89 down, to
    // within the pixel the projection rounds: this fixture's box is only about
    // fourteen screen pixels wide, so the cut lands on a rounding boundary.
    // hk-format's own tests hold the world-box split exactly.
    assert!((spans[0].2 - x0 - (x1 - x0) * 64 / 91).abs() <= 1);
    assert!((spans[0].3 - y0 - (y1 - y0) * 64 / 89).abs() <= 1);
    for s in spans.iter() {
        assert!(s.2 > s.0 && s.3 > s.1, "degenerate tile quad {s:?}");
    }
}
#[test]
fn a_tile_outside_the_view_costs_neither_a_quad_nor_a_key() {
    let tiled = tiled_room();
    let many = hk_format::Room::parse(&tiled).unwrap();
    let r = region();
    let mut w = enemies::EnemyWorld::new();
    w.sync_region(&r);
    let whole = w.prepare_draws(&r, &many, (0, 0));
    let mut needed = [0u16; 8];
    let mut len = 0;
    whole.append_needed(&mut needed, &mut len);
    assert_eq!(len, 4);
    // Far enough that the actor leaves the view entirely: no tile survives, so
    // a distant tiled actor holds none of the twenty slots.
    let gone = w.prepare_draws(&r, &many, (300 * ONE, 0));
    let mut needed = [0u16; 8];
    let mut len = 0;
    gone.append_needed(&mut needed, &mut len);
    assert_eq!(len, 0);
    assert_eq!(gone.draw(), 0);
}
fn no_attack() -> AttackParams {
    AttackParams {
        duration: 20,
        cooldown: 24,
        alternate_reset: 30,
        hit_start: 1,
        hit_end: 8, ..AttackParams::ZERO }
}
fn tick(
    w: &mut enemies::EnemyWorld,
    r: &world::Region,
    room: &hk_format::Room,
    p: &mut Player,
    v: &mut Vitals,
    n: &Nail,
) -> enemies::Events {
    w.tick(
        r,
        room,
        &mut world::State,
        p,
        v,
        n,
        &hk_sim::DreamNail::new(),
        None,
        &mut NailResponse::new(),
        no_attack(),
        [&[[-ONE, -2 * ONE], [ONE, -2 * ONE], [ONE, ONE], [-ONE, ONE]]; 4],
        cheats::Settings::new(),
        [0,0,-2496922],
        |_| panic!("Crawler emitted Runner event"),
        |_,_,_|None,
    )
}
#[test]
fn grid_swap_keeps_position_and_dead_health_until_scene_reset() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    tick(&mut w, &r, &room, &mut p, &mut v, &n);
    let moved = w.actor_state(0, 12546).unwrap();
    assert!(moved.0 < 0);
    let empty = world::Region {
        scene: 0,
        bounds: r.bounds,
        collision_bounds: r.collision_bounds,
        actors: &[],
    };
    w.sync_region(&empty);
    w.sync_region(&r);
    assert_eq!(w.actor_state(0, 12546).unwrap(), moved);
    p.x = moved.0;
    p.y = ONE;
    n.active = true;
    n.age = 1;
    assert_eq!(tick(&mut w, &r, &room, &mut p, &mut v, &n).hits, 1);
    w.take_geo_deaths(0,|_,_,_|panic!("Living actor paid Geo"));
    n.active = false;
    for _ in 0..12 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    p.x = w.actor_state(0, 12546).unwrap().0;
    n.active = true;
    assert_eq!(tick(&mut w, &r, &room, &mut p, &mut v, &n).kills, 1);
    assert_eq!(v.soul, 22);
    let mut payouts=Vec::new();
    w.take_geo_deaths(1,|_,_,_|panic!("Wrong scene paid Geo"));
    w.take_geo_deaths(0,|_,_,_|false); // A full emitter must remain retryable.
    w.take_geo_deaths(0,|id,x,y|{payouts.push((id,x,y));true});
    assert_eq!(payouts.len(),1);assert_eq!(payouts[0].0,SPEC.source_id);
    w.sync_region(&r);
    w.take_geo_deaths(0,|_,_,_|panic!("Grid swap paid the same death twice"));
    assert!(w.actor_state(0, 12546).unwrap().2 <= 0);
    w.reset_scene(0);
    w.sync_region(&r);
    assert_eq!(w.actor_state(0, 12546), Some((SPEC.x, SPEC.y, 8)));
    w.take_geo_deaths(0,|_,_,_|panic!("New living actor paid Geo"));
}
#[test]
fn contact_is_debounced_and_unloaded_collision_freezes_position() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    assert_eq!(
        tick(&mut w, &r, &room, &mut p, &mut v, &n).hurt,
        Hurt::Recoiling
    );
    assert_eq!(
        tick(&mut w, &r, &room, &mut p, &mut v, &n).hurt,
        Hurt::Ignored
    );
    assert_eq!(v.health, 4);
    let before = w.actor_state(0, 12546);
    let remote = world::Region {
        scene: 0,
        bounds: [100 * ONE, 0, 120 * ONE, 20 * ONE],
        collision_bounds: [97 * ONE, -5 * ONE, 123 * ONE, 25 * ONE],
        actors: &[&SPEC],
    };
    for _ in 0..60 {
        tick(&mut w, &remote, &room, &mut p, &mut v, &n);
    }
    assert_eq!(w.actor_state(0, 12546), before);
}
#[test]
fn visible_working_set_culls_remote_actor_without_fixed_point_overflow() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = region();
    let mut w = enemies::EnemyWorld::new();
    w.sync_region(&r);
    let mut needed = [0u16; 8];
    let mut len = 1;
    let batch = w.prepare_draws(&r, &room, (0, 0));
    batch.append_needed(&mut needed, &mut len);
    assert_eq!(len, 1);
    assert_eq!(batch.draw(), 1);
    let remote = w.prepare_draws(&r, &room, (300 * ONE, 0));
    assert_eq!(remote.draw(), 0);
}

#[test]
fn static_spike_accepts_downslash_only_with_active_source_polygon() {
    let r = region();
    let mut p = Player::spawn(0, ONE);
    let mut n = Nail::new();
    let mut response = NailResponse::new();
    let poly: &[[i32; 2]] = &[[-ONE, -2 * ONE], [ONE, -2 * ONE], [ONE, 0], [-ONE, 0]];
    n.active = true;
    n.age = 1;
    n.kind = 0;
    assert!(!enemies::pogo_contact(
        &r,
        &mut world::State,
        &mut p,
        &n,
        no_attack(),
        [poly; 4],
        &mut response
    ));
    n.kind = 3;
    assert!(enemies::pogo_contact(
        &r,
        &mut world::State,
        &mut p,
        &n,
        no_attack(),
        [poly; 4],
        &mut response
    ));
    assert_eq!(response.bounce_left, 15);
    n.age = 9;
    assert!(!enemies::pogo_contact(
        &r,
        &mut world::State,
        &mut p,
        &n,
        no_attack(),
        [poly; 4],
        &mut response
    ));
}

#[test]
fn actor_outside_activation_grid_still_moves_and_contacts_in_collision_apron() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let mut r = region();
    r.bounds = [ONE, -5 * ONE, 20 * ONE, 20 * ONE];
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    assert_eq!(
        tick(&mut w, &r, &room, &mut p, &mut v, &Nail::new()).hurt,
        Hurt::Recoiling
    );
    assert!(w.actor_state(0, 12546).unwrap().0 < 0);
    assert_eq!(v.health, 4);
}

#[test]
fn spawn_overlap_waits_for_resident_terrain_and_never_repeats_on_grid_swap() {
    const OVERLAPPING: ActorSpec = ActorSpec {
        y: ONE - ONE / 8,
        dream_soul: 0, ..SPEC
    };
    let bytes = room();
    let floor = hk_format::Room::parse(&bytes).unwrap();
    let mut r = region();
    r.actors = &[&OVERLAPPING];
    let remote = world::Region {
        collision_bounds: [97 * ONE, -5 * ONE, 123 * ONE, 25 * ONE],
        ..r
    };
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    tick(&mut w, &remote, &floor, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, SPEC.source_id).unwrap().1, OVERLAPPING.y);
    tick(&mut w, &r, &floor, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, SPEC.source_id).unwrap().1, ONE);
    // A new grid's intersecting edge must not trigger the initial placement
    // correction again. Ordinary movement retains the existing sweep rules.
    let mut changed = room();
    let edge_offset = 40 + 16 + 20 + 16;
    for offset in [4, 12] {
        changed[edge_offset + offset..edge_offset + offset + 4]
            .copy_from_slice(&(ONE / 4).to_le_bytes());
    }
    let changed = hk_format::Room::parse(&changed).unwrap();
    w.sync_region(&remote);
    w.sync_region(&r);
    tick(&mut w, &r, &changed, &mut p, &mut v, &n);
    assert!(w.actor_state(0, SPEC.source_id).unwrap().1 < ONE);
}

#[test]
fn unresolved_initial_geometry_suspends_without_retry_until_scene_reset() {
    const EMBEDDED: ActorSpec = ActorSpec { y: ONE / 2, ..SPEC };
    let mut bytes = room();
    bytes[28..32].copy_from_slice(&40u32.to_le_bytes());
    let edge_offset = 40 + 16 + 20 + 16;
    let mut edges = Vec::new();
    for y in -20..20 {
        for n in [-20 * ONE, y * ONE / 4, 20 * ONE, y * ONE / 4] {
            edges.extend(n.to_le_bytes());
        }
    }
    bytes.splice(edge_offset..edge_offset + 16, edges);
    let dense = hk_format::Room::parse(&bytes).unwrap();
    let mut r = region();
    r.actors = &[&EMBEDDED];
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    tick(&mut w, &r, &dense, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, SPEC.source_id).unwrap().1, EMBEDDED.y);
    let clear = room();
    let clear = hk_format::Room::parse(&clear).unwrap();
    for _ in 0..5 {
        tick(&mut w, &r, &clear, &mut p, &mut v, &n);
    }
    assert_eq!(w.actor_state(0, SPEC.source_id).unwrap().1, EMBEDDED.y);
    w.reset_scene(0);
    tick(&mut w, &r, &clear, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, SPEC.source_id).unwrap().1, ONE);
}

#[test]
fn lethal_hit_replaces_live_actor_with_persistent_nondamaging_source_corpse() {
    const WITH_CORPSE:ActorSpec=ActorSpec {health:EnemyParams{health:5,..SPEC.health},
        corpse:Some(CorpseSpec{air_clip:0,land_clip:0,bounds:SPEC.bounds,spawn_offset:[0,ONE/2],bounce_factor:19661, fling_speed: 15 * hk_sim::ONE, gravity: 48 * hk_sim::ONE, breaker: false, smash_bounces: 0, remove_after_land: 0, hold_ticks: 0}),..SPEC};
    let bytes=room();let room=hk_format::Room::parse(&bytes).unwrap();
    let r=world::Region{actors:&[&WITH_CORPSE],..region()};
    let mut w=enemies::EnemyWorld::new();let mut p=Player::spawn(0,ONE);
    let mut v=Vitals::new(VITAL_PARAMS);let mut n=Nail::new();n.active=true;n.age=1;
    let hit=tick(&mut w,&r,&room,&mut p,&mut v,&n);
    assert_eq!((hit.hits,hit.kills),(1,1));assert_eq!(hit.hurt,Hurt::Ignored);
    assert_eq!(world::DEATHS.with(|v|v.borrow().len()),1);
    assert_eq!(world::DEATHS.with(|v|v.borrow()[0].0),12546);
    assert_eq!(w.prepare_draws(&r,&room,(0,2*ONE)).draw(),1);
    n.active=false;
    for _ in 0..300 {assert_eq!(tick(&mut w,&r,&room,&mut p,&mut v,&n).hurt,Hurt::Ignored);}
    assert_eq!(world::DEATHS.with(|v|v.borrow().len()),1);
    w.sync_region(&world::Region{bounds:[0,0,ONE,ONE],..r});
    assert_eq!(w.prepare_draws(&r,&room,(0,2*ONE)).draw(),1);
    assert_eq!(v.soul,11);assert_eq!(v.health,5);
    w.reset_scene(0);w.sync_region(&r);
    assert_eq!(w.actor_state(0,12546).unwrap().2,5);
}

/// Source Egg Sac (level81:4050): hp 20, no Rigidbody2D, no DamageHero and no
/// Recoil, so it holds its authored transform above the floor and answers only
/// the nail. Its corpse holds one clip for 84 ticks, then plays the other.
const EGG_SAC: ActorSpec = ActorSpec {
    controller: ActorController::Static { idle_clip: 0 },
    source_id: 4050, x: 3 * ONE, y: 5 * ONE,
    health: EnemyParams { health: 20, contact_damage: 0, ..SPEC.health },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    corpse: Some(CorpseSpec { air_clip: 0, land_clip: 0, bounds: [0; 4], spawn_offset: [0, 0],
        bounce_factor: 0, fling_speed: 0, gravity: 0, breaker: false, smash_bounces: 0,
        remove_after_land: 14, hold_ticks: 84 }),
    recoil_speed: 0, recoil_ticks: 0, ..SPEC
};
#[test]
fn static_actor_never_falls_or_hurts_and_its_corpse_bursts_where_it_stood() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = world::Region { actors: &[&EGG_SAC], ..region() };
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(EGG_SAC.x, EGG_SAC.y);
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    for _ in 0..120 {
        assert_eq!(tick(&mut w, &r, &room, &mut p, &mut v, &n).hurt, Hurt::Ignored);
    }
    // Four units above the fixture floor, overlapping the Knight the whole time.
    assert_eq!(w.actor_state(0, EGG_SAC.source_id), Some((EGG_SAC.x, EGG_SAC.y, 20)));
    assert_eq!(v.health, 5);
    assert_eq!(w.prepare_draws(&r, &room, (0, 0)).draw(), 1);
    for round in 0..4 {
        n.active = true;
        n.age = 1;
        let events = tick(&mut w, &r, &room, &mut p, &mut v, &n);
        assert_eq!((events.hits, events.kills), (1, u16::from(round == 3)));
        n.active = false;
        if round < 3 {
            for _ in 0..12 { tick(&mut w, &r, &room, &mut p, &mut v, &n); }
        }
    }
    assert_eq!(w.actor_state(0, EGG_SAC.source_id).unwrap().2, 0);
    assert_eq!(world::DEATHS.with(|v| v.borrow().len()), 1);
    for _ in 0..97 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
        assert_eq!(w.prepare_draws(&r, &room, (0, 0)).draw(), 1);
    }
    tick(&mut w, &r, &room, &mut p, &mut v, &n);
    assert_eq!(w.prepare_draws(&r, &room, (0, 0)).draw(), 0);
}

#[test]
fn cheat_nail_and_contact_protection_use_actual_enemy_runtime() {
 let bytes=room();let room=hk_format::Room::parse(&bytes).unwrap();let r=region();
 for enabled in [false,true] {
  let settings=cheats::Settings{invincible:enabled,max_nail:enabled,..cheats::Settings::new()};
  let mut w=enemies::EnemyWorld::new();let mut p=Player::spawn(0,ONE);let mut v=Vitals::new(VITAL_PARAMS);let mut n=Nail::new();
  let polygons=[&[[-ONE,-2*ONE],[ONE,-2*ONE],[ONE,ONE],[-ONE,ONE]][..];4];
  let hit=w.tick(&r,&room,&mut world::State,&mut p,&mut v,&n,&hk_sim::DreamNail::new(),None,&mut NailResponse::new(),no_attack(),polygons,settings,[0,0,-2496922],|_| panic!("Crawler event"),|_,_,_|None);
  assert_eq!(hit.hurt,if enabled{Hurt::Ignored}else{Hurt::Recoiling});assert_eq!(v.health,if enabled{5}else{4});
  p.x=w.actor_state(0,SPEC.source_id).unwrap().0;n.active=true;n.age=1;
  let hit=w.tick(&r,&room,&mut world::State,&mut p,&mut v,&n,&hk_sim::DreamNail::new(),None,&mut NailResponse::new(),no_attack(),polygons,settings,[0,0,-2496922],|_| panic!("Crawler event"),|_,_,_|None);
  assert_eq!(hit.hits,1);assert_eq!(hit.kills,u16::from(enabled));
  assert_eq!(w.actor_state(0,SPEC.source_id).unwrap().2,if enabled{-13}else{3});
 }
}

#[test]
fn resident_actor_view_matches_unpartitioned_patrol_across_knight_apron() {
    let bytes=room();
    let floor=hk_format::Room::parse(&bytes).unwrap();
    let complete=region();
    let narrow=world::Region {
        bounds:[-ONE,-5*ONE,ONE,20*ONE],
        collision_bounds:[-2*ONE,-10*ONE,2*ONE,25*ONE],
        ..region()
    };
    let mut expected=enemies::EnemyWorld::new();
    let mut actual=enemies::EnemyWorld::new();
    let mut p=Player::spawn(15*ONE,ONE);
    let mut v=Vitals::new(VITAL_PARAMS);
    let mut farthest=0;
    for _ in 0..600 {
        tick(&mut expected,&complete,&floor,&mut p,&mut v,&Nail::new());
        actual.tick(&narrow,&floor,&mut world::State,&mut p,&mut v,&Nail::new(),&hk_sim::DreamNail::new(),None,
            &mut NailResponse::new(),no_attack(),[&[];4],cheats::Settings::new(),[0,0,-2496922],|_| panic!("Crawler event"),
            |scene,x,y| {
                assert_eq!(scene,0);
                assert!(world::contains(complete.collision_bounds,x,y));
                Some((complete,hk_format::Room::parse(&bytes).unwrap()))
            });
        assert_eq!(actual.actor_state(0,SPEC.source_id),expected.actor_state(0,SPEC.source_id));
        farthest=farthest.max(actual.actor_state(0,SPEC.source_id).unwrap().0.abs());
    }
    assert!(farthest>10*ONE);
}

#[test]
fn unrelated_scene_view_cannot_activate_suspended_actor() {
    let bytes=room();let floor=hk_format::Room::parse(&bytes).unwrap();
    let foreign=world::Region{scene:1,..region()};
    let mut current=region();current.bounds=[100*ONE,0,120*ONE,20*ONE];
    current.collision_bounds=current.bounds;
    let mut actors=enemies::EnemyWorld::new();
    actors.tick(&current,&floor,&mut world::State,&mut Player::spawn(10*ONE,ONE),
        &mut Vitals::new(VITAL_PARAMS),&Nail::new(),&hk_sim::DreamNail::new(),None,&mut NailResponse::new(),
        no_attack(),[&[];4],cheats::Settings::new(),[0,0,-2496922],|_| panic!("Crawler event"),
        |_,_,_|Some((foreign,hk_format::Room::parse(&bytes).unwrap())));
    assert_eq!(actors.actor_state(0,SPEC.source_id),Some((SPEC.x,SPEC.y,8)));
}

#[test]
fn oversized_actor_terrain_retains_exact_direct_fallback() {
    let bytes=room();
    let small=hk_format::Room::parse(&bytes).unwrap();
    let mut large=bytes.clone();
    large[28..32].copy_from_slice(&129u32.to_le_bytes());
    let edge:Vec<u8>=small.edge(0).iter().flat_map(|v|v.to_le_bytes()).collect();
    large.splice(92..108,edge.repeat(129));
    let big=hk_format::Room::parse(&large).unwrap();
    let r=region();let mut a=enemies::EnemyWorld::new();let mut b=enemies::EnemyWorld::new();
    let mut p=Player::spawn(15*ONE,ONE);let mut v=Vitals::new(VITAL_PARAMS);
    for _ in 0..600 {
        tick(&mut a,&r,&small,&mut p,&mut v,&Nail::new());
        tick(&mut b,&r,&big,&mut p,&mut v,&Nail::new());
        assert_eq!(a.actor_state(0,SPEC.source_id),b.actor_state(0,SPEC.source_id));
    }
}

/// Source False Knight (`level48:40`) and the `Battle Scene` trigger that starts
/// its fight. The authored transform hangs it inside the ceiling slab above the
/// arena, so the fixture places it in the air and expects nothing to happen to
/// it until the hero crosses the trigger.
const FALSE_KNIGHT: ActorSpec = ActorSpec {
    controller: ActorController::FalseKnight {
        jump_antic_clip: 2, land_clip: 3, stun_opened_clip: 4, attack_clip: 5,
        trigger: [-2 * ONE, -ONE, 2 * ONE, 10 * ONE],
        // `FK Barrel Summon` hangs well above the arena floor in the source, so
        // the fixture drops barrels from above the boss's own spawn height.
        barrel_clip: 6, barrel_spawn_y: 22 * ONE,
    },
    source_id: 100040, x: 0, y: 8 * ONE,
    // Asymmetric on purpose: a symmetric box would hide which way it faces.
    bounds: [-ONE, -2 * ONE, ONE / 2, 0],
    health: EnemyParams { health: 65, contact_damage: 1, evasion_ticks: 15,
        invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 10, turn_cooldown_ticks: 0 },
    // `Turn L` writes the source scale -1.3, which the cook takes the absolute
    // of, so the one authored placement, which faces left, starts at +1 here.
    initial_direction: 1,
    walk_clip: 0, turn_clip: 1, corpse: None, recoil_speed: 0, recoil_ticks: 0, ..SPEC
};
/// Wide enough that the fight's own jumps stay inside one collision view: the
/// slam leaps up to eighteen units past the hero.
fn boss_region() -> world::Region {
    world::Region {
        scene: 0,
        bounds: [-60 * ONE, -5 * ONE, 60 * ONE, 40 * ONE],
        collision_bounds: [-64 * ONE, -10 * ONE, 64 * ONE, 44 * ONE],
        actors: &[&FALSE_KNIGHT],
    }
}
/// The six cooked clips over six textures, plus the barrel's own one-frame
/// clip, so which one is playing is read off what was submitted rather than
/// guessed from a quad count.
const BOSS_CLIPS: u32 = 7;
fn boss_room() -> Vec<u8> {
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, BOSS_CLIPS, 0, BOSS_CLIPS, BOSS_CLIPS, 1, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for texture in 0..BOSS_CLIPS as u16 {
        for n in [0u16, texture * 8, 0, 8, 8, texture] {
            b.extend(n.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
    }
    for texture in 0..BOSS_CLIPS as i32 {
        for n in [texture, -ONE, -2 * ONE, ONE / 2, 0] {
            b.extend(n.to_le_bytes());
        }
    }
    // Idle loops (wrap 0); the other five play once (wrap 2), as the source
    // library declares them. One frame each names the clip in a draw.
    for start in 0..BOSS_CLIPS {
        for n in [start, 1, 12 * 65536, if start == 0 { 0 } else { 2 }] {
            b.extend(n.to_le_bytes());
        }
    }
    for n in [-60 * ONE, 0, 60 * ONE, 0] {
        b.extend(n.to_le_bytes());
    }
    b.resize(b.len() + 32 * BOSS_CLIPS as usize + 32768, 0);
    b
}
/// One swing and its recovery frame, with the hero parked on the boss so that
/// both the body box and the exposed Head box are inside the nail polygon. A
/// fresh `Vitals` each tick because the Hitter would otherwise kill the
/// five-mask harness hero long before the fight ends; the fight is what is
/// under test, not how survivable it is.
fn swing(w: &mut enemies::EnemyWorld, r: &world::Region, room: &hk_format::Room,
         p: &mut Player, n: &mut Nail) -> enemies::Events {
    let (x, y, _) = w.actor_state(0, FALSE_KNIGHT.source_id).unwrap();
    p.x = x;
    p.y = y + ONE;
    n.active = true;
    n.age = 1;
    let events = tick(w, r, room, p, &mut Vitals::new(VITAL_PARAMS), n);
    n.active = false;
    tick(w, r, room, p, &mut Vitals::new(VITAL_PARAMS), n);
    events
}
#[test]
fn nothing_of_the_boss_runs_until_the_hero_crosses_the_battle_trigger() {
    let bytes = boss_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = boss_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    // `Dormant` runs no solver: the body holds the authored transform, and it
    // is not drawn either, because the source hides it inside the ceiling slab.
    assert_eq!(w.actor_state(0, FALSE_KNIGHT.source_id).unwrap(), (0, 8 * ONE, 65));
    assert!(submitted(&w, &r, &room).is_empty());
    // `Detect` -> `Start`: BATTLE START, and `Start Fall` drops it.
    p.x = 0;
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    // Body bottom is two units below the transform, so a boss standing on the
    // fixture floor at y 0 rests at y 2.
    let (_, y, health) = w.actor_state(0, FALSE_KNIGHT.source_id).unwrap();
    assert_eq!(y, 2 * ONE);
    assert_eq!(health, 65);
    assert_eq!(submitted(&w, &r, &room).len(), 1, "it is drawn once it has dropped");
}
#[test]
fn the_body_staggers_at_zero_instead_of_dying_and_opens_its_armour() {
    let bytes = boss_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = boss_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    // Thirteen five-damage hits empty a 65 hp body. `Check Health` answers zero
    // with SetHP 65 and STUN, so it never dies and never stops being drawn.
    let mut opened = false;
    for _ in 0..500 {
        let events = swing(&mut w, &r, &room, &mut p, &mut n);
        assert_eq!(events.kills, 0, "the body is not killable");
        assert!(w.actor_state(0, FALSE_KNIGHT.source_id).unwrap().2 > 0);
        if submitted(&w, &r, &room).iter().any(|(id, _)| *id == 4) {
            opened = true;
            break;
        }
    }
    assert!(opened, "the stagger reaches `Opened` and shows the exposed head");
}
#[test]
fn three_conversions_take_it_through_the_death_sequence() {
    let bytes = boss_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = boss_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    // A player who punishes the stagger and then backs off, rather than the
    // impossible one that swings every other tick forever: the armour closing
    // holds fire for the 700 ticks a rage takes. That is not politeness, it is
    // what makes the fight finish at all. The source body is damageable during
    // its rage, so a hero who can stand inside the boss and keep hitting it
    // empties the 65 hp again before the eighth slam, `Check Health` staggers it
    // out of `Rage Check`, and the branch that reads `Stunned Amount >= 3` is
    // never reached however many conversions the player wins.
    let mut hits = 0u32;
    let mut died = false;
    let mut was_open = false;
    let mut hold = 0u32;
    for _ in 0..30000 {
        let (x, y, _) = w.actor_state(0, FALSE_KNIGHT.source_id).unwrap();
        // The camera follows the boss so the open-armour clip is never culled.
        render::QUADS.with(|q| q.borrow_mut().clear());
        w.prepare_draws(&r, &room, (x, y)).draw();
        let open = render::QUADS.with(|q| q.borrow().iter().any(|(id, _)| *id == 4));
        if was_open && !open { hold = 700; }
        was_open = open;
        if hold == 0 {
            p.x = x;
            p.y = y + ONE;
            n.active = true;
            n.age = 1;
        }
        hold = hold.saturating_sub(1);
        let events = tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        n.active = false;
        assert_eq!(events.kills, 0, "neither HealthManager reports a kill");
        hits += events.hits as u32;
        // `Decrement Battle Enemies` and then the HealthManager death event the
        // arena has been waiting on. The boss pays no Geo, but this drain is
        // where a dead actor's death event surfaces.
        w.take_geo_deaths(0, |_, _, _| { died = true; true });
        if died { break; }
    }
    assert!(died, "the fight never reached the death event");
    // Three staggers converted: 13 body hits and 8 head hits each, at least.
    assert!(hits >= 63, "the fight took {hits} nail hits, which is too few to have run");
    // The route counters are one process-wide set and the other boss cases in
    // this file run beside this one, so what holds here is what interference
    // can only add to, plus the relation between two of them. Their exact
    // values belong to a replay, which is the whole reason they exist.
    unsafe {
        assert!(enemies::HK_FK_TRIGGERED >= 1 && enemies::HK_FK_DROPPED >= 1);
        assert!(enemies::HK_FK_CONVERSIONS >= 3, "three phases, from three emptied Heads");
        // The last exposure empties the Head too, and only the 450-tick tail
        // after it counts as a death, so a route can tell the two apart.
        assert!(enemies::HK_FK_DEATHS >= 1);
        assert!(enemies::HK_FK_STAGGERS >= enemies::HK_FK_CONVERSIONS,
                "every converted phase cost a stagger first");
    }
    // `End Wait` writes `Activated` before its two-second wait and then sends
    // BG OPEN. Both have to have happened by the time the gates reopen.
    for _ in 0..hk_sim::boss::END_WAIT_TICKS + 2 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    }
    assert_eq!(script::player_data(script::FIELD_FALSE_KNIGHT_ARENA), 1,
               "a won arena has to persist, or a reload re-arms it");
    let events = battle_gates::EVENTS.with(|e| e.borrow().clone());
    assert_eq!(events.first(), Some(&("placement", 0)));
    assert!(events.contains(&("close", 0)), "the trigger shuts the room");
    assert_eq!(events.last(), Some(&("open", 0)), "and the win reopens it");
}

/// The reload the whole of `Battle Control`'s `Init` branch exists for: the same
/// room, entered with the persisted bool set, must place its gates open and
/// never fight again however long the hero stands in the trigger.
#[test]
fn a_persisted_arena_quick_opens_its_gates_and_refuses_to_fight() {
    let bytes = boss_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = boss_region();
    script::set_player_data(script::FIELD_FALSE_KNIGHT_ARENA, 1);
    script::set_player_data(script::FIELD_FALSE_KNIGHT_FIRST_PLOP, 1);
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    // Standing in the trigger from the first tick, which is the worst case: a
    // hero who reloads inside the curtain must not re-arm the fight.
    for _ in 0..600 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    assert_eq!(battle_gates::EVENTS.with(|e| e.borrow().clone()), vec![("quick open", 0)]);
    // Still hanging in the ceiling slab at its authored transform, undrawn.
    assert_eq!(w.actor_state(0, FALSE_KNIGHT.source_id).unwrap(), (0, 8 * ONE, 65));
    assert!(submitted(&w, &r, &room).is_empty());
}

/// The one thing that proves the shake and the two voices are wired rather than
/// merely present: `camera::request`, `audio::boss_land` and `audio::boss_swing`
/// have no other caller in the guest, so if this arm is dropped the guest still
/// builds, every other case here still passes, and the fight goes silent.
#[test]
fn the_fight_reaches_the_camera_and_the_spu() {
    let bytes = boss_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = boss_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    assert!(boss_effects::take().is_empty(), "`Dormant` sends nothing at all");
    // `Start Fall` shakes as the body is released from the ceiling slab, and
    // `State 2` lands it with AverageShake over the landing voice.
    p.x = 0;
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    assert_eq!(boss_effects::take(), vec!["shake big", "shake average", "land voice"]);
    let mut log: Vec<&'static str> = Vec::new();
    let slam = |log: &[&'static str]| log.windows(2).any(|pair| pair == ["swing voice", "shake big"]);
    for _ in 0..4000 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        log.extend(boss_effects::take());
        if slam(&log) {
            break;
        }
    }
    assert!(log.contains(&"shake kill"), "a launch sends EnemyKillShake: {log:?}");
    assert!(log.contains(&"land voice"), "and a landing plays its own voice: {log:?}");
    // `S Attack` swings and `Slam` shakes eight ticks later. The jump attack
    // fires the same two the other way round, so this order is the slam's.
    assert!(slam(&log), "the slam has to reach the camera: {log:?}");
}

/// `FK Barrel Summon`, end to end: the boss sends SUMMON, the summoner spaces
/// the burst out, and each `Falling Barrel` falls from the summoner's own y
/// until terrain or the hero ends it.
///
/// Driven through a real fight rather than by poking the pool, because the
/// thing worth proving is that the phase tables reach the summoner at all: in
/// phase one both barrel tables are [0, 0] and nothing may fall, and the rage
/// that follows the first conversion asks for the whole pool.
#[test]
fn the_fight_drops_barrels_the_hero_can_be_hit_by() {
    let bytes = boss_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = boss_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, ONE);
    let mut n = Nail::new();
    // The barrel's own clip, which is the only way to tell one from the boss in
    // a submitted frame: boss_room gives clip k the texture k.
    let barrel_texture = 6usize;
    let barrels = |w: &enemies::EnemyWorld, x: i32, y: i32| {
        render::QUADS.with(|q| q.borrow_mut().clear());
        w.prepare_draws(&r, &room, (x, y)).draw();
        render::QUADS.with(|q| q.borrow().iter().filter(|(id, _)| *id == barrel_texture)
            .map(|(_, quad)| span(quad).1).collect::<Vec<_>>())
    };
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    }
    assert!(barrels(&w, 0, 0).is_empty(), "the entrance summons nothing");

    let mut was_open = false;
    let mut hold = 0u32;
    let mut fell = Vec::new();
    let mut hurt_in_the_air = 0u32;
    let mut phase_one_barrels = 0u32;
    let mut conversions = 0u32;
    for _ in 0..30000 {
        let (x, y, _) = w.actor_state(0, FALSE_KNIGHT.source_id).unwrap();
        render::QUADS.with(|q| q.borrow_mut().clear());
        w.prepare_draws(&r, &room, (x, y)).draw();
        let open = render::QUADS.with(|q| q.borrow().iter().any(|(id, _)| *id == 4));
        if was_open && !open { hold = 700; conversions += 1; }
        was_open = open;
        if hold == 0 {
            p.x = x;
            p.y = y + ONE;
            n.active = true;
            n.age = 1;
        } else {
            // Ten units above the arena floor and in the middle of the summon
            // span, which is out of reach of the body box and of the Hitter's
            // eight units and leaves a falling barrel as the only thing that
            // can touch the hero.
            p.x = hk_sim::false_knight::RAGE_POINT_X;
            p.y = 12 * ONE;
        }
        hold = hold.saturating_sub(1);
        let events = tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        n.active = false;
        let seen = barrels(&w, x, y);
        if conversions == 0 { phase_one_barrels += seen.len() as u32; }
        if !seen.is_empty() { fell.push(seen); }
        if hold > 0 && events.hurt != Hurt::Ignored { hurt_in_the_air += 1; }
        if conversions >= 2 { break; }
    }
    assert_eq!(phase_one_barrels, 0,
               "JUMP_BARRELS[0] and SLAM_BARRELS[0] are both [0,0]: phase one drops nothing");
    assert!(fell.len() > 60, "the rage asks for the whole pool; saw {} frames with a barrel", fell.len());
    // `Falling Barrel` is a dynamic body under gravity 0.325 and nothing pushes
    // it sideways, so every barrel's screen y only ever increases.
    let highest = fell.iter().map(|ys| *ys.iter().min().unwrap()).min().unwrap();
    let lowest = fell.iter().map(|ys| *ys.iter().max().unwrap()).max().unwrap();
    assert!(lowest > highest, "the barrels never fell: {highest}..{lowest}");
    assert!(hurt_in_the_air > 0,
            "no barrel reached a hero parked in the middle of the summon span");
    // A barrel's box is a trigger, so terrain ends it rather than stopping it,
    // and the pool has to come back or the next burst evicts a live barrel.
    for _ in 0..600 {
        let (x, y, _) = w.actor_state(0, FALSE_KNIGHT.source_id).unwrap();
        p.x = x;
        p.y = y + ONE;
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        let _ = barrels(&w, x, y);
    }
    let (x, y, _) = w.actor_state(0, FALSE_KNIGHT.source_id).unwrap();
    assert!(barrels(&w, x, y).is_empty(), "every barrel of the burst reached the floor");
}

/// What the barrel costs in static RAM, which is the budget this port is
/// tightest on: `.hkpsx/build.json` reports 11,556 unallocated bytes before the
/// reserved stack, and `game/src/audio.rs` `include_bytes!`s the sound bank out
/// of the same figure. Every field below is a fixed-width integer, so the
/// native layout here is the MIPS layout.
///
/// The barrel art is not in this: it rides the scene pack, which is read from
/// disc into the scene arena, and `pack_scenes::scene_arena_bytes` sizes that
/// arena from the largest scene, which is Tutorial_01 rather than Crossroads_10.
#[test]
fn the_barrel_pool_is_paid_for_in_padding_and_twenty_bytes() {
    use core::mem::size_of;
    // `Shot` before the barrel rode the same pool, field for field, and after.
    #[allow(dead_code)]
    struct ShotBefore { scene: u8, x: i32, y: i32, vx: i32, vy: i32,
        animation_tick: u32, impact: bool, shot_clip: u16, impact_clip: u16 }
    #[allow(dead_code)]
    struct ShotAfter { scene: u8, x: i32, y: i32, vx: i32, vy: i32,
        animation_tick: u32, impact: bool, barrel: bool, shot_clip: u16, impact_clip: u16 }
    // The pool costs nothing: `barrel` is a second bool beside `impact`, and
    // `Option` still has a bool's niche to put its discriminant in.
    assert_eq!(size_of::<ShotAfter>(), size_of::<ShotBefore>());
    assert_eq!(size_of::<[Option<ShotAfter>; 8]>(), size_of::<[Option<ShotBefore>; 8]>());
    // The held SUMMON costs four bytes on the boss runtime and none in
    // `EnemyWorld`, because `Runtime` is sized by a larger variant than the
    // False Knight's. Measured rather than argued: without this pair a later
    // field would grow the boss runtime past that variant in silence.
    #[allow(dead_code)]
    struct BossBefore { controller: hk_sim::false_knight::FalseKnight, arena: hk_sim::boss::Arena,
        head: ActorHealth, animation_tick: u32, clip: hk_sim::false_knight::Clip, vx: i32,
        gravity: i32, kinematic: bool, hitter: bool, contact_damage: u16, separated: bool }
    #[allow(dead_code)]
    struct BossAfter { controller: hk_sim::false_knight::FalseKnight, arena: hk_sim::boss::Arena,
        head: ActorHealth, animation_tick: u32, clip: hk_sim::false_knight::Clip, vx: i32,
        gravity: i32, kinematic: bool, hitter: bool, contact_damage: u16, summon: u8, separated: bool }
    assert!(size_of::<BossAfter>() - size_of::<BossBefore>() <= 4);
    // What the summoner itself costs, once for the whole world rather than per
    // actor slot, which is why it lives on `EnemyWorld` and not on the boss.
    assert_eq!(size_of::<hk_sim::false_knight::Summon>() + size_of::<Option<(u8, i32, u16)>>(), 20);
    // 5,856 with the standing Sentry, 6,112 with the whole fight, 6,136 with
    // the barrels. Asserted as a ceiling rather than an equality because other
    // controllers share this type; docs/FALSE_KNIGHT.md carries the reading.
    assert!(size_of::<enemies::EnemyWorld>() <= 6136,
            "EnemyWorld is {} bytes of the 11,556 unallocated in build.json",
            size_of::<enemies::EnemyWorld>());
}
