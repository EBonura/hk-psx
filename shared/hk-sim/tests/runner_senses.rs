use hk_sim::{runner_senses::*, ONE};
use std::cell::Cell;
thread_local! { static INDEX: Cell<Option<EdgeColumns>> = const { Cell::new(None) }; }
fn indexed(b: [i32; 4]) -> EdgeMask {
    INDEX.with(|i| i.get().map_or(ALL_EDGES, |i| i.near(b)))
}

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

#[test]
fn a_mirrored_placement_senses_with_the_box_it_was_placed_with() {
    // Crossroads_37 level67:4145, the one mirrored Runner on the disc (a
    // Leaper starting to face right), and its unmirrored twin 4146. Both
    // placements' own boxes must come back out at their starting facing and
    // mirror when they turn, or the guest's collider check panics on entry.
    let alert = [-6 * ONE, -2 * ONE, 6 * ONE, 3 * ONE];
    let actor = [30 * ONE, 4 * ONE];
    let at = |b: [i32; 4]| {
        [
            actor[0] + b[0],
            actor[1] + b[1],
            actor[0] + b[2],
            actor[1] + b[3],
        ]
    };
    for (body, facing) in [
        ([-24576, -96256, 25600, 72704], 1),
        ([-25600, -96256, 24576, 72704], -1),
    ] {
        let shape = Shape::from_placement(body, alert, facing);
        assert_eq!(body_bounds_of(shape, actor, facing).unwrap(), at(body));
        let turned = [-body[2], body[1], -body[0], body[3]];
        assert_eq!(body_bounds_of(shape, actor, -facing).unwrap(), at(turned));
        assert_eq!(shape.alert_local, alert);
    }
    // An unmirrored placement is exactly what from_boxes always did.
    let body = [-38912, -97280, 40960, 17408];
    assert_eq!(
        Shape::from_placement(body, alert, -1),
        Shape::from_boxes(body, alert)
    );
}

/// The box rejection in front of the exact test never changes an answer:
/// random sweeps in every direction against random edges, most of them near
/// the rays (touching, collinear, sloped, degenerate), compared with the
/// exact test alone.
#[test]
fn box_rejection_matches_the_exact_sweep_test() {
    let mut seed = 0x2545_f491u32;
    let mut next = |range: i32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % (2 * range as u32 + 1)) as i32 - range
    };
    let directions = [
        Direction::Left,
        Direction::Right,
        Direction::Down,
        Direction::Up,
    ];
    let (mut hits, mut checked) = (0, 0);
    for round in 0..4000 {
        let actor = [next(40 * ONE), next(40 * ONE)];
        let offset = [next(2 * ONE), next(2 * ONE)];
        let extents = [next(ONE).abs(), next(ONE).abs()];
        let sweep = Sweep::new(
            actor,
            offset,
            extents,
            directions[round % 4],
            next(2 * ONE),
            SKIN,
        )
        .unwrap();
        let o = sweep.origins();
        for _ in 0..24 {
            // Endpoints near a ray origin, or snapped onto it, so touches
            // and collinear edges are common.
            let base = o[(next(1) + 1) as usize];
            let mut coord = |v: i32| if next(3) == 0 { v } else { v + next(3 * ONE) };
            let e = [
                coord(base[0]),
                coord(base[1]),
                coord(base[0]),
                coord(base[1]),
            ];
            assert_eq!(
                sweep.hits_edge_filtered(e),
                sweep.hits_edge_unfiltered(e),
                "{sweep:?} {e:?}"
            );
            hits += sweep.hits_edge_unfiltered(e) as u32;
            checked += 1;
        }
    }
    assert!(
        hits > 1000 && hits < checked - 1000,
        "{hits} hits of {checked}"
    );
}

/// The column index changes which edges a walker or sight query visits, never
/// its answer: random rooms of up to 160 edges (axis-aligned, sloped, removed
/// `[0; 4]` and out-of-limit ones), probed from random positions.
#[test]
fn edge_columns_give_the_same_walker_and_sight_answers_as_every_edge() {
    let mut seed = 0x2545_f491u32;
    let mut next = move |n: i32| -> i32 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % n as u32) as i32
    };
    // Answers seen: probes, walls, floors, sight blocked, errors.
    let mut seen = [0; 5];
    for room in 0..300 {
        let span = [8, 40, 200, 600][room % 4];
        let count = 1 + next(160) as usize;
        let edges: Vec<[i32; 4]> = (0..count)
            .map(|_| {
                let x = (next(2 * span) - span) * ONE / 2 + next(ONE);
                let y = (next(span) - span / 2) * ONE / 2;
                match next(10) {
                    0 => [0; 4],
                    1 if room % 20 == 0 => [x, y, WORLD_LIMIT + ONE, y],
                    2 => [x, y, x + next(6 * ONE), y + next(6 * ONE) - 3 * ONE],
                    3..=5 => [x, y, x, y + next(8 * ONE) - 4 * ONE],
                    _ => [x, y, x + next(12 * ONE) - 6 * ONE, y],
                }
            })
            .collect();
        let index = EdgeColumns::build(&edges);
        INDEX.with(|i| i.set(Some(index)));
        for _ in 0..40 {
            seen[0] += 1;
            let near_edge = edges[next(count as i32) as usize];
            let actor = if next(3) == 0 {
                [
                    (next(2 * span) - span) * ONE / 2,
                    (next(span) - span / 2) * ONE / 2,
                ]
            } else {
                [
                    near_edge[0].clamp(-WORLD_LIMIT, WORLD_LIMIT) + next(4 * ONE) - 2 * ONE,
                    near_edge[1] + next(3 * ONE),
                ]
            };
            let hero = [
                actor[0] + next(30 * ONE) - 15 * ONE,
                actor[1] + next(10 * ONE) - 5 * ONE,
            ];
            let direction = if next(2) == 0 { -1 } else { 1 };
            let all = walker_queries_of(Shape::RUNNER, actor, direction, count, |i| edges[i]);
            match &all {
                Ok(q) => {
                    seen[1] += q.wall as usize;
                    seen[2] += q.floor_ahead as usize;
                }
                Err(_) => seen[4] += 1,
            }
            if line_of_sight(actor, hero, true, count, |i| edges[i]) == Ok(false) {
                seen[3] += 1;
            }
            assert_eq!(
                all,
                walker_queries_near(
                    Shape::RUNNER,
                    actor,
                    direction,
                    count,
                    |i| edges[i],
                    indexed
                ),
                "room {room} actor {actor:?}"
            );
            assert_eq!(
                line_of_sight(actor, hero, true, count, |i| edges[i]),
                line_of_sight_near(actor, hero, true, count, |i| edges[i], indexed),
                "room {room} actor {actor:?} hero {hero:?}"
            );
        }
    }
    // Every kind of answer occurs, so the comparison above is not vacuous.
    assert!(seen[1..].iter().all(|&n| n > 20), "{seen:?}");
}

/// `selected` visits exactly the edges `each_edge` does, in the same order,
/// for random masks and counts on both sides of 128.
#[test]
fn selected_visits_what_each_edge_visits() {
    let mut seed = 0x9e37_79b9u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for round in 0..5000 {
        let count = (next() % 200) as usize;
        let mut mask: EdgeMask = [next(), next(), next(), next()];
        // Sparse, empty and full words too.
        for w in &mut mask {
            match next() % 4 {
                0 => *w &= next() & next(),
                1 => *w = 0,
                2 => *w = u32::MAX,
                _ => {}
            }
        }
        let mut expected = Vec::new();
        each_edge(count, mask, |i| {
            expected.push(i);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            selected(count, mask).collect::<Vec<_>>(),
            expected,
            "round {round} count {count} mask {mask:x?}"
        );
    }
}
