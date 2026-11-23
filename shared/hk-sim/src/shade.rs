//! Source-derived Hollow Shade `Shade Control` FSM: alert, chase, the
//! Position approach and the lunging Slash, the spells the Knight owned at
//! death (the Shadow Ball, the Quake dive and the Scream), the Max Roam
//! retreat and the death sequence. Evidence: host/shade.py's audit.
//!
//! The caller owns physics (gravity-free dynamic body), the alert circle and
//! line of sight, the Quake and Scream range triggers, Recoil, hits, the Shadow
//! Ball's flight, the Geo pool and the soul limiter. `Spells` is what `Init`
//! reads from PlayerData: the three levels and `shadeMP`, the casts it has.
//! The source 50 Hz fixed steps run from an accumulator inside the 60 Hz tick.
//!
//! The source rolls `SendRandomEvent` in Attack Choice, Q Other?, Quake? and
//! Scream? whether or not the roll can change anything; here a roll is drawn
//! only where its outcome can, so a Shade without spells draws exactly what it
//! drew before the spells existed. The distributions are the source's.
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
/// Attack Choice: FIREBALL at weight .2 against SLASH at .8.
pub const FIREBALL_ODDS: u32 = 5;
/// Fireball Pos: DistanceFly 12 (speed 4, acceleration .2, level), ATTACK
/// within 1 of that distance and level with the hero, or after Wait 3.
pub const FIREBALL_DISTANCE: i32 = 12 * ONE;
pub const FIREBALL_TOLERANCE: i32 = ONE;
pub const FIREBALL_POS_TIMEOUT: u16 = 180;
/// Cast Antic: speed times .8 per fixed step, Wait .5.
pub const CAST_ANTIC_TICKS: u16 = 30;
pub const CAST_ANTIC_DECEL: i32 = 52429;
/// Left 2 / Right 2: `X Speed` 10 away from the cast; Charge Wait .25.
pub const CAST_RECOIL: i32 = 10 * ONE;
pub const CHARGE_WAIT_TICKS: u16 = 15;
/// Cast and Decel: the Cast clip (8 frames at 12 fps), speed times .9 per fixed step.
pub const CAST_TICKS: u16 = 40;
pub const CAST_DECEL: i32 = 58982;
/// Cooldown: the same .9, Wait .25.
pub const COOLDOWN_TICKS: u16 = 15;
/// Quake Antic: SetVelocity2d (0, 10), DecelerateXY y .85, Wait .8.
pub const QUAKE_RISE: i32 = 10 * ONE;
pub const QUAKE_ANTIC_TICKS: u16 = 48;
/// Quake Start: its clip, 1 frame at 12 fps.
pub const QUAKE_START_TICKS: u16 = 5;
/// Quake: SetVelocity2d y -30 until the bottom collides, or Wait 1.
pub const QUAKE_FALL: i32 = -30 * ONE;
pub const QUAKE_TICKS: u16 = 60;
/// Land: Quake Land, 3 frames at 15 fps; Quake Hit's own Deactivate waits .2.
pub const QUAKE_LAND_TICKS: u16 = 12;
pub const SPELL_HIT_TICKS: u16 = 12;
/// Return Pause: the mesh off, Wait 1. Quake Return: iTweenMoveTo the
/// position Quake Antic stored, at speed 10, easeInOutSine.
pub const RETURN_PAUSE_TICKS: u16 = 60;
pub const QUAKE_RETURN_SPEED: i32 = 10 * ONE;
/// Scream Antic: Decelerate .85, Wait 1; Scream: Wait 1; Scream End, 2 frames at 12 fps.
pub const SCREAM_ANTIC_TICKS: u16 = 60;
pub const SCREAM_TICKS: u16 = 60;
pub const SCREAM_END_TICKS: u16 = 10;
/// Shadow Ball: SetVelocity2d 20 from Shoot R / Shoot L, `Shadow ball` Wait 1,
/// then Fade for Fireball End (4 frames at 24 fps), which does not slow it.
pub const BALL_SPEED: i32 = 20 * ONE;
pub const BALL_TICKS: u16 = 60;
pub const BALL_END_TICKS: u16 = 10;

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
    /// Fireball Pos, Cast Antic, Cast Charge's Charge Wait, Cast with Decel, Cooldown.
    FireballPos,
    CastAntic,
    ChargeWait,
    Cast,
    Cooldown,
    QuakeAntic,
    QuakeStart,
    Quake,
    /// Land: Quake Hit is live for its first `SPELL_HIT_TICKS`, all of it.
    QuakeLand,
    ReturnPause,
    QuakeReturn,
    Reform,
    ScreamAntic,
    /// Scream Hit is live for its first `SPELL_HIT_TICKS`.
    Scream,
    ScreamRecover,
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
    CastAntic,
    CastCharge,
    Cast,
    QuakeAntic,
    QuakeStart,
    Quake,
    QuakeLand,
    ScreamAntic,
    Scream,
    ScreamEnd,
    RetreatStart,
    RetreatEnd,
    DeathStart,
    Death,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Velocity([i32; 2]),
    /// Retreat's and Quake Return's iTween path, as a straight line.
    MoveTo([i32; 2]),
    Play(Clip, u32),
    Facing(i32),
    /// The Slash child's polygon collider and its Slash Effect clip.
    SlashOn,
    SlashOff,
    /// Shoot R (1) or Shoot L (-1): a Shadow Ball leaves the Shade's position.
    Shoot(i32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    // Fly's worst tick: its velocity, a turn (two) and a Quake Antic (two).
    values: [Option<Action>; 6],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 6],
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
/// What `Init` reads from PlayerData: `shadeFireballLevel`, `shadeQuakeLevel`,
/// `shadeScreamLevel` (the Knight's own levels at death) and `shadeMP`, one
/// cast plus one per reserve vessel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spells {
    pub fireball: u8,
    pub quake: u8,
    pub scream: u8,
    pub mp: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// Inside the 7.27 alert circle AND an unobstructed line of sight.
    pub can_see_hero: bool,
    /// The Quake Range and Scream Range children's triggers hold the hero.
    pub quake_range: bool,
    pub scream_range: bool,
    /// The body's bottom touched terrain on the last step (CheckCollisionSide).
    pub grounded: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shade {
    phase: Phase,
    velocity: [i32; 2],
    start: [i32; 2],
    /// An iTween's ends and length: the retreat to `start`, or the Quake's
    /// return to where Quake Antic began.
    move_from: [i32; 2],
    move_to: [i32; 2],
    move_ticks: u16,
    timer: u16,
    facing: i32,
    fixed_accumulator: u8,
    spells: Spells,
    /// Q Other?'s Will Quake (1) and Will Scream (2).
    will: u8,
    rng: u32,
}
const WILL_QUAKE: u8 = 1;
const WILL_SCREAM: u8 = 2;
impl Shade {
    /// `start` is the spawn position, which Max Roam measures against.
    pub fn new(start: [i32; 2], seed: u32, spells: Spells) -> Self {
        Self {
            phase: Phase::Idle,
            velocity: [0; 2],
            start,
            move_from: start,
            move_to: start,
            move_ticks: RETREAT_TICKS,
            timer: 0,
            facing: -1,
            fixed_accumulator: 0,
            spells,
            will: 0,
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
    pub fn spells(self) -> Spells {
        self.spells
    }
    /// The Slash child's damage box is only live during the Slash state.
    pub fn slashing(self) -> bool {
        self.phase == Phase::Slash
    }
    /// Quake Hit, active from Land for its Deactivate's .2 s.
    pub fn quake_hitting(self) -> bool {
        self.phase == Phase::QuakeLand
    }
    /// Scream Hit, active from Scream for its Deactivate's .2 s.
    pub fn scream_hitting(self) -> bool {
        self.phase == Phase::Scream && self.timer > SCREAM_TICKS - SPELL_HIT_TICKS
    }
    /// Return Pause turns the mesh off and Reform turns it back on.
    pub fn hidden(self) -> bool {
        matches!(self.phase, Phase::ReturnPause | Phase::QuakeReturn)
    }
    /// Moved by an iTween, not by its body.
    pub fn tweening(self) -> bool {
        matches!(self.phase, Phase::Retreat | Phase::QuakeReturn)
    }
    /// Alive for hit and contact purposes: the death sequence is not, nor
    /// the Quake from Land's SetCollider false to Collider On.
    pub fn vulnerable(self) -> bool {
        !matches!(
            self.phase,
            Phase::QuakeLand
                | Phase::ReturnPause
                | Phase::QuakeReturn
                | Phase::Reform
                | Phase::DeathStart
                | Phase::Death
                | Phase::Gone
        )
    }
    fn random(&mut self) -> u32 {
        self.rng = psx_fx::NrLcg::step(self.rng);
        self.rng >> 8
    }
    /// HealthManager death: the corpse FSM's Death Start owns the Geo return.
    pub fn die(&mut self) -> Actions {
        let mut out = Actions::new();
        if matches!(self.phase, Phase::DeathStart | Phase::Death | Phase::Gone) {
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
        self.enter(Phase::Startle, STARTLE_TICKS, Clip::Startle, out);
    }
    fn enter(&mut self, phase: Phase, timer: u16, clip: Clip, out: &mut Actions) {
        self.phase = phase;
        self.timer = timer;
        out.push(Action::Play(clip, 0));
    }
    fn begin_fly(&mut self, out: &mut Actions) {
        let wait = FLY_WAIT_MIN + (self.random() % (FLY_WAIT_SPAN as u32 + 1)) as u16;
        self.enter(Phase::Fly, wait, Clip::Fly, out);
    }
    /// FaceObject every frame: the sprite faces the hero, with no pause.
    fn face_hero(&mut self, senses: Senses, out: &mut Actions) {
        let want = if senses.hero[0] > senses.position[0] {
            1
        } else {
            -1
        };
        if want != self.facing {
            self.facing = want;
            out.push(Action::Facing(want));
            out.push(Action::Play(Clip::TurnToFly, 0));
        }
    }
    /// ChaseObject.DoBuzz then ChaseObjectV2.DoChase, in the source order.
    fn chase(&mut self, senses: Senses) {
        let mut v = self.velocity;
        for (axis, slot) in v.iter_mut().enumerate() {
            *slot += if senses.hero[axis] > senses.position[axis] {
                CHASE_ACCELERATION
            } else {
                -CHASE_ACCELERATION
            };
        }
        clamp(&mut v, CHASE_SPEED_MAX);
        // ClampMagnitude(hero - self, 1) * accelerationForce, then a magnitude clamp.
        let dx = crate::fixed::diff8(senses.hero[0], senses.position[0]);
        let dy = crate::fixed::diff8(senses.hero[1], senses.position[1]);
        let length = crate::fixed::hypot(dx, dy);
        if length > 0 {
            // dx and dy are i32 differences shifted by 8, so they and length fit i32.
            v[0] += psx_math::int32::mul_div_i32(dx, CHASE_FORCE, length as i32);
            v[1] += psx_math::int32::mul_div_i32(dy, CHASE_FORCE, length as i32);
        }
        clamp_magnitude(&mut v, CHASE_SPEED_MAX);
        self.velocity = v;
    }
    /// DistanceFly toward `distance` from the hero, level with it.
    #[cfg_attr(target_arch = "mips", optimize(size))]
    #[inline(never)]
    fn keep_distance(&mut self, senses: Senses, distance: i32, out: &mut Actions) {
        let mut v = self.velocity;
        distance_fly_height(
            senses.position,
            senses.hero,
            distance,
            0,
            CHASE_SPEED_MAX,
            CHASE_ACCELERATION,
            &mut v,
        );
        self.velocity = v;
        out.push(Action::Velocity(v));
    }
    /// Velocity times `factor` (Q16) on each axis: DecelerateV2 and FloatMultiplyV2.
    #[cfg_attr(target_arch = "mips", optimize(size))]
    #[inline(never)]
    fn decelerate(&mut self, factor: i32, out: &mut Actions) {
        for axis in self.velocity.iter_mut() {
            *axis = psx_math::int32::mul_shr_i32(*axis, factor, 16);
        }
        out.push(Action::Velocity(self.velocity));
    }
    /// Distance from the spawn beyond Max Roam sends RETREAT from Fly/Position.
    fn roamed(&self, senses: Senses) -> bool {
        let dx = crate::fixed::diff8(senses.position[0], self.start[0]);
        let dy = crate::fixed::diff8(senses.position[1], self.start[1]);
        crate::fixed::sum_sq(dx, dy) > crate::fixed::square(MAX_ROAM >> 8)
    }
    fn begin_retreat(&mut self, out: &mut Actions) {
        self.enter(Phase::RetreatStart, RETREAT_START_TICKS, Clip::RetreatStart, out);
        out.push(Action::SlashOff);
    }
    /// Quake? then Scream? then Attack Choice, as Fly's ATTACK and Position's
    /// END reach them. Each spell gate is its level above 0, a cast left and
    /// its range trigger holding the hero, then an even roll.
    #[cfg_attr(target_arch = "mips", optimize(size))]
    #[inline(never)]
    fn attack_chain(&mut self, senses: Senses, out: &mut Actions) {
        let s = self.spells;
        if s.quake > 0 && s.mp >= 1 && senses.quake_range && self.random() % 2 == 1 {
            return self.quake_antic(senses, out);
        }
        if s.scream > 0 && s.mp >= 1 && senses.scream_range && self.random() % 2 == 1 {
            return self.scream_antic(out);
        }
        // Attack Choice and Sp Check: FIREBALL with Fireball Level 0 or no cast
        // left RETURNs to the choice, so only SLASH can come of it then.
        if s.fireball >= 1 && s.mp >= 1 && self.random() % FIREBALL_ODDS == 0 {
            self.spells.mp -= 1;
            self.phase = Phase::FireballPos;
            self.timer = FIREBALL_POS_TIMEOUT;
            return;
        }
        // Q Other?: SCREAM, QUAKE and FINISHED evenly, unless no cast is left;
        // Will Quake and Will Scream only stick with that spell's level.
        self.will = 0;
        if s.mp != 0 && (s.quake > 0 || s.scream > 0) {
            match self.random() % 3 {
                0 if s.scream > 0 => self.will = WILL_SCREAM,
                1 if s.quake > 0 => self.will = WILL_QUAKE,
                _ => {}
            }
        }
        // Position's entry: a willed spell goes ahead only in its range.
        if self.will == WILL_QUAKE && senses.quake_range {
            return self.quake_antic(senses, out);
        }
        if self.will == WILL_SCREAM && senses.scream_range {
            return self.scream_antic(out);
        }
        self.phase = Phase::Position;
        self.timer = POSITION_TIMEOUT;
    }
    fn quake_antic(&mut self, senses: Senses, out: &mut Actions) {
        self.move_to = senses.position;
        self.spells.mp -= 1;
        self.velocity = [0, QUAKE_RISE];
        out.push(Action::Velocity(self.velocity));
        self.enter(Phase::QuakeAntic, QUAKE_ANTIC_TICKS, Clip::QuakeAntic, out);
    }
    fn scream_antic(&mut self, out: &mut Actions) {
        self.spells.mp -= 1;
        self.enter(Phase::ScreamAntic, SCREAM_ANTIC_TICKS, Clip::ScreamAntic, out);
    }
    /// One nominal 60 Hz frame.
    #[cfg_attr(target_arch = "mips", optimize(size))]
    #[inline(never)]
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
        // Every timed state counts down here; `done` is its last tick.
        self.timer = self.timer.saturating_sub(1);
        let done = self.timer == 0;
        match self.phase {
            Phase::Idle => {
                if senses.can_see_hero {
                    self.startle(senses, &mut out);
                }
            }
            Phase::Startle => {
                if done {
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
                    self.begin_retreat(&mut out);
                } else if done {
                    self.attack_chain(senses, &mut out);
                }
            }
            Phase::Position | Phase::FireballPos => {
                let fireball = self.phase == Phase::FireballPos;
                if fixed {
                    let distance = if fireball {
                        FIREBALL_DISTANCE
                    } else {
                        POSITION_DISTANCE
                    };
                    self.keep_distance(senses, distance, &mut out);
                }
                self.face_hero(senses, &mut out);
                if self.roamed(senses) {
                    self.begin_retreat(&mut out);
                    return out;
                }
                let dx = crate::fixed::diff8(senses.hero[0], senses.position[0]);
                let dy = crate::fixed::diff8(senses.hero[1], senses.position[1]);
                let d = crate::fixed::sum_sq(dx, dy);
                let in_range = if fireball {
                    // FloatTestToBool 12 with tolerance 1.
                    d >= crate::fixed::square((FIREBALL_DISTANCE - FIREBALL_TOLERANCE) >> 8)
                        && d <= crate::fixed::square((FIREBALL_DISTANCE + FIREBALL_TOLERANCE) >> 8)
                } else {
                    d < crate::fixed::square(SLASH_RANGE >> 8)
                };
                let same_y = (senses.hero[1] - senses.position[1]).abs() <= SAME_Y_TOLERANCE;
                if fireball && (done || in_range && same_y) {
                    self.enter(Phase::CastAntic, CAST_ANTIC_TICKS, Clip::CastAntic, &mut out);
                } else if in_range && same_y {
                    self.enter(Phase::SlashAntic, SLASH_ANTIC_TICKS, Clip::SlashAntic, &mut out);
                } else if done {
                    // Wait 6 sends END, back into the attack chain.
                    self.attack_chain(senses, &mut out);
                }
            }
            Phase::SlashAntic => {
                if fixed {
                    self.keep_distance(senses, POSITION_DISTANCE, &mut out);
                }
                if done {
                    // Check Dir: the sprite faces left, so facing -1 lunges left.
                    self.velocity = [self.facing * LUNGE_SPEED, 0];
                    self.enter(Phase::Slash, SLASH_TICKS, Clip::Slash, &mut out);
                    out.push(Action::Velocity(self.velocity));
                    out.push(Action::SlashOn);
                }
            }
            Phase::Slash => {
                if done {
                    self.phase = Phase::SlashBox;
                    self.timer = SLASH_BOX_TICKS;
                    out.push(Action::SlashOff);
                }
            }
            Phase::SlashBox => {
                if done {
                    self.enter(Phase::SlashCd, SLASH_CD_TICKS, Clip::SlashCd, &mut out);
                }
            }
            Phase::SlashCd | Phase::Cooldown | Phase::ScreamRecover => {
                if fixed && self.phase == Phase::Cooldown {
                    self.decelerate(CAST_DECEL, &mut out);
                }
                if done {
                    self.begin_fly(&mut out);
                }
            }
            Phase::CastAntic => {
                if fixed {
                    self.decelerate(CAST_ANTIC_DECEL, &mut out);
                }
                if done {
                    // Cast Charge, Check Dir 2 and Left 2 / Right 2 in one frame:
                    // `X Speed` is 10 away from the side the Shade faces.
                    self.move_from[0] = -self.facing * CAST_RECOIL;
                    self.enter(Phase::ChargeWait, CHARGE_WAIT_TICKS, Clip::CastCharge, &mut out);
                }
            }
            Phase::ChargeWait => {
                if done {
                    // Check Dir 3, Shoot R / Shoot L, then Cast's SetVelocity2d x.
                    out.push(Action::Shoot(self.facing));
                    self.velocity[0] = self.move_from[0];
                    out.push(Action::Velocity(self.velocity));
                    self.enter(Phase::Cast, CAST_TICKS, Clip::Cast, &mut out);
                }
            }
            Phase::Cast => {
                if fixed {
                    self.decelerate(CAST_DECEL, &mut out);
                }
                if done {
                    self.enter(Phase::Cooldown, COOLDOWN_TICKS, Clip::Fly, &mut out);
                }
            }
            Phase::QuakeAntic => {
                if fixed {
                    // DecelerateXY: y only.
                    self.velocity[1] = psx_math::int32::mul_shr_i32(self.velocity[1], DECELERATION, 16);
                    out.push(Action::Velocity(self.velocity));
                }
                if done {
                    self.enter(Phase::QuakeStart, QUAKE_START_TICKS, Clip::QuakeStart, &mut out);
                }
            }
            Phase::QuakeStart => {
                if done {
                    self.velocity[1] = QUAKE_FALL;
                    out.push(Action::Velocity(self.velocity));
                    self.enter(Phase::Quake, QUAKE_TICKS, Clip::Quake, &mut out);
                }
            }
            Phase::Quake => {
                if done || senses.grounded {
                    self.velocity = [0; 2];
                    out.push(Action::Velocity(self.velocity));
                    self.enter(Phase::QuakeLand, QUAKE_LAND_TICKS, Clip::QuakeLand, &mut out);
                }
            }
            Phase::QuakeLand => {
                if done {
                    self.phase = Phase::ReturnPause;
                    self.timer = RETURN_PAUSE_TICKS;
                }
            }
            Phase::ReturnPause => {
                if done {
                    // iTween's speed sets the time: the path length over 10.
                    let dx = crate::fixed::diff8(self.move_to[0], senses.position[0]);
                    let dy = crate::fixed::diff8(self.move_to[1], senses.position[1]);
                    let length = crate::fixed::hypot(dx, dy);
                    let ticks = (length * 60).div_ceil((QUAKE_RETURN_SPEED >> 8) as u32);
                    self.begin_move(senses.position, ticks.clamp(1, u16::MAX as u32) as u16);
                    self.phase = Phase::QuakeReturn;
                }
            }
            Phase::Retreat | Phase::QuakeReturn => {
                let quake = self.phase == Phase::QuakeReturn;
                let total = self.move_ticks as i32;
                let gone = total - self.timer as i32;
                // Retreat stays the straight interpolation the port has always used;
                // Quake Return eases in and out on a sine, `easeInOutSine`.
                let (part, whole) = if quake {
                    let angle = (gone * 2048 / total) as u16;
                    (4096 - psx_math::sincos::cos_q12(angle), 8192)
                } else {
                    (gone, total)
                };
                let at = |from: i32, to: i32| {
                    // to - from fits i32 while positions stay inside +/-2^30.
                    from + psx_math::int32::mul_div_i32(to - from, part, whole)
                };
                out.push(Action::MoveTo([
                    at(self.move_from[0], self.move_to[0]),
                    at(self.move_from[1], self.move_to[1]),
                ]));
                if done {
                    let (next, ticks) = if quake {
                        (Phase::Reform, RETREAT_END_TICKS)
                    } else {
                        (Phase::RetreatEnd, RETREAT_END_TICKS)
                    };
                    self.enter(next, ticks, Clip::RetreatEnd, &mut out);
                }
            }
            Phase::Reform => {
                if done {
                    // Collider On, then Fly.
                    self.begin_fly(&mut out);
                }
            }
            Phase::ScreamAntic => {
                if fixed {
                    self.decelerate(DECELERATION, &mut out);
                }
                if done {
                    self.enter(Phase::Scream, SCREAM_TICKS, Clip::Scream, &mut out);
                }
            }
            Phase::Scream => {
                if done {
                    self.enter(Phase::ScreamRecover, SCREAM_END_TICKS, Clip::ScreamEnd, &mut out);
                }
            }
            Phase::RetreatStart => {
                if fixed {
                    self.decelerate(DECELERATION, &mut out);
                }
                if done {
                    self.phase = Phase::Retreat;
                    self.move_to = self.start;
                    self.begin_move(senses.position, RETREAT_TICKS);
                    self.velocity = [0; 2];
                    out.push(Action::Velocity([0; 2]));
                }
            }
            Phase::RetreatEnd => {
                if done {
                    // Retreat Reset returns to Idle with the Idle clip.
                    self.phase = Phase::Idle;
                    out.push(Action::Play(Clip::Idle, 0));
                }
            }
            Phase::DeathStart => {
                if done {
                    self.enter(Phase::Death, DEATH_TICKS, Clip::Death, &mut out);
                }
            }
            Phase::Death => {
                if done {
                    self.phase = Phase::Gone;
                }
            }
            Phase::Gone => {}
        }
        out
    }
    fn begin_move(&mut self, from: [i32; 2], ticks: u16) {
        self.move_from = from;
        self.move_ticks = ticks;
        self.timer = ticks;
    }
}
/// The Shadow Ball, flying on its own once Shoot R or Shoot L has made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowBall {
    pub position: [i32; 2],
    /// 1 for Shoot R; Shoot L also mirrors the ball (SetScale x -1).
    pub facing: i32,
    pub age: u16,
}
impl ShadowBall {
    pub fn new(position: [i32; 2], facing: i32) -> Self {
        Self {
            position,
            facing,
            age: 0,
        }
    }
    /// One 60 Hz step; false once Fade's Fireball End has played (DestroySelf).
    /// The trigger collider never meets terrain, so nothing else ends it.
    pub fn tick(&mut self) -> bool {
        self.position[0] += self.facing * (BALL_SPEED / 60);
        self.age += 1;
        self.age < BALL_TICKS + BALL_END_TICKS
    }
    /// Past `Shadow ball`'s Wait 1, in Fade.
    pub fn fading(&self) -> bool {
        self.age >= BALL_TICKS
    }
    /// A right-facing local box (min x, min y, max x, max y) on the ball, mirrored with it.
    pub fn bounds(&self, local: [i32; 4]) -> [i32; 4] {
        mirrored(self.position, self.facing, local)
    }
}
/// A box authored on a child of an object whose localScale x is `facing`
/// (1 keeps it, -1 mirrors it about the object's origin).
pub fn mirrored(at: [i32; 2], facing: i32, local: [i32; 4]) -> [i32; 4] {
    let (x0, x1) = if facing > 0 {
        (local[0], local[2])
    } else {
        (-local[2], -local[0])
    };
    [at[0] + x0, at[1] + local[1], at[0] + x1, at[1] + local[3]]
}
/// Vector2.ClampMagnitude: scale down only when longer than `max`.
fn clamp_magnitude(v: &mut [i32; 2], max: i32) {
    let (x, y) = (v[0] >> 8, v[1] >> 8);
    let limit = max >> 8;
    let square = crate::fixed::sum_sq(x, y);
    if square <= crate::fixed::square(limit) {
        return;
    }
    // The truncated >>8 magnitude underestimates, so bias the divisor up by one
    // to keep the scaled result at or under `max` when it is measured again.
    let length = square.isqrt().max(1) + 1;
    // limit and length come from i32 values shifted by 8, so both fit i32.
    v[0] = psx_math::int32::mul_div_i32(v[0], limit, length as i32);
    v[1] = psx_math::int32::mul_div_i32(v[1], limit, length as i32);
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;
    const START: [i32; 2] = [0, 0];
    fn senses(shade: [i32; 2], hero: [i32; 2], see: bool) -> Senses {
        Senses {
            position: shade,
            hero,
            can_see_hero: see,
            quake_range: false,
            scream_range: false,
            grounded: false,
        }
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
        let mut s = Shade::new(START, 1, Spells::default());
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
            let speed = crate::fixed::hypot(v[0] >> 8, v[1] >> 8);
            assert!(speed <= (CHASE_SPEED_MAX >> 8) as u32, "speed {speed}");
        }
        assert_eq!(s.phase(), Phase::Position);
        assert_eq!(s.facing(), 1);
    }
    #[test]
    fn position_only_slashes_in_range_and_level_with_the_hero() {
        let mut s = Shade::new(START, 7, Spells::default());
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
        let mut s = Shade::new(START, 3, Spells::default());
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
        run(
            &mut s,
            START,
            [-ONE, 0],
            true,
            (SLASH_BOX_TICKS + SLASH_CD_TICKS) as u32,
        );
        assert_eq!(s.phase(), Phase::Fly);
    }
    #[test]
    fn roaming_past_max_roam_retreats_to_the_spawn_and_returns_to_idle() {
        let mut s = Shade::new(START, 5, Spells::default());
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
        let mut s = Shade::new(START, 9, Spells::default());
        assert!(s.vulnerable());
        assert!(s
            .die()
            .iter()
            .any(|a| a == Action::Play(Clip::DeathStart, 0)));
        assert!(!s.vulnerable());
        assert_eq!(
            s.die().iter().count(),
            0,
            "a second death is not a second sequence"
        );
        run(&mut s, START, START, false, DEATH_START_TICKS as u32);
        assert_eq!(s.phase(), Phase::Death);
        run(&mut s, START, START, false, DEATH_TICKS as u32);
        assert_eq!(s.phase(), Phase::Gone);
        run(&mut s, START, START, false, 10);
        assert_eq!(s.phase(), Phase::Gone);
    }
    #[test]
    fn taking_damage_only_startles_from_idle() {
        let mut s = Shade::new(START, 2, Spells::default());
        assert_eq!(
            s.took_damage(senses(START, [ONE, 0], false)).iter().count(),
            1
        );
        assert_eq!(s.phase(), Phase::Startle);
        assert_eq!(
            s.took_damage(senses(START, [ONE, 0], false)).iter().count(),
            0
        );
    }
    fn ranged(shade: [i32; 2], hero: [i32; 2], quake: bool, scream: bool) -> Senses {
        Senses {
            quake_range: quake,
            scream_range: scream,
            ..senses(shade, hero, true)
        }
    }
    /// A Shade with `spells` that has just left Fly, by the first seed whose rolls
    /// take the branch `want` names; the hero stays at `hero`.
    fn first_seed_to(spells: Spells, s: Senses, want: Phase) -> Shade {
        for seed in 1..200 {
            let mut shade = Shade::new(START, seed, spells);
            shade.tick(s);
            run_senses(&mut shade, s, STARTLE_TICKS as u32);
            assert_eq!(shade.phase(), Phase::Fly);
            while shade.phase() == Phase::Fly {
                shade.tick(s);
            }
            if shade.phase() == want {
                return shade;
            }
        }
        panic!("no seed reached {want:?}");
    }
    fn run_senses(s: &mut Shade, at: Senses, ticks: u32) -> Vec<Action> {
        let mut all = Vec::new();
        for _ in 0..ticks {
            all.extend(s.tick(at).iter());
        }
        all
    }
    #[test]
    fn without_a_level_or_a_cast_the_attack_chain_is_the_slash_alone() {
        // Level 0 in every spell, or a cast count of 0, leaves Position the only way out of Fly.
        for spells in [
            Spells { mp: 5, ..Spells::default() },
            Spells { fireball: 2, quake: 1, scream: 1, mp: 0 },
        ] {
            for seed in 1..40 {
                let mut s = Shade::new(START, seed, spells);
                let at = ranged(START, [20 * ONE, 0], true, true);
                for _ in 0..2000 {
                    s.tick(at);
                    assert!(
                        matches!(s.phase(), Phase::Idle | Phase::Startle | Phase::Fly | Phase::Position),
                        "{:?} with {spells:?}",
                        s.phase()
                    );
                }
            }
        }
    }
    #[test]
    fn the_shadow_ball_cast_runs_its_timings_and_recoils_away_from_the_shot() {
        let spells = Spells { fireball: 1, mp: 1, ..Spells::default() };
        let hero = [12 * ONE, 0];
        let at = ranged(START, hero, false, false);
        let mut s = first_seed_to(spells, at, Phase::FireballPos);
        assert_eq!(s.spells().mp, 0, "Fireball Pos spends the cast");
        // Twelve away and level: ATTACK at once.
        s.tick(at);
        assert_eq!(s.phase(), Phase::CastAntic);
        run_senses(&mut s, at, CAST_ANTIC_TICKS as u32);
        assert_eq!(s.phase(), Phase::ChargeWait);
        let shots = run_senses(&mut s, at, CHARGE_WAIT_TICKS as u32);
        assert_eq!(s.phase(), Phase::Cast);
        assert!(shots.contains(&Action::Shoot(1)), "Shoot R toward the hero on the right");
        assert_eq!(s.velocity()[0], -CAST_RECOIL, "Right 2's X Speed is -10");
        run_senses(&mut s, at, CAST_TICKS as u32);
        assert_eq!(s.phase(), Phase::Cooldown);
        assert!(s.velocity()[0] > -CAST_RECOIL / 10, "Decel .9 per fixed step");
        run_senses(&mut s, at, COOLDOWN_TICKS as u32);
        assert_eq!(s.phase(), Phase::Fly);
        // The cast is spent: the next chain is the slash's.
        while s.phase() == Phase::Fly {
            s.tick(at);
        }
        assert_eq!(s.phase(), Phase::Position);
    }
    #[test]
    fn fireball_pos_casts_after_wait_3_out_of_range() {
        let spells = Spells { fireball: 1, mp: 1, ..Spells::default() };
        let at = ranged(START, [30 * ONE, 4 * ONE], false, false);
        let mut s = first_seed_to(spells, at, Phase::FireballPos);
        run_senses(&mut s, at, FIREBALL_POS_TIMEOUT as u32 - 1);
        assert_eq!(s.phase(), Phase::FireballPos);
        s.tick(at);
        assert_eq!(s.phase(), Phase::CastAntic);
    }
    #[test]
    fn the_quake_rises_dives_hits_on_landing_hides_and_returns_eased() {
        let spells = Spells { quake: 1, mp: 1, ..Spells::default() };
        let hero = [2 * ONE, -5 * ONE];
        let at = ranged(START, hero, true, false);
        let mut s = first_seed_to(spells, at, Phase::QuakeAntic);
        assert_eq!(s.spells().mp, 0);
        assert_eq!(s.velocity(), [0, QUAKE_RISE]);
        run_senses(&mut s, at, QUAKE_ANTIC_TICKS as u32);
        assert_eq!(s.phase(), Phase::QuakeStart);
        assert!(s.velocity()[1] > 0 && s.velocity()[1] < QUAKE_RISE / 100, "DecelerateXY .85");
        run_senses(&mut s, at, QUAKE_START_TICKS as u32);
        assert_eq!((s.phase(), s.velocity()[1]), (Phase::Quake, QUAKE_FALL));
        // The floor stops the dive before Wait 1 does.
        let low = [0, -4 * ONE];
        s.tick(Senses { grounded: true, ..ranged(low, hero, true, false) });
        assert_eq!(s.phase(), Phase::QuakeLand);
        assert_eq!(s.velocity(), [0; 2]);
        for _ in 0..SPELL_HIT_TICKS {
            assert!(s.quake_hitting() && !s.vulnerable());
            s.tick(ranged(low, hero, true, false));
        }
        assert_eq!(s.phase(), Phase::ReturnPause);
        assert!(!s.quake_hitting() && s.hidden() && !s.vulnerable());
        run_senses(&mut s, ranged(low, hero, true, false), RETURN_PAUSE_TICKS as u32);
        assert_eq!(s.phase(), Phase::QuakeReturn);
        // Four units at speed 10 is 0.4 s; easeInOutSine is half way at half time.
        let mut path = Vec::new();
        while s.phase() == Phase::QuakeReturn {
            for a in s.tick(ranged(low, hero, true, false)).iter() {
                if let Action::MoveTo(p) = a {
                    path.push(p);
                }
            }
        }
        assert_eq!(path.len(), 24);
        assert_eq!(*path.last().unwrap(), START);
        assert!((path[11][1] - -2 * ONE).abs() < ONE / 8, "{:?}", path[11]);
        assert!(path[0][1] - low[1] < ONE / 32, "eases in");
        assert_eq!(s.phase(), Phase::Reform);
        assert!(!s.hidden() && !s.vulnerable());
        run_senses(&mut s, at, RETREAT_END_TICKS as u32);
        assert_eq!(s.phase(), Phase::Fly);
        assert!(s.vulnerable());
    }
    #[test]
    fn a_quake_that_meets_no_floor_lands_after_wait_1() {
        let spells = Spells { quake: 1, mp: 1, ..Spells::default() };
        let at = ranged(START, [ONE, -5 * ONE], true, false);
        let mut s = first_seed_to(spells, at, Phase::QuakeAntic);
        run_senses(&mut s, at, (QUAKE_ANTIC_TICKS + QUAKE_START_TICKS + QUAKE_TICKS - 1) as u32);
        assert_eq!(s.phase(), Phase::Quake);
        s.tick(at);
        assert_eq!(s.phase(), Phase::QuakeLand);
    }
    #[test]
    fn the_scream_holds_its_hit_for_its_deactivate_then_recovers() {
        let spells = Spells { scream: 1, mp: 1, ..Spells::default() };
        let at = ranged(START, [ONE, 4 * ONE], false, true);
        let mut s = first_seed_to(spells, at, Phase::ScreamAntic);
        assert_eq!(s.spells().mp, 0);
        run_senses(&mut s, at, SCREAM_ANTIC_TICKS as u32 - 1);
        assert!(!s.scream_hitting());
        s.tick(at);
        assert_eq!(s.phase(), Phase::Scream);
        for _ in 0..SPELL_HIT_TICKS {
            assert!(s.scream_hitting() && s.vulnerable());
            s.tick(at);
        }
        assert!(!s.scream_hitting());
        run_senses(&mut s, at, (SCREAM_TICKS - SPELL_HIT_TICKS) as u32);
        assert_eq!(s.phase(), Phase::ScreamRecover);
        run_senses(&mut s, at, SCREAM_END_TICKS as u32);
        assert_eq!(s.phase(), Phase::Fly);
    }
    #[test]
    fn a_spell_out_of_its_range_trigger_is_never_cast() {
        // Quake? and Scream? need their range, and Q Other?'s will is only
        // acted on at Position's entry in range: out of range, only the slash.
        let spells = Spells { quake: 2, scream: 2, mp: 3, ..Spells::default() };
        for seed in 1..40 {
            let mut s = Shade::new(START, seed, spells);
            let at = ranged(START, [20 * ONE, 0], false, false);
            for _ in 0..1500 {
                s.tick(at);
                assert!(matches!(
                    s.phase(),
                    Phase::Idle | Phase::Startle | Phase::Fly | Phase::Position
                ));
            }
            assert_eq!(s.spells().mp, 3);
        }
    }
    #[test]
    fn the_shadow_ball_flies_at_20_for_its_wait_and_fade_and_mirrors() {
        let mut ball = ShadowBall::new([0, 0], -1);
        let mut ticks = 1;
        while ball.tick() {
            ticks += 1;
            assert_eq!(ball.fading(), ball.age >= BALL_TICKS);
        }
        assert_eq!(ticks, (BALL_TICKS + BALL_END_TICKS) as u32);
        // 70 ticks at 20 units a second, to within the per-tick rounding.
        assert!((ball.position[0] + 70 * 20 * ONE / 60).abs() < 70);
        let local = [-ONE, -ONE / 2, 2 * ONE, ONE / 2];
        assert_eq!(ShadowBall::new([0, 0], 1).bounds(local), local);
        assert_eq!(ball.bounds(local)[0], ball.position[0] - 2 * ONE);
        assert_eq!(ball.bounds(local)[2], ball.position[0] + ONE);
    }
}
