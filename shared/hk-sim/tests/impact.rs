#![allow(dead_code)] // includes game modules by path and exercises part of each
use hk_sim::ONE;
const KNIGHT_SCALE: i32 = 60693;
mod render {
    pub fn texture(_: usize, _: [(i16, i16); 4], _: (u8, u8, u8)) {}
}
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/impact.rs"]
mod impact;
fn room() -> Vec<u8> {
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 1, 0, 1, 2, 0, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u16, 0, 0, 4, 4, 0] {
        b.extend(n.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    for n in [0i32, -ONE / 2, -ONE / 2, ONE / 2, ONE / 2] {
        b.extend(n.to_le_bytes());
    }
    for _ in 0..2 {
        for n in [0u32, 1, 30 * 65536, 2] {
            b.extend(n.to_le_bytes());
        }
    }
    b.resize(b.len() + 32 + 32768, 0);
    b
}
const SPEC: impact::Spec = impact::Spec {
    clips: [0, 1],
    ticks: 10,
};
#[test]
fn original_clip_duration_expires_without_fake_particles_or_repeat() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut pool = impact::Pool::new();
    assert!(pool.spawn(0, 3, [-ONE, -ONE, ONE, ONE], [-ONE, -ONE, ONE, ONE], 1));
    for _ in 0..10 {
        assert_eq!(pool.draw(0, Some(SPEC), &r, (0, 0)), 1);
        pool.tick();
    }
    assert_eq!(pool.draw(0, Some(SPEC), &r, (0, 0)), 0);
    assert_eq!(pool.active(), 0);
}
#[test]
fn residency_does_not_restart_and_true_scene_reset_clears() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut p = impact::Pool::new();
    p.spawn(0, 2, [0, 0, ONE, ONE], [0, 0, ONE, ONE], -1);
    for _ in 0..4 {
        p.tick();
    }
    assert_eq!(p.draw(1, Some(SPEC), &r, (0, 0)), 0);
    assert_eq!(p.active(), 1);
    assert_eq!(p.draw(0, None, &r, (0, 0)), 0);
    assert_eq!(p.active(), 1);
    assert_eq!(p.draw(0, Some(SPEC), &r, (0, 0)), 1);
    p.clear_scene(1);
    assert_eq!(p.active(), 1);
    p.clear_scene(0);
    assert_eq!(p.active(), 0);
}
#[test]
fn full_pool_reports_overflow_preserves_existing_effects_and_reuses_expired_slots() {
    let mut p = impact::Pool::new();
    for i in 0..impact::CAPACITY {
        assert!(p.spawn(0, i, [0; 4], [0; 4], 1));
    }
    assert!(!p.spawn(0, 99, [0; 4], [0; 4], 1));
    assert_eq!(p.dropped, 1);
    assert_eq!(p.active(), 32);
    for _ in 0..10 {
        p.tick();
    }
    assert!(p.spawn(0, 99, [0; 4], [0; 4], 1));
    assert_eq!(p.active(), 1);
    assert!(core::mem::size_of::<impact::Pool>() <= 1100);
}
#[test]
fn offscreen_effects_are_culled_without_losing_lifetime() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut p = impact::Pool::new();
    p.spawn(
        0,
        0,
        [100 * ONE, 0, 101 * ONE, ONE],
        [100 * ONE, 0, 101 * ONE, ONE],
        1,
    );
    assert_eq!(p.draw(0, Some(SPEC), &r, (0, 0)), 0);
    assert_eq!(p.draw(0, Some(SPEC), &r, (100 * ONE, 0)), 1);
}
