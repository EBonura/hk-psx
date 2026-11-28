//! The Knight's last-mask warning and the parts of his death the clips do not carry.
//!
//! One mask left (`Knight Damage`, resources.assets 24999): `Last HP?` activates `Low Health
//! Effect` (the `Low_Health_Hit` Animator clip, 1081) on the hit that leaves one mask, and the
//! `Low Health Leak` particle system (5844) breathes dark steam off the Knight while it stays at
//! one. The death (`Hero Death Anim`, 21158): `Initiate` flares `Death Crack` (`death_crack_new`,
//! 1133) whatever the time scale, `Blow` throws `Corpse Nail Hero` (6648), `Ash Spew` spits ash
//! (`Ash 1` to `3`) to both sides until the Death clip ends, and then the head (`Corpse Head`,
//! 6436) is thrown.
//!
//! The burst and the flare are one white splat (`low_health_hit0000` to `0006`, 5.56 units at
//! scale 1): the low-health hit colours it black, the death leaves it white. Their seven frames are
//! not cooked (a splat that grows to ten times its size on a death needs no detail at this
//! resolution, and every cooked byte of the Knight's art is linked); each is the hero light's soft
//! radial ramp (hero_light.rs) drawn as a fan, subtracted for the black splat and added for the
//! white, at the size and strength the clips' curves give (decoded from their streamed keys). The
//! steam and the ash are small fans of the same ramp, subtracted: both are near-black (the
//! `Particle_Shade` smoke and the ash sprites, average 15 grey). The nail and the head are their
//! own sprites (ability art `CORPSE_NAIL`, `CORPSE_HEAD`).
use crate::ability_art;
use psx_fx::NrLcg;

/// World units (Q16) per unit.
const ONE: i32 = 65536;
/// `Physics2DSettings.m_Gravity` (globalgamemanagers): -60 units/s², per tick per tick in Q16.
const GRAVITY: i32 = 60 * ONE / 3600;

/// Ticks and (scale, alpha), both Q8, of `Low_Health_Hit` (25 samples a second, 0.32 s): scale 1,
/// a dip to 0.78 between 0.08 and 0.16 s, 2.45 at 0.24 s and 2.68 at 0.28 s; alpha 1 until 0.24 s,
/// 0.39 at 0.28 s, 0 at 0.32 s. Linear between keys (the clip's own are cubic).
const BURST_KEYS: [(u8, i32, i32); 8] = [
    (0, 256, 256),
    (5, 256, 256),
    (7, 199, 256),
    (10, 256, 256),
    (12, 256, 256),
    (14, 627, 256),
    (17, 687, 100),
    (19, 687, 0),
];
/// `death_crack_new` (12 samples a second, 1/3 s): scale 2.62, 3.80 at 1/12 s, 4.64 at 1/6 s,
/// 10.11 at 1/4 s; alpha 1 - 27t² + 54t³ from 1 to 0 over the third of a second.
const FLARE_KEYS: [(u8, i32, i32); 6] = [
    (0, 670, 256),
    (5, 973, 215),
    (10, 1189, 128),
    (15, 2588, 40),
    (18, 2588, 10),
    (20, 2588, 0),
];
/// Half the splat's 5.56 units at scale 1, Q8.
const SPLAT_RADIUS_Q8: i32 = 712;

#[derive(Clone, Copy)]
struct Blob {
    /// World position and velocity, Q16 units and Q16 units a tick.
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    /// Radius, Q8 units.
    r: i16,
    age: u8,
    life: u8,
    /// 0 none, 1 steam (fades in to 36.5 % of its life, then out), 2 ash (falls, fades at the end).
    kind: u8,
}
const NO_BLOB: Blob = Blob { x: 0, y: 0, vx: 0, vy: 0, r: 0, age: 0, life: 0, kind: 0 };
const BLOBS: usize = 16;
const STEAM: u8 = 1;
const ASH: u8 = 2;

/// A thrown corpse part: the nail or the head.
#[derive(Clone, Copy)]
struct Part {
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    /// Turn, 4096 a revolution, and its rate a tick.
    turn: i32,
    spin: i32,
    /// Gravity a tick (the part's `m_GravityScale`) and the bounce factor, Q8.
    gravity: i32,
    bounce: i32,
    floor: i32,
    live: bool,
    resting: bool,
}
const NO_PART: Part = Part {
    x: 0,
    y: 0,
    vx: 0,
    vy: 0,
    turn: 0,
    spin: 0,
    gravity: 0,
    bounce: 0,
    floor: 0,
    live: false,
    resting: false,
};

struct State {
    blobs: [Blob; BLOBS],
    /// The burst (black) and the flare (white): ticks since they began, 0xFF when none.
    burst_age: u8,
    burst_at: [i32; 2],
    flare_age: u8,
    flare_at: [i32; 2],
    /// Ticks of the death since the hit stop ended, 0xFFFF when he is not dying.
    death_age: u16,
    death_at: [i32; 2],
    facing: i32,
    floor: i32,
    parts: [Part; 2],
    seed: u32,
}
static mut STATE: State = State {
    blobs: [NO_BLOB; BLOBS],
    burst_age: 0xFF,
    burst_at: [0; 2],
    flare_age: 0xFF,
    flare_at: [0; 2],
    death_age: 0xFFFF,
    death_at: [0; 2],
    facing: 1,
    floor: 0,
    parts: [NO_PART; 2],
    seed: 0x6d2b_79f5,
};
/// Low-health bursts, death flares and steam puffs so far, for the route pins and the stills.
#[no_mangle]
pub static mut HK_HURT_FX: [u32; 3] = [0; 3];

fn state() -> &'static mut State {
    unsafe { &mut *(&raw mut STATE) }
}
fn random(s: &mut State, low: i32, high: i32) -> i32 {
    s.seed = NrLcg::step(s.seed);
    low + ((s.seed >> 16) as i32 * (high - low + 1)) / 65536
}
fn spawn(s: &mut State, blob: Blob) {
    if let Some(slot) = s.blobs.iter_mut().find(|b| b.kind == 0) {
        *slot = blob;
    }
}
/// Units a second at `degrees` to a Q16 velocity a tick.
fn velocity(speed: i32, degrees: i32) -> (i32, i32) {
    let turn = (degrees.rem_euclid(360) * 4096 / 360) as u16;
    let per_tick = speed * ONE / 60;
    (
        (per_tick >> 4) * psx_math::sincos::cos_q12(turn) / 256,
        (per_tick >> 4) * psx_math::sincos::sin_q12(turn) / 256,
    )
}

/// The hit that leaves one mask: `Last HP?` starts the black burst at the Knight.
pub fn last_mask(at: [i32; 2]) {
    let s = state();
    s.burst_age = 0;
    s.burst_at = at;
    unsafe {
        HK_HURT_FX[0] = HK_HURT_FX[0].saturating_add(1);
    }
}
/// The killing hit: the flare now, the nail, ash and head as the death plays (`tick`). `floor` is
/// the Knight's feet, where the thrown parts come to rest; `facing` is +1 for right.
pub fn death(at: [i32; 2], floor: i32, facing: i32) {
    let s = state();
    s.flare_age = 0;
    s.flare_at = at;
    s.death_age = 0;
    s.death_at = at;
    s.facing = facing;
    s.floor = floor;
    s.burst_age = 0xFF;
    unsafe {
        HK_HURT_FX[1] = HK_HURT_FX[1].saturating_add(1);
    }
}
/// A respawn or a scene change: nothing outlives it.
pub fn clear() {
    let s = state();
    s.blobs = [NO_BLOB; BLOBS];
    s.burst_age = 0xFF;
    s.flare_age = 0xFF;
    s.death_age = 0xFFFF;
    s.parts = [NO_PART; 2];
}
/// Every fixed tick that is not paused, hit stop included: `Death Crack` is not scaled by time.
pub fn tick_unscaled() {
    let s = state();
    if s.flare_age != 0xFF {
        s.flare_age = if s.flare_age >= FLARE_KEYS[FLARE_KEYS.len() - 1].0 { 0xFF } else { s.flare_age + 1 };
    }
}
/// `Blow` follows `Start`'s 0.7 s wait; `Ash Spew` runs until the Death clip ends (18 frames at
/// 10 fps), then the head is thrown.
const BLOW_TICKS: u16 = 42;
const HEAD_TICKS: u16 = 108;
/// One simulation tick, with the Knight's masks after this tick's damage and where he is. The hit
/// that leaves him one mask starts the burst (`Last HP?` follows every `DAMAGE`, hazards included).
pub fn tick(health: u16, dead: bool, at: [i32; 2]) {
    static mut SEEN: u16 = 0;
    let seen = unsafe { core::mem::replace(&mut *(&raw mut SEEN), health) };
    if health == 1 && seen > 1 && !dead {
        last_mask(at);
    }
    let low = health == 1 && !dead;
    let s = state();
    if s.burst_age != 0xFF {
        s.burst_age = if s.burst_age >= BURST_KEYS[BURST_KEYS.len() - 1].0 { 0xFF } else { s.burst_age + 1 };
    }
    // `Low Health Leak`: 10 a second from a box 0.75 units wide 0.68 below the Knight, straight up
    // at 0.2 to 1 unit a second, living 1 to 2 s, 0.1 to 0.8 units across.
    if low {
        s.seed = NrLcg::step(s.seed);
        if s.seed >> 16 < 65536 / 6 {
            let x = at[0] + random(s, -ONE * 3 / 8, ONE * 3 / 8);
            let vy = random(s, ONE / 300, ONE / 60);
            let life = random(s, 60, 120) as u8;
            let r = random(s, 13, 102) as i16;
            spawn(s, Blob { x, y: at[1] - ONE * 68 / 100, vx: 0, vy, r, age: 0, life, kind: STEAM });
            unsafe {
                HK_HURT_FX[2] = HK_HURT_FX[2].saturating_add(1);
            }
        }
    }
    if s.death_age != 0xFFFF {
        let age = s.death_age;
        s.death_age = s.death_age.saturating_add(1).min(0xFFFE);
        if age == BLOW_TICKS {
            // `Face Right`/`Face Left`: thrown at 120 (or 60) degrees, 20 units a second, torque
            // 120 the other way; `Corpse Nail Hero` falls at 0.8 g and keeps 0.3 of a bounce.
            let (vx, vy) = velocity(20, if s.facing > 0 { 120 } else { 60 });
            s.parts[0] = Part {
                x: s.death_at[0],
                y: s.death_at[1],
                vx,
                vy,
                turn: 0,
                spin: if s.facing > 0 { 40 } else { -40 },
                gravity: GRAVITY * 8 / 10,
                bounce: 77,
                floor: s.floor,
                live: true,
                resting: false,
            };
        }
        if age == HEAD_TICKS {
            // `Head Right`/`Head Left`: turned -15 (or 20) degrees, thrown at 100 (or 80) degrees,
            // 15 units a second; `Corpse Head` falls at 1.5 g and keeps half of a bounce.
            let right = s.facing > 0;
            let (vx, vy) = velocity(15, if right { 100 } else { 80 });
            s.parts[1] = Part {
                x: s.death_at[0],
                y: s.death_at[1],
                vx,
                vy,
                turn: if right { -171 } else { 228 },
                spin: if right { 8 } else { -8 },
                gravity: GRAVITY * 3 / 2,
                bounce: 128,
                floor: s.floor,
                live: true,
                resting: false,
            };
        }
        // Six spawners of two every 0.05 s, at 25 to 45 units a second within 30 degrees of
        // either side: thinned here to one each way every third tick.
        if (BLOW_TICKS..HEAD_TICKS).contains(&age) && age % 3 == 0 {
            for side in [0, 180] {
                let (vx, vy) = velocity(random(s, 25, 45), side + random(s, -30, 30));
                let x = s.death_at[0] + random(s, -ONE / 10, ONE / 10);
                let y = s.death_at[1] + random(s, -ONE * 3 / 10, ONE * 3 / 10);
                spawn(s, Blob { x, y, vx, vy, r: 24, age: 0, life: 24, kind: ASH });
            }
        }
    }
    for b in s.blobs.iter_mut().filter(|b| b.kind != 0) {
        b.age += 1;
        if b.age >= b.life {
            b.kind = 0;
            continue;
        }
        if b.kind == ASH {
            b.vy -= GRAVITY;
        }
        b.x += b.vx;
        b.y += b.vy;
    }
    for p in s.parts.iter_mut().filter(|p| p.live && !p.resting) {
        p.vy -= p.gravity;
        p.x += p.vx;
        p.y += p.vy;
        p.turn = (p.turn + p.spin) & 4095;
        if p.y <= p.floor && p.vy < 0 {
            p.y = p.floor;
            p.vy = -p.vy * p.bounce / 256;
            p.vx = p.vx * p.bounce / 256;
            p.spin = p.spin * p.bounce / 256;
            if p.vy < ONE / 30 {
                // Lying where it fell: flat on its long side.
                p.resting = true;
                p.turn = if (1024..3072).contains(&p.turn) { 2048 } else { 0 };
            }
        }
    }
}
/// (scale, alpha) Q8 of a keyed clip at `age`.
fn keyed(keys: &[(u8, i32, i32)], age: u8) -> (i32, i32) {
    for w in keys.windows(2) {
        let ((t0, s0, a0), (t1, s1, a1)) = (w[0], w[1]);
        if age <= t1 {
            let (t, span) = (i32::from(age - t0.min(age)), i32::from(t1 - t0).max(1));
            return (s0 + (s1 - s0) * t / span, a0 + (a1 - a0) * t / span);
        }
    }
    let last = keys[keys.len() - 1];
    (last.1, last.2)
}
/// The two corpse parts' frames, to ask the animation cache for.
fn part_frames() -> [Option<usize>; 2] {
    let s = state();
    [
        s.parts[0].live.then(|| ability_art::frame_index(ability_art::CORPSE_NAIL, 0)),
        s.parts[1].live.then(|| ability_art::frame_index(ability_art::CORPSE_HEAD, 0)),
    ]
}
/// Ask for this frame's animation keys (the thrown parts'). False when the cache has no room left.
pub fn request(needed: &mut [u16], needed_len: &mut usize) -> bool {
    let frames = part_frames();
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
fn screen(x: i32, y: i32, camera: (i32, i32)) -> (i32, i32) {
    (
        160 + (((x - camera.0) >> 8) * crate::KNIGHT_SCALE >> 20),
        120 - (((y - camera.1) >> 8) * crate::KNIGHT_SCALE >> 20),
    )
}
/// Draw what is up: the parts (after `request` said yes this frame), then the steam and ash, the
/// burst and the flare.
#[cfg(not(test))]
pub fn draw(camera: (i32, i32), parts: bool) -> u32 {
    let s = state();
    let mut drawn = 0;
    if parts {
        for (p, frame) in s.parts.iter().zip(part_frames()) {
            let Some(frame) = frame else { continue };
            let turn = p.turn as u16;
            let (sin, cos) = (psx_math::sincos::sin_q12(turn), psx_math::sincos::cos_q12(turn));
            drawn += ability_art::draw_turned(frame, [p.x, p.y], (sin, cos), 16, camera, 128, false);
        }
    }
    #[cfg(feature = "hero-light")]
    {
        for b in s.blobs.iter().filter(|b| b.kind != 0) {
            let (age, life) = (i32::from(b.age), i32::from(b.life));
            let alpha = if b.kind == STEAM {
                // Alpha 0 to 1 by 36.5 % of the life, then back to 0.
                let rise = life * 365 / 1000;
                if age < rise { age * 256 / rise.max(1) } else { (life - age) * 256 / (life - rise).max(1) }
            } else {
                ((life - age) * 256 / 8).min(256)
            };
            // Near-black smoke and ash: at most about a third of the light taken off.
            let strength = (alpha * 48 / 256) as u32;
            let r = i32::from(b.r) * crate::KNIGHT_SCALE >> 20;
            drawn += soft_disc(screen(b.x, b.y, camera), r.max(1), strength, false, 4);
        }
        if s.burst_age != 0xFF {
            let (scale, alpha) = keyed(&BURST_KEYS, s.burst_age);
            let r = (SPLAT_RADIUS_Q8 * scale >> 8) * crate::KNIGHT_SCALE >> 20;
            drawn += soft_disc(screen(s.burst_at[0], s.burst_at[1], camera), r, (alpha as u32 * 128) >> 8, false, 8);
        }
        if s.flare_age != 0xFF {
            let (scale, alpha) = keyed(&FLARE_KEYS, s.flare_age);
            let r = (SPLAT_RADIUS_Q8 * scale >> 8) * crate::KNIGHT_SCALE >> 20;
            drawn += soft_disc(screen(s.flare_at[0], s.flare_at[1], camera), r, (alpha as u32 * 128) >> 8, true, 8);
        }
    }
    drawn
}
/// A soft disc of the hero light's ramp (hero_light.rs `RAMP_*`): `strength` 0..128 of white at its
/// centre, falling off as `light_effect_v02` does to nothing at radius `r` (pixels); added for light,
/// subtracted for shade. `spokes` is 4 (small discs) or 8.
#[cfg(all(feature = "hero-light", not(test)))]
fn soft_disc(centre: (i32, i32), r: i32, strength: u32, light: bool, spokes: usize) -> u32 {
    if strength == 0
        || centre.0 + r < 0
        || centre.0 - r >= 320
        || centre.1 + r < 0
        || centre.1 - r >= 240
    {
        return 0;
    }
    let colour = strength | strength << 8 | strength << 16;
    let (clut, tpage, uv0, uv_rim) = crate::hero_light::ramp_words(if light { 1 } else { 2 });
    let vertex = |p: (i32, i32)| {
        ((p.1.clamp(-1023, 1023) as u32 & 0xffff) << 16) | (p.0.clamp(-1023, 1023) as u32 & 0xffff)
    };
    let step = 8 / spokes;
    let rim = |k: usize| {
        let (c, s) = crate::hero_light::DIRS[(k * step) & 7];
        (centre.0 + (r * c >> 12), centre.1 - (r * s >> 12))
    };
    let mut tris = [[0u32; 7]; 8];
    for (k, tri) in tris.iter_mut().enumerate().take(spokes) {
        *tri = [
            psx_hw::gpu::packet::FLAT_TEXTURED_TRIANGLE | psx_hw::gpu::packet::SEMI_TRANSPARENT | colour,
            vertex(centre),
            uv0 | clut << 16,
            vertex(rim(k)),
            uv_rim | tpage << 16,
            vertex(rim(k + 1)),
            uv_rim,
        ];
    }
    crate::render::soft_fan(&tris[..spokes])
}
