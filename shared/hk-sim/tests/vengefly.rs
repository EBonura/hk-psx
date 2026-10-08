use hk_sim::vengefly::*;
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
    let (position, actions) = run(&mut fly, position, hero, false, 1);
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
