//! CameraTarget/CameraController subset (.hkpsx/camera/source.il and the
//! serialized resources.assets components): the target follows the hero with
//! a one-unit look-ahead and 0.075 s damping, the camera follows the target
//! with 0.15 s damping at most 30 units/s, both clamped to the active
//! CameraLockArea limits or the view's camera bounds. SmoothDamp is a
//! critically damped spring; this is a per-tick exponential approach with the
//! same time constants (not a claim of source trajectory parity). Fall catcher,
//! look up/down, quake and super dash offsets are not modelled.
use crate::ONE;

const TARGET_DAMP_Q16: i32 = 14564; // 1 / (0.075 s * 60)
const CAMERA_DAMP_Q16: i32 = 7282; // 1 / (0.15 s * 60)
const MAX_STEP: i32 = ONE / 2; // 30 units/s
const LOOK_AHEAD: i32 = ONE;
/// A hero displacement this large in one tick is a respawn or gate: snap.
const SNAP_DISTANCE: i32 = 20 * ONE;
/// Hero transform to camera target vertical offset used by the guest views.
pub const HERO_OFFSET_Y: i32 = 2 * ONE;

#[derive(Clone, Copy)]
pub struct Camera {
    target: (i32, i32),
    position: (i32, i32),
    last_hero: Option<(i32, i32)>,
}
fn clamp2(p: (i32, i32), limits: [i32; 4]) -> (i32, i32) {
    (p.0.clamp(limits[0].min(limits[2]), limits[2].max(limits[0])), p.1.clamp(limits[1].min(limits[3]), limits[3].max(limits[1])))
}
fn approach(from: i32, to: i32, factor_q16: i32, max_step: i32) -> i32 {
    let delta = ((to as i64 - from as i64) * factor_q16 as i64 >> 16) as i32;
    from + delta.clamp(-max_step, max_step)
}
impl Camera {
    pub const fn new() -> Self {
        Self { target: (0, 0), position: (0, 0), last_hero: None }
    }
    pub fn position(&self) -> (i32, i32) {
        self.position
    }
    /// PositionToStart / EnterLockZoneInstant: camera and target on the hero.
    pub fn snap(&mut self, hero: (i32, i32), facing: i32, limits: [i32; 4]) {
        let goal = clamp2((hero.0 + facing * LOOK_AHEAD, hero.1 + HERO_OFFSET_Y), limits);
        self.target = goal;
        self.position = goal;
        self.last_hero = Some(hero);
    }
    /// One 60 Hz tick.
    pub fn tick(&mut self, hero: (i32, i32), facing: i32, limits: [i32; 4]) {
        if self.last_hero.is_none_or(|h| (h.0 - hero.0).abs() > SNAP_DISTANCE || (h.1 - hero.1).abs() > SNAP_DISTANCE) {
            self.snap(hero, facing, limits);
            return;
        }
        self.last_hero = Some(hero);
        let goal = clamp2((hero.0 + facing * LOOK_AHEAD, hero.1 + HERO_OFFSET_Y), limits);
        self.target = (approach(self.target.0, goal.0, TARGET_DAMP_Q16, i32::MAX), approach(self.target.1, goal.1, TARGET_DAMP_Q16, i32::MAX));
        // KeepWithinLockBounds applies to the camera itself as well.
        let target = clamp2(self.target, limits);
        self.position = (approach(self.position.0, target.0, CAMERA_DAMP_Q16, MAX_STEP), approach(self.position.1, target.1, CAMERA_DAMP_Q16, MAX_STEP));
    }
}
/// The lock whose trigger the hero body overlaps (the last one in bank order,
/// as the source keeps the most recently entered zone), else the view bounds.
pub fn limits(region: &crate::world::Region, hero_body: [i32; 4]) -> [i32; 4] {
    let mut limits = region.camera;
    for lock in crate::world::camera_locks(region) {
        let b = lock.bounds;
        if b[0] <= hero_body[2] && b[2] >= hero_body[0] && b[1] <= hero_body[3] && b[3] >= hero_body[1] {
            limits = lock.limits;
        }
    }
    limits
}
