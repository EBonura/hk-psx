//! The checked walking action closure required by original Crawler graphs.
use crate::fsm::{Error, Phase, ResultAction};
use crate::fsm_actions::{event, Environment};
use hk_format::fsm::{opcode::*, Action, Operand, Program, NO_EVENT, UPDATE};

// Checked decoding is shared once within this walking image. The underlying
// validation rules are the same helpers used by the reference/flying kernels.
#[inline(never)]
fn operand(p: &Program<'_>, a: Action, index: u8) -> Result<Operand, Error> {
    crate::fsm_actions::operand(p, a, index)
}
#[inline(never)]
fn read(p: &Program<'_>, o: Operand, v: &[i32]) -> Result<i32, Error> {
    crate::fsm_actions::read(p, o, v)
}
#[inline(never)]
fn write(p: &Program<'_>, o: Operand, v: &mut [i32], x: i32) -> Result<(), Error> {
    crate::fsm_actions::write(p, o, v, x)
}
#[inline(never)]
fn value(p: &Program<'_>, a: Action, index: u8, v: &[i32]) -> Result<i32, Error> {
    read(p, operand(p, a, index)?, v)
}
#[inline(never)]
fn flag(p: &Program<'_>, a: Action, index: u8, v: &[i32]) -> Result<bool, Error> {
    match value(p, a, index, v)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Parameter),
    }
}

#[inline(never)]
pub(crate) fn execute<E: Environment>(
    env: &mut E,
    p: &Program<'_>,
    a: Action,
    phase: Phase,
    v: &mut [i32],
) -> Result<ResultAction, Error> {
    if phase == Phase::CollisionStay || phase == Phase::CollisionEnter {
        return Ok(ResultAction::running());
    }
    let mut result = ResultAction {
        finished: a.flags & UPDATE == 0,
        event: NO_EVENT,
    };
    if a.opcode == WAIT {
        let duration = value(p, a, 0, v)?;
        let timer = operand(p, a, 1)?;
        let real = flag(p, a, 3, v)?;
        let remaining = if phase == Phase::Enter {
            duration.max(0).checked_mul(60).ok_or(Error::Parameter)?
        } else {
            read(p, timer, v)?
                .saturating_sub(env.elapsed_ticks(real)? as i32 * 65536)
                .max(0)
        };
        write(p, timer, v, remaining)?;
        result.finished = remaining == 0;
        if result.finished {
            result.event = event(operand(p, a, 2)?)?;
        }
    } else if a.opcode == TRANSLATE {
        let mut delta = [None; 3];
        for i in 0..3 {
            let o = operand(p, a, i + 1)?;
            if o.kind != 4 {
                delta[i as usize] = Some(read(p, o, v)?);
            }
        }
        env.translate(
            value(p, a, 0, v)?,
            delta,
            flag(p, a, 4, v)?,
            flag(p, a, 5, v)?,
        )?;
    } else if a.opcode == WALK_LEFT_RIGHT {
        env.walk(
            value(p, a, 0, v)?,
            value(p, a, 1, v)?,
            flag(p, a, 2, v)?,
            flag(p, a, 3, v)?,
            flag(p, a, 4, v)?,
            phase,
        )?;
        result.finished = false;
    } else if a.opcode == BOOL_TEST {
        let value = flag(p, a, 0, v)?;
        result.event = event(operand(p, a, if value { 1 } else { 2 })?)?;
    } else {
        return Err(Error::UnsupportedAction);
    }
    Ok(result)
}
