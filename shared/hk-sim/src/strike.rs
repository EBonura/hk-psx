//! The damaging shapes of the Knight's spells and nail arts.
//!
//! Desolate Dive's fall and slam, Howling Wraiths' three heads and the nail arts' slashes are polygon
//! colliders of the Knight prefab (`Knight/Spells/*`, `Knight/Attacks/*`) that a `Hit Box Control` or
//! `Control Collider` FSM switches on for a few frames of an effect. Each is a `HitSpec`: its corners
//! about the Knight's origin as the prefab authors them, how hard it hits and when. A `Strike` is one
//! of them placed in the world for one tick, which is what an enemy tests itself against.

/// Corners a shape may have (the prefab's colliders are five or fewer).
pub const MAX_POINTS: usize = 5;

/// A damaging shape, cooked from the install (host/hk-cook ability_params.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitSpec {
    /// Corners in 1/256 world units about the Knight's origin, as authored: the prefab faces left, so a
    /// Knight facing right sees them mirrored (`HitSpec::place`).
    pub points: [[i16; 2]; MAX_POINTS],
    pub len: u8,
    /// Damage with no charm (`Set Damage`'s first value, or the `damages_enemy` default).
    pub damage: u8,
    /// Ticks after the effect began between which it hurts: from `start`, up to but not at `end`.
    pub start: u8,
    pub end: u8,
}
impl HitSpec {
    pub const NONE: Self = Self { points: [[0; 2]; MAX_POINTS], len: 0, damage: 0, start: 0, end: 0 };
    /// Whether the shape hurts `age` ticks into its effect.
    pub fn live(&self, age: u16) -> bool {
        self.len != 0 && (u16::from(self.start)..u16::from(self.end)).contains(&age)
    }
    /// The shape for a Knight at (`x`, `y`) facing `facing` (+1 right).
    pub fn place(&self, x: i32, y: i32, facing: i32) -> Strike {
        let mut out = Strike { points: [[0; 2]; MAX_POINTS], len: self.len, damage: u16::from(self.damage) };
        for (o, p) in out.points.iter_mut().zip(&self.points) {
            *o = [x - (i32::from(p[0]) << 8) * facing, y + (i32::from(p[1]) << 8)];
        }
        out
    }
}

/// A `HitSpec` in the world for one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strike {
    pub points: [[i32; 2]; MAX_POINTS],
    pub len: u8,
    pub damage: u16,
}
impl Strike {
    pub const NONE: Self = Self { points: [[0; 2]; MAX_POINTS], len: 0, damage: 0 };
    /// The corners, for `polygon_hits_box`.
    pub fn corners(&self) -> &[[i32; 2]] {
        &self.points[..usize::from(self.len)]
    }
}
