//! Source-derived level37 Climber; independent of Crawler and Zombie Runner.
//!
//! Evidence: .hkpsx/climber70/CONTRACT.md, source.il, animation-wait.il and
//! clip-source.il, Windows Assembly-CSharp SHA256
//! e9048ef6a633970f735e01ec166d3959f610eaea7a88d827d48d67b1e5fb87bd.
//!
//! Caller owns physics, transformed point-ray hits, rotated body/render geometry,
//! animation/audio, accepted HealthManager hits, corpse and once-only Geo payout.
//! No invented audio events: source live loop is PlayOnAwake and Climber itself
//! never changes it during Walk/Turn/Stun. `freeze` is Recoil.OnHandleFreeze,
//! not a raw nail contact (ignored/evaded hits must not invoke it).
//!
//! Callback boundaries are explicit. After `attach` or successful `end_of_frame`,
//! source StartCoroutine(Walk) immediately runs its first iteration: caller may
//! invoke `walk_frame` immediately, before its next scheduled physics step.
//! `turn_frame` advances one nominal 60 Hz yielded frame; its entry sample is
//! elapsed zero, and completion does NOT write the translated target position.
//! After turn completion the parent Walk coroutine yields another frame before
//! sensing again; caller controls that resumption. `stun_tick` advances a
//! nominal 1/60 s timer, then requires a matching explicit end-of-frame callback.
//! Original runner-crossroads70 trace confirms attachment, clockwise outside
//! turns, 2-unit velocity and scaled-time freeze/ramp. Use `turn_advance` and
//! `stun_advance` for scaled time; nominal helpers assume an unscaled tick.
//! Inside turns/stun were not observed. Unity callback ordering and float
//! endpoint rounding remain gaps; tests establish this quantized model, not
//! original-game trajectory parity.
use crate::ONE;

pub const SPEED: i32 = 2 * ONE;
pub const CONSTRAIN: i32 = 6554; // round(0.1f * ONE)
pub const MIN_TURN_DISTANCE: i32 = ONE / 4;
pub const TURN_TICKS: u8 = 15;
pub const STUN_TICKS: u8 = 35; // seven frames / 12 fps, not Recoil's .25 s
pub const BODY_OFFSET: [i32; 2] = [1024, 31232];
pub const BODY_HALF: [i32; 2] = [35840, 30208];
pub const WALL_REACH: i32 = BODY_HALF[0] + CONSTRAIN;
pub const WORLD_LIMIT: i32 = 512 * ONE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    CoordinateLimit,
    ScaleSign,
}
fn valid_point(p: [i32; 2]) -> Result<(), Error> {
    if p.iter()
        .any(|&x| !(-WORLD_LIMIT..=WORLD_LIMIT).contains(&x))
    {
        Err(Error::CoordinateLimit)
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    East,
    South,
    West,
    North,
}
impl Direction {
    pub const fn velocity(self) -> [i32; 2] {
        match self {
            Self::East => [SPEED, 0],
            Self::South => [0, -SPEED],
            Self::West => [-SPEED, 0],
            Self::North => [0, SPEED],
        }
    }
    fn turn(self, clockwise: bool) -> Self {
        match (self as u8 + if clockwise { 1 } else { 3 }) % 4 {
            0 => Self::East,
            1 => Self::South,
            2 => Self::West,
            _ => Self::North,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    AwaitAttachment,
    Walking,
    Turning,
    Stunned,
    StunEndOfFrame,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Walk,
    Stun,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Position([i32; 2]),
    Velocity([i32; 2]),
    Rotation(i32),
    Play(Clip),
    /// Expired clip-duration timer now needs WaitForEndOfFrame, even though
    /// the source Stun clip is still playing its LoopSection.
    AwaitEndOfFrame(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 5],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 5],
            count: 0,
        }
    }
    fn push(&mut self, action: Action) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize]
            .iter()
            .map(|a| a.unwrap())
    }
}
/// Caller implements TransformPoint(local_origin) using rotation AND scale,
/// TransformDirection(local_direction) using rotation (not scale), then casts
/// the original point ray on terrain layer mask 256. This is not a body Sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ray {
    pub position: [i32; 2],
    pub rotation_degrees_q16: i32,
    pub scale_x_sign: i8,
    pub local_origin: [i32; 2],
    pub local_direction: [i32; 2],
    pub length: i32,
    pub layer_mask: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Climber {
    phase: Phase,
    direction: Direction,
    clockwise: bool,
    scale_x_sign: i8,
    rotation: i32,
    previous_pos: [i32; 2],
    previous_turn_pos: [i32; 2],
    turn_origin: [i32; 2],
    turn_delta: [i32; 2],
    turn_rotation: i32,
    turn_clockwise: bool,
    tween: bool,
    elapsed: u32,
    stun_token: u32,
}
impl Climber {
    /// Both actual instances have positive scale, start_right=true, rotation=0.
    /// General handedness/rotation bands are source-derived. Only unit X scale
    /// magnitude is supported here; other scaled collider variants need audit.
    pub fn new(
        position: [i32; 2],
        rotation_degrees_q16: i32,
        scale_x_sign: i8,
        start_right: bool,
    ) -> Result<Self, Error> {
        valid_point(position)?;
        if scale_x_sign != 1 && scale_x_sign != -1 {
            return Err(Error::ScaleSign);
        }
        let clockwise = (scale_x_sign > 0) == start_right;
        let rotation = rotation_degrees_q16.rem_euclid(360 * ONE);
        // Source inclusive branch order: 135 belongs to first band, 225 to
        // second, 315 to third. Don't replace with rounded quarter turns.
        let mut direction = if (45 * ONE..=135 * ONE).contains(&rotation) {
            Direction::North
        } else if (135 * ONE..=225 * ONE).contains(&rotation) {
            Direction::West
        } else if (225 * ONE..=315 * ONE).contains(&rotation) {
            Direction::South
        } else {
            Direction::East
        };
        if !clockwise {
            direction = direction.turn(true).turn(true);
        }
        Ok(Self {
            phase: Phase::AwaitAttachment,
            direction,
            clockwise,
            scale_x_sign,
            rotation,
            previous_pos: position,
            previous_turn_pos: [0; 2],
            turn_origin: position,
            turn_delta: [0; 2],
            turn_rotation: rotation,
            turn_clockwise: clockwise,
            tween: false,
            elapsed: 0,
            stun_token: 0,
        })
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn direction(&self) -> Direction {
        self.direction
    }
    pub fn clockwise(&self) -> bool {
        self.clockwise
    }
    pub fn rotation(&self) -> i32 {
        self.rotation
    }
    pub fn previous_position(&self) -> [i32; 2] {
        self.previous_pos
    }
    pub fn previous_turn_position(&self) -> [i32; 2] {
        self.previous_turn_pos
    }
    fn ray(&self, position: [i32; 2], local_direction: [i32; 2], length: i32) -> Ray {
        Ray {
            position,
            rotation_degrees_q16: self.rotation,
            scale_x_sign: self.scale_x_sign,
            local_origin: BODY_OFFSET,
            local_direction,
            length,
            layer_mask: 256,
        }
    }
    pub fn attachment_ray(&self) -> Option<Ray> {
        (self.phase == Phase::AwaitAttachment)
            .then(|| self.ray(self.previous_pos, [0, -ONE], 2 * ONE))
    }
    fn walk(&mut self, out: &mut Actions) {
        self.phase = Phase::Walking;
        out.push(Action::Play(Clip::Walk));
        out.push(Action::Velocity(self.direction.velocity()));
    }
    /// StickToGround assigns the complete transform position to hit.point,
    /// including X. No body-height adjustment. No hit preserves authored pose.
    pub fn attach(&mut self, hit: Option<[i32; 2]>) -> Result<Actions, Error> {
        let mut out = Actions::new();
        if self.phase != Phase::AwaitAttachment {
            return Ok(out);
        }
        if let Some(point) = hit {
            valid_point(point)?;
            self.previous_pos = point;
            out.push(Action::Position(point));
        }
        self.walk(&mut out);
        Ok(out)
    }
    pub fn tween_delta(&self) -> [i32; 2] {
        let [hx, hy] = BODY_HALF;
        let handed = if self.clockwise { 1 } else { -1 };
        match self.direction {
            Direction::East => [hx + CONSTRAIN, handed * hy],
            Direction::South => [handed * hx, -hy - CONSTRAIN],
            Direction::West => [-hx - CONSTRAIN, -handed * hy],
            Direction::North => [-handed * hx, hy + CONSTRAIN],
        }
    }
    fn begin_turn(
        &mut self,
        position: [i32; 2],
        clockwise: bool,
        tween: bool,
        out: &mut Actions,
    ) -> Result<(), Error> {
        let delta = self.tween_delta();
        if tween {
            valid_point([position[0] + delta[0], position[1] + delta[1]])?;
        }
        self.phase = Phase::Turning;
        self.turn_origin = position;
        self.turn_delta = delta;
        self.turn_rotation = self.rotation;
        self.turn_clockwise = clockwise;
        self.tween = tween;
        self.elapsed = 0;
        out.push(Action::Velocity([0; 2]));
        out.push(Action::Rotation(self.rotation));
        if tween {
            out.push(Action::Position(position));
        } // source elapsed=0 sample
        Ok(())
    }
    /// One yielded Walk iteration, or its caller-dispatched immediate entry.
    /// Ray callback receives corrected position; ground is queried before wall.
    /// Source Walk writes velocity only on entry, not in each iteration.
    pub fn walk_frame(
        &mut self,
        position: [i32; 2],
        mut cast: impl FnMut(Ray) -> bool,
    ) -> Result<Actions, Error> {
        let mut out = Actions::new();
        if self.phase != Phase::Walking {
            return Ok(out);
        }
        valid_point(position)?;
        let mut corrected = position;
        for axis in 0..2 {
            if (position[axis] - self.previous_pos[axis]).abs() > CONSTRAIN {
                corrected[axis] = self.previous_pos[axis];
            }
        }
        if corrected != position {
            out.push(Action::Position(corrected));
        } else {
            self.previous_pos = position;
        }
        let dx = corrected[0] as i64 - self.previous_turn_pos[0] as i64;
        let dy = corrected[1] as i64 - self.previous_turn_pos[1] as i64;
        if dx * dx + dy * dy < MIN_TURN_DISTANCE as i64 * MIN_TURN_DISTANCE as i64 {
            return Ok(out);
        }
        if !cast(self.ray(corrected, [0, -ONE], ONE)) {
            self.begin_turn(corrected, self.clockwise, false, &mut out)?;
        } else if cast(self.ray(
            corrected,
            [if self.clockwise { ONE } else { -ONE }, 0],
            WALL_REACH,
        )) {
            self.begin_turn(corrected, !self.clockwise, true, &mut out)?;
        }
        Ok(out)
    }
    /// Resume Turn after one nominal unscaled 60 Hz frame. Use `turn_advance`
    /// for source freeze/slow-motion. Caller supplies actual transform
    /// position so collision/external motion is retained at completion. The
    /// current sample overwrites position only on an inside-wall tween.
    pub fn turn_frame(&mut self, actual_position: [i32; 2]) -> Result<Actions, Error> {
        self.turn_advance(actual_position, ONE as u32)
    }
    /// Resume with scaled elapsed time in Q16 ticks (ONE = 1/60 second).
    /// Zero preserves frozen time; fractional ticks represent the observed
    /// source kill-freeze ramp. Arbitrary advances saturate at the finite wait
    /// duration, so all counters/products stay bounded without looping.
    pub fn turn_advance(
        &mut self,
        actual_position: [i32; 2],
        scaled_ticks_q16: u32,
    ) -> Result<Actions, Error> {
        let mut out = Actions::new();
        if self.phase != Phase::Turning {
            return Ok(out);
        }
        valid_point(actual_position)?;
        let duration = TURN_TICKS as u32 * ONE as u32;
        self.elapsed = self.elapsed.saturating_add(scaled_ticks_q16).min(duration);
        let angle = if self.turn_clockwise {
            -90 * ONE
        } else {
            90 * ONE
        };
        if self.elapsed < duration {
            self.rotation = (self.turn_rotation
                + psx_math::int32::mul_div_i32(angle, self.elapsed as i32, duration as i32))
            .rem_euclid(360 * ONE);
            out.push(Action::Rotation(self.rotation));
            if self.tween {
                out.push(Action::Position(core::array::from_fn(|i| {
                    self.turn_origin[i]
                        + psx_math::int32::mul_div_i32(
                            self.turn_delta[i],
                            self.elapsed as i32,
                            duration as i32,
                        )
                })));
            }
        } else {
            self.rotation = (self.turn_rotation + angle).rem_euclid(360 * ONE);
            out.push(Action::Rotation(self.rotation));
            self.direction = self.direction.turn(self.turn_clockwise);
            out.push(Action::Velocity(self.direction.velocity()));
            self.previous_pos = actual_position;
            self.previous_turn_pos = actual_position;
            self.phase = Phase::Walking;
            // No Play(Walk): original Walk animation continued throughout turn.
        }
        Ok(out)
    }
    /// Accepted Recoil.OnHandleFreeze event. Source ignores stun while a turn
    /// coroutine is active; damage/death acceptance remains external.
    pub fn freeze(&mut self) -> Actions {
        let mut out = Actions::new();
        if matches!(
            self.phase,
            Phase::AwaitAttachment | Phase::Turning | Phase::Dead
        ) {
            return out;
        }
        self.stun_token = self.stun_token.wrapping_add(1);
        self.elapsed = 0;
        self.phase = Phase::Stunned;
        out.push(Action::Velocity([0; 2]));
        out.push(Action::Play(Clip::Stun));
        out
    }
    pub fn stun_tick(&mut self) -> Actions {
        self.stun_advance(ONE as u32)
    }
    /// Advance the clip-duration WaitForSeconds using scaled Q16 ticks,
    /// independently of animation wrap and end-of-frame callback ordering.
    pub fn stun_advance(&mut self, scaled_ticks_q16: u32) -> Actions {
        let mut out = Actions::new();
        if self.phase != Phase::Stunned {
            return out;
        }
        let duration = STUN_TICKS as u32 * ONE as u32;
        self.elapsed = self.elapsed.saturating_add(scaled_ticks_q16).min(duration);
        if self.elapsed == duration {
            self.phase = Phase::StunEndOfFrame;
            out.push(Action::AwaitEndOfFrame(self.stun_token));
        }
        out
    }
    /// Matching WaitForEndOfFrame callback; stale callbacks cannot terminate a
    /// restarted stun. Does not reset previousPos (source DoStun doesn't).
    pub fn end_of_frame(&mut self, token: u32) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::StunEndOfFrame && self.stun_token == token {
            self.walk(&mut out);
        }
        out
    }
    /// Irreversible controller termination; owner handles body destruction,
    /// corpse, source audio and payout exactly once through HealthManager.
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
    }
}
