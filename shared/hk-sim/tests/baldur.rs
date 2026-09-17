use hk_sim::baldur::*;
use hk_sim::ONE;

fn senses(actor_x: i32, hero_x: i32, see: bool, wall: bool, grounded: bool) -> Senses {
    Senses { actor_x, hero_x, can_see_hero: see, wall, grounded }
}
fn vx(actions: &Actions) -> Option<i32> {
    actions.iter().find_map(|a| match a { Action::VelocityX(v) => Some(v), Action::Velocity(v) => Some(v[0]), _ => None })
}

#[test]
fn idle_faces_hero_then_starts_rolls_and_stops_after_the_roll_time() {
    let mut b = Baldur::new(3);
    let a = b.tick(senses(0, 5 * ONE, false, false, true));
    assert!(a.iter().any(|x| x == Action::Facing(1)));
    assert_eq!(vx(&a), Some(0));
    let a = b.tick(senses(0, -5 * ONE, true, false, true));
    assert_eq!(b.phase(), Phase::Start);
    assert!(a.iter().any(|x| x == Action::Play(Clip::Start)) && a.iter().any(|x| x == Action::Facing(-1)));
    for _ in 0..START_TICKS { b.tick(senses(0, -5 * ONE, true, false, true)); }
    assert_eq!(b.phase(), Phase::Roll);
    let mut last = 0;
    let mut ticks = 0;
    while b.phase() == Phase::Roll {
        let a = b.tick(senses(0, -5 * ONE, true, false, true));
        if let Some(v) = vx(&a) { assert!(v <= last && v >= -MAX_SPEED); last = v; }
        ticks += 1;
        assert!(ticks < 200);
    }
    assert_eq!(last, -MAX_SPEED);
    assert!((120..=181).contains(&ticks), "{ticks}");
    assert_eq!(b.phase(), Phase::Stop);
    for _ in 0..STOP_TICKS { b.tick(senses(0, -5 * ONE, true, false, true)); }
    assert_eq!(b.phase(), Phase::Rest);
    for _ in 0..REST_TICKS { b.tick(senses(0, -5 * ONE, true, false, true)); }
    assert_eq!(b.phase(), Phase::Idle);
}

#[test]
fn wall_bounces_away_and_up_then_rolls_the_other_way_after_landing() {
    let mut b = Baldur::new(1);
    b.tick(senses(0, 5 * ONE, true, false, true));
    for _ in 0..START_TICKS { b.tick(senses(0, 5 * ONE, true, false, true)); }
    b.tick(senses(0, 5 * ONE, true, false, true));
    let a = b.tick(senses(0, 5 * ONE, true, true, true));
    assert_eq!(b.phase(), Phase::InAir);
    assert!(a.iter().any(|x| x == Action::Velocity([-BOUNCE_VELOCITY[0], BOUNCE_VELOCITY[1]])));
    for _ in 0..20 { assert!(b.tick(senses(0, 5 * ONE, true, false, false)).iter().next().is_none()); }
    let a = b.tick(senses(0, 5 * ONE, true, false, true));
    assert_eq!(b.phase(), Phase::Roll);
    assert!(a.iter().any(|x| x == Action::Play(Clip::Roll)));
    let a = b.tick(senses(0, 5 * ONE, true, false, true));
    assert_eq!(vx(&a), Some(-ACCELERATION));
    // A horizontal recoil resets the roll speed but keeps rolling.
    for _ in 0..10 { b.tick(senses(0, 5 * ONE, true, false, true)); }
    let a = b.horizontal_recoil();
    assert_eq!(vx(&a), Some(0));
    assert_eq!(b.phase(), Phase::Roll);
    b.die();
    assert!(b.tick(senses(0, 0, true, true, true)).iter().next().is_none());
}
