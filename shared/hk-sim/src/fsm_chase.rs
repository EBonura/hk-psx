//! Vengefly action closure under construction. Unknown actions stay refused;
//! these scalar kernels do not imply admission of either complete graph.
use crate::fsm::{Error, Kernel, Phase, ResultAction};
/// One shared decode pass per action. Kernels consume typed cells instead of
/// repeating serialized operand/constant/type lookups for every parameter.
struct Arguments {
    values: [i32; 8],
    indices: [u8; 8],
    kinds: [u8; 8],
    types: [u8; 8],
    count: u8,
}
impl Arguments {
    #[inline(never)]
    fn decode(p: &Program<'_>, a: Action, variables: &[i32]) -> Result<Self, Error> {
        if a.operand_count > 8 {
            return Err(Error::Parameter);
        }
        let mut result = Self {
            values: [0; 8],
            indices: [0; 8],
            kinds: [4; 8],
            types: [0; 8],
            count: a.operand_count,
        };
        for i in 0..a.operand_count {
            let o = p
                .operand_for_format(a.operands + i as u16)
                .ok_or(Error::Parameter)?;
            let at = i as usize;
            result.kinds[at] = o.kind;
            result.values[at] = match o.kind {
                0 => p.constant_for_format(o.index).ok_or(Error::Parameter)?,
                1 => {
                    result.indices[at] = u8::try_from(o.index).map_err(|_| Error::Parameter)?;
                    result.types[at] = p.variable_kind(o.index).ok_or(Error::Parameter)?;
                    *variables.get(o.index as usize).ok_or(Error::Parameter)?
                }
                2 | 3 => o.index as i32,
                4 => 0,
                _ => return Err(Error::Parameter),
            };
        }
        Ok(result)
    }
    #[inline(always)]
    fn operand(&self, index: u8) -> Result<hk_format::fsm::Operand, Error> {
        if index >= self.count {
            return Err(Error::Parameter);
        }
        Ok(hk_format::fsm::Operand {
            kind: self.kinds[index as usize],
            index: index as u16,
        })
    }
    #[inline(never)]
    fn read(&self, o: hk_format::fsm::Operand) -> Result<i32, Error> {
        if !matches!(o.kind, 0 | 1 | 3) {
            return Err(Error::Parameter);
        }
        Ok(self.values[o.index as usize])
    }
    #[inline(never)]
    fn write(&self, o: hk_format::fsm::Operand, v: &mut [i32], value: i32) -> Result<(), Error> {
        if o.kind != 1 {
            return Err(Error::Parameter);
        }
        let at = o.index as usize;
        if self.types[at] == 1 && !(0..=1).contains(&value)
            || self.types[at] == 3 && !(0..=3).contains(&value)
        {
            return Err(Error::Parameter);
        }
        *v.get_mut(self.indices[at] as usize)
            .ok_or(Error::Parameter)? = value;
        Ok(())
    }
    #[inline(never)]
    fn event(&self, o: hk_format::fsm::Operand) -> Result<u16, Error> {
        if o.kind != 2 {
            return Err(Error::Parameter);
        }
        Ok(self.values[o.index as usize] as u16)
    }
    #[inline(never)]
    fn boolean(&self, o: hk_format::fsm::Operand) -> Result<bool, Error> {
        match self.read(o)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Parameter),
        }
    }
}
use hk_format::fsm::{opcode::*, Action, Program, NO_EVENT, UPDATE};

pub trait Environment {
    fn audio_one_shot(
        &mut self,
        _role: i32,
        _weights: [i32; 3],
        _pitch: [i32; 2],
        _volume: i32,
        _rng: &mut u32,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn send_direction_message(&mut self) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn set_rotation_z(&mut self, _role: i32, _angle: i32, _space: i32) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn face_object(
        &mut self,
        _roles: [i32; 2],
        _sprite_right: bool,
        _clip: Option<i32>,
        _reset_frame: bool,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn idle_buzz(
        &mut self,
        _role: i32,
        _parameters: [i32; 5],
        _enter: bool,
        _rng: &mut u32,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn chase(
        &mut self,
        _role: i32,
        _target: i32,
        _speed: i32,
        _acceleration: i32,
        _enter: bool,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn face_direction(
        &mut self,
        _role: i32,
        _sprite_right: bool,
        _clip: Option<i32>,
        _pause: Option<i32>,
    ) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn play_animation(&mut self, _role: i32, _clip: i32) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn play_frame(&mut self, _role: i32, _frame: i32) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }
    fn animation_complete(&self, _role: i32) -> Result<bool, Error> {
        Err(Error::UnsupportedAction)
    }
    fn set_audio_pitch(&mut self, _role: i32, _pitch: i32) -> Result<(), Error> {
        Err(Error::UnsupportedAction)
    }

    fn elapsed_ticks(&self, _real_time: bool) -> Result<u16, Error> {
        Err(Error::UnsupportedAction)
    }
    fn can_see_hero(&self) -> Result<bool, Error> {
        Err(Error::UnsupportedAction)
    }
    fn in_alert_range(&self, _role: i32) -> Result<bool, Error> {
        Err(Error::UnsupportedAction)
    }
    fn find_alert_range(&self, _role: i32) -> Result<i32, Error> {
        Err(Error::UnsupportedAction)
    }

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
    #[inline(never)]
    fn execute(
        &mut self,
        p: &Program<'_>,
        a: Action,
        phase: Phase,
        v: &mut [i32],
        rng: &mut u32,
    ) -> Result<ResultAction, Error> {
        if matches!(phase, Phase::CollisionEnter | Phase::CollisionStay) {
            return Ok(ResultAction::running());
        }
        let fixed = matches!(
            a.opcode,
            SET_VELOCITY_2D | ADD_FORCE_2D | DECELERATE | IDLE_BUZZ | CHASE_OBJECT
        );
        if phase != Phase::Enter
            && ((fixed && phase != Phase::FixedUpdate) || (!fixed && phase != Phase::Update))
        {
            return Ok(ResultAction::running());
        }
        let args = Arguments::decode(p, a, v)?;
        let mut result = ResultAction {
            finished: a.flags & UPDATE == 0,
            event: NO_EVENT,
        };
        match a.opcode {
            AUDIO_ONE_SHOT => {
                if a.operand_count != 7 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                let weights = [
                    args.read(args.operand(1)?)?,
                    args.read(args.operand(2)?)?,
                    args.read(args.operand(3)?)?,
                ];
                let pitch = [args.read(args.operand(4)?)?, args.read(args.operand(5)?)?];
                let volume = args.read(args.operand(6)?)?;
                if weights.iter().any(|n| *n < 0)
                    || weights.iter().all(|n| *n == 0)
                    || pitch[0] <= 0
                    || pitch[1] < pitch[0]
                    || volume < 0
                    || volume > 65536
                {
                    return Err(Error::Parameter);
                }
                self.0.audio_one_shot(role, weights, pitch, volume, rng)?;
            }

            SEND_EVENT_BY_NAME => {
                if a.operand_count != 1 || args.read(args.operand(0)?)? != 1 {
                    return Err(Error::Parameter);
                }
                self.0.send_direction_message()?;
            }

            SET_ROTATION => {
                if a.operand_count != 3 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                let angle = args.read(args.operand(1)?)?;
                let space = args.read(args.operand(2)?)?;
                if space != 0 {
                    return Err(Error::Parameter);
                }
                self.0.set_rotation_z(role, angle, space)?;
            }
            FACE_OBJECT => {
                if a.operand_count != 6 {
                    return Err(Error::Parameter);
                }
                let roles = [args.read(args.operand(0)?)?, args.read(args.operand(1)?)?];
                let sprite_right = args.boolean(args.operand(2)?)?;
                let clip = if args.boolean(args.operand(3)?)? {
                    Some(args.read(args.operand(4)?)?)
                } else {
                    None
                };
                let reset = args.boolean(args.operand(5)?)?;
                self.0.face_object(roles, sprite_right, clip, reset)?;
            }

            IDLE_BUZZ => {
                if a.operand_count != 6 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                let mut parameters = [0; 5];
                for (i, value) in parameters.iter_mut().enumerate() {
                    *value = args.read(args.operand(i as u8 + 1)?)?;
                    if *value < 0 {
                        return Err(Error::Parameter);
                    }
                }
                if parameters[0] > parameters[1] {
                    return Err(Error::Parameter);
                }
                self.0
                    .idle_buzz(role, parameters, phase == Phase::Enter, rng)?;
            }
            CHASE_OBJECT => {
                if a.operand_count != 4 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                let target = args.read(args.operand(1)?)?;
                let speed = args.read(args.operand(2)?)?;
                let acceleration = args.read(args.operand(3)?)?;
                if speed < 0 || acceleration < 0 {
                    return Err(Error::Parameter);
                }
                self.0
                    .chase(role, target, speed, acceleration, phase == Phase::Enter)?;
            }

            FACE_DIRECTION => {
                if a.operand_count != 6 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                let sprite_right = args.boolean(args.operand(1)?)?;
                let clip = if args.boolean(args.operand(2)?)? {
                    Some(args.read(args.operand(3)?)?)
                } else {
                    None
                };
                let pause = if args.boolean(args.operand(4)?)? {
                    let time = args.read(args.operand(5)?)?;
                    if time < 0 {
                        return Err(Error::Parameter);
                    }
                    Some(time)
                } else {
                    None
                };
                self.0.face_direction(role, sprite_right, clip, pause)?;
            }

            PLAY_ANIMATION | PLAY_FRAME | SET_AUDIO_PITCH => {
                if a.operand_count != 2 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                let value = args.read(args.operand(1)?)?;
                if a.opcode == PLAY_ANIMATION {
                    self.0.play_animation(role, value)?;
                } else if a.opcode == PLAY_FRAME {
                    if value < 0 {
                        return Err(Error::Parameter);
                    }
                    self.0.play_frame(role, value)?;
                } else {
                    if value <= 0 {
                        return Err(Error::Parameter);
                    }
                    self.0.set_audio_pitch(role, value)?;
                }
            }
            WATCH_ANIMATION => {
                if a.operand_count != 2 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                result.finished = self.0.animation_complete(role)?;
                if result.finished {
                    result.event = args.event(args.operand(1)?)?;
                }
            }

            GET_OWNER => {
                if a.operand_count != 1 {
                    return Err(Error::Parameter);
                }
                args.write(args.operand(0)?, v, 1)?;
            }
            BOOL_TEST => {
                if a.operand_count != 3 {
                    return Err(Error::Parameter);
                }
                let selected = if args.boolean(args.operand(0)?)? {
                    1
                } else {
                    2
                };
                result.event = args.event(args.operand(selected)?)?;
            }
            WAIT => {
                if a.operand_count != 4 {
                    return Err(Error::Parameter);
                }
                let timer = args.operand(1)?;
                let remaining = if phase == Phase::Enter {
                    args.read(args.operand(0)?)?
                        .max(0)
                        .checked_mul(60)
                        .ok_or(Error::Parameter)?
                } else {
                    let real = args.boolean(args.operand(3)?)?;
                    let elapsed = (self.0.elapsed_ticks(real)? as i32).saturating_mul(65536);
                    args.read(timer)?.saturating_sub(elapsed).max(0)
                };
                args.write(timer, v, remaining)?;
                result.finished = remaining == 0;
                if result.finished {
                    result.event = args.event(args.operand(2)?)?;
                }
            }

            CHECK_CAN_SEE_HERO => {
                if a.operand_count != 1 {
                    return Err(Error::Parameter);
                }
                args.write(args.operand(0)?, v, self.0.can_see_hero()? as i32)?;
            }
            CHECK_ALERT_RANGE => {
                if a.operand_count != 2 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                args.write(args.operand(1)?, v, self.0.in_alert_range(role)? as i32)?;
            }
            FIND_ALERT_RANGE => {
                if a.operand_count != 2 {
                    return Err(Error::Parameter);
                }
                let role = args.read(args.operand(0)?)?;
                args.write(args.operand(1)?, v, self.0.find_alert_range(role)?)?;
            }

            GET_VELOCITY_2D => {
                if a.operand_count != 4 {
                    return Err(Error::Parameter);
                }
                let space = args.read(args.operand(3)?)?;
                if space != 0 && space != 1 {
                    return Err(Error::Parameter);
                }
                let velocity = self.0.velocity(args.read(args.operand(0)?)?, space)?;
                for i in 0..2 {
                    let output = args.operand(i + 1)?;
                    if output.kind != 4 {
                        args.write(output, v, velocity[i as usize])?;
                    }
                }
            }
            SET_VELOCITY_2D => {
                if a.operand_count != 3 {
                    return Err(Error::Parameter);
                }
                let mut velocity = [None; 2];
                for i in 0..2 {
                    let value = args.operand(i + 1)?;
                    if value.kind != 4 {
                        velocity[i as usize] = Some(args.read(value)?);
                    }
                }
                self.0
                    .set_velocity(args.read(args.operand(0)?)?, velocity)?;
            }
            ADD_FORCE_2D => {
                if a.operand_count != 6 {
                    return Err(Error::Parameter);
                }
                let force = [args.read(args.operand(1)?)?, args.read(args.operand(2)?)?];
                let limits = [
                    args.read(args.operand(3)?)?,
                    args.read(args.operand(4)?)?,
                    args.read(args.operand(5)?)?,
                ];
                if limits.iter().any(|n| *n < 0) {
                    return Err(Error::Parameter);
                }
                self.0
                    .add_force(args.read(args.operand(0)?)?, force, limits)?;
            }
            DECELERATE => {
                if a.operand_count != 2 {
                    return Err(Error::Parameter);
                }
                let amount = args.read(args.operand(1)?)?;
                if amount < 0 {
                    return Err(Error::Parameter);
                }
                self.0.decelerate(args.read(args.operand(0)?)?, amount)?;
            }
            FLOAT_CLAMP => {
                if a.operand_count != 3 {
                    return Err(Error::Parameter);
                }
                let target = args.operand(0)?;
                let low = args.read(args.operand(1)?)?;
                let high = args.read(args.operand(2)?)?;
                if low > high {
                    return Err(Error::Parameter);
                }
                let value = args.read(target)?.clamp(low, high);
                args.write(target, v, value)?;
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
                    all &= args.boolean(args.operand(i)?)?;
                }
                let output = args.operand(a.operand_count - 2)?;
                // The selected graphs have no output. A named output would
                // require the original event-before-store ordering contract.
                if output.kind != 4 {
                    return Err(Error::Parameter);
                }
                if all {
                    result.event = args.event(args.operand(a.operand_count - 1)?)?;
                }
            }
            NEXT_FRAME_EVENT => {
                if a.operand_count != 1 {
                    return Err(Error::Parameter);
                }
                result.finished = phase == Phase::Update;
                if result.finished {
                    result.event = args.event(args.operand(0)?)?;
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
