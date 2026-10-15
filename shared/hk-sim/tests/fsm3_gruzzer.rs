//! The Gruzzer on HKFSM03 against the Gruzzer on HKFSM01, call by call.
use hk_sim::gruzzer as v1;
use hk_sim::gruzzer3 as v2;
use hk_sim::ONE;

const CAMERA: [i32; 3] = [0, 0, -2496922];

fn side1(i: u32) -> v1::Side {
    [v1::Side::Up, v1::Side::Right, v1::Side::Down, v1::Side::Left][(i % 4) as usize]
}
fn side2(i: u32) -> v2::Side {
    [v2::Side::Up, v2::Side::Right, v2::Side::Down, v2::Side::Left][(i % 4) as usize]
}
fn same(a: &v1::Gruzzer, b: &v2::Gruzzer, at: &str) {
    assert_eq!(a.angle(), b.angle(), "angle {at}");
    assert_eq!(a.facing(), b.facing(), "facing {at}");
    let phase1 = match a.phase() {
        v1::Phase::Waiting => 0,
        v1::Phase::Flying => 1,
        v1::Phase::Dead => 2,
    };
    let phase2 = match b.phase() {
        v2::Phase::Waiting => 0,
        v2::Phase::Flying => 1,
        v2::Phase::Dead => 2,
    };
    assert_eq!(phase1, phase2, "phase {at}");
}

#[test]
fn waits_for_the_camera_then_aims_anywhere() {
    let mut g = v2::Gruzzer::new(5);
    g.tick(CAMERA, [30 * ONE, 0, 0]);
    assert_eq!(g.phase(), v2::Phase::Waiting);
    assert_eq!(g.facing(), -1);
    g.tick(CAMERA, [10 * ONE, 5 * ONE, 0]);
    assert_eq!(g.phase(), v2::Phase::Flying);
    assert!((0..360 * ONE).contains(&g.angle()));
}

#[test]
fn every_seed_and_call_order_matches_v1() {
    for seed in 0..200u32 {
        let mut a = v1::Gruzzer::new(seed);
        let mut b = v2::Gruzzer::new(seed);
        let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
        for step in 0..400u32 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            match (x >> 24) % 5 {
                0 | 1 => {
                    // Positions on both sides of the 44 unit camera range, away from its band edge.
                    let d = ((x >> 8) % 60) as i32;
                    let p = [d * ONE, ((x >> 4) % 7) as i32 * ONE - 3 * ONE, 0];
                    a.tick(CAMERA, p);
                    b.tick(CAMERA, p);
                }
                2 | 3 => {
                    a.bonk(side1(x >> 12));
                    b.bonk(side2(x >> 12));
                }
                _ => {
                    a.tick(CAMERA, [0, 0, 0]);
                    b.tick(CAMERA, [0, 0, 0]);
                }
            }
            same(&a, &b, &format!("seed {seed} step {step}"));
        }
    }
}

#[test]
fn bonks_re_aim_from_the_authored_ranges() {
    let deg = |a: i32| a / ONE;
    for seed in 0..64u32 {
        let mut g = v2::Gruzzer::new(seed);
        g.tick(CAMERA, [0, 0, 0]);
        g.bonk(v2::Side::Down);
        let a = deg(g.angle());
        let right = g.facing() == 1;
        assert!(if right { (10..=40).contains(&a) } else { (140..=170).contains(&a) }, "{a} {right}");
        g.bonk(v2::Side::Up);
        let a = deg(g.angle());
        assert!(if right { (320..=350).contains(&a) } else { (190..=220).contains(&a) }, "{a} {right}");
        g.bonk(v2::Side::Right);
        assert!((190..=220).contains(&deg(g.angle())));
        g.bonk(v2::Side::Left);
        assert!((320..=350).contains(&deg(g.angle())));
        g.bonk(v2::Side::Down);
        g.bonk(v2::Side::Right);
        assert!((140..=170).contains(&deg(g.angle())));
        g.bonk(v2::Side::Down);
        g.bonk(v2::Side::Left);
        assert!((10..=40).contains(&deg(g.angle())));
        assert_eq!(g.facing(), -1);
        g.tick(CAMERA, [0, 0, 0]);
        assert_eq!(g.facing(), 1);
    }
}

#[test]
fn dead_and_waiting_ignore_bonks() {
    let mut g = v2::Gruzzer::new(1);
    g.bonk(v2::Side::Up);
    assert_eq!(g.angle(), 0);
    g.tick(CAMERA, [0, 0, 0]);
    g.die();
    let before = g.angle();
    g.bonk(v2::Side::Left);
    assert_eq!((g.phase(), g.angle()), (v2::Phase::Dead, before));
}
