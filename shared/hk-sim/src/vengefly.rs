//! Source-derived Vengefly (Buzzer): the original `chaser` and `flyer_receive_direction_msg` FSMs on the
//! shared HKFSM03 kernel. The caller owns physics (a gravity-free dynamic body against terrain), the sight
//! and alert-range queries, Recoil, HealthManager hits, the corpse and Geo.
//!
//! `tick` is one nominal 60 Hz frame; the 50 Hz FixedUpdate steps run from an accumulator, ahead of the
//! update pass as Unity orders them, so the per-step constants stay as authored. Random.Range is a
//! deterministic per-actor sequence.
use crate::fsm3::{Env, Tree, DIRTY_FACING, DIRTY_VEL, PH_FIXED, PH_UPDATE, SOUND_STARTLE};
use crate::ONE;
use hk_format::fsm3::Bank;
include!("../../../data/vengefly_fsm3.rs");

pub const CHASE_SPEED_MAX: i32 = 5 * ONE;
pub const IDLE_SPEED_MAX: i32 = 114688; // 1.75
pub const STARTLE_TICKS: u16 = 20; // four frames at 12 fps
pub const STOP_TICKS: u16 = 60;
pub const ATTENTION_TICKS: u16 = 600;
/// Tk2dPlayFrame 3 at 12 fps, in 60 Hz animation ticks.
pub const CHASE_START_FRAME_TICKS: u32 = 15;
/// Alert Range New: a 7.804-unit circle.
const ALERT_RADIUS: i32 = 511463;

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
    values: [Option<Action>; 7],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self { values: [None; 7], count: 0 }
    }
    fn push(&mut self, action: Action) {
        if (self.count as usize) < self.values.len() {
            self.values[self.count as usize] = Some(action);
            self.count += 1;
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize].iter().map(|a| a.unwrap())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// LineOfSightDetector.canSeeHero: alert range AND unobstructed raycast.
    pub can_see_hero: bool,
}

#[derive(Clone, Copy)]
pub struct Vengefly {
    mem: [u32; RUNTIME_BYTES / 4],
    vel: [i32; 2],
    anim_left: u16,
    facing: i8,
    fixed_accumulator: u8,
}
// The widest controller the Vengefly's actor runtime can carry (its slot is the False Knight's, 88 bytes).
const _: () = assert!(core::mem::size_of::<Vengefly>() <= 80);

fn bank() -> Bank<'static> {
    // SAFETY: the cooker parses the bank it emits and the module transport hash-checks it.
    unsafe { Bank::from_verified(&BANK) }
}
fn clip_of(id: u32) -> Clip {
    if id == CLIP_STARTLE {
        Clip::Startle
    } else if id == CLIP_CHASE {
        Clip::Chase
    } else if id == CLIP_TURN_TO_IDLE {
        Clip::TurnToIdle
    } else if id == CLIP_TURN_TO_FLY {
        Clip::TurnToFly
    } else {
        Clip::Idle
    }
}

impl Vengefly {
    #[inline(never)]
    pub fn new(position: [i32; 2], seed: u32) -> Self {
        let mut me =
            Self { mem: [0; RUNTIME_BYTES / 4], vel: [0; 2], anim_left: 0xffff, facing: -1, fixed_accumulator: 0 };
        let mut env = me.env(Senses { position, hero: position, can_see_hero: false });
        // SAFETY: `mem` is the bank's runtime size, 4-aligned.
        let mut tree = unsafe { Tree::init(bank(), me.mem.as_mut_ptr().cast(), &mut env, seed) };
        // The actor spawns with its FSMs already running: Initiate, then Idle, entered where it stands.
        tree.step(crate::fsm3::PH_ENTER);
        me.vel = env.vel;
        me.anim_left = env.anim_left;
        me
    }
    fn state(&mut self) -> u8 {
        let mut env = Env::new();
        // SAFETY: the block was laid out for this bank by `new`.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.state(0)
    }
    pub fn phase(mut self) -> Phase {
        let s = self.state();
        if self.is_dead() {
            Phase::Dead
        } else if s == STATE_STARTLE {
            Phase::Startle
        } else if s == STATE_CHASE_START || s == STATE_IN_SIGHT || s == STATE_OUT_OF_SIGHT {
            Phase::Chase
        } else if s == STATE_STOP {
            Phase::Stop
        } else {
            Phase::Idle
        }
    }

    fn is_dead(&mut self) -> bool {
        let mut env = Env::new();
        // SAFETY: as above.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.dead(0)
    }
    pub fn velocity(self) -> [i32; 2] {
        self.vel
    }
    pub fn facing(self) -> i32 {
        self.facing as i32
    }
    pub fn die(&mut self) {
        self.vel = [0; 2];
        let mut env = Env::new();
        // SAFETY: as above.
        let mut t = unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) };
        t.kill(0);
        t.kill(1);
    }
    fn env(&self, senses: Senses) -> Env {
        let mut env = Env::new();
        env.pos = [senses.position[0], senses.position[1], 0];
        env.hero = [senses.hero[0], senses.hero[1], 0];
        env.vel = self.vel;
        env.facing = self.facing;
        env.can_see_hero = senses.can_see_hero;
        env.in_alert_range = senses.can_see_hero;
        env.alert_range = ALERT_RADIUS;
        env.anim_left = self.anim_left;
        env.clips[0] = (CLIP_STARTLE, STARTLE_TICKS);
        env
    }
    /// Fold what the actions did back into the actor, and say it.
    fn harvest(&mut self, env: &Env, out: &mut Actions) {
        self.vel = env.vel;
        self.facing = env.facing;
        self.anim_left = env.anim_left;
        if env.sound & SOUND_STARTLE != 0 {
            out.push(Action::StartleSound);
        }
        if env.dirty & DIRTY_VEL != 0 {
            out.push(Action::Velocity(env.vel));
        }
        for (id, from) in &env.plays[..env.play_count as usize] {
            out.push(Action::Play(clip_of(*id), *from as u32));
        }
        if env.dirty & DIRTY_FACING != 0 {
            out.push(Action::Facing(env.facing as i32));
        }
    }
    /// HealthManager TOOK DAMAGE: only the Idle state listens.
    #[inline(never)]
    pub fn took_damage(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        let mut env = self.env(senses);
        // SAFETY: as above. A dead tree ignores the event.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.event(0, EV_TOOK_DAMAGE);
        self.harvest(&env, &mut out);
        out
    }
    /// One nominal 60 Hz frame.
    #[inline(never)]
    pub fn tick(&mut self, senses: Senses) -> Actions {
        let mut out = Actions::new();
        if self.is_dead() {
            return out;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
        }
        if self.anim_left != 0 && self.anim_left != 0xffff {
            self.anim_left -= 1;
        }
        let mut env = self.env(senses);
        // SAFETY: as above.
        let mut tree = unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) };
        if fixed {
            tree.step(PH_FIXED);
        }
        tree.step(PH_UPDATE);
        self.harvest(&env, &mut out);
        out
    }
}
