use hk_sim::vengefly3::*;
use hk_sim::ONE;

fn senses(position: [i32; 2], hero: [i32; 2], can_see_hero: bool) -> Senses {
    Senses { position, hero, can_see_hero }
}
fn run(fly: &mut Vengefly, mut position: [i32; 2], hero: [i32; 2], see: bool, ticks: usize) -> ([i32; 2], Vec<Action>) {
    let mut out = Vec::new();
    for _ in 0..ticks {
        out.extend(fly.tick(senses(position, hero, see)).iter());
        let v = fly.velocity();
        position[0] += v[0] / 60;
        position[1] += v[1] / 60;
    }
    (position, out)
}

#[test]
fn idle_roams_within_range_and_faces_velocity() {
    let mut fly = Vengefly::new([10 * ONE, 20 * ONE], 7);
    let far = [-100 * ONE, 0];
    let (position, actions) = run(&mut fly, [10 * ONE, 20 * ONE], far, false, 60 * 30);
    assert_eq!(fly.phase(), Phase::Idle);
    let v = fly.velocity();
    assert!(v[0].abs() <= IDLE_SPEED_MAX && v[1].abs() <= IDLE_SPEED_MAX);
    // Roaming bounces keep the body near its origin (range plus overshoot).
    assert!((position[0] - 10 * ONE).abs() < 3 * ONE, "{position:?}");
    assert!((position[1] - 20 * ONE).abs() < 3 * ONE, "{position:?}");
    assert!(actions.iter().any(|a| matches!(a, Action::Velocity(_))));
    assert!(actions.iter().any(|a| matches!(a, Action::Play(Clip::TurnToIdle, 0))));
    assert!(!actions.iter().any(|a| matches!(a, Action::Play(Clip::Startle, _) | Action::StartleSound)));
}

#[test]
fn sight_startles_then_chases_with_attention_span_then_stops() {
    let start = [0, 0];
    let hero = [8 * ONE, -2 * ONE];
    let mut fly = Vengefly::new(start, 1);
    let actions = fly.tick(senses(start, hero, true));
    let actions: Vec<_> = actions.iter().collect();
    assert_eq!(fly.phase(), Phase::Startle);
    assert!(actions.contains(&Action::StartleSound));
    assert!(actions.contains(&Action::Velocity([0; 2])));
    assert!(actions.contains(&Action::Play(Clip::Startle, 0)));
    assert!(actions.contains(&Action::Facing(1)));
    let (_, actions) = run(&mut fly, start, hero, true, STARTLE_TICKS as usize);
    assert_eq!(fly.phase(), Phase::Chase);
    assert!(actions.contains(&Action::Play(Clip::Chase, CHASE_START_FRAME_TICKS)));
    // Accelerates toward the hero on both axes, bounded by the chase speed.
    let (position, _) = run(&mut fly, start, hero, true, 30);
    let v = fly.velocity();
    assert!(v[0] > 0 && v[1] < 0 && v[0] <= CHASE_SPEED_MAX, "{v:?}");
    assert!(position[0] > start[0] && position[1] < start[1]);
    let (position, _) = run(&mut fly, position, hero, true, 120);
    // Oscillates around the hero once it arrives.
    assert!((position[0] - hero[0]).abs() < 3 * ONE && (position[1] - hero[1]).abs() < 3 * ONE, "{position:?}");
    // Out of sight: chase continues for the attention span, then Stop.
    let (_, actions) = run(&mut fly, position, hero, false, ATTENTION_TICKS as usize - 1);
    assert_eq!(fly.phase(), Phase::Chase);
    assert!(!actions.iter().any(|a| matches!(a, Action::Play(Clip::Idle, _))));
    // The original reaches Stop 600 or 601 ticks after the last sighting, by which of the two chase
    // states saw it last (In Sight hands over to Out of Sight on the next frame): inside the 1 tick bar.
    let (position, actions) = run(&mut fly, position, hero, false, 2);
    assert_eq!(fly.phase(), Phase::Stop);
    assert!(actions.contains(&Action::Play(Clip::Idle, CHASE_START_FRAME_TICKS)));
    let (position, actions) = run(&mut fly, position, hero, false, STOP_TICKS as usize);
    assert_eq!(fly.phase(), Phase::Idle);
    assert!(actions.contains(&Action::Play(Clip::Idle, 0)));
    // Stop decelerated 50 steps of .12: any chase speed is gone.
    let v = fly.velocity();
    assert!(v[0].abs() <= IDLE_SPEED_MAX && v[1].abs() <= IDLE_SPEED_MAX, "{v:?}");
    let _ = position;
}

#[test]
fn seeing_the_hero_resets_attention_and_damage_startles_only_idle() {
    let hero = [-6 * ONE, 0];
    let mut fly = Vengefly::new([0, 0], 3);
    assert!(fly.took_damage(senses([0, 0], hero, false)).iter().any(|a| a == Action::Facing(-1)));
    assert_eq!(fly.phase(), Phase::Startle);
    let (p, _) = run(&mut fly, [0, 0], hero, false, STARTLE_TICKS as usize);
    assert_eq!(fly.phase(), Phase::Chase);
    let (p, _) = run(&mut fly, p, hero, false, ATTENTION_TICKS as usize - 5);
    let (p, _) = run(&mut fly, p, hero, true, 1);
    let (_, _) = run(&mut fly, p, hero, false, ATTENTION_TICKS as usize - 5);
    assert_eq!(fly.phase(), Phase::Chase);
    assert!(fly.took_damage(senses(p, hero, false)).iter().next().is_none());
    fly.die();
    assert_eq!(fly.phase(), Phase::Dead);
    assert!(fly.tick(senses(p, hero, true)).iter().next().is_none());
}

#[test]
fn deterministic_per_seed() {
    let a = run(&mut Vengefly::new([0, 0], 9), [0, 0], [50 * ONE, 0], false, 300);
    let b = run(&mut Vengefly::new([0, 0], 9), [0, 0], [50 * ONE, 0], false, 300);
    let c = run(&mut Vengefly::new([0, 0], 10), [0, 0], [50 * ONE, 0], false, 300);
    assert_eq!(a, b);
    assert_ne!(a.0, c.0);
}

mod against_the_hand_port {
    use hk_sim::vengefly as v1;
    use hk_sim::vengefly3 as v2;
    use hk_sim::ONE;

    fn p1(p: v1::Phase) -> u8 {
        p as u8
    }
    fn p2(p: v2::Phase) -> u8 {
        p as u8
    }
    /// Phase changes as (tick, phase) over a long scripted run of sight and damage.
    fn timeline(seed: u32) -> (Vec<(usize, u8)>, Vec<(usize, u8)>) {
        let hero = [8 * ONE, -2 * ONE];
        let mut a = v1::Vengefly::new([0, 0], seed);
        let mut b = v2::Vengefly::new([0, 0], seed);
        let (mut la, mut lb) = (p1(a.phase()), p2(b.phase()));
        let (mut ta, mut tb) = (vec![], vec![]);
        let mut x = seed.wrapping_mul(2654435761).wrapping_add(7);
        let mut see = false;
        for tick in 0..6000usize {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            // Sight comes and goes in long stretches; a hit lands now and then.
            if (x >> 20) % 400 == 0 {
                see = !see;
            }
            let hit = (x >> 8) % 1500 == 0;
            let s1 = v1::Senses { position: [0, 0], hero, can_see_hero: see };
            let s2 = v2::Senses { position: [0, 0], hero, can_see_hero: see };
            if hit {
                let _ = a.took_damage(s1);
                let _ = b.took_damage(s2);
            }
            let _ = a.tick(s1);
            let _ = b.tick(s2);
            if p1(a.phase()) != la {
                la = p1(a.phase());
                ta.push((tick, la));
            }
            if p2(b.phase()) != lb {
                lb = p2(b.phase());
                tb.push((tick, lb));
            }
        }
        (ta, tb)
    }

    #[test]
    fn phase_sequences_agree_with_the_hand_port_within_two_ticks() {
        let mut total = 0;
        for seed in 1..40 {
            let (a, b) = timeline(seed);
            // The hand port lands in Idle for a tick before it sees the hero; the FSM's enter pass sees at
            // once. A phase held for one tick is dropped from both sides.
            let blip = |v: Vec<(usize, u8)>| -> Vec<(usize, u8)> {
                let mut out: Vec<(usize, u8)> = vec![];
                for (i, (t, p)) in v.iter().enumerate() {
                    if v.get(i + 1).is_some_and(|n| n.0 <= t + 1) {
                        continue;
                    }
                    if out.last().is_some_and(|l| l.1 == *p) {
                        continue;
                    }
                    out.push((*t, *p));
                }
                out
            };
            let (a, b) = (blip(a), blip(b));
            assert_eq!(a.len(), b.len(), "seed {seed}: {a:?} vs {b:?}");
            let mut skew = 0i64;
            for ((ta, pa), (tb, pb)) in a.iter().zip(&b) {
                assert_eq!(pa, pb, "seed {seed}");
                skew += (*tb as i64 - *ta as i64).abs();
                assert!((*tb as i64 - *ta as i64).abs() <= 2, "seed {seed}: phase {pa} at {ta} vs {tb}");
            }
            let _ = skew;
            total += a.len();
        }
        assert!(total > 100, "the script should exercise many phase changes, saw {total}");
    }
}
