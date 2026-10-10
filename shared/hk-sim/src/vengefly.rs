//! Source-derived Vengefly (Buzzer) `chaser` FSM: IdleBuzz roaming, alert,
//! Startle, ChaseObject and Decelerate. Evidence: .hkpsx/vengefly/CONTRACT.md.
//!
//! The caller owns physics (gravity-free dynamic body against terrain), sight
//! and alert-range queries, Recoil displacement, HealthManager hits, corpse and
//! Geo. `tick` is one nominal 60 Hz frame; the source 50 Hz FixedUpdate steps
//! are scheduled from an accumulator so the per-step constants stay authored.
//! Random.Range is replaced by a deterministic per-actor sequence.
use crate::ONE;

pub use crate::buzz::{IDLE_ACCELERATION_MAX, IDLE_SPEED_MAX, ROAMING_RANGE, WAIT_MAX, WAIT_MIN};
pub const CHASE_SPEED_MAX: i32 = 5 * ONE;
pub const CHASE_ACCELERATION: i32 = 2949; // .045 per fixed step
pub const DECELERATION: i32 = 7864; // .12 per fixed step
pub const STARTLE_TICKS: u16 = 20; // four frames at 12 fps
pub const STOP_TICKS: u16 = 60;
pub const ATTENTION_TICKS: u16 = 600;
pub const FACE_PAUSE_TICKS: u16 = 30;
/// Tk2dPlayFrame 3 at 12 fps, in 60 Hz animation ticks.
pub const CHASE_START_FRAME_TICKS: u32 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Startle,
    Chase,
    Stop,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Idle,
    TurnToIdle,
    Startle,
    Chase,
    TurnToFly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Velocity([i32; 2]),
    /// Play a clip from the given 60 Hz animation tick.
    Play(Clip, u32),
    /// Sprite facing, +1 right (source scale.x negative) or -1 left.
    Facing(i32),
    StartleSound,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// LineOfSightDetector.canSeeHero: alert range AND unobstructed raycast.
    pub can_see_hero: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vengefly {
    phase: Phase,
    velocity: [i32; 2],
    buzz: crate::buzz::IdleBuzz,
    timer: u16,
    face_pause: u16,
    facing: i32,
    fixed_accumulator: u8,
    rng: u32,
}
impl Vengefly {
    pub fn new(position: [i32; 2], seed: u32) -> Self {
        Self {
            phase: Phase::Idle,
            velocity: [0; 2],
            buzz: crate::buzz::IdleBuzz::new(position),
            timer: 0,
            face_pause: 0,
            facing: -1,
            fixed_accumulator: 0,
            rng: seed,
        }
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn velocity(self) -> [i32; 2] {
        self.velocity
    }
    pub fn facing(self) -> i32 {
        self.facing
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.velocity = [0; 2];
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    /// Random.Range(low, high) over Q16 values, deterministic.
    fn range(&mut self, low: i32, high: i32) -> i32 {
        debug_assert!(low <= high);
        let span = (high - low) as i64;
        low + ((self.random() as i64 * span) >> 24) as i32
    }
    /// IdleBuzz.DoBuzz for one fixed step (also the OnEnter call).
    fn idle_buzz(&mut self, position: [i32; 2]) {
        let mut v = self.velocity;
        let mut buzz = self.buzz;
        buzz.step(position, &mut v, &mut |lo, hi| self.range(lo, hi));
        self.buzz = buzz;
        self.velocity = v;
    }
    /// ChaseObject.DoBuzz: per-axis acceleration toward the hero.
    fn chase(&mut self, position: [i32; 2], hero: [i32; 2]) {
        let mut v = self.velocity;
        for axis in 0..2 {
            v[axis] += if hero[axis] > position[axis] {
                CHASE_ACCELERATION
            } else {
                -CHASE_ACCELERATION
            };
        }
        crate::buzz::clamp(&mut v, CHASE_SPEED_MAX);
        self.velocity = v;
    }
    fn decelerate(&mut self) {
        for axis in self.velocity.iter_mut() {
            *axis = axis.signum() * (axis.abs() - DECELERATION).max(0);
        }
    }
    /// FaceDirection.DoFace every frame: face the velocity sign with a pause.
    fn face(&mut self, turn: Clip, out: &mut Actions) {
        if self.face_pause > 0 {
            self.face_pause -= 1;
            return;
        }
        let want = if self.velocity[0] > 0 { 1 } else { -1 };
        if want != self.facing {
            self.facing = want;
            self.face_pause = FACE_PAUSE_TICKS;
            out.push(Action::Facing(want));
            out.push(Action::Play(turn, 0));
        }
    }
    fn startle(&mut self, senses: Senses, out: &mut Actions) {
        self.phase = Phase::Startle;
        self.timer = STARTLE_TICKS;
        self.velocity = [0; 2];
        // FaceObject: the sprite faces the hero.
        self.facing = if senses.hero[0] > senses.position[0] {
            1
        } else {
            -1
        };
        out.push(Action::StartleSound);
        out.push(Action::Velocity([0; 2]));
        out.push(Action::Play(Clip::Startle, 0));
        out.push(Action::Facing(self.facing));
    }
    fn chase_start(&mut self, senses: Senses, out: &mut Actions) {
        self.phase = Phase::Chase;
        self.timer = ATTENTION_TICKS;
        out.push(Action::Play(Clip::Chase, CHASE_START_FRAME_TICKS));
        // Chase Start's Wait 0 enters Chase - In Sight in the same frame.
        self.chase(senses.position, senses.hero);
        out.push(Action::Velocity(self.velocity));
    }
    /// HealthManager TOOK DAMAGE: only the Idle state listens.
    pub fn took_damage(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Idle {
            self.startle(senses, &mut out);
        }
        out
    }
    /// One nominal 60 Hz frame.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Dead {
            return out;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
        }
        match self.phase {
            Phase::Idle => {
                if senses.can_see_hero {
                    self.startle(senses, &mut out);
                    return out;
                }
                if fixed {
                    self.idle_buzz(senses.position);
                    out.push(Action::Velocity(self.velocity));
                }
                self.face(Clip::TurnToIdle, &mut out);
            }
            Phase::Startle => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.chase_start(senses, &mut out);
                }
            }
            Phase::Chase => {
                if senses.can_see_hero {
                    // In Sight <-> Out of Sight ping-pong: OnEnter DoBuzz per
                    // frame and the attention Wait restarts.
                    self.timer = ATTENTION_TICKS;
                    self.chase(senses.position, senses.hero);
                } else {
                    self.timer -= 1;
                }
                if fixed {
                    self.chase(senses.position, senses.hero);
                }
                out.push(Action::Velocity(self.velocity));
                self.face(Clip::TurnToFly, &mut out);
                if self.timer == 0 {
                    self.phase = Phase::Stop;
                    self.timer = STOP_TICKS;
                    out.push(Action::Play(Clip::Idle, CHASE_START_FRAME_TICKS));
                }
            }
            Phase::Stop => {
                if fixed {
                    self.decelerate();
                    out.push(Action::Velocity(self.velocity));
                }
                self.timer -= 1;
                if self.timer == 0 {
                    // IdleBuzz.OnEnter re-samples the roaming origin, then DoBuzz.
                    self.phase = Phase::Idle;
                    self.buzz.enter(senses.position);
                    self.idle_buzz(senses.position);
                    out.push(Action::Play(Clip::Idle, 0));
                    out.push(Action::Velocity(self.velocity));
                }
            }
            Phase::Dead => {}
        }
        out
    }
}
