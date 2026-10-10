//! The original's camera, CameraTarget + CameraController + CameraLockArea
//! (Assembly-CSharp IL, transcribed in hk-camera's CAMERA.md), at the 60 Hz
//! simulation tick in Q16 integer maths.
//!
//! The target sits on the Knight (`stickToHero` once it has caught him, a
//! 0.075 s SmoothDamp otherwise, 0.5 s after a lock change); the camera aims
//! at the target plus a look-ahead that eases to one unit in front of him at
//! 6 units/s (1.5 while dashing) and follows with a 0.15 s SmoothDamp. Inside a
//! CameraLockArea the target is clamped to the lock's limits, and the camera,
//! lock or not, never leaves the scene bounds: in the original 14.6 to tilemap
//! width - 14.6 and 8.3 to tilemap height - 8.3, half its view in from the
//! scene edges, which is why its camera stops at a room's edge. The PS1 view
//! is narrower (21.6 by 16.2 units against 29.2 by 16.6), so here the same
//! clamp uses the PS1's own half view and lock limits move with it: the view
//! stops on the same wall the original's does. A falling Knight
//! pulls the camera down after him (the fall catcher); holding up or down while
//! standing still for 0.85 s looks 6 units that way. The SmoothDamp is Unity's
//! critically damped spring itself, velocity included.
//!
//! `CameraShake` (`resources.assets:22920`, on `_GameCameras/CameraParent`) is
//! modelled, because it is the whole of what a boss slam looks like here. Every
//! `SendEventByName("...Shake")` in the game reaches that one FSM.
use crate::ONE;

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
        Self {
            priority: 0,
            ticks: 0,
            elapsed: 0,
            extents: 0,
            rng: SHAKE_SEED,
            offset: (0, 0),
        }
    }
}
static mut SHAKER: Shaker = Shaker::new();
#[no_mangle]
pub static mut HK_CAMERA_SHAKES: u32 = 0;
/// The follow camera's centre (Q16 world units, before shake), published
/// every tick so a replay can compare it with the active view's cooked
/// camera range: scenery is cooked per view for that range only.
#[no_mangle]
pub static mut HK_CAMERA_X: i32 = 0;
#[no_mangle]
pub static mut HK_CAMERA_Y: i32 = 0;
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
        *shaker = Shaker {
            priority,
            ticks,
            elapsed: 0,
            extents,
            rng: shaker.rng,
            offset: (0, 0),
        };
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
            *shaker = Shaker {
                rng: shaker.rng,
                ..Shaker::new()
            };
            return;
        }
        // Clamp01(1 - timer / Duration), sampled after the timer advances.
        let amount = ONE - (shaker.elapsed as i32 * ONE / shaker.ticks as i32);
        let extents = shaker.extents;
        shaker.offset = (
            shake_axis(&mut shaker.rng, extents, amount),
            shake_axis(&mut shaker.rng, extents, amount),
        );
    }
}
/// `CancelAllShake` / `New Scene Reset`: zero the priority and the displacement.
fn cancel_shake() {
    unsafe {
        *(&raw mut SHAKER) = Shaker::new();
    }
}
/// `KeepWithinSceneBounds`' lower bounds in the original, 14.6 and 8.3
/// units (Q16 of the float constants): half its 16:9 view at the gameplay
/// plane, so its view's edge stops on the tilemap's edge.
const ORIGINAL_HALF: (i32, i32) = (956_826, 543_949);
/// The same for the PS1's 320x240 view at KNIGHT_SCALE (14.8178 pixels a
/// unit): 160 and 120 pixels are 10.798 and 8.098 units. The camera clamps
/// with these, so the narrower PS1 view reaches the wall the original's
/// reaches, and every lock limit moves by the same difference (view edges
/// kept where the original's are), see `view_limits`.
const X_MIN: i32 = (160i64 * (1 << 28) / crate::KNIGHT_SCALE as i64) as i32;
const Y_MIN: i32 = (120i64 * (1 << 28) / crate::KNIGHT_SCALE as i64) as i32;
/// CameraTarget's serialized `xLookAhead` 1, `dashLookAhead` 1.5 and
/// `snapDistance` 0.15, and the look-ahead's `deltaTime * 6` per frame.
const LOOK_AHEAD: i32 = ONE;
const LOOK_AHEAD_STEP: i32 = 6_554;
const DASH_LOOK_AHEAD: i32 = 98_304;
const SNAP: i32 = 9_830;
/// SmoothDamp times in Q16 seconds: CameraTarget `dampTimeNormal` 0.075 and
/// `dampTimeSlow` 0.5 (which `SetDampTime` walks back down by 0.007 a frame),
/// CameraController `dampTime` 0.15.
const DAMP_NORMAL: i32 = 4_915;
const DAMP_SLOW: i32 = 32_768;
const DAMP_STEP: i32 = 459;
const CAMERA_DAMP: i32 = 9_830;
/// `slowTime` and `startLockedTimer`, 0.5 s: frames the timer is still above
/// zero at its test (float32 counts down to -1.8e-7 on the 30th).
const SLOW_TICKS: u8 = 30;
const START_LOCKED_TICKS: u8 = 30;
/// `DoPositionToHero` holds the camera FROZEN for `WaitForSeconds(0.1)`.
const FROZEN_TICKS: u8 = 6;
/// The fall catcher: the camera may sit 0.1 above a falling Knight; the catch
/// speed grows by 80 units/s each second while under 25 (Q16 units/s).
const FALL_STICK: i32 = 6_554;
const FALL_CATCH_STEP: i32 = 87_381;
const FALL_CATCH_MAX: i32 = 25 * ONE;
/// `lookOffset` while looking up or down, and `LOOK_DELAY` 0.85 s: the float
/// timer first reaches it after 52 frames, so the 53rd standing frame looks.
const LOOK_OFFSET: i32 = 6 * ONE;
const LOOK_DELAY_TICKS: u8 = 52;
/// A hero displacement this large in one tick, inside one scene, is a respawn:
/// the hazard respawn's `CameraRepositionToHero`.
const SNAP_DISTANCE: i32 = 20 * ONE;
/// CameraLockArea flags the cooker writes (tools/world_metadata.py).
pub const LOCK_PREVENT_LOOK_DOWN: u16 = 1;
pub const LOCK_PREVENT_LOOK_UP: u16 = 2;
pub const LOCK_MAX_PRIORITY: u16 = 4;
/// A Battle Control's `CameraLockArea B`: live while its scene's arena is
/// sealed (`battle_gates::fighting`), off otherwise.
pub const LOCK_BATTLE: u16 = 8;
/// A scene's lock list (Crossroads_ShamanTemple has the most, 23).
const MAX_SCENE_LOCKS: usize = 24;
/// CameraTarget `superDashLookAhead`.
const SUPER_DASH_LOOK_AHEAD: i32 = 6 * ONE;
/// Lock areas the Knight can be inside at once, plus the ones he left that are
/// still in `lockZoneList`. The densest admitted overlap is three.
const MAX_AREAS: usize = 8;
const NONE: u8 = u8::MAX;

/// CameraController.CameraMode: FADEOUT, FADEIN and PANNING never reach a
/// gameplay tick here (the gate fade stops the tick), so FROZEN stands in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Frozen,
    Following,
    Locked,
}
/// CameraTarget.TargetMode, less BOSS, which nothing sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetMode {
    Follow,
    Lock,
    Free,
}

/// One CameraLockArea as the camera sees it: its trigger box, its limits (with
/// `ValidateBounds`' -1 already resolved by the cooker), flags, whether the
/// Knight's body overlaps it, and its place in `lockZoneList` (0: not listed).
#[derive(Clone, Copy)]
struct Area {
    id: u32,
    trigger: [i32; 4],
    limits: [i32; 4],
    flags: u16,
    inside: bool,
    listed: u16,
}
impl Area {
    const EMPTY: Self = Self {
        id: 0,
        trigger: [0; 4],
        limits: [0; 4],
        flags: 0,
        inside: false,
        listed: 0,
    };
}

/// What the camera reads off the Knight each tick.
#[derive(Clone, Copy)]
pub struct Hero {
    pub x: i32,
    pub y: i32,
    /// The body box CameraLockArea's trigger tests (Knight BoxCollider2D).
    pub body: [i32; 4],
    pub facing: i32,
    /// `cState.dashing` with `|current_velocity.x| > 5`.
    pub dashing: bool,
    /// The Superdash FSM's `SetSuperDash`: true from Dash Start / Enter Super
    /// Dash, false at Cancel, Air Cancel and Hit Wall, so while it travels.
    pub super_dashing: bool,
    /// `cState.falling`: airborne and moving down.
    pub falling: bool,
    /// `cState.transitioning`: walking in through a gate.
    pub transitioning: bool,
}
/// HeroController's look state inputs: `hero_state == idle`, up and down held,
/// horizontal input past 0.6 (FixedUpdate's ResetLook) and the moves that
/// call ResetLook (attack, jump, dash, losing control).
#[derive(Clone, Copy)]
pub struct LookInput {
    pub idle: bool,
    pub up: bool,
    pub down: bool,
    pub moving: bool,
    pub reset: bool,
}

#[derive(Clone, Copy)]
pub struct Camera {
    scene: u16,
    /// The scene's CameraLockAreas: object indices in its bank's first region
    /// and trigger bounds, so a tick reads a lock's object only when the
    /// Knight's body reaches its box.
    scene_locks: [u16; MAX_SCENE_LOCKS],
    scene_lock_bounds: [[i32; 4]; MAX_SCENE_LOCKS],
    scene_lock_count: u8,
    /// CameraController `xLimit`, `yLimit`.
    limit: (i32, i32),
    /// CameraController transform, without the shake (CameraParent).
    position: (i32, i32),
    /// SmoothDamp velocities in Q16 units per tick.
    velocity: (i32, i32),
    mode: Mode,
    prev_mode: Mode,
    frozen: u8,
    /// `xLockMin`, `xLockMax`, `yLockMin`, `yLockMax`.
    lock: [i32; 4],
    current: u8,
    areas: [Area; MAX_AREAS],
    order: u16,
    start_locked: u8,
    target: (i32, i32),
    target_velocity: (i32, i32),
    target_mode: TargetMode,
    x_offset: i32,
    dash_offset: i32,
    /// CameraTarget `dampTimeX/Y` in Q16 seconds (0 until the first SetDampTime).
    damp: (i32, i32),
    slow: u8,
    stick: (bool, bool),
    fall_catcher: i32,
    fall_stick: bool,
    hero_prev: (i32, i32),
    target_lock: [i32; 4],
    /// entered/exited Left, Right, Top, Bot as bits 0..3.
    entered: u8,
    exited: u8,
    look_timer: u8,
    /// +1 looking up, -1 looking down.
    looking: i8,
    last_hero: Option<(i32, i32)>,
}

#[no_mangle]
pub static mut HK_CAMERA_LOCK: u32 = 0;
fn publish(p: (i32, i32)) {
    unsafe {
        HK_CAMERA_X = p.0;
        HK_CAMERA_Y = p.1;
    }
}
/// Q16 product, rounded to nearest so a spring's tail does not creep.
fn mul(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64 + 0x8000) >> 16) as i32
}
/// Vector3.SmoothDamp on one axis with an infinite maxSpeed and the 1/60 s
/// tick: omega = 2 / smoothTime, x = omega * dt, exp the cubic Pade of e^-x.
/// `velocity` is the source's units/s times dt. Its overshoot test, which
/// lands on the target with zero velocity, is the product form Vector3 uses.
#[inline(never)]
#[optimize(size)]
fn smooth_damp(current: i32, target: i32, velocity: &mut i32, smooth: i32) -> i32 {
    let smooth = smooth.max(7) as u32; // Mathf.Max(0.0001f, smoothTime)
    let x = (143_165_577 / smooth) as i32; // 2 / (60 * smoothTime), Q16
    let x2 = mul(x, x);
    let x3 = mul(x2, x);
    let denominator = ONE + x + mul(31_457, x2) + mul(15_401, x3);
    let exp = (u32::MAX / denominator as u32 + 1) as i32; // 2^32 / d in Q16
    let change = current - target;
    let temp = *velocity + mul(x, change);
    *velocity = mul(*velocity - mul(x, temp), exp);
    let output = target + mul(change + temp, exp);
    if (target - current) as i64 * (output - target) as i64 > 0 {
        *velocity = 0;
        return target;
    }
    output
}
/// A lock's camera-centre limits for the PS1 view: each limit keeps the view
/// edge it gives the original's view, so a minimum moves down and a maximum
/// up by the difference in half extents (a lock that pins the original's
/// camera lets the narrower view slide inside the original's frame). Negative
/// limits stay as authored: LockToArea reads them as "the scene's own".
fn view_limits(l: [i32; 4]) -> [i32; 4] {
    let (dx, dy) = (X_MIN - ORIGINAL_HALF.0, Y_MIN - ORIGINAL_HALF.1);
    let at = |v: i32, d: i32| if v < 0 { v } else { v + d };
    [at(l[0], dx), at(l[1], -dx), at(l[2], dy), at(l[3], -dy)]
}
fn clamp_box(p: (i32, i32), b: [i32; 4]) -> (i32, i32) {
    // The source's two ifs per axis: a lower bound above the upper one leaves
    // the upper one in force (Crossroads_18 authors xMin 26.27 > xMax 25.43).
    let mut x = p.0;
    if x < b[0] {
        x = b[0];
    }
    if x > b[1] {
        x = b[1];
    }
    let mut y = p.1;
    if y < b[2] {
        y = b[2];
    }
    if y > b[3] {
        y = b[3];
    }
    (x, y)
}
/// OnTriggerEnter2D/Exit2D's side tests against the trigger's bounds: within
/// one unit of the left, right or bottom edge, two of the top.
fn sides(trigger: [i32; 4], x: i32, y: i32) -> u8 {
    let near = |v: i32, edge: i32, reach: i32| v > edge - reach && v < edge + reach;
    (near(x, trigger[0], ONE) as u8)
        | (near(x, trigger[2], ONE) as u8) << 1
        | (near(y, trigger[3], 2 * ONE) as u8) << 2
        | (near(y, trigger[1], ONE) as u8) << 3
}
const LEFT: u8 = 1;
const RIGHT: u8 = 2;
const TOP: u8 = 4;
const BOT: u8 = 8;

impl Camera {
    pub const fn new() -> Self {
        Self {
            scene: u16::MAX,
            scene_locks: [0; MAX_SCENE_LOCKS],
            scene_lock_bounds: [[0; 4]; MAX_SCENE_LOCKS],
            scene_lock_count: 0,
            limit: (0, 0),
            position: (0, 0),
            velocity: (0, 0),
            mode: Mode::Following,
            prev_mode: Mode::Following,
            frozen: 0,
            lock: [0; 4],
            current: NONE,
            areas: [Area::EMPTY; MAX_AREAS],
            order: 0,
            start_locked: 0,
            target: (0, 0),
            target_velocity: (0, 0),
            target_mode: TargetMode::Follow,
            x_offset: 0,
            dash_offset: 0,
            damp: (0, 0),
            slow: 0,
            stick: (true, true),
            fall_catcher: 0,
            fall_stick: false,
            hero_prev: (0, 0),
            target_lock: [0; 4],
            entered: 0,
            exited: 0,
            look_timer: 0,
            looking: 0,
            last_hero: None,
        }
    }
    /// The rendered viewpoint: CameraController's transform, CameraParent's
    /// shake included.
    pub fn position(&self) -> (i32, i32) {
        let offset = unsafe { (*(&raw const SHAKER)).offset };
        (self.position.0 + offset.0, self.position.1 + offset.1)
    }
    /// One 60 Hz tick: CameraLockArea triggers (FixedUpdate), HeroController's
    /// look state and CameraTarget.Update (Update), CameraController.LateUpdate.
    /// `scene_ticks` counts ticks since the scene loaded (lock lifetimes).
    #[inline(never)]
    #[optimize(size)]
    pub fn tick(
        &mut self,
        region: &crate::world::Region,
        hero: &Hero,
        look: LookInput,
        scene_ticks: u32,
        broken: &dyn Fn(usize) -> bool,
    ) {
        advance_shake();
        self.look(look);
        let h = (hero.x, hero.y);
        if self.scene != region.scene as u16 || self.last_hero.is_none() {
            self.enter_scene(region, hero, broken);
        } else if self
            .last_hero
            .is_some_and(|p| (p.0 - h.0).abs() > SNAP_DISTANCE || (p.1 - h.1).abs() > SNAP_DISTANCE)
        {
            // A hazard respawn: the death froze the camera, the Knight's
            // triggers fire where he lands, then CameraRepositionToHero.
            self.mode = Mode::Frozen;
            self.triggers(region, hero, scene_ticks, broken);
            self.position_to_hero(hero);
        } else {
            self.triggers(region, hero, scene_ticks, broken);
        }
        // DoPositionToHero resumes after the physics step, so the frame it
        // positions still runs CameraTarget.Update and a (FROZEN) LateUpdate.
        self.update_target(hero);
        self.update_camera(hero);
        self.last_hero = Some(h);
        publish(self.position);
        unsafe {
            HK_CAMERA_LOCK = if self.current == NONE {
                0
            } else {
                self.areas[self.current as usize].id
            };
        }
    }
    /// The room-entry seat before the first frame renders: a new scene (or the
    /// first one) runs the scene entry now; inside one scene it does nothing.
    #[inline(never)]
    #[optimize(size)]
    pub fn seat(
        &mut self,
        region: &crate::world::Region,
        hero: &Hero,
        broken: &dyn Fn(usize) -> bool,
    ) {
        if self.scene != region.scene as u16 || self.last_hero.is_none() {
            self.enter_scene(region, hero, broken);
            self.last_hero = Some((hero.x, hero.y));
            publish(self.position);
        }
    }
    /// HeroController.Update's look block, which runs while `hero_state` is
    /// idle, and the ResetLook calls around it.
    #[inline(never)]
    #[optimize(size)]
    fn look(&mut self, input: LookInput) {
        if input.reset || (self.looking != 0 && input.moving) {
            self.looking = 0;
            self.look_timer = 0;
        }
        if !input.idle {
            return;
        }
        if input.up || input.down {
            if (self.looking > 0) != input.up && self.looking != 0 {
                self.looking = 0;
            }
            if self.look_timer >= LOOK_DELAY_TICKS {
                self.looking = if input.up { 1 } else { -1 };
            } else {
                self.look_timer += 1;
            }
        } else {
            self.looking = 0;
            self.look_timer = 0;
        }
    }
    /// GetTilemapInfo, both SceneInits, the hero placed (his trigger enters
    /// inside `startLockedTimer` lock instantly), then DoPositionToHero.
    #[inline(never)]
    #[optimize(size)]
    fn enter_scene(
        &mut self,
        region: &crate::world::Region,
        hero: &Hero,
        broken: &dyn Fn(usize) -> bool,
    ) {
        // OnLevelUnload: ReleaseLock on lockZoneList[0] until it is empty, with
        // the old scene's limits (the target's damping and slow timer carry on).
        while let Some(first) = (0..MAX_AREAS)
            .filter(|&s| self.areas[s].listed != 0)
            .min_by_key(|&s| self.areas[s].listed)
        {
            self.release_lock(first, hero);
        }
        let [w, h] = crate::world::SCENE_CAMERA[region.scene];
        self.scene = region.scene as u16;
        self.limit = (w as i32 * ONE - X_MIN, h as i32 * ONE - Y_MIN);
        self.scene_lock_count = 0;
        crate::world::camera_lock_objects(|local, bounds| {
            let n = self.scene_lock_count as usize;
            if n < MAX_SCENE_LOCKS {
                self.scene_locks[n] = local;
                self.scene_lock_bounds[n] = bounds;
                self.scene_lock_count += 1;
            }
        });
        // OnLevelUnload released every lock; the gate froze the camera.
        self.areas = [Area::EMPTY; MAX_AREAS];
        self.current = NONE;
        self.mode = Mode::Frozen;
        self.lock = [0, self.limit.0, 0, self.limit.1];
        self.start_locked = START_LOCKED_TICKS;
        self.target_mode = TargetMode::Follow;
        self.stick = (true, true);
        self.fall_catcher = 0;
        self.fall_stick = false;
        self.target_lock = [0, self.limit.0, 0, self.limit.1];
        cancel_shake();
        self.triggers(region, hero, 0, broken);
        self.position_to_hero(hero);
    }
    /// The trigger callbacks for the Knight's body this tick: enters, then
    /// exits, then stays (a stay re-runs LockToArea, which only acts on a lock
    /// not yet listed).
    #[inline(never)]
    #[optimize(size)]
    fn triggers(
        &mut self,
        region: &crate::world::Region,
        hero: &Hero,
        scene_ticks: u32,
        broken: &dyn Fn(usize) -> bool,
    ) {
        let mut now = [0u32; MAX_AREAS];
        let mut count = 0;
        let body = hero.body;
        for i in 0..self.scene_lock_count as usize {
            let b = self.scene_lock_bounds[i];
            if !(b[0] <= body[2] && b[2] >= body[0] && b[1] <= body[3] && b[3] >= body[1]) {
                continue;
            }
            let Some(lock) = crate::world::camera_lock(self.scene_locks[i]) else {
                continue;
            };
            let known = self
                .areas
                .iter()
                .position(|a| a.id == lock.id && (a.inside || a.listed != 0));
            // Crossroads_01's `Disable` lifetime, and a battle lock outside its
            // fight: the lock is switched off, and OnDisable releases it with no
            // trigger exit.
            if (lock.expires != 0 && scene_ticks >= lock.expires as u32)
                || (lock.flags & LOCK_BATTLE != 0 && !crate::battle_gates::fighting(region.scene))
            {
                if let Some(slot) = known {
                    self.areas[slot].inside = false;
                    self.release_lock(slot, hero);
                }
                continue;
            }
            if lock.owner.is_some_and(broken) || !lock.touches(hero.body) {
                continue;
            }
            let Some(slot) =
                known.or_else(|| self.areas.iter().position(|a| !a.inside && a.listed == 0))
            else {
                continue;
            };
            if count < MAX_AREAS {
                now[count] = lock.id;
                count += 1;
            }
            if !self.areas[slot].inside {
                self.areas[slot] = Area {
                    id: lock.id,
                    trigger: lock.bounds,
                    limits: view_limits(lock.limits),
                    flags: lock.flags,
                    inside: true,
                    listed: self.areas[slot].listed,
                };
                self.entered = sides(lock.bounds, hero.x, hero.y);
                self.lock_to_area(slot, hero);
            }
        }
        for slot in 0..MAX_AREAS {
            if self.areas[slot].inside && !now[..count].contains(&self.areas[slot].id) {
                self.areas[slot].inside = false;
                self.exited = sides(self.areas[slot].trigger, hero.x, hero.y);
                self.release_lock(slot, hero);
            }
        }
        for slot in 0..MAX_AREAS {
            if self.areas[slot].inside {
                self.lock_to_area(slot, hero);
            }
        }
    }
    /// CameraController.LockToArea.
    #[inline(never)]
    #[optimize(size)]
    fn lock_to_area(&mut self, slot: usize, hero: &Hero) {
        if self.areas[slot].listed != 0 {
            return;
        }
        self.order += 1;
        self.areas[slot].listed = self.order;
        if self.current != NONE
            && self.areas[self.current as usize].flags & LOCK_MAX_PRIORITY != 0
            && self.areas[slot].flags & LOCK_MAX_PRIORITY == 0
        {
            return;
        }
        self.current = slot as u8;
        self.set_mode(Mode::Locked);
        let l = self.areas[slot].limits;
        self.lock = [
            if l[0] < 0 { X_MIN } else { l[0] },
            if l[1] < 0 { self.limit.0 } else { l[1] },
            if l[2] < 0 { Y_MIN } else { l[2] },
            if l[3] < 0 { self.limit.1 } else { l[3] },
        ];
        if self.start_locked > 0 {
            let p = clamp_box((hero.x, hero.y), self.lock);
            self.target = p;
            self.enter_lock_zone_instant(self.lock);
            self.position = p;
        } else {
            self.enter_lock_zone(self.lock, hero);
        }
    }
    /// CameraController.ReleaseLock: the most recently listed lock left takes
    /// over, with its raw (validated) limits.
    #[inline(never)]
    #[optimize(size)]
    fn release_lock(&mut self, slot: usize, hero: &Hero) {
        self.areas[slot].listed = 0;
        if self.current as usize != slot {
            return;
        }
        let next = (0..MAX_AREAS)
            .filter(|&s| self.areas[s].listed != 0)
            .max_by_key(|&s| self.areas[s].listed);
        if let Some(next) = next {
            self.current = next as u8;
            self.lock = self.areas[next].limits;
            self.enter_lock_zone(self.lock, hero);
        } else {
            self.exit_lock_zone(hero);
            self.current = NONE;
            self.set_mode(Mode::Following);
        }
    }
    fn set_mode(&mut self, mode: Mode) {
        if mode != self.mode {
            self.prev_mode = self.mode;
            self.mode = mode;
        }
    }
    /// CameraTarget.EnterLockZone: slow damping on an axis unless the Knight came
    /// in over a side whose limit is the scene's own.
    #[inline(never)]
    #[optimize(size)]
    fn enter_lock_zone(&mut self, bounds: [i32; 4], hero: &Hero) {
        self.target_lock = bounds;
        self.target_mode = TargetMode::Lock;
        self.slow_down(self.entered, bounds, hero);
    }
    /// EnterLockZone and ExitLockZone's shared tail: 0.5 s slow damping on an
    /// axis unless the Knight crossed a side whose limit is the scene's own,
    /// the slow timer, and stickToHero only where the target already is on him.
    #[inline(never)]
    #[optimize(size)]
    fn slow_down(&mut self, sides: u8, b: [i32; 4], hero: &Hero) {
        if (sides & LEFT == 0 || b[0] != X_MIN) && (sides & RIGHT == 0 || b[1] != self.limit.0) {
            self.damp.0 = DAMP_SLOW;
        }
        if (sides & BOT == 0 || b[2] != Y_MIN) && (sides & TOP == 0 || b[3] != self.limit.1) {
            self.damp.1 = DAMP_SLOW;
        }
        self.slow = SLOW_TICKS;
        self.stick = (
            (self.target.0 - hero.x).abs() <= SNAP,
            (self.target.1 - hero.y).abs() <= SNAP,
        );
    }
    fn enter_lock_zone_instant(&mut self, bounds: [i32; 4]) {
        self.target_lock = bounds;
        self.target_mode = TargetMode::Lock;
        self.target = clamp_box(self.target, bounds);
        self.stick = (true, true);
    }
    /// CameraTarget.ExitLockZone.
    #[inline(never)]
    #[optimize(size)]
    fn exit_lock_zone(&mut self, hero: &Hero) {
        if self.target_mode == TargetMode::Free {
            return;
        }
        self.target_mode = TargetMode::Follow;
        self.slow_down(self.exited, self.target_lock, hero);
        self.fall_stick = false;
        self.target_lock = [0, self.limit.0, 0, self.limit.1];
    }
    /// CameraTarget.SetDampTime.
    fn set_damp_time(&mut self) {
        if self.slow > 0 {
            self.slow -= 1;
            return;
        }
        for damp in [&mut self.damp.0, &mut self.damp.1] {
            if *damp > DAMP_NORMAL {
                *damp -= DAMP_STEP;
            } else if *damp < DAMP_NORMAL {
                *damp = DAMP_NORMAL;
            }
        }
    }
    fn keep_within_scene(&self, p: (i32, i32)) -> (i32, i32) {
        clamp_box(p, [X_MIN, self.limit.0, Y_MIN, self.limit.1])
    }
    /// CameraTarget.Update.
    #[inline(never)]
    #[optimize(size)]
    fn update_target(&mut self, hero: &Hero) {
        let h = (hero.x, hero.y);
        let lock = self.target_lock;
        let locked = self.target_mode == TargetMode::Lock;
        if self.target_mode != TargetMode::Free {
            self.set_damp_time();
            let destination = if locked { clamp_box(h, lock) } else { h };
            let x = smooth_damp(
                self.target.0,
                destination.0,
                &mut self.target_velocity.0,
                self.damp.0,
            );
            let y = if !self.fall_stick && self.fall_catcher <= 0 {
                smooth_damp(
                    self.target.1,
                    destination.1,
                    &mut self.target_velocity.1,
                    self.damp.1,
                )
            } else {
                self.target.1
            };
            self.target = (x, y);
            // stickToHero: the target caught (or crossed) the Knight on an axis.
            let caught = |prev: i32, now: i32, t: i32| {
                (prev < t && now > t)
                    || (prev > t && now < t)
                    || (t >= now - SNAP && t <= now + SNAP)
            };
            if caught(self.hero_prev.0, h.0, x) {
                self.stick.0 = true;
            }
            if caught(self.hero_prev.1, h.1, y) {
                self.stick.1 = true;
            }
            // In a lock, only while the Knight is inside its limits or the
            // target moves toward them.
            let holds = |now: i32, t: i32, low: i32, high: i32| {
                !locked
                    || (now >= low && now <= high)
                    || (now <= high && now >= t)
                    || (now >= low && now <= t)
            };
            if self.stick.0 && holds(h.0, x, lock[0], lock[1]) {
                self.target.0 = h.0;
            }
            if self.stick.1 && holds(h.1, y, lock[2], lock[3]) {
                self.target.1 = h.1;
            }
        }
        let tx = self.target.0;
        let facing_right = hero.facing > 0;
        if facing_right {
            if self.x_offset < LOOK_AHEAD {
                self.x_offset += LOOK_AHEAD_STEP;
            }
        } else if self.x_offset > -LOOK_AHEAD {
            self.x_offset -= LOOK_AHEAD_STEP;
        }
        self.x_offset = self.x_offset.clamp(-LOOK_AHEAD, LOOK_AHEAD);
        if locked {
            if h.0 < lock[0] && facing_right {
                self.x_offset = h.0 - tx + ONE;
            }
            if h.0 > lock[1] && !facing_right {
                self.x_offset = h.0 - tx - ONE;
            }
            if tx + self.x_offset > lock[1] {
                self.x_offset = lock[1] - tx;
            }
            if tx + self.x_offset < lock[0] {
                self.x_offset = lock[0] - tx;
            }
            self.x_offset = self.x_offset.clamp(-LOOK_AHEAD, LOOK_AHEAD);
        }
        self.dash_offset = 0;
        if hero.dashing || hero.super_dashing {
            let ahead = if hero.dashing {
                DASH_LOOK_AHEAD
            } else {
                SUPER_DASH_LOOK_AHEAD
            };
            self.dash_offset = if facing_right { ahead } else { -ahead };
            if locked
                && (tx + self.dash_offset > lock[1]
                    || tx + self.dash_offset < lock[0]
                    || h.0 > lock[1]
                    || h.0 < lock[0])
            {
                self.dash_offset = 0;
            }
        }
        self.hero_prev = h;
        if !hero.falling {
            self.fall_catcher = 0;
            self.fall_stick = false;
        }
        if self.target_mode == TargetMode::Free {
            return;
        }
        // The fall catcher moves the camera itself (its world transform).
        let shake = unsafe { (*(&raw const SHAKER)).offset.1 };
        let mut camera_y = self.position.1 + shake;
        let floor = |y: i32| {
            let y = if locked && y < lock[2] { lock[2] } else { y };
            y.max(Y_MIN)
        };
        if hero.falling
            && camera_y > h.1 + FALL_STICK
            && !self.fall_stick
            && !hero.transitioning
            && !(locked && camera_y - FALL_STICK < lock[2])
        {
            camera_y = floor(camera_y - self.fall_catcher / 60);
            if self.fall_catcher < FALL_CATCH_MAX {
                self.fall_catcher += FALL_CATCH_STEP;
            }
            if camera_y < h.1 + FALL_STICK {
                self.fall_stick = true;
            }
            self.target.1 = camera_y;
        }
        if self.fall_stick {
            self.fall_catcher = 0;
            if !(locked && h.1 + FALL_STICK < lock[2]) {
                camera_y = h.1 + FALL_STICK;
                self.target.1 = camera_y;
            }
            camera_y = floor(camera_y);
        }
        self.position.1 = camera_y - shake;
    }
    /// CameraController.LateUpdate. CameraParent's shake is part of the world
    /// position the SmoothDamp starts from, as in the source.
    #[inline(never)]
    #[optimize(size)]
    fn update_camera(&mut self, hero: &Hero) {
        let offset = unsafe { (*(&raw const SHAKER)).offset };
        let mut world = (self.position.0 + offset.0, self.position.1 + offset.1);
        if self.frozen > 0 {
            self.frozen -= 1;
            if self.frozen == 0 {
                // DoPositionToHero's end: back to the mode it found.
                let mode = match self.prev_mode {
                    Mode::Frozen => Mode::Following,
                    Mode::Locked if self.current == NONE => Mode::Following,
                    mode => mode,
                };
                self.mode = mode;
            }
        } else if self.mode != Mode::Frozen {
            let look = match self.looking {
                1 => hero.y - self.target.1 + LOOK_OFFSET,
                -1 => hero.y - self.target.1 - LOOK_OFFSET,
                _ => 0,
            };
            let mut destination = (
                self.target.0 + self.x_offset + self.dash_offset,
                self.target.1 + look,
            );
            if self.mode == Mode::Locked && self.current != NONE {
                let area = self.areas[self.current as usize];
                if look > 0
                    && area.flags & LOCK_PREVENT_LOOK_UP != 0
                    && destination.1 > area.limits[3]
                {
                    destination.1 = if world.1 > area.limits[3] {
                        destination.1 - look
                    } else {
                        area.limits[3]
                    };
                }
                if look < 0
                    && area.flags & LOCK_PREVENT_LOOK_DOWN != 0
                    && destination.1 < area.limits[2]
                {
                    destination.1 = if world.1 < area.limits[2] {
                        destination.1 - look
                    } else {
                        area.limits[2]
                    };
                }
            }
            let destination = self.keep_within_scene(destination);
            world.0 = smooth_damp(world.0, destination.0, &mut self.velocity.0, CAMERA_DAMP);
            world.1 = smooth_damp(world.1, destination.1, &mut self.velocity.1, CAMERA_DAMP);
        }
        // The scene clamp, which tests the world position plus the parent's.
        if world.0 + offset.0 < X_MIN {
            world.0 = X_MIN;
        }
        if world.0 + offset.0 > self.limit.0 {
            world.0 = self.limit.0;
        }
        if world.1 + offset.1 < Y_MIN {
            world.1 = Y_MIN;
        }
        if world.1 + offset.1 > self.limit.1 {
            world.1 = self.limit.1;
        }
        self.position = (world.0 - offset.0, world.1 - offset.1);
        self.start_locked = self.start_locked.saturating_sub(1);
    }
    /// CameraTarget.PositionToStart, then CameraController.DoPositionToHero
    /// (without its FixedUpdate wait: the Knight is already placed).
    #[inline(never)]
    #[optimize(size)]
    fn position_to_hero(&mut self, hero: &Hero) {
        let h = (hero.x, hero.y);
        let facing_right = hero.facing > 0;
        let old_x = self.target.0;
        self.target_velocity = (0, 0);
        self.x_offset = if facing_right { ONE } else { -ONE };
        let lock = self.target_lock;
        if self.target_mode == TargetMode::Lock {
            if h.0 < lock[0] && facing_right {
                self.x_offset = h.0 - old_x + ONE;
            }
            if h.0 > lock[1] && !facing_right {
                self.x_offset = h.0 - old_x - ONE;
            }
            if old_x + self.x_offset > lock[1] {
                self.x_offset = lock[1] - old_x;
            }
            if old_x + self.x_offset < lock[0] {
                self.x_offset = lock[0] - old_x;
            }
        }
        self.x_offset = self.x_offset.clamp(-LOOK_AHEAD, LOOK_AHEAD);
        match self.target_mode {
            TargetMode::Follow => self.target = self.keep_within_scene(h),
            TargetMode::Lock => self.target = clamp_box(h, lock),
            TargetMode::Free => {}
        }
        self.hero_prev = h;
        let delta_x = self.target.0 + self.x_offset + self.dash_offset;
        self.prev_mode = self.mode;
        self.mode = Mode::Frozen;
        self.frozen = FROZEN_TICKS;
        let new = self.keep_within_scene(self.target);
        let ahead = if facing_right { ONE } else { -ONE };
        self.position = if self.current != NONE {
            clamp_box((new.0 + self.x_offset, new.1), self.lock)
        } else {
            (new.0 + ahead, new.1)
        };
        let left = new.0 <= X_MIN;
        if left || new.0 >= self.limit.0 {
            // At a horizontal scene bound: facing it, or the look-ahead would
            // still touch a side, the camera stays on the bound.
            if left != facing_right || delta_x <= X_MIN || delta_x >= self.limit.0 {
                self.position = new;
            } else {
                self.position = (h.0 + ahead, new.1);
            }
        }
        self.velocity = (0, 0);
    }
}
