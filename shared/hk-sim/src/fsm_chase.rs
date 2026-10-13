//! Vengefly action closure under construction. Unknown actions stay refused;
//! these scalar kernels do not imply admission of either complete graph.
use crate::{
    fsm::{Error, Kernel, Phase, ResultAction},
    fsm_actions::{boolean, event, operand, read, write},
};
use hk_format::fsm::{opcode::*, Action, Program, NO_EVENT, UPDATE};

pub trait Environment {
    fn tick(&self) -> u32;
    fn velocity(&self, _object: i32, _space: i32) -> Result<[i32; 2], Error> {
        Err(Error::UnsupportedAction)
    }
    fn set_velocity(&mut self, _object: i32, _velocity: [Option<i32>; 2]) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    /// Queue force for the original fixed physics boundary, then apply the
    /// source action's X/Y/magnitude clamps to existing velocity immediately.
    /// All limits are present objects, including serialized FsmFloat None.
    fn add_force(
        &mut self,
        _object: i32,
        _force: [i32; 2],
        _limits: [i32; 3],
    ) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn decelerate(&mut self, _object: i32, _amount: i32) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
}
pub struct Actions<E>(pub E);
impl<E: Environment> Kernel for Actions<E> {
    fn execute(
        &mut self,
        p: &Program<'_>,
        a: Action,
        phase: Phase,
        v: &mut [i32],
        _: &mut u32,
    ) -> Result<ResultAction, Error> {
        if matches!(phase, Phase::CollisionEnter | Phase::CollisionStay) {
            return Ok(ResultAction::running());
        }
        let fixed = matches!(a.opcode, SET_VELOCITY_2D | ADD_FORCE_2D | DECELERATE);
        if phase != Phase::Enter
            && ((fixed && phase != Phase::FixedUpdate) || (!fixed && phase != Phase::Update))
        {
            return Ok(ResultAction::running());
        }
        let mut result = ResultAction {
            finished: a.flags & UPDATE == 0,
            event: NO_EVENT,
        };
        match a.opcode {
            GET_VELOCITY_2D => {
                if a.operand_count != 4 {
                    return Err(Error::Parameter);
                }
                let space = read(p, operand(p, a, 3)?, v)?;
                if space != 0 && space != 1 {
                    return Err(Error::Parameter);
                }
                let velocity = self.0.velocity(read(p, operand(p, a, 0)?, v)?, space)?;
                for i in 0..2 {
                    let output = operand(p, a, i + 1)?;
                    if output.kind != 4 {
                        write(p, output, v, velocity[i as usize])?;
                    }
                }
            }
            SET_VELOCITY_2D => {
                if a.operand_count != 3 {
                    return Err(Error::Parameter);
                }
                let mut velocity = [None; 2];
                for i in 0..2 {
                    let value = operand(p, a, i + 1)?;
                    if value.kind != 4 {
                        velocity[i as usize] = Some(read(p, value, v)?);
                    }
                }
                self.0
                    .set_velocity(read(p, operand(p, a, 0)?, v)?, velocity)?;
            }
            ADD_FORCE_2D => {
                if a.operand_count != 6 {
                    return Err(Error::Parameter);
                }
                let force = [
                    read(p, operand(p, a, 1)?, v)?,
                    read(p, operand(p, a, 2)?, v)?,
                ];
                let limits = [
                    read(p, operand(p, a, 3)?, v)?,
                    read(p, operand(p, a, 4)?, v)?,
                    read(p, operand(p, a, 5)?, v)?,
                ];
                if limits.iter().any(|n| *n < 0) {
                    return Err(Error::Parameter);
                }
                self.0
                    .add_force(read(p, operand(p, a, 0)?, v)?, force, limits)?;
            }
            DECELERATE => {
                if a.operand_count != 2 {
                    return Err(Error::Parameter);
                }
                let amount = read(p, operand(p, a, 1)?, v)?;
                if amount < 0 {
                    return Err(Error::Parameter);
                }
                self.0.decelerate(read(p, operand(p, a, 0)?, v)?, amount)?;
            }
            FLOAT_CLAMP => {
                if a.operand_count != 3 {
                    return Err(Error::Parameter);
                }
                let target = operand(p, a, 0)?;
                let low = read(p, operand(p, a, 1)?, v)?;
                let high = read(p, operand(p, a, 2)?, v)?;
                if low > high {
                    return Err(Error::Parameter);
                }
                let value = read(p, target, v)?.clamp(low, high);
                write(p, target, v, value)?;
            }
            BOOL_ALL_TRUE => {
                if a.operand_count < 2 {
                    return Err(Error::Parameter);
                }
                // Original empty arrays do not emit or change storeResult.
                if a.operand_count == 2 {
                    return Ok(result);
                }
                let mut all = true;
                for i in 0..a.operand_count - 2 {
                    all &= boolean(p, operand(p, a, i)?, v)?;
                }
                let output = operand(p, a, a.operand_count - 2)?;
                // The selected graphs have no output. A named output would
                // require the original event-before-store ordering contract.
                if output.kind != 4 {
                    return Err(Error::Parameter);
                }
                if all {
                    result.event = event(operand(p, a, a.operand_count - 1)?)?;
                }
            }
            NEXT_FRAME_EVENT => {
                if a.operand_count != 1 {
                    return Err(Error::Parameter);
                }
                result.finished = phase == Phase::Update;
                if result.finished {
                    result.event = event(operand(p, a, 0)?)?;
                }
            }
            _ => return Err(Error::UnsupportedAction),
        }
        Ok(result)
    }
}

/// TimeManager:8, authored rational fixed interval at time scale 1. The caller
/// advances this once for each processed HZ60 simulation tick, independently of
/// rendering, and shares its result across all enabled enemy components.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixedClock {
    remainder: u32,
}
impl FixedClock {
    pub const SOURCE_RATE: u32 = 141_120_000;
    pub const FIXED_UNITS: u32 = 2_822_399;
    pub const SIMULATION_UNITS: u32 = Self::SOURCE_RATE / 60;
    pub const fn new() -> Self {
        Self { remainder: 0 }
    }
    #[inline(always)]
    pub fn step(&mut self) -> bool {
        self.remainder += Self::SIMULATION_UNITS;
        if self.remainder >= Self::FIXED_UNITS {
            self.remainder -= Self::FIXED_UNITS;
            true
        } else {
            false
        }
    }
}
