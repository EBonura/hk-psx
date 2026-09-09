use hk_sim::{Corpse, CorpsePhase, CorpseSpec, ONE};
const SPEC: CorpseSpec = CorpseSpec {
    air_clip: 13,
    land_clip: 14,
    bounds: [-46080, -55296, 47104, 4096],
    spawn_offset: [0, 32768],
};
fn floor(_: usize) -> [i32; 4] {
    [-100 * ONE, 0, 100 * ONE, 0]
}
#[test]
fn source_launch_cardinals_and_animation_switch_are_bounded() {
    let right = Corpse::spawn(SPEC, 0, 2 * ONE, 0, 1, 12546);
    let left = Corpse::spawn(SPEC, 0, 2 * ONE, 0, -1, 12546);
    assert_eq!((right.vx, right.vy), (491520, 851338));
    assert_eq!((left.vx, left.vy), (-491520, 851338));
    assert_eq!(right.y, 2 * ONE + 32768);
    assert_eq!(right.clip(SPEC), 13);
    let down = Corpse::spawn(SPEC, 0, 2 * ONE, 3, 1, 12546);
    assert_eq!((down.vx, down.vy), (0, -15 * ONE));
    for seed in 0..1000 {
        let up = Corpse::spawn(SPEC, 0, 2 * ONE, 2, 1, seed);
        assert!(up.vx.abs() <= 330765);
        assert!(up.vy >= 1234360 && up.vy <= 1277952);
    }
}
#[test]
fn landed_corpse_keeps_original_final_sprite_after_one_second_and_long_wait() {
    let mut c = Corpse::spawn(SPEC, 0, 2 * ONE, 0, 1, 12546);
    for _ in 0..300 {
        c.tick(SPEC, 1, floor);
        if c.phase == CorpsePhase::Land {
            break;
        }
    }
    assert_eq!(c.phase, CorpsePhase::Land);
    assert_eq!(c.animation_tick, 0);
    assert_eq!(c.clip(SPEC), 14);
    for _ in 0..59 {
        c.tick(SPEC, 1, floor);
    }
    assert_eq!(c.phase, CorpsePhase::Land);
    c.tick(SPEC, 1, floor);
    assert_eq!(c.phase, CorpsePhase::Rest);
    for _ in 0..3600 {
        c.tick(SPEC, 1, floor);
    }
    assert!(c.visible());
    assert_eq!(c.clip(SPEC), 14);
    assert!(c.vx.abs() < ONE);
    assert!((c.y + SPEC.bounds[1]).abs() < ONE / 10);
}
#[test]
fn unsupported_void_does_not_create_a_floor_and_source_air_limit_removes() {
    let mut c = Corpse::spawn(SPEC, 0, 0, 3, 1, 12546);
    for _ in 0..120 {
        c.tick(SPEC, 0, |_| panic!());
    }
    assert_eq!(c.phase, CorpsePhase::Removed);
    assert!(!c.visible());
}
#[test]
fn identical_source_seed_and_edges_produce_identical_physics() {
    let mut a = Corpse::spawn(SPEC, 0, 2 * ONE, 2, 1, 12547);
    let mut b = a;
    for _ in 0..300 {
        a.tick(SPEC, 1, floor);
        b.tick(SPEC, 1, floor);
        assert_eq!(
            (a.x, a.y, a.vx, a.vy, a.phase),
            (b.x, b.y, b.vx, b.vy, b.phase)
        );
    }
    assert!(core::mem::size_of::<Corpse>() <= 48);
}
