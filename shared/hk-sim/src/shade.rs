//! Source-derived Hollow Shade `Shade Control` FSM: alert, chase, the
//! Position approach and the lunging Slash, plus the Max Roam retreat and the
//! death sequence. Evidence: .hkpsx/shade/CONTRACT.md.
//!
//! The caller owns physics (gravity-free dynamic body), the alert circle and
//! line of sight, Recoil, hits, the Geo pool and the soul limiter. At this
//! port's spell levels the Fireball, Quake, Scream, Friendly, Lake and Jar
//! branches are unreachable, so the ATTACK chain collapses to Position and the
//! only observable randomness is Fly's WaitRandom. The source 50 Hz fixed
//! steps run from an accumulator inside the 60 Hz tick.
use crate::buzz::{clamp, distance_fly_height};
use crate::ONE;

pub const ALERT_RADIUS: i32 = 476448; // 7.27
pub const CHASE_SPEED_MAX: i32 = 4 * ONE;
pub const CHASE_ACCELERATION: i32 = 13107; // .2 per fixed step
/// ChaseObjectV2 AddForce 8 at the source 0.02 fixed step, mass 1.
pub const CHASE_FORCE: i32 = 10486;
pub const POSITION_DISTANCE: i32 = 3 * ONE;
pub const SLASH_RANGE: i32 = 5 * ONE;
pub const SAME_Y_TOLERANCE: i32 = 13107; // .2
pub const MAX_ROAM: i32 = 25 * ONE;
pub const LUNGE_SPEED: i32 = 8 * ONE;
pub const STARTLE_TICKS: u16 = 35; // 7 frames at 12 fps
pub const FLY_WAIT_MIN: u16 = 60; // WaitRandom 1..2 s
pub const FLY_WAIT_SPAN: u16 = 60;
pub const POSITION_TIMEOUT: u16 = 360; // Wait 6
pub const SLASH_ANTIC_TICKS: u16 = 30; // 6 frames at 12 fps
pub const SLASH_TICKS: u16 = 5; // Wait .083
pub const SLASH_BOX_TICKS: u16 = 1; // Slash (2 at 24 fps) has already completed
pub const SLASH_CD_TICKS: u16 = 10; // 2 frames at 12 fps
pub const RETREAT_START_TICKS: u16 = 26; // 7 frames at 16 fps
pub const RETREAT_TICKS: u16 = 60; // iTweenMoveTo time 1
pub const RETREAT_END_TICKS: u16 = 26;
pub const DEATH_START_TICKS: u16 = 30; // Wait .5
pub const DEATH_TICKS: u16 = 45; // Death, 12 frames at 16 fps
pub const DECELERATION: i32 = 55706; // Decelerate .85 retained per fixed step

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Startle,
    Fly,
    Position,
    SlashAntic,
    /// The damage box is live for exactly this phase.
    Slash,
    SlashBox,
    SlashCd,
    RetreatStart,
    Retreat,
    RetreatEnd,
    DeathStart,
    Death,
    /// Removed; the caller drops the Shade.
    Gone,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Idle,
    Startle,
    Fly,
    TurnToFly,
    SlashAntic,
    Slash,
    SlashCd,
    RetreatStart,
    RetreatEnd,
    DeathStart,
    Death,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Velocity([i32; 2]),
    /// Retreat's iTween path, as a straight interpolation to the start.
    MoveTo([i32; 2]),
    Play(Clip, u32),
    Facing(i32),
    /// The Slash child's polygon collider and its Slash Effect clip.
    SlashOn,
    SlashOff,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 4],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self { values: [None; 4], count: 0 }
    }
    fn push(&mut self, action: Action) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize].iter().map(|a| a.unwrap())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// Inside the 7.27 alert circle AND an unobstructed line of sight.
    pub can_see_hero: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shade {
    phase: Phase,
    velocity: [i32; 2],
    start: [i32; 2],
    retreat_from: [i32; 2],
    timer: u16,
    facing: i32,
    fixed_accumulator: u8,
    rng: u32,
}
impl Shade {
    /// `start` is the spawn position, which Max Roam measures against.
    pub fn new(start: [i32; 2], seed: u32) -> Self {
        Self { phase: Phase::Idle, velocity: [0; 2], start, retreat_from: start, timer: 0,
            facing: -1, fixed_accumulator: 0, rng: seed }
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
    /// The Slash child's damage box is only live during the Slash state.
    pub fn slashing(self) -> bool {
        self.phase == Phase::Slash
    }
    /// Alive for hit and contact purposes: the death sequence is not.
    pub fn vulnerable(self) -> bool {
        !matches!(self.phase, Phase::DeathStart | Phase::Death | Phase::Gone)
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    /// HealthManager death: the corpse FSM's Death Start owns the Geo return.
    pub fn die(&mut self) -> Actions {
        let mut out = Actions::new();
        if !self.vulnerable() {
            return out;
        }
        self.phase = Phase::DeathStart;
        self.timer = DEATH_START_TICKS;
        self.velocity = [0; 2];
        out.push(Action::Velocity([0; 2]));
        out.push(Action::Play(Clip::DeathStart, 0));
        out.push(Action::SlashOff);
        out
    }
    /// Only Idle listens for TOOK DAMAGE.
    pub fn took_damage(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Idle {
            self.startle(senses, &mut out);
        }
        out
    }
    fn startle(&mut self, _senses: Senses, out: &mut Actions) {
        self.phase = Phase::Startle;
        self.timer = STARTLE_TICKS;
        out.push(Action::Play(Clip::Startle, 0));
    }
    fn begin_fly(&mut self, out: &mut Actions) {
        self.phase = Phase::Fly;
        self.timer = FLY_WAIT_MIN + (self.random() % (FLY_WAIT_SPAN as u32 + 1)) as u16;
        out.push(Action::Play(Clip::Fly, 0));
    }
    /// FaceObject every frame: the sprite faces the hero, with no pause.
    fn face_hero(&mut self, senses: Senses, out: &mut Actions) {
        let want = if senses.hero[0] > senses.position[0] { 1 } else { -1 };
        if want != self.facing {
            self.facing = want;
            out.push(Action::Facing(want));
            out.push(Action::Play(Clip::TurnToFly, 0));
        }
    }
    /// ChaseObject.DoBuzz then ChaseObjectV2.DoChase, in the source order.
    fn chase(&mut self, senses: Senses) {
        let mut v = self.velocity;
        for axis in 0..2 {
            v[axis] += if senses.hero[axis] > senses.position[axis] { CHASE_ACCELERATION } else { -CHASE_ACCELERATION };
        }
        clamp(&mut v, CHASE_SPEED_MAX);
        // ClampMagnitude(hero - self, 1) * accelerationForce, then a magnitude clamp.
        let dx = (senses.hero[0] as i64 - senses.position[0] as i64) >> 8;
        let dy = (senses.hero[1] as i64 - senses.position[1] as i64) >> 8;
        let length = isqrt(dx * dx + dy * dy);
        if length > 0 {
            // dx and dy are i32 differences shifted by 8, so they and length fit i32.
            v[0] += psx_math::int32::mul_div_i32(dx as i32, CHASE_FORCE, length as i32);
            v[1] += psx_math::int32::mul_div_i32(dy as i32, CHASE_FORCE, length as i32);
        }
        clamp_magnitude(&mut v, CHASE_SPEED_MAX);
        self.velocity = v;
    }
    /// Distance from the spawn beyond Max Roam sends RETREAT from Fly/Position.
    fn roamed(&self, senses: Senses) -> bool {
        let dx = (senses.position[0] as i64 - self.start[0] as i64) >> 8;
        let dy = (senses.position[1] as i64 - self.start[1] as i64) >> 8;
        let limit = (MAX_ROAM >> 8) as i64;
        dx * dx + dy * dy > limit * limit
    }
    fn begin_retreat(&mut self, senses: Senses, out: &mut Actions) {
        self.phase = Phase::RetreatStart;
        self.timer = RETREAT_START_TICKS;
        self.retreat_from = senses.position;
        out.push(Action::Play(Clip::RetreatStart, 0));
        out.push(Action::SlashOff);
    }
    /// One nominal 60 Hz frame.
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Gone {
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
                }
            }
            Phase::Startle => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.begin_fly(&mut out);
                }
            }
            Phase::Fly => {
                if fixed {
                    self.chase(senses);
                    out.push(Action::Velocity(self.velocity));
                }
                self.face_hero(senses, &mut out);
                if self.roamed(senses) {
                    self.begin_retreat(senses, &mut out);
                    return out;
                }
                self.timer -= 1;
                if self.timer == 0 {
                    // The whole ATTACK chain falls through to Position here.
                    self.phase = Phase::Position;
                    self.timer = POSITION_TIMEOUT;
                }
            }
            Phase::Position => {
                if fixed {
                    let mut v = self.velocity;
                    distance_fly_height(senses.position, senses.hero, POSITION_DISTANCE, 0, CHASE_SPEED_MAX, CHASE_ACCELERATION, &mut v);
                    self.velocity = v;
                    out.push(Action::Velocity(self.velocity));
                }
                self.face_hero(senses, &mut out);
                if self.roamed(senses) {
                    self.begin_retreat(senses, &mut out);
                    return out;
                }
                let dx = (senses.hero[0] as i64 - senses.position[0] as i64) >> 8;
                let dy = (senses.hero[1] as i64 - senses.position[1] as i64) >> 8;
                let range = (SLASH_RANGE >> 8) as i64;
                let in_range = dx * dx + dy * dy < range * range;
                let same_y = (senses.hero[1] - senses.position[1]).abs() <= SAME_Y_TOLERANCE;
                if in_range && same_y {
                    self.phase = Phase::SlashAntic;
                    self.timer = SLASH_ANTIC_TICKS;
                    out.push(Action::Play(Clip::SlashAntic, 0));
                    return out;
                }
                self.timer -= 1;
                if self.timer == 0 {
                    // Wait 6 re-enters the attack chain, which returns here.
                    self.timer = POSITION_TIMEOUT;
                }
            }
            Phase::SlashAntic => {
                if fixed {
                    let mut v = self.velocity;
                    distance_fly_height(senses.position, senses.hero, POSITION_DISTANCE, 0, CHASE_SPEED_MAX, CHASE_ACCELERATION, &mut v);
                    self.velocity = v;
                    out.push(Action::Velocity(self.velocity));
                }
                self.timer -= 1;
                if self.timer == 0 {
                    // Check Dir: the sprite faces left, so facing -1 lunges left.
                    self.velocity = [self.facing * LUNGE_SPEED, 0];
                    self.phase = Phase::Slash;
                    self.timer = SLASH_TICKS;
                    out.push(Action::Velocity(self.velocity));
                    out.push(Action::Play(Clip::Slash, 0));
                    out.push(Action::SlashOn);
                }
            }
            Phase::Slash => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::SlashBox;
                    self.timer = SLASH_BOX_TICKS;
                    out.push(Action::SlashOff);
                }
            }
            Phase::SlashBox => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::SlashCd;
                    self.timer = SLASH_CD_TICKS;
                    out.push(Action::Play(Clip::SlashCd, 0));
                }
            }
            Phase::SlashCd => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.begin_fly(&mut out);
                }
            }
            Phase::RetreatStart => {
                if fixed {
                    // Decelerate .85 retained per fixed step.
                    for axis in self.velocity.iter_mut() {
                        *axis = ((*axis as i64 * DECELERATION as i64) >> 16) as i32;
                    }
                    out.push(Action::Velocity(self.velocity));
                }
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Retreat;
                    self.timer = RETREAT_TICKS;
                    self.retreat_from = senses.position;
                    self.velocity = [0; 2];
                    out.push(Action::Velocity([0; 2]));
                }
            }
            Phase::Retreat => {
                self.timer -= 1;
                let done = RETREAT_TICKS - self.timer;
                let at = |from: i32, to: i32| {
                    // to - from fits i32 while positions stay inside +/-2^30.
                    from + psx_math::int32::mul_div_i32(to - from, done as i32, RETREAT_TICKS as i32)
                };
                out.push(Action::MoveTo([at(self.retreat_from[0], self.start[0]), at(self.retreat_from[1], self.start[1])]));
                if self.timer == 0 {
                    self.phase = Phase::RetreatEnd;
                    self.timer = RETREAT_END_TICKS;
                    out.push(Action::Play(Clip::RetreatEnd, 0));
                }
            }
            Phase::RetreatEnd => {
                self.timer -= 1;
                if self.timer == 0 {
                    // Retreat Reset returns to Idle with the Idle clip.
                    self.phase = Phase::Idle;
                    out.push(Action::Play(Clip::Idle, 0));
                }
            }
            Phase::DeathStart => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Death;
                    self.timer = DEATH_TICKS;
                    out.push(Action::Play(Clip::Death, 0));
                }
            }
            Phase::Death => {
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Gone;
                }
            }
            Phase::Gone => {}
        }
        out
    }
}
/// Floor square root using the shared SDK restoring algorithm.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        0
    } else {
        psx_math::int32::isqrt_u64(n as u64) as i64
    }
}
/// Vector2.ClampMagnitude: scale down only when longer than `max`.
fn clamp_magnitude(v: &mut [i32; 2], max: i32) {
    let x = (v[0] >> 8) as i64;
    let y = (v[1] >> 8) as i64;
    let limit = (max >> 8) as i64;
    let square = x * x + y * y;
    if square <= limit * limit {
        return;
    }
    // The truncated >>8 magnitude underestimates, so bias the divisor up by one
    // to keep the scaled result at or under `max` when it is measured again.
    let length = isqrt(square).max(1) + 1;
    // limit and length come from i32 values shifted by 8, so both fit i32.
    v[0] = psx_math::int32::mul_div_i32(v[0], limit as i32, length as i32);
    v[1] = psx_math::int32::mul_div_i32(v[1], limit as i32, length as i32);
}

#[cfg(test)]
mod tests {
    use super::*;
    const START: [i32; 2] = [0, 0];
    fn senses(shade: [i32; 2], hero: [i32; 2], see: bool) -> Senses {
        Senses { position: shade, hero, can_see_hero: see }
    }
    fn run(s: &mut Shade, at: [i32; 2], hero: [i32; 2], see: bool, ticks: u32) {
        for _ in 0..ticks {
            s.tick(senses(at, hero, see));
        }
    }
    /// Advance to a phase within a bounded number of ticks.
    fn until(s: &mut Shade, at: [i32; 2], hero: [i32; 2], phase: Phase) {
        for _ in 0..600 {
            if s.phase() == phase {
                return;
            }
            s.tick(senses(at, hero, true));
        }
        panic!("phase not reached");
    }
    #[test]
    fn sight_startles_then_chases_and_the_chase_speed_stays_bounded() {
        let mut s = Shade::new(START, 1);
        s.tick(senses(START, [10 * ONE, 0], false));
        assert_eq!(s.phase(), Phase::Idle);
        s.tick(senses(START, [10 * ONE, 0], true));
        assert_eq!(s.phase(), Phase::Startle);
        run(&mut s, START, [10 * ONE, 0], true, STARTLE_TICKS as u32);
        assert_eq!(s.phase(), Phase::Fly);
        // ChaseObjectV2's ClampMagnitude bounds the Fly speed, unlike the
        // per-axis DistanceFly clamp that Position uses.
        while s.phase() == Phase::Fly {
            s.tick(senses(START, [10 * ONE, 0], true));
            let v = s.velocity();
            let speed = isqrt(((v[0] >> 8) as i64).pow(2) + ((v[1] >> 8) as i64).pow(2));
            assert!(speed <= (CHASE_SPEED_MAX >> 8) as i64, "speed {speed}");
        }
        assert_eq!(s.phase(), Phase::Position);
        assert_eq!(s.facing(), 1);
    }
    #[test]
    fn position_only_slashes_in_range_and_level_with_the_hero() {
        let mut s = Shade::new(START, 7);
        // Fly waits 1..2 s before the attack chain, whatever the hero does.
        until(&mut s, START, [20 * ONE, 0], Phase::Position);
        // Level but beyond the 5-unit slash range stays in Position.
        run(&mut s, START, [20 * ONE, 0], true, 5);
        assert_eq!(s.phase(), Phase::Position);
        // In range but not level stays in Position.
        run(&mut s, START, [ONE, 4 * ONE], true, 5);
        assert_eq!(s.phase(), Phase::Position);
        s.tick(senses(START, [ONE, SAME_Y_TOLERANCE], true));
        assert_eq!(s.phase(), Phase::SlashAntic);
    }
    #[test]
    fn the_slash_lunges_toward_the_faced_side_and_only_damages_during_slash() {
        let mut s = Shade::new(START, 3);
        until(&mut s, START, [-20 * ONE, 0], Phase::Position);
        s.tick(senses(START, [-ONE, 0], true));
        assert_eq!(s.phase(), Phase::SlashAntic);
        assert_eq!(s.facing(), -1);
        assert!(!s.slashing());
        run(&mut s, START, [-ONE, 0], true, SLASH_ANTIC_TICKS as u32);
        assert_eq!(s.phase(), Phase::Slash);
        assert_eq!(s.velocity(), [-LUNGE_SPEED, 0]);
        assert!(s.slashing());
        run(&mut s, START, [-ONE, 0], true, SLASH_TICKS as u32);
        assert!(!s.slashing());
        run(&mut s, START, [-ONE, 0], true, (SLASH_BOX_TICKS + SLASH_CD_TICKS) as u32);
        assert_eq!(s.phase(), Phase::Fly);
    }
    #[test]
    fn roaming_past_max_roam_retreats_to_the_spawn_and_returns_to_idle() {
        let mut s = Shade::new(START, 5);
        let far = [30 * ONE, 0];
        s.tick(senses(START, far, true));
        run(&mut s, START, far, true, STARTLE_TICKS as u32);
        assert_eq!(s.phase(), Phase::Fly);
        s.tick(senses(far, far, true));
        assert_eq!(s.phase(), Phase::RetreatStart);
        run(&mut s, far, far, true, RETREAT_START_TICKS as u32);
        assert_eq!(s.phase(), Phase::Retreat);
        let mut last = far;
        for _ in 0..RETREAT_TICKS {
            for action in s.tick(senses(last, far, true)).iter() {
                if let Action::MoveTo(p) = action {
                    last = p;
                }
            }
        }
        assert_eq!(last, START);
        assert_eq!(s.phase(), Phase::RetreatEnd);
        run(&mut s, START, far, false, RETREAT_END_TICKS as u32);
        assert_eq!(s.phase(), Phase::Idle);
    }
    #[test]
    fn death_runs_its_sequence_once_and_stops_being_vulnerable() {
        let mut s = Shade::new(START, 9);
        assert!(s.vulnerable());
        assert!(s.die().iter().any(|a| a == Action::Play(Clip::DeathStart, 0)));
        assert!(!s.vulnerable());
        assert_eq!(s.die().iter().count(), 0, "a second death is not a second sequence");
        run(&mut s, START, START, false, DEATH_START_TICKS as u32);
        assert_eq!(s.phase(), Phase::Death);
        run(&mut s, START, START, false, DEATH_TICKS as u32);
        assert_eq!(s.phase(), Phase::Gone);
        run(&mut s, START, START, false, 10);
        assert_eq!(s.phase(), Phase::Gone);
    }
    #[test]
    fn taking_damage_only_startles_from_idle() {
        let mut s = Shade::new(START, 2);
        assert_eq!(s.took_damage(senses(START, [ONE, 0], false)).iter().count(), 1);
        assert_eq!(s.phase(), Phase::Startle);
        assert_eq!(s.took_damage(senses(START, [ONE, 0], false)).iter().count(), 0);
    }
}
