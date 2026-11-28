//! The Knight's last-mask warning.
//!
//! One mask left (`Knight Damage`, resources.assets 24999): `Last HP?` activates `Low Health
//! Effect` (the `Low_Health_Hit` Animator clip, 1081) on the hit that leaves one mask, and the
//! `Low Health Leak` particle system (5844) breathes dark steam off the Knight while it stays at
//! one.
//!
//! The burst is a white splat (`low_health_hit0000` to `0006`, 5.56 units at scale 1) the hit
//! colours black. Its seven frames are not cooked (every cooked byte of the Knight's art is
//! linked); it is the hero light's soft radial ramp (hero_light.rs) drawn as a fan and subtracted,
//! at the size and strength the clip's curves give (decoded from its streamed keys). The steam is
//! small fans of the same ramp, subtracted: the `Particle_Shade` smoke is near-black.
use psx_fx::NrLcg;

/// World units (Q16) per unit.
const ONE: i32 = 65536;

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
    /// 0 none, 1 steam (fades in to 36.5 % of its life, then out).
    kind: u8,
}
const NO_BLOB: Blob = Blob { x: 0, y: 0, vx: 0, vy: 0, r: 0, age: 0, life: 0, kind: 0 };
const BLOBS: usize = 16;
const STEAM: u8 = 1;

struct State {
    blobs: [Blob; BLOBS],
    /// Ticks since the burst began, 0xFF when none.
    burst_age: u8,
    burst_at: [i32; 2],
    seed: u32,
}
static mut STATE: State = State {
    blobs: [NO_BLOB; BLOBS],
    burst_age: 0xFF,
    burst_at: [0; 2],
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

/// The hit that leaves one mask: `Last HP?` starts the black burst at the Knight.
pub fn last_mask(at: [i32; 2]) {
    let s = state();
    s.burst_age = 0;
    s.burst_at = at;
    unsafe {
        HK_HURT_FX[0] = HK_HURT_FX[0].saturating_add(1);
    }
}
/// A respawn or a scene change: nothing outlives it.
pub fn clear() {
    let s = state();
    s.blobs = [NO_BLOB; BLOBS];
    s.burst_age = 0xFF;
}
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
    for b in s.blobs.iter_mut().filter(|b| b.kind != 0) {
        b.age += 1;
        if b.age >= b.life {
            b.kind = 0;
            continue;
        }
        b.x += b.vx;
        b.y += b.vy;
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
fn screen(x: i32, y: i32, camera: (i32, i32)) -> (i32, i32) {
    (
        160 + (((x - camera.0) >> 8) * crate::KNIGHT_SCALE >> 20),
        120 - (((y - camera.1) >> 8) * crate::KNIGHT_SCALE >> 20),
    )
}
/// Draw what is up: the steam, then the burst.
#[cfg(not(test))]
pub fn draw(camera: (i32, i32)) -> u32 {
    let s = state();
    let mut drawn = 0;
    #[cfg(feature = "hero-light")]
    {
        for b in s.blobs.iter().filter(|b| b.kind != 0) {
            let (age, life) = (i32::from(b.age), i32::from(b.life));
            // Alpha 0 to 1 by 36.5 % of the life, then back to 0.
            let rise = life * 365 / 1000;
            let alpha = if age < rise { age * 256 / rise.max(1) } else { (life - age) * 256 / (life - rise).max(1) };
            // Near-black smoke: at most about a third of the light taken off.
            let strength = (alpha * 48 / 256) as u32;
            let r = i32::from(b.r) * crate::KNIGHT_SCALE >> 20;
            drawn += soft_disc(screen(b.x, b.y, camera), r.max(1), strength, false, 4);
        }
        if s.burst_age != 0xFF {
            let (scale, alpha) = keyed(&BURST_KEYS, s.burst_age);
            let r = (SPLAT_RADIUS_Q8 * scale >> 8) * crate::KNIGHT_SCALE >> 20;
            drawn += soft_disc(screen(s.burst_at[0], s.burst_at[1], camera), r, (alpha as u32 * 128) >> 8, false, 8);
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
