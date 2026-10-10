//! Source-derived Aspid Hunter (Spitter) `spitter` FSM: IdleBuzz roaming,
//! alert, DistanceFly at 7 units, sight check, fly back, Fire Long anticipation
//! and one FireAtTarget shot at 15 units/s. Evidence: .hkpsx/aspid/CONTRACT.md.
//!
//! The caller owns physics (gravity-free dynamic body), the alert circle and
//! terrain raycasts, the projectile pool, Recoil, hits, corpse and Geo. The
//! source 50 Hz fixed steps run from an accumulator inside the 60 Hz tick.
//! Random.Range is a deterministic per-actor sequence.
use crate::buzz::{distance_fly, IdleBuzz};
use crate::ONE;

pub const SHOT_SPEED: i32 = 15 * ONE;
pub const FIRE_TRIGGER_TICKS: u16 = 45; // Fire Long frame 9 of 12 at 12 fps
pub const FIRE_LONG_TICKS: u16 = 60; // twelve frames at 12 fps
pub const FLY_BACK_TICKS: u16 = 30;
pub const UNALERT_TICKS: u16 = 480; // Range Out Timer > 8 s
pub const RAYCAST_DISTANCE: i32 = 14 * ONE;
pub const FACE_PAUSE_TICKS: u16 = 30;
pub const DISTANCE_FLY: (i32, i32, i32) = (7 * ONE, 4 * ONE, 6554); // distance, speedMax, .1 per step
pub const FLY_BACK: (i32, i32, i32) = (540672, 4 * ONE, 6554); // 8.25
pub const ANTICIPATE: (i32, i32, i32) = (9 * ONE, 2 * ONE, 6554);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    DistanceFly,
    /// Raycast state: one frame, resolved by the caller's sight answer.
    Raycast,
    FlyBack,
    FireAnticipate,
    /// Fire Long keeps playing after the shot until it completes.
    FireDribble,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Fly,
    TurnToFly,
    FireLong,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Velocity([i32; 2]),
    Play(Clip, u32),
    Facing(i32),
    /// Spawn the shot at the actor with this Q16 velocity.
    Fire([i32; 2]),
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
    /// Alert Range New (7.804 circle) AND unobstructed sight.
    pub can_see_hero: bool,
    /// Unalert Range child: hero within 12.1 units AND unobstructed sight.
    pub in_unalert_range: bool,
    /// Terrain raycast from the actor to the hero is clear (Raycast state).
    pub sight_clear: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aspid {
    phase: Phase,
    velocity: [i32; 2],
    buzz: IdleBuzz,
    timer: u16,
    range_out: u16,
    face_pause: u16,
    facing: i32,
    fixed_accumulator: u8,
    rng: u32,
}
/// Floor square root using the shared SDK restoring algorithm.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        0
    } else {
        psx_math::int32::isqrt_u64(n as u64) as i64
    }
}
impl Aspid {
    pub fn new(position: [i32; 2], seed: u32) -> Self {
        Self {
            phase: Phase::Idle,
            velocity: [0; 2],
            buzz: IdleBuzz::new(position),
            timer: 0,
            range_out: 0,
            face_pause: 0,
            facing: -1,
            fixed_accumulator: 0,
            rng: seed,
        }
    }
    /// FSM variable `startAlert`: Idle's BoolTest sends ALERT on its first frame.
    pub fn new_alert(position: [i32; 2], seed: u32) -> Self {
        let mut aspid = Self::new(position, seed);
        let mut out = Actions::new();
        aspid.begin_distance_fly(&mut out);
        aspid
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
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + ((self.random() as i64 * (high - low) as i64) >> 24) as i32
    }
    /// FaceDirection (Idle) follows the x velocity; FaceObject (chasing) the hero.
    fn face(&mut self, want: i32, turn: bool, out: &mut Actions) {
        if self.face_pause > 0 {
            self.face_pause -= 1;
            return;
        }
        if want != self.facing {
            self.facing = want;
            self.face_pause = FACE_PAUSE_TICKS;
            out.push(Action::Facing(want));
            if turn {
                out.push(Action::Play(Clip::TurnToFly, 0));
            }
        }
    }
    fn face_hero(&mut self, senses: Senses, turn: bool, out: &mut Actions) {
        let want = if senses.hero[0] > senses.position[0] {
            1
        } else {
            -1
        };
        self.face(want, turn, out);
    }
    fn begin_distance_fly(&mut self, out: &mut Actions) {
        self.phase = Phase::DistanceFly;
        // WaitRandom 1.5 to 2.25 s.
        self.timer = 90 + ((self.random() % 46) as u16);
        out.push(Action::Play(Clip::Fly, 10)); // Tk2dPlayFrame 2 at 12 fps
    }
    /// Range Out Timer: counts while the Unalert Range is not satisfied, resets otherwise.
    fn unalert(&mut self, senses: Senses) -> bool {
        if senses.in_unalert_range {
            self.range_out = 0;
            false
        } else {
            self.range_out = self.range_out.saturating_add(1);
            self.range_out > UNALERT_TICKS
        }
    }
    fn fly(&mut self, senses: Senses, fixed: bool, params: (i32, i32, i32), out: &mut Actions) {
        if fixed {
            let mut v = self.velocity;
            distance_fly(
                senses.position,
                senses.hero,
                params.0,
                params.1,
                params.2,
                &mut v,
            );
            self.velocity = v;
            out.push(Action::Velocity(self.velocity));
        }
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
                    // Alert: pitch 1.2, then Distance Fly.
                    self.range_out = 0;
                    self.begin_distance_fly(&mut out);
                    return out;
                }
                if fixed {
                    let mut v = self.velocity;
                    let mut buzz = self.buzz;
                    buzz.step(senses.position, &mut v, &mut |lo, hi| self.range(lo, hi));
                    self.buzz = buzz;
                    self.velocity = v;
                    out.push(Action::Velocity(self.velocity));
                }
                let want = if self.velocity[0] > 0 { 1 } else { -1 };
                self.face(want, true, &mut out);
            }
            Phase::DistanceFly => {
                self.fly(senses, fixed, DISTANCE_FLY, &mut out);
                self.face_hero(senses, true, &mut out);
                if self.unalert(senses) {
                    self.phase = Phase::Idle;
                    self.buzz.enter(senses.position);
                    out.push(Action::Play(Clip::Fly, 10));
                    return out;
                }
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.phase = Phase::Raycast;
                }
            }
            Phase::Raycast => {
                // Distance > 14 or an obstructed ray: another Distance Fly.
                let dx = (senses.hero[0] as i64 - senses.position[0] as i64) >> 8;
                let dy = (senses.hero[1] as i64 - senses.position[1] as i64) >> 8;
                let limit = (RAYCAST_DISTANCE >> 8) as i64;
                if dx * dx + dy * dy > limit * limit || !senses.sight_clear {
                    self.begin_distance_fly(&mut out);
                } else {
                    self.phase = Phase::FlyBack;
                    self.timer = FLY_BACK_TICKS;
                }
            }
            Phase::FlyBack => {
                self.fly(senses, fixed, FLY_BACK, &mut out);
                self.face_hero(senses, true, &mut out);
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::FireAnticipate;
                    self.timer = FIRE_TRIGGER_TICKS;
                    out.push(Action::Play(Clip::FireLong, 0));
                }
            }
            Phase::FireAnticipate => {
                self.fly(senses, fixed, ANTICIPATE, &mut out);
                self.face_hero(senses, false, &mut out);
                self.timer -= 1;
                if self.timer == 0 {
                    // Fire: the shot leaves the actor toward the hero at 15.
                    let dx = senses.hero[0] as i64 - senses.position[0] as i64;
                    let dy = senses.hero[1] as i64 - senses.position[1] as i64;
                    let length = isqrt(dx * dx + dy * dy).max(1);
                    // dx, dy and length fit i32 while positions stay inside +/-2^30 (Q16 +/-16384 units).
                    let v = [
                        psx_math::int32::mul_div_i32(dx as i32, SHOT_SPEED, length as i32),
                        psx_math::int32::mul_div_i32(dy as i32, SHOT_SPEED, length as i32),
                    ];
                    out.push(Action::Fire(v));
                    self.face_hero(senses, false, &mut out);
                    self.phase = Phase::FireDribble;
                    self.timer = FIRE_LONG_TICKS - FIRE_TRIGGER_TICKS;
                }
            }
            Phase::FireDribble => {
                // Spatter dribbles are cosmetic; the Fire Long clip completes.
                self.timer -= 1;
                if self.timer == 0 {
                    self.begin_distance_fly(&mut out);
                }
            }
            Phase::Dead => {}
        }
        out
    }
}
