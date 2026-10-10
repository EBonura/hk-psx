//! Source-derived Crossroads_15 Zombie Shield (`ZombieShieldControl`) controller.
//!
//! Motion is Q16.16 units/second and one `tick` is 1/60 s. The caller owns
//! physics, terrain Sweep/LOS/AlertRange queries, health, recoil, corpse and
//! the animation clock; clip completion is delivered through `Senses::completed`
//! with the token the controller handed out, the way `crate::runner` does.
//!
//! Two source components are layered here, because the placement carries both
//! and they run at the same time:
//!
//! - `Walker`, the same component the Runner uses, but authored with
//!   `pauses = 0`. `Walker::BeginStopped` calls `EndStopping` straight away
//!   when `pauses` is false and `UpdateWalking` skips its walk-timer countdown,
//!   so this variant never idles: it walks until something turns it. The turn
//!   causes are `UpdateWalking`'s own order, wall then hero-behind then hole.
//! - `ZombieShieldControl`, which stops the Walker with `StopWalker`
//!   (`Walker::Stop(1)`, the silent stop that neither plays Idle nor resumes),
//!   raises a shield, and either counters when its own counter runs out or
//!   answers a blocked hit with the three-hit chain.
//!
//! The shield is `SetInvincible`. Which directions it covers is
//! `HealthManager::IsBlockingByDirection` read from the installed CIL: the
//! overhead shield sets `invincibleFromDirection` 0, which blocks every
//! cardinal, and the front shield sets 5 (facing left) or 6 (facing right),
//! which block the hero's own side and an upward slash but not a downward one.
//! So a pogo still lands on a front shield, which is what the source does.
//!
//! `Unshield Front`/`Unshield Top` carry no `SetInvincible`, so a Shield that
//! loses the hero keeps the shield's invincibility until the next lunge clears
//! it. That is reproduced rather than repaired: a source quirk the player can
//! see is not this port's to fix.
use crate::ONE;

/// FSM `Y Adjust`: the hero counts as high above this much of the body.
pub const Y_ADJUST: i32 = 78643; // 1.2
/// `Shield Start`'s `RandomInt(60, 100)` inclusive, as [hi, lo] ticks.
pub const SHIELD_TICKS: [u16; 2] = [100, 60];
/// `Block High`/`Block Low`: `Wait` 0.4 s before the three-hit chain.
pub const BUMP_TICKS: u16 = 24;
/// FSM `Lunge1 Speed`, the single counter-attack.
pub const LUNGE1_SPEED: i32 = 18 * ONE;
/// FSM `Lunge3 Speed`, each hit of the blocked-hit chain.
pub const LUNGE3_SPEED: i32 = 16 * ONE;
/// `Walker::walkSpeedR` on both placements (`walkSpeedL` is its negative).
pub const WALK_SPEED: i32 = 2 * ONE;
/// `Walker::BeginTurning` cooldown, in ticks.
pub const TURN_COOLDOWN: u16 = 60;
/// `Rigidbody2D::gravityScale` 1 times the installed Physics2D gravity.
pub const GRAVITY: i32 = 60 * ONE;

/// `invincibleFromDirection` codes this FSM writes.
const GUARD_NONE: u8 = 255;
const GUARD_ALL: u8 = 0;
const GUARD_LEFT: u8 = 5;
const GUARD_RIGHT: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    /// `Walker::walkClip`, the shared `ActorSpec::walk_clip`.
    Walk,
    /// `Walker::turnClip`, the shared `ActorSpec::turn_clip`.
    Turn,
    Idle,
    ShieldFront,
    ShieldTop,
    BumpFront,
    BumpTop,
    UnshieldFront,
    UnshieldTop,
    A1Antic,
    A1Lunge,
    A1Slash,
    A1Cooldown,
    A3Antic,
    A3Lunge1,
    A3Slash1,
    A3Cooldown1,
    A3Lunge2,
    A3Cooldown2,
    A3Lunge3,
    A3Slash3,
    A3Cooldown3,
}
impl Clip {
    /// Clips carried by `ActorController::ZombieShield`; Walk and Turn are the
    /// shared `ActorSpec` slots every controller already has.
    pub const COUNT: usize = 20;
    pub const fn slot(self) -> Option<usize> {
        match self {
            Self::Walk | Self::Turn => None,
            other => Some(other as usize - 2),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    pub clip: Clip,
    pub serial: u32,
}
/// Which shield is up, which is the pair of `Tk2dPlayAnimation` clips and the
/// `SetInvincible` code the four source Shield states choose between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raised {
    /// `Hero Is High`: the overhead shield rather than the front one.
    pub high: bool,
    /// `Hero Is Right`: the side `SetWalkerFacing` turns towards.
    pub right: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Walker::UpdateWaitingForConditions`, before the camera is near enough.
    Waiting,
    Walking,
    Turning,
    /// One of the four source Shield states.
    Shield(Raised),
    /// `Block High`/`Block Low`, the 0.4 s bump before the chain.
    Bump(Raised),
    Unshield(Raised),
    /// `Attack 1` (chain false) or `Attack 3` (chain true), by step.
    Attack {
        chain: bool,
        step: u8,
    },
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Animation),
    /// Horizontal velocity only: nothing in this FSM writes the vertical one.
    Velocity(i32),
    /// `StopWalker`/`StartWalker`, for a caller that presents the walk loop.
    AudioStop,
    AudioPlay,
}
/// Ordered commands of one callback. The longest is a shield that both runs
/// its counter out and starts the counter-attack on the same tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    commands: [Option<Action>; 8],
    len: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            commands: [None; 8],
            len: 0,
        }
    }
    fn push(&mut self, action: Action) {
        self.commands[self.len as usize] = Some(action);
        self.len += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.commands[..self.len as usize]
            .iter()
            .map(|a| a.unwrap())
    }
    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
/// Sampled once per tick with the actor's current facing and body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub camera_in_start_range: bool,
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// `Attack Range`, the `AlertRange` both `Walker` and the FSM read.
    pub in_attack_range: bool,
    pub can_see_hero: bool,
    pub wall: bool,
    pub floor_ahead: bool,
    pub completed: Option<Animation>,
}
impl Default for Senses {
    fn default() -> Self {
        Self {
            camera_in_start_range: false,
            position: [0; 2],
            hero: [0; 2],
            in_attack_range: false,
            can_see_hero: false,
            wall: false,
            floor_ahead: true,
            completed: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZombieShield {
    phase: Phase,
    facing: i8,
    turning_facing: i8,
    /// `HealthManager::invincibleFromDirection`, or `GUARD_NONE` while the
    /// HealthManager is not invincible at all.
    guard: u8,
    /// `Low Block Direction`, which both antics re-arm the guard with.
    low_block: u8,
    turn_cooldown: u16,
    shield_counter: u16,
    wait: u16,
    /// The signed `Lunge1 Speed`/`Lunge3 Speed` of the shield that was up.
    lunge1: i32,
    lunge3: i32,
    vx: i32,
    animation: Option<Animation>,
    serial: u32,
}
impl ZombieShield {
    /// `facing` is the placement's authored direction, which its transform
    /// mirror decides: `rightScale` is -1, so an unmirrored placement is left.
    pub const fn with_facing(facing: i8) -> Self {
        Self {
            phase: Phase::Waiting,
            facing,
            turning_facing: facing,
            guard: GUARD_NONE,
            low_block: if facing > 0 { GUARD_RIGHT } else { GUARD_LEFT },
            turn_cooldown: 0,
            shield_counter: 0,
            wait: 0,
            lunge1: 0,
            lunge3: 0,
            vx: 0,
            animation: None,
            serial: 0,
        }
    }
    pub const fn new() -> Self {
        Self::with_facing(-1)
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn facing(&self) -> i32 {
        self.facing as i32
    }
    pub fn velocity_x(&self) -> i32 {
        self.vx
    }
    pub fn animation(&self) -> Option<Animation> {
        self.animation
    }
    pub fn clip(&self) -> Clip {
        self.animation.map_or(Clip::Idle, |a| a.clip)
    }
    /// `HealthManager::IsBlockingByDirection` for a nail, with the cardinal the
    /// caller read off the swing (0 right, 1 up, 2 left, 3 down, which is
    /// `DirectionUtils::GetCardinalDirection`'s own numbering). A Spell is
    /// answered before this in the source and never reaches here.
    pub fn blocks(&self, cardinal: u8) -> bool {
        match self.guard {
            GUARD_ALL => true,
            GUARD_LEFT => cardinal == 0 || cardinal == 1,
            GUARD_RIGHT => cardinal == 1 || cardinal == 2,
            _ => false,
        }
    }
    /// Whether the HealthManager is invincible at all, whatever the direction.
    pub fn invincible(&self) -> bool {
        self.guard != GUARD_NONE
    }
    /// `HealthManager::Invincible`'s `BLOCKED HIT`. Only the four Shield states
    /// carry that transition; every other state ignores the event.
    pub fn blocked_hit(&mut self) -> Actions {
        let mut out = Actions::new();
        if let Phase::Shield(raised) = self.phase {
            self.phase = Phase::Bump(raised);
            self.wait = BUMP_TICKS;
            self.play(
                if raised.high {
                    Clip::BumpTop
                } else {
                    Clip::BumpFront
                },
                &mut out,
            );
        }
        out
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.animation = None;
        self.guard = GUARD_NONE;
        self.vx = 0;
    }
    fn play(&mut self, clip: Clip, out: &mut Actions) {
        self.serial = self.serial.wrapping_add(1);
        let token = Animation {
            clip,
            serial: self.serial,
        };
        self.animation = Some(token);
        out.push(Action::Play(token));
    }
    fn velocity(&mut self, x: i32, out: &mut Actions) {
        self.vx = x;
        out.push(Action::Velocity(x));
    }
    /// `Walker::BeginWalking`: the walk clip, the walk velocity, and the audio
    /// loop the stop silenced.
    fn begin_walking(&mut self, out: &mut Actions) {
        self.phase = Phase::Walking;
        self.play(Clip::Walk, out);
        out.push(Action::AudioPlay);
        self.velocity(self.facing as i32 * WALK_SPEED, out);
    }
    /// `Walker::BeginTurning`.
    fn begin_turning(&mut self, out: &mut Actions) {
        self.phase = Phase::Turning;
        self.turning_facing = -self.facing;
        self.turn_cooldown = TURN_COOLDOWN;
        self.velocity(0, out);
        self.play(Clip::Turn, out);
    }
    /// `Shield Start`, which stops the Walker and picks the first shield.
    fn shield_start(&mut self, senses: Senses, counter: u16, out: &mut Actions) {
        assert!(
            counter >= SHIELD_TICKS[1] && counter <= SHIELD_TICKS[0],
            "Zombie Shield counter outside RandomInt(60, 100)"
        );
        self.shield_counter = counter;
        out.push(Action::AudioStop);
        self.velocity(0, out);
        // `Shield Start` sets `Hero Is Right` only on greaterThan, so a hero at
        // exactly the actor's x takes the left branch the way the source does.
        let raised = Raised {
            high: senses.hero[1] > senses.position[1] + Y_ADJUST,
            right: senses.hero[0] > senses.position[0],
        };
        self.raise(raised, out);
    }
    /// One of the four Shield states, entered or re-entered.
    fn raise(&mut self, raised: Raised, out: &mut Actions) {
        self.phase = Phase::Shield(raised);
        self.low_block = if raised.right {
            GUARD_RIGHT
        } else {
            GUARD_LEFT
        };
        self.guard = if raised.high {
            GUARD_ALL
        } else {
            self.low_block
        };
        // `SetWalkerFacing` turns the Walker towards the hero; the transform
        // mirror follows, which is what `SetScale` writes.
        self.facing = if raised.right { 1 } else { -1 };
        self.turning_facing = self.facing;
        self.lunge1 = self.facing as i32 * LUNGE1_SPEED;
        self.lunge3 = self.facing as i32 * LUNGE3_SPEED;
        self.play(
            if raised.high {
                Clip::ShieldTop
            } else {
                Clip::ShieldFront
            },
            out,
        );
    }
    /// `Reset`: `StartWalker` is `Walker::StartMoving` then `ClearTurnCooldown`.
    fn reset(
        &mut self,
        senses: Senses,
        counter: &mut impl FnMut([u16; 2]) -> u16,
        out: &mut Actions,
    ) {
        self.begin_walking(out);
        self.turn_cooldown = 0;
        self.detect(senses, counter, out);
    }
    /// `Detect`, which runs beside the Walker on every tick it is not shielded.
    fn detect(
        &mut self,
        senses: Senses,
        counter: &mut impl FnMut([u16; 2]) -> u16,
        out: &mut Actions,
    ) {
        if senses.in_attack_range && senses.can_see_hero {
            let value = counter(SHIELD_TICKS);
            self.shield_start(senses, value, out);
        }
    }
    fn begin_attack(&mut self, chain: bool, out: &mut Actions) {
        // Both antics re-arm the guard from `Low Block Direction`, so an
        // overhead shield's all-directions cover narrows to the front one here.
        self.guard = self.low_block;
        self.phase = Phase::Attack { chain, step: 0 };
        self.play(if chain { Clip::A3Antic } else { Clip::A1Antic }, out);
    }
    /// One 60 Hz tick. `counter` samples `Shield Start`'s `RandomInt(60, 100)`
    /// inclusive from the caller's own deterministic generator.
    pub fn tick(&mut self, senses: Senses, mut counter: impl FnMut([u16; 2]) -> u16) -> Actions {
        let mut out = Actions::new();
        if self.phase == Phase::Dead {
            return out;
        }
        // `Walker::Update` counts the turn cooldown down whatever its state is.
        self.turn_cooldown = self.turn_cooldown.saturating_sub(1);
        let complete = self.animation.is_some() && senses.completed == self.animation;
        match self.phase {
            // `Walker` and `ZombieShieldControl` are two components of one
            // object, so the Walker's own update and the FSM's `Detect` both
            // run on a tick where the walker is live. A hero that turns the
            // Walker and trips `Detect` on the same tick does both: the turn
            // begins and `StopWalker` then stops it where it stands.
            Phase::Waiting => {
                // `UpdateWaitingForConditions`: `BeginStopped(0)` then
                // `StartMoving`, and with `pauses` false the stop ends at once.
                if senses.camera_in_start_range {
                    self.begin_walking(&mut out);
                }
                self.detect(senses, &mut counter, &mut out);
            }
            Phase::Walking => {
                self.update_walking(senses, &mut out);
                self.detect(senses, &mut counter, &mut out);
            }
            Phase::Turning => {
                self.velocity(0, &mut out);
                if complete {
                    self.facing = self.turning_facing;
                    self.begin_walking(&mut out);
                }
                self.detect(senses, &mut counter, &mut out);
            }
            Phase::Shield(raised) => self.update_shield(senses, raised, &mut out),
            Phase::Bump(raised) => {
                self.wait = self.wait.saturating_sub(1);
                if self.wait == 0 {
                    let _ = raised;
                    self.begin_attack(true, &mut out);
                }
            }
            Phase::Unshield(_) => {
                if complete {
                    self.reset(senses, &mut counter, &mut out);
                }
            }
            Phase::Attack { chain, step } => {
                if complete {
                    self.advance_attack(senses, chain, step, &mut counter, &mut out);
                }
            }
            Phase::Dead => {}
        }
        out
    }
    /// `Walker::UpdateWalking` in its own order: wall, then the hero behind a
    /// seen-and-in-range walker, then a missing floor. `pauses` is false on
    /// this placement, so the walk timer that would stop it never runs.
    fn update_walking(&mut self, senses: Senses, out: &mut Actions) {
        if self.turn_cooldown == 0 {
            let hero_behind = (senses.hero[0] > senses.position[0]) != (self.facing > 0);
            let turn = senses.wall
                || (hero_behind && senses.can_see_hero && senses.in_attack_range)
                || !senses.floor_ahead;
            if turn {
                self.begin_turning(out);
                return;
            }
        }
        self.velocity(self.facing as i32 * WALK_SPEED, out);
    }
    /// One tick inside a Shield state, in the source action order: re-aim,
    /// count down, then leave when the hero is gone.
    fn update_shield(&mut self, senses: Senses, raised: Raised, out: &mut Actions) {
        // The four Shield states set `Hero Is Left` on equal as well as on
        // lessThan, so a hero exactly level counts as left and as low.
        let wanted = Raised {
            high: senses.hero[1] > senses.position[1] + Y_ADJUST,
            right: senses.hero[0] > senses.position[0],
        };
        if wanted != raised {
            self.raise(wanted, out);
        }
        self.shield_counter = self.shield_counter.saturating_sub(1);
        if self.shield_counter == 0 {
            self.begin_attack(false, out);
            return;
        }
        if !(senses.in_attack_range && senses.can_see_hero) {
            let current = if let Phase::Shield(r) = self.phase {
                r
            } else {
                raised
            };
            self.phase = Phase::Unshield(current);
            self.play(
                if current.high {
                    Clip::UnshieldTop
                } else {
                    Clip::UnshieldFront
                },
                out,
            );
        }
    }
    /// The two attack chains, one completed clip at a time. Both drop the
    /// shield on their first lunge and neither raises it again.
    fn advance_attack(
        &mut self,
        senses: Senses,
        chain: bool,
        step: u8,
        counter: &mut impl FnMut([u16; 2]) -> u16,
        out: &mut Actions,
    ) {
        let next = step + 1;
        let last = if chain { 8 } else { 3 };
        if step == last {
            self.reset(senses, counter, out);
            return;
        }
        self.phase = Phase::Attack { chain, step: next };
        if chain {
            match next {
                1 => {
                    self.guard = GUARD_NONE;
                    self.velocity(self.lunge3, out);
                    self.play(Clip::A3Lunge1, out);
                }
                2 => {
                    self.play(Clip::A3Slash1, out);
                    self.velocity(0, out);
                }
                3 => self.play(Clip::A3Cooldown1, out),
                4 => {
                    self.velocity(self.lunge3, out);
                    self.play(Clip::A3Lunge2, out);
                }
                5 => {
                    self.velocity(0, out);
                    self.play(Clip::A3Cooldown2, out);
                }
                6 => {
                    self.velocity(self.lunge3, out);
                    self.play(Clip::A3Lunge3, out);
                }
                7 => {
                    self.velocity(0, out);
                    self.play(Clip::A3Slash3, out);
                }
                _ => {
                    self.velocity(0, out);
                    self.play(Clip::A3Cooldown3, out);
                }
            }
        } else {
            match next {
                1 => {
                    self.guard = GUARD_NONE;
                    self.velocity(self.lunge1, out);
                    self.play(Clip::A1Lunge, out);
                }
                2 => {
                    self.velocity(0, out);
                    self.play(Clip::A1Slash, out);
                }
                _ => self.play(Clip::A1Cooldown, out),
            }
        }
    }
}
impl Default for ZombieShield {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(_: [u16; 2]) -> u16 {
        60
    }
    fn walking() -> (ZombieShield, Senses) {
        let mut shield = ZombieShield::new();
        let senses = Senses {
            camera_in_start_range: true,
            ..Senses::default()
        };
        shield.tick(senses, counter);
        assert_eq!(shield.phase(), Phase::Walking);
        (shield, senses)
    }
    /// Run until the current clip completes, which the caller's animation clock
    /// would report; the controller never invents a completion of its own.
    fn complete(shield: &mut ZombieShield, senses: Senses) -> Actions {
        let token = shield.animation().expect("a clip to complete");
        shield.tick(
            Senses {
                completed: Some(token),
                ..senses
            },
            counter,
        )
    }

    #[test]
    fn waits_for_the_camera_then_walks_without_pausing() {
        let mut shield = ZombieShield::new();
        assert_eq!(shield.tick(Senses::default(), counter).len(), 0);
        assert_eq!(shield.phase(), Phase::Waiting);
        let senses = Senses {
            camera_in_start_range: true,
            ..Senses::default()
        };
        shield.tick(senses, counter);
        assert_eq!(shield.phase(), Phase::Walking);
        assert_eq!(shield.velocity_x(), -WALK_SPEED);
        // `pauses` is 0, so no number of ticks turns this into an idle.
        for _ in 0..600 {
            shield.tick(senses, counter);
            assert_eq!(shield.phase(), Phase::Walking);
        }
    }

    #[test]
    fn a_wall_turns_it_and_the_turn_clip_gates_the_new_facing() {
        let (mut shield, senses) = walking();
        let blocked = Senses {
            wall: true,
            ..senses
        };
        shield.tick(blocked, counter);
        assert_eq!(shield.phase(), Phase::Turning);
        assert_eq!(shield.facing(), -1, "the facing waits for the turn clip");
        assert_eq!(shield.velocity_x(), 0);
        complete(&mut shield, blocked);
        assert_eq!(shield.facing(), 1);
        assert_eq!(shield.phase(), Phase::Walking);
    }

    #[test]
    fn the_shield_faces_the_hero_and_re_aims_while_it_is_up() {
        let (mut shield, base) = walking();
        let seen = Senses {
            in_attack_range: true,
            can_see_hero: true,
            hero: [3 * ONE, 0],
            position: [0; 2],
            ..base
        };
        shield.tick(seen, counter);
        assert_eq!(
            shield.phase(),
            Phase::Shield(Raised {
                high: false,
                right: true
            })
        );
        assert_eq!(shield.facing(), 1);
        assert_eq!(shield.velocity_x(), 0);
        assert!(shield.invincible());
        // Same side, now overhead: the source swaps to the top shield.
        let above = Senses {
            hero: [3 * ONE, 4 * ONE],
            ..seen
        };
        shield.tick(above, counter);
        assert_eq!(
            shield.phase(),
            Phase::Shield(Raised {
                high: true,
                right: true
            })
        );
        // And across to the far side.
        let across = Senses {
            hero: [-3 * ONE, 0],
            ..seen
        };
        shield.tick(across, counter);
        assert_eq!(
            shield.phase(),
            Phase::Shield(Raised {
                high: false,
                right: false
            })
        );
        assert_eq!(shield.facing(), -1);
    }

    #[test]
    fn the_front_shield_blocks_the_hero_side_and_an_up_slash_but_not_a_pogo() {
        let (mut shield, base) = walking();
        let seen = Senses {
            in_attack_range: true,
            can_see_hero: true,
            hero: [3 * ONE, 0],
            position: [0; 2],
            ..base
        };
        shield.tick(seen, counter);
        // Hero on the right: `invincibleFromDirection` 6 blocks a swing
        // travelling left (cardinal 2) and an up slash (1), not a down one (3).
        assert!(shield.blocks(2));
        assert!(shield.blocks(1));
        assert!(!shield.blocks(3));
        assert!(!shield.blocks(0));
        let above = Senses {
            hero: [3 * ONE, 4 * ONE],
            ..seen
        };
        shield.tick(above, counter);
        for cardinal in 0..4 {
            assert!(
                shield.blocks(cardinal),
                "the overhead shield blocks every direction"
            );
        }
    }

    #[test]
    fn a_blocked_hit_bumps_then_runs_the_three_hit_chain() {
        let (mut shield, base) = walking();
        let seen = Senses {
            in_attack_range: true,
            can_see_hero: true,
            hero: [3 * ONE, 0],
            position: [0; 2],
            ..base
        };
        shield.tick(seen, counter);
        shield.blocked_hit();
        assert!(matches!(shield.phase(), Phase::Bump(_)));
        for _ in 0..BUMP_TICKS - 1 {
            shield.tick(seen, counter);
            assert!(matches!(shield.phase(), Phase::Bump(_)));
        }
        shield.tick(seen, counter);
        assert_eq!(
            shield.phase(),
            Phase::Attack {
                chain: true,
                step: 0
            }
        );
        assert!(shield.invincible(), "the antic keeps the guard up");
        // Three lunges at `Lunge3 Speed`, each followed by a stop.
        let mut lunges = 0;
        for _ in 0..8 {
            complete(&mut shield, seen);
            if shield.velocity_x() == LUNGE3_SPEED {
                lunges += 1;
            }
        }
        assert_eq!(lunges, 3);
        assert!(!shield.invincible(), "the first lunge drops the shield");
        complete(&mut shield, seen);
        assert_eq!(
            shield.phase(),
            Phase::Shield(Raised {
                high: false,
                right: true
            }),
            "Reset walks, and Detect shields again with the hero still there"
        );
    }

    #[test]
    fn the_counter_runs_out_into_the_single_attack() {
        let (mut shield, base) = walking();
        let seen = Senses {
            in_attack_range: true,
            can_see_hero: true,
            hero: [3 * ONE, 0],
            position: [0; 2],
            ..base
        };
        shield.tick(seen, counter);
        // `Shield Start` seeds the counter; every later tick of a Shield state
        // takes one off it, and reaching zero is COUNTER END.
        for _ in 0..SHIELD_TICKS[1] - 1 {
            shield.tick(seen, counter);
            assert!(matches!(shield.phase(), Phase::Shield(_)));
        }
        shield.tick(seen, counter);
        assert_eq!(
            shield.phase(),
            Phase::Attack {
                chain: false,
                step: 0
            }
        );
        complete(&mut shield, seen);
        assert_eq!(shield.velocity_x(), LUNGE1_SPEED);
        assert!(!shield.invincible());
    }

    #[test]
    fn losing_the_hero_unshields_but_keeps_the_source_invincibility() {
        let (mut shield, base) = walking();
        let seen = Senses {
            in_attack_range: true,
            can_see_hero: true,
            hero: [3 * ONE, 0],
            position: [0; 2],
            ..base
        };
        shield.tick(seen, counter);
        let gone = Senses {
            in_attack_range: false,
            ..seen
        };
        shield.tick(gone, counter);
        assert!(matches!(shield.phase(), Phase::Unshield(_)));
        complete(&mut shield, gone);
        assert_eq!(shield.phase(), Phase::Walking);
        // Neither Unshield state carries SetInvincible, so it walks off guarded.
        assert!(shield.invincible());
        assert!(shield.blocks(2));
    }

    #[test]
    fn death_stops_every_write() {
        let (mut shield, senses) = walking();
        shield.die();
        assert_eq!(shield.phase(), Phase::Dead);
        assert!(!shield.invincible());
        assert!(shield.tick(senses, counter).is_empty());
    }
}
