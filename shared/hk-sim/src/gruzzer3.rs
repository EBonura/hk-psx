//! Original `Bouncer Control` graph through the shared HKFSM03 kernel.
//! The caller retains camera activation, physics, recoil, damage and corpses.
//! Port RNG remains deterministic LCG rather than Unity's global RNG.
use crate::fsm3::{Env, Tree, PH_COLLISION_STAY, PH_UPDATE};
use crate::{fsm3::PH_ENTER, ONE};
use hk_format::fsm3::Bank;
include!("../../../data/gruzzer_fsm3.rs");

pub const SPEED: i32 = 340787;
pub const CAMERA_RANGE: i32 = 44 * ONE;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Flying,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Up,
    Right,
    Down,
    Left,
}
#[derive(Clone, Copy)]
pub struct Gruzzer {
    mem: [u32; RUNTIME_BYTES / 4],
    facing: i8,
    dead: bool,
}
// Replaces the old controller inside the existing widest Runtime payload.
const _: () = assert!(core::mem::size_of::<Gruzzer>() <= 80);

fn bank() -> Bank<'static> {
    // SAFETY: the cooker parses the bank it emits and the module transport hash-checks it.
    unsafe { Bank::from_verified(&BANK) }
}

impl Gruzzer {
    #[inline(never)]
    pub fn new(seed: u32) -> Self {
        let mut me = Self { mem: [0; RUNTIME_BYTES / 4], facing: -1, dead: false };
        let mut env = Env::new();
        // SAFETY: `mem` is the bank's runtime size, 4-aligned.
        unsafe { Tree::init(bank(), me.mem.as_mut_ptr().cast(), &mut env, seed) };
        me
    }
    fn state(&mut self) -> u8 {
        let mut env = Env::new();
        // SAFETY: the block was laid out for this bank by `new`.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.state(0)
    }
    pub fn phase(mut self) -> Phase {
        let state = self.state();
        if self.dead {
            Phase::Dead
        } else if state == STATE_INITIALISE || state == STATE_STOPPED {
            Phase::Waiting
        } else {
            Phase::Flying
        }
    }
    pub fn angle(mut self) -> i32 {
        let mut env = Env::new();
        // SAFETY: as above.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.cell(0, CELL_ANGLE as usize)
    }
    pub fn facing(self) -> i32 {
        self.facing as i32
    }
    pub fn die(&mut self) {
        self.dead = true;
        let mut env = Env::new();
        // SAFETY: as above.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.kill(0);
    }
    fn step(&mut self, camera: [i32; 3], position: [i32; 3], contacts: u8, phase: u8) {
        if self.dead {
            return;
        }
        let flying = self.phase() == Phase::Flying;
        let mut env = Env::new();
        env.camera = camera;
        env.pos = position;
        env.angle = self.angle();
        env.speed = if flying { SPEED } else { 0 };
        env.contacts = contacts;
        env.facing = self.facing;
        env.vel[0] = crate::fsm3::heading_sign(env.angle, env.speed);
        // SAFETY: as above.
        unsafe { Tree::attach(bank(), self.mem.as_mut_ptr().cast(), &mut env) }.step(phase);
        self.facing = env.facing;
    }
    pub fn tick(&mut self, camera: [i32; 3], position: [i32; 3]) {
        self.step(camera, position, 0, PH_UPDATE);
    }
    pub fn bonk(&mut self, side: Side) {
        if self.phase() != Phase::Flying {
            return;
        }
        let bit = match side {
            Side::Up => 1,
            Side::Right => 2,
            Side::Down => 4,
            Side::Left => 8,
        };
        self.step([0; 3], [0; 3], bit, PH_COLLISION_STAY);
    }
}
#[allow(dead_code)]
const _UNUSED: u8 = PH_ENTER;
