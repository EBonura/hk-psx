//! Source-derived Plant Trap (Snapper Trap): the `Plant Trap Control` FSM.
//! It sits in the ground until the Knight stands in its `Detector` box, rears
//! for 0.75 s, snaps for a second and retracts, then rests half a second
//! before it can be tripped again. Evidence: docs/PLANT_TRAP.md.
//!
//! The object carries no collider of its own. tk2d builds a `BoxCollider2D`
//! from the sprite definition of whichever frame is showing, and most frames
//! define none, so the trap can hurt and be hurt only while its jaws are
//! open: the three `Snap` frames and the first three `Retract` frames. Those
//! boxes are constants here (the recognizer proves each against the sprite
//! definitions). The caller owns hits, contact damage, the corpse and Geo.
use crate::ONE;

/// `Detector`'s trigger box relative to the trap: offset (0.125, -1.932),
/// size (2.469, 1.261).
pub const DETECT: [i32; 4] = [-72_704, -167_936, 89_088, -85_283];
/// `Ready`'s Wait 0.75 s.
pub const READY_TICKS: u16 = 45;
/// `Snap` Wait 1 s, holding the third frame once the 0.25 s clip is done.
pub const SNAP_TICKS: u16 = 60;
/// `Retract`: seven frames at 12 fps, to completion.
pub const RETRACT_TICKS: u16 = 35;
/// `Cooldown` Wait 0.5 s.
pub const COOLDOWN_TICKS: u16 = 30;
/// Ticks per frame of the 12 fps clips.
pub const FRAME_TICKS: u16 = 5;
/// The clip clock at rest: long past the last frame of a Once clip.
pub const REST_TICKS: u32 = 1000;
/// The collider boxes the sprite definitions give, as [x0, y0, x1, y1] in
/// Q16 relative to the trap (centre minus and plus the half extents):
/// `Snap` frame 0, `Snap` frame 1, `Snap` frame 2 (and `Retract` frame 0),
/// `Retract` frame 1 and `Retract` frame 2.
pub const SNAP_0: [i32; 4] = [-129_024, -167_936, 134_144, -54_272];
pub const SNAP_1: [i32; 4] = [-56_320, -167_936, 41_984, 89_088];
pub const SNAP_2: [i32; 4] = [-56_320, -167_936, 49_152, 36_864];
pub const RETRACT_1: [i32; 4] = [-34_816, -167_936, 33_792, 36_864];
pub const RETRACT_2: [i32; 4] = [-34_816, -167_936, 30_720, -1_024];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Ready,
    Snap,
    Retract,
    Cooldown,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    /// Idle shows `Retract`'s last frame, which is the sprite the trap
    /// is authored with and returns to.
    Rest,
    SnapReady,
    Snap,
    Retract,
}
impl Clip {
    pub const COUNT: usize = 3;
    /// Index into the spec's cooked clips (Snap Ready, Snap, Retract).
    pub const fn slot(self) -> usize {
        match self {
            Clip::SnapReady => 0,
            Clip::Snap => 1,
            Clip::Retract | Clip::Rest => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlantTrap {
    phase: Phase,
    /// Ticks since the phase began.
    ticks: u16,
}
impl Default for PlantTrap {
    fn default() -> Self {
        Self::new()
    }
}
impl PlantTrap {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            ticks: 0,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn dead(&self) -> bool {
        self.phase == Phase::Dead
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
    }
    /// Ticks the current clip has been playing.
    pub fn clip_ticks(&self) -> u32 {
        match self.phase {
            Phase::Idle | Phase::Cooldown | Phase::Dead => REST_TICKS,
            _ => self.ticks as u32,
        }
    }
    pub fn clip(&self) -> Clip {
        match self.phase {
            Phase::Ready => Clip::SnapReady,
            Phase::Snap => Clip::Snap,
            Phase::Retract => Clip::Retract,
            _ => Clip::Rest,
        }
    }
    /// The collider tk2d has built for the frame showing, if any: the only
    /// part of the trap that hurts or can be hurt.
    pub fn collider(&self) -> Option<[i32; 4]> {
        let frame = self.ticks / FRAME_TICKS;
        match self.phase {
            Phase::Snap => Some(match frame {
                0 => SNAP_0,
                1 => SNAP_1,
                _ => SNAP_2,
            }),
            Phase::Retract => match frame {
                0 => Some(SNAP_2),
                1 => Some(RETRACT_1),
                2 => Some(RETRACT_2),
                _ => None,
            },
            _ => None,
        }
    }
    /// One nominal 60 Hz frame. `detected` is the Knight standing in `DETECT`.
    pub fn tick(&mut self, detected: bool) {
        match self.phase {
            Phase::Dead => {}
            Phase::Idle => {
                if detected {
                    self.phase = Phase::Ready;
                    self.ticks = 0;
                }
            }
            Phase::Ready => self.step(READY_TICKS, Phase::Snap),
            Phase::Snap => self.step(SNAP_TICKS, Phase::Retract),
            Phase::Retract => self.step(RETRACT_TICKS, Phase::Cooldown),
            Phase::Cooldown => self.step(COOLDOWN_TICKS, Phase::Idle),
        }
    }
    fn step(&mut self, length: u16, next: Phase) {
        self.ticks += 1;
        if self.ticks >= length {
            self.phase = next;
            self.ticks = 0;
        }
    }
}
const _: () = assert!(ONE == 65_536);

#[cfg(test)]
mod tests {
    use super::*;

    fn run_to(t: &mut PlantTrap, phase: Phase, limit: u32) -> u32 {
        for n in 0..limit {
            if t.phase() == phase {
                return n;
            }
            t.tick(false);
        }
        panic!("never reached {phase:?}: {:?}", t.phase());
    }

    #[test]
    fn rests_unhurtable_until_the_knight_steps_into_the_detector() {
        let mut t = PlantTrap::new();
        for _ in 0..600 {
            t.tick(false);
        }
        assert_eq!((t.phase(), t.collider()), (Phase::Idle, None));
        t.tick(true);
        assert_eq!(t.phase(), Phase::Ready);
        assert_eq!(
            t.collider(),
            None,
            "no frame of Snap Ready defines a collider"
        );
    }

    #[test]
    fn one_cycle_has_the_authored_lengths() {
        let mut t = PlantTrap::new();
        t.tick(true);
        assert_eq!(run_to(&mut t, Phase::Snap, 100), READY_TICKS as u32);
        assert_eq!(run_to(&mut t, Phase::Retract, 100), SNAP_TICKS as u32);
        assert_eq!(run_to(&mut t, Phase::Cooldown, 100), RETRACT_TICKS as u32);
        assert_eq!(run_to(&mut t, Phase::Idle, 100), COOLDOWN_TICKS as u32);
    }

    #[test]
    fn the_jaws_hurt_through_the_snap_and_the_first_of_the_retract() {
        let mut t = PlantTrap::new();
        t.tick(true);
        run_to(&mut t, Phase::Snap, 100);
        let mut armed = 0;
        let mut boxes = [None; 4];
        let mut seen = 0;
        for _ in 0..SNAP_TICKS {
            if let Some(b) = t.collider() {
                armed += 1;
                if !boxes[..seen].contains(&Some(b)) {
                    boxes[seen] = Some(b);
                    seen += 1;
                }
            }
            t.tick(false);
        }
        assert_eq!(
            armed, SNAP_TICKS,
            "the snap holds its open jaws for the whole second"
        );
        assert_eq!(&boxes[..3], &[Some(SNAP_0), Some(SNAP_1), Some(SNAP_2)]);
        let mut armed = 0;
        for _ in 0..RETRACT_TICKS {
            armed += t.collider().is_some() as u32;
            t.tick(false);
        }
        assert_eq!(armed, 3 * FRAME_TICKS as u32);
    }

    #[test]
    fn cooldown_ignores_the_knight_and_death_is_final() {
        let mut t = PlantTrap::new();
        t.tick(true);
        run_to(&mut t, Phase::Cooldown, 300);
        t.tick(true);
        assert_eq!(
            t.phase(),
            Phase::Cooldown,
            "DETECT only means something in Idle"
        );
        t.die();
        t.tick(true);
        assert!(t.dead() && t.collider().is_none());
    }
}
