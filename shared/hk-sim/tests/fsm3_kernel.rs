//! The HKFSM03 kernel on hand-assembled banks: the PlayMaker rules every cooked tree relies on (action order,
//! interruption by an event, FINISHED, the transition budget, delayed and cross-FSM events, a Wait).
use hk_format::fsm3::{self as f3, ops, Bank, FINISHED, F_UPDATE, NONE8};
use hk_sim::fsm3::{Env, Tree, PH_UPDATE};

const T_CONST: u16 = f3::T_CONST;

struct Action {
    family: u8,
    op: u8,
    flags: u8,
    operands: Vec<u16>,
}
struct State {
    transitions: Vec<(u8, u8)>,
    actions: Vec<Action>,
}
struct Fsm {
    states: Vec<State>,
    globals: Vec<(u8, u8)>,
    cells: u16,
    bits: u16,
    scratch: u8,
    pool: Vec<i32>,
    start: u8,
}
fn op(tag: u16, payload: u16) -> u16 {
    f3::operand(tag, payload)
}
fn ev(e: u8) -> u16 {
    op(f3::T_EVENT, e as u16)
}
fn lit(v: i32) -> u16 {
    op(f3::T_INLINE, (v as u16) & 0x1fff)
}
fn none() -> u16 {
    op(f3::T_NONE, 0)
}
fn state(transitions: &[(u8, u8)], actions: Vec<Action>) -> State {
    State { transitions: transitions.to_vec(), actions }
}
fn bool_test(bit: u16, t: u16, f: u16, flags: u8) -> Action {
    Action { family: 0, op: ops::flow::BOOL_TEST, flags, operands: vec![op(f3::T_BIT, bit), t, f] }
}
fn put(b: &mut Vec<u8>, v: u16) {
    b.extend(v.to_le_bytes());
}

/// Serialize a tree in the documented layout (host/hk-fsm `v3::serialize` is the cooker's; this is the test's own).
fn assemble(fsms: &[Fsm], events: usize, queue: u8) -> Vec<u8> {
    let mut b = vec![];
    b.extend(b"HKF3");
    b.extend([f3::VERSION, fsms.len() as u8, events as u8, 1]);
    put(&mut b, 0);
    b.extend([queue, 0, 0, 0, 0, 0]);
    let dir_at = b.len();
    b.resize(dir_at + f3::DIR * fsms.len(), 0);
    let mask = fsms.len().div_ceil(8);
    let map_at = b.len();
    b.resize(map_at + events * mask, 0);
    for (f, fsm) in fsms.iter().enumerate() {
        for (e, _) in fsm.states.iter().flat_map(|s| s.transitions.iter()).chain(fsm.globals.iter()) {
            b[map_at + *e as usize * mask + f / 8] |= 1 << (f % 8);
        }
    }
    for (f, fsm) in fsms.iter().enumerate() {
        while b.len() % 2 != 0 {
            b.push(0);
        }
        let start = b.len();
        let table = b.len();
        b.resize(table + 4 * fsm.states.len(), 0);
        let mut blocks = vec![];
        for s in &fsm.states {
            blocks.push(((b.len() - start) / 2) as u16);
            for (e, t) in &s.transitions {
                b.extend([*e, *t]);
            }
            for a in &s.actions {
                b.extend([a.family | a.flags << 4, a.op]);
                for o in &a.operands {
                    put(&mut b, *o);
                }
            }
        }
        for (i, s) in fsm.states.iter().enumerate() {
            b[table + 4 * i..table + 4 * i + 2].copy_from_slice(&blocks[i].to_le_bytes());
            b[table + 4 * i + 2] = s.actions.len() as u8;
            b[table + 4 * i + 3] = s.transitions.len() as u8;
        }
        let globals_at = b.len();
        for (e, t) in &fsm.globals {
            b.extend([*e, *t]);
        }
        let arrays_at = b.len();
        while b.len() % 4 != 0 {
            b.push(0);
        }
        let pool_at = b.len();
        for v in &fsm.pool {
            b.extend(v.to_le_bytes());
        }
        let init_at = b.len();
        let end = b.len();
        let d = dir_at + f3::DIR * f;
        let rec = &mut b[d..d + f3::DIR];
        rec[0] = fsm.states.len() as u8;
        rec[1] = fsm.globals.len() as u8;
        rec[2] = fsm.start;
        rec[3] = fsm.states.iter().map(|s| s.actions.len()).max().unwrap_or(0).div_ceil(8) as u8;
        rec[4..6].copy_from_slice(&fsm.cells.to_le_bytes());
        rec[6..8].copy_from_slice(&fsm.bits.to_le_bytes());
        rec[10] = fsm.scratch;
        for (k, v) in [start, globals_at, arrays_at, pool_at, init_at, end].iter().enumerate() {
            rec[12 + 2 * k..14 + 2 * k].copy_from_slice(&(*v as u16).to_le_bytes());
        }
        rec[24..26].copy_from_slice(&(fsm.pool.len() as u16).to_le_bytes());
    }
    let len = b.len() as u32;
    b[12..16].copy_from_slice(&len.to_le_bytes());
    b
}
fn fsm(states: Vec<State>) -> Fsm {
    Fsm { states, globals: vec![], cells: 1, bits: 2, scratch: 0, pool: vec![], start: 0 }
}

struct Run {
    bank: &'static [u8],
    mem: Vec<u32>,
    env: Env,
}
impl Run {
    fn new(fsms: &[Fsm], events: usize, queue: u8) -> Self {
        let bank: &'static [u8] = Box::leak(assemble(fsms, events, queue).into_boxed_slice());
        let b = Bank::parse(bank).expect("assembled bank parses");
        let mut env = Env::new();
        let mut mem = vec![0u32; b.runtime_bytes().div_ceil(4)];
        // SAFETY: `mem` is the bank's runtime size, 4-aligned.
        unsafe { Tree::init(b, mem.as_mut_ptr().cast(), &mut env, 1) };
        Run { bank, mem, env }
    }
    fn tree(&mut self) -> Tree<'_> {
        // SAFETY: laid out by `new` for this bank.
        unsafe { Tree::attach(Bank::from_verified(self.bank), self.mem.as_mut_ptr().cast(), &mut self.env) }
    }
}

#[test]
fn a_state_whose_actions_all_finish_raises_finished_once() {
    // S0: BoolTest(false -> no event) finishes on entry; FINISHED -> S1; S1 has nothing to do.
    let f = fsm(vec![state(&[(FINISHED, 1)], vec![bool_test(0, none(), none(), 0)]), state(&[], vec![])]);
    let mut run = Run::new(&[f], 1, 0);
    run.tree().step(PH_UPDATE);
    assert_eq!(run.tree().state(0), 1);
    run.tree().step(PH_UPDATE);
    assert_eq!(run.tree().state(0), 1, "FINISHED is sent once per entry");
}

#[test]
fn an_event_interrupts_the_actions_after_it() {
    // S0: set bit0, BoolTest(bit0 -> E1), then SetFloatValue(cell0 = 7). E1 -> S1; the set must never run.
    let f2 = fsm(vec![
        state(
            &[(1, 1)],
            vec![
                Action { family: 1, op: ops::math::SET_BOOL_VALUE, flags: 0, operands: vec![op(f3::T_BIT, 0), lit(1)] },
                bool_test(0, ev(1), none(), 0),
                Action {
                    family: 1,
                    op: ops::math::SET_FLOAT_VALUE,
                    flags: 0,
                    operands: vec![op(f3::T_CELL, 0), lit(7)],
                },
            ],
        ),
        state(&[], vec![]),
    ]);
    let mut run = Run::new(&[f2], 2, 0);
    run.tree().step(PH_UPDATE);
    assert_eq!(run.tree().state(0), 1);
    assert_eq!(run.tree().cell(0, 0), 0, "the SetFloatValue after the event did not run");
}

#[test]
fn a_loop_of_empty_states_runs_out_of_transition_budget_and_stops() {
    // Two empty states: each raises FINISHED on entry and the other takes it.
    let f = fsm(vec![state(&[(FINISHED, 1)], vec![]), state(&[(FINISHED, 0)], vec![])]);
    let mut run = Run::new(&[f], 1, 0);
    run.tree().step(PH_UPDATE);
    assert!(run.tree().dead(0), "the budget stopped it");
    assert_eq!(run.tree().faults(), 1);
    // A stopped FSM does nothing further.
    let at = run.tree().state(0);
    run.tree().step(PH_UPDATE);
    assert_eq!(run.tree().state(0), at);
}

#[test]
fn a_global_transition_is_taken_from_any_state_after_the_states_own() {
    let mut f = fsm(vec![
        state(&[(2, 1)], vec![bool_test(0, none(), none(), F_UPDATE)]),
        state(&[], vec![]),
        state(&[], vec![]),
    ]);
    f.globals = vec![(1, 2), (2, 2)];
    let mut run = Run::new(&[f], 3, 0);
    run.tree().step(PH_UPDATE);
    run.tree().event(0, 1);
    assert_eq!(run.tree().state(0), 2, "the global takes E1 from S0");
    // S0's own transition for E2 wins over the global one: go back to S0 first.
    let mut g = fsm(vec![
        state(&[(2, 1)], vec![bool_test(0, none(), none(), F_UPDATE)]),
        state(&[], vec![]),
        state(&[], vec![]),
    ]);
    g.globals = vec![(2, 2)];
    let mut run = Run::new(&[g], 3, 0);
    run.tree().step(PH_UPDATE);
    run.tree().event(0, 2);
    assert_eq!(run.tree().state(0), 1);
}

#[test]
fn an_event_sent_to_the_tree_reaches_the_other_fsm_after_the_sender_finishes_its_pass() {
    // FSM 0 raises E1 to the whole tree with SendEventByName; FSM 1 waits in S0 for E1.
    let send = Action {
        family: 2,
        op: ops::events::SEND_EVENT_BY_NAME,
        flags: 0,
        operands: vec![lit(1), none(), none(), ev(1), lit(0)],
    };
    let a = fsm(vec![state(&[], vec![send])]);
    let b = fsm(vec![state(&[(1, 1)], vec![bool_test(0, none(), none(), F_UPDATE)]), state(&[], vec![])]);
    let mut run = Run::new(&[a, b], 2, 1);
    run.tree().step(PH_UPDATE);
    assert_eq!(run.tree().state(1), 1, "delivered in the same step, from the queue and not by a nested call");
}

#[test]
fn a_delayed_event_waits_its_ticks() {
    // SendEventByName with a 0.05 s delay is 3 ticks.
    let delay = op(T_CONST, 0);
    let send = Action {
        family: 2,
        op: ops::events::SEND_EVENT_BY_NAME,
        flags: 0,
        operands: vec![lit(1), none(), none(), ev(1), delay],
    };
    let mut a = fsm(vec![state(&[(1, 1)], vec![send]), state(&[], vec![])]);
    a.pool = vec![(0.05 * 65536.0f64).round() as i32];
    let mut run = Run::new(&[a], 2, 1);
    let mut at = None;
    for tick in 0..8 {
        run.tree().step(PH_UPDATE);
        if run.tree().state(0) == 1 && at.is_none() {
            at = Some(tick);
        }
    }
    assert_eq!(at, Some(3));
}

#[test]
fn a_wait_counts_ticks_and_raises_its_event() {
    // Wait 0.5 s (game time): time, finishEvent, realTime, scratch slot.
    let wait = Action {
        family: 3,
        op: ops::timers::WAIT,
        flags: F_UPDATE,
        operands: vec![op(T_CONST, 0), ev(1), lit(0), lit(0)],
    };
    let mut a = fsm(vec![state(&[(1, 1)], vec![wait]), state(&[], vec![])]);
    a.pool = vec![32768];
    a.scratch = 4;
    let mut run = Run::new(&[a], 2, 0);
    for tick in 1..=40 {
        run.tree().step(PH_UPDATE);
        assert_eq!(run.tree().state(0) == 1, tick >= 30, "tick {tick}");
    }
    let _ = NONE8;
}
