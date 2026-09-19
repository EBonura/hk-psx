//! The actual guest camera module, with only `world` replaced. `CameraShake`
//! keeps its state in module statics because the one camera is a global, so
//! every assertion that touches it runs inside one test.
pub const ONE: i32 = 65536;
pub mod world {
    pub struct Region {
        pub camera: [i32; 4],
    }
    pub struct CameraLock {
        pub bounds: [i32; 4],
        pub limits: [i32; 4],
    }
    /// No lock in the fixture: the camera clamps to the view bounds instead.
    pub fn camera_locks(_region: &Region) -> impl Iterator<Item = CameraLock> {
        core::iter::empty()
    }
}
#[path = "../game/src/camera.rs"]
mod camera;
use camera::{Camera, Shake};

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

#[test]
fn shake_displaces_decays_arbitrates_and_stops() {
    let limits = [-1_000_000, -1_000_000, 1_000_000, 1_000_000];
    let hero = (30 * ONE, 30 * ONE);
    let mut c = Camera::new();
    c.snap(hero, 1, limits);
    let rest = c.position();

    // Idle: the rendered viewpoint is the follow camera untouched.
    for _ in 0..10 {
        c.tick(hero, 1, limits);
        assert_eq!(c.position(), rest);
    }

    // BigShake: the first tick after the request still sits at no offset,
    // because ShakePositionV2's OnExit restores the position before OnEnter.
    camera::request(Shake::Big);
    assert_eq!(c.position(), rest);
    let (extents, ticks, _) = camera::shake_spec(Shake::Big);
    let mut peaks = [0i32; 60];
    let mut moved = 0;
    for tick in 0..ticks as usize {
        c.tick(hero, 1, limits);
        let (x, y) = c.position();
        let (dx, dy) = (x - rest.0, y - rest.1);
        assert!(dx.abs() <= extents && dy.abs() <= extents, "tick {tick} left the extents");
        peaks[tick] = dx.abs().max(dy.abs());
        moved += i32::from(peaks[tick] != 0);
    }
    assert!(moved > ticks as i32 * 3 / 4, "only {moved} of {ticks} ticks displaced anything");
    // Clamp01(1 - timer / Duration): the tail is quieter than the head.
    let head = peaks[..15].iter().copied().max().unwrap();
    let tail = peaks[45..60].iter().copied().max().unwrap();
    assert!(tail * 3 < head, "no decay: head {head}, tail {tail}");
    // DoneShaking returns to Normal and puts the parent back.
    c.tick(hero, 1, limits);
    assert_eq!(c.position(), rest);

    // Priority: a weaker shake is swallowed while a stronger one runs.
    camera::request(Shake::Big);
    for _ in 0..5 {
        c.tick(hero, 1, limits);
    }
    let refused = unsafe { camera::HK_CAMERA_SHAKES_REFUSED };
    for weaker in [Shake::Small, Shake::Kill, Shake::Average, Shake::Big] {
        camera::request(weaker);
    }
    assert_eq!(unsafe { camera::HK_CAMERA_SHAKES_REFUSED }, refused + 4,
               "an equal or weaker shake must not retrigger");
    // Let the Big run out, then prove a stronger shake preempts a weaker one
    // and that the swapped state starts from no offset.
    for _ in 0..ticks {
        c.tick(hero, 1, limits);
    }
    assert_eq!(c.position(), rest);
    camera::request(Shake::Small);
    for _ in 0..5 {
        c.tick(hero, 1, limits);
    }
    let started = unsafe { camera::HK_CAMERA_SHAKES };
    camera::request(Shake::Average);
    assert_eq!(unsafe { camera::HK_CAMERA_SHAKES }, started + 1);
    assert_eq!(c.position(), rest);
    c.tick(hero, 1, limits);
    assert_ne!(c.position(), rest);

    // LEVEL LOADED: a gate or respawn snap cancels whatever was running.
    camera::request(Shake::Big);
    c.tick(hero, 1, limits);
    assert_ne!(c.position(), rest);
    c.snap(hero, 1, limits);
    assert_eq!(c.position(), rest);
    for _ in 0..90 {
        c.tick(hero, 1, limits);
        assert_eq!(c.position(), rest, "a cancelled shake came back");
    }
}
