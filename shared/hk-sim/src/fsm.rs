//! Ordered original FSM execution. Action semantics are supplied once by a kernel.
use hk_format::fsm::{Action, Program, ENABLED, UPDATE, FINISHED, MAX_VARIABLES, NO_EVENT};

pub const MAX_TRANSITIONS: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Enter, Update, CollisionStay, CollisionEnter }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error { Program, UnsupportedAction, Parameter, TransitionBudget }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultAction { pub finished: bool, pub event: u16 }
impl ResultAction {
    pub const fn running() -> Self { Self { finished: false, event: NO_EVENT } }
    pub const fn finished() -> Self { Self { finished: true, event: NO_EVENT } }
}
pub trait Kernel {
    fn execute(&mut self, program: Program<'_>, action: Action, phase: Phase,
        variables: &mut [i32; MAX_VARIABLES], rng: &mut u32) -> Result<ResultAction, Error>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machine {
    pub variables: [i32; MAX_VARIABLES],
    pub rng: u32,
    state: u16,
    finished: u16,
    flags: u16,
}
const ENTERED: u16 = 1;
const FINISH_SENT: u16 = 2;
const DEAD: u16 = 4;
const _: () = assert!(core::mem::size_of::<Machine>() <= 76);
impl Machine {
    pub fn new(program: Program<'_>, seed: u32) -> Self {
        let mut variables = [0; MAX_VARIABLES];
        for i in 0..program.variable_count() { variables[i as usize] = program.initial(i).unwrap(); }
        Self { variables, rng: seed, state: program.start_state(), finished: 0, flags: 0 }
    }
    pub fn state(&self) -> u16 { self.state }
    pub fn die(&mut self) { self.flags |= DEAD; }
    #[inline(always)]
    fn transition(&mut self, program: Program<'_>, event: u16, budget: &mut usize) -> Result<bool, Error> {
        if event == NO_EVENT { return Ok(false); }
        let Some(target) = program.target(self.state, event) else { return Ok(false); };
        if *budget == 0 { return Err(Error::TransitionBudget); }
        *budget -= 1;
        self.state = target;
        self.finished = 0;
        self.flags &= !(ENTERED | FINISH_SENT);
        Ok(true)
    }
    #[inline(always)]
    fn actions(&mut self, program: Program<'_>, phase: Phase, kernel: &mut impl Kernel,
        budget: &mut usize) -> Result<bool, Error> {
        let state = program.state(self.state).ok_or(Error::Program)?;
        let mut all = 0u16;
        for offset in 0..state.action_count {
            let bit = 1u16 << offset;
            let action = program.action(state.actions + offset).ok_or(Error::Program)?;
            if action.flags & ENABLED == 0 { continue; }
            all |= bit;
            if self.finished & bit != 0 { continue; }
            if phase == Phase::Update && action.flags & UPDATE == 0 { continue; }
            let result = kernel.execute(program, action, phase, &mut self.variables, &mut self.rng)?;
            if result.finished { self.finished |= bit; }
            // A handled event changes state immediately. Later old-state
            // actions must not overwrite the new state's effects.
            if self.transition(program, result.event, budget)? { return Ok(true); }
        }
        if self.finished & all == all && self.flags & FINISH_SENT == 0 {
            self.flags |= FINISH_SENT;
            if self.transition(program, FINISHED, budget)? { return Ok(true); }
        }
        Ok(false)
    }
    #[inline(always)]
    fn enter(&mut self, program: Program<'_>, kernel: &mut impl Kernel,
        budget: &mut usize) -> Result<(), Error> {
        while self.flags & ENTERED == 0 {
            self.flags |= ENTERED;
            if !self.actions(program, Phase::Enter, kernel, budget)? { break; }
        }
        Ok(())
    }
    #[inline(always)]
    pub fn step(&mut self, program: Program<'_>, phase: Phase, kernel: &mut impl Kernel) -> Result<(), Error> {
        if self.flags & DEAD != 0 { return Ok(()); }
        let mut budget = MAX_TRANSITIONS;
        self.enter(program, kernel, &mut budget)?;
        if phase != Phase::Enter && self.actions(program, phase, kernel, &mut budget)? {
            self.enter(program, kernel, &mut budget)?;
        }
        Ok(())
    }
    #[inline(always)]
    pub fn event(&mut self, program: Program<'_>, event: u16, kernel: &mut impl Kernel) -> Result<(), Error> {
        if self.flags & DEAD != 0 { return Ok(()); }
        let mut budget = MAX_TRANSITIONS;
        if self.transition(program, event, &mut budget)? { self.enter(program, kernel, &mut budget)?; }
        Ok(())
    }
}
