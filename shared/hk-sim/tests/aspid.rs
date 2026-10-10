use hk_sim::aspid::*;
use hk_sim::ONE;

fn senses(position: [i32; 2], hero: [i32; 2], see: bool) -> Senses {
    Senses {
        position,
        hero,
        can_see_hero: see,
        in_unalert_range: see,
        sight_clear: true,
    }
}
fn run(
    a: &mut Aspid,
    mut p: [i32; 2],
    hero: [i32; 2],
    see: bool,
    ticks: usize,
) -> ([i32; 2], Vec<Action>) {
    let mut out = Vec::new();
    for _ in 0..ticks {
        out.extend(a.tick(senses(p, hero, see)).iter());
        let v = a.velocity();
        p[0] += v[0] / 60;
        p[1] += v[1] / 60;
    }
    (p, out)
}

#[test]
fn idle_roams_then_keeps_distance_and_fires_one_shot_toward_the_hero() {
    let start = [0, 10 * ONE];
    let hero = [3 * ONE, 0];
    let mut a = Aspid::new(start, 4);
    let (p, actions) = run(&mut a, start, [-50 * ONE, 0], false, 120);
    assert_eq!(a.phase(), Phase::Idle);
    assert!(
        (p[0] - start[0]).abs() < 3 * ONE
            && actions.iter().any(|x| matches!(x, Action::Velocity(_)))
    );
    a.tick(senses(p, hero, true));
    assert_eq!(a.phase(), Phase::DistanceFly);
    // Hovers about 7 units from the hero, checks sight, flies back, anticipates and fires.
    let mut p = p;
    let mut actions = Vec::new();
    let mut ticks = 0;
    while !actions.iter().any(|x| matches!(x, Action::Fire(_))) {
        let (np, more) = run(&mut a, p, hero, true, 1);
        p = np;
        actions.extend(more);
        ticks += 1;
        assert!(ticks < 600, "no shot within ten seconds");
    }
    assert!(ticks > 90 + FLY_BACK_TICKS as usize + FIRE_TRIGGER_TICKS as usize);
    let shot = actions
        .iter()
        .find_map(|x| {
            if let Action::Fire(v) = x {
                Some(*v)
            } else {
                None
            }
        })
        .expect("one shot");
    let speed2 = shot[0] as i64 * shot[0] as i64 + shot[1] as i64 * shot[1] as i64;
    let expected = SHOT_SPEED as i64 * SHOT_SPEED as i64;
    assert!((speed2 - expected).abs() < expected / 50, "{shot:?}");
    // Aimed at the hero: same direction sign as the displacement.
    assert_eq!((shot[0] > 0, shot[1] > 0), (hero[0] > p[0], hero[1] > p[1]));
    assert!(actions
        .iter()
        .any(|x| matches!(x, Action::Play(Clip::FireLong, 0))));
    assert_eq!(
        actions
            .iter()
            .filter(|x| matches!(x, Action::Fire(_)))
            .count(),
        1
    );
    assert_eq!(a.phase(), Phase::FireDribble);
    let (_, actions) = run(
        &mut a,
        p,
        hero,
        true,
        (FIRE_LONG_TICKS - FIRE_TRIGGER_TICKS) as usize,
    );
    assert_eq!(a.phase(), Phase::DistanceFly);
    let _ = FLY_BACK_TICKS;
    assert!(actions
        .iter()
        .any(|x| matches!(x, Action::Play(Clip::Fly, 10))));
}

#[test]
fn losing_the_hero_for_eight_seconds_returns_to_idle_and_obstructed_sight_keeps_flying() {
    let mut a = Aspid::new([0, 0], 1);
    a.tick(senses([0, 0], [5 * ONE, 0], true));
    assert_eq!(a.phase(), Phase::DistanceFly);
    // The Range Out Timer only counts in Distance Fly; it survives the shot cycle.
    let mut ticks = 0;
    while a.phase() != Phase::Idle {
        a.tick(Senses {
            in_unalert_range: false,
            ..senses([0, 0], [5 * ONE, 0], false)
        });
        ticks += 1;
        assert!(ticks < 2000);
    }
    assert!(ticks > UNALERT_TICKS as usize);
    let mut b = Aspid::new([0, 0], 2);
    b.tick(senses([0, 0], [5 * ONE, 0], true));
    for _ in 0..200 {
        b.tick(Senses {
            sight_clear: false,
            ..senses([0, 0], [5 * ONE, 0], true)
        });
        assert!(matches!(b.phase(), Phase::DistanceFly | Phase::Raycast));
    }
    b.die();
    assert!(b.tick(senses([0, 0], [0, 0], true)).iter().next().is_none());
}
