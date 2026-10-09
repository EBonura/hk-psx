use hk_sim::ONE;
const KNIGHT_SCALE: i32 = 60693;
mod render {
    std::thread_local! {pub static VERTS:std::cell::RefCell<Vec<[(i16,i16);4]>>=const {std::cell::RefCell::new(Vec::new())};}
    pub fn texture(_: usize, v: [(i16, i16); 4], _: (u8, u8, u8)) {
        VERTS.with(|a| a.borrow_mut().push(v));
    }
}
#[path = "../../../game/src/debris.rs"]
mod debris;
#[path = "../../../game/src/impact.rs"]
mod impact;
use debris::{Body, Pool, Spec};
const POLYGON: &[[i32; 2]] = &[
    [-ONE / 2, -ONE / 2],
    [ONE / 2, -ONE / 2],
    [ONE / 2, ONE / 2],
    [-ONE / 2, ONE / 2],
];
const SPEC: Spec = Spec {
    id: 0,
    door: 38,
    frame: 0,
    source: 10618,
    origin: [59 * ONE, 12 * ONE],
    centroid: [0, 0],
    scale: [ONE, ONE],
    polygon: POLYGON,
    torque: 20 * ONE,
    angle_offset: -60,
};
#[test]
fn source_fling_ranges_include_broad_downward_case() {
    assert_eq!(debris::launch_range(0, 1), (30, 70, 2));
    assert_eq!(debris::launch_range(1, -1), (120, 160, 2));
    assert_eq!(debris::launch_range(2, -1), (70, 110, 3));
    assert_eq!(debris::launch_range(3, 1), (160, 380, 2));
    for kind in 0..4 {
        let a = Body::launch(SPEC, kind, 1);
        let b = Body::launch(SPEC, kind, 1);
        assert_eq!(a.center, b.center);
        assert_eq!(a.velocity, b.velocity);
        assert_eq!(a.angle, b.angle);
        assert_eq!(a.angle % ONE, 0);
        assert!((0..360 * ONE).contains(&a.angle));
    }
}
#[test]
fn torque_occurs_on_second_source_callback_once_and_gravity_is_sixty() {
    let mut b = Body::launch(SPEC, 0, 1);
    b.velocity = [5 * ONE, 0];
    b.omega = 0;
    let y = b.center[1];
    b.step(SPEC, 0, |_| [0; 4]);
    assert_eq!(b.omega, 0);
    assert_eq!(b.velocity[1], -60 * ONE / 50);
    assert_eq!(b.center[1], y + (-60 * ONE / 50) / 50);
    b.step(SPEC, 0, |_| [0; 4]);
    let expected = (100 * ONE as i64 * 1000 / 1001) as i32;
    assert_eq!(b.omega, expected);
    b.step(SPEC, 0, |_| [0; 4]);
    assert_eq!(b.omega, (expected as i64 * 1000 / 1001) as i32);
    let mut left = Body::launch(SPEC, 0, -1);
    left.velocity = [-5 * ONE, 0];
    left.step(SPEC, 0, |_| [0; 4]);
    left.step(SPEC, 0, |_| [0; 4]);
    assert_eq!(left.omega, -expected);
}
#[test]
fn original_pivot_centroid_and_mirrored_parent_are_preserved() {
    let s = Spec {
        centroid: [ONE / 3, -ONE / 5],
        scale: [-ONE, 2 * ONE],
        ..SPEC
    };
    let b = Body::launch(s, 0, 1);
    assert_eq!(b.point([0, 0], s.centroid, s.scale), s.origin);
    assert_eq!(debris::rotate([ONE, 0], 90 * ONE), [0, ONE]);
    assert_eq!(debris::rotate([ONE, 0], 180 * ONE), [-ONE, 0]);
}
#[test]
fn source_floor_bounce_settles_without_invented_lifetime() {
    let mut b = Body::launch(SPEC, 0, 1);
    b.center = [0, ONE];
    b.angle = 0;
    b.velocity = [0, -40 * ONE];
    let floor = [-512 * ONE, 0, 512 * ONE, 0];
    b.step(SPEC, 1, |_| floor);
    assert!(!b.stopped);
    assert!(b.center[1] >= ONE / 2 - 2);
    for _ in 0..1000 {
        b.step(SPEC, 1, |_| floor);
    }
    assert!(b.center[1] >= ONE / 2 - 8);
    eprintln!("rest {:?}", b);
    assert!(b.stopped);
    assert_eq!(b.velocity, [0; 2]);
    assert_eq!(b.omega, 0);
    let rest = b.center;
    for _ in 0..100 {
        b.step(SPEC, 1, |_| floor);
    }
    assert_eq!(b.center, rest);
}
#[test]
fn pool_is_permanent_bounded_and_preserves_source_identity_across_residency() {
    let specs: Vec<_> = (0..28)
        .map(|id| Spec {
            id,
            door: (id / 4) as u16,
            source: 10000 + id as u32,
            ..SPEC
        })
        .collect();
    let mut p = Pool::new();
    for door in 0..7 {
        p.spawn(0, &specs, door, 0, 1);
    }
    assert_eq!(p.active(), 28);
    let original = p.body(0).unwrap().center;
    p.spawn(0, &specs, 0, 0, -1);
    assert_eq!(p.body(0).unwrap().center, original);
    p.tick(
        0,
        &specs,
        [-512 * ONE, -512 * ONE, 512 * ONE, 512 * ONE],
        0,
        |_| [0; 4],
    );
    assert_eq!(p.body(0).unwrap().center, original); //50Hz accumulator
    p.tick(
        0,
        &specs,
        [-512 * ONE, -512 * ONE, 512 * ONE, 512 * ONE],
        0,
        |_| [0; 4],
    );
    assert_ne!(p.body(0).unwrap().center, original);
    let held = p.body(0).unwrap().center;
    for _ in 0..120 {
        p.tick(0, &specs, [0, 0, ONE, ONE], 0, |_| [0; 4]);
    }
    assert_eq!(p.body(0).unwrap().center, held);
    p.clear_scene(1);
    assert_eq!(p.active(), 28);
    p.clear_scene(0);
    assert_eq!(p.active(), 0);
    assert!(core::mem::size_of::<Pool>() <= 2500);
}
#[test]
fn disabled_zero_edges_never_stop_a_fragment() {
    let mut b = Body::launch(SPEC, 0, 1);
    for _ in 0..5 {
        b.step(SPEC, 1, |_| [0; 4]);
    }
    assert!(!b.stopped);
}

fn room() -> Vec<u8> {
    let mut b = Vec::from(*b"HKROOM02");
    for n in [1u32, 1, 0, 1, 2, 0, 0, 0] {
        b.extend(n.to_le_bytes());
    }
    for n in [0u16, 0, 0, 4, 4, 0] {
        b.extend(n.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    for n in [0i32, -ONE / 2, -ONE / 2, ONE / 2, ONE / 2] {
        b.extend(n.to_le_bytes());
    }
    for _ in 0..2 {
        for n in [0u32, 1, 30 * 65536, 2] {
            b.extend(n.to_le_bytes());
        }
    }
    b.resize(b.len() + 32 + 32768, 0);
    b
}
#[test]
fn four_original_frames_only_after_activation_and_shared_draw_budget_is_bounded() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut p = Pool::new();
    let specs: Vec<_> = (0..28)
        .map(|id| Spec {
            id,
            door: (id / 4) as u16,
            source: 10000 + id as u32,
            ..SPEC
        })
        .collect();
    assert_eq!(p.draw(0, &specs, &r, (59 * ONE, 12 * ONE)), 0);
    p.spawn(0, &specs, 0, 0, 1);
    assert_eq!(p.draw(0, &specs, &r, (59 * ONE, 12 * ONE)), 4);
    assert_eq!(p.draw(1, &specs, &r, (59 * ONE, 12 * ONE)), 0);
    for door in 1..7 {
        p.spawn(0, &specs, door, 0, 1);
    }
    let drawn = p.draw(0, &specs, &r, (59 * ONE, 12 * ONE));
    assert_eq!(drawn, 28);
    let mut impact = impact::Pool::new();
    for i in 0..32 {
        impact.spawn(0, i, [0; 4], [0; 4], 1);
    }
    assert_eq!(
        impact.draw_limited(
            0,
            Some(impact::Spec {
                clips: [0, 1],
                ticks: 10
            }),
            &r,
            (0, 0),
            32 - drawn
        ),
        (4, 28)
    );
    let vertices = render::VERTS.with(|v| v.borrow().clone());
    assert!(vertices.iter().all(|v| v
        .iter()
        .all(|q| (-1024..1024).contains(&q.0) && (-1024..1024).contains(&q.1))));
}
#[test]
fn walls_ceiling_and_large_source_edges_stop_without_tunneling() {
    for (velocity, edge) in [
        ([40 * ONE, 0], [ONE, -512 * ONE, ONE, 512 * ONE]),
        ([0, 40 * ONE], [-512 * ONE, ONE, 512 * ONE, ONE]),
    ] {
        let mut b = Body::launch(SPEC, 0, 1);
        b.center = [0, 0];
        b.angle = 0;
        b.velocity = velocity;
        b.step(SPEC, 1, |_| edge);
        assert!(!b.stopped);
        assert!(b.center[0] <= ONE / 2 + 8 && b.center[1] <= ONE / 2 + 8);
    }
}

#[test]
fn fourth_step_cached_speed_causes_source_bounce_once_per_contact() {
    let mut b = Body::launch(SPEC, 0, 1);
    b.center = [0, 4 * ONE];
    b.velocity = [0, -8 * ONE];
    b.angle = 0;
    let floor = [-512 * ONE, 0, 512 * ONE, 0];
    let mut observed = false;
    for _ in 0..100 {
        let before = b.bounces;
        b.step(SPEC, 1, |_| floor);
        if b.bounces > before {
            assert!(b.velocity[1] > 0);
            observed = true;
            break;
        }
    }
    assert!(
        observed,
        "source speed/displacement sampled before ground contact"
    );
}

#[test]
fn bulk_pose_and_manifold_preserve_recorded_fixed_point_trajectories() {
    // Golden captured before transform/manifold hoisting: 64 launch seeds,
    // mirrored/nonuniform parents, floor/walls/slopes, 600 source callbacks.
    let terrain = [
        [-20 * ONE, 0, 20 * ONE, 0],
        [-20 * ONE, 0, -20 * ONE, 20 * ONE],
        [20 * ONE, 0, 20 * ONE, 20 * ONE],
        [-6 * ONE, 4 * ONE, 6 * ONE, 7 * ONE],
    ];
    let mut hash = 0xcbf29ce484222325u64;
    for seed in 0..64 {
        let spec = Spec {
            source: 10618 + seed,
            centroid: [ONE / 7, -ONE / 9],
            scale: [
                if seed & 1 == 0 { ONE } else { -ONE },
                ONE + ((seed % 3) as i32) * ONE / 3,
            ],
            ..SPEC
        };
        let mut body = Body::launch(spec, (seed % 4) as u8, if seed & 2 == 0 { 1 } else { -1 });
        body.center = [((seed % 13) as i32 - 6) * ONE, 8 * ONE];
        for _ in 0..600 {
            body.step(spec, terrain.len(), |i| terrain[i]);
            for word in [
                body.center[0],
                body.center[1],
                body.velocity[0],
                body.velocity[1],
                body.angle,
                body.omega,
                i32::from(body.stopped),
                i32::from(body.bounces),
            ] {
                for byte in word.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    assert_eq!(
        hash, 0xe18f1fc7929a0772,
        "every trajectory word must match the pre-hoist reference"
    );
}
