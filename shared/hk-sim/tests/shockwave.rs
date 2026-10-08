//! The False Knight's slam wave (`hk_sim::shockwave`), against a flat floor.
use hk_sim::shockwave::{Params, Wave};
use hk_sim::ONE;

/// host/false_knight_art.py's reading of `Shockwave Wave` and `Shockwave Spurt`
/// (sharedassets48.assets:63 and :68), as data/false_knight_art.rs carries it.
const P: Params = Params {
    start_speed: 36045, accel: 44 * ONE, wave_box: [-37090, 21182, 5971, 115753],
    ground_ray: 104858, spurt_box: [-16352, -118, 10151, 114151],
    damage_from: 3, damage_to: 6, damage: 1, spurt_ticks: 18,
};
/// A floor at y 0 from -40 to 40 with a wall at x 20, and the wave where
/// `S Attack Recover` puts it: a fifth of a unit inside the floor.
const EDGES: [[i32; 4]; 2] = [[-40 * ONE, 0, 40 * ONE, 0], [20 * ONE, -5 * ONE, 20 * ONE, 10 * ONE]];
const SPAWN: [i32; 2] = [0, -13107];

/// Closed segment intersection, as `enemies.rs`'s `segment_hits_terrain`.
fn crosses(a: [i32; 2], b: [i32; 2], e: [i32; 4]) -> bool {
    let p = |x: i32, y: i32| (x as i64, y as i64);
    let (a, b, c, d) = (p(a[0], a[1]), p(b[0], b[1]), p(e[0], e[1]), p(e[2], e[3]));
    let side = |o: (i64, i64), u: (i64, i64), v: (i64, i64)| ((u.0 - o.0) * (v.1 - o.1) - (u.1 - o.1) * (v.0 - o.0)).signum();
    side(a, b, c) * side(a, b, d) <= 0 && side(c, d, a) * side(c, d, b) <= 0
}
fn hits<'a>(edges: &'a [[i32; 4]]) -> impl Fn([i32; 2], [i32; 2]) -> bool + 'a {
    move |a, b| edges.iter().any(|&e| crosses(a, b, e))
}
/// Steps a wave with the hero's body at `hero` until it is gone; returns the
/// ticks it hurt on and the furthest x it reached.
fn run(wave: &mut Wave, edges: &[[i32; 4]], hero: [i32; 4]) -> (Vec<u32>, i32) {
    let (mut hurt, mut far) = (Vec::new(), i32::MIN);
    for t in 0..400u32 {
        let done = wave.step(&P, hits(edges));
        if wave.hurts(&P, hero) { hurt.push(t); }
        far = far.max(wave.x);
        if done { return (hurt, far); }
    }
    panic!("the wave never ended");
}

#[test]
fn a_wave_creeps_out_speeds_up_and_stops_at_the_wall() {
    let mut wave = Wave::new(SPAWN, 1, &P);
    let mut xs = Vec::new();
    for _ in 0..30 {
        wave.step(&P, hits(&EDGES));
        xs.push(wave.x);
    }
    // 0.55 units a second at first, 44 more every second after: half a second
    // in it has covered a little over five and a half units.
    let expected = P.start_speed / 2 + P.accel / 8;
    assert!((xs[29] - expected).abs() < ONE / 2, "after 30 ticks at {} not {}", xs[29], expected);
    assert!(xs.windows(3).all(|w| w[2] - w[1] >= w[1] - w[0]), "the wave slowed down");
    let (_, far) = run(&mut wave, &EDGES, [100 * ONE; 4]);
    assert!(far < 20 * ONE && far > 19 * ONE, "stopped at {far}, the wall is at 20");
}

#[test]
fn a_hero_on_the_floor_is_hit_and_one_above_it_is_not() {
    let standing = [9 * ONE, 0, 10 * ONE, ONE + ONE / 4];
    let (hurt, _) = run(&mut Wave::new(SPAWN, 1, &P), &EDGES, standing);
    assert!(!hurt.is_empty(), "a hero standing in the wave's path was never hit");
    // Each spurt is armed for three ticks of its life, and the wave passes a
    // one-unit hero in a few ticks, so the hits come in one short run.
    assert!(hurt.last().unwrap() - hurt[0] < 12, "hit over {:?}", hurt);
    let jumping = [9 * ONE, 2 * ONE, 10 * ONE, 3 * ONE + ONE / 4];
    let (hurt, _) = run(&mut Wave::new(SPAWN, 1, &P), &EDGES, jumping);
    assert!(hurt.is_empty(), "a hero two units above the wave was hit");
    let (hurt, _) = run(&mut Wave::new(SPAWN, -1, &P), &EDGES, standing);
    assert!(hurt.is_empty(), "a leftward wave hit a hero to its right");
}

#[test]
fn a_wave_ends_where_the_floor_does_and_its_spurts_play_out() {
    let edges = [[-40 * ONE, 0, 8 * ONE, 0]];
    let mut wave = Wave::new(SPAWN, 1, &P);
    let mut ticks = 0;
    while wave.moving {
        assert!(!wave.step(&P, hits(&edges)));
        ticks += 1;
        assert!(ticks < 200);
    }
    assert!(wave.x <= 8 * ONE && wave.x > 7 * ONE, "left the floor at {}", wave.x);
    // Every third spurt is drawn, each no older than its clip.
    assert!(wave.spurts(&P, 3).all(|(_, age)| age < P.spurt_ticks));
    assert!(wave.spurts(&P, 3).count() >= 5);
    let mut left = 0;
    while !wave.step(&P, hits(&edges)) { left += 1; }
    assert!(left < P.spurt_ticks as i32, "the spurts outlived their clip");
    assert_eq!(wave.spurts(&P, 1).count(), 0);
}
