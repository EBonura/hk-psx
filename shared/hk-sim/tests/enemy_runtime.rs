//! Native compile/execution of the guest enemy module with hardware-only drawing
//! stubbed. Fixtures exercise gameplay rather than mirroring GPU internals.
use hk_sim::*;
include!("common/enemy_stubs.rs");
#[path = "../../../game/src/enemies.rs"]
mod enemies;
/// The one placement every fixture that does not need its own stands at.
const PLACEMENT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
    source_id: 12546,
    x: 0,
    y: ONE,
    initial_direction: -1,
    random_start_direction: false,
    start_alert: false,
    start_right: false,
    rotation_q16: 0,
    fsm_activator: false,
};
const SPEC: ActorSpec = ActorSpec {
    controller: ActorController::Crawler,
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
        actors: &[(PLACEMENT, &SPEC)],
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
    tick_camera(w, r, room, p, v, n, [0, 0, -2496922])
}
fn tick_camera(
    w: &mut enemies::EnemyWorld,
    r: &world::Region,
    room: &hk_format::Room,
    p: &mut Player,
    v: &mut Vitals,
    n: &Nail,
    camera: [i32; 3],
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
        camera,
        |_| panic!("Crawler emitted Runner event"),
        |_,_,_,_|None,
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
    assert_eq!(payouts.len(),1);assert_eq!(payouts[0].0,PLACEMENT.source_id);
    w.sync_region(&r);
    w.take_geo_deaths(0,|_,_,_|panic!("Grid swap paid the same death twice"));
    assert!(w.actor_state(0, 12546).unwrap().2 <= 0);
    w.reset_scene(0);
    w.sync_region(&r);
    assert_eq!(w.actor_state(0, 12546), Some((PLACEMENT.x, PLACEMENT.y, 8)));
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
        actors: &[(PLACEMENT, &SPEC)],
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
    const OVERLAPPING_SPEC: ActorSpec = ActorSpec { dream_soul: 0, ..SPEC };
    const OVERLAPPING: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
        y: ONE - ONE / 8, ..PLACEMENT
    };
    let bytes = room();
    let floor = hk_format::Room::parse(&bytes).unwrap();
    let mut r = region();
    r.actors = &[(OVERLAPPING, &OVERLAPPING_SPEC)];
    let remote = world::Region {
        collision_bounds: [97 * ONE, -5 * ONE, 123 * ONE, 25 * ONE],
        ..r
    };
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    tick(&mut w, &remote, &floor, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, PLACEMENT.source_id).unwrap().1, OVERLAPPING.y);
    tick(&mut w, &r, &floor, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, PLACEMENT.source_id).unwrap().1, ONE);
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
    assert!(w.actor_state(0, PLACEMENT.source_id).unwrap().1 < ONE);
}

#[test]
fn unresolved_initial_geometry_suspends_without_retry_until_scene_reset() {
    const EMBEDDED: hk_sim::ActorPlacement = hk_sim::ActorPlacement { y: ONE / 2, ..PLACEMENT };
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
    r.actors = &[(EMBEDDED, &SPEC)];
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    tick(&mut w, &r, &dense, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, PLACEMENT.source_id).unwrap().1, EMBEDDED.y);
    let clear = room();
    let clear = hk_format::Room::parse(&clear).unwrap();
    for _ in 0..5 {
        tick(&mut w, &r, &clear, &mut p, &mut v, &n);
    }
    assert_eq!(w.actor_state(0, PLACEMENT.source_id).unwrap().1, EMBEDDED.y);
    w.reset_scene(0);
    tick(&mut w, &r, &clear, &mut p, &mut v, &n);
    assert_eq!(w.actor_state(0, PLACEMENT.source_id).unwrap().1, ONE);
}

#[test]
fn lethal_hit_replaces_live_actor_with_persistent_nondamaging_source_corpse() {
    const WITH_CORPSE:ActorSpec=ActorSpec {health:EnemyParams{health:5,..SPEC.health},
        corpse:Some(CorpseSpec{air_clip:0,land_clip:0,bounds:SPEC.bounds,spawn_offset:[0,ONE/2],bounce_factor:19661, fling_speed: 15 * hk_sim::ONE, gravity: 48 * hk_sim::ONE, breaker: false, smash_bounces: 0, remove_after_land: 0, hold_ticks: 0}),..SPEC};
    let bytes=room();let room=hk_format::Room::parse(&bytes).unwrap();
    let r=world::Region{actors:&[(PLACEMENT, &WITH_CORPSE)],..region()};
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
const EGG_SAC_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
    source_id: 4050, x: 3 * ONE, y: 5 * ONE, ..PLACEMENT
};
const EGG_SAC: ActorSpec = ActorSpec {
    controller: ActorController::Static { idle_clip: 0 },
    health: EnemyParams { health: 20, contact_damage: 0, ..SPEC.health },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    corpse: Some(CorpseSpec { air_clip: 0, land_clip: 0, bounds: [0; 4], spawn_offset: [0, 0],
        bounce_factor: 0, fling_speed: 0, gravity: 0, breaker: false, smash_bounces: 0,
        remove_after_land: 14, hold_ticks: 84 }),
    recoil_speed: 0, recoil_ticks: 0, ..SPEC
};
#[test]
fn a_nail_hit_flashes_the_actor_and_the_flash_fades_out() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = world::Region { actors: &[(EGG_SAC_AT, &EGG_SAC)], ..region() };
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(EGG_SAC_AT.x, EGG_SAC_AT.y);
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    for _ in 0..30 { tick(&mut w, &r, &room, &mut p, &mut v, &n); }
    let flashes = || render::FLASHES.with(|f| core::mem::take(&mut *f.borrow_mut()));
    w.prepare_draws(&r, &room, (0, 0)).draw();
    assert!(flashes().is_empty(), "no flash before a hit");
    n.active = true;
    n.age = 1;
    assert_eq!(tick(&mut w, &r, &room, &mut p, &mut v, &n).hits, 1);
    n.active = false;
    w.prepare_draws(&r, &room, (0, 0)).draw();
    // SpriteFlash.flashInfected: 0.9 * (1, 0.31, 0) at full strength.
    assert_eq!(flashes().iter().map(|f| f.1).collect::<Vec<_>>(), [(115, 36, 0)]);
    let mut last = (115, 36, 0);
    for _ in 0..15 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
        w.prepare_draws(&r, &room, (0, 0)).draw();
        let tint = flashes()[0].1;
        assert!(tint.0 < last.0, "the flash fades every tick");
        last = tint;
    }
    tick(&mut w, &r, &room, &mut p, &mut v, &n);
    w.prepare_draws(&r, &room, (0, 0)).draw();
    assert!(flashes().is_empty(), "the flash is over after 16 ticks");
}

#[test]
fn static_actor_never_falls_or_hurts_and_its_corpse_bursts_where_it_stood() {
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = world::Region { actors: &[(EGG_SAC_AT, &EGG_SAC)], ..region() };
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(EGG_SAC_AT.x, EGG_SAC_AT.y);
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    for _ in 0..120 {
        assert_eq!(tick(&mut w, &r, &room, &mut p, &mut v, &n).hurt, Hurt::Ignored);
    }
    // Four units above the fixture floor, overlapping the Knight the whole time.
    assert_eq!(w.actor_state(0, EGG_SAC_AT.source_id), Some((EGG_SAC_AT.x, EGG_SAC_AT.y, 20)));
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
    assert_eq!(w.actor_state(0, EGG_SAC_AT.source_id).unwrap().2, 0);
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
  let hit=w.tick(&r,&room,&mut world::State,&mut p,&mut v,&n,&hk_sim::DreamNail::new(),None,&mut NailResponse::new(),no_attack(),polygons,settings,[0,0,-2496922],|_| panic!("Crawler event"),|_,_,_,_|None);
  assert_eq!(hit.hurt,if enabled{Hurt::Ignored}else{Hurt::Recoiling});assert_eq!(v.health,if enabled{5}else{4});
  p.x=w.actor_state(0,PLACEMENT.source_id).unwrap().0;n.active=true;n.age=1;
  let hit=w.tick(&r,&room,&mut world::State,&mut p,&mut v,&n,&hk_sim::DreamNail::new(),None,&mut NailResponse::new(),no_attack(),polygons,settings,[0,0,-2496922],|_| panic!("Crawler event"),|_,_,_,_|None);
  assert_eq!(hit.hits,1);assert_eq!(hit.kills,u16::from(enabled));
  assert_eq!(w.actor_state(0,PLACEMENT.source_id).unwrap().2,if enabled{-13}else{3});
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
            |scene,x,y,_| {
                assert_eq!(scene,0);
                assert!(world::contains(complete.collision_bounds,x,y));
                Some((complete,hk_format::Room::parse(&bytes).unwrap()))
            });
        assert_eq!(actual.actor_state(0,PLACEMENT.source_id),expected.actor_state(0,PLACEMENT.source_id));
        farthest=farthest.max(actual.actor_state(0,PLACEMENT.source_id).unwrap().0.abs());
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
        |_,_,_,_|Some((foreign,hk_format::Room::parse(&bytes).unwrap())));
    assert_eq!(actors.actor_state(0,PLACEMENT.source_id),Some((PLACEMENT.x,PLACEMENT.y,8)));
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
        assert_eq!(a.actor_state(0,PLACEMENT.source_id),b.actor_state(0,PLACEMENT.source_id));
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
    // Asymmetric on purpose: a symmetric box would hide which way it faces.
    bounds: [-ONE, -2 * ONE, ONE / 2, 0],
    health: EnemyParams { health: 65, contact_damage: 1, evasion_ticks: 15,
        invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 10, turn_cooldown_ticks: 0 },
    walk_clip: 0, turn_clip: 1, corpse: None, recoil_speed: 0, recoil_ticks: 0, ..SPEC
};
/// `Turn L` writes the source scale -1.3, which the cook takes the absolute of,
/// so the one authored placement, which faces left, starts at +1 here.
const FALSE_KNIGHT_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
    source_id: 100040, x: 0, y: 8 * ONE, initial_direction: 1, ..PLACEMENT
};
/// Wide enough that the fight's own jumps stay inside one collision view: the
/// slam leaps up to eighteen units past the hero.
fn boss_region() -> world::Region {
    world::Region {
        scene: 0,
        bounds: [-60 * ONE, -5 * ONE, 60 * ONE, 40 * ONE],
        collision_bounds: [-64 * ONE, -10 * ONE, 64 * ONE, 44 * ONE],
        actors: &[(FALSE_KNIGHT_AT, &FALSE_KNIGHT)],
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
    let (x, y, _) = w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap();
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
    assert_eq!(w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap(), (0, 8 * ONE, 65));
    assert!(submitted(&w, &r, &room).is_empty());
    // `Detect` -> `Start`: BATTLE START, and `Start Fall` drops it.
    p.x = 0;
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    // Body bottom is two units below the transform, so a boss standing on the
    // fixture floor at y 0 rests at y 2.
    let (_, y, health) = w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap();
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
        assert!(w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap().2 > 0);
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
        let (x, y, _) = w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap();
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
    assert_eq!(persist::get(persist::Kind::BattleScene, 0, 0), Some(1),
               "a won arena has to persist, or a reload re-arms it");
    assert!(persist::player(persist::FALSE_KNIGHT_DEFEATED), "and the boss's own death writes falseKnightDefeated");
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
    persist::set(persist::Kind::BattleScene, 0, 0, 1);
    persist::set_player(persist::FALSE_KNIGHT_FIRST_PLOP);
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
    assert_eq!(w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap(), (0, 8 * ONE, 65));
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
    // `Start Fall` shakes as the body is released from the ceiling slab, with
    // the ceiling breaking on the scene voice, and `State 2` lands it with
    // AverageShake over the landing voice and `Rubble End`'s heavy landing.
    p.x = 0;
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut v, &n);
    }
    assert_eq!(boss_effects::take(), vec!["shake big", "scene voice", "shake average", "land voice", "scene voice"]);
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
        let (x, y, _) = w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap();
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
        let (x, y, _) = w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap();
        p.x = x;
        p.y = y + ONE;
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        let _ = barrels(&w, x, y);
    }
    let (x, y, _) = w.actor_state(0, FALSE_KNIGHT_AT.source_id).unwrap();
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
    // 6,904 with each actor's cached view box (24 bytes a slot, which this
    // test did not see while the stand-in `disc::Located` was empty), 7,208
    // with the per-slot placement table (a u8 and a spec reference a slot; the
    // reference is 8 bytes here and 4 on the guest, which pays 168).
    // 7,464 with the Head's clip and the Death Head's slide on the boss runtime,
    // eight bytes on the widest variant and so eight a slot. 7,592 with the two
    // `Shockwave Wave` slots, 64 bytes each, once on the world. 8,264 with the
    // shot pool at 32 slots (28 bytes each) so Brooding Mawlek's 25-shot spit
    // is on screen whole; main.rs's frame grew by the same 672 on the guest.
    assert!(size_of::<enemies::EnemyWorld>() <= 8264,
            "EnemyWorld is {} bytes of the 11,556 unallocated in build.json",
            size_of::<enemies::EnemyWorld>());
}

/// Source Crossroads_11_alt Blocker (`level50:4585`), which is the placement
/// that can sleep. It stands where the fixture's floor is, faces right because
/// its transform is mirrored, and deals no contact damage at all: the source
/// object carries no `DamageHero`, which is why the hero can stand on it.
const BLOCKER: ActorSpec = ActorSpec {
    controller: ActorController::Blocker {
        // Idle and Closed are the shared slots; these six are `Clip::slot`
        // order, and the last two belong to the pooled `Shot Mawlek`.
        clips: [2, 3, 4, 5, 6, 7],
        shot_clip: 8,
        impact_clip: 9,
        sleeps: true,
    },
    // The source body box, which is also its `Terrain Block`.
    bounds: [-128000, -145408, 54272, 61440],
    health: EnemyParams { health: 60, contact_damage: 0, evasion_ticks: 12,
        invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0, turn_clip: 1, corpse: None, recoil_speed: 0, recoil_ticks: 0,
    ..SPEC
};
const BLOCKER_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
    source_id: 504585, x: 0, y: 4 * ONE, initial_direction: 1, ..PLACEMENT
};
fn blocker_region() -> world::Region {
    world::Region {
        scene: 0,
        bounds: [-20 * ONE, -5 * ONE, 20 * ONE, 20 * ONE],
        collision_bounds: [-23 * ONE, -10 * ONE, 23 * ONE, 25 * ONE],
        actors: &[(BLOCKER_AT, &BLOCKER)],
    }
}
/// A room carrying the Blocker's ten clips at their source frame counts and
/// rates, because the controller leaves five of its states on clip completion
/// and completion is measured off these numbers.
fn blocker_room() -> Vec<u8> {
    // (frames, fps, wrap): Idle and Closed loop, the rest play once, and the
    // last two are `Shot Mawlek`'s own Idle and Impact.
    let clips: [(u32, u32, u32); 10] = [
        (7, 12, 0), (1, 30, 0), (4, 15, 2), (2, 15, 2), (4, 15, 2),
        (3, 15, 2), (4, 15, 2), (7, 12, 2), (4, 20, 0), (6, 20, 2),
    ];
    let frames = 7;
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 1, 0, frames, clips.len() as u32, 1, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u16, 0, 0, 4, 4, 0] {
        b.extend(n.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    for _ in 0..frames {
        for n in [0i32, -ONE / 2, -ONE, ONE / 2, 0] {
            b.extend(n.to_le_bytes());
        }
    }
    for (count, fps, wrap) in clips {
        for n in [0u32, count, fps * 65536, wrap] {
            b.extend(n.to_le_bytes());
        }
    }
    for n in [-20 * ONE, 0, 20 * ONE, 0] {
        b.extend(n.to_le_bytes());
    }
    b.resize(b.len() + 32 + 32768, 0);
    b
}
/// The whole Blocker cycle through the real runtime: the shell, the nail, the
/// shot and the fact that none of it ever moves the body.
///
/// The hero stands inside `Alert Range New` and outside `Attack Range`, which
/// is the only band where the Blocker opens and stays open. Those two boxes are
/// `hk_sim::blocker` constants, so this fixture is also what proves the
/// recognizer's geometry and the runtime's agree.
#[test]
fn a_blocker_opens_for_the_hero_shuts_underneath_and_lobs_one_goop() {
    let bytes = blocker_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = blocker_region();
    let mut w = enemies::EnemyWorld::new();
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut n = Nail::new();
    // Out of every box: the shell stays up, so the nail is blocked outright.
    let mut far = Player::spawn(-18 * ONE, 4 * ONE);
    n.active = true;
    n.age = 1;
    for _ in 0..3 {
        let events = tick(&mut w, &r, &room, &mut far, &mut v, &n);
        assert_eq!(events.hits, 0, "a shut Blocker takes no damage from anywhere");
    }
    n.active = false;
    assert_eq!(w.actor_state(0, 504585), Some((0, 4 * ONE, 60)));
    // Inside the alert band and clear of the attack box underneath it.
    let mut hero = Player::spawn(10 * ONE, 4 * ONE);
    let mut fired = None;
    for t in 0..240u32 {
        tick(&mut w, &r, &room, &mut hero, &mut v, &n);
        let quads = submitted(&w, &r, &room);
        // The Blocker's own quad is always there; a second one is the shot.
        assert!(!quads.is_empty(), "the Blocker always draws");
        if quads.len() > 1 && fired.is_none() {
            fired = Some(t);
        }
    }
    let fired = fired.expect("the Blocker lobs a goop within four seconds");
    // `Open` is sixteen ticks and `Shoot Antic` twelve, with a WaitRandom of
    // 48..72 between them, so the first shot cannot beat this and the whole
    // chain has to have run for it to arrive at all.
    assert!((29..=101).contains(&fired), "first shot at tick {fired}");
    // Which gravity the pool gave it is `blocker_goop_arcs_over_where_an_aspid_shot_would_not`
    // in `enemies`; this fixture only proves one came out.
    // Nothing above moved the body.
    assert_eq!(w.actor_state(0, 504585).map(|s| (s.0, s.1)), Some((0, 4 * ONE)));
    let mut hit = Nail::new();
    hit.active = true;
    hit.age = 1;
    // Walk underneath: `Attack Range` reaches 6.5 units to its right, so the
    // shell comes up and the nail cannot touch it from that side at all.
    let mut under = Player::spawn(ONE, 4 * ONE);
    for _ in 0..40 {
        tick(&mut w, &r, &room, &mut under, &mut v, &n);
    }
    let mut blocked = 0;
    for _ in 0..3 {
        blocked += tick(&mut w, &r, &room, &mut under, &mut v, &hit).hits;
    }
    assert_eq!(blocked, 0, "a shut Blocker blocks every direction, the pogo included");
    // Past it on the left, where `Attack Range` stops 2.475 units out. `Closed`
    // watches only that box, so it opens again even though the hero is now
    // behind `Alert Range New` as well, and there the nail reaches it. That
    // narrow left-hand window is the whole of the Blocker's melee surface.
    let mut behind = Player::spawn(-28 * ONE / 10, 4 * ONE);
    for _ in 0..40 {
        tick(&mut w, &r, &room, &mut behind, &mut v, &n);
    }
    let mut landed = 0;
    for _ in 0..3 {
        landed += tick(&mut w, &r, &room, &mut behind, &mut v, &hit).hits;
    }
    assert_eq!(landed, 1, "an opened Blocker answers the nail from behind");
    assert_eq!(w.actor_state(0, 504585), Some((0, 4 * ONE, 55)));
}

/// Source Fungus1_01 Pigeon (`level128:7965`). One hit point, no contact
/// damage, no corpse, and a collider that is a trigger rather than a body, so
/// the hero walks through it and it walks through the floor.
const PIGEON: ActorSpec = ActorSpec {
    // `Idle 01` and `Fly` are the shared slots; these two are `Clip::slot`
    // order, which is `Idle 02` then `Idle 03`.
    controller: ActorController::Pigeon { clips: [2, 3] },
    // The source trigger box, measured off the placement at its 0.8 scale.
    bounds: [-11600, 10813, 23724, 58327],
    health: EnemyParams { health: 1, contact_damage: 0, evasion_ticks: 12,
        invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0, turn_clip: 1, corpse: None, recoil_speed: 0, recoil_ticks: 0,
    dream_soul: 0,
    ..SPEC
};
const PIGEON_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement {
    source_id: 1287965, x: 0, y: 4 * ONE, initial_direction: -1, ..PLACEMENT
};
fn pigeon_region() -> world::Region {
    world::Region {
        scene: 0,
        bounds: [-20 * ONE, -5 * ONE, 20 * ONE, 20 * ONE],
        collision_bounds: [-23 * ONE, -10 * ONE, 23 * ONE, 25 * ONE],
        actors: &[(PIGEON_AT, &PIGEON)],
    }
}
/// A room carrying the family's four clips at their source frame counts, all
/// looping at 12 fps. The counts matter because `Set Frame` seeks into them.
fn pigeon_room() -> Vec<u8> {
    let clips: [(u32, u32, u32); 4] = [(67, 12, 0), (4, 12, 0), (41, 12, 0), (61, 12, 0)];
    let frames = 67;
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 1, 0, frames, clips.len() as u32, 1, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u16, 0, 0, 4, 4, 0] {
        b.extend(n.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    for _ in 0..frames {
        // The frame's world box, which `prepare_draws` mirrors by the facing.
        for n in [0i32, -11600, 10813, 23724, 58327] {
            b.extend(n.to_le_bytes());
        }
    }
    for (count, fps, wrap) in clips {
        for n in [0u32, count, fps * 65536, wrap] {
            b.extend(n.to_le_bytes());
        }
    }
    for n in [-20 * ONE, 0, 20 * ONE, 0] {
        b.extend(n.to_le_bytes());
    }
    b.resize(b.len() + 32 + 32768, 0);
    b
}
/// The whole of a Pigeon through the real runtime: it sits, the hero walking up
/// lifts it away from him, and the floor never touches it on the way.
///
/// The hero stands eight units off, which is outside the 5.072-unit `Hero
/// Range` circle `hk_sim::pigeon` carries, and then closes to two, which is
/// inside it. That circle is the constant `host/pigeon.py` proves each
/// placement's own child against, so this fixture is also what keeps the
/// recognizer's geometry and the runtime's the same number.
#[test]
fn a_pigeon_sits_until_the_hero_is_close_and_then_leaves_the_other_way() {
    let bytes = pigeon_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = pigeon_region();
    let mut w = enemies::EnemyWorld::new();
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    let before = v.health;
    // Outside the circle: it perches, draws, and never moves.
    let mut far = Player::spawn(8 * ONE, 4 * ONE);
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut far, &mut v, &n);
    }
    assert_eq!(w.actor_state(0, 1287965), Some((0, 4 * ONE, 1)));
    let perched = submitted(&w, &r, &room);
    assert_eq!(perched.len(), 1, "a perched bird is one quad");
    let facing_left = span(&perched[0].1);
    // Contact costs the hero nothing: the source object has no `DamageHero`.
    assert_eq!(v.health, before, "a Pigeon cannot hurt the hero");
    // Close in on the right. `CheckTargetDirection` answers that with `Left`.
    let mut near = Player::spawn(2 * ONE, 4 * ONE);
    tick(&mut w, &r, &room, &mut near, &mut v, &n);
    let (x, y, _) = w.actor_state(0, 1287965).expect("the bird is still seated");
    assert!(x < 0, "it leaves away from the hero, not towards him");
    // `Fly`'s `Translate` lifts it half a unit before the force does anything.
    assert!(y >= 4 * ONE + ONE / 2, "the takeoff lift is missing");
    // The mirror follows the flight direction, which reaches the drawn box.
    let flying = submitted(&w, &r, &room);
    assert_eq!(flying.len(), 1);
    assert_ne!(span(&flying[0].1), facing_left, "the flight mirror never reached the draw");
    // It accelerates the whole way out, and the floor at y=0 never stops it:
    // its one collider is a trigger, so there is no body to solve.
    let mut last = x;
    let mut held = 0;
    for _ in 0..90 {
        tick(&mut w, &r, &room, &mut near, &mut v, &n);
        let (x, y, _) = w.actor_state(0, 1287965).expect("still seated while it flies");
        assert!(x <= last, "it never turns round");
        assert!(y >= 4 * ONE + ONE / 2, "it never falls back through the floor");
        if x == last { held += 1; }
        last = x;
    }
    assert!(last < -20 * ONE, "a second and a half should clear the room, reached {last}");
    // And then it holds where it is, because it has left the fixture's own
    // collision bounds. That is the actor neighbourhood gate rather than
    // anything the controller does; see the recognizer's limitations.
    assert!(held > 0, "the gate outside the room never took hold");
    assert_eq!(v.health, before);
}

/// One nail hit is the whole fight, and nothing is left behind afterwards.
#[test]
fn a_pigeon_dies_to_one_hit_and_leaves_no_corpse() {
    let bytes = pigeon_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = pigeon_region();
    let mut w = enemies::EnemyWorld::new();
    let mut v = Vitals::new(VITAL_PARAMS);
    let mut nail = Nail::new();
    nail.active = true;
    nail.age = 1;
    // Standing in it, which is where a trigger body lets the hero stand.
    let mut hero = Player::spawn(0, 4 * ONE);
    let quiet = Nail::new();
    tick(&mut w, &r, &room, &mut hero, &mut v, &quiet);
    assert_eq!(submitted(&w, &r, &room).len(), 1);
    let events = tick(&mut w, &r, &room, &mut hero, &mut v, &nail);
    assert_eq!((events.hits, events.kills), (1, 1), "one hit point is one hit");
    // `EnemyDeathEffectsNoEffect` names no corpse prefab, so the body goes.
    assert!(submitted(&w, &r, &room).is_empty(), "a dead Pigeon leaves nothing");
    for _ in 0..60 {
        let events = tick(&mut w, &r, &room, &mut hero, &mut v, &nail);
        assert_eq!(events.hits, 0, "there is nothing left to hit");
    }
    assert!(submitted(&w, &r, &room).is_empty());
}

/// Gruz Mother at its cooked body box (host/false_knight_art.py's Gruz Mother
/// section: 3.52 x 1.52 at offset (0.148, -0.805), scale 1.25), serialized
/// invincible as the source is, with no DamageHero of its own.
const GRUZ: ActorSpec = ActorSpec {
    controller: ActorController::GruzMother,
    bounds: [-131840, -128000, 156160, -3840],
    health: EnemyParams { health: 90, contact_damage: 0, evasion_ticks: 12, invincible: true, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0, turn_clip: 0, corpse: None, recoil_speed: 0, recoil_ticks: 0, dream_soul: 0,
};
const GRUZ_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement { source_id: 4991, x: 0, y: 4 * ONE, ..PLACEMENT };
/// The seven reserve flies under `Fly Spawn` at (0, -30), each a unit's
/// fraction off it the way the source scatters them.
const RESERVE: ActorSpec = ActorSpec {
    controller: ActorController::GruzzerReserve { origin: [0, -30 * ONE] },
    bounds: [-26624, -29696, 31744, 25600],
    health: EnemyParams { health: 8, contact_damage: 1, evasion_ticks: 12, invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0, turn_clip: 0, corpse: None, recoil_speed: 15 * ONE, recoil_ticks: 9, dream_soul: 0,
};
const fn reserve(source_id: u32, dx: i32, dy: i32) -> (hk_sim::ActorPlacement, &'static ActorSpec) {
    (hk_sim::ActorPlacement { source_id, x: dx, y: -30 * ONE + dy, ..PLACEMENT }, &RESERVE)
}
const GRUZ_ACTORS: [(hk_sim::ActorPlacement, &ActorSpec); 8] = [
    (GRUZ_AT, &GRUZ),
    reserve(124, 19708, 21160), reserve(338, -23114, -78748), reserve(407, -59421, 203680),
    reserve(558, -62777, 9365), reserve(744, 13769, 267367), reserve(846, -76408, 100046), reserve(960, 27310, 126963),
];
fn gruz_region() -> world::Region {
    world::Region {
        scene: 0,
        bounds: [-60 * ONE, -5 * ONE, 60 * ONE, 40 * ONE],
        collision_bounds: [-64 * ONE, -10 * ONE, 64 * ONE, 44 * ONE],
        actors: &GRUZ_ACTORS,
    }
}
/// A wide floor at y 0 and a ceiling at y 16, so a slam has both to bounce
/// between and the burster lands wherever it is flung.
fn gruz_room() -> Vec<u8> {
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 1, 0, 1, 1, 2, 0, 0] {
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
    for n in [-60 * ONE, 0, 60 * ONE, 0, 60 * ONE, 16 * ONE, -60 * ONE, 16 * ONE] {
        b.extend(n.to_le_bytes());
    }
    b.resize(b.len() + 32 + 32768, 0);
    b
}
/// The hero on the actor, one nail frame, then the evasion window out.
fn swing_at(w: &mut enemies::EnemyWorld, r: &world::Region, room: &hk_format::Room, source_id: u32) -> enemies::Events {
    let (x, y, _) = w.actor_state(0, source_id).unwrap();
    let mut p = Player::spawn(x, y);
    let mut n = Nail::new();
    n.active = true;
    n.age = 1;
    let events = tick(w, r, room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    n.active = false;
    for _ in 0..16 {
        tick(w, r, room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    }
    events
}
#[test]
fn gruz_mother_sleeps_until_hit_in_range_and_wakes_into_a_sealed_arena() {
    let bytes = gruz_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = gruz_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(40 * ONE, ONE);
    let n = Nail::new();
    for _ in 0..120 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    }
    // Asleep: nothing moves it, the arena is untouched, the reserve is parked.
    assert_eq!(w.actor_state(0, GRUZ_AT.source_id).unwrap(), (0, 4 * ONE, 90));
    assert_eq!(w.actor_state(0, 124).unwrap(), (19708, -30 * ONE + 21160, 8));
    battle_gates::EVENTS.with(|e| assert_eq!(*e.borrow(), [("placement", 0)]));
    // The hero inside `Battle Range` hits it: damage, and the wake.
    let events = swing_at(&mut w, &r, &room, GRUZ_AT.source_id);
    assert_eq!(events.hits, 1);
    let (_, y, hp) = w.actor_state(0, GRUZ_AT.source_id).unwrap();
    assert_eq!(hp, 90 - cheats::Settings::new().params(VITAL_PARAMS).nail_damage as i16);
    assert!(y > 4 * ONE, "`Wake` lifts it at 2.5 a second");
    battle_gates::EVENTS.with(|e| assert!(e.borrow().contains(&("close", 0)), "BATTLE START seals the arena"));
    // Awake it flies: within a few seconds it has left where it slept.
    let mut moved = false;
    for _ in 0..240 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        let (x, y, _) = w.actor_state(0, GRUZ_AT.source_id).unwrap();
        assert!((-60 * ONE..=60 * ONE).contains(&x) && (ONE..=17 * ONE).contains(&y), "inside the room: {x} {y}");
        moved |= (x - 0).abs() > ONE;
    }
    assert!(moved);
    assert_eq!(w.actor_state(0, 124).unwrap(), (19708, -30 * ONE + 21160, 8), "the reserve waits for the burster");
}
#[test]
fn the_burster_releases_the_reserve_whose_deaths_open_the_arena() {
    let bytes = gruz_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = gruz_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(40 * ONE, ONE);
    let n = Nail::new();
    tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    for _ in 0..40 {
        if w.actor_state(0, GRUZ_AT.source_id).unwrap().2 <= 0 { break; }
        swing_at(&mut w, &r, &room, GRUZ_AT.source_id);
    }
    let (dx, dy, hp) = w.actor_state(0, GRUZ_AT.source_id).unwrap();
    assert!(hp <= 0, "killed");
    persist::STORE.with(|s| assert!(s.borrow().get(persist::Kind::BattleScene, 0, 0).is_some(),
        "the corpse's `Init` writes `Activated`"));
    // Steam and Blow, then the burster lands and gurgles; every reserve fly
    // stays parked until `Spawn Flies 2`.
    let mut released = None;
    for t in 0..1400 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
        let (x, y, _) = w.actor_state(0, 124).unwrap();
        if y != -30 * ONE + 21160 {
            released = Some((t, x, y));
            break;
        }
    }
    let (t, x, y) = released.expect("the burster released the reserve");
    assert!(t > 270, "not before the corpse has blown ({t})");
    let (bx, by, _) = w.actor_state(0, GRUZ_AT.source_id).unwrap();
    assert!((bx, by) != (dx, dy), "the burster was flung from where the body died");
    assert!(by.abs() < 2 * ONE, "and came to rest on the floor: {by}");
    // The first tick after the release moved it at most one step from the
    // burster plus its own offset from `Fly Spawn`.
    assert!((x - (bx + 19708)).abs() < ONE / 4 && (y - (by + 21160)).abs() < ONE / 4, "{x} {y} vs {bx} {by}");
    battle_gates::EVENTS.with(|e| assert!(!e.borrow().contains(&("open", 0))));
    // Seven `Battle Enemies`: the gates open two seconds after the last.
    for &(placement, _) in &GRUZ_ACTORS[1..] {
        for _ in 0..4 {
            if w.actor_state(0, placement.source_id).unwrap().2 <= 0 { break; }
            swing_at(&mut w, &r, &room, placement.source_id);
        }
        assert!(w.actor_state(0, placement.source_id).unwrap().2 <= 0);
    }
    for _ in 0..130 {
        tick(&mut w, &r, &room, &mut p, &mut Vitals::new(VITAL_PARAMS), &n);
    }
    battle_gates::EVENTS.with(|e| assert!(e.borrow().contains(&("open", 0)), "BG OPEN after End Wait"));
}

/// Acid Flyer (4) of Fungus1_09: Move Vector -2 at Speed 4, so half a second
/// each way after the half-second wait. Boxes as host/vengefly.py cooks them:
/// the body (mirrors with facing) and the detached Shell (never mirrors).
const ACID: ActorSpec = ActorSpec {
    controller: ActorController::AcidFlyer { amount: 2 * ONE, speed: 4 * ONE, lead: [0, 0],
        shell: [-62317, -65502, 49247, 47390] },
    bounds: [-61643, -66571, -6121, 8729],
    health: EnemyParams { health: 30, contact_damage: 1, evasion_ticks: 12, invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0, turn_clip: 0, corpse: None, recoil_speed: 0, recoil_ticks: 9, dream_soul: 0,
};
const ACID_AT: hk_sim::ActorPlacement = hk_sim::ActorPlacement { source_id: 1811, x: 0, y: 4 * ONE, ..PLACEMENT };
const ACID_ACTORS: [(hk_sim::ActorPlacement, &ActorSpec); 1] = [(ACID_AT, &ACID)];
fn acid_region() -> world::Region {
    world::Region { scene: 0, bounds: [-30 * ONE, -5 * ONE, 30 * ONE, 30 * ONE],
        collision_bounds: [-34 * ONE, -10 * ONE, 34 * ONE, 34 * ONE], actors: &ACID_ACTORS }
}
fn acid_tick(w: &mut enemies::EnemyWorld, r: &world::Region, room: &hk_format::Room, p: &mut Player,
             n: &Nail, response: &mut NailResponse) -> enemies::Events {
    w.tick(r, room, &mut world::State, p, &mut Vitals::new(VITAL_PARAMS), n, &hk_sim::DreamNail::new(), None, response,
        no_attack(), [&[[-ONE, -2 * ONE], [ONE, -2 * ONE], [ONE, ONE], [-ONE, ONE]]; 4], cheats::Settings::new(),
        [0, 0, -2496922], |_| panic!("Acid Flyer emitted a Runner event"), |_, _, _, _| None)
}
#[test]
fn an_acid_flyer_waits_then_bobs_one_full_cycle() {
    let bytes = gruz_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = acid_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, 20 * ONE);
    let quiet = Nail::new();
    let mut highest = 0;
    // Wait 30, up 30, down 30: a full cycle back where it started.
    for t in 0..90 {
        acid_tick(&mut w, &r, &room, &mut p, &quiet, &mut NailResponse::new());
        let (x, y, _) = w.actor_state(0, ACID_AT.source_id).unwrap();
        assert_eq!(x, 0, "the tween is vertical");
        if t < 29 { assert_eq!(y, 4 * ONE, "Init's half-second wait"); }
        highest = highest.max(y);
    }
    assert_eq!(highest, 6 * ONE, "the full Move Vector at the top of the tween");
    assert_eq!(w.actor_state(0, ACID_AT.source_id).unwrap().1, 4 * ONE, "Reset Pos after a full cycle");
}
#[test]
fn only_a_side_slash_hurts_it_and_a_pogo_off_the_body_bounces_high() {
    let bytes = gruz_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = acid_region();
    let mut nail = Nail::new();
    nail.active = true;
    nail.age = 1;
    for (kind, hits, bounce) in [(0u16, 1, 0), (2, 0, 0), (3, 0, 17)] {
        let mut w = enemies::EnemyWorld::new();
        let mut p = Player::spawn(0, 4 * ONE);
        acid_tick(&mut w, &r, &room, &mut p, &Nail::new(), &mut NailResponse::new());
        nail.kind = kind;
        let mut response = NailResponse::new();
        let mut p = Player::spawn(0, 4 * ONE);
        let events = acid_tick(&mut w, &r, &room, &mut p, &nail, &mut response);
        assert_eq!(events.hits, hits, "slash kind {kind}");
        assert_eq!(response.bounce_left, bounce, "slash kind {kind}");
    }
}
#[test]
fn the_shell_above_the_body_takes_an_ordinary_pogo_and_no_damage() {
    let bytes = gruz_room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = acid_region();
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(0, 20 * ONE);
    acid_tick(&mut w, &r, &room, &mut p, &Nail::new(), &mut NailResponse::new());
    let mut nail = Nail::new();
    nail.active = true;
    nail.age = 1;
    nail.kind = 3;
    // The slash's bottom edge at y 4.4: inside the Shell (top 4.72), clear of
    // the body (top 4.13).
    let mut p = Player::spawn(0, 6 * ONE + 26214);
    let mut response = NailResponse::new();
    let events = acid_tick(&mut w, &r, &room, &mut p, &nail, &mut response);
    assert_eq!(events.hits, 0);
    assert_eq!(response.bounce_left, VITAL_BOUNCE_TICKS);
    assert_eq!(w.actor_state(0, ACID_AT.source_id).unwrap().2, 30);
}
const VITAL_BOUNCE_TICKS: u16 = NAIL_RESPONSE_PARAMS.bounce_ticks;

#[test]
fn an_fsm_activator_enemy_waits_for_the_cameras_active_region() {
    static AWAITING: hk_sim::ActorPlacement = hk_sim::ActorPlacement { fsm_activator: true, random_start_direction: false, ..PLACEMENT };
    static ACTORS: [(hk_sim::ActorPlacement, &ActorSpec); 1] = [(AWAITING, &SPEC)];
    let bytes = room();
    let room = hk_format::Room::parse(&bytes).unwrap();
    let r = world::Region { actors: &ACTORS, ..region() };
    let mut w = enemies::EnemyWorld::new();
    let mut p = Player::spawn(10 * ONE, ONE);
    let mut v = Vitals::new(VITAL_PARAMS);
    let n = Nail::new();
    let start = {
        tick_camera(&mut w, &r, &room, &mut p, &mut v, &n, [0, 0, -2496922]);
        w.actor_state(0, 12546).unwrap().0
    };
    // The camera is 60 units away: the 50 wide ActiveRegion stops 35 units short of the enemy.
    let mut w = enemies::EnemyWorld::new();
    for _ in 0..120 {
        tick_camera(&mut w, &r, &room, &mut p, &mut v, &n, [60 * ONE, 0, -2496922]);
    }
    assert_eq!(w.actor_state(0, 12546).unwrap().0, PLACEMENT.x, "it must not move before the region reaches it");
    // The region reaches it: from then on it patrols, and stays active if the camera leaves again.
    for _ in 0..5 {
        tick_camera(&mut w, &r, &room, &mut p, &mut v, &n, [20 * ONE, 0, -2496922]);
    }
    let woken = w.actor_state(0, 12546).unwrap().0;
    assert_ne!(woken, PLACEMENT.x);
    assert!(start != PLACEMENT.x);
    for _ in 0..30 {
        tick_camera(&mut w, &r, &room, &mut p, &mut v, &n, [200 * ONE, 0, -2496922]);
    }
    assert_ne!(w.actor_state(0, 12546).unwrap().0, woken, "an activated enemy keeps running");
}
