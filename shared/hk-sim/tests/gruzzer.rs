use hk_sim::gruzzer::*;
use hk_sim::ONE;

const CAMERA: [i32; 3] = [0, 0, -2496922]; // z about -38.1 units

#[test]
fn waits_for_the_camera_then_aims_anywhere() {
    let mut g = Gruzzer::new(5);
    g.tick(CAMERA, [30 * ONE, 0, 0]); // planar 30 > sqrt(44^2 - 38.1^2) = 22
    assert_eq!(g.phase(), Phase::Waiting);
    assert_eq!(g.facing(), -1);
    g.tick(CAMERA, [10 * ONE, 5 * ONE, 0]);
    assert_eq!(g.phase(), Phase::Flying);
    assert!((0..360 * ONE).contains(&g.angle()));
}

#[test]
fn bonks_re_aim_from_the_authored_ranges() {
    let deg = |a: i32| a / ONE;
    for seed in 0..64u32 {
        let mut g = Gruzzer::new(seed);
        g.tick(CAMERA, [0, 0, 0]);
        // Force a known facing through a floor bonk from each facing half.
        g.bonk(Side::Down);
        let a = deg(g.angle());
        let right = g.facing() == 1;
        assert!(
            if right {
                (10..=40).contains(&a)
            } else {
                (140..=170).contains(&a)
            },
            "{a} {right}"
        );
        g.bonk(Side::Up);
        let a = deg(g.angle());
        assert!(
            if right {
                (320..=350).contains(&a)
            } else {
                (190..=220).contains(&a)
            },
            "{a} {right}"
        );
        // Now heading down (angle >= 180): a wall bonk sends it down-away.
        g.bonk(Side::Right);
        assert!((190..=220).contains(&deg(g.angle())));
        g.bonk(Side::Left);
        assert!((320..=350).contains(&deg(g.angle())));
        // Heading up after a floor bonk: wall bonks send it up-away.
        g.bonk(Side::Down);
        g.bonk(Side::Right);
        assert!((140..=170).contains(&deg(g.angle())));
        g.bonk(Side::Down);
        g.bonk(Side::Left);
        assert!((10..=40).contains(&deg(g.angle())));
        assert_eq!(g.facing(), 1);
    }
}

#[test]
fn dead_and_waiting_ignore_bonks_and_seeds_are_deterministic() {
    let mut g = Gruzzer::new(1);
    g.bonk(Side::Up);
    assert_eq!(g.angle(), 0);
    let mut a = Gruzzer::new(9);
    let mut b = Gruzzer::new(9);
    a.tick(CAMERA, [0, 0, 0]);
    b.tick(CAMERA, [0, 0, 0]);
    assert_eq!(a, b);
    a.die();
    let before = a.angle();
    a.bonk(Side::Left);
    assert_eq!((a.phase(), a.angle()), (Phase::Dead, before));
}
