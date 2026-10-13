//! Ordered original FSM execution. Action semantics are supplied once by a kernel.
use hk_format::fsm::{Action, Program, ENABLED, FINISHED, MAX_VARIABLES, NO_EVENT, UPDATE};

pub const MAX_TRANSITIONS: usize = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Enter,
    Update,
    CollisionStay,
    CollisionEnter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Program,
    UnsupportedAction,
    Parameter,
    TransitionBudget,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultAction {
    pub finished: bool,
    pub event: u16,
}
impl ResultAction {
    pub const fn running() -> Self {
        Self {
            finished: false,
            event: NO_EVENT,
        }
    }
    pub const fn finished() -> Self {
        Self {
            finished: true,
            event: NO_EVENT,
        }
    }
}
pub trait Kernel {
    fn execute(
        &mut self,
        program: &Program<'_>,
        action: Action,
        phase: Phase,
        variables: &mut [i32; MAX_VARIABLES],
        rng: &mut u32,
    ) -> Result<ResultAction, Error>;
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
    #[inline(always)]
    pub fn new(program: Program<'_>, seed: u32) -> Self {
        let mut variables = [0; MAX_VARIABLES];
        for i in 0..program.variable_count() {
            variables[i as usize] = program.initial(i).unwrap();
        }
        Self {
            variables,
            rng: seed,
            state: program.start_state(),
            finished: 0,
            flags: 0,
        }
    }
    #[inline(always)]
    pub fn state(&self) -> u16 {
        self.state
    }
    #[inline(always)]
    pub fn die(&mut self) {
        self.flags |= DEAD;
    }
    #[inline(always)]
    fn transition(
        &mut self,
        program: &Program<'_>,
        event: u16,
        budget: &mut usize,
    ) -> Result<bool, Error> {
        if event == NO_EVENT {
            return Ok(false);
        }
        let Some(target) = program.target(self.state, event) else {
            return Ok(false);
        };
        if *budget == 0 {
            return Err(Error::TransitionBudget);
        }
        *budget -= 1;
        self.state = target;
        self.finished = 0;
        self.flags &= !(ENTERED | FINISH_SENT);
        Ok(true)
    }
}
// One algorithm emits named static closures. Their call graph contains only
// the selected concrete action group, so the final linker can assign each
// closure to its own room module without generic-symbol ownership guesses.
macro_rules! ordered_executor {
    ($actions:ident,$enter:ident,$step:ident,$event:ident,$transition:ident,$kernel:ty) => {
        impl Machine {
            #[inline(never)]
            fn $actions(
                &mut self,
                program: &Program<'_>,
                phase: Phase,
                kernel: &mut $kernel,
                budget: &mut usize,
            ) -> Result<bool, Error> {
                let state = program.state(self.state).ok_or(Error::Program)?;
                let mut all = 0u16;
                for offset in 0..state.action_count {
                    let bit = 1u16 << offset;
                    let action = program
                        .action(state.actions + offset)
                        .ok_or(Error::Program)?;
                    if action.flags & ENABLED == 0 {
                        continue;
                    }
                    all |= bit;
                    if self.finished & bit != 0 {
                        continue;
                    }
                    if phase == Phase::Update && action.flags & UPDATE == 0 {
                        continue;
                    }
                    let result = kernel.execute(
                        program,
                        action,
                        phase,
                        &mut self.variables,
                        &mut self.rng,
                    )?;
                    if result.finished {
                        self.finished |= bit;
                    }
                    // A handled event immediately interrupts later old-state actions.
                    if self.$transition(program, result.event, budget)? {
                        return Ok(true);
                    }
                }
                if self.finished & all == all && self.flags & FINISH_SENT == 0 {
                    self.flags |= FINISH_SENT;
                    if self.$transition(program, FINISHED, budget)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            #[inline(always)]
            fn $enter(
                &mut self,
                program: &Program<'_>,
                kernel: &mut $kernel,
                budget: &mut usize,
            ) -> Result<(), Error> {
                while self.flags & ENTERED == 0 {
                    self.flags |= ENTERED;
                    if !self.$actions(program, Phase::Enter, kernel, budget)? {
                        break;
                    }
                }
                Ok(())
            }
            #[inline(never)]
            pub fn $step(
                &mut self,
                program: &Program<'_>,
                phase: Phase,
                kernel: &mut $kernel,
            ) -> Result<(), Error> {
                if self.flags & DEAD != 0 {
                    return Ok(());
                }
                let mut budget = MAX_TRANSITIONS;
                self.$enter(program, kernel, &mut budget)?;
                if phase != Phase::Enter && self.$actions(program, phase, kernel, &mut budget)? {
                    self.$enter(program, kernel, &mut budget)?;
                }
                Ok(())
            }
            #[inline(never)]
            pub fn $event(
                &mut self,
                program: &Program<'_>,
                event: u16,
                kernel: &mut $kernel,
            ) -> Result<(), Error> {
                if self.flags & DEAD != 0 {
                    return Ok(());
                }
                let mut budget = MAX_TRANSITIONS;
                if self.$transition(program, event, &mut budget)? {
                    self.$enter(program, kernel, &mut budget)?;
                }
                Ok(())
            }
        }
    };
}
impl Machine {
    #[inline(never)]
    fn transition_walk(
        &mut self,
        program: &Program<'_>,
        event: u16,
        budget: &mut usize,
    ) -> Result<bool, Error> {
        self.transition(program, event, budget)
    }
}
ordered_executor!(actions, enter, step, event, transition, impl Kernel);
ordered_executor!(
    actions_walk,
    enter_walk,
    step_walk,
    event_walk,
    transition_walk,
    crate::fsm_actions::WalkingActions<crate::fsm_body::Walking>
);
ordered_executor!(
    actions_fly,
    enter_fly,
    step_fly,
    event_fly,
    transition,
    crate::fsm_actions::FlyingActions<crate::fsm_body::Flying>
);
