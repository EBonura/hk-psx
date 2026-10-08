//! The actual guest camera module, with only `world` replaced. `CameraShake`
//! keeps its state in module statics because the one camera is a global, so
//! every assertion that touches it runs inside one test.
#![feature(optimize_attribute)]
pub const ONE: i32 = 65536;
pub const KNIGHT_SCALE: i32 = 60693;
/// The PS1 view's half extents at KNIGHT_SCALE.
const HW: f64 = 160.0 * 4096.0 / 60693.0;
const HH: f64 = 120.0 * 4096.0 / 60693.0;
pub mod world {
    pub struct Region {
        pub scene: usize,
        pub camera: [i32; 4],
    }
    /// Scene 0: a 100 x 40 tilemap (camera centre 14.6..85.4, 8.3..31.7).
    pub static SCENE_CAMERA: &[[u16; 2]] = &[[100, 40]];
    #[derive(Clone, Copy)]
    pub struct CameraLock {
        pub id: u32,
        pub bounds: [i32; 4],
        pub limits: [i32; 4],
        pub flags: u16,
        pub owner: Option<usize>,
        pub expires: u16,
    }
    impl CameraLock {
        pub fn touches(&self, body: [i32; 4]) -> bool {
            let b = self.bounds;
            b[0] <= body[2] && b[2] >= body[0] && b[1] <= body[3] && b[3] >= body[1]
        }
    }
    pub static mut LOCKS: [Option<CameraLock>; 4] = [None; 4];
    pub fn camera_lock_objects(mut each: impl FnMut(u16, [i32; 4])) {
        for (i, lock) in unsafe { (*(&raw const LOCKS)).iter().enumerate() } {
            if let Some(lock) = lock {
                each(i as u16, lock.bounds);
            }
        }
    }
    pub fn camera_lock(local: u16) -> Option<CameraLock> {
        unsafe { (*(&raw const LOCKS))[local as usize] }
    }
}
pub mod battle_gates {
    pub static mut FIGHTING: bool = false;
    pub fn fighting(_scene: usize) -> bool {
        unsafe { FIGHTING }
    }
}
#[path = "../game/src/camera.rs"]
mod camera;
use camera::{Camera, Hero, LookInput, Shake};

const NO_LOOK: LookInput = LookInput { idle: false, up: false, down: false, moving: false, reset: false };
const REGION: world::Region = world::Region { scene: 0, camera: [0; 4] };

fn q(units: f64) -> i32 {
    (units * 65536.0).round() as i32
}
fn units(q16: i32) -> f64 {
    q16 as f64 / 65536.0
}
fn hero(x: f64, y: f64, facing: i32) -> Hero {
    let (x, y) = (q(x), q(y));
    Hero { x, y, body: [x - q(0.25), y - q(1.39), x + q(0.25), y - q(0.11)], facing, dashing: false, super_dashing: false, falling: false, transitioning: false }
}
fn tick(c: &mut Camera, h: &Hero) {
    c.tick(&REGION, h, NO_LOOK, 1000, &|_| false);
}
fn set_locks(locks: &[world::CameraLock]) {
    unsafe {
        let all = &mut *(&raw mut world::LOCKS);
        *all = [None; 4];
        for (slot, lock) in all.iter_mut().zip(locks) {
            *slot = Some(*lock);
        }
    }
}

/// 14.8178 screen pixels per world unit, the projection the room art is cooked
/// at (`FOCAL / -CAM_Z`, and `KNIGHT_SCALE` 60693 in the guest).
fn pixels(units_q16: i32) -> f64 {
    units_q16 as f64 / 65536.0 * 14.8178
}

/// Extents, duration and priority exactly as `resources.assets:22920` serializes
/// them, so a typo in the table fails here rather than on the disc.
#[test]
fn shake_table_matches_the_serialized_fsm() {
    for (kind, extent, seconds, priority) in [
        (Shake::Small, 0.07999999821186066_f64, 0.5_f64, 3u8),
        (Shake::Kill, 0.10499999672174454, 0.5, 6),
        (Shake::Average, 0.15000000596046448, 1.0, 7),
        (Shake::Big, 0.5, 1.0, 10),
    ] {
        let (extents, ticks, live) = camera::shake_spec(kind);
        assert_eq!(extents, (extent * 65536.0).round() as i32, "{kind:?} extents");
        assert_eq!(ticks, (seconds * 60.0) as u16, "{kind:?} duration");
        assert_eq!(live, priority, "{kind:?} priority");
    }
    // The biggest shake has to be worth looking at on a 320x240 frame.
    assert!((7.0..8.0).contains(&pixels(camera::shake_spec(Shake::Big).0)));
}

/// Unity's Vector3.SmoothDamp on one axis (infinite maxSpeed, dt 1/60), in f64.
fn smooth_damp(current: f64, target: f64, velocity: &mut f64, smooth: f64) -> f64 {
    let omega = 2.0 / smooth;
    let x = omega / 60.0;
    let exp = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) / 60.0;
    *velocity = (*velocity - omega * temp) * exp;
    let output = target + (change + temp) * exp;
    if (target - current) * (output - target) > 0.0 {
        *velocity = 0.0;
        return target;
    }
    output
}

/// Everything that touches the module's statics (the shake, the stub locks)
/// runs in this one test, in order.
#[test]
fn camera_follows_clamps_locks_and_shakes_like_the_source() {
    set_locks(&[]);
    // The Knight walks right at RUN_SPEED from the middle of the room: the
    // target sticks to him, the camera chases target + 1 (look-ahead) with
    // a 0.15 s SmoothDamp. Compare against the float spring.
    let mut c = Camera::new();
    let mut x = 40.0;
    tick(&mut c, &hero(x, 20.0, 1));
    let start = units(c.position().0);
    assert!((start - 41.0).abs() < 1e-3, "DoPositionToHero puts the camera one unit ahead, at {start}");
    for _ in 0..5 {
        tick(&mut c, &hero(x, 20.0, 1)); // the 0.1 s FROZEN hold
    }
    let (mut reference, mut velocity) = (start, 0.0);
    let mut worst: f64 = 0.0;
    for _ in 0..120 {
        x += 8.3 / 60.0;
        tick(&mut c, &hero(x, 20.0, 1));
        reference = smooth_damp(reference, x + 1.0, &mut velocity, 0.15);
        worst = worst.max((units(c.position().0) - reference).abs());
        assert!((units(c.position().1) - 20.0).abs() < 1e-3, "free follow keeps the camera on the Knight's height");
    }
    assert!(worst < 0.01, "fixed-point SmoothDamp strays {worst} from the float spring");
    // No speed limit: maxVelocity only ever clamps a field nothing writes.
    // Walk to the right wall: the camera stops at width - 14.6 while he goes on.
    for _ in 0..600 {
        x = (x + 8.3 / 60.0).min(99.0);
        tick(&mut c, &hero(x, 20.0, 1));
        assert!(units(c.position().0) <= 100.0 - HW + 1e-4);
    }
    assert!((units(c.position().0) - (100.0 - HW)).abs() < 1e-3, "the view's edge rests on the wall, not on the Knight");
    // Down at the floor and left to the far wall: 8.3 and 14.6 hold too.
    for _ in 0..1200 {
        x = (x - 8.3 / 60.0).max(1.0);
        tick(&mut c, &hero(x, 2.0, -1));
    }
    assert!((units(c.position().0) - HW).abs() < 1e-3 && (units(c.position().1) - HH).abs() < 1e-3,
            "corner bounds, got {:?}", (units(c.position().0), units(c.position().1)));

    // Lock areas: A spans x 20..60 and pins the camera to y 15; B (maxPriority)
    // overlaps its right part and pins x to 50; C overlaps B and is ordinary.
    let lock = |id, x0: f64, x1: f64, limits: [f64; 4], flags| world::CameraLock {
        id, bounds: [q(x0), q(0.0), q(x1), q(40.0)], limits: limits.map(q), flags, owner: None, expires: 0 };
    let a = lock(1, 20.0, 60.0, [14.6, 85.4, 15.0, 15.0], 0);
    let b = lock(2, 45.0, 70.0, [50.0, 50.0, 8.3, 31.7], camera::LOCK_MAX_PRIORITY);
    let cc = lock(3, 55.0, 80.0, [14.6, 85.4, 25.0, 25.0], 0);
    set_locks(&[a, b, cc]);
    let mut c = Camera::new(); // reads the scene's locks on entering it
    let mut x = 10.0;
    tick(&mut c, &hero(x, 12.0, 1));
    let walk = |c: &mut Camera, x: &mut f64, to: f64| {
        while *x < to {
            *x += 8.3 / 60.0;
            tick(c, &hero(*x, 12.0, 1));
        }
        for _ in 0..240 {
            tick(c, &hero(*x, 12.0, 1));
        }
    };
    walk(&mut c, &mut x, 35.0);
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 1);
    // A pins the original's camera to y 15; the PS1's view keeps the same
    // edges, so it may sit anywhere in 15 -+ (8.3 - HH), here at the bottom.
    assert!((units(c.position().1) - (15.0 - (8.3 - HH))).abs() < 1e-3, "A's view edges, got {}", units(c.position().1));
    walk(&mut c, &mut x, 57.0);
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 2, "B takes over (most recent)");
    assert!((units(c.position().0) - (50.0 + (14.6 - HW))).abs() < 1e-3, "B's view edges at x 50 -+ 3.8, got {}", units(c.position().0));
    walk(&mut c, &mut x, 65.0);
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 2, "C does not displace a maxPriority lock");
    walk(&mut c, &mut x, 72.0);
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 3, "leaving B hands over to the latest listed lock, C");
    assert!((units(c.position().1) - (25.0 - (8.3 - HH))).abs() < 1e-3, "C's view edges, got {}", units(c.position().1));
    walk(&mut c, &mut x, 82.0);
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 0);
    set_locks(&[]);

    // Shake: rides on CameraParent, so the rendered position moves by its
    // displacement and the follow underneath is fed from the shaken position.
    let mut c = Camera::new();
    let h = hero(50.0, 20.0, 1);
    for _ in 0..60 {
        tick(&mut c, &h);
    }
    let rest = c.position();
    camera::request(Shake::Big);
    assert_eq!(c.position(), rest, "the first tick after a request sits at no offset");
    let (extents, ticks, _) = camera::shake_spec(Shake::Big);
    let mut moved = 0;
    for _ in 0..ticks {
        tick(&mut c, &h);
        let (dx, dy) = (c.position().0 - rest.0, c.position().1 - rest.1);
        assert!(dx.abs() <= 2 * extents && dy.abs() <= 2 * extents);
        moved += i32::from(dx != 0 || dy != 0);
    }
    assert!(moved > ticks as i32 * 3 / 4, "only {moved} of {ticks} ticks displaced anything");
    for _ in 0..120 {
        tick(&mut c, &h);
    }
    let settled = c.position();
    assert!((settled.0 - rest.0).abs() <= 16 && (settled.1 - rest.1).abs() <= 16, "the follow settles back after the shake: {settled:?} vs {rest:?}");
    // Priority: a weaker shake is swallowed while a stronger one runs.
    camera::request(Shake::Big);
    for _ in 0..5 {
        tick(&mut c, &h);
    }
    let refused = unsafe { camera::HK_CAMERA_SHAKES_REFUSED };
    for weaker in [Shake::Small, Shake::Kill, Shake::Average, Shake::Big] {
        camera::request(weaker);
    }
    assert_eq!(unsafe { camera::HK_CAMERA_SHAKES_REFUSED }, refused + 4,
               "an equal or weaker shake must not retrigger");

    // A Battle Control's lock pins the camera only while its arena is sealed.
    let battle = lock(4, 20.0, 80.0, [40.0, 40.0, 8.3, 31.7], camera::LOCK_BATTLE);
    set_locks(&[battle]);
    let mut c = Camera::new();
    let h = hero(60.0, 20.0, 1);
    for _ in 0..120 {
        tick(&mut c, &h);
    }
    assert!((units(c.position().0) - 61.0).abs() < 1e-2, "no fight, no lock: {}", units(c.position().0));
    unsafe { battle_gates::FIGHTING = true };
    for _ in 0..240 {
        tick(&mut c, &h);
    }
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 4);
    assert!((units(c.position().0) - (40.0 + (14.6 - HW))).abs() < 1e-3, "the fight's lock holds the view's right edge: {}", units(c.position().0));
    unsafe { battle_gates::FIGHTING = false };
    for _ in 0..240 {
        tick(&mut c, &h);
    }
    assert_eq!(unsafe { camera::HK_CAMERA_LOCK }, 0, "BG OPEN switches the lock off");
    set_locks(&[]);
    // Super dash: superDashLookAhead aims six units ahead while it travels.
    let mut c = Camera::new();
    let mut h = hero(50.0, 20.0, 1);
    tick(&mut c, &h);
    h.super_dashing = true;
    for _ in 0..120 {
        tick(&mut c, &h);
    }
    assert!((units(c.position().0) - 57.0).abs() < 1e-2, "1 look-ahead + 6 super dash: {}", units(c.position().0));

    // Look up: 52 standing frames holding up count the delay, the 53rd looks
    // 6 units up (the scene bound still applies).
    let mut c = Camera::new();
    let h = hero(50.0, 15.0, 1);
    tick(&mut c, &h);
    for _ in 0..30 {
        tick(&mut c, &h);
    }
    let level = units(c.position().1);
    let up = LookInput { idle: true, up: true, down: false, moving: false, reset: false };
    for _ in 0..52 {
        c.tick(&REGION, &h, up, 1000, &|_| false);
    }
    assert!((units(c.position().1) - level).abs() < 1e-3, "no look before LOOK_DELAY");
    for _ in 0..120 {
        c.tick(&REGION, &h, up, 1000, &|_| false);
    }
    assert!((units(c.position().1) - 21.0).abs() < 1e-2, "looking up aims 6 units above, got {}", units(c.position().1));
}
