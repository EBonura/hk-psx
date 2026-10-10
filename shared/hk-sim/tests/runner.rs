use hk_sim::{
    runner::{camera_in_start_range, Action, Clip, Runner, Senses, Swipe, TurnCause, Wait, Walker},
    ONE,
};

fn choose(wait: Wait, _: [u16; 2]) -> u16 {
    wait.endpoints()[1]
}
fn tick(r: &mut Runner) -> Vec<Action> {
    r.step(Senses::default(), choose).iter().collect()
}
fn start(r: &mut Runner) -> Vec<Action> {
    r.step(
        Senses {
            camera_in_start_range: true,
            ..Senses::default()
        },
        choose,
    )
    .iter()
    .collect()
}
fn complete(r: &mut Runner) -> Vec<Action> {
    r.step(
        Senses {
            completed: r.animation(),
            ..Senses::default()
        },
        choose,
    )
    .iter()
    .collect()
}
fn vx(x: i32) -> Action {
    Action::Velocity {
        x: Some(x),
        y: None,
    }
}
fn attack(r: &mut Runner, hero: i32) -> Vec<Action> {
    r.took_damage(hero, 0).iter().collect()
}

#[test]
fn camera_threshold_is_strict_3d_overflow_safe_and_latched() {
    assert!(!camera_in_start_range([60 * ONE, 0, 0], [0; 3]));
    assert!(camera_in_start_range([60 * ONE - 1, 0, 0], [0; 3]));
    assert!(!camera_in_start_range([36 * ONE, 0, 48 * ONE], [0; 3]));
    assert!(!camera_in_start_range([i32::MIN; 3], [i32::MAX; 3]));
    let mut r = Runner::new();
    assert!(tick(&mut r).is_empty());
    let mut draws = Vec::new();
    let actions: Vec<_> = r
        .step(
            Senses {
                camera_in_start_range: true,
                ..Senses::default()
            },
            |w, e| {
                draws.push(e);
                choose(w, e)
            },
        )
        .iter()
        .collect();
    assert_eq!(draws, [[150, 90], [240, 90]]);
    assert_eq!(actions[0], Action::AudioStop);
    assert_eq!(actions[4], Action::AudioPlay);
    assert_eq!(actions[5], vx(-ONE - ONE / 2));
    for _ in 0..30 {
        assert_eq!(tick(&mut r), [vx(-ONE - ONE / 2)]);
    }
    assert_eq!(r.walker(), Walker::Walking);
}

#[test]
fn pause_timing_uses_injected_choices_and_resumes_same_facing() {
    let mut r = Runner::new();
    start(&mut r);
    for _ in 0..88 {
        tick(&mut r);
    }
    assert_eq!(r.walker(), Walker::Walking);
    let pause = tick(&mut r);
    assert_eq!(r.walker(), Walker::Paused);
    assert_eq!(pause[0], Action::AudioStop);
    assert_eq!(pause[2], vx(0));
    for _ in 0..89 {
        assert!(tick(&mut r).is_empty());
    }
    assert_eq!(r.walker(), Walker::Paused);
    let walk = tick(&mut r);
    assert_eq!(r.walker(), Walker::Walking);
    assert_eq!(r.facing(), -1);
    assert_eq!(walk[1..], [Action::AudioPlay, vx(-ONE - ONE / 2)]);
}

#[test]
#[should_panic(expected = "Runner wait outside source endpoints")]
fn rejects_unproven_pause_samples() {
    Runner::new().step(
        Senses {
            camera_in_start_range: true,
            ..Senses::default()
        },
        |_, _| 1,
    );
}

#[test]
fn wall_hero_hole_precedence_and_turn_cooldown() {
    for (wall, visible, expected) in [
        (true, true, TurnCause::Wall),
        (false, true, TurnCause::Hero),
        (false, false, TurnCause::Hole),
    ] {
        let mut r = Runner::new();
        start(&mut r);
        let actions: Vec<_> = r
            .step(
                Senses {
                    wall,
                    floor_ahead: false,
                    hero_x: ONE,
                    in_alert_range: visible,
                    can_see_hero: visible,
                    ..Senses::default()
                },
                choose,
            )
            .iter()
            .collect();
        assert_eq!(
            actions
                .iter()
                .filter_map(|a| if let Action::Turn(c) = a {
                    Some(*c)
                } else {
                    None
                })
                .collect::<Vec<_>>(),
            [expected]
        );
        // Swipe's independent Ready alert interrupts a turn after Walker Update.
        assert_eq!(
            r.swipe(),
            if visible {
                Swipe::Anticipate
            } else {
                Swipe::Ready
            }
        );
    }
    let mut r = Runner::new();
    start(&mut r);
    r.step(
        Senses {
            wall: true,
            ..Senses::default()
        },
        choose,
    );
    let token = r.animation().unwrap();
    for _ in 0..9 {
        assert_eq!(tick(&mut r), [vx(0)]);
    }
    assert_eq!(r.facing(), -1);
    let a = complete(&mut r);
    assert_eq!(r.facing(), 1);
    assert_eq!(r.turn_cooldown(), 50);
    assert_eq!(a.last(), Some(&vx(ONE + ONE / 2)));
    for _ in 0..49 {
        let a: Vec<_> = r
            .step(
                Senses {
                    wall: true,
                    completed: Some(token),
                    ..Senses::default()
                },
                choose,
            )
            .iter()
            .collect();
        assert_eq!(a, [vx(ONE + ONE / 2)]);
    }
    r.step(
        Senses {
            wall: true,
            ..Senses::default()
        },
        choose,
    );
    assert_eq!(r.walker(), Walker::Turning);
    assert_eq!(r.turn_cooldown(), 60);
}

#[test]
fn attack_requires_both_queries_and_tie_faces_left() {
    for (range, sight) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut r = Runner::new();
        r.step(
            Senses {
                in_alert_range: range,
                can_see_hero: sight,
                ..Senses::default()
            },
            choose,
        );
        assert_eq!(
            r.swipe(),
            if range && sight {
                Swipe::Anticipate
            } else {
                Swipe::Ready
            }
        );
    }
    for (hero, direction) in [(-ONE, -1), (0, -1), (ONE, 1)] {
        let mut r = Runner::new();
        let actions = attack(&mut r, hero);
        assert_eq!(r.facing(), direction);
        assert_eq!(
            actions[0..3],
            [
                Action::AudioStop,
                Action::ChaseSound,
                Action::Velocity {
                    x: Some(0),
                    y: Some(0)
                }
            ]
        );
        for _ in 0..200 {
            assert!(tick(&mut r).is_empty());
        }
        assert_eq!(r.walker(), Walker::StoppedForAttack);
        assert_eq!(r.swipe(), Swipe::Anticipate); // no unrelated countdown
    }
}

// Minimal caller-owned clip clock: lengths and rates from the source library.
fn duration(clip: Clip) -> u16 {
    match clip {
        Clip::Anticipate => 5 * 60 / 12,
        Clip::Lunge => 8 * 60 / 12,
        Clip::Cooldown => 60 / 12,
        Clip::Turn => 2 * 60 / 12,
        _ => panic!("loop clip"),
    }
}
fn play_to_end(r: &mut Runner) -> Vec<Action> {
    let token = r.animation().unwrap();
    for _ in 1..duration(token.clip) {
        tick(r);
    }
    complete(r)
}
#[test]
fn actual_clip_completion_drives_chain_and_preserves_y() {
    let mut r = Runner::new();
    attack(&mut r, ONE);
    let stale = r.animation();
    let a = play_to_end(&mut r);
    assert_eq!(r.swipe(), Swipe::Lunge);
    assert_eq!(a[0], Action::DustStart);
    assert_eq!(a[2], vx(6 * ONE));
    assert_eq!(tick(&mut r), [vx(6 * ONE)]);
    let a: Vec<_> = r
        .step(
            Senses {
                completed: stale,
                ..Senses::default()
            },
            choose,
        )
        .iter()
        .collect();
    assert_eq!(a, [vx(6 * ONE)]); // old callback cannot finish lunge
    let a = play_to_end(&mut r);
    assert_eq!(r.swipe(), Swipe::Cooldown);
    assert_eq!(a[0], Action::DustStop);
    assert_eq!(a[2], vx(0));
    assert!(tick(&mut r).is_empty()); // cooldown writes X only on entry
    play_to_end(&mut r);
    assert_eq!(r.swipe(), Swipe::Idle);
    for _ in 0..14 {
        assert!(tick(&mut r).is_empty());
    }
    assert_eq!(r.swipe(), Swipe::Idle);
    assert_eq!(tick(&mut r).last(), Some(&vx(ONE + ONE / 2)));
    assert_eq!(r.swipe(), Swipe::Ready);
    assert_eq!(r.turn_cooldown(), 0);
}

#[test]
fn damage_then_horizontal_recoil_resets_all_attack_phases_vertical_does_not() {
    for transitions in 0..4 {
        let mut r = Runner::new();
        attack(&mut r, ONE);
        for _ in 0..transitions {
            complete(&mut r);
        }
        let phase = r.swipe();
        assert!(r.took_damage(-ONE, 0).iter().next().is_none());
        assert_eq!(r.swipe(), phase); // vertical hit has no global reset
        let out: Vec<_> = r
            .horizontal_recoil(Senses::default(), choose)
            .iter()
            .collect();
        assert_eq!(r.swipe(), Swipe::Ready);
        assert_eq!(r.walker(), Walker::Walking);
        assert_eq!(r.facing(), 1);
        assert_eq!(
            out[1..],
            [Action::AudioPlay, vx(ONE + ONE / 2), vx(ONE + ONE / 2)]
        );
        assert!(!out.contains(&Action::DustStop)); // Reset lacks source dust stop
    }
    let mut r = Runner::new();
    start(&mut r);
    let before = r.animation();
    assert_eq!(
        r.horizontal_recoil(Senses::default(), choose)
            .iter()
            .collect::<Vec<_>>(),
        [vx(-ONE - ONE / 2)]
    );
    assert_eq!(r.animation(), before); // Ready walking doesn't restart animation
    let a = attack(&mut r, -ONE);
    let b: Vec<_> = r
        .horizontal_recoil(Senses::default(), choose)
        .iter()
        .collect();
    assert_eq!(a[1], Action::ChaseSound);
    assert_eq!(b[1], Action::AudioPlay); // retain source ordering; recoil follows
}

#[test]
fn stale_same_clip_callback_cannot_finish_restart_and_death_is_terminal() {
    let mut r = Runner::new();
    attack(&mut r, 0);
    let old = r.animation();
    r.horizontal_recoil(Senses::default(), choose);
    attack(&mut r, 0);
    assert_ne!(old, r.animation());
    r.step(
        Senses {
            completed: old,
            ..Senses::default()
        },
        choose,
    );
    assert_eq!(r.swipe(), Swipe::Anticipate);
    for transitions in 0..4 {
        let mut r = Runner::new();
        attack(&mut r, 0);
        for _ in 0..transitions {
            complete(&mut r);
        }
        let old = r.animation();
        r.die();
        r.die();
        for _ in 0..200 {
            assert!(r
                .step(
                    Senses {
                        completed: old,
                        in_alert_range: true,
                        can_see_hero: true,
                        ..Senses::default()
                    },
                    choose
                )
                .iter()
                .next()
                .is_none());
            assert!(r.took_damage(0, 0).iter().next().is_none());
            assert!(r
                .horizontal_recoil(Senses::default(), choose)
                .iter()
                .next()
                .is_none());
        }
        assert_eq!(r.walker(), Walker::Dead);
        assert_eq!(r.swipe(), Swipe::Dead);
        assert_eq!(r.animation(), None);
    }
    assert!(core::mem::size_of::<Runner>() <= 64);
}

#[test]
fn independent_callbacks_preserve_both_possible_component_orders() {
    let senses = Senses {
        in_alert_range: true,
        can_see_hero: true,
        hero_x: ONE,
        ..Senses::default()
    };
    let mut walker_first = Runner::new();
    start(&mut walker_first);
    let a: Vec<_> = walker_first.step_walker(senses, choose).iter().collect();
    assert!(a.contains(&Action::Turn(TurnCause::Hero)));
    assert_eq!(walker_first.walker(), Walker::Turning);
    walker_first.step_swipe(senses, choose);
    assert_eq!(walker_first.swipe(), Swipe::Anticipate);
    let mut swipe_first = Runner::new();
    start(&mut swipe_first);
    swipe_first.step_swipe(senses, choose);
    assert!(swipe_first
        .step_walker(senses, choose)
        .iter()
        .next()
        .is_none());
    assert_eq!(swipe_first.walker(), Walker::StoppedForAttack);
}

#[test]
fn source_clip_clock_completes_nominal_attack_after_85_ticks() {
    let mut r = Runner::new();
    attack(&mut r, ONE);
    let mut clock = 0;
    let mut phase_start = 0;
    let mut expected = [
        (25, Swipe::Lunge),
        (65, Swipe::Cooldown),
        (70, Swipe::Idle),
        (85, Swipe::Ready),
    ]
    .into_iter();
    let mut next = expected.next();
    while clock < 85 {
        clock += 1;
        let old = r.swipe();
        let done = if old == Swipe::Idle {
            None
        } else if clock - phase_start == duration(r.animation().unwrap().clip) {
            r.animation()
        } else {
            None
        };
        r.step(
            Senses {
                completed: done,
                ..Senses::default()
            },
            choose,
        );
        if r.swipe() != old {
            assert_eq!(next, Some((clock, r.swipe())));
            next = expected.next();
            phase_start = clock;
        }
    }
    assert_eq!(next, None);
    println!(
        "Runner={} bytes; Actions={} bytes",
        core::mem::size_of::<Runner>(),
        core::mem::size_of::<hk_sim::runner::Actions>()
    );
}

fn last_idle_tick(r: &mut Runner) {
    attack(r, ONE);
    complete(r); // Anticipate -> Lunge
    complete(r); // Lunge -> Cooldown
    complete(r); // Cooldown -> Idle
    for _ in 0..14 {
        tick(r);
    }
    assert_eq!(r.swipe(), Swipe::Idle);
}

#[test]
fn idle_reset_checks_ready_immediately_after_synchronous_walker_update() {
    // Original first Runner: Idle at test_frame 348, Anticipate at 363 with
    // both detector values true. This tests the callback, not Unity trajectory.
    let mut r = Runner::new();
    last_idle_tick(&mut r);
    let mut draws = Vec::new();
    let out: Vec<_> = r
        .step_swipe(
            Senses {
                hero_x: ONE,
                in_alert_range: true,
                can_see_hero: true,
                ..Senses::default()
            },
            |w, e| {
                draws.push(w);
                choose(w, e)
            },
        )
        .iter()
        .collect();
    assert_eq!(r.swipe(), Swipe::Anticipate);
    assert_eq!(r.walker(), Walker::StoppedForAttack);
    assert_eq!(draws, [Wait::Walking]);
    assert!(matches!(out[0], Action::Play(a) if a.clip == Clip::Walk));
    assert_eq!(
        out[1..7],
        [
            Action::AudioPlay,
            vx(ONE + ONE / 2),
            vx(ONE + ONE / 2),
            Action::AudioStop,
            Action::ChaseSound,
            Action::Velocity {
                x: Some(0),
                y: Some(0)
            }
        ]
    );
    assert!(matches!(out[7], Action::Play(a) if a.clip == Clip::Anticipate));
    assert_eq!(out.len(), 8);
}

#[test]
fn reset_can_turn_before_clear_cooldown_and_stay_ready_without_sight() {
    // Same source actor's later cycle: Idle 463 -> Turning/Ready 478, cooldown
    // already cleared. Wall versus hole cannot be inferred from that trace;
    // exercise both source-proven branches with explicit query results here.
    for (wall, floor_ahead, cause) in [
        (true, true, TurnCause::Wall),
        (false, false, TurnCause::Hole),
    ] {
        let mut r = Runner::new();
        last_idle_tick(&mut r);
        let out: Vec<_> = r
            .step_swipe(
                Senses {
                    wall,
                    floor_ahead,
                    ..Senses::default()
                },
                choose,
            )
            .iter()
            .collect();
        assert_eq!(r.walker(), Walker::Turning);
        assert_eq!(r.swipe(), Swipe::Ready);
        assert_eq!(r.turn_cooldown(), 0); // not the 60 assigned by BeginTurning
        assert_eq!(out.len(), 6);
        assert_eq!(out[1..4], [Action::AudioPlay, vx(ONE + ONE / 2), vx(0)]);
        assert!(matches!(out[4], Action::Play(a) if a.clip == Clip::Turn));
        assert_eq!(out[5], Action::Turn(cause));
    }
}

#[test]
fn reset_respects_existing_cooldown_until_after_the_nested_update() {
    let mut r = Runner::new();
    start(&mut r);
    r.step_walker(
        Senses {
            wall: true,
            ..Senses::default()
        },
        choose,
    );
    assert_eq!(r.turn_cooldown(), 60);
    attack(&mut r, ONE); // stop that turn; retains source cooldown
    let out: Vec<_> = r
        .horizontal_recoil(
            Senses {
                wall: true,
                ..Senses::default()
            },
            choose,
        )
        .iter()
        .collect();
    assert_eq!(r.walker(), Walker::Walking); // cooldown still suppresses wall
    assert!(!out.iter().any(|a| matches!(a, Action::Turn(_))));
    assert_eq!(r.turn_cooldown(), 0);
    let next: Vec<_> = r
        .step_walker(
            Senses {
                wall: true,
                ..Senses::default()
            },
            choose,
        )
        .iter()
        .collect();
    assert!(next.contains(&Action::Turn(TurnCause::Wall)));
}

#[test]
fn recoil_updates_existing_turn_without_restarting_then_checks_ready() {
    let mut r = Runner::new();
    start(&mut r);
    r.step_walker(
        Senses {
            wall: true,
            ..Senses::default()
        },
        choose,
    );
    let turn = r.animation();
    let out: Vec<_> = r
        .horizontal_recoil(Senses::default(), |_, _| {
            panic!("StartMoving must not draw again during a running turn")
        })
        .iter()
        .collect();
    assert_eq!(out, [vx(0)]);
    assert_eq!(r.animation(), turn);
    assert_eq!(r.turn_cooldown(), 0);
    let out: Vec<_> = r
        .horizontal_recoil(
            Senses {
                completed: turn,
                hero_x: ONE,
                in_alert_range: true,
                can_see_hero: true,
                ..Senses::default()
            },
            choose,
        )
        .iter()
        .collect();
    assert_eq!(r.facing(), 1);
    assert_eq!(r.swipe(), Swipe::Anticipate);
    assert_eq!(out[0], vx(0)); // UpdateTurning before EndTurning/BeginWalking
    assert!(matches!(out[1], Action::Play(a) if a.clip == Clip::Walk));
    assert_eq!(out[4], Action::AudioStop); // immediate Ready interrupts new Walk
}

#[test]
fn startup_turn_and_ready_attack_fit_ordered_command_bound() {
    let mut r = Runner::new();
    let mut draws = Vec::new();
    let out: Vec<_> = r
        .step(
            Senses {
                camera_in_start_range: true,
                wall: true,
                in_alert_range: true,
                can_see_hero: true,
                hero_x: ONE,
                ..Senses::default()
            },
            |w, e| {
                draws.push(w);
                choose(w, e)
            },
        )
        .iter()
        .collect();
    assert_eq!(draws, [Wait::Paused, Wait::Walking]);
    assert_eq!(out.len(), 13);
    assert_eq!(out[0], Action::AudioStop);
    assert_eq!(out[4], Action::AudioPlay);
    assert_eq!(out[5], vx(-ONE - ONE / 2));
    assert_eq!(out[6], vx(0));
    assert_eq!(out[8], Action::Turn(TurnCause::Wall));
    assert_eq!(out[9], Action::AudioStop);
    assert_eq!(out[10], Action::ChaseSound);
    assert!(matches!(out[12], Action::Play(a) if a.clip == Clip::Anticipate));
    assert_eq!(r.swipe(), Swipe::Anticipate);
}

#[test]
fn leaper_launches_on_the_trigger_frame_lands_and_walks_again() {
    use hk_sim::runner::{Animation, Params};
    let mut r = Runner::with_params(Params::LEAPER);
    let start = Senses {
        camera_in_start_range: true,
        ..Senses::default()
    };
    r.step(start, choose);
    assert_eq!(r.walker(), Walker::Walking);
    // Hero 4 units to the right and in range: StopWalker, face, Attack clip.
    let seen = Senses {
        hero_x: 4 * ONE,
        in_alert_range: true,
        can_see_hero: true,
        ..start
    };
    let actions: Vec<_> = r.step(seen, choose).iter().collect();
    assert_eq!(
        (r.walker(), r.swipe(), r.facing()),
        (Walker::StoppedForAttack, Swipe::Anticipate, 1)
    );
    assert!(actions.iter().any(|a| matches!(
        a,
        Action::Play(Animation {
            clip: Clip::Anticipate,
            ..
        })
    )));
    // Fourteen frames of anticipation, then the launch with x = 4 * 1.25.
    for _ in 0..14 {
        assert_eq!(r.swipe(), Swipe::Anticipate);
        r.step(
            Senses {
                grounded: true,
                ..seen
            },
            choose,
        );
    }
    let actions: Vec<_> = r
        .step(
            Senses {
                grounded: true,
                ..seen
            },
            choose,
        )
        .iter()
        .collect();
    assert_eq!(r.swipe(), Swipe::Lunge);
    assert!(actions.contains(&Action::Velocity {
        x: Some(5 * ONE),
        y: Some(20 * ONE)
    }));
    // Airborne: no clip change; landing plays Land (cooldown clip) and zeroes x.
    for _ in 0..10 {
        assert!(r
            .step(
                Senses {
                    grounded: false,
                    ..seen
                },
                choose
            )
            .iter()
            .next()
            .is_none());
    }
    let actions: Vec<_> = r
        .step(
            Senses {
                grounded: true,
                ..seen
            },
            choose,
        )
        .iter()
        .collect();
    assert_eq!(r.swipe(), Swipe::Cooldown);
    assert!(actions.iter().any(|a| matches!(
        a,
        Action::Play(Animation {
            clip: Clip::Cooldown,
            ..
        })
    )));
    let token = r.animation().unwrap();
    r.step(
        Senses {
            completed: Some(token),
            ..seen
        },
        choose,
    );
    assert_eq!(r.swipe(), Swipe::Idle);
    for _ in 0..29 {
        r.step(seen, choose);
    }
    let actions: Vec<_> = r
        .step(
            Senses {
                in_alert_range: false,
                ..seen
            },
            choose,
        )
        .iter()
        .collect();
    assert_eq!((r.walker(), r.swipe()), (Walker::Walking, Swipe::Ready));
    assert!(actions.iter().any(|a| matches!(
        a,
        Action::Play(Animation {
            clip: Clip::Walk,
            ..
        })
    )));
    // No damage or recoil transitions in the Leap FSM.
    assert!(r.took_damage(0, 0).iter().next().is_none());
    assert!(r.horizontal_recoil(seen, choose).iter().next().is_none());
}

#[test]
fn mossman_runner_ignores_a_hit_in_ready_and_walks_on_with_a_knight_behind_it() {
    use hk_sim::runner::Params;
    let mut r = Runner::with_params(Params::MOSSMAN);
    start(&mut r);
    assert_eq!(r.walker(), Walker::Walking);
    // The Crossroads Runner turns on TOOK DAMAGE out of Ready; this older FSM has no such transition.
    assert!(r.took_damage(10 * ONE, 0).iter().next().is_none());
    assert_eq!(r.swipe(), Swipe::Ready);
    // A Knight in range and in sight behind a walking body turns the Runner's Walker, not this one's.
    let behind = Senses {
        hero_x: 5 * ONE,
        in_alert_range: false,
        can_see_hero: true,
        ..Senses::default()
    };
    let mut turned = false;
    for _ in 0..20 {
        turned |= r
            .step(
                Senses {
                    in_alert_range: false,
                    ..behind
                },
                choose,
            )
            .iter()
            .any(|a| matches!(a, Action::Turn(_)));
    }
    assert!(!turned);
    // It still lunges at 9 once the Knight is in its attack range.
    let seen = Senses {
        in_alert_range: true,
        ..behind
    };
    r.step(seen, choose);
    assert_eq!(r.swipe(), Swipe::Anticipate);
    r.step(
        Senses {
            completed: r.animation(),
            ..seen
        },
        choose,
    );
    assert_eq!(r.swipe(), Swipe::Lunge);
    let lunge: Vec<_> = r.step(seen, choose).iter().collect();
    assert!(lunge.contains(&vx(9 * ONE)));
}

#[test]
fn shaker_walks_on_through_the_delay_then_stops_shakes_and_bursts() {
    use hk_sim::runner::{gas, Params};
    let delay = |w: Wait, e: [u16; 2]| if w == Wait::Delay { 10u16 } else { e[1] };
    let mut r = Runner::with_params(Params::SHAKER);
    r.step(
        Senses {
            camera_in_start_range: true,
            ..Senses::default()
        },
        delay,
    );
    let seen = Senses {
        in_alert_range: true,
        can_see_hero: true,
        hero_x: -3 * ONE,
        ..Senses::default()
    };
    r.step(seen, delay);
    assert_eq!(r.swipe(), Swipe::GasDelay);
    assert_eq!(r.walker(), Walker::Walking, "Attack Delay does not stop it");
    // No facing change, no lunge: it keeps the way it was walking (left as authored).
    assert_eq!(r.facing(), -1);
    for _ in 0..9 {
        r.step(seen, delay);
        assert_eq!(r.swipe(), Swipe::GasDelay);
    }
    r.step(seen, delay);
    assert_eq!(
        (r.swipe(), r.walker()),
        (Swipe::GasAntic, Walker::StoppedForAttack)
    );
    assert_eq!(r.gas_ticks(), None);
    for _ in 0..gas::ANTIC_TICKS {
        r.step(seen, delay);
    }
    assert_eq!(r.swipe(), Swipe::Gas);
    let mut live = 0;
    while r.swipe() == Swipe::Gas {
        assert_eq!(r.gas_ticks(), Some(live));
        live += 1;
        r.step(seen, delay);
    }
    assert_eq!(live, gas::BURST_TICKS);
    assert_eq!(r.swipe(), Swipe::GasCool);
    for _ in 0..gas::COOL_TICKS {
        r.step(seen, delay);
    }
    assert_eq!(r.swipe(), Swipe::Idle);
    for _ in 0..gas::IDLE_TICKS {
        r.step(seen, delay);
    }
    // Reset: StartWalker and Ready, whose immediate check sees the Knight and starts again.
    assert_eq!(r.walker(), Walker::Walking);
    assert_eq!(r.swipe(), Swipe::GasDelay);
}

#[test]
fn gas_box_grows_from_a_fifth_to_full_size_and_mirrors_with_the_facing() {
    use hk_sim::runner::gas;
    assert_eq!(gas::scale(0), gas::START_SCALE);
    assert_eq!(gas::scale(gas::TWEEN_TICKS), ONE);
    assert!(gas::scale(6) > gas::scale(3) && gas::scale(12) < ONE);
    // easeOutCirc is front-loaded: a quarter of the way in it is already over half grown.
    assert!(gas::scale(6) > gas::START_SCALE + (ONE - gas::START_SCALE) / 2);
    let left = gas::world([100 * ONE, 10 * ONE], -1, gas::TWEEN_TICKS);
    let right = gas::world([100 * ONE, 10 * ONE], 1, gas::TWEEN_TICKS);
    for (l, r) in left.iter().zip(right.iter()) {
        assert_eq!(l[0] - 100 * ONE, -(r[0] - 100 * ONE));
        assert_eq!(l[1], r[1]);
    }
    // The lowest points sit 1.42 below the Shaker's origin, give or take the collider.
    let low = left.iter().map(|p| p[1]).min().unwrap();
    assert!(low < 10 * ONE - ONE && low > 10 * ONE - 2 * ONE);
}
