//! The dark slashes the Knight throws out when he is hit or dies.
//!
//! `Knight Damage` (resources.assets, on `Knight/Effects/Damage Effect`) flings two to three
//! `Slash Effect GhostDark1` and as many `GhostDark2` to each side of the Knight on every hit, at
//! 20 to 35 units a second, the left ones between 140 and 220 degrees and the right ones within 40
//! of straight ahead. `Hero Death Anim`'s `Initiate` bursts eight of each in every direction
//! (`SpawnRandomObjectsV2`, angle 0 to 360) as the body breaks up. Each is a tk2d clip that
//! recycles itself when it ends (`RecycleAfter2dtkAnimation`) and turns to face its velocity
//! (`FaceAngleSimple`).
//!
//! One burst at a time: a hit is followed by 79 ticks of invulnerability and a burst lasts about
//! twenty, so a second one only ever replaces the end of the first, and a burst's slashes all
//! share one age, so a frame needs two animation keys, not sixteen. Position is the origin plus the
//! velocity times the age, which keeps each slash to six bytes.
//!
//! Not presented: `Hit Crack` and `Hit Pt 1/2` (Unity Animator and particle systems rather than
//! tk2d), the low-health burst and the leak, the sword and head the death throws.
use crate::ability_art;

/// A slash clip's length in ticks: 18 fps, six frames for the first and five for the second.
/// A burst ends with its longest.
const LIFE_TICKS: u8 = 20;
const MAX_SLASHES: usize = 16;

#[derive(Clone, Copy)]
struct Slash {
    /// Direction of flight, whole degrees.
    angle: i16,
    /// World units a second.
    speed: u8,
    /// 0 is `GhostDark1`, 1 is `GhostDark2`.
    kind: u8,
    /// Sixteenths: the prefab's own scale is baked into the frame, this is the FSM's 1 to 1.75.
    scale: u8,
    /// `Rotate Min/Max` (-30 to 30 degrees) on top of facing the velocity.
    jitter: i8,
}
struct Burst {
    origin: [i32; 2],
    age: u8,
    count: u8,
    slashes: [Slash; MAX_SLASHES],
}
const NONE: Slash = Slash {
    angle: 0,
    speed: 0,
    kind: 0,
    scale: 16,
    jitter: 0,
};
static mut BURST: Burst = Burst {
    origin: [0; 2],
    age: 0,
    count: 0,
    slashes: [NONE; MAX_SLASHES],
};
static mut SEED: u32 = 0x2545_f491;
/// Bursts thrown so far, for the route pins.
#[no_mangle]
pub static mut HK_HIT_FX_BURSTS: u32 = 0;

fn burst() -> &'static mut Burst {
    unsafe { &mut *(&raw mut BURST) }
}
/// A value in `low..=high` from a fixed-seed generator: the same input tape gives the same slashes.
fn random(low: i32, high: i32) -> i32 {
    unsafe {
        SEED = SEED.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        low + ((SEED >> 16) as i32 * (high - low + 1)) / 65536
    }
}
fn push(b: &mut Burst, kind: u8, angle: i32) {
    if (b.count as usize) < MAX_SLASHES {
        b.slashes[b.count as usize] = Slash {
            angle: angle as i16,
            speed: random(20, 35) as u8,
            kind,
            scale: random(16, 28) as u8,
            jitter: random(-30, 30) as i8,
        };
        b.count += 1;
    }
}
fn start(at: [i32; 2]) -> &'static mut Burst {
    let b = burst();
    b.origin = at;
    b.age = 0;
    b.count = 0;
    unsafe {
        HK_HIT_FX_BURSTS = HK_HIT_FX_BURSTS.saturating_add(1);
    }
    b
}
/// `Knight Damage`'s `Gen`: two or three of each kind to the left and to the right.
pub fn hit(at: [i32; 2]) {
    let b = start(at);
    for kind in 0..2 {
        for _ in 0..random(2, 3) {
            push(b, kind, random(140, 220));
        }
        for _ in 0..random(2, 3) {
            push(b, kind, random(-40, 40));
        }
    }
}
/// `Hero Death Anim`'s `Initiate`: eight of each kind, in every direction.
pub fn death(at: [i32; 2]) {
    let b = start(at);
    for kind in 0..2 {
        for _ in 0..8 {
            push(b, kind, random(0, 360));
        }
    }
}
/// A scene left or a respawn: nothing outlives it.
pub fn clear() {
    burst().count = 0;
}
/// One 60 Hz simulation tick.
pub fn tick() {
    let b = burst();
    if b.count == 0 {
        return;
    }
    b.age += 1;
    if b.age >= LIFE_TICKS {
        b.count = 0;
    }
}
/// The two ability frames this burst shows at its age, one per kind present.
fn frames() -> [Option<usize>; 2] {
    let b = burst();
    if b.count == 0 {
        return [None; 2];
    }
    let mut present = [false; 2];
    for s in &b.slashes[..b.count as usize] {
        present[s.kind as usize] = true;
    }
    let clip = |kind: usize| {
        let clip = ability_art::SLASH_GHOST_1 + kind;
        let age = u32::from(b.age);
        // A clip that ended before the burst does is gone, not held on its last frame.
        (age < ability_art::clip_ticks(clip)).then(|| ability_art::frame_index(clip, age))
    };
    [
        present[0].then(|| clip(0)).flatten(),
        present[1].then(|| clip(1)).flatten(),
    ]
}
/// Ask for this frame's animation keys. False, and nothing asked for, when the cache's slots are
/// not left for both (a tiled enemy can hold twenty of its twenty-four): the slashes are the one thing in a frame that may simply not draw.
pub fn request(needed: &mut [u16], needed_len: &mut usize) -> bool {
    let frames = frames();
    let wanted = frames.iter().flatten().count();
    if wanted == 0 || *needed_len + wanted > hk_cache::SLOTS.min(needed.len()) {
        return false;
    }
    for i in frames.into_iter().flatten() {
        needed[*needed_len] = ability_art::KEY_BASE + i as u16;
        *needed_len += 1;
    }
    true
}
/// Draw the burst. Only after `request` said yes this frame.
#[cfg(not(test))]
pub fn draw(camera: (i32, i32)) -> u32 {
    let frames = frames();
    let b = burst();
    let mut drawn = 0;
    for s in &b.slashes[..b.count as usize] {
        let Some(frame) = frames[s.kind as usize] else {
            continue;
        };
        // Degrees to the table's 4096 a turn.
        let turn = ((i32::from(s.angle) + i32::from(s.jitter)).rem_euclid(360) * 4096 / 360) as u16;
        let travelled = i32::from(s.speed) * i32::from(b.age) * 65536 / 60;
        let (sin, cos) = (psx_math::sincos::sin_q12(turn), psx_math::sincos::cos_q12(turn));
        // The path itself follows the flight angle, not the jittered facing.
        let flight = (i32::from(s.angle).rem_euclid(360) * 4096 / 360) as u16;
        let at = [
            b.origin[0] + (travelled >> 4) * psx_math::sincos::cos_q12(flight) / 256,
            b.origin[1] + (travelled >> 4) * psx_math::sincos::sin_q12(flight) / 256,
        ];
        drawn += ability_art::draw_slash(frame, at, (sin, cos), i32::from(s.scale), camera, 128);
    }
    drawn
}
