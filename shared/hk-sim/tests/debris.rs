#![allow(dead_code)] // includes game modules by path and exercises part of each
use hk_sim::ONE;
const KNIGHT_SCALE: i32 = 60693;
mod render {
    std::thread_local! {pub static VERTS:std::cell::RefCell<Vec<[(i16,i16);4]>>=const {std::cell::RefCell::new(Vec::new())};}
    pub fn texture(_: usize, v: [(i16, i16); 4], _: (u8, u8, u8)) {
        VERTS.with(|a| a.borrow_mut().push(v));
    }
}
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/debris.rs"]
mod debris;
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/impact.rs"]
mod impact;
use debris::{Body, Pool, Spec};
const POLYGON: &[[i32; 2]] = &[
    [-ONE / 2, -ONE / 2],
    [ONE / 2, -ONE / 2],
    [ONE / 2, ONE / 2],
    [-ONE / 2, ONE / 2],
];
/// A SpinSelf-like piece: random start, torque 20, finishing off. `inv_i` is what the
/// guest used to recover from the torque (20 * 14298 / 65536 in Q16), so the recorded
/// trajectories below stay exactly the ones the old Spec produced.
const SPEC: Spec = Spec {
    index: 0,
    owner: 38,
    source: 10618,
    origin: [59 * ONE, 12 * ONE],
    centroid: [0, 0],
    scale: [ONE, ONE],
    points: [[0; 2]; debris::MAX_POINTS],
    len: 0,
    torque: 20 * ONE,
    inv_i: 285_960,
    start_pose: 0,
    random_start: true,
    finishing: false,
    tint: false,
    angle_offset: -60,
}
.with_polygon(POLYGON);
/// The pieces `specs` numbers `id` and puts four to an owner, as the old fixtures did.
fn pieces(count: u16) -> Vec<Spec> {
    (0..count)
        .map(|id| Spec {
            index: id,
            owner: (id / 4) as u8,
            source: 10000 + id as u32,
            ..SPEC
        })
        .collect()
}
fn resolve(specs: &[Spec]) -> impl Fn(u16) -> Option<Spec> + '_ {
    move |i| specs.iter().copied().find(|s| s.index == i)
}
fn of_owner(specs: &[Spec], owner: u8) -> impl Iterator<Item = Spec> + '_ {
    specs.iter().copied().filter(move |s| s.owner == owner)
}
const FLOOR: [i32; 4] = [-512 * ONE, 0, 512 * ONE, 0];
const WORLD: [i32; 4] = [-512 * ONE, -512 * ONE, 512 * ONE, 512 * ONE];
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
fn pool_is_bounded_and_preserves_source_identity_across_residency() {
    let specs = pieces(28);
    let mut p = Pool::new();
    for owner in 0..7 {
        p.spawn(0, of_owner(&specs, owner), 0, 1);
    }
    assert_eq!(p.active(), 28);
    let original = p.body(0).unwrap().center;
    p.spawn(0, of_owner(&specs, 0), 0, -1);
    assert_eq!(p.body(0).unwrap().center, original, "a live piece keeps its flight");
    p.tick(0, resolve(&specs), WORLD, 0, |_| [0; 4]);
    assert_eq!(p.body(0).unwrap().center, original); //50Hz accumulator
    p.tick(0, resolve(&specs), WORLD, 0, |_| [0; 4]);
    assert_ne!(p.body(0).unwrap().center, original);
    let held = p.body(0).unwrap().center;
    for _ in 0..120 {
        p.tick(0, resolve(&specs), [0, 0, ONE, ONE], 0, |_| [0; 4]);
    }
    assert_eq!(p.body(0).unwrap().center, held);
    p.clear_scene(1);
    assert_eq!(p.active(), 28);
    p.clear_scene(0);
    assert_eq!(p.active(), 0);
    // Seven more bytes a piece than the old pool's slots, for index, rest and the finishing state.
    assert!(core::mem::size_of::<Pool>() <= 2900, "{}", core::mem::size_of::<Pool>());
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
fn original_frames_only_after_activation_and_shared_draw_budget_is_bounded() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let mut p = Pool::new();
    let specs = pieces(28);
    let cam = (59 * ONE, 12 * ONE);
    let frame = |_: u16| Some(0u16);
    assert_eq!(p.draw(0, resolve(&specs), frame, &r, cam), 0);
    p.spawn(0, of_owner(&specs, 0), 0, 1);
    assert_eq!(p.draw(0, resolve(&specs), frame, &r, cam), 4);
    assert_eq!(p.draw(1, resolve(&specs), frame, &r, cam), 0);
    assert_eq!(
        p.draw(0, resolve(&specs), |_| None, &r, cam),
        0,
        "a region with no frame for a piece does not draw it"
    );
    for owner in 1..7 {
        p.spawn(0, of_owner(&specs, owner), 0, 1);
    }
    let drawn = p.draw(0, resolve(&specs), frame, &r, cam);
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

// ---- the classes the cooker now admits

/// Two source callbacks at vx = 5: the second applies the one AddTorque step.
fn spin_after_two_steps(spec: Spec, vx: i32) -> i32 {
    let mut b = Body::launch(spec, 0, 1);
    b.velocity = [vx, 0];
    b.omega = 0;
    b.step(spec, 0, |_| [0; 4]);
    b.step(spec, 0, |_| [0; 4]);
    b.omega
}

#[test]
fn a_negative_coefficient_turns_against_the_speed_as_simple_rock_and_spin_self_simple_do() {
    let against = Spec {
        torque: -SPEC.torque,
        ..SPEC
    };
    let with = spin_after_two_steps(SPEC, 5 * ONE);
    assert!(with > 0);
    assert_eq!(spin_after_two_steps(against, 5 * ONE), -with);
    assert_eq!(spin_after_two_steps(against, -5 * ONE), with);
}

#[test]
fn a_spinless_piece_keeps_its_authored_pose_and_never_turns_in_flight() {
    let spinless = Spec {
        torque: 0,
        random_start: false,
        start_pose: 123 * ONE,
        ..SPEC
    };
    // The break's offset turns it by angle_offset * facing for a slash from the side.
    let b = Body::launch(spinless, 0, 1);
    assert_eq!(b.angle, (123 - 60) * ONE);
    assert_eq!(Body::launch(spinless, 0, -1).angle, (123 + 60) * ONE);
    // A random-start piece of the same spec does not keep it.
    let random = Body::launch(Spec { random_start: true, ..spinless }, 0, 1);
    assert_ne!(random.angle, (123 - 60) * ONE);
    assert_eq!(spin_after_two_steps(spinless, 5 * ONE), 0);
}

#[test]
fn contact_response_follows_the_inverse_inertia_not_the_spin() {
    // The guest recovered the inertia from the torque, right only at factor 4, so a
    // piece with factor 8 or 10 answered a contact two to two and a half times too lightly
    // in rotation. With vx = 0 the torque step is nothing, so any difference is the contact's.
    let flight = |spec: Spec| {
        let mut b = Body::launch(spec, 0, 1);
        b.center = [0, 3 * ONE];
        b.angle = 20 * ONE;
        b.velocity = [0, -9 * ONE];
        for _ in 0..30 {
            b.step(spec, 1, |_| FLOOR);
        }
        (b.angle, b.omega, b.center)
    };
    let a = flight(SPEC);
    assert_eq!(flight(Spec { torque: 4 * SPEC.torque, ..SPEC }), a, "the spin does not enter");
    assert_ne!(flight(Spec { inv_i: SPEC.inv_i / 3, ..SPEC }), a, "the inverse inertia does");
}

fn settle(p: &mut Pool, specs: &[Spec], ticks: usize) {
    for _ in 0..ticks {
        p.tick(0, resolve(specs), WORLD, 1, |_| FLOOR);
    }
}

fn dropped_on_the_floor(count: u16, finishing: bool) -> (Vec<Spec>, Pool) {
    let specs: Vec<Spec> = (0..count)
        .map(|id| Spec {
            index: id,
            owner: 0,
            source: 500 + id as u32,
            origin: [id as i32 * 2 * ONE, ONE],
            finishing,
            ..SPEC
        })
        .collect();
    let mut p = Pool::new();
    p.spawn(0, specs.iter().copied(), 0, 1);
    (specs, p)
}

#[test]
fn a_finishing_piece_sleeps_eight_seconds_shrinks_for_one_and_is_freed() {
    let (specs, mut p) = dropped_on_the_floor(1, true);
    let mut asleep_at = None;
    for tick in 0..4000 {
        settle(&mut p, &specs, 1);
        if p.body(0).is_some_and(|b| b.stopped) {
            asleep_at = Some(tick);
            break;
        }
    }
    assert!(asleep_at.is_some(), "it comes to rest on the floor");
    // `rest` counts 60 Hz ticks from the first tick after the body stopped.
    settle(&mut p, &specs, (debris::FINISH_WAIT + debris::FINISH_SHRINK - 1) as usize);
    assert_eq!(p.active(), 1, "one tick short of the end of the shrink");
    settle(&mut p, &specs, 1);
    assert_eq!(p.active(), 0);
    assert_eq!(p.finished, 1);
}

#[test]
fn a_piece_without_the_finishing_component_stays_for_good() {
    let (specs, mut p) = dropped_on_the_floor(1, false);
    settle(&mut p, &specs, 4000 + 2 * 540);
    assert!(p.body(0).unwrap().stopped);
    assert_eq!(p.active(), 1);
}

#[test]
fn a_finishing_piece_that_stays_off_screen_ten_draws_is_freed_and_others_never() {
    let raw = room();
    let r = hk_format::Room::parse(&raw).unwrap();
    let frame = |_: u16| Some(0u16);
    let far = (900 * ONE, 900 * ONE);
    for (finishing, expected) in [(true, 0), (false, 1)] {
        let (specs, mut p) = dropped_on_the_floor(1, finishing);
        for _ in 0..9 {
            p.draw(0, resolve(&specs), frame, &r, far);
        }
        assert_eq!(p.active(), 1, "nine draws are not ten");
        p.draw(0, resolve(&specs), frame, &r, far);
        assert_eq!(p.active(), expected);
    }
    // One visible draw resets the count.
    let (specs, mut p) = dropped_on_the_floor(1, true);
    for _ in 0..9 {
        p.draw(0, resolve(&specs), frame, &r, far);
    }
    p.draw(0, resolve(&specs), frame, &r, (0, ONE));
    for _ in 0..9 {
        p.draw(0, resolve(&specs), frame, &r, far);
    }
    assert_eq!(p.active(), 1);
}

#[test]
fn a_full_pool_retires_its_longest_stopped_piece_and_drops_when_all_still_move() {
    let (mut specs, mut p) = dropped_on_the_floor(28, false);
    // All 28 moving: the 29th is dropped and counted.
    let extra = Spec { index: 100, owner: 1, ..SPEC };
    p.spawn(0, std::iter::once(extra), 0, 1);
    assert_eq!((p.active(), p.dropped, p.evicted), (28, 1, 0));
    // Let them all come to rest; the first to stop has rested longest.
    specs.push(extra);
    settle(&mut p, &specs, 3000);
    let stopped = (0..28).filter(|&i| p.body(i).unwrap().stopped).count();
    assert_eq!(stopped, 28);
    p.spawn(0, std::iter::once(extra), 0, 1);
    assert_eq!((p.active(), p.evicted), (28, 1));
    assert!(p.body(100).is_some(), "the newcomer is in");
    assert_eq!((0..28).filter(|&i| p.body(i).is_some()).count(), 27);
}

#[test]
fn the_bank_decodes_into_the_spec_the_cooker_meant() {
    // tests/golden/fragment_bank.hkwm is written by tools/world_metadata.py
    // (tests/test_world_metadata_fragments.py pins it); its first region's first two
    // objects are the pieces of owner 3.
    let bytes = include_bytes!("../../../tests/golden/fragment_bank.hkwm");
    let bank = hk_format::WorldMeta::parse(bytes).unwrap();
    let region = bank.region(0).unwrap();
    let plain = debris::decode(&region.object(0).unwrap(), 0).unwrap();
    let rock = debris::decode(&region.object(1).unwrap(), 1).unwrap();
    assert_eq!((plain.index, plain.owner, plain.source), (0, 3, 4));
    assert_eq!((plain.random_start, plain.finishing, plain.tint), (false, false, false));
    assert_eq!(plain.start_pose, 0);
    assert_eq!(plain.torque, (157_286i64 * 8 * 37_549 / 65_536) as i32);
    assert_eq!((rock.owner, rock.source, rock.start_pose), (3, 5, 64 * 123 << 10));
    assert_eq!((rock.random_start, rock.finishing, rock.tint), (true, true, true));
    // Truncated toward zero, as every product of the solver is.
    assert_eq!(rock.torque, (157_286i64 * -15 * 37_549 / 65_536) as i32);
    assert_eq!((rock.inv_i, rock.scale), (157_286, [-ONE, ONE]));
    assert_eq!((rock.origin, rock.centroid), ([10 * ONE, -3 * ONE], [1000, -2000]));
    assert_eq!(rock.polygon(), &[[-32768, -65536], [32768, -65536], [32768, 65536], [-32768, 65536]]);
    // Anything that is not a piece is not decoded.
    assert!(debris::decode(&region.object(2).unwrap(), 2).is_none());
}
