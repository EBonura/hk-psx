//! Source-derived Gruzzer (Fly) `Bouncer Control` FSM: wait for the camera,
//! then fly at a fixed speed along a random angle and re-aim on each bonk.
//! Evidence: .hkpsx/gruzzer/CONTRACT.md and source.il.
//!
//! The caller owns physics (gravity-free dynamic body), turns the angle into a
//! velocity, detects the blocked side after its solver step (CheckCollisionSide
//! priority up, right, down, left) and owns hits, Recoil, corpse and Geo.
//! Random.Range is replaced by a deterministic per-actor sequence.
use crate::ONE;

pub const SPEED: i32 = 340787; // 5.2 units/s
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gruzzer {
    phase: Phase,
    /// Q16 degrees, counter-clockwise from +x as the source angle.
    angle: i32,
    facing_right: bool,
    rng: u32,
}
impl Gruzzer {
    pub fn new(seed: u32) -> Self {
        Self {
            phase: Phase::Waiting,
            angle: 0,
            facing_right: false,
            rng: seed,
        }
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn angle(self) -> i32 {
        self.angle
    }
    /// FaceDirection every frame: +1 when the x velocity is positive.
    pub fn facing(self) -> i32 {
        let a = self.angle.rem_euclid(360 * ONE);
        if self.phase == Phase::Flying && !(90 * ONE..=270 * ONE).contains(&a) {
            1
        } else {
            -1
        }
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
    /// `Left or Right?`: FloatSwitch below 90 RIGHT, below 270 LEFT, below 360 RIGHT.
    fn aim(&mut self, low_degrees: i32, high_degrees: i32) {
        self.angle = self.range(low_degrees * ONE, high_degrees * ONE);
        self.facing_right =
            self.angle < 90 * ONE || (self.angle >= 270 * ONE && self.angle < 360 * ONE);
    }
    /// Initialise: GetDistance(owner, MainCamera) < 44 every frame, then Aim.
    pub fn tick(&mut self, camera: [i32; 3], position: [i32; 3]) {
        if self.phase != Phase::Waiting {
            return;
        }
        let limit = CAMERA_RANGE as i64;
        let mut squared = 0i64;
        for axis in 0..3 {
            let delta = camera[axis] as i64 - position[axis] as i64;
            if delta <= -limit || delta >= limit {
                return;
            }
            squared += delta * delta;
        }
        if squared < limit * limit {
            self.phase = Phase::Flying;
            self.aim(0, 360);
        }
    }
    /// CheckCollisionSide events while flying: each side re-aims from the
    /// authored range for the current facing or vertical half.
    pub fn bonk(&mut self, side: Side) {
        if self.phase != Phase::Flying {
            return;
        }
        let up = self.angle < 180 * ONE;
        match (side, self.facing_right, up) {
            (Side::Up, true, _) => self.aim(320, 350),
            (Side::Up, false, _) => self.aim(190, 220),
            (Side::Down, true, _) => self.aim(10, 40),
            (Side::Down, false, _) => self.aim(140, 170),
            (Side::Right, _, true) => self.aim(140, 170),
            (Side::Right, _, false) => self.aim(190, 220),
            (Side::Left, _, true) => self.aim(10, 40),
            (Side::Left, _, false) => self.aim(320, 350),
        }
    }
}
