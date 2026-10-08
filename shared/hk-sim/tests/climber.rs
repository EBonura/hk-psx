// Direct module inclusion allows this bounded module to be validated before
// parent integration adds the public hk-sim export.
const ONE: i32 = hk_sim::ONE;
#[path = "../src/climber.rs"]
mod climber;
use climber::*;

fn born(position: [i32; 2], angle: i32, clockwise: bool) -> Climber {
    let mut c = Climber::new(position, angle, 1, clockwise).unwrap();
    c.attach(None).unwrap();
    c
}
fn list(a: Actions) -> Vec<Action> {
    a.iter().collect()
}
fn apply_position(a: &[Action], p: &mut [i32; 2]) {
    for action in a {
        if let Action::Position(position) = action {
            *p = *position;
        }
    }
}
fn stun_done(c: &mut Climber) -> u32 {
    for _ in 0..STUN_TICKS - 1 {
        assert!(list(c.stun_tick()).is_empty());
    }
    match list(c.stun_tick()).as_slice() {
        [Action::AwaitEndOfFrame(token)] => *token,
        _ => panic!("missing end-of-frame wait"),
    }
}

#[test]
fn attachment_uses_offset_point_ray_and_assigns_entire_hit_point_once() {
    let position = [10 * ONE, 10 * ONE];
    let mut c = Climber::new(position, 0, 1, true).unwrap();
    let ray = c.attachment_ray().unwrap();
    assert_eq!(ray.position, position);
    assert_eq!(ray.local_origin, [1024, 31232]);
    assert_eq!(ray.local_direction, [0, -ONE]);
    assert_eq!(ray.length, 2 * ONE);
    assert_eq!(ray.layer_mask, 256);
    assert_eq!(ray.scale_x_sign, 1);
    let hit = [10 * ONE + 1024, 9 * ONE];
    assert_eq!(
        list(c.attach(Some(hit)).unwrap()),
        [
            Action::Position(hit),
            Action::Play(Clip::Walk),
            Action::Velocity([2 * ONE, 0])
        ]
    );
    assert_eq!(c.previous_position(), hit);
    assert_eq!(c.previous_turn_position(), [0, 0]);
    assert_eq!(c.attachment_ray(), None);
    assert!(list(c.attach(Some([0, 0])).unwrap()).is_empty());
    assert_eq!(c.previous_position(), hit);
    let mut c = Climber::new(position, 0, -1, true).unwrap();
    assert_eq!(c.attachment_ray().unwrap().scale_x_sign, -1);
    assert_eq!(
        list(c.attach(None).unwrap()),
        [Action::Play(Clip::Walk), Action::Velocity([-2 * ONE, 0])]
    );
    assert_eq!(c.previous_position(), position);
}

#[test]
fn source_rotation_band_endpoints_and_handedness_select_cardinal_velocity() {
    for (angle, expected) in [
        (0, Direction::East),
        (45 * ONE - 1, Direction::East),
        (45 * ONE, Direction::North),
        (135 * ONE, Direction::North),
        (135 * ONE + 1, Direction::West),
        (225 * ONE, Direction::West),
        (225 * ONE + 1, Direction::South),
        (315 * ONE, Direction::South),
        (315 * ONE + 1, Direction::East),
        (360 * ONE, Direction::East),
        (-90 * ONE, Direction::South),
    ] {
        let c = born([0; 2], angle, true);
        assert!(c.clockwise());
        assert_eq!(c.direction(), expected);
        let reverse = born([0; 2], angle, false);
        let v = expected.velocity();
        assert_eq!(reverse.direction().velocity(), [-v[0], -v[1]]);
    }
    assert!(Climber::new([0; 2], 0, -1, false).unwrap().clockwise());
    assert!(!Climber::new([0; 2], 0, -1, true).unwrap().clockwise());
}

#[test]
fn ground_precedes_wall_and_both_rays_use_source_lengths_and_corrected_pose() {
    let p = [10 * ONE, 10 * ONE];
    let mut c = born(p, 0, true);
    let mut rays = Vec::new();
    c.walk_frame(p, |ray| {
        rays.push(ray);
        false
    })
    .unwrap();
    assert_eq!(rays.len(), 1);
    assert_eq!(rays[0].length, ONE);
    assert_eq!(c.phase(), Phase::Turning);
    let mut c = born(p, 0, false);
    let mut rays = Vec::new();
    c.walk_frame([p[0] + CONSTRAIN + 1, p[1]], |ray| {
        rays.push(ray);
        true
    })
    .unwrap();
    assert_eq!(rays.len(), 2);
    assert_eq!(rays[0].position, p);
    assert_eq!(rays[1].position, p);
    assert_eq!(rays[1].local_direction, [-ONE, 0]);
    assert_eq!(rays[1].length, 42394);
    assert_eq!(rays[1].layer_mask, 256);
}

#[test]
fn per_axis_constraint_is_strict_and_retains_entire_previous_position_on_correction() {
    let p = [10 * ONE, 10 * ONE];
    let mut c = born(p, 0, true);
    let next = [p[0] + CONSTRAIN, p[1] + CONSTRAIN];
    assert!(list(c.walk_frame(next, |r| r.local_direction[1] != 0).unwrap()).is_empty());
    assert_eq!(c.previous_position(), next);
    let next2 = [next[0] + CONSTRAIN + 1, next[1] + 1];
    assert_eq!(
        list(c.walk_frame(next2, |r| r.local_direction[1] != 0).unwrap()),
        [Action::Position([next[0], next[1] + 1])]
    );
    assert_eq!(c.previous_position(), next); // even uncorrected Y isn't recorded
    let next3 = [next[0] - 1, next[1] - CONSTRAIN - 1];
    assert_eq!(
        list(c.walk_frame(next3, |r| r.local_direction[1] != 0).unwrap()),
        [Action::Position([next[0] - 1, next[1]])]
    );
    assert_eq!(c.previous_position(), next);
}

#[test]
fn distance_gate_is_inclusive_at_quarter_unit_and_walk_does_not_rewrite_velocity() {
    let mut c = born([MIN_TURN_DISTANCE - 1, 0], 0, true);
    assert!(list(
        c.walk_frame([MIN_TURN_DISTANCE - 1, 0], |_| panic!("below gate"))
            .unwrap()
    )
    .is_empty());
    let mut calls = 0;
    assert!(list(
        c.walk_frame([MIN_TURN_DISTANCE, 0], |r| {
            calls += 1;
            r.local_direction[1] != 0
        })
        .unwrap()
    )
    .is_empty());
    assert_eq!(calls, 2);
    assert_eq!(c.phase(), Phase::Walking);
    let mut c = born([0, MIN_TURN_DISTANCE], 0, true);
    c.walk_frame([0, MIN_TURN_DISTANCE], |_| false).unwrap();
    assert_eq!(c.phase(), Phase::Turning);
}

#[test]
fn all_directions_handedness_and_inside_outside_corners_follow_authored_table() {
    for clockwise in [true, false] {
        for angle in [0, 90 * ONE, 180 * ONE, 270 * ONE] {
            for inside in [false, true] {
                let mut p = [10 * ONE, 10 * ONE];
                let mut c = born(p, angle, clockwise);
                let old = c.direction();
                let hand = if clockwise { 1 } else { -1 };
                let [hx, hy] = BODY_HALF;
                let expected = match old {
                    Direction::East => [hx + CONSTRAIN, hand * hy],
                    Direction::South => [hand * hx, -hy - CONSTRAIN],
                    Direction::West => [-hx - CONSTRAIN, -hand * hy],
                    Direction::North => [-hand * hx, hy + CONSTRAIN],
                };
                assert_eq!(c.tween_delta(), expected);
                let actions = list(c.walk_frame(p, |_| inside).unwrap());
                assert_eq!(actions[0], Action::Velocity([0; 2]));
                assert_eq!(
                    actions.iter().any(|a| matches!(a, Action::Position(_))),
                    inside
                );
                let turn_clockwise = if inside { !clockwise } else { clockwise };
                for _ in 1..TURN_TICKS {
                    let actions = list(c.turn_frame(p).unwrap());
                    apply_position(&actions, &mut p);
                }
                let before = p;
                let actions = list(c.turn_frame(p).unwrap());
                assert!(!actions
                    .iter()
                    .any(|a| matches!(a, Action::Position(_) | Action::Play(_))));
                assert_eq!(c.phase(), Phase::Walking);
                assert_eq!(c.previous_turn_position(), before);
                let want = (old as u8 + if turn_clockwise { 1 } else { 3 }) % 4;
                assert_eq!(c.direction() as u8, want);
                assert_eq!(
                    c.rotation(),
                    (angle + if turn_clockwise { -90 * ONE } else { 90 * ONE })
                        .rem_euclid(360 * ONE)
                );
                assert_eq!(
                    actions.last(),
                    Some(&Action::Velocity(c.direction().velocity()))
                );
            }
        }
    }
}

#[test]
fn inside_turn_keeps_last_14_over_15_position_and_external_position_at_completion() {
    let origin = [10 * ONE, 10 * ONE];
    let mut p = origin;
    let mut c = born(origin, 0, true);
    let delta = c.tween_delta();
    c.walk_frame(p, |_| true).unwrap();
    for i in 1..TURN_TICKS {
        let actions = list(c.turn_frame(p).unwrap());
        apply_position(&actions, &mut p);
        assert_eq!(
            p,
            core::array::from_fn(|axis| origin[axis] + (delta[axis] as i64 * i as i64 / 15) as i32)
        );
        assert_eq!(c.rotation(), i as i32 * 6 * ONE);
        assert!(list(c.freeze()).is_empty()); // accepted damage doesn't cancel turn
    }
    assert_ne!(p, [origin[0] + delta[0], origin[1] + delta[1]]);
    p[0] += 1; // caller physics/transform change after final interpolated sample
    let actions = list(c.turn_frame(p).unwrap());
    assert_eq!(
        actions,
        [Action::Rotation(90 * ONE), Action::Velocity([0, 2 * ONE])]
    );
    assert_eq!(c.previous_position(), p);
    assert_eq!(c.previous_turn_position(), p);
    assert!(list(
        c.walk_frame(p, |_| panic!("no immediate second turn"))
            .unwrap()
    )
    .is_empty());
}

#[test]
fn stun_uses_clip_duration_then_end_of_frame_restarts_and_ignores_stale_callbacks() {
    let p = [10 * ONE, 10 * ONE];
    let mut c = born(p, 0, true);
    assert_eq!(
        list(c.freeze()),
        [Action::Velocity([0; 2]), Action::Play(Clip::Stun)]
    );
    for _ in 0..15 {
        assert!(list(c.stun_tick()).is_empty());
    }
    assert_eq!(c.phase(), Phase::Stunned); // serialized .25s recoil doesn't end it
    c.freeze();
    let old = stun_done(&mut c);
    assert_eq!(c.phase(), Phase::StunEndOfFrame);
    for _ in 0..100 {
        assert!(list(c.stun_tick()).is_empty());
    }
    c.freeze();
    let new = stun_done(&mut c);
    assert_ne!(old, new);
    assert!(list(c.end_of_frame(old)).is_empty());
    assert_eq!(c.phase(), Phase::StunEndOfFrame);
    assert_eq!(
        list(c.end_of_frame(new)),
        [Action::Play(Clip::Walk), Action::Velocity([SPEED, 0])]
    );
    assert_eq!(c.previous_position(), p);
    assert!(list(c.end_of_frame(new)).is_empty());
    assert_eq!(
        list(
            c.walk_frame([p[0] + CONSTRAIN + 1, p[1]], |r| r.local_direction[1] != 0)
                .unwrap()
        ),
        [Action::Position(p)]
    );
}

#[test]
fn death_is_terminal_during_attachment_walking_turning_stun_and_end_of_frame() {
    for phase in 0..5 {
        let p = [10 * ONE, 10 * ONE];
        let mut c = Climber::new(p, 0, 1, true).unwrap();
        let mut token = 0;
        if phase > 0 {
            c.attach(None).unwrap();
        }
        if phase == 2 {
            c.walk_frame(p, |_| false).unwrap();
        }
        if phase >= 3 {
            c.freeze();
        }
        if phase == 4 {
            token = stun_done(&mut c);
        }
        c.die();
        c.die();
        assert_eq!(c.phase(), Phase::Dead);
        for _ in 0..100 {
            assert!(list(c.attach(Some([0; 2])).unwrap()).is_empty());
            assert!(list(c.walk_frame(p, |_| panic!("dead cast")).unwrap()).is_empty());
            assert!(list(c.turn_frame(p).unwrap()).is_empty());
            assert!(list(c.freeze()).is_empty());
            assert!(list(c.stun_tick()).is_empty());
            assert!(list(c.end_of_frame(token)).is_empty());
        }
    }
}

#[test]
fn checked_coordinates_and_bounded_controller_storage() {
    assert_eq!(
        Climber::new([i32::MAX, 0], 0, 1, true),
        Err(Error::CoordinateLimit)
    );
    assert_eq!(Climber::new([0; 2], 0, 0, true), Err(Error::ScaleSign));
    let mut c = Climber::new([0; 2], i32::MIN, 1, true).unwrap();
    assert_eq!(c.attach(Some([i32::MIN, 0])), Err(Error::CoordinateLimit));
    assert_eq!(c.phase(), Phase::AwaitAttachment);
    c.attach(None).unwrap();
    assert_eq!(
        c.walk_frame([i32::MIN, 0], |_| false),
        Err(Error::CoordinateLimit)
    );
    println!(
        "Climber={} B; Actions={} B",
        core::mem::size_of::<Climber>(),
        core::mem::size_of::<Actions>()
    );
    assert!(core::mem::size_of::<Climber>() <= 64);
}

#[test]
fn scaled_time_freeze_ramp_and_large_advances_keep_waits_bounded() {
    let p = [10 * ONE, 10 * ONE];
    let mut c = born(p, 180 * ONE, true);
    c.walk_frame(p, |_| false).unwrap();
    for _ in 0..4 {
        c.turn_frame(p).unwrap();
    }
    assert_eq!(c.rotation(), 156 * ONE);
    for _ in 0..26 {
        assert_eq!(
            list(c.turn_advance(p, 0).unwrap()),
            [Action::Rotation(156 * ONE)]
        );
        assert_eq!(c.phase(), Phase::Turning);
    }
    c.turn_advance(p, ONE as u32 / 2).unwrap();
    assert_eq!(c.rotation(), 153 * ONE);
    c.turn_advance(p, ONE as u32 / 4).unwrap();
    assert_eq!(c.rotation(), 151 * ONE + ONE / 2);
    let a = list(c.turn_advance(p, u32::MAX).unwrap());
    assert_eq!(
        a,
        [Action::Rotation(90 * ONE), Action::Velocity([0, 2 * ONE])]
    );
    assert_eq!(c.phase(), Phase::Walking);
    c.freeze();
    for _ in 0..70 {
        assert!(list(c.stun_advance(0)).is_empty());
    }
    for _ in 0..69 {
        assert!(list(c.stun_advance(ONE as u32 / 2)).is_empty());
    }
    assert_eq!(c.phase(), Phase::Stunned);
    assert!(matches!(
        list(c.stun_advance(ONE as u32 / 2)).as_slice(),
        [Action::AwaitEndOfFrame(_)]
    ));
    c.freeze();
    assert!(matches!(
        list(c.stun_advance(u32::MAX)).as_slice(),
        [Action::AwaitEndOfFrame(_)]
    ));
}
