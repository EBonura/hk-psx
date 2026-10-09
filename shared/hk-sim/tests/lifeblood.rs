#[path = "../../../game/src/lifeblood.rs"]
mod lifeblood;
use hk_sim::{Hurt, VitalParams, Vitals, ONE};
use lifeblood::*;
const SPEC: Spec = Spec {
    scene: 0,
    origin: [0, 3 * ONE],
    bounds: [-ONE, 2 * ONE, ONE, 4 * ONE],
    fling_speed: [0, 0],
    fling_angle: [90 * ONE, 90 * ONE],
    spread: [0, 0],
    scale: [ONE, ONE],
    speed: [6 * ONE, 9 * ONE],
    body: [-ONE / 2, -ONE / 2, ONE / 2, ONE / 2],
    gravity: 36 * ONE,
    acceleration: 19661,
    activate_ticks: 15,
    heal_ticks: 72,
    land_ticks: 15,
    bounce_ticks: 30,
};
const COVER: [i32; 4] = [-100 * ONE, -100 * ONE, 100 * ONE, 100 * ONE];
const POLY: [[i32; 2]; 4] = [
    [-ONE, 2 * ONE],
    [ONE, 2 * ONE],
    [ONE, 4 * ONE],
    [-ONE, 4 * ONE],
];
fn tick(w: &mut World, spec: Spec) -> u16 {
    w.tick_with(spec, 0, 0, COVER, 1, |_| [-100 * ONE, 0, 100 * ONE, 0])
}
#[test]
fn opening_once_emits_exactly_two_bugs_no_contact_health_and_startup_hit_lock() {
    let p = Spec { gravity: 0, ..SPEC };
    let mut w = World::new();
    assert_eq!(w.strike_with(p, 1, &POLY), Strike::default());
    assert!(!w.opened());
    assert_eq!(
        w.strike_with(p, 0, &POLY),
        Strike {
            opened: true,
            hit_bugs: 0
        }
    );
    assert_eq!(w.bugs.iter().flatten().count(), 2);
    for _ in 0..14 {
        assert_eq!(tick(&mut w, p), 0);
        assert_eq!(w.strike_with(p, 0, &POLY), Strike::default());
    }
    tick(&mut w, p);
    assert_eq!(
        w.strike_with(p, 0, &POLY),
        Strike {
            opened: false,
            hit_bugs: 2
        }
    );
    assert_eq!(w.strike_with(p, 0, &POLY), Strike::default());
    assert_eq!(w.granted, 0);
    for _ in 0..71 {
        assert_eq!(tick(&mut w, p), 0);
    }
    assert_eq!(tick(&mut w, p), 2);
    assert!(w.bugs.iter().all(Option::is_none));
    assert_eq!(w.granted, 2);
    assert_eq!(tick(&mut w, p), 0);
}
#[test]
fn scene_unload_grants_pending_once_without_awarding_unstruck_bugs() {
    let p = Spec { gravity: 0, ..SPEC };
    let mut w = World::new();
    w.strike_with(p, 0, &POLY);
    for _ in 0..15 {
        tick(&mut w, p);
    }
    // Separate the bugs and strike only the first.
    w.bugs[1].as_mut().unwrap().body.x = 10 * ONE;
    assert_eq!(w.strike_with(p, 0, &POLY).hit_bugs, 1);
    assert_eq!(w.leave_scene(), 1);
    assert_eq!(w.leave_scene(), 0);
    assert!(w.opened());
    assert_eq!(w.strike_with(p, 0, &POLY).opened, false);
    w.reset();
    assert!(!w.opened());
    assert_eq!(w.granted, 0);
    assert!(w.strike_with(p, 0, &POLY).opened);
}
#[test]
fn overlapping_views_do_not_reset_lifecycle_and_offscreen_pending_heal_continues() {
    let p = Spec { gravity: 0, ..SPEC };
    let mut w = World::new();
    w.strike_with(p, 0, &POLY);
    for _ in 0..15 {
        w.tick_with(
            p,
            0,
            0,
            [100 * ONE, 100 * ONE, 110 * ONE, 110 * ONE],
            0,
            |_| [0; 4],
        );
    }
    assert_eq!(w.strike_with(p, 0, &POLY).hit_bugs, 2);
    for _ in 0..71 {
        assert_eq!(w.tick_with(p, 0, 0, [0; 4], 0, |_| [0; 4]), 0);
    }
    assert_eq!(w.tick_with(p, 0, 0, [0; 4], 0, |_| [0; 4]), 2);
}
#[test]
fn authored_floor_landing_animation_then_runs_away_from_hero() {
    let mut w = World::new();
    w.strike_with(SPEC, 0, &POLY);
    for _ in 0..180 {
        w.tick_with(SPEC, 0, 10 * ONE, COVER, 1, |_| {
            [-100 * ONE, 0, 100 * ONE, 0]
        });
    }
    for b in w.bugs.iter().flatten() {
        assert_eq!(b.phase, Phase::Run);
        assert!(b.body.grounded);
        assert_eq!(b.body.y, ONE / 2);
        assert!(b.vx < 0 && b.body.x < 0);
        assert!(b.vx >= -b.max_speed);
    }
}
#[test]
fn wall_contact_launches_source_bounce_then_resumes_run() {
    let mut w = World::new();
    w.strike_with(SPEC, 0, &POLY);
    let b = w.bugs[0].as_mut().unwrap();
    b.body.x = 0;
    b.body.y = ONE / 2;
    b.body.grounded = true;
    b.vx = 6 * ONE;
    b.phase = Phase::Run;
    b.direction = -1;
    let edges = [[-100 * ONE, 0, 100 * ONE, 0], [ONE, 0, ONE, 10 * ONE]];
    for _ in 0..8 {
        w.tick_with(SPEC, 0, -10 * ONE, COVER, 2, |i| edges[i]);
    }
    let b = w.bugs[0].unwrap();
    assert_eq!(b.phase, Phase::Bounce);
    assert!(b.vx < 0 && b.body.vy > 0);
    assert!(b.timer <= 30);
}
#[test]
fn source_fling_is_deterministic_and_matches_authored_ranges() {
    let p = Spec {
        origin: [3733905, 4069151],
        fling_speed: [10 * ONE, 15 * ONE],
        fling_angle: [40 * ONE, 140 * ONE],
        spread: [ONE / 2; 2],
        scale: [88474, 98304],
        ..SPEC
    };
    let poly = [
        [p.bounds[0], p.bounds[1]],
        [p.bounds[2], p.bounds[1]],
        [p.bounds[2], p.bounds[3]],
        [p.bounds[0], p.bounds[3]],
    ];
    let mut a = World::new();
    let mut b = World::new();
    a.strike_with(p, 0, &poly);
    b.strike_with(p, 0, &poly);
    for (a, b) in a.bugs.iter().flatten().zip(b.bugs.iter().flatten()) {
        assert_eq!(a.body, b.body);
        assert_eq!(a.vx, b.vx);
        assert!((p.scale[0]..=p.scale[1]).contains(&a.scale));
        assert!(a.body.vy > 0);
        assert!((a.body.x - p.origin[0]).abs() <= ONE / 2);
    }
}
#[test]
fn earned_two_masks_absorb_damage_before_normal_and_focus_does_not_restore_them() {
    let p = Spec { gravity: 0, ..SPEC };
    let mut w = World::new();
    w.strike_with(p, 0, &POLY);
    for _ in 0..15 {
        tick(&mut w, p);
    }
    w.strike_with(p, 0, &POLY);
    let vp = VitalParams {
        max_health: 5,
        max_soul: 99,
        nail_damage: 5,
        soul_per_hit: 11,
        invulnerable_ticks: 0,
        hazard_invulnerable_ticks: 0,
        recoil_ticks: 0,
        freeze_ticks: 0,
        death_ticks: 171,
        recoil_speed: 0,
    };
    let mut v = Vitals::new(vp);
    for _ in 0..72 {
        v.add_blue_health(tick(&mut w, p));
    }
    assert_eq!((v.health, v.blue_health), (5, 2));
    assert_eq!(v.hurt(vp, 1, 1, false), Hurt::Recoiling);
    assert_eq!((v.health, v.blue_health), (5, 1));
    v.hurt(vp, 2, 1, false);
    assert_eq!((v.health, v.blue_health), (4, 0));
    v.heal(vp, 1);
    assert_eq!((v.health, v.blue_health), (5, 0));
}

#[test]
fn source_splat_last_blank_frame_retires_after15ticks_even_outside_coverage() {
    let p = Spec { gravity: 0, ..SPEC };
    let mut w = World::new();
    assert!(!w.splat_visible());
    w.strike_with(p, 0, &POLY);
    assert!(w.splat_visible());
    for _ in 0..14 {
        w.tick_with(
            p,
            0,
            0,
            [100 * ONE, 100 * ONE, 110 * ONE, 110 * ONE],
            0,
            |_| [0; 4],
        );
        assert!(w.splat_visible());
    }
    w.tick_with(p, 0, 0, [0; 4], 0, |_| [0; 4]);
    assert!(!w.splat_visible());
    assert!(w.opened());
    w.reset();
    assert!(!w.splat_visible());
    w.strike_with(p, 0, &POLY);
    assert!(w.splat_visible());
    w.leave_scene();
    assert!(!w.splat_visible());
    assert!(w.opened());
}
#[test]
fn world_stays_within_its_ram_budget() {
    // The bugs embed a Player each, purely for its collision. Player carries the
    // hero's ability state too, which P14 and P15 grew from 40 to 80 bytes, so
    // this budget moved with it. If it needs raising again, split the hero-only
    // fields out of Player instead: 32 actors, the corpses and these bugs all pay
    // for state only the Knight ever reads.
    assert!(
        core::mem::size_of::<World>() <= 384,
        "lifeblood World is {} bytes",
        core::mem::size_of::<World>()
    );
}
