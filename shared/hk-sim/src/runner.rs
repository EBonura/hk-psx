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
//! StartMoving invokes an additional Walker.Update synchronously, including its
//! delta-time countdown. Reset then clears turn cooldown and enters Ready with
//! an immediate alert check. These callbacks do not advance the physics clock.
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
    /// Authored level37 Runner Random.Range argument order, quantized to
    /// nominal 60 Hz ticks (`Params::RUNNER`).
    pub const fn endpoints(self) -> [u16; 2] {
        match self {
            Self::Walking => [240, 90],
            Self::Paused => [150, 90],
        }
    }
}
/// The attack FSM layered on the shared Walker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attack {
    /// Zombie Swipe: Anticipate clip, Lunge clip at `lunge_speed`, Cooldown clip, 15-tick Idle.
    Swipe,
    /// Zombie Leap (Leaper): the Attack clip's trigger frame launches at
    /// ((hero x - self x) * factor, jump_speed_y); the airborne clip keeps
    /// playing; bottom contact plays Land; Idle waits, then StartWalker.
    Leap { trigger_ticks: u16, jump_speed_y: i32, jump_x_factor: i32, idle_ticks: u16 },
}
/// Per-variant Walker fields and the FSM `Lunge Speed`: the Zombie Swipe FSM
/// is otherwise identical across Runner, Barger and Hornhead placements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Walker walkSpeedR in Q16 units/s (walkSpeedL is its negative).
    pub walk_speed: i32,
    /// FSM `Lunge Speed` in Q16 units/s.
    pub lunge_speed: i32,
    /// pauseWaitMin/Max in ticks, as authored (hi, lo after sorting).
    pub walking_wait: [u16; 2],
    /// pauseTimeMin/Max in ticks (hi, lo).
    pub paused_wait: [u16; 2],
    pub attack: Attack,
    /// Rigidbody2D gravityScale * 60 in Q16 units/s^2 (60 Runner, 48 Leaper); the caller's body uses it.
    pub gravity: i32,
}
impl Params {
    pub const RUNNER: Self = Self { walk_speed: ONE + ONE / 2, lunge_speed: 6 * ONE, walking_wait: [240, 90], paused_wait: [150, 90], attack: Attack::Swipe, gravity: 60 * ONE };
    /// The level57 Leaper: trigger frame 3 of the 12 fps Attack clip, jump
    /// (1.25 * dx, 20), Idle Time .5 s, gravity scale .8.
    pub const LEAPER: Self = Self { walk_speed: 2 * ONE + ONE / 4, lunge_speed: 0, walking_wait: [240, 90], paused_wait: [150, 90],
        attack: Attack::Leap { trigger_ticks: 15, jump_speed_y: 20 * ONE, jump_x_factor: ONE + ONE / 4, idle_ticks: 30 }, gravity: 48 * ONE };
    const fn endpoints(self, wait: Wait) -> [u16; 2] {
        match wait {
            Wait::Walking => self.walking_wait,
            Wait::Paused => self.paused_wait,
        }
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
/// At most thirteen commands are emitted by one step (startup, turn, attack).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    commands: [Option<Action>; 16],
    len: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            commands: [None; 16],
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
/// Sample at each public callback, using the current actor facing/body. The
/// synchronous StartMoving pass uses this same world snapshot: no physics step
/// occurs between its writes and Ready OnEnter. LOS/range are detector values,
/// not a request to advance their independent Unity component callbacks.
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
    /// Bottom contact this frame (CheckCollisionSide LAND for the Leaper).
    pub grounded: bool,
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
            grounded: true,
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
    params: Params,
    /// Leap: chosen jump x velocity; Swipe: unused.
    jump_x: i32,
}
impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}
impl Runner {
    /// The level37 Runner variant, which starts facing left like most.
    pub const fn new() -> Self {
        Self::with_params(Params::RUNNER)
    }
    /// Facing left, which is how every placement was authored until
    /// Crossroads_37's Leaper, whose transform carries a plain x mirror.
    pub const fn with_params(params: Params) -> Self {
        Self::with_facing(params, -1)
    }
    /// `facing` is the placement's authored direction: the mirror in its
    /// transform decides it, not the controller.
    pub const fn with_facing(params: Params, facing: i8) -> Self {
        Self {
            params,
            walker: Walker::Waiting,
            swipe: Swipe::Ready,
            facing,
            turning_facing: facing,
            wait: 0,
            turn_cooldown: 0,
            animation: None,
            serial: 0,
            jump_x: 0,
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
    fn sample(&self, wait: Wait, choose: &mut impl FnMut(Wait, [u16; 2]) -> u16) -> u16 {
        let [hi, lo] = self.params.endpoints(wait);
        let value = choose(wait, [hi, lo]);
        assert!(value >= lo && value <= hi, "Runner wait outside source endpoints");
        value
    }
    fn walk(&mut self, choose: &mut impl FnMut(Wait, [u16; 2]) -> u16, out: &mut Actions) {
        self.walker = Walker::Walking;
        self.play(Clip::Walk, out);
        self.wait = self.sample(Wait::Walking, choose);
        out.push(Action::AudioPlay);
        Self::vx(out, self.facing() * self.params.walk_speed);
    }
    fn pause(&mut self, choose: &mut impl FnMut(Wait, [u16; 2]) -> u16, out: &mut Actions) {
        self.walker = Walker::Paused;
        out.push(Action::AudioStop);
        self.play(Clip::Idle, out);
        Self::vx(out, 0);
        self.wait = self.sample(Wait::Paused, choose);
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
        if let Attack::Leap { jump_x_factor, trigger_ticks, .. } = self.params.attack {
            // Left or Right?: Jump X Speed = (Hero X - Self X) * 1.25; Anticipate
            // stops, plays Attack (one random-pitch sample) and waits for its
            // trigger frame.
            self.jump_x = ((hero_x as i64 - actor_x as i64) * jump_x_factor as i64 >> 16) as i32;
            self.wait = trigger_ticks;
        }
        out.push(Action::ChaseSound);
        out.push(Action::Velocity {
            x: Some(0),
            y: Some(0),
        });
        self.play(Clip::Anticipate, out);
    }
    fn reset(&mut self, senses: Senses, choose: &mut impl FnMut(Wait, [u16; 2]) -> u16, out: &mut Actions) {
        // StartMoving only begins walking from stopped/waiting states. A global
        // recoil while already walking/turning must not restart that clip.
        if matches!(
            self.walker,
            Walker::StoppedForAttack | Walker::Paused | Walker::Waiting
        ) {
            self.walk(choose, out);
        }
        // StartMoving IL004c calls Update even when already walking/turning.
        // StartWalker.Apply IL002e clears cooldown only AFTER this callback.
        self.walker_callback(senses, choose, out);
        self.turn_cooldown = 0;
        self.swipe = Swipe::Ready;
        self.check_ready(senses, out);
        // Source Reset does not stop Charge Dust. Do not invent that event.
    }
    /// An accepted nonlethal HealthManager TOOK DAMAGE event. Ready attacks
    /// even without sight/range. Other Swipe states ignore this event; the
    /// Leap FSM has no TOOK DAMAGE transition.
    pub fn took_damage(&mut self, hero_x: i32, actor_x: i32) -> Actions {
        let mut out = Actions::new();
        if self.swipe == Swipe::Ready && self.params.attack == Attack::Swipe {
            self.attack(hero_x, actor_x, &mut out);
        }
        out
    }
    /// RECOIL HORIZONTAL is global; vertical recoil has no Swipe transition.
    /// Caller performs this after took_damage, before applying recoil velocity.
    pub fn horizontal_recoil(
        &mut self,
        senses: Senses,
        mut choose: impl FnMut(Wait, [u16; 2]) -> u16,
    ) -> Actions {
        let mut out = Actions::new();
        // The Leap FSM has no RECOIL HORIZONTAL transition.
        if self.swipe != Swipe::Dead && self.params.attack == Attack::Swipe {
            self.reset(senses, &mut choose, &mut out);
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
    pub fn step(&mut self, senses: Senses, mut choose: impl FnMut(Wait, [u16; 2]) -> u16) -> Actions {
        let mut out = self.step_walker(senses, &mut choose);
        for action in self.step_swipe(senses, &mut choose).iter() {
            out.push(action);
        }
        out
    }
    /// Walker Update only. Call once per 60 Hz tick, including while stopped,
    /// so turn cooldown advances. Independently callable from Swipe Update.
    pub fn step_walker(&mut self, senses: Senses, mut choose: impl FnMut(Wait, [u16; 2]) -> u16) -> Actions {
        let mut out = Actions::new();
        if self.walker == Walker::Dead {
            return out;
        }
        self.walker_callback(senses, &mut choose, &mut out);
        out
    }
    /// Swipe Update only, with the guest's FixedUpdate lunge X write. Call once
    /// per 60 Hz tick. Source FixedUpdate scheduling needs a reference trace;
    /// clip completion always comes from the caller's real animation clock.
    pub fn step_swipe(&mut self, senses: Senses, mut choose: impl FnMut(Wait, [u16; 2]) -> u16) -> Actions {
        let mut out = Actions::new();
        if self.swipe == Swipe::Dead {
            return out;
        }
        let complete = self.animation.is_some() && senses.completed == self.animation;
        if let Attack::Leap { jump_speed_y, idle_ticks, .. } = self.params.attack {
            match self.swipe {
                Swipe::Anticipate => {
                    self.wait = self.wait.saturating_sub(1);
                    if self.wait == 0 {
                        // Launch, then Lunge until bottom contact; the Attack clip keeps playing.
                        self.swipe = Swipe::Lunge;
                        out.push(Action::Velocity { x: Some(self.jump_x), y: Some(jump_speed_y) });
                    }
                }
                Swipe::Lunge if senses.grounded => {
                    self.swipe = Swipe::Cooldown;
                    self.play(Clip::Cooldown, &mut out);
                    Self::vx(&mut out, 0);
                }
                Swipe::Cooldown if complete => {
                    self.swipe = Swipe::Idle;
                    self.play(Clip::Idle, &mut out);
                    self.wait = idle_ticks;
                }
                Swipe::Idle => {
                    self.wait = self.wait.saturating_sub(1);
                    if self.wait == 0 {
                        // Reset: Idle clip and StartWalker.
                        self.play(Clip::Idle, &mut out);
                        if matches!(self.walker, Walker::StoppedForAttack | Walker::Paused | Walker::Waiting) {
                            self.walk(&mut choose, &mut out);
                        }
                        self.turn_cooldown = 0;
                        self.swipe = Swipe::Ready;
                        self.check_ready(senses, &mut out);
                    }
                }
                Swipe::Ready => self.check_ready(senses, &mut out),
                _ => {}
            }
            return out;
        }
        match self.swipe {
            Swipe::Anticipate if complete => {
                self.swipe = Swipe::Lunge;
                out.push(Action::DustStart);
                self.play(Clip::Lunge, &mut out);
                Self::vx(&mut out, self.facing() * self.params.lunge_speed);
            }
            Swipe::Lunge => {
                if complete {
                    self.swipe = Swipe::Cooldown;
                    out.push(Action::DustStop);
                    self.play(Clip::Cooldown, &mut out);
                    Self::vx(&mut out, 0);
                } else {
                    Self::vx(&mut out, self.facing() * self.params.lunge_speed);
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
                    self.reset(senses, &mut choose, &mut out);
                }
            }
            Swipe::Ready => {
                self.check_ready(senses, &mut out);
            }
            _ => {}
        }
        out
    }
    fn check_ready(&mut self, senses: Senses, out: &mut Actions) {
        // GetCanSeeHero and CheckAlertRange both Apply on OnEnter, not only
        // OnUpdate. Coward=false Reset therefore need not spend a tick in Ready.
        if senses.in_alert_range && senses.can_see_hero {
            self.attack(senses.hero_x, senses.actor_x, out);
        }
    }
    fn walker_callback(
        &mut self,
        senses: Senses,
        choose: &mut impl FnMut(Wait, [u16; 2]) -> u16,
        out: &mut Actions,
    ) {
        self.turn_cooldown = self.turn_cooldown.saturating_sub(1);
        let complete = self.animation.is_some() && senses.completed == self.animation;
        self.update_walker(senses, complete, choose, out);
    }
    fn update_walker(
        &mut self,
        senses: Senses,
        complete: bool,
        choose: &mut impl FnMut(Wait, [u16; 2]) -> u16,
        out: &mut Actions,
    ) {
        match self.walker {
            Walker::Waiting if senses.camera_in_start_range => {
                // BeginStopped(0) then StartMoving: includes the otherwise unused
                // pause draw and AudioSource.Stop before AudioSource.Play.
                self.pause(choose, out);
                self.walk(choose, out);
                // The single nested call is bounded: state is now Walking, so
                // it cannot take the startup branch again or recurse further.
                self.walker_callback(senses, choose, out);
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
                    Self::vx(out, self.facing() * self.params.walk_speed);
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
