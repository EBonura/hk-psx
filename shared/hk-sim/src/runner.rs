//! Source-derived level37 Zombie Runner (Coward=false, Reverse=false) controller.
//!
//! Motion is Q16.16 units/second; one `step` is 1/60 s. The caller owns physics,
//! terrain Sweep/LOS/AlertRange queries, health, recoil, corpse and animation.
//! Deliver real animation completion using the returned token, and accepted
//! HealthManager events in source order: `took_damage`, then horizontal recoil
//! (if applicable). Apply recoil velocity AFTER those callbacks. Ignored/evaded
//! hits must not call `took_damage`. This is not a replacement for ActorHealth.
//!
//! Evidence: .hkpsx/crossroads68/RUNNER-CONTRACT.md and Walker/Swipe CIL. Unity
//! Update/FixedUpdate ordering and reversed Random.Range distribution still need
//! original-runtime validation. This deterministic 60 Hz controller makes that
//! integration boundary explicit rather than claiming trajectory parity. Source
//! StartMoving also invokes Walker.Update synchronously; this model defers that
//! additional sensing pass to the caller's scheduled Walker callback. Reference
//! integration must resolve that reentrant Update/elapsed-time behavior too.
use crate::ONE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    Idle,
    Walk,
    Turn,
    Anticipate,
    Lunge,
    Cooldown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    pub clip: Clip,
    pub serial: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walker {
    Waiting,
    Paused,
    Walking,
    Turning,
    StoppedForAttack,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swipe {
    Ready,
    Anticipate,
    Lunge,
    Cooldown,
    Idle,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnCause {
    Wall,
    Hero,
    Hole,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    Walking,
    Paused,
}
impl Wait {
    /// Authored Random.Range argument order, quantized to nominal 60 Hz ticks.
    pub const fn endpoints(self) -> [u16; 2] {
        match self {
            Self::Walking => [240, 90],
            Self::Paused => [150, 90],
        }
    }
    fn sample(self, choose: &mut impl FnMut(Wait) -> u16) -> u16 {
        let value = choose(self);
        let [hi, lo] = self.endpoints();
        assert!(
            value >= lo && value <= hi,
            "Runner wait outside source endpoints"
        );
        value
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play(Animation),
    /// None means preserve the current physics/recoil velocity on that axis.
    Velocity {
        x: Option<i32>,
        y: Option<i32>,
    },
    AudioStop,
    AudioPlay,
    /// Source first draws pitch .9..1.1, then overrides it with .85..1.15
    /// and selects one of two equal-weight samples. Audio/RNG owner executes it.
    ChaseSound,
    DustStart,
    DustStop,
    Turn(TurnCause),
}
/// Ordered commands, including writes superseded later in the same callback.
/// At most ten commands are emitted by one step (startup plus attack).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    commands: [Option<Action>; 12],
    len: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            commands: [None; 12],
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
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub camera_in_start_range: bool,
    pub hero_x: i32,
    pub actor_x: i32,
    pub in_alert_range: bool,
    pub can_see_hero: bool,
    pub wall: bool,
    pub floor_ahead: bool,
    pub completed: Option<Animation>,
}
impl Default for Senses {
    fn default() -> Self {
        Self {
            camera_in_start_range: false,
            hero_x: 0,
            actor_x: 0,
            in_alert_range: false,
            can_see_hero: false,
            wall: false,
            floor_ahead: true,
            completed: None,
        }
    }
}
/// Strict source 3D camera distance < 60. Early axis rejection bounds all
/// products, even for arbitrary i32 Q16 inputs (no overflow or square root).
pub fn camera_in_start_range(camera: [i32; 3], actor: [i32; 3]) -> bool {
    let limit = 60 * ONE as i64;
    let mut squared = 0i64;
    for axis in 0..3 {
        let delta = camera[axis] as i64 - actor[axis] as i64;
        if delta <= -limit || delta >= limit {
            return false;
        }
        squared += delta * delta;
    }
    squared < limit * limit
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runner {
    walker: Walker,
    swipe: Swipe,
    facing: i8,
    turning_facing: i8,
    wait: u16,
    turn_cooldown: u16,
    animation: Option<Animation>,
    serial: u32,
}
impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}
impl Runner {
    /// Both supported source instances start facing left.
    pub const fn new() -> Self {
        Self {
            walker: Walker::Waiting,
            swipe: Swipe::Ready,
            facing: -1,
            turning_facing: -1,
            wait: 0,
            turn_cooldown: 0,
            animation: None,
            serial: 0,
        }
    }
    pub fn walker(&self) -> Walker {
        self.walker
    }
    pub fn swipe(&self) -> Swipe {
        self.swipe
    }
    pub fn facing(&self) -> i32 {
        self.facing as i32
    }
    pub fn animation(&self) -> Option<Animation> {
        self.animation
    }
    pub fn turn_cooldown(&self) -> u16 {
        self.turn_cooldown
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
    fn vx(out: &mut Actions, x: i32) {
        out.push(Action::Velocity {
            x: Some(x),
            y: None,
        });
    }
    fn walk(&mut self, choose: &mut impl FnMut(Wait) -> u16, out: &mut Actions) {
        self.walker = Walker::Walking;
        self.play(Clip::Walk, out);
        self.wait = Wait::Walking.sample(choose);
        out.push(Action::AudioPlay);
        Self::vx(out, self.facing() * (ONE + ONE / 2));
    }
    fn pause(&mut self, choose: &mut impl FnMut(Wait) -> u16, out: &mut Actions) {
        self.walker = Walker::Paused;
        out.push(Action::AudioStop);
        self.play(Clip::Idle, out);
        Self::vx(out, 0);
        self.wait = Wait::Paused.sample(choose);
    }
    fn turn(&mut self, cause: TurnCause, out: &mut Actions) {
        self.walker = Walker::Turning;
        self.turning_facing = -self.facing;
        self.turn_cooldown = 60;
        Self::vx(out, 0);
        self.play(Clip::Turn, out);
        out.push(Action::Turn(cause));
    }
    fn attack(&mut self, hero_x: i32, actor_x: i32, out: &mut Actions) {
        self.walker = Walker::StoppedForAttack;
        out.push(Action::AudioStop);
        self.facing = if hero_x > actor_x { 1 } else { -1 };
        self.swipe = Swipe::Anticipate;
        out.push(Action::ChaseSound);
        out.push(Action::Velocity {
            x: Some(0),
            y: Some(0),
        });
        self.play(Clip::Anticipate, out);
    }
    fn reset(&mut self, choose: &mut impl FnMut(Wait) -> u16, out: &mut Actions) {
        self.swipe = Swipe::Ready;
        // StartMoving only begins walking from stopped/waiting states. A global
        // recoil while already walking/turning must not restart that clip.
        if matches!(
            self.walker,
            Walker::StoppedForAttack | Walker::Paused | Walker::Waiting
        ) {
            self.walk(choose, out);
        }
        self.turn_cooldown = 0;
        // Source Reset does not stop Charge Dust. Do not invent that event.
    }
    /// An accepted nonlethal HealthManager TOOK DAMAGE event. Ready attacks
    /// even without sight/range. Other Swipe states ignore this event.
    pub fn took_damage(&mut self, hero_x: i32, actor_x: i32) -> Actions {
        let mut out = Actions::new();
        if self.swipe == Swipe::Ready {
            self.attack(hero_x, actor_x, &mut out);
        }
        out
    }
    /// RECOIL HORIZONTAL is global; vertical recoil has no Swipe transition.
    /// Caller performs this after took_damage, before applying recoil velocity.
    pub fn horizontal_recoil(&mut self, mut choose: impl FnMut(Wait) -> u16) -> Actions {
        let mut out = Actions::new();
        if self.swipe != Swipe::Dead {
            self.reset(&mut choose, &mut out);
        }
        out
    }
    /// Terminal controller shutdown. Corpse/audio/death VFX remain caller-owned;
    /// no fabricated dust or audio cleanup is emitted as a source FSM action.
    pub fn die(&mut self) {
        self.walker = Walker::Dead;
        self.swipe = Swipe::Dead;
        self.animation = None;
        self.wait = 0;
    }
    /// Convenience guest schedule, Walker then Swipe. This ordering is a
    /// deterministic policy, not established original execution order. A
    /// reference-driven caller can invoke the two callbacks in observed order.
    pub fn step(&mut self, senses: Senses, mut choose: impl FnMut(Wait) -> u16) -> Actions {
        let mut out = self.step_walker(senses, &mut choose);
        for action in self.step_swipe(senses, &mut choose).iter() {
            out.push(action);
        }
        out
    }
    /// Walker Update only. Call once per 60 Hz tick, including while stopped,
    /// so turn cooldown advances. Independently callable from Swipe Update.
    pub fn step_walker(&mut self, senses: Senses, mut choose: impl FnMut(Wait) -> u16) -> Actions {
        let mut out = Actions::new();
        if self.walker == Walker::Dead {
            return out;
        }
        self.turn_cooldown = self.turn_cooldown.saturating_sub(1);
        let complete = self.animation.is_some() && senses.completed == self.animation;
        self.update_walker(senses, complete, &mut choose, &mut out);
        out
    }
    /// Swipe Update only, with the guest's FixedUpdate lunge X write. Call once
    /// per 60 Hz tick. Source FixedUpdate scheduling needs a reference trace;
    /// clip completion always comes from the caller's real animation clock.
    pub fn step_swipe(&mut self, senses: Senses, mut choose: impl FnMut(Wait) -> u16) -> Actions {
        let mut out = Actions::new();
        if self.swipe == Swipe::Dead {
            return out;
        }
        let complete = self.animation.is_some() && senses.completed == self.animation;
        match self.swipe {
            Swipe::Anticipate if complete => {
                self.swipe = Swipe::Lunge;
                out.push(Action::DustStart);
                self.play(Clip::Lunge, &mut out);
                Self::vx(&mut out, self.facing() * 6 * ONE);
            }
            Swipe::Lunge => {
                if complete {
                    self.swipe = Swipe::Cooldown;
                    out.push(Action::DustStop);
                    self.play(Clip::Cooldown, &mut out);
                    Self::vx(&mut out, 0);
                } else {
                    Self::vx(&mut out, self.facing() * 6 * ONE);
                }
            }
            Swipe::Cooldown if complete => {
                self.swipe = Swipe::Idle;
                self.play(Clip::Idle, &mut out);
                self.wait = 15;
            }
            Swipe::Idle => {
                self.wait = self.wait.saturating_sub(1);
                if self.wait == 0 {
                    self.reset(&mut choose, &mut out);
                }
            }
            Swipe::Ready => {
                // Source Ready is independent of Walker's startup camera gate.
                if senses.in_alert_range && senses.can_see_hero {
                    self.attack(senses.hero_x, senses.actor_x, &mut out);
                }
            }
            _ => {}
        }
        out
    }
    fn update_walker(
        &mut self,
        senses: Senses,
        complete: bool,
        choose: &mut impl FnMut(Wait) -> u16,
        out: &mut Actions,
    ) {
        match self.walker {
            Walker::Waiting if senses.camera_in_start_range => {
                // BeginStopped(0) then StartMoving: includes the otherwise unused
                // pause draw and AudioSource.Stop before AudioSource.Play.
                self.pause(choose, out);
                self.walk(choose, out);
            }
            Walker::Paused => {
                self.wait = self.wait.saturating_sub(1);
                if self.wait == 0 {
                    self.walk(choose, out);
                }
            }
            Walker::Walking => {
                if self.turn_cooldown == 0 {
                    let hero_behind = (senses.hero_x > senses.actor_x) != (self.facing > 0);
                    let cause = if senses.wall {
                        Some(TurnCause::Wall)
                    } else if hero_behind && senses.in_alert_range && senses.can_see_hero {
                        Some(TurnCause::Hero)
                    } else if !senses.floor_ahead {
                        Some(TurnCause::Hole)
                    } else {
                        None
                    };
                    if let Some(cause) = cause {
                        self.turn(cause, out);
                        return;
                    }
                }
                self.wait = self.wait.saturating_sub(1);
                if self.wait == 0 {
                    self.pause(choose, out);
                } else {
                    Self::vx(out, self.facing() * (ONE + ONE / 2));
                }
            }
            Walker::Turning => {
                Self::vx(out, 0);
                if complete {
                    self.facing = self.turning_facing;
                    self.walk(choose, out);
                }
            }
            _ => {}
        }
    }
}
