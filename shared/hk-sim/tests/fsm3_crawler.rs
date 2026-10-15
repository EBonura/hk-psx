//! The Crawler on HKFSM03 against the Crawler on HKFSM01 (the v1 runtime), tick by tick, over the
//! same sense and event streams. v1 never took the graph's FINISHED transitions (its cooker read the
//! two event names as empty), so the two disagree on the state index after a Wait; they must not
//! disagree on anything the body sees.
use hk_format::fsm::Program;
use hk_sim::crawler_fsm::{Crawler as V1, BANK as V1_BANK, GRAPHS as V1_GRAPHS};
use hk_sim::crawler_fsm3::{Crawler as V2, EV_GO_LEFT, EV_GO_RIGHT, GRAPHS};
use hk_sim::fsm::{Machine, Phase};
use hk_sim::fsm3::{Env, PH_UPDATE};
use hk_sim::fsm_actions::WalkingActions;
use hk_sim::fsm_body::Walking;
use hk_sim::{WalkParams, WalkState, ONE};

const PARAMS: WalkParams = WalkParams { speed: 4 * ONE, turn_ticks: 7, turn_cooldown_ticks: 60 };

fn env(direction: i32, initial_turn: bool, real_ticks: u16) -> Env {
    let mut e = Env::new();
    e.walk = WalkState::new(direction, initial_turn);
    e.walk_params = PARAMS;
    e.pos = [0, ONE, 0];
    e.dt_real = real_ticks as i32 * 65536;
    e
}
fn body(direction: i32, initial_turn: bool, real_ticks: u16) -> WalkingActions<Walking> {
    WalkingActions(Walking {
        walk: WalkState::new(direction, initial_turn),
        position: [0, ONE, 0],
        params: PARAMS,
        senses: (true, false, false),
        velocity: 0,
        real_ticks,
        stationary_fixture: false,
    })
}
fn senses(tick: u32) -> (bool, bool, bool) {
    (tick % 19 != 0, tick % 67 == 3, tick % 83 == 4)
}

#[test]
fn every_admitted_instance_selects_its_own_checked_graph() {
    assert_eq!(GRAPHS.len(), 10);
    for &(scene, source, _, _) in GRAPHS {
        assert!(V2::new(scene as usize, source, 1).is_some());
    }
    assert!(V2::new(0, u32::MAX, 1).is_none());
    assert!(V2::new(999, GRAPHS[0].1, 1).is_none());
}

#[test]
fn walking_matches_v1_over_turns_and_edges() {
    for direction in [-1, 1] {
        for initial_turn in [false, true] {
            let (scene, source, _, _) = GRAPHS[0];
            let mut v2 = V2::new(scene as usize, source, 9).unwrap();
            let (s1, i1, _, _) = V1_GRAPHS[0];
            let mut v1 = V1::new(s1 as usize, i1, 9).unwrap();
            let mut e = env(direction, initial_turn, 2);
            let mut k = body(direction, initial_turn, 2);
            for tick in 0..1200 {
                let s = senses(tick);
                e.senses = s;
                k.0.senses = s;
                e.velocity = 0;
                k.0.velocity = 0;
                v2.step(&mut e);
                v1.step(&mut k).unwrap();
                assert_eq!(e.velocity, k.0.velocity, "velocity at {tick}");
                assert_eq!(e.walk, k.0.walk, "walk at {tick}");
                assert_eq!(e.pos, k.0.position, "position at {tick}");
                assert_eq!(v2.state(), 0);
            }
        }
    }
}

#[test]
fn directed_starts_match_v1_through_wait_and_translate() {
    let (scene, source, _, _) = GRAPHS[0];
    let (s1, i1, start, len) = V1_GRAPHS[0];
    let program = Program::parse(&V1_BANK[start as usize..start as usize + len as usize]).unwrap();
    let _ = (s1, i1);
    // (event, v1 target state, expected v2 state after the wait)
    for (event, target, ev2) in [(0, 2u16, EV_GO_LEFT), (1, 3u16, EV_GO_RIGHT)] {
        let _ = event;
        for real in [1u16, 2] {
            let mut v2 = V2::new(scene as usize, source, 5).unwrap();
            let mut e = env(-1, false, real);
            let mut machine = Machine::new(program, 5);
            let mut k = body(-1, false, real);
            // One quiet step in the Walk state, then the directed start on both.
            let s0 = (true, false, false);
            e.senses = s0;
            k.0.senses = s0;
            v2.step(&mut e);
            machine.step_walk(&program, Phase::Update, &mut k).unwrap();
            let st = program.state(0).unwrap();
            let ev1 = (st.transitions..st.transitions + st.transition_count)
                .find_map(|i| {
                    let (ev, t) = program.transition(i).unwrap();
                    (t == target).then_some(ev)
                })
                .unwrap();
            v2.event(&mut e, ev2);
            machine.event_walk(&program, ev1, &mut k).unwrap();
            assert_eq!(e.pos, k.0.position, "translate on entry");
            assert_eq!(e.walk, k.0.walk);
            let timer_var = if target == 2 { 1 } else { 2 };
            let mut states = vec![];
            for tick in 0..200 {
                let s = senses(tick + 7);
                e.senses = s;
                k.0.senses = s;
                e.velocity = 0;
                k.0.velocity = 0;
                v2.step(&mut e);
                machine.step_walk(&program, Phase::Update, &mut k).unwrap();
                assert_eq!(e.velocity, k.0.velocity, "velocity at {tick}");
                assert_eq!(e.walk, k.0.walk, "walk at {tick}");
                assert_eq!(e.pos, k.0.position, "position at {tick}");
                // The wait's remaining time is the same number in both.
                let t2 = v2.scratch_word(0) as i32;
                if v2.state() == 2 || v2.state() == 3 {
                    assert_eq!(t2, machine.variables[timer_var], "timer at {tick}");
                }
                states.push(v2.state());
            }
            // v2 takes FINISHED to Maintain (4) once the wait ends; v1 stays in the start state.
            assert_eq!(machine.state(), target, "v1 never takes the Wait's FINISHED transition");
            assert_eq!(*states.last().unwrap(), 4);
            assert!(states.iter().take_while(|s| **s != 4).all(|s| *s == if target == 2 { 2 } else { 3 }));
        }
    }
}

#[test]
fn paused_reactivation_expires_a_real_wait_but_not_a_scaled_one() {
    let (scene, source, _, _) = GRAPHS[0];
    for (event, remaining) in [(EV_GO_LEFT, 0), (EV_GO_RIGHT, 29 * ONE)] {
        let mut v2 = V2::new(scene as usize, source, 1).unwrap();
        let mut e = env(-1, false, 1);
        v2.step(&mut e);
        v2.event(&mut e, event);
        assert_eq!(v2.scratch_word(0) as i32, 30 * ONE);
        // No simulation updates ran during a 90-VBlank pause; the first step after it sees them all.
        e.dt_real = 90 * 65536;
        v2.step(&mut e);
        assert_eq!(v2.scratch_word(0) as i32, remaining);
    }
}

#[test]
fn a_dead_crawler_stops() {
    let (scene, source, _, _) = GRAPHS[0];
    let mut v2 = V2::new(scene as usize, source, 1).unwrap();
    let mut e = env(-1, false, 1);
    v2.step(&mut e);
    v2.die();
    e.velocity = 0;
    v2.step(&mut e);
    assert_eq!(e.velocity, 0);
}

#[test]
fn real_clock_preserves_stalls_and_counter_wrap() {
    let (s, id, _, _) = GRAPHS[0];
    let mut c = V2::new(s as usize, id, 1).unwrap();
    assert_eq!(c.elapsed_real_ticks(u32::MAX - 2), 0);
    assert_eq!(c.elapsed_real_ticks(1), 4);
    assert_eq!(c.elapsed_real_ticks(91), 90);
    assert_eq!(c.elapsed_real_ticks(100000), u16::MAX);
    let _ = PH_UPDATE;
}
