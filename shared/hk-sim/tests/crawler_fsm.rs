//! The Crawler's original graph on the shared HKFSM03 kernel. The walker's terrain senses, recoil and corpse are the
//! actor's; the graph decides when to walk, turn, nudge and wait.
use hk_sim::crawler_fsm::{Crawler, EV_GO_LEFT, EV_GO_RIGHT, GRAPHS};
use hk_sim::fsm3::Env;
use hk_sim::{WalkParams, WalkState, ONE};

const PARAMS: WalkParams = WalkParams { speed: 4 * ONE, turn_ticks: 7, turn_cooldown_ticks: 60 };
const MAINTAIN: u8 = 4;

fn env(direction: i32, initial_turn: bool) -> Env {
    let mut e = Env::new();
    e.walk = WalkState::new(direction, initial_turn);
    e.walk_params = PARAMS;
    e.pos = [0, ONE, 0];
    e
}
fn senses(tick: u32) -> (bool, bool, bool) {
    (tick % 19 != 0, tick % 67 == 3, tick % 83 == 4)
}
fn crawler() -> Crawler {
    let (scene, source, _, _) = GRAPHS[0];
    Crawler::new(scene as usize, source, 1).unwrap()
}

#[test]
fn every_admitted_instance_selects_its_own_checked_graph() {
    assert_eq!(GRAPHS.len(), 10);
    for &(scene, source, _, _) in GRAPHS {
        assert!(Crawler::new(scene as usize, source, 1).is_some());
    }
    assert!(Crawler::new(0, u32::MAX, 1).is_none());
    assert!(Crawler::new(999, GRAPHS[0].1, 1).is_none());
}

#[test]
fn walking_follows_the_walker_state_machine_turn_for_turn() {
    for direction in [-1, 1] {
        for initial_turn in [false, true] {
            let mut c = crawler();
            let mut e = env(direction, initial_turn);
            let mut reference = WalkState::new(direction, initial_turn);
            for tick in 0..1200 {
                e.senses = senses(tick);
                e.velocity = 0;
                c.step(&mut e);
                let expected = reference.tick(PARAMS, e.senses.0, e.senses.1, e.senses.2);
                assert_eq!(e.velocity, expected, "velocity at {tick}");
                assert_eq!(e.walk, reference, "walk at {tick}");
                assert_eq!(c.state(), 0);
            }
        }
    }
}

#[test]
fn a_directed_start_nudges_once_waits_half_a_second_then_keeps_walking() {
    for (event, nudge, scaled, wait_state) in [(EV_GO_LEFT, -6554, false, 2u8), (EV_GO_RIGHT, 6554, true, 3u8)] {
        let mut c = crawler();
        let mut e = env(-1, false);
        c.step(&mut e);
        c.event(&mut e, event);
        assert_eq!(c.state(), wait_state);
        assert_eq!(c.scratch_word(0), 30 * ONE, "the half second wait, in 60 Hz ticks");
        // Self-space x runs against the heading: left is -nudge, right is the same distance leftward (the sprite
        // faces left natively and turns by rotating about y).
        assert_eq!(e.pos[0], if scaled { -nudge } else { nudge });
        let at = e.pos;
        for tick in 1..=30 {
            e.senses = (true, false, false);
            c.step(&mut e);
            assert_eq!(c.state() == MAINTAIN, tick == 30, "FINISHED on the thirtieth tick, not {tick}");
        }
        assert_eq!(c.state(), MAINTAIN);
        assert_eq!(e.pos, at, "the nudge ran once");
    }
}

#[test]
fn paused_reactivation_expires_a_real_wait_but_not_a_scaled_one() {
    for (event, remaining) in [(EV_GO_LEFT, 0), (EV_GO_RIGHT, 29 * ONE)] {
        let mut c = crawler();
        let mut e = env(-1, false);
        c.step(&mut e);
        c.event(&mut e, event);
        assert_eq!(c.scratch_word(0), 30 * ONE);
        // No simulation updates ran during a 90-VBlank pause; the first step after it sees them all.
        e.dt_real = 90 * ONE;
        c.step(&mut e);
        assert_eq!(c.scratch_word(0), remaining);
    }
}

#[test]
fn a_dead_crawler_stops() {
    let mut c = crawler();
    let mut e = env(-1, false);
    c.step(&mut e);
    c.die();
    e.velocity = 0;
    c.step(&mut e);
    assert_eq!(e.velocity, 0);
}

#[test]
fn real_clock_preserves_stalls_and_counter_wrap() {
    let mut c = crawler();
    assert_eq!(c.elapsed_real_ticks(u32::MAX - 2), 0);
    assert_eq!(c.elapsed_real_ticks(1), 4);
    assert_eq!(c.elapsed_real_ticks(91), 90);
    assert_eq!(c.elapsed_real_ticks(100000), u16::MAX);
}
