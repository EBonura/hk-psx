//! Source-derived Baldur (Roller) `Roller` FSM: idle facing the hero, roll
//! toward it with per-frame acceleration for a random 2 to 3 s, bounce off
//! walls with a fixed jump, stop and rest. Evidence: .hkpsx/baldur/CONTRACT.md.
//!
//! The caller owns the gravity body (scale 0.8), sight and alert-box queries,
//! wall/ground detection after its solver step, Recoil displacement, hits,
//! corpse and Geo. Random.Range is a deterministic per-actor sequence.
use crate::ONE;

pub const ACCELERATION: i32 = 29491; // .45 units/s per frame
pub const MAX_SPEED: i32 = 11 * ONE;
/// `Spawn Roller v2`, the Elder Baldur's spat roller: the same FSM at Max Speed 14.
pub const SPAWNED_MAX_SPEED: i32 = 14 * ONE;
pub const ROLL_MIN: i32 = 2 * ONE;
pub const ROLL_MAX: i32 = 3 * ONE;
pub const START_TICKS: u16 = 24; // Start clip, four frames at 10 fps
pub const STOP_TICKS: u16 = 24; // Stop clip, four frames at 10 fps
pub const REST_TICKS: u16 = 30; // Stop Time .5 s
pub const BOUNCE_SPEED: i32 = 12 * ONE;
/// Collide Right launches at 115 degrees, Collide Left at 65: (cos, sin) * 12.
pub const BOUNCE_VELOCITY: [i32; 2] = [332360, 712749]; // 5.0714, 10.8757

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Start,
    Roll,
    InAir,
    Stop,
    Rest,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Idle,
    Start,
    Roll,
    Stop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Horizontal velocity to hold this frame (the body keeps its own vertical).
    VelocityX(i32),
    /// Full velocity, as SetVelocity2d / SetVelocityAsAngle.
    Velocity([i32; 2]),
    Play(Clip),
    Facing(i32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 4],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 4],
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub actor_x: i32,
    pub hero_x: i32,
    pub can_see_hero: bool,
    /// Blocked this frame while moving in the roll direction (CheckCollisionSide WALL).
    pub wall: bool,
    /// Bottom contact this frame (CheckCollisionSide GROUND).
    pub grounded: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baldur {
    phase: Phase,
    moving_right: bool,
    /// FaceObject in Idle: +1 when the hero is to the right.
    facing: i32,
    velocity_x: i32,
    /// Roll time in Q16 seconds; decremented per frame in Roll and In Air.
    roll_time: i32,
    timer: u16,
    rng: u32,
    max_speed: i32,
}
impl Baldur {
    pub fn new(seed: u32) -> Self {
        Self {
            phase: Phase::Idle,
            moving_right: false,
            facing: -1,
            velocity_x: 0,
            roll_time: 0,
            timer: 0,
            rng: seed,
            max_speed: MAX_SPEED,
        }
    }
    /// `Spawn Roller v2` as the Blocker's `Fire` leaves it: `Initiate` draws a
    /// roll time, `Moving Right?` takes the Blocker's facing, and it is `In Air`
    /// until it lands, when it rolls that way at up to 14 units/s.
    pub fn spawned(seed: u32, moving_right: bool) -> Self {
        let mut roller = Self {
            phase: Phase::InAir,
            moving_right,
            facing: if moving_right { 1 } else { -1 },
            velocity_x: 0,
            roll_time: 0,
            timer: 0,
            rng: seed,
            max_speed: SPAWNED_MAX_SPEED,
        };
        roller.roll_time = roller.range(ROLL_MIN, ROLL_MAX);
        roller
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn facing(self) -> i32 {
        self.facing
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32
    }
    fn face(&mut self, right: bool, out: &mut Actions) {
        let want = if right { 1 } else { -1 };
        if want != self.facing {
            self.facing = want;
            out.push(Action::Facing(want));
        }
    }
    fn begin_roll(&mut self, out: &mut Actions) {
        self.phase = Phase::Roll;
        self.velocity_x = 0;
        out.push(Action::Play(Clip::Roll));
        self.face(self.moving_right, out);
    }
    /// Recoil's global RECOIL HORIZONTAL while rolling: velocity x to zero,
    /// then Roll re-enters and accelerates again.
    pub fn horizontal_recoil(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Roll {
            self.velocity_x = 0;
            out.push(Action::VelocityX(0));
            out.push(Action::Play(Clip::Roll));
        }
        out
    }
    /// One 60 Hz frame with the frame's senses (wall/ground from the caller's
    /// step of the previous velocity).
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        match self.phase {
            Phase::Idle => {
                out.push(Action::VelocityX(0));
                self.face(senses.hero_x >= senses.actor_x, &mut out);
                if senses.can_see_hero {
                    // Facing Check: equal counts as RIGHT.
                    self.moving_right = senses.hero_x >= senses.actor_x;
                    self.face(self.moving_right, &mut out);
                    self.phase = Phase::Start;
                    self.timer = START_TICKS;
                    self.roll_time = self.range(ROLL_MIN, ROLL_MAX);
                    out.push(Action::Play(Clip::Start));
                }
            }
            Phase::Start => {
                out.push(Action::VelocityX(0));
                self.timer -= 1;
                if self.timer == 0 {
                    self.begin_roll(&mut out);
                }
            }
            Phase::Roll => {
                if senses.wall {
                    // Collide: flip, stop, launch away and up; In Air until ground.
                    self.moving_right = !self.moving_right;
                    self.phase = Phase::InAir;
                    self.velocity_x = 0;
                    let sign = if self.moving_right { 1 } else { -1 };
                    out.push(Action::Velocity([
                        sign * BOUNCE_VELOCITY[0],
                        BOUNCE_VELOCITY[1],
                    ]));
                    // Both Collide states set scale.x = 1 (source faces left).
                    self.face(false, &mut out);
                    self.roll_time -= ONE / 60;
                    return out;
                }
                self.velocity_x += if self.moving_right {
                    ACCELERATION
                } else {
                    -ACCELERATION
                };
                self.velocity_x = self.velocity_x.clamp(-self.max_speed, self.max_speed);
                out.push(Action::VelocityX(self.velocity_x));
                self.roll_time -= ONE / 60;
                if self.roll_time <= 0 {
                    self.phase = Phase::Stop;
                    self.timer = STOP_TICKS;
                    self.velocity_x = 0;
                    out.push(Action::Velocity([0; 2]));
                    out.push(Action::Play(Clip::Stop));
                }
            }
            Phase::InAir => {
                self.roll_time -= ONE / 60;
                if senses.grounded {
                    // Land -> Left or right?: roll the new way.
                    self.begin_roll(&mut out);
                }
            }
            Phase::Stop => {
                out.push(Action::VelocityX(0));
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Rest;
                    self.timer = REST_TICKS;
                    out.push(Action::Play(Clip::Idle));
                }
            }
            Phase::Rest => {
                out.push(Action::VelocityX(0));
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Idle;
                }
            }
            Phase::Dead => {}
        }
        out
    }
}

#[cfg(test)]
mod spawned_tests {
    use super::*;
    /// The Blocker's roller is in the air until it lands, then rolls the way
    /// it was told at up to 14 units/s rather than the Mound Baldur's 11.
    #[test]
    fn a_spawned_roller_lands_then_rolls_to_fourteen() {
        let mut roller = Baldur::spawned(7, true);
        assert_eq!(roller.phase(), Phase::InAir);
        let air = Senses {
            actor_x: 0,
            hero_x: 0,
            can_see_hero: false,
            wall: false,
            grounded: false,
        };
        assert_eq!(roller.tick(air).iter().count(), 0);
        let ground = Senses {
            grounded: true,
            ..air
        };
        roller.tick(ground);
        assert_eq!(roller.phase(), Phase::Roll);
        let mut last = 0;
        for _ in 0..40 {
            for action in roller.tick(ground).iter() {
                if let Action::VelocityX(v) = action {
                    last = v;
                }
            }
        }
        assert_eq!(last, SPAWNED_MAX_SPEED);
    }
}
