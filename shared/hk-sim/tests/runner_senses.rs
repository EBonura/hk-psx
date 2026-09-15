use hk_sim::{runner_senses::*, ONE};

fn sight(a: [i32; 2], b: [i32; 2], edges: &[[i32; 4]]) -> bool {
    line_of_sight(a, b, true, edges.len(), |i| edges[i]).unwrap()
}
fn ray(direction: Direction, reach: i32) -> Sweep {
    Sweep::new([0; 2], [0; 2], [0; 2], direction, reach, 0).unwrap()
}
fn hits(s: &Sweep, edges: &[[i32; 4]]) -> bool {
    s.hits(edges.len(), |i| edges[i]).unwrap()
}

#[test]
fn transformed_alert_bounds_and_actual_hero_body_overlap_all_four_boundaries() {
    let actor = [40 * ONE, 3 * ONE];
    let [l, b, r, t] = alert_bounds(actor).unwrap();
    assert_eq!(
        [l - actor[0], b - actor[1], r - actor[0], t - actor[1]],
        ALERT_LOCAL
    );
    assert!(r - l > 11 * ONE); // scaled child, not the raw 1-unit collider
    for (touch, outside) in [
        ([l - ONE, b, l, t], [l - ONE - 1, b, l - 1, t]),
        ([r, b, r + ONE, t], [r + 1, b, r + ONE + 1, t]),
        ([l, b - ONE, r, b], [l, b - ONE - 1, r, b - 1]),
        ([l, t, r, t + ONE], [l, t + 1, r, t + ONE + 1]),
    ] {
        assert!(alert_overlap(actor, touch).unwrap());
        assert!(!alert_overlap(actor, outside).unwrap());
    }
    assert_eq!(
        alert_overlap(actor, [2, 0, 1, 1]),
        Err(QueryError::InvalidBounds)
    );
}

#[test]
fn body_offset_is_mirrored_while_alert_box_is_symmetric() {
    assert_eq!(
        body_bounds([0; 2], -1).unwrap(),
        [-51200, -94208, 28672, 12288]
    );
    assert_eq!(
        body_bounds([0; 2], 1).unwrap(),
        [-28672, -94208, 51200, 12288]
    );
    assert_eq!(body_bounds([0; 2], 0), Err(QueryError::InvalidFacing));
    assert_eq!(ALERT_LOCAL[0], -ALERT_LOCAL[2]);
}

#[test]
fn los_range_gate_and_clear_blocked_collinear_closed_endpoints() {
    assert_eq!(
        line_of_sight([0, 0], [2 * ONE, 0], false, 1, |_| panic!(
            "range must gate terrain"
        )),
        Ok(false)
    );
    let a = [0, 0];
    let b = [2 * ONE, 0];
    assert!(sight(a, b, &[]));
    assert!(!sight(a, b, &[[ONE, -ONE, ONE, ONE]]));
    assert!(!sight(b, a, &[[ONE, -ONE, ONE, ONE]])); // no facing cone
    assert!(sight(a, b, &[[ONE, 1, ONE, ONE]]));
    assert!(!sight(a, b, &[[0, -ONE, 0, ONE]]));
    assert!(!sight(a, b, &[[2 * ONE, 0, 3 * ONE, 0]]));
    assert!(!sight(a, b, &[[-ONE, 0, ONE, 0]]));
    assert!(sight(a, b, &[[2 * ONE + 1, 0, 3 * ONE, 0]]));
    assert!(sight(a, b, &[[ONE, 0, ONE, 0]])); // cooked disabled edge sentinel
    assert_eq!(
        line_of_sight(a, a, true, 0, |_| unreachable!()),
        Err(QueryError::ZeroLengthSight)
    );
}

#[test]
fn diagonal_los_rejects_aabb_only_overlap_and_handles_slopes() {
    let a = [0, 0];
    let b = [4 * ONE, 4 * ONE];
    assert!(sight(a, b, &[[0, 3 * ONE, ONE, 4 * ONE]]));
    assert!(!sight(a, b, &[[ONE, 3 * ONE, 3 * ONE, ONE]]));
    assert!(!sight(a, b, &[[ONE, ONE, 3 * ONE, 3 * ONE]]));
    assert!(sight(a, b, &[[-ONE, -ONE, -1, -1]]));
}

#[test]
fn exact_source_three_ray_origins_include_scaled_offset_extents_and_skin() {
    let s = Sweep::new(
        [0; 2],
        BODY_OFFSET_LEFT,
        BODY_EXTENTS,
        Direction::Left,
        39936 + ONE / 2,
        SKIN,
    )
    .unwrap();
    assert_eq!(
        s.origins(),
        [
            [-51200 + SKIN, -94208],
            [-51200 + SKIN, -40960],
            [-51200 + SKIN, 12288]
        ]
    );
    assert_eq!(s.reach(), 39936 + ONE / 2 + SKIN);
    // Each of the three fringe rays can be the only ray to detect a short wall.
    for p in s.origins() {
        assert!(hits(
            &s,
            &[[p[0] - ONE / 2, p[1] - 1, p[0] - ONE / 2, p[1] + 1]]
        ));
    }
    assert!(!hits(&s, &[[-ONE, -60000, -ONE, -55000]])); // between rays
    let floor = Sweep::new(
        [-39936 - ONE / 2, 0],
        BODY_OFFSET_LEFT,
        BODY_EXTENTS,
        Direction::Down,
        ONE / 4,
        SKIN,
    )
    .unwrap();
    assert_eq!(
        floor.origins(),
        [
            [-123904, -94208 + SKIN],
            [-83968, -94208 + SKIN],
            [-44032, -94208 + SKIN]
        ]
    );
}

#[test]
fn source_endpoint_is_strict_origin_included_and_negative_skin_distance_counts() {
    let s = Sweep::new([0; 2], [0; 2], [0; 2], Direction::Right, ONE, SKIN).unwrap();
    assert_eq!(s.origins(), [[-SKIN, 0]; 3]);
    assert!(!hits(&s, &[[ONE, -ONE, ONE, ONE]]));
    assert!(hits(&s, &[[ONE - 1, -ONE, ONE - 1, ONE]]));
    assert!(!hits(&s, &[[ONE + 1, -ONE, ONE + 1, ONE]]));
    assert!(hits(&s, &[[-SKIN, -ONE, -SKIN, ONE]]));
    assert!(!hits(&s, &[[-SKIN - 1, -ONE, -SKIN - 1, ONE]]));
    assert!(hits(&s, &[[-1, -ONE, -1, ONE]])); // adjusted hit distance -1
    for distance in [0, -ONE] {
        assert!(!hits(
            &Sweep::new([0; 2], [0; 2], [0; 2], Direction::Right, distance, SKIN).unwrap(),
            &[[0, -ONE, 0, ONE]]
        ));
    }
}

#[test]
fn slope_intersection_smaller_than_one_q16_unit_is_not_rounded_away() {
    // Intersection x=reach-1/3 Q16 units: true. +1/3 and exact reach: false.
    let s = ray(Direction::Right, ONE);
    for (e, expected) in [
        ([ONE - 1, -2, ONE, 1], true),
        ([ONE, -2, ONE + 1, 1], false),
        ([ONE - 1, -1, ONE + 1, 1], false),
    ] {
        assert_eq!(hits(&s, &[e]), expected);
        assert_eq!(hits(&s, &[[e[2], e[3], e[0], e[1]]]), expected);
    }
}

#[test]
fn cardinal_rotations_and_collinear_overlap_preserve_half_open_ray() {
    let cases = [
        ([-ONE, 0, 0, 0], true),
        ([ONE, 0, 2 * ONE, 0], false),
        ([ONE - 1, 0, 2 * ONE, 0], true),
        ([-2 * ONE, 0, -1, 0], false),
        ([-ONE, 0, 2 * ONE, 0], true),
        ([0, 1, ONE, 1], false),
    ];
    for direction in [
        Direction::Right,
        Direction::Left,
        Direction::Up,
        Direction::Down,
    ] {
        let transform = |x: i32, y: i32| match direction {
            Direction::Right => [x, y],
            Direction::Left => [-x, y],
            Direction::Up => [y, x],
            Direction::Down => [y, -x],
        };
        for (e, expected) in cases {
            let a = transform(e[0], e[1]);
            let b = transform(e[2], e[3]);
            assert_eq!(
                hits(&ray(direction, ONE), &[[a[0], a[1], b[0], b[1]]]),
                expected,
                "{direction:?} {e:?}"
            );
        }
    }
}

#[test]
fn runner_wall_and_hole_sweeps_use_all_rays_once_and_mirror() {
    let edges = [
        [-ONE - ONE / 2, -2 * ONE, -ONE - ONE / 2, ONE],
        [-2 * ONE, -3 * ONE / 2, -ONE, -3 * ONE / 2],
    ];
    let calls = core::cell::Cell::new(0);
    let found = walker_queries([0; 2], -1, edges.len(), |i| {
        calls.set(calls.get() + 1);
        edges[i]
    })
    .unwrap();
    assert_eq!(
        found,
        WalkerQueries {
            wall: true,
            floor_ahead: true
        }
    );
    assert_eq!(calls.get(), edges.len());
    let mirror: Vec<_> = edges.iter().map(|e| [-e[0], e[1], -e[2], e[3]]).collect();
    assert_eq!(
        walker_queries([0; 2], 1, mirror.len(), |i| mirror[i]).unwrap(),
        found
    );
    assert_eq!(
        walker_queries([0; 2], -1, 0, |_| unreachable!()).unwrap(),
        WalkerQueries {
            wall: false,
            floor_ahead: false
        }
    );
    let slope = [-2 * ONE, -7 * ONE / 4, -ONE, -5 * ONE / 4];
    assert!(
        walker_queries([0; 2], -1, 1, |_| slope)
            .unwrap()
            .floor_ahead
    );
}

#[test]
fn coordinate_limits_are_checked_even_after_earlier_hit_and_cannot_overflow() {
    assert!(!sight(
        [-WORLD_LIMIT, -WORLD_LIMIT],
        [WORLD_LIMIT, WORLD_LIMIT],
        &[[-WORLD_LIMIT, WORLD_LIMIT, WORLD_LIMIT, -WORLD_LIMIT]]
    ));
    assert_eq!(
        line_of_sight([i32::MIN, 0], [0, 0], true, 0, |_| unreachable!()),
        Err(QueryError::CoordinateLimit)
    );
    assert_eq!(
        alert_bounds([WORLD_LIMIT, 0]),
        Err(QueryError::CoordinateLimit)
    );
    assert_eq!(
        Sweep::new([WORLD_LIMIT, 0], [0; 2], [0; 2], Direction::Right, ONE, 0),
        Err(QueryError::CoordinateLimit)
    );
    assert_eq!(
        Sweep::new([0; 2], [i32::MAX, 0], [0; 2], Direction::Right, ONE, 0),
        Err(QueryError::LocalLimit)
    );
    assert_eq!(
        Sweep::new([0; 2], [0; 2], [-1, 0], Direction::Right, ONE, 0),
        Err(QueryError::LocalLimit)
    );
    assert_eq!(
        walker_queries([i32::MAX, 0], 1, 0, |_| unreachable!()),
        Err(QueryError::CoordinateLimit)
    );
    let edges = [[ONE / 2, -ONE, ONE / 2, ONE], [i32::MAX, 0, i32::MAX, ONE]];
    assert_eq!(
        ray(Direction::Right, ONE).hits(2, |i| edges[i]),
        Err(QueryError::CoordinateLimit)
    );
    assert_eq!(
        line_of_sight([0, 0], [ONE, 0], true, 2, |i| edges[i]),
        Err(QueryError::CoordinateLimit)
    );
}
