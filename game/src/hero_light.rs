//! The original's HeroLight and hero vignette, at a cost the GPU can carry.
//!
//! HeroLight is `light_effect_v02` (one colour, alpha falling off like a
//! Gaussian) scaled 3x, 0.6 units below the Knight and just behind him, in a
//! Linear Light blend tinted by the scene's `heroLightColor`. Here it is a fan
//! of eight additive textured triangles over a 64-texel radial ramp that holds
//! the sprite's own falloff (one VRAM row in the spare CLUT strip), tinted by
//! the scene's colour (`host/scene_grading.py`), drawn right before the Knight.
//! The whole fan is drawn, below the Knight's feet too: a floor in front of the
//! Knight is drawn over that part, and a thin platform has background there
//! that the original's light does fall on. (The fan used to be clipped at the
//! feet while the Knight stood, which cut the light off in a straight line
//! under any platform.) Pixels cost the GPU about one clock each, so the fan
//! is all the light is.
//!
//! The vignette is `vignette_large_v01` (black, a soft hole) on the Knight at
//! the scale the Darkness Control FSM picks for the scene's darkness level:
//! B*(1-alpha). It costs no pixels at all: the scenery draws' own colour
//! modulation takes 1-alpha at each draw's nearest point to the Knight
//! (`vignette_factor`, render::set_vignette), which darkens the screen edges
//! where the original does and never darkens the middle of a large
//! background quad that reaches the Knight. Actors keep full brightness.
#[cfg(feature = "hero-light")]
use psx_vram::{Clut, TexDepth, Tpage, VramRect};
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scene_grading.rs"));

/// Octagon directions, Q12 (cos, sin).
const DIRS: [(i32, i32); 8] = [(4096, 0), (2896, 2896), (0, 4096), (-2896, 2896), (-4096, 0), (-2896, -2896), (0, -4096), (2896, -2896)];
/// light_effect_v02's radial mean alpha at the HeroLight's scale of 3:
/// world radius (Q8 units) and alpha (0..255).
const LIGHT_PROFILE: [(i32, i32); 9] = [(0, 198), (256, 181), (512, 145), (768, 102), (1024, 63), (1280, 34), (1536, 15), (1792, 4), (2048, 0)];
/// The fan's radius (Q8 units): 3.5 units, where the sprite's alpha is about
/// a fifth of its peak; the ramp eases to zero there. Over the 49 builder
/// routes the light alone cost 0.44% of presented fps on average and at most
/// 2.95%; with the vignette too, 0.71% and 3.90% (boss-fight).
pub const LIGHT_RADIUS_Q8: i32 = 896;
/// The ramp texel row and its grey CLUT in the spare CLUT strip
/// (hk_cache::residency: rows 480-481 at x 368 are claimed by nothing).
const RAMP_XY: (u16, u16) = (368, 480);
const RAMP_CLUT_XY: (u16, u16) = (368, 481);
/// Texture page holding RAMP_XY (x 320, y 256, 4bpp) and the ramp's u, v there.
const RAMP_U0: u8 = ((RAMP_XY.0 - 320) * 4) as u8;
const RAMP_V: u8 = (RAMP_XY.1 - 256) as u8;
/// Vignette alpha at scale 5.5 (Darkness Level 0), from vignette_large_v01's
/// radial mean: world distance (Q8 units) and alpha (0..256).
const VIGNETTE_PROFILE: [(i32, i32); 6] = [(0, 0), (768, 13), (2048, 79), (3328, 147), (5632, 230), (9216, 250)];
const VIGNETTE_BASE_Q8: i32 = 1408; // 5.5

fn interpolate(table: &[(i32, i32)], x: i32) -> i32 {
    if x <= table[0].0 {
        return table[0].1;
    }
    for w in table.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    table[table.len() - 1].1
}

fn scene_light(scene: usize) -> Option<SceneLight> {
    SCENE_LIGHTS.get(scene).copied()
}

fn screen(x: i32, y: i32, camera: (i32, i32)) -> (i32, i32) {
    (160 + (((x - camera.0) >> 8) * crate::KNIGHT_SCALE >> 20), 120 - (((y - camera.1) >> 8) * crate::KNIGHT_SCALE >> 20))
}

/// Once, with the other first-room uploads: the 64-texel ramp (the light's
/// falloff over LIGHT_RADIUS, 15 grey levels) and its CLUT, every visible
/// entry semi-transparent so the Add equation applies texel by texel.
#[cfg(feature = "hero-light")]
pub fn upload() {
    let mut texels = [0u8; 32];
    // The falloff, lowered by its value at the rim so the fan has no edge.
    let rim = interpolate(&LIGHT_PROFILE, LIGHT_RADIUS_Q8);
    let peak = LIGHT_PROFILE[0].1 - rim;
    for j in 0..64 {
        let r = LIGHT_RADIUS_Q8 * j as i32 / 63;
        let level = (((interpolate(&LIGHT_PROFILE, r) - rim) * 15 + peak / 2) / peak).clamp(0, 15) as u8;
        texels[j / 2] |= level << ((j & 1) * 4);
    }
    let mut clut = [0u8; 32];
    for k in 1..16u16 {
        let grey = k * 31 / 15;
        let word = 0x8000 | grey | grey << 5 | grey << 10;
        clut[k as usize * 2..k as usize * 2 + 2].copy_from_slice(&word.to_le_bytes());
    }
    psx_gpu::draw_sync();
    psx_vram::upload_bytes(VramRect::new(RAMP_XY.0, RAMP_XY.1, 16, 1), &texels);
    psx_vram::upload_bytes(VramRect::new(RAMP_CLUT_XY.0, RAMP_CLUT_XY.1, 16, 1), &clut);
}

/// The hero light, drawn immediately before the Knight.
#[cfg(feature = "hero-light")]
pub fn draw_light(scene: usize, x: i32, y: i32, camera: (i32, i32)) -> u32 {
    let Some(light) = scene_light(scene) else { return 0 };
    if light.rgb == [0, 0, 0] {
        return 0;
    }
    let centre = screen(x, y - 39322, camera); // HeroLight local y -0.6
    let r = LIGHT_RADIUS_Q8 * crate::KNIGHT_SCALE >> 20;
    if centre.0 + r < 0 || centre.0 - r >= 320 || centre.1 + r < 0 || centre.1 - r >= 240 {
        return 0;
    }
    // Ramp texel 63 is 15/15 of the CLUT's white, which modulates to 248 at
    // a tint of 128: scale the tint so the peak lands on the scene's colour.
    let tint = light.rgb.map(|c| (c as u32 * 128 / 248).min(255));
    let colour = tint[0] | tint[1] << 8 | tint[2] << 16;
    let clut = Clut::new(RAMP_CLUT_XY.0, RAMP_CLUT_XY.1).uv_clut_word() as u32;
    let tpage = Tpage::new(320, 256, TexDepth::Bit4).uv_tpage_word(1) as u32;
    let vertex = |p: (i32, i32)| ((p.1.clamp(-1023, 1023) as u32 & 0xffff) << 16) | (p.0.clamp(-1023, 1023) as u32 & 0xffff);
    let rim = |k: usize| (centre.0 + (r * DIRS[k & 7].0 >> 12), centre.1 - (r * DIRS[k & 7].1 >> 12));
    let mut tris = [[0u32; 7]; 8];
    let mut n = 0;
    for k in 0..8 {
        let (a, b) = (rim(k), rim(k + 1));
        let uv0 = (RAMP_U0 as u32) | (RAMP_V as u32) << 8;
        let uv_rim = ((RAMP_U0 + 63) as u32) | (RAMP_V as u32) << 8;
        // GP0(26h): textured triangle, semi-transparent, modulated.
        tris[n] = [0x2600_0000 | colour, vertex(centre), uv0 | clut << 16, vertex(a), uv_rim | tpage << 16, vertex(b), uv_rim];
        n += 1;
    }
    crate::render::light_fan(&tris[..n])
}

/// 1-alpha (0..256) of the vignette by screen distance in 8-pixel steps, for
/// the scene in VIGNETTE_SCENE; rebuilt only when the scene changes.
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_LUT: [u16; 64] = [256; 64];
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_SCENE: usize = usize::MAX;

/// Point the vignette at `scene`; false when it has none.
#[cfg(feature = "hero-vignette")]
pub fn vignette_scene(scene: usize) -> bool {
    unsafe {
        if VIGNETTE_SCENE != scene {
            VIGNETTE_SCENE = scene;
            let scale = scene_light(scene).map_or(0, |l| l.vignette_q8) as i32;
            for (i, f) in (*(&raw mut VIGNETTE_LUT)).iter_mut().enumerate() {
                *f = if scale == 0 {
                    256
                } else {
                    // Screen pixels to world units (Q8) on the gameplay plane,
                    // then to the scale-5.5 profile's distance.
                    let d_q8 = ((i as i32 * 8) << 20) / crate::KNIGHT_SCALE;
                    (256 - interpolate(&VIGNETTE_PROFILE, d_q8 * VIGNETTE_BASE_Q8 / scale)).clamp(0, 256) as u16
                };
            }
        }
        (*(&raw const VIGNETTE_LUT))[63] != 256 || (*(&raw const VIGNETTE_LUT))[1] != 256
    }
}

/// 1-alpha (0..256) at screen offset (dx, dy) pixels from the Knight's
/// vignette centre. Octagonal distance (max + min/2), no multiply or divide.
#[cfg(feature = "hero-vignette")]
#[inline]
pub fn vignette_factor(dx: i32, dy: i32) -> u32 {
    let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
    let d = ax.max(ay) + (ax.min(ay) >> 1);
    unsafe { (*(&raw const VIGNETTE_LUT))[(d >> 3).min(63) as usize] as u32 }
}
