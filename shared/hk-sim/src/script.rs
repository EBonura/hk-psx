//! The shared script executor (P10 step 4, first slice).
//!
//! Comparisons and branches, booleans and ints, waits, and synchronous state
//! and event sends. Everything else in the measured action inventory reaches
//! native code through one typed escape, `Op::Native`, so the opcode set stays
//! small while the supported action set grows. docs/SCRIPT_IR.md has the shape
//! and the reasoning.
//!
//! Every bound here is fixed and checked. A dropped event is the failure mode
//! that would be hardest to notice, so exceeding a bound is a refusal the
//! caller sees, never a silent no-op.

/// Ops are a fixed 8-byte record so the guest decodes by table index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub code: Code,
    /// Opcode-specific selector: the comparison for `IntCompare`, the value for
    /// `SetBool`, the active flag for `Activate`.
    pub flags: u8,
    pub a: u16,
    pub b: u16,
    pub c: u16,
}
/// The opcode numbers are part of the cooked format, so `host/script_ir.py`
/// reads this enum's variant order on import and refuses to run when it drifts.
/// Adding a variant anywhere but the end changes every cooked bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Nop,
    /// Block for `a` ticks, then send event `b` to self.
    Wait,
    /// Variable `a` becomes `flags != 0`.
    SetBool,
    /// Variable `a` becomes constant `b`.
    SetInt,
    /// Send `b` when variable `a` is true, `c` when false. Zero means no event.
    BoolTest,
    /// Send `c` when `var[a] <cmp> const[b]` holds, with `cmp` from `flags`.
    IntCompare,
    /// Send event `a` to self, synchronously.
    SendEvent,
    /// Send event `a` to self at the start of the next tick.
    NextFrameEvent,
    /// Call native function `flags` with the values in variables `a`, `b` and
    /// `c`. The id is in `flags` so all three operands can be arguments: an
    /// object plus a two-component value is the common shape.
    Native,
    /// The same call with `a`, `b` and `c` as constant-pool indices instead of
    /// variable slots. Cook-time-resolved object references arrive this way, so
    /// an action over a fixed set of objects needs no temporaries.
    NativeConst,
    /// Read PlayerData field `b` into variable `a`.
    PlayerDataGet,
    /// Write variable `b` into PlayerData field `a`.
    PlayerDataSet,
    /// Send `b` when PlayerData field `a` is true, `c` when false. The source
    /// action is one step, so this is too: no compiler temporary is needed.
    PlayerDataBoolTest,
    /// Send `c` while the hero body is in phase `flags` with the trigger volume
    /// of the object at constant `a`. `Trigger2dEvent` is the source action,
    /// and it is live for as long as its state is current, so the cooker always
    /// sets `REPEAT` on it: without that the volume would be sampled once on
    /// state entry and never again, which is a trigger that silently never
    /// fires rather than one that refused to compile.
    HeroTrigger,
}
/// An op with this flag bit set re-runs every tick for as long as its state is
/// current, which is what PlayMaker's `everyFrame` means: the action's OnEnter
/// runs once with the rest of the state, then its OnUpdate runs each frame.
/// Without it a condition authored to be watched would be tested once and the
/// state would sit there for ever.
pub const REPEAT: u8 = 0x80;
/// `IntCompare` selectors, in the order the cooker emits them. They share the
/// flags byte with `REPEAT`, so a selector never uses the high bit.
pub const CMP_LT: u8 = 0;
pub const CMP_LE: u8 = 1;
pub const CMP_EQ: u8 = 2;
pub const CMP_NE: u8 = 3;
pub const CMP_GE: u8 = 4;
pub const CMP_GT: u8 = 5;
/// `HeroTrigger` phases, in the declaration order of PlayMaker's
/// `Trigger2DType`, which is the field the source action stores. They share the
/// flags byte with `REPEAT` the same way the comparison selectors do.
pub const TRIGGER_ENTER: u8 = 0;
pub const TRIGGER_STAY: u8 = 1;
pub const TRIGGER_EXIT: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub struct State {
    pub first_op: u16,
    pub op_count: u16,
    pub first_transition: u16,
    pub transition_count: u16,
}
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    pub event: u16,
    pub target: u16,
}
/// One cooked FSM definition. Instances bind to it and hold only their own
/// variables and position.
#[derive(Clone, Copy)]
pub struct Program<'a> {
    pub states: &'a [State],
    pub transitions: &'a [Transition],
    pub ops: &'a [Op],
    pub constants: &'a [i32],
    /// Slots this definition's instances need from the caller's pool.
    pub var_count: u16,
}
/// Bounds, all checked. Raising one is a deliberate budget change.
///
/// Variables are not among them: they live in a pool the caller owns, with a
/// span per instance, because the measured distribution runs from zero to 107
/// slots and a fixed array wide enough for the tail would be mostly padding in
/// the 85.5% of FSMs that use sixteen or fewer.
pub const MAX_PENDING: usize = 8;
pub const MAX_DEPTH: u8 = 4;
pub const MAX_OPS_PER_TICK: u16 = 256;

/// Why a tick stopped early. Anything but `Ran` is a refusal the caller sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    Ran,
    /// More than `MAX_OPS_PER_TICK` ops in one tick.
    OpBudget,
    /// A synchronous send nested deeper than `MAX_DEPTH`.
    Depth,
    /// The delayed-event queue was full.
    PendingFull,
    /// The program referenced a state, op, variable or constant out of range.
    BadProgram,
}
/// What a `Native` op asks the host game to do. The executor never interprets
/// it; the caller maps the id to an implemented function and refuses unknown
/// ids at cook time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeCall {
    pub id: u8,
    pub args: [i32; 3],
}
/// The reserved object id every script means by "the Knight". Source ids are
/// path ids masked to 31 bits, so the top of that range cannot collide.
pub const HERO: i32 = 0x7fff_ffff;
/// No object. A native call given this refuses rather than acting on a default.
pub const NO_OBJECT: i32 = 0;
/// The services a running script needs from the game around it. Keeping this a
/// trait rather than a closure is what lets later slices add services without
/// rewriting every call site.
pub trait Host {
    fn native(&mut self, call: NativeCall);
    /// PlayerData field `index`, through the cooked field table. Progression
    /// flags live in the persistent store, so a script reads the same value the
    /// save file holds.
    fn player_data(&mut self, field: u16) -> i32;
    fn set_player_data(&mut self, field: u16, value: i32);
    /// Whether the hero body is entering, staying inside or leaving `object`'s
    /// trigger volume this tick, for one of the three `TRIGGER_` phases.
    ///
    /// The host remembers last tick's overlap, not the executor. Unity raises
    /// `OnTriggerEnter2D` from the physics step rather than from the state that
    /// listens, so an FSM that enters a state while the Knight is already
    /// standing inside sees Stay and never Enter; an edge kept per op here
    /// would instead fire Enter on every re-entry to that state, which is a
    /// wrong answer the trace comparison would only catch by luck.
    ///
    /// `NO_OBJECT` is inside nothing, so the host answers false for it rather
    /// than falling back to some other volume.
    fn hero_trigger(&mut self, object: i32, phase: u8) -> bool;
}
/// One running FSM instance.
#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub state: u16,
    /// Next op within the current state; reset on every state entry.
    pub cursor: u16,
    pub wait: u16,
    /// The event a finished wait sends, zero when none is pending.
    wait_event: u16,
    pending: [u16; MAX_PENDING],
    pending_len: u8,
    pub finished: bool,
}
impl Default for Instance {
    fn default() -> Self {
        Self::new()
    }
}
impl Instance {
    pub const fn new() -> Self {
        Self {
            state: 0,
            cursor: 0,
            wait: 0,
            wait_event: 0,
            pending: [0; MAX_PENDING],
            pending_len: 0,
            finished: false,
        }
    }
    /// Enter `state` from the top, as PlayMaker does on every transition.
    fn enter(&mut self, state: u16) {
        self.state = state;
        self.cursor = 0;
        self.wait = 0;
        self.wait_event = 0;
    }
    /// Deliver an event from outside this instance: a trigger volume the
    /// Knight entered, a collision, or a send from another FSM. It is the same
    /// synchronous path a self-send takes, so the target state's ops run before
    /// this returns, which is what PlayMaker does and what the Runner's
    /// same-callback restart depends on.
    ///
    /// An event the current state has no transition for is ignored, exactly as
    /// PlayMaker ignores it. That is not a silent drop: the state genuinely has
    /// nothing to do with it.
    pub fn receive(
        &mut self,
        p: Program,
        vars: &mut [i32],
        host: &mut impl Host,
        event: u16,
    ) -> Halt {
        if vars.len() < p.var_count as usize {
            return Halt::BadProgram;
        }
        match self.send(p, event, vars, host, 0) {
            Ok(Some(target)) => {
                self.enter(target);
                Halt::Ran
            }
            Ok(None) => Halt::Ran,
            Err(halt) => halt,
        }
    }
    /// One 60 Hz tick. `native` receives every `Op::Native` in order and
    /// returns the value the call produced, which the op stores nowhere: a
    /// native call that has to return something writes through its own system.
    pub fn tick(&mut self, p: Program, vars: &mut [i32], host: &mut impl Host) -> Halt {
        if vars.len() < p.var_count as usize {
            return Halt::BadProgram;
        }
        // Delayed sends from the previous tick run first, in order.
        let drained = self.pending_len;
        self.pending_len = 0;
        for i in 0..drained as usize {
            let event = self.pending[i];
            if let Some(target) = transition_for(p, self.state, event) {
                self.enter(target);
            }
        }
        if self.wait > 0 {
            self.wait -= 1;
            if self.wait > 0 {
                return Halt::Ran;
            }
            let event = core::mem::take(&mut self.wait_event);
            if event != 0 {
                if let Some(target) = transition_for(p, self.state, event) {
                    self.enter(target);
                }
            }
        }
        let mut budget = MAX_OPS_PER_TICK;
        loop {
            let Some(state) = p.states.get(self.state as usize) else {
                return Halt::BadProgram;
            };
            if self.cursor >= state.op_count {
                // The state's one-shot pass is done, so from here only the ops
                // authored everyFrame keep running, once each per tick.
                return self.repeat(p, *state, vars, host, &mut budget);
            }
            if budget == 0 {
                return Halt::OpBudget;
            }
            budget -= 1;
            let index = state.first_op as usize + self.cursor as usize;
            let Some(op) = p.ops.get(index).copied() else {
                return Halt::BadProgram;
            };
            self.cursor += 1;
            match self.run(p, op, vars, host, 0) {
                Ok(Some(target)) => self.enter(target),
                Ok(None) => {
                    if self.wait > 0 {
                        return Halt::Ran;
                    }
                }
                Err(halt) => return halt,
            }
        }
    }
    /// The everyFrame pass for a state whose ops have all run once.
    fn repeat(
        &mut self,
        p: Program,
        state: State,
        vars: &mut [i32],
        host: &mut impl Host,
        budget: &mut u16,
    ) -> Halt {
        for i in 0..state.op_count {
            let Some(op) = p.ops.get(state.first_op as usize + i as usize).copied() else {
                return Halt::BadProgram;
            };
            if op.flags & REPEAT == 0 {
                continue;
            }
            if *budget == 0 {
                return Halt::OpBudget;
            }
            *budget -= 1;
            match self.run(p, op, vars, host, 0) {
                // A repeated op that transitions leaves this state at once, and
                // the ops after it do not get their repeat this tick.
                Ok(Some(target)) => {
                    self.enter(target);
                    return Halt::Ran;
                }
                Ok(None) if self.wait > 0 => return Halt::Ran,
                Ok(None) => {}
                Err(halt) => return halt,
            }
            if self.cursor == 0 {
                // `run` entered a new state through a synchronous send.
                return Halt::Ran;
            }
        }
        Halt::Ran
    }
    /// Run one op. `Ok(Some(state))` means it transitioned.
    fn run(
        &mut self,
        p: Program,
        op: Op,
        vars: &mut [i32],
        host: &mut impl Host,
        depth: u8,
    ) -> Result<Option<u16>, Halt> {
        let var =
            |i: u16| -> Result<i32, Halt> { vars.get(i as usize).copied().ok_or(Halt::BadProgram) };
        match op.code {
            Code::Nop => Ok(None),
            Code::Wait => {
                self.wait = op.a.max(1);
                self.wait_event = op.b;
                Ok(None)
            }
            Code::SetBool => {
                *vars.get_mut(op.a as usize).ok_or(Halt::BadProgram)? =
                    i32::from(op.flags & !REPEAT != 0);
                Ok(None)
            }
            Code::SetInt => {
                let value = *p.constants.get(op.b as usize).ok_or(Halt::BadProgram)?;
                *vars.get_mut(op.a as usize).ok_or(Halt::BadProgram)? = value;
                Ok(None)
            }
            Code::BoolTest => {
                let event = if var(op.a)? != 0 { op.b } else { op.c };
                self.send(p, event, vars, host, depth)
            }
            Code::IntCompare => {
                let left = var(op.a)?;
                let right = *p.constants.get(op.b as usize).ok_or(Halt::BadProgram)?;
                let holds = match op.flags & !REPEAT {
                    CMP_LT => left < right,
                    CMP_LE => left <= right,
                    CMP_EQ => left == right,
                    CMP_NE => left != right,
                    CMP_GE => left >= right,
                    CMP_GT => left > right,
                    _ => return Err(Halt::BadProgram),
                };
                if holds {
                    self.send(p, op.c, vars, host, depth)
                } else {
                    Ok(None)
                }
            }
            Code::SendEvent => self.send(p, op.a, vars, host, depth),
            Code::NextFrameEvent => {
                if op.a == 0 {
                    return Ok(None);
                }
                if self.pending_len as usize >= MAX_PENDING {
                    return Err(Halt::PendingFull);
                }
                self.pending[self.pending_len as usize] = op.a;
                self.pending_len += 1;
                Ok(None)
            }
            Code::Native => {
                host.native(NativeCall {
                    id: op.flags & !REPEAT,
                    args: [var(op.a)?, var(op.b)?, var(op.c)?],
                });
                Ok(None)
            }
            Code::NativeConst => {
                let konst = |i: u16| -> Result<i32, Halt> {
                    p.constants.get(i as usize).copied().ok_or(Halt::BadProgram)
                };
                host.native(NativeCall {
                    id: op.flags & !REPEAT,
                    args: [konst(op.a)?, konst(op.b)?, konst(op.c)?],
                });
                Ok(None)
            }
            Code::PlayerDataGet => {
                let value = host.player_data(op.b);
                *vars.get_mut(op.a as usize).ok_or(Halt::BadProgram)? = value;
                Ok(None)
            }
            Code::PlayerDataSet => {
                let value = var(op.b)?;
                host.set_player_data(op.a, value);
                Ok(None)
            }
            Code::PlayerDataBoolTest => {
                let event = if host.player_data(op.a) != 0 {
                    op.b
                } else {
                    op.c
                };
                self.send(p, event, vars, host, depth)
            }
            Code::HeroTrigger => {
                let phase = op.flags & !REPEAT;
                if phase > TRIGGER_EXIT {
                    return Err(Halt::BadProgram);
                }
                let object = *p.constants.get(op.a as usize).ok_or(Halt::BadProgram)?;
                if host.hero_trigger(object, phase) {
                    self.send(p, op.c, vars, host, depth)
                } else {
                    Ok(None)
                }
            }
        }
    }
    /// A synchronous send: the target state is entered inside the sending op,
    /// as PlayMaker does, and its own ops run before this one returns.
    fn send(
        &mut self,
        p: Program,
        event: u16,
        vars: &mut [i32],
        host: &mut impl Host,
        depth: u8,
    ) -> Result<Option<u16>, Halt> {
        if event == 0 {
            return Ok(None);
        }
        if depth >= MAX_DEPTH {
            return Err(Halt::Depth);
        }
        let Some(target) = transition_for(p, self.state, event) else {
            return Ok(None);
        };
        // Enter here rather than returning, so a nested send inside the target
        // sees the state it actually landed in.
        self.enter(target);
        let state = *p.states.get(target as usize).ok_or(Halt::BadProgram)?;
        for i in 0..state.op_count {
            let op = *p
                .ops
                .get(state.first_op as usize + i as usize)
                .ok_or(Halt::BadProgram)?;
            self.cursor = i + 1;
            match self.run(p, op, vars, host, depth + 1)? {
                Some(next) => return Ok(Some(next)),
                None if self.wait > 0 => return Ok(None),
                None => {}
            }
        }
        Ok(None)
    }
}
fn transition_for(p: Program, state: u16, event: u16) -> Option<u16> {
    let s = p.states.get(state as usize)?;
    let first = s.first_transition as usize;
    let end = first + s.transition_count as usize;
    p.transitions
        .get(first..end)?
        .iter()
        .find(|t| t.event == event)
        .map(|t| t.target)
}

#[cfg(test)]
mod tests {
    use super::*;
    const START: u16 = 0;
    const DONE: u16 = 1;
    /// Events are interned ids; zero means none.
    const FINISHED: u16 = 1;
    const YES: u16 = 2;

    fn op(code: Code, flags: u8, a: u16, b: u16, c: u16) -> Op {
        Op {
            code,
            flags,
            a,
            b,
            c,
        }
    }
    /// A host that records native calls and keeps four PlayerData fields.
    #[derive(Default)]
    struct Recorder {
        calls: [Option<NativeCall>; 4],
        count: usize,
        fields: [i32; 4],
        /// The one volume and phase this host answers true for; the hero is
        /// outside everything else.
        inside: Option<(i32, u8)>,
        last_ask: Option<(i32, u8)>,
        asks: usize,
    }
    impl Host for Recorder {
        fn native(&mut self, call: NativeCall) {
            if self.count < self.calls.len() {
                self.calls[self.count] = Some(call);
            }
            self.count += 1;
        }
        fn player_data(&mut self, field: u16) -> i32 {
            self.fields.get(field as usize).copied().unwrap_or(0)
        }
        fn set_player_data(&mut self, field: u16, value: i32) {
            if let Some(slot) = self.fields.get_mut(field as usize) {
                *slot = value;
            }
        }
        fn hero_trigger(&mut self, object: i32, phase: u8) -> bool {
            self.last_ask = Some((object, phase));
            self.asks += 1;
            self.inside == Some((object, phase))
        }
    }
    #[test]
    fn a_wait_blocks_for_its_ticks_then_takes_its_transition() {
        let ops = [op(Code::Wait, 0, 3, FINISHED, 0)];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 0,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: FINISHED,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        for tick in 0..3 {
            assert_eq!(i.tick(p, &mut vars, &mut Recorder::default()), Halt::Ran);
            assert_eq!(i.state, START, "tick {tick} is still waiting");
        }
        assert_eq!(i.tick(p, &mut vars, &mut Recorder::default()), Halt::Ran);
        assert_eq!(i.state, DONE);
    }
    #[test]
    fn a_bool_test_sends_the_branch_its_variable_selects() {
        let ops = [op(Code::BoolTest, 0, 0, YES, 0)];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 0,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, START, "a false variable takes no branch");
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        vars[0] = 1;
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, DONE);
    }
    #[test]
    fn an_int_compare_uses_its_selector_and_the_constant_pool() {
        let ops = [op(Code::IntCompare, CMP_GE, 0, 0, YES)];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 0,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[33],
            var_count: 2,
        };
        for (soul, expected) in [(32, START), (33, DONE), (99, DONE)] {
            let mut i = Instance::new();
            let mut vars = [0i32; 2];
            vars[0] = soul;
            i.tick(p, &mut vars, &mut Recorder::default());
            assert_eq!(i.state, expected, "soul {soul}");
        }
    }
    #[test]
    fn a_send_runs_the_target_state_inside_the_sending_op() {
        // START sends YES; DONE then calls native 7 in the same tick, which is
        // the synchronous nesting PlayMaker has and a queue would not give.
        let ops = [
            op(Code::SendEvent, 0, YES, 0, 0),
            op(Code::Native, 7, 0, 1, 2),
        ];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 1,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 3,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 3];
        vars[0] = 11;
        vars[1] = 22;
        vars[2] = 33;
        let mut host = Recorder::default();
        i.tick(p, &mut vars, &mut host);
        assert_eq!(i.state, DONE);
        assert_eq!(
            host.calls[0],
            Some(NativeCall {
                id: 7,
                args: [11, 22, 33]
            })
        );
    }
    #[test]
    fn a_next_frame_event_waits_for_the_next_tick() {
        let ops = [op(Code::NextFrameEvent, 0, YES, 0, 0)];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 0,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, START, "not in the tick that queued it");
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, DONE);
    }
    #[test]
    fn a_self_transition_loop_hits_the_op_budget_rather_than_hanging() {
        // One state that sends itself back to itself for ever.
        let ops = [op(Code::SendEvent, 0, YES, 0, 0)];
        let states = [State {
            first_op: 0,
            op_count: 1,
            first_transition: 0,
            transition_count: 1,
        }];
        let transitions = [Transition {
            event: YES,
            target: START,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        assert_eq!(
            i.tick(p, &mut vars, &mut Recorder::default()),
            Halt::Depth,
            "the nesting bound catches it first"
        );
    }
    #[test]
    fn player_data_reads_and_writes_the_hosts_progression_flags() {
        const HAS_DASH: u16 = 2;
        // Read the flag, branch on it, and record having done so.
        let ops = [
            op(Code::PlayerDataBoolTest, 0, HAS_DASH, YES, 0),
            op(Code::PlayerDataSet, 0, HAS_DASH, 0, 0),
            op(Code::PlayerDataGet, 0, 1, HAS_DASH, 0),
        ];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 2,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 2,
        };
        let mut host = Recorder::default();
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        i.tick(p, &mut vars, &mut host);
        assert_eq!(i.state, START, "a false flag takes no branch");
        host.fields[HAS_DASH as usize] = 1;
        vars[0] = 7;
        let mut i = Instance::new();
        i.tick(p, &mut vars, &mut host);
        assert_eq!(i.state, DONE);
        assert_eq!(
            host.fields[HAS_DASH as usize], 7,
            "the write went to the host"
        );
        assert_eq!(vars[1], 7, "and the read came back from it");
    }
    #[test]
    fn a_const_native_call_carries_cook_time_object_references() {
        // ActivateAllChildren over a static parent is one call per child, with
        // the child's cooked id and the active flag straight from the pool, so
        // no compiler temporary is needed for a fixed set of objects.
        const ACTIVATE: u8 = 3;
        let ops = [
            op(Code::NativeConst, ACTIVATE, 0, 2, 0),
            op(Code::NativeConst, ACTIVATE, 1, 2, 0),
        ];
        let states = [State {
            first_op: 0,
            op_count: 2,
            first_transition: 0,
            transition_count: 0,
        }];
        let p = Program {
            states: &states,
            transitions: &[],
            ops: &ops,
            constants: &[4242, 4243, 1],
            var_count: 0,
        };
        let mut host = Recorder::default();
        let mut i = Instance::new();
        i.tick(p, &mut [], &mut host);
        assert_eq!(
            host.calls[0],
            Some(NativeCall {
                id: ACTIVATE,
                args: [4242, 1, 4242]
            })
        );
        assert_eq!(
            host.calls[1],
            Some(NativeCall {
                id: ACTIVATE,
                args: [4243, 1, 4242]
            })
        );
    }
    #[test]
    fn an_every_frame_condition_keeps_being_tested_after_its_state_settles() {
        // The Superdash's ground charge watches Y Speed every frame; without
        // the repeat pass a state like this would test once and sit for ever.
        let ops = [
            op(Code::SetBool, 1, 0, 0, 0),
            op(Code::IntCompare, CMP_GE | REPEAT, 1, 0, YES),
        ];
        let states = [
            State {
                first_op: 0,
                op_count: 2,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 2,
                op_count: 0,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[5],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        for tick in 0..4 {
            assert_eq!(i.tick(p, &mut vars, &mut Recorder::default()), Halt::Ran);
            assert_eq!(
                i.state, START,
                "tick {tick}: the watched value is still low"
            );
            assert_eq!(vars[0], 1, "the one-shot op ran once and stays run");
        }
        vars[1] = 9;
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, DONE, "the repeated compare caught it");
    }
    #[test]
    fn the_first_test_that_transitions_wins_and_the_rest_of_the_state_is_skipped() {
        // `FsmState::ActivateActions` in the shipped PlayMaker.dll returns as
        // soon as `Fsm.IsSwitchingState`, so a state's remaining actions never
        // run once one of them has transitioned. Every conditional dialogue
        // branch in the game depends on that: Elderbug's 21 branches are a
        // priority list in declaration order, not independent tests.
        const NO: u16 = 3;
        let ops = [
            op(Code::BoolTest, 0, 0, YES, 0),
            op(Code::BoolTest, 0, 1, NO, 0),
        ];
        let states = [
            State {
                first_op: 0,
                op_count: 2,
                first_transition: 0,
                transition_count: 2,
            },
            State {
                first_op: 2,
                op_count: 0,
                first_transition: 2,
                transition_count: 0,
            },
            State {
                first_op: 2,
                op_count: 0,
                first_transition: 2,
                transition_count: 0,
            },
        ];
        let transitions = [
            Transition {
                event: YES,
                target: 1,
            },
            Transition {
                event: NO,
                target: 2,
            },
        ];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 2,
        };
        // Both conditions hold, so declaration order decides.
        let mut i = Instance::new();
        let mut vars = [1i32, 1];
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, 1, "the first branch won");
        // With only the second true, it is reached because the first did not fire.
        let mut i = Instance::new();
        let mut vars = [0i32, 1];
        i.tick(p, &mut vars, &mut Recorder::default());
        assert_eq!(i.state, 2);
    }
    #[test]
    fn an_event_from_outside_runs_the_target_state_at_once() {
        // A trigger volume the Knight entered, a collision, or another FSM's
        // send. Together those are the single largest group of actions the
        // compiler still refuses, and all of them arrive this way.
        const ACTED: u8 = 9;
        let ops = [op(Code::Native, ACTED, 0, 0, 0)];
        let states = [
            State {
                first_op: 0,
                op_count: 0,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[],
            var_count: 1,
        };
        let mut host = Recorder::default();
        let mut i = Instance::new();
        let mut vars = [0i32; 1];
        assert_eq!(i.receive(p, &mut vars, &mut host, YES), Halt::Ran);
        assert_eq!(i.state, DONE);
        assert_eq!(host.count, 1, "the target state ran inside the delivery");
        // An event this state has no transition for is ignored, as PlayMaker
        // ignores it, rather than being queued or refused.
        assert_eq!(i.receive(p, &mut vars, &mut host, YES), Halt::Ran);
        assert_eq!(i.state, DONE);
    }
    #[test]
    fn a_hero_trigger_keeps_watching_its_volume_and_sends_when_the_hero_arrives() {
        // Trigger2dEvent listens for as long as its state is current, so the
        // cooker always sets REPEAT on it. Sampled once on entry it would be a
        // trigger that never fires again, which is the silent wrong answer
        // rather than the refusal.
        const VOLUME: i32 = 4242;
        let ops = [op(Code::HeroTrigger, TRIGGER_STAY | REPEAT, 0, 0, YES)];
        let states = [
            State {
                first_op: 0,
                op_count: 1,
                first_transition: 0,
                transition_count: 1,
            },
            State {
                first_op: 1,
                op_count: 0,
                first_transition: 1,
                transition_count: 0,
            },
        ];
        let transitions = [Transition {
            event: YES,
            target: DONE,
        }];
        let p = Program {
            states: &states,
            transitions: &transitions,
            ops: &ops,
            constants: &[VOLUME],
            var_count: 0,
        };
        let mut host = Recorder::default();
        let mut i = Instance::new();
        for tick in 0..3 {
            assert_eq!(i.tick(p, &mut [], &mut host), Halt::Ran);
            assert_eq!(i.state, START, "tick {tick}: the hero is somewhere else");
        }
        assert_eq!(
            host.last_ask,
            Some((VOLUME, TRIGGER_STAY)),
            "the volume is the cooked object id, the phase the authored one"
        );
        // Four asks for three ticks: the entry tick runs the op in the one-shot
        // pass and again in that same tick's repeat pass, then every later tick
        // runs only the repeat.
        assert_eq!(host.asks, 4);
        host.inside = Some((VOLUME, TRIGGER_STAY));
        i.tick(p, &mut [], &mut host);
        assert_eq!(i.state, DONE);
    }
    #[test]
    fn a_hero_trigger_phase_the_executor_does_not_know_refuses() {
        // The three phases are PlayMaker's Trigger2DType. A fourth value means
        // the bank and this executor disagree, which must stop rather than pick
        // a phase.
        let ops = [op(Code::HeroTrigger, TRIGGER_EXIT + 1, 0, 0, YES)];
        let states = [State {
            first_op: 0,
            op_count: 1,
            first_transition: 0,
            transition_count: 0,
        }];
        let p = Program {
            states: &states,
            transitions: &[],
            ops: &ops,
            constants: &[4242],
            var_count: 0,
        };
        let mut i = Instance::new();
        assert_eq!(
            i.tick(p, &mut [], &mut Recorder::default()),
            Halt::BadProgram
        );
    }
    #[test]
    fn a_bad_index_refuses_rather_than_reading_past_the_program() {
        let ops = [op(Code::SetInt, 0, 0, 9, 0)];
        let states = [State {
            first_op: 0,
            op_count: 1,
            first_transition: 0,
            transition_count: 0,
        }];
        let p = Program {
            states: &states,
            transitions: &[],
            ops: &ops,
            constants: &[1],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        assert_eq!(
            i.tick(p, &mut vars, &mut Recorder::default()),
            Halt::BadProgram
        );
    }
    #[test]
    fn the_pending_queue_refuses_rather_than_dropping_an_event() {
        // The queue drains at the start of every tick, so overflowing it takes
        // one state that queues more than the bound within a single tick.
        let many = [op(Code::NextFrameEvent, 0, YES, 0, 0); MAX_PENDING + 1];
        let states = [State {
            first_op: 0,
            op_count: many.len() as u16,
            first_transition: 0,
            transition_count: 0,
        }];
        let p = Program {
            states: &states,
            transitions: &[],
            ops: &many,
            constants: &[],
            var_count: 2,
        };
        let mut i = Instance::new();
        let mut vars = [0i32; 2];
        assert_eq!(
            i.tick(p, &mut vars, &mut Recorder::default()),
            Halt::PendingFull
        );
    }
}
