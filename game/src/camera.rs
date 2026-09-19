//! CameraTarget/CameraController subset (.hkpsx/camera/source.il and the
//! serialized resources.assets components): the target follows the hero with
//! a one-unit look-ahead and 0.075 s damping, the camera follows the target
//! with 0.15 s damping at most 30 units/s, both clamped to the active
//! CameraLockArea limits or the view's camera bounds. SmoothDamp is a
//! critically damped spring; this is a per-tick exponential approach with the
//! same time constants (not a claim of source trajectory parity). Fall catcher,
//! look up/down, quake and super dash offsets are not modelled.
//!
//! `CameraShake` (`resources.assets:22920`, on `_GameCameras/CameraParent`) is
//! modelled, because it is the whole of what a boss slam looks like here. Every
//! `SendEventByName("...Shake")` in the game reaches that one FSM.
use crate::ONE;

const TARGET_DAMP_Q16: i32 = 14564; // 1 / (0.075 s * 60)
const CAMERA_DAMP_Q16: i32 = 7282; // 1 / (0.15 s * 60)
const MAX_STEP: i32 = ONE / 2; // 30 units/s
const LOOK_AHEAD: i32 = ONE;
/// A hero displacement this large in one tick is a respawn or gate: snap.
const SNAP_DISTANCE: i32 = 20 * ONE;
/// Hero transform to camera target vertical offset used by the guest views.
pub const HERO_OFFSET_Y: i32 = 2 * ONE;

/// The four `CameraShake` states an enemy can reach, in rising `Priority`.
/// Each is one `ShakePositionV2` with its own `Extents` vector and `Duration`;
/// the FSM's serialized values are in `SHAKES` below and nothing else varies
/// between them, so one table is the whole difference.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shake {
    /// `ShakingSmall`.
    Small,
    /// `ShakingKill`: `FalseyControl`'s jumps and `Death Anim Start`.
    Kill,
    /// `ShakingAverage`: the False Knight's `S Land` and `Rage Slam`.
    Average,
    /// `ShakingBig`: `Slam`, `JA Slam`, `Stun Start`, `Start Fall`, `Floor Break`.
    Big,
}
/// (extents in Q16 world units, duration in 60 Hz ticks, `Priority`), read off
/// the serialized FSM: SmallShake 0.08/0.5 s/3, EnemyKillShake 0.105/0.5 s/6,
/// AverageShake 0.15/1 s/7, BigShake 0.5/1 s/10. Both axes carry the same
/// extent in every one of them, so this holds one value rather than a vector.
const SHAKES: [(i32, u16, u8); 4] = [(5243, 30, 3), (6881, 30, 6), (9830, 60, 7), (32768, 60, 10)];
/// The serialized row behind one shake, so a test can check it against the FSM.
pub const fn shake_spec(kind: Shake) -> (i32, u16, u8) {
    SHAKES[kind as usize]
}
/// Fixed so a replayed route shakes identically; `snap` restores it per scene.
const SHAKE_SEED: u32 = 0x53_48_4b_21;
#[derive(Clone, Copy)]
struct Shaker {
    /// `Priority`, which only `Normal` clears, on `DoneShaking`.
    priority: u8,
    /// `Duration` in ticks; zero is `Normal`, where nothing is displaced.
    ticks: u16,
    elapsed: u16,
    extents: i32,
    rng: u32,
    offset: (i32, i32),
}
impl Shaker {
    const fn new() -> Self {
        Self { priority: 0, ticks: 0, elapsed: 0, extents: 0, rng: SHAKE_SEED, offset: (0, 0) }
    }
}
static mut SHAKER: Shaker = Shaker::new();
#[no_mangle]
pub static mut HK_CAMERA_SHAKES: u32 = 0;
#[no_mangle]
pub static mut HK_CAMERA_SHAKES_REFUSED: u32 = 0;
/// One `Random.Range(-1f, 1f)` axis of `ShakePositionV2`, scaled by the shake's
/// extent and by the linear decay. One LCG step per axis rather than one draw
/// split in half, because an LCG's low bits are not independent of its high.
fn shake_axis(rng: &mut u32, extents: i32, amount_q16: i32) -> i32 {
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    let unit = ((*rng >> 15) as i32) - ONE; // [-1, 1) in Q16
    ((((extents as i64 * unit as i64) >> 16) * amount_q16 as i64) >> 16) as i32
}
/// `SendEventByName`: the global transition runs `To <name> Shake`, whose
/// `FloatCompare` enters the shake only while the live `Priority` is strictly
/// lower, and otherwise `GotoPreviousState` drops the request. That is why
/// three slams in a row do not stack into one long tremor, and why the boss's
/// jump never interrupts the slam it is part of.
pub fn request(kind: Shake) {
    let (extents, ticks, priority) = SHAKES[kind as usize];
    unsafe {
        let shaker = &mut *(&raw mut SHAKER);
        if shaker.priority >= priority {
            HK_CAMERA_SHAKES_REFUSED = HK_CAMERA_SHAKES_REFUSED.wrapping_add(1);
            return;
        }
        // The outgoing ShakePositionV2's OnExit restores the unshaken position
        // before the new state's first sample, so one tick sits at no offset.
        *shaker = Shaker { priority, ticks, elapsed: 0, extents, rng: shaker.rng, offset: (0, 0) };
        HK_CAMERA_SHAKES = HK_CAMERA_SHAKES.wrapping_add(1);
    }
}
/// One 60 Hz sample, which is also the action's own `FpsLimit`. Costs a branch
/// on a tick with no shake running, which is every tick outside a boss arena.
fn advance_shake() {
    unsafe {
        let shaker = &mut *(&raw mut SHAKER);
        if shaker.ticks == 0 {
            return;
        }
        shaker.elapsed += 1;
        if shaker.elapsed >= shaker.ticks {
            *shaker = Shaker { rng: shaker.rng, ..Shaker::new() };
            return;
        }
        // Clamp01(1 - timer / Duration), sampled after the timer advances.
        let amount = ONE - (shaker.elapsed as i32 * ONE / shaker.ticks as i32);
        let extents = shaker.extents;
        shaker.offset = (shake_axis(&mut shaker.rng, extents, amount),
                         shake_axis(&mut shaker.rng, extents, amount));
    }
}
/// `CancelAllShake` / `New Scene Reset`: zero the priority and the displacement.
fn cancel_shake() {
    unsafe {
        *(&raw mut SHAKER) = Shaker::new();
    }
}
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
    /// The rendered viewpoint: the smoothed follow plus whatever `CameraShake`
    /// is displacing `CameraParent` by. The shake rides on top rather than
    /// feeding back, because in the source it moves the follow camera's parent
    /// and the damping below reads the camera's own unshaken transform.
    pub fn position(&self) -> (i32, i32) {
        let offset = unsafe { (*(&raw const SHAKER)).offset };
        (self.position.0 + offset.0, self.position.1 + offset.1)
    }
    /// PositionToStart / EnterLockZoneInstant: camera and target on the hero.
    pub fn snap(&mut self, hero: (i32, i32), facing: i32, limits: [i32; 4]) {
        let goal = clamp2((hero.0 + facing * LOOK_AHEAD, hero.1 + HERO_OFFSET_Y), limits);
        self.target = goal;
        self.position = goal;
        self.last_hero = Some(hero);
        // A gate, a respawn or a scene load is the FSM's own LEVEL LOADED.
        cancel_shake();
    }
    /// One 60 Hz tick.
    pub fn tick(&mut self, hero: (i32, i32), facing: i32, limits: [i32; 4]) {
        advance_shake();
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
