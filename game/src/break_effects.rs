//! Source Breakable particle families sharing the fixed particle pool.
//! Fixed-step motion, sampled curves and radius/edge contacts approximate Unity.
use super::{dot3, length3, mulq, range, Particle, Pool};
use hk_sim::ONE;
#[derive(Clone, Copy)]
pub struct Sample {
    pub size: [i32; 2],
    pub alpha: [u8; 2],
    pub spin: [i32; 2],
}
#[derive(Clone, Copy)]
pub struct Style {
    /// The two ends of the source start colour. Equal ends are one authored
    /// colour; differing ends are Unity's random-between-two, picked per particle.
    pub life: [u16; 2],
    pub speed: [i32; 2],
    pub size: [i32; 2],
    pub rotation: [i32; 2],
    pub colors: [[u8; 3]; 2],
    /// Each end's opacity as a fraction of the one baked into `samples`, so a
    /// single authored colour is [255,255] and costs the draw nothing.
    pub start_alpha: [u8; 2],
    pub count: u16,
    pub rate: i32,
    pub shape: u8,
    pub radius: i32,
    pub arc: i32,
    pub shape_scale: [i32; 3],
    pub force: [[i32; 2]; 3],
    pub velocity: [[i32; 2]; 3],
    pub limit: i32,
    pub dampen: i32,
    pub spin_speed: [i32; 2],
    pub spin_range: [i32; 2],
    pub collision: bool,
    pub bounce: i32,
    pub collision_dampen: i32,
    pub life_loss: i32,
    pub kill_speed: i32,
    pub radius_scale: i32,
    pub samples: &'static [Sample],
    pub frames: &'static [[u16; 3]],
}
#[derive(Clone, Copy)]
pub struct Emitter {
    pub scene: u8,
    pub owner: u16,
    pub source: u32,
    pub style: u8,
    pub angle_offset: i32,
    pub origin: [i32; 3],
    pub basis: [[i32; 3]; 3],
}
#[derive(Clone, Copy)]
pub struct Art {
    pub u: u8,
    pub v: u8,
    pub w: u8,
    pub h: u8,
    pub clut: u16,
    pub tpage: u16,
}
pub struct Upload {
    pub offset: usize,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}
/// One HLZC disc chunk per catalogue scene holding that scene's effect texels.
#[derive(Clone, Copy)]
pub struct EffectArtDesc {
    pub raw_len: usize,
    pub raw_fnv: u32,
    pub stored_len: usize,
    pub stored_fnv: u32,
}
#[cfg(not(test))]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/break_effects.rs"
));
#[cfg(test)]
pub static CURVES: &[&[Sample]] = &[];
#[cfg(test)]
pub static PARTICLE_TRACKS: &[[[u32; 2]; 33]] = &[];
#[cfg(test)]
pub static EMITTER_TRACK_BASES: &[u16] = &[];
#[cfg(test)]
pub const FX_MAX_STYLES: usize = 18;
#[cfg(test)]
pub const FX_MAX_EMITTERS: usize = 100;
#[cfg(test)]
pub const FX_MAX_ART: usize = 48;
#[cfg(test)]
pub const FX_MAX_UPLOADS: usize = 48;
#[cfg(test)]
pub const FX_MAX_FRAMES: usize = 96;

/// The admitted scene's effect tables, decoded from its HKFX0001 chunk at
/// scene admission (`load_scene`). Exactly one scene is resident, so these
/// bounded tables replace the per-scene linked statics; `Style::samples`
/// borrows the linked curve table and `Style::frames` this frame pool.
struct Resident {
    scene: usize,
    styles: [Style; FX_MAX_STYLES],
    style_count: usize,
    emitters: [Emitter; FX_MAX_EMITTERS],
    emitter_count: usize,
    art: [Art; FX_MAX_ART],
    art_count: usize,
    frames: [[u16; 3]; FX_MAX_FRAMES],
    /// Styles drawn as soft additive light rather than dithered coverage
    /// (`soft_styles`): the smoke and dust ones.
    soft: [bool; FX_MAX_STYLES],
    /// Styles of `Legacy Shaders/Particles/Additive (Soft)` (the Knight's rising motes): soft, and
    /// drawn as the full `B + F` of their texel rather than a quarter of it.
    additive: [bool; FX_MAX_STYLES],
    /// Each soft palette and its copy, every coloured entry semi-transparent,
    /// that soft draws use instead.
    soft_cluts: [(u16, u16); SOFT_CLUT_SLOTS.len()],
    soft_clut_count: usize,
    /// The style of the Knight's Focus dust emitters (`HERO_DUST_OWNER`), drawn brighter than the
    /// rest of the soft styles; `NO_STYLE` where the scene has none.
    hero_dust: u8,
}
/// Owner the cook gives the Knight's Focus dust emitters (host/hk-cook break_effects.rs `HERO_DUST_OWNER`).
pub const HERO_DUST_OWNER: u16 = 0xFFFF;
const NO_STYLE: u8 = u8::MAX;
/// A soft particle adds a quarter of its tinted texel (`B + F/4`), which suits a smoke that piles
/// up in dozens and left the Focus dust, faint puffs that overlap a few at a time, a trace beside
/// the original's. The dust's tint is scaled by this many halves (the GPU's texture modulation
/// reaches 2x, so the quarter is 2.5x as bright for the same palette). Measured on the ground
/// band beside the Knight while Focus runs, against a real run of the original on the same
/// view: mean light added 10.2 (original), 3.4 before, 10.4 at this gain (11.9 at a gain of 3).
const HERO_DUST_GAIN_HALVES: u32 = 5;
const EMPTY_STYLE: Style = Style {
    life: [0; 2],
    speed: [0; 2],
    size: [0; 2],
    rotation: [0; 2],
    colors: [[0; 3]; 2],
    start_alpha: [255; 2],
    count: 0,
    rate: 0,
    shape: 0,
    radius: 0,
    arc: 0,
    shape_scale: [0; 3],
    force: [[0; 2]; 3],
    velocity: [[0; 2]; 3],
    limit: 0,
    dampen: 0,
    spin_speed: [0; 2],
    spin_range: [0; 2],
    collision: false,
    bounce: 0,
    collision_dampen: 0,
    life_loss: 0,
    kill_speed: 0,
    radius_scale: 0,
    samples: &[],
    frames: &[],
};
const EMPTY_EMITTER: Emitter = Emitter {
    scene: 0,
    owner: 0,
    source: 0,
    style: 0,
    angle_offset: 0,
    origin: [0; 3],
    basis: [[0; 3]; 3],
};
static mut RESIDENT: Resident = Resident {
    scene: usize::MAX,
    styles: [EMPTY_STYLE; FX_MAX_STYLES],
    style_count: 0,
    emitters: [EMPTY_EMITTER; FX_MAX_EMITTERS],
    emitter_count: 0,
    art: [Art {
        u: 0,
        v: 0,
        w: 0,
        h: 0,
        clut: 0,
        tpage: 0,
    }; FX_MAX_ART],
    art_count: 0,
    frames: [[0; 3]; FX_MAX_FRAMES],
    soft: [false; FX_MAX_STYLES],
    additive: [false; FX_MAX_STYLES],
    soft_cluts: [(0, 0); SOFT_CLUT_SLOTS.len()],
    soft_clut_count: 0,
    hero_dust: NO_STYLE,
};
/// Styles and emitters of a catalogue scene; only the resident scene has any.
pub fn scene_styles(scene: usize) -> &'static [Style] {
    let r = unsafe { &*(&raw const RESIDENT) };
    if r.scene == scene {
        &r.styles[..r.style_count]
    } else {
        &[]
    }
}
pub fn scene_emitters(scene: usize) -> &'static [Emitter] {
    let r = unsafe { &*(&raw const RESIDENT) };
    if r.scene == scene {
        &r.emitters[..r.emitter_count]
    } else {
        &[]
    }
}
fn scene_art(scene: usize) -> &'static [Art] {
    let r = unsafe { &*(&raw const RESIDENT) };
    if r.scene == scene {
        &r.art[..r.art_count]
    } else {
        &[]
    }
}
const STYLE_BYTES: usize = 168;
const EMITTER_BYTES: usize = 64;
const ART_BYTES: usize = 8;
const UPLOAD_BYTES: usize = 12;
const FRAME_BYTES: usize = 6;
fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn pair(b: &[u8], at: usize) -> [i32; 2] {
    [i32_at(b, at), i32_at(b, at + 4)]
}
/// The GPU CLUT word of a palette at VRAM `(x,y)`.
fn clut_word(x: u16, y: u16) -> u16 {
    (y << 6) | (x >> 4)
}
/// Particles drawn through the soft path, and soft palettes copied.
#[no_mangle]
pub static mut HK_SOFT_PARTICLES_DRAWN: u32 = 0;
#[no_mangle]
pub static mut HK_SOFT_PALETTES: u32 = 0;
/// VRAM rows for the semi-transparent palette copies: inside the first effect
/// rectangle (352,176,32,64) and outside every scene's uploads (the cooked
/// uploads stop at row 206 in that column). A scene whose own uploads reach
/// one leaves that slot unused.
const SOFT_CLUT_SLOTS: [(u16, u16); 4] = [(368, 239), (368, 238), (368, 237), (368, 236)];
/// Mark the soft styles and give each soft palette a copy slot.
///
/// A soft style authors less than full opacity: a curve sample below 255 or a
/// random start alpha. In the admitted scenes that is exactly the Break Dust
/// smoke (resources.assets:371); every rock, chip and shard style is opaque.
/// Its coverage art, three opacity classes dithered in 4 bpp and blended by
/// Average, piled up into a bright speckled block where the source draws
/// faint grey-blue puffs. A soft style is drawn instead from its densest art
/// through a copy of its palette with every coloured entry semi-transparent,
/// added a quarter at a time at its colour times its opacity (`B + a*F/4`):
/// on the dark backgrounds these effects play over that reads as the
/// source's translucent puffs, and dozens of overlapping ones stay short of
/// white. The effects share their
/// palettes across textures, so the copy is what keeps the rocks opaque. A
/// soft style whose palette gets no slot keeps the coverage path.
fn soft_styles(r: &mut Resident, styles: usize, uploads: &[[u16; 4]]) {
    r.soft_clut_count = 0;
    let free = |&(x, y): &(u16, u16)| {
        !uploads
            .iter()
            .any(|u| x < u[0] + u[2] && u[0] < x + 16 && y >= u[1] && y < u[1] + u[3])
    };
    let mut slots = SOFT_CLUT_SLOTS.iter().copied().filter(free);
    for i in 0..styles {
        let s = &r.styles[i];
        let faint = s.start_alpha[0] < 255
            || r.additive[i]
            || s.samples
                .iter()
                .any(|x| x.alpha[0] < 255 || x.alpha[1] < 255);
        let mut ok = faint && !s.frames.is_empty();
        for cell in s.frames.iter().filter(|_| faint) {
            let clut = r.art[cell[2] as usize].clut;
            if r.soft_cluts[..r.soft_clut_count]
                .iter()
                .any(|c| c.0 == clut)
            {
                continue;
            }
            match slots.next() {
                Some((x, y)) => {
                    r.soft_cluts[r.soft_clut_count] = (clut, clut_word(x, y));
                    r.soft_clut_count += 1;
                }
                None => ok = false,
            }
        }
        r.soft[i] = ok;
    }
}
/// The semi-transparent copy of a soft palette.
fn soft_clut(r: &Resident, clut: u16) -> Option<u16> {
    r.soft_cluts[..r.soft_clut_count]
        .iter()
        .find(|c| c.0 == clut)
        .map(|c| c.1)
}
/// Parse a scene's HKFX0001 chunk into the resident tables and upload its
/// texels to the fixed effect VRAM rects. Returns false on any bound or
/// format violation; nothing of the previous scene survives a failure.
#[cfg(not(test))]
pub fn load_scene(scene: usize, data: &[u8]) -> bool {
    let r = unsafe { &mut *(&raw mut RESIDENT) };
    r.scene = usize::MAX;
    r.style_count = 0;
    r.emitter_count = 0;
    r.art_count = 0;
    if data.len() < 32 || &data[..8] != b"HKFX0001" {
        return false;
    }
    let counts: [usize; 6] = core::array::from_fn(|i| u32_at(data, 8 + i * 4) as usize);
    let [styles, emitters, art, uploads, frames, texel_base] = counts;
    if styles > FX_MAX_STYLES
        || emitters > FX_MAX_EMITTERS
        || art > FX_MAX_ART
        || uploads > FX_MAX_UPLOADS
        || frames > FX_MAX_FRAMES
    {
        return false;
    }
    let (s0, e0, a0, u0, f0) = (
        32,
        32 + styles * STYLE_BYTES,
        32 + styles * STYLE_BYTES + emitters * EMITTER_BYTES,
        32 + styles * STYLE_BYTES + emitters * EMITTER_BYTES + art * ART_BYTES,
        32 + styles * STYLE_BYTES
            + emitters * EMITTER_BYTES
            + art * ART_BYTES
            + uploads * UPLOAD_BYTES,
    );
    let tables_end = f0 + frames * FRAME_BYTES;
    if texel_base < tables_end || texel_base % 4 != 0 || texel_base > data.len() {
        return false;
    }
    for i in 0..frames {
        let at = f0 + i * FRAME_BYTES;
        r.frames[i] = [u16_at(data, at), u16_at(data, at + 2), u16_at(data, at + 4)];
        if r.frames[i].iter().any(|&f| f as usize >= art) {
            return false;
        }
    }
    for i in 0..styles {
        let b = &data[s0 + i * STYLE_BYTES..s0 + (i + 1) * STYLE_BYTES];
        let curve = u16_at(b, 160) as usize;
        let first = u16_at(b, 162) as usize;
        let count = u16_at(b, 164) as usize;
        if curve >= CURVES.len()
            || CURVES[curve].len() != 33
            || first + count > frames
            || count == 0
        {
            return false;
        }
        let frame_pool: &'static [[u16; 3]] = unsafe {
            core::slice::from_raw_parts(
                (&raw const RESIDENT.frames).cast::<[u16; 3]>().add(first),
                count,
            )
        };
        r.styles[i] = Style {
            life: [u16_at(b, 0), u16_at(b, 2)],
            speed: pair(b, 4),
            size: pair(b, 12),
            rotation: pair(b, 20),
            colors: [[b[28], b[29], b[30]], [b[31], b[32], b[33]]],
            start_alpha: [b[41], b[42]],
            count: u16_at(b, 34),
            rate: i32_at(b, 36),
            shape: b[40],
            radius: i32_at(b, 44),
            arc: i32_at(b, 48),
            shape_scale: [i32_at(b, 52), i32_at(b, 56), i32_at(b, 60)],
            force: [pair(b, 64), pair(b, 72), pair(b, 80)],
            velocity: [pair(b, 88), pair(b, 96), pair(b, 104)],
            limit: i32_at(b, 112),
            dampen: i32_at(b, 116),
            spin_speed: pair(b, 120),
            spin_range: pair(b, 128),
            collision: u32_at(b, 136) != 0,
            bounce: i32_at(b, 140),
            collision_dampen: i32_at(b, 144),
            life_loss: i32_at(b, 148),
            kill_speed: i32_at(b, 152),
            radius_scale: i32_at(b, 156),
            samples: CURVES[curve],
            frames: frame_pool,
        };
        r.additive[i] = b[43] != 0;
        if r.styles[i].count == 0
            || r.styles[i].count as usize > super::CAPACITY
            || r.styles[i].rate <= 0
            || r.styles[i].life[0] == 0
        {
            return false;
        }
    }
    for i in 0..emitters {
        let b = &data[e0 + i * EMITTER_BYTES..e0 + (i + 1) * EMITTER_BYTES];
        r.emitters[i] = Emitter {
            scene: b[0],
            owner: u16_at(b, 2),
            source: u32_at(b, 4),
            style: b[8],
            angle_offset: i32_at(b, 12),
            origin: [i32_at(b, 16), i32_at(b, 20), i32_at(b, 24)],
            basis: [
                [i32_at(b, 28), i32_at(b, 32), i32_at(b, 36)],
                [i32_at(b, 40), i32_at(b, 44), i32_at(b, 48)],
                [i32_at(b, 52), i32_at(b, 56), i32_at(b, 60)],
            ],
        };
        if r.emitters[i].style as usize >= styles || r.emitters[i].scene as usize != scene {
            return false;
        }
    }
    for i in 0..art {
        let b = &data[a0 + i * ART_BYTES..a0 + (i + 1) * ART_BYTES];
        r.art[i] = Art {
            u: b[0],
            v: b[1],
            w: b[2],
            h: b[3],
            clut: u16_at(b, 4),
            tpage: u16_at(b, 6),
        };
    }
    let mut rects = [[0u16; 4]; FX_MAX_UPLOADS];
    for (i, rect) in rects[..uploads].iter_mut().enumerate() {
        let b = &data[u0 + i * UPLOAD_BYTES..u0 + (i + 1) * UPLOAD_BYTES];
        *rect = [u16_at(b, 4), u16_at(b, 6), u16_at(b, 8), u16_at(b, 10)];
    }
    soft_styles(r, styles, &rects[..uploads]);
    let mut copied = 0;
    for i in 0..uploads {
        let b = &data[u0 + i * UPLOAD_BYTES..u0 + (i + 1) * UPLOAD_BYTES];
        let (offset, x, y, w, h) = (
            u32_at(b, 0) as usize,
            u16_at(b, 4),
            u16_at(b, 6),
            u16_at(b, 8),
            u16_at(b, 10),
        );
        let end = offset + w as usize * h as usize * 2;
        if offset < texel_base || end > data.len() {
            return false;
        }
        psx_vram::upload_bytes(psx_vram::VramRect::new(x, y, w, h), &data[offset..end]);
        // A soft palette also goes to its slot with every coloured entry's
        // semi-transparency bit set, so the whole sprite blends.
        if h != 1 {
            continue;
        }
        for k in 0..(w / 16) as usize {
            let Some(copy) = soft_clut(r, clut_word(x + 16 * k as u16, y)) else {
                continue;
            };
            let mut entries = [0u8; 32];
            entries.copy_from_slice(&data[offset + k * 32..offset + k * 32 + 32]);
            for e in entries.chunks_exact_mut(2) {
                let v = u16::from_le_bytes([e[0], e[1]]);
                if v != 0 {
                    e.copy_from_slice(&(v | 0x8000).to_le_bytes());
                }
            }
            psx_vram::upload_bytes(
                psx_vram::VramRect::new((copy & 63) << 4, copy >> 6, 16, 1),
                &entries,
            );
            copied += 1;
        }
    }
    // Every copy a soft style draws through must exist, or none is used.
    if copied < r.soft_clut_count {
        r.soft = [false; FX_MAX_STYLES];
        r.soft_clut_count = 0;
    }
    unsafe {
        HK_SOFT_PALETTES = r.soft_clut_count as u32;
    }
    r.hero_dust = r.emitters[..emitters]
        .iter()
        .find(|e| e.owner == HERO_DUST_OWNER)
        .map_or(NO_STYLE, |e| e.style);
    r.style_count = styles;
    r.emitter_count = emitters;
    r.art_count = art;
    r.scene = scene;
    true
}
#[inline]
fn checkpoint() {
    #[cfg(not(test))]
    crate::input::checkpoint();
}
/// Exact `pair[0]+(delta*seed)/255` without the software 64-bit division:
/// 65536 = 257*255+1, so for n = a*65536+b, n/255 = a*257+(a+b)/255.
fn lerp(pair: [i32; 2], seed: u8) -> i32 {
    if seed == 0 || pair[0] == pair[1] {
        return pair[0];
    }
    if seed == 255 {
        return pair[1];
    }
    // A difference under 2^23 keeps the product in 32 bits: one multiply and
    // one division by the constant instead of the 64-bit pair below.
    let d = pair[1].wrapping_sub(pair[0]);
    if (pair[1] >= pair[0]) == (d >= 0) && (d as u32).wrapping_add(1 << 23) < 1 << 24 {
        let n = d * seed as i32;
        let q = (n.unsigned_abs() / 255) as i32;
        return pair[0] + if n < 0 { -q } else { q };
    }
    let n = (pair[1] as i64 - pair[0] as i64) * seed as i64;
    let m = n.unsigned_abs();
    let a = (m >> 16) as u32;
    let b = (m & 0xffff) as u32;
    let q = a as u64 * 257 + ((a + b) / 255) as u64;
    pair[0] + (if n < 0 { q.wrapping_neg() } else { q }) as i32
}
/// Clamping before division preserves the source fraction exactly. The authored
/// 3..12 speed interval needs at most150405120 in the numerator, so only future
/// wider ranges use the software64-bit division. Fast particles need no divide.
fn speed_fraction(speed: i32, range: [i32; 2]) -> u8 {
    if speed <= range[0] {
        return 0;
    }
    if speed >= range[1] {
        return 255;
    }
    let delta = (speed as i64 - range[0] as i64) as u32;
    let span = (range[1] as i64 - range[0] as i64) as u32;
    match delta.checked_mul(255) {
        Some(n) => (n / span) as u8,
        None => (delta as u64 * 255 / span as u64) as u8,
    }
}
/// Preserve truncation toward zero; small components (notably source depth)
/// use the R3000's32-bit divider without changing the general wide fallback.
pub(super) fn scale_velocity(v: i32, scale: i32, speed: i32) -> i32 {
    match v.checked_mul(scale) {
        Some(n) => n / speed,
        None if speed > 0 && scale >= 0 && scale <= speed => {
            // Damping cannot increase magnitude, including i32::MIN. Thus the
            // unsigned quotient fits32 bits; signed restoration preserves the
            // old truncation toward zero without a software wide divider.
            let numerator = v.unsigned_abs() as u64 * scale as u64;
            let q = psx_math::int32::div_u64_by_u32(
                (numerator >> 32) as u32,
                numerator as u32,
                speed as u32,
            );
            if v < 0 {
                q.wrapping_neg() as i32
            } else {
                q as i32
            }
        }
        None => (v as i64 * scale as i64 / speed as i64) as i32,
    }
}
fn phase(p: &Particle) -> usize {
    (p.age as usize * 32 / p.life.get() as usize).min(32)
}
fn sample(p: &Particle, style: &Style) -> Sample {
    style.samples[phase(p)]
}
// Baked tracks were retired (BAKE_TRACKS=False); every emitter uses the scalar path.
fn generated_tracks(_styles: &[Style]) -> &'static [[[u32; 2]; 33]] {
    &[]
}
#[inline]
fn cached_frame(track: u16, phase: usize, tracks: &[[[u32; 2]; 33]]) -> Option<[u32; 2]> {
    if track == 0 {
        None
    } else {
        tracks.get(track as usize - 1).map(|t| t[phase])
    }
}
#[inline]
fn frame_spin(words: [u32; 2]) -> i32 {
    (words[1] << 6) as i32 >> 6
}
#[inline]
fn tick_shape(p: &Particle, s: &Style, words: Option<[u32; 2]>) -> (i32, i32) {
    if let Some(w) = words {
        (frame_spin(w), (w[0] >> 16) as i32)
    } else {
        let sample = sample(p, s);
        (
            lerp(sample.spin, p.gradient),
            mulq(
                mulq(p.size, lerp(sample.size, p.gradient)) / 2,
                s.radius_scale,
            ),
        )
    }
}
#[inline]
fn draw_shape(p: &Particle, s: &Style, words: Option<[u32; 2]>) -> (i32, i32) {
    if let Some(w) = words {
        ((w[0] & 65535) as i32, ((w[1] >> 26) & 3) as i32)
    } else {
        let sample = sample(p, s);
        // The style's baked curve, then this particle's share of the start
        // alpha: lerp from zero is an exact multiply by start_alpha/255, and a
        // 255 share takes lerp's own identity path.
        let alpha = lerp(
            [0, lerp(sample.alpha.map(i32::from), p.gradient)],
            p.start_alpha,
        );
        (
            mulq(p.size, lerp(sample.size, p.gradient)) / 2,
            ((alpha * 3 + 127) / 255).clamp(0, 3),
        )
    }
}
#[inline(never)]
pub fn spawn_specs(
    pool: &mut Pool,
    scene: usize,
    owner: usize,
    kind: u8,
    facing: i32,
    emitters: &[Emitter],
    styles: &[Style],
) {
    spawn_specs_with_bases(
        pool,
        scene,
        owner,
        kind,
        facing,
        emitters,
        styles,
        &[],
        [0; 2],
    );
}
/// `spawn_specs` with every emitter moved by `offset`: an effect cooked at one
/// place and raised wherever its owner is now (a fallen stalactite's dust).
pub fn spawn_specs_at(
    pool: &mut Pool,
    scene: usize,
    owner: usize,
    emitters: &[Emitter],
    styles: &[Style],
    offset: [i32; 2],
) {
    spawn_specs_with_bases(pool, scene, owner, 2, 1, emitters, styles, &[], offset);
}
#[inline(never)]
fn spawn_specs_with_bases(
    pool: &mut Pool,
    scene: usize,
    owner: usize,
    kind: u8,
    facing: i32,
    emitters: &[Emitter],
    styles: &[Style],
    bases: &[u16],
    shift: [i32; 2],
) {
    for (emitter_index, e) in emitters
        .iter()
        .enumerate()
        .filter(|(_, e)| e.scene as usize == scene && e.owner as usize == owner)
    {
        let s = &styles[e.style as usize];
        assert!(
            s.samples.len() == 33
                && s.rate > 0
                && s.count as usize <= super::CAPACITY
                && s.life[0] > 0
        );
        let mut rng = e.source ^ 0xd1b54a35;
        let mut search = 0;
        for i in 0..s.count {
            checkpoint();
            while search < super::CAPACITY && pool.particles[search].is_some() {
                search += 1;
            }
            if search == super::CAPACITY {
                pool.dropped = pool.dropped.saturating_add((s.count - i) as u32);
                break;
            }
            let life = range(&mut rng, s.life.map(i32::from)) as u16;
            let speed = range(&mut rng, s.speed);
            let size = range(&mut rng, s.size);
            let angle = range(&mut rng, s.rotation);
            let gradient = (super::random(&mut rng) >> 24) as u8;
            // Unity's random-between-two start colours. One seed drives all
            // four channels, so a particle lands on the line between the ends
            // rather than in the box they span. Two identical ends draw nothing,
            // which keeps every already-admitted one-colour emitter's stream and
            // so its trajectories unchanged.
            let (color, start_alpha) =
                if s.colors[0] == s.colors[1] && s.start_alpha[0] == s.start_alpha[1] {
                    (s.colors[0], s.start_alpha[0])
                } else {
                    let tint = (super::random(&mut rng) >> 24) as u8;
                    (
                        core::array::from_fn(|j| {
                            lerp([s.colors[0][j] as i32, s.colors[1][j] as i32], tint) as u8
                        }),
                        lerp(s.start_alpha.map(i32::from), tint) as u8,
                    )
                };
            let (local, direction) = if s.shape == 10 {
                let a = range(&mut rng, [0, s.arc]);
                let unit = crate::world::debris::rotate([ONE, 0], a);
                // Area-uniform circle sector; radius sampled through integer sqrt.
                let radial = range(&mut rng, [0, ONE]);
                let root = integer_sqrt(radial as u64 * ONE as u64) as i32;
                let p = [
                    mulq(mulq(unit[0], root), s.radius),
                    mulq(mulq(unit[1], root), s.radius),
                    0,
                ];
                (p, [unit[0], unit[1], 0])
            } else {
                assert_eq!(s.shape, 5);
                (
                    core::array::from_fn(|j| {
                        mulq(range(&mut rng, [-ONE / 2, ONE / 2]), s.shape_scale[j])
                    }),
                    [0, 0, ONE],
                )
            };
            let offset = if kind >= 2 {
                0
            } else {
                e.angle_offset * facing
            };
            let rotate = |v: [i32; 3]| {
                let xy = crate::world::debris::rotate([v[0], v[1]], offset);
                [xy[0], xy[1], v[2]]
            };
            let local = rotate(local);
            let direction = rotate(direction);
            let direction = core::array::from_fn::<_, 3, _>(|j| dot3(e.basis[j], direction));
            let norm = length3(direction).max(1);
            let velocity = core::array::from_fn(|j| {
                ((direction[j] as i64 * speed as i64) / norm as i64) as i32
                    + range(&mut rng, ordered(s.velocity[j]))
            });
            // Only trusted generated emitter/style slices supply a base. Generic
            // callers and unpackable future emitters keep the scalar path.
            pool.tracks[search] = bases
                .get(emitter_index)
                .copied()
                .filter(|&b| b != u16::MAX)
                .and_then(|b| b.checked_add(i))
                .and_then(|t| t.checked_add(1))
                .unwrap_or(0);
            pool.placed(search);
            pool.particles[search] = Some(Particle {
                position: core::array::from_fn(|j| {
                    e.origin[j] + (if j < 2 { shift[j] } else { 0 }) + dot3(e.basis[j], local)
                }),
                velocity,
                size,
                force: lerp(s.force[1], gradient) / 60,
                angle,
                scene: e.scene,
                kind: e.style + 2,
                life: super::NonZeroU16::new(life).expect("positive particle lifetime"),
                age: 0,
                delay: (((i as u64 + 1) * 60 * ONE as u64 - 1) / s.rate as u64).min(255) as u8,
                cell: (super::random(&mut rng) % s.frames.len() as u32) as u8,
                gradient,
                color,
                start_alpha,
            });
            pool.active_count += 1;
            pool.spawned = pool.spawned.saturating_add(1);
            search += 1;
        }
    }
    pool.publish();
}
fn ordered(p: [i32; 2]) -> [i32; 2] {
    [p[0].min(p[1]), p[0].max(p[1])]
}
fn integer_sqrt(n: u64) -> u64 {
    psx_math::int32::isqrt_u64(n) as u64
}
/// Non-empty edges are decoded once per tick into this bounded scratch; the
/// callback returns 16 bytes through memory and a burst visits ~10k edges per
/// tick. Larger future rooms keep the direct callback rather than dropping terrain.
const EDGE_SCRATCH: usize = 128;
/// Swept-circle extent for this tick; identical inputs give identical bounds in both passes.
fn sweep(p: &Particle) -> [i32; 3] {
    core::array::from_fn(|j| p.position[j] + p.velocity[j] / 60)
}
fn overlaps(e: &[i32; 4], b: &[i32; 4]) -> bool {
    e[0].min(e[2]) <= b[2]
        && e[0].max(e[2]) >= b[0]
        && e[1].min(e[3]) <= b[3]
        && e[1].max(e[3]) >= b[1]
}
/// One existing60Hz simulation callback; unrelated spatial views do not respawn.
/// Pass one integrates every particle and unions their swept boxes; only edges
/// touching that union can pass any particle's own box test, so pass two
/// collides against that exact subset. Particles never interact, so the split
/// preserves the sequential per-particle result.
#[inline(never)]
pub fn tick_specs(
    pool: &mut Pool,
    scene: usize,
    styles: &[Style],
    coverage: [i32; 4],
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) {
    tick_specs_with_tracks(
        pool,
        scene,
        styles,
        coverage,
        count,
        edge,
        generated_tracks(styles),
    );
}
#[inline(never)]
fn tick_specs_with_tracks(
    pool: &mut Pool,
    scene: usize,
    styles: &[Style],
    coverage: [i32; 4],
    count: usize,
    edge: impl Fn(usize) -> [i32; 4],
    tracks: &[[[u32; 2]; 33]],
) {
    let mut union = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    let mut pending = [0u32; super::CAPACITY / 32];
    // Pass one alone changes velocity/age. A pending collision keeps its
    // position unchanged, so these exact swept values remain valid in pass two.
    // Write a slot before setting its pending bit; no other slot is read.
    let mut sweeps = [core::mem::MaybeUninit::<([i32; 3], i32)>::uninit(); super::CAPACITY];
    let span = pool.span();
    for (index, slot) in pool.particles[..span].iter_mut().enumerate() {
        let Some(p) = slot.as_mut() else {
            continue;
        };
        if p.scene as usize != scene || p.kind < 2 {
            continue;
        }
        checkpoint();
        if p.delay > 0 {
            p.delay -= 1;
            continue;
        }
        p.age += 1;
        if p.age >= p.life.get() {
            *slot = None;
            pool.active_count -= 1;
            continue;
        }
        let s = &styles[(p.kind - 2) as usize];
        if s.collision
            && (p.position[0] < coverage[0]
                || p.position[0] > coverage[2]
                || p.position[1] < coverage[1]
                || p.position[1] > coverage[3])
        {
            continue;
        }
        let words = cached_frame(pool.tracks[index], phase(p), tracks);
        let (base_spin, radius) = tick_shape(p, s, words);
        p.velocity[1] += p.force;
        for j in [0, 2] {
            if (s.force[j][0] | s.force[j][1]) != 0 {
                p.velocity[j] += lerp(s.force[j], p.gradient) / 60;
            }
        }
        let spin_by_speed =
            (s.spin_speed[0] | s.spin_speed[1]) != 0 && s.spin_range[1] > s.spin_range[0];
        // The root only matters for spin or above the limit: isqrt(L) > limit
        // exactly when L >= (limit+1)^2, so below it the square decides alone.
        let speed = if spin_by_speed
            || (s.limit >= 0
                && super::length_squared(p.velocity) >= (s.limit as u64 + 1) * (s.limit as u64 + 1))
        {
            length3(p.velocity)
        } else {
            0
        };
        if s.limit >= 0 && speed > s.limit {
            let limited = speed - mulq(speed - s.limit, s.dampen);
            for v in &mut p.velocity {
                *v = scale_velocity(*v, limited, speed);
            }
        }
        let spin = base_spin
            + if spin_by_speed {
                lerp(s.spin_speed, speed_fraction(speed, s.spin_range))
            } else {
                0
            };
        if spin != 0 || !(0..360 * ONE).contains(&p.angle) {
            p.angle = (p.angle + spin / 60).rem_euclid(360 * ONE);
        }
        let next = sweep(p);
        if s.collision {
            let old = p.position;
            union = [
                union[0].min(next[0].min(old[0]) - radius),
                union[1].min(next[1].min(old[1]) - radius),
                union[2].max(next[0].max(old[0]) + radius),
                union[3].max(next[1].max(old[1]) + radius),
            ];
            sweeps[index].write((next, radius));
            pending[index / 32] |= 1 << (index % 32);
        } else {
            p.position = next;
        }
    }
    if pending.iter().fold(0, |bits, &word| bits | word) != 0 {
        let mut scratch = [core::mem::MaybeUninit::<[i32; 4]>::uninit(); EDGE_SCRATCH];
        let mut n = 0;
        if count <= EDGE_SCRATCH {
            for i in 0..count {
                if i & 31 == 0 {
                    checkpoint();
                }
                let e = edge(i);
                if (e[0] | e[1] | e[2] | e[3]) != 0 && overlaps(&e, &union) {
                    scratch[n].write(e);
                    n += 1;
                }
            }
        }
        // Only the initialized prefix is exposed.
        let edges = unsafe { core::slice::from_raw_parts(scratch.as_ptr() as *const [i32; 4], n) };
        for word in 0..pending.len() {
            let mut bits = pending[word];
            while bits != 0 {
                let index = word * 32 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                checkpoint();
                let slot = &mut pool.particles[index];
                let p = slot.as_mut().unwrap();
                let s = &styles[(p.kind - 2) as usize];
                let old = p.position;
                let (mut next, radius) = unsafe { sweeps[index].assume_init() };
                let mut normal = [0, 0];
                if count <= EDGE_SCRATCH {
                    collide(old, &mut next, &mut normal, radius, edges.iter().copied());
                } else {
                    collide(
                        old,
                        &mut next,
                        &mut normal,
                        radius,
                        (0..count)
                            .map(|i| {
                                if i & 31 == 0 {
                                    checkpoint();
                                }
                                edge(i)
                            })
                            .filter(|e| (e[0] | e[1] | e[2] | e[3]) != 0 && overlaps(e, &union)),
                    );
                }
                p.position = next;
                if (normal[0] | normal[1]) != 0 {
                    let unit = if normal[0] == 0 {
                        [0, normal[1].signum() * ONE]
                    } else if normal[1] == 0 {
                        [normal[0].signum() * ONE, 0]
                    } else {
                        let n = length3([normal[0], normal[1], 0]).max(1);
                        normal.map(|v| (v as i64 * ONE as i64 / n as i64) as i32)
                    };
                    let dot = mulq(p.velocity[0], unit[0]) + mulq(p.velocity[1], unit[1]);
                    for j in 0..2 {
                        p.velocity[j] = mulq(
                            p.velocity[j] - mulq(ONE + s.bounce, mulq(dot, unit[j])),
                            ONE - s.collision_dampen,
                        );
                    }
                    p.age = p.age.saturating_add(
                        ((p.life.get() as u32 * s.life_loss.max(0) as u32) / 65536).min(65535)
                            as u16,
                    );
                    if (s.kill_speed > 0
                        && super::length_squared(p.velocity)
                            < s.kill_speed as u64 * s.kill_speed as u64)
                        || p.age >= p.life.get()
                    {
                        *slot = None;
                        pool.active_count -= 1;
                    }
                }
            }
        }
    }
    pool.publish();
}
/// Sequential swept-circle contacts against every non-empty edge, in edge order.
#[inline(never)]
fn collide(
    old: [i32; 3],
    next: &mut [i32; 3],
    normal: &mut [i32; 2],
    radius: i32,
    edges: impl Iterator<Item = [i32; 4]>,
) {
    for [x0, y0, x1, y1] in edges {
        if next[0].max(old[0]) + radius < x0.min(x1)
            || next[0].min(old[0]) - radius > x0.max(x1)
            || next[1].max(old[1]) + radius < y0.min(y1)
            || next[1].min(old[1]) - radius > y0.max(y1)
        {
            continue;
        }
        if x0 == x1 {
            if next[0] > old[0] && old[0] + radius <= x0 && next[0] + radius > x0 {
                next[0] = x0 - radius;
                *normal = [-ONE, 0];
            } else if next[0] < old[0] && old[0] - radius >= x0 && next[0] - radius < x0 {
                next[0] = x0 + radius;
                *normal = [ONE, 0];
            }
        } else {
            let height = |x: i32| {
                y0 + (((x.clamp(x0.min(x1), x0.max(x1)) - x0) as i64 * (y1 - y0) as i64)
                    / (x1 - x0) as i64) as i32
            };
            let (before, surface) = if y0 == y1 {
                (y0, y0)
            } else {
                (height(old[0]), height(next[0]))
            };
            if next[1] < old[1] && old[1] - radius >= before && next[1] - radius <= surface {
                next[1] = surface + radius;
                *normal = [-(y1 - y0), x1 - x0];
            } else if next[1] > old[1] && old[1] + radius <= before && next[1] + radius >= surface {
                next[1] = surface - radius;
                *normal = [y1 - y0, -(x1 - x0)];
            }
        }
    }
}
#[cfg(not(test))]
#[inline(never)]
pub(super) fn draw_particle(p: &Particle, track: u16, camera: (i32, i32)) -> bool {
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        prim::QuadTextured,
    };
    let s = &scene_styles(p.scene as usize)[(p.kind - 2) as usize];
    let words = cached_frame(track, phase(p), PARTICLE_TRACKS);
    let (half, level) = draw_shape(p, s, words);
    // A soft style (soft_styles) adds its densest art at colour times opacity.
    let resident = unsafe { &*(&raw const RESIDENT) };
    let soft = resident.soft[(p.kind - 2) as usize];
    let (art_level, tint, blend) = if soft {
        let sample = sample(p, s);
        let alpha = lerp(
            [0, lerp(sample.alpha.map(i32::from), p.gradient)],
            p.start_alpha,
        ) as u32;
        if alpha == 0 {
            return false;
        }
        let additive = resident.additive[(p.kind - 2) as usize];
        let gain = if resident.hero_dust == p.kind - 2 {
            HERO_DUST_GAIN_HALVES
        } else {
            2
        };
        let c = p
            .color
            .map(|v| (u32::from(v) * alpha * gain / 510).min(255) as u8);
        unsafe {
            HK_SOFT_PARTICLES_DRAWN = HK_SOFT_PARTICLES_DRAWN.wrapping_add(1);
        }
        let blend = if additive {
            BlendMode::Add
        } else {
            BlendMode::AddQuarter
        };
        (3, (c[0], c[1], c[2]), blend)
    } else {
        if level == 0 {
            return false;
        }
        (
            level,
            (p.color[0], p.color[1], p.color[2]),
            BlendMode::Average,
        )
    };
    let a = scene_art(p.scene as usize)[s.frames[p.cell as usize][art_level as usize - 1] as usize];
    let depth = 2496922 + p.position[2];
    if depth <= ONE {
        return false;
    }
    // Source-plane particles skip the 64-bit software division exactly.
    let scale = if p.position[2] == 0 {
        crate::KNIGHT_SCALE as i64
    } else {
        super::perspective::scale(depth) as i64
    };
    let rotation = crate::world::debris::Rotation::new(p.angle);
    let vertices = [[-half, half], [half, half], [-half, -half], [half, -half]].map(|q| {
        let v = rotation.apply(q);
        (
            160 + (((p.position[0] as i64 + v[0] as i64 - camera.0 as i64) * scale) >> 28) as i32,
            120 - (((p.position[1] as i64 + v[1] as i64 - camera.1 as i64) * scale) >> 28) as i32,
        )
    });
    if vertices.iter().all(|p| p.0 < 0)
        || vertices.iter().all(|p| p.0 >= 320)
        || vertices.iter().all(|p| p.1 < 0)
        || vertices.iter().all(|p| p.1 >= 240)
    {
        return false;
    }
    let r = (a.u as u16 + a.w as u16 - 1) as u8;
    let b = (a.v as u16 + a.h as u16 - 1) as u8;
    let template = QuadTextured::with_material(
        [(0, 0); 4],
        [(a.u, a.v), (r, a.v), (a.u, b), (r, b)],
        TextureMaterial::blended(
            if soft {
                soft_clut(resident, a.clut).unwrap_or(a.clut)
            } else {
                a.clut
            },
            a.tpage,
            tint,
            blend,
        ),
    );
    crate::render::resident_quad_tinted(
        &template,
        vertices.map(|(x, y)| (x as i16, y as i16)),
        tint,
    );
    true
}
#[cfg(test)]
pub(super) fn draw_particle(_: &Particle, _: u16, _: (i32, i32)) -> bool {
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tick_reference(
        pool: &mut Pool,
        scene: usize,
        styles: &[Style],
        coverage: [i32; 4],
        count: usize,
        edge: impl Fn(usize) -> [i32; 4],
    ) {
        for slot in &mut pool.particles {
            let Some(p) = slot.as_mut() else {
                continue;
            };
            if p.scene as usize != scene || p.kind < 2 {
                continue;
            }
            checkpoint();
            if p.delay > 0 {
                p.delay -= 1;
                continue;
            }
            p.age += 1;
            if p.age >= p.life.get() {
                *slot = None;
                continue;
            }
            let s = &styles[(p.kind - 2) as usize];
            let sample = sample(p, s);
            if s.collision
                && (p.position[0] < coverage[0]
                    || p.position[0] > coverage[2]
                    || p.position[1] < coverage[1]
                    || p.position[1] > coverage[3])
            {
                continue;
            }
            for j in 0..3 {
                p.velocity[j] += lerp_reference(s.force[j], p.gradient) / 60;
            }
            let speed = length3_reference(p.velocity);
            if s.limit >= 0 && speed > s.limit {
                let limited = speed - mulq(speed - s.limit, s.dampen);
                for v in &mut p.velocity {
                    *v = (*v as i64 * limited as i64 / speed as i64) as i32;
                }
            }
            let spin = lerp_reference(sample.spin, p.gradient)
                + if s.spin_range[1] > s.spin_range[0] {
                    let f = ((speed - s.spin_range[0]) as i64 * 255
                        / (s.spin_range[1] - s.spin_range[0]) as i64)
                        .clamp(0, 255) as u8;
                    lerp_reference(s.spin_speed, f)
                } else {
                    0
                };
            p.angle = (p.angle + spin / 60).rem_euclid(360 * ONE);
            let old = p.position;
            let mut next = core::array::from_fn(|j| old[j] + p.velocity[j] / 60);
            let mut normal = [0, 0];
            if s.collision {
                let radius = mulq(
                    mulq(p.size, lerp_reference(sample.size, p.gradient)) / 2,
                    s.radius_scale,
                );
                for i in 0..count {
                    if i & 31 == 0 {
                        checkpoint();
                    }
                    let [x0, y0, x1, y1] = edge(i);
                    if [x0, y0, x1, y1] == [0; 4] {
                        continue;
                    }
                    if next[0].max(old[0]) + radius < x0.min(x1)
                        || next[0].min(old[0]) - radius > x0.max(x1)
                        || next[1].max(old[1]) + radius < y0.min(y1)
                        || next[1].min(old[1]) - radius > y0.max(y1)
                    {
                        continue;
                    }
                    if x0 == x1 {
                        if next[0] > old[0] && old[0] + radius <= x0 && next[0] + radius > x0 {
                            next[0] = x0 - radius;
                            normal = [-ONE, 0];
                        } else if next[0] < old[0] && old[0] - radius >= x0 && next[0] - radius < x0
                        {
                            next[0] = x0 + radius;
                            normal = [ONE, 0];
                        }
                    } else {
                        let height = |x: i32| {
                            y0 + (((x.clamp(x0.min(x1), x0.max(x1)) - x0) as i64
                                * (y1 - y0) as i64)
                                / (x1 - x0) as i64) as i32
                        };
                        let before = height(old[0]);
                        let surface = height(next[0]);
                        if next[1] < old[1]
                            && old[1] - radius >= before
                            && next[1] - radius <= surface
                        {
                            next[1] = surface + radius;
                            normal = [-(y1 - y0), x1 - x0];
                        } else if next[1] > old[1]
                            && old[1] + radius <= before
                            && next[1] + radius >= surface
                        {
                            next[1] = surface - radius;
                            normal = [y1 - y0, -(x1 - x0)];
                        }
                    }
                }
            }
            p.position = next;
            if normal != [0, 0] {
                let n = length3_reference([normal[0], normal[1], 0]).max(1);
                let unit = normal.map(|v| (v as i64 * ONE as i64 / n as i64) as i32);
                let dot = mulq(p.velocity[0], unit[0]) + mulq(p.velocity[1], unit[1]);
                for j in 0..2 {
                    p.velocity[j] = mulq(
                        p.velocity[j] - mulq(ONE + s.bounce, mulq(dot, unit[j])),
                        ONE - s.collision_dampen,
                    );
                }
                p.age = p.age.saturating_add(
                    ((p.life.get() as u32 * s.life_loss.max(0) as u32) / 65536).min(65535) as u16,
                );
                if length3_reference(p.velocity) < s.kill_speed || p.age >= p.life.get() {
                    *slot = None;
                }
            }
        }
        pool.active_count = pool.particles.iter().flatten().count() as u32;
        pool.publish();
    }
    fn lerp_reference(pair: [i32; 2], seed: u8) -> i32 {
        if pair[0] == pair[1] {
            pair[0]
        } else {
            pair[0] + (((pair[1] as i64 - pair[0] as i64) * seed as i64) / 255) as i32
        }
    }
    fn length3_reference(v: [i32; 3]) -> i32 {
        let mut n = v.iter().map(|x| *x as i64 * *x as i64).sum::<i64>() as u64;
        let mut result = 0u64;
        let mut bit = 1u64 << 62;
        while bit > n {
            bit >>= 2;
        }
        while bit != 0 {
            if n >= result + bit {
                n -= result + bit;
                result = (result >> 1) + bit;
            } else {
                result >>= 1;
            }
            bit >>= 2;
        }
        result as i32
    }

    const CURVE: [Sample; 33] = [Sample {
        size: [ONE; 2],
        alpha: [255; 2],
        spin: [0; 2],
    }; 33];
    const FRAMES: [[u16; 3]; 4] = [[0, 1, 2]; 4];
    const STYLE: Style = Style {
        life: [60, 60],
        speed: [0, 0],
        size: [ONE, ONE],
        rotation: [0, 0],
        colors: [[128; 3]; 2],
        start_alpha: [255; 2],
        count: 30,
        rate: 5000 * ONE,
        shape: 10,
        radius: 0,
        arc: 180 * ONE,
        shape_scale: [ONE; 3],
        force: [[0, 0]; 3],
        velocity: [[0, 0]; 3],
        limit: -1,
        dampen: 0,
        spin_speed: [0, 0],
        spin_range: [0, ONE],
        collision: false,
        bounce: 39322,
        collision_dampen: 3277,
        life_loss: 0,
        kill_speed: 0,
        radius_scale: ONE,
        samples: &CURVE,
        frames: &FRAMES,
    };
    const EMITTER: Emitter = Emitter {
        scene: 0,
        owner: 7,
        source: 3860,
        style: 0,
        angle_offset: 0,
        origin: [0, 3 * ONE, 0],
        basis: [[ONE, 0, 0], [0, ONE, 0], [0, 0, ONE]],
    };
    const COVER: [i32; 4] = [-100 * ONE, -100 * ONE, 100 * ONE, 100 * ONE];
    #[test]
    fn integer_norm_matches_previous_restoring_algorithm() {
        for v in [
            [0; 3],
            [ONE, 0, 0],
            [-ONE, 0, 0],
            [1, 1, 1],
            [i32::MAX, 0, 0],
            [1234567, -7654321, 987654],
            [-180 * ONE, 300 * ONE, 20 * ONE],
        ] {
            assert_eq!(length3(v), length3_reference(v), "{v:?}");
        }
        let mut rng = 0x12345678u32;
        for _ in 0..20000 {
            let v = core::array::from_fn(|_| {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                (rng as i32) / 4
            });
            assert_eq!(length3(v), length3_reference(v), "{v:?}");
        }
    }
    #[test]
    fn integer_lerp_matches_previous_wide_division() {
        for pair in [
            [0, 0],
            [ONE, -ONE],
            [-2949120, 32768001],
            [32768001, -2949120],
            [5, -7],
            [1, 0],
            [-600 * ONE, 600 * ONE],
        ] {
            for seed in [0u8, 1, 2, 127, 128, 254, 255] {
                assert_eq!(
                    lerp(pair, seed),
                    lerp_reference(pair, seed),
                    "{pair:?} {seed}"
                );
            }
        }
        let mut rng = 0x9e3779b9u32;
        for _ in 0..50000 {
            let pair = core::array::from_fn(|_| {
                rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                (rng as i32) / 4
            });
            let seed = (rng >> 24) as u8;
            assert_eq!(
                lerp(pair, seed),
                lerp_reference(pair, seed),
                "{pair:?} {seed}"
            );
        }
    }
    #[test]
    fn speed_fraction_preserves_clamps_truncation_and_wide_fallback() {
        for bounds in [
            [3 * ONE, 12 * ONE],
            [0, ONE],
            [-10 * ONE, 10 * ONE],
            [-1_000_000_000, 1_000_000_000],
        ] {
            for speed in [
                i32::MIN,
                bounds[0],
                bounds[0] + 1,
                0,
                bounds[1] - 1,
                bounds[1],
                i32::MAX,
            ] {
                let expected = ((speed as i64 - bounds[0] as i64) * 255
                    / (bounds[1] as i64 - bounds[0] as i64))
                    .clamp(0, 255) as u8;
                assert_eq!(
                    speed_fraction(speed, bounds),
                    expected,
                    "{speed} {bounds:?}"
                );
            }
        }
        for speed in (0..20 * ONE).step_by(7) {
            let expected =
                ((speed as i64 - 3 * ONE as i64) * 255 / (9 * ONE as i64)).clamp(0, 255) as u8;
            assert_eq!(speed_fraction(speed, [3 * ONE, 12 * ONE]), expected);
        }
    }
    #[test]
    fn velocity_scale_preserves_signed_rounding_and_wide_products() {
        for v in [i32::MIN, -30 * ONE, -100, -1, 0, 1, 100, 30 * ONE, i32::MAX] {
            for scale in [0, 1, 786432, i32::MAX] {
                for speed in [1, 7, ONE, 786432, i32::MAX] {
                    assert_eq!(
                        scale_velocity(v, scale, speed),
                        (v as i64 * scale as i64 / speed as i64) as i32
                    );
                }
            }
        }
    }
    #[test]
    fn narrowed_velocity_division_keeps_signed_extremes_and_general_fallback() {
        let mut seed = 0x218f937au32;
        for i in 0..300000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let v = seed as i32;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let speed = (seed & 0x7fffffff).max(1) as i32;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let scale = if i % 4 == 0 {
                seed as i32
            } else {
                (seed % (speed as u32 + 1)) as i32
            };
            assert_eq!(
                scale_velocity(v, scale, speed),
                (v as i64 * scale as i64 / speed as i64) as i32
            );
        }
        for v in [i32::MIN, -65537, -1, 0, 1, 65537, i32::MAX] {
            for speed in [i32::MIN, -65537, -7] {
                for scale in [i32::MIN, -ONE, 0, ONE, i32::MAX] {
                    assert_eq!(
                        scale_velocity(v, scale, speed),
                        (v as i64 * scale as i64 / speed as i64) as i32
                    );
                }
            }
        }
        for v in [i32::MIN, i32::MIN + 1, -65537, -1, 0, 1, 65537, i32::MAX] {
            for speed in [1, 65535, 65536, 65537, i32::MAX] {
                for scale in [0, 1, speed / 2, speed] {
                    assert_eq!(
                        scale_velocity(v, scale, speed),
                        (v as i64 * scale as i64 / speed as i64) as i32
                    );
                }
            }
        }
    }
    #[test]
    fn optimized_full_pool_flight_matches_naive_every_tick() {
        flight_matches_naive(0, false);
    }
    #[test]
    fn rooms_beyond_edge_scratch_keep_the_direct_callback() {
        flight_matches_naive(EDGE_SCRATCH, false);
    }
    #[test]
    fn precomputed_shape_tracks_preserve_full_trajectories_and_draw_fields() {
        flight_matches_naive(0, true);
        flight_matches_naive(EDGE_SCRATCH, true);
    }
    fn flight_matches_naive(padding: usize, cached: bool) {
        const SPIN_CURVE: [Sample; 33] = [Sample {
            size: [ONE / 2, ONE],
            alpha: [0, 255],
            spin: [-200 * ONE, 200 * ONE],
        }; 33];
        // 52+79+42+52 authors one particle more than the pool holds, so the
        // flight still runs against a full pool with exactly one drop.
        let styles = [
            Style {
                count: 52,
                life: [120, 600],
                speed: [3 * ONE, 30 * ONE],
                force: [[0, 0], [-86 * ONE, -82 * ONE], [0, 0]],
                velocity: [[0, 0], [0, 30 * ONE], [0, 0]],
                limit: 12 * ONE,
                dampen: 19661,
                spin_speed: [3 * ONE, 12 * ONE],
                spin_range: [0, ONE],
                collision: true,
                kill_speed: ONE,
                radius_scale: 41288,
                samples: &SPIN_CURVE,
                ..STYLE
            },
            Style {
                count: 79,
                rate: 150 * ONE,
                life: [36, 60],
                shape: 5,
                speed: [0, ONE],
                size: [ONE / 2, 2 * ONE],
                force: [[0, 0], [-4 * ONE, -ONE], [0, 0]],
                ..STYLE
            },
            Style {
                count: 42,
                life: [60, 480],
                speed: [5 * ONE, 25 * ONE],
                force: [[-ONE, ONE], [-50 * ONE, -40 * ONE], [-ONE, ONE]],
                limit: 0,
                dampen: 3932,
                collision: true,
                life_loss: 19661,
                kill_speed: ONE,
                radius_scale: 41288,
                samples: &SPIN_CURVE,
                ..STYLE
            },
            Style {
                count: 52,
                life: [600, 600],
                speed: [3 * ONE, 15 * ONE],
                force: [[0, 0], [-60 * ONE, -40 * ONE], [0, 0]],
                collision: true,
                kill_speed: 0,
                ..STYLE
            },
        ];
        let emitters = core::array::from_fn::<_, 4, _>(|i| Emitter {
            style: i as u8,
            source: 3860 + i as u32,
            origin: [i as i32 * ONE, 3 * ONE, 0],
            ..EMITTER
        });
        // Include both orientations of slopes, floor, ceiling and wall.
        let edges = [
            [-20 * ONE, 0, 20 * ONE, 0],
            [6 * ONE, -ONE, 6 * ONE, 10 * ONE],
            [-6 * ONE, 10 * ONE, -6 * ONE, -ONE],
            [-20 * ONE, 8 * ONE, 20 * ONE, 8 * ONE],
            [4 * ONE, 0, 8 * ONE, 2 * ONE],
            [0, 0, -4 * ONE, 2 * ONE],
        ];
        let (mut fast, mut reference) = (Pool::new(), Pool::new());
        for p in [&mut fast, &mut reference] {
            spawn_specs(p, 0, 7, 0, 1, &emitters, &styles);
        }
        assert_eq!(fast.active(), super::super::CAPACITY);
        assert_eq!(fast.dropped, 1);
        // Force contact cases as well as random flight, and canonicalize a360-degree spawn.
        for p in [&mut fast, &mut reference] {
            for (i, v) in [
                [0, -30 * ONE, 0],
                [30 * ONE, 0, 0],
                [-30 * ONE, 0, 0],
                [0, 30 * ONE, 0],
            ]
            .into_iter()
            .enumerate()
            {
                let q = p.particles[i].as_mut().unwrap();
                q.position = [0, 3 * ONE, 0];
                q.velocity = v;
                q.angle = 360 * ONE;
            }
        }
        // Build exact immutable phase values once for this synthetic fixture.
        // Production obtains these from the host; no guest spawn does this work.
        let mut tracks = Vec::new();
        if cached {
            for (i, p) in fast.particles.iter().enumerate() {
                if let Some(p) = p {
                    let s = &styles[(p.kind - 2) as usize];
                    let mut valid = true;
                    let track = core::array::from_fn(|phase| {
                        let sample = &s.samples[phase];
                        let half = mulq(p.size, lerp_reference(sample.size, p.gradient)) / 2;
                        let radius = mulq(half, s.radius_scale);
                        let spin = lerp_reference(sample.spin, p.gradient);
                        let alpha = lerp_reference(sample.alpha.map(i32::from), p.gradient);
                        let level = ((alpha * 3 + 127) / 255).clamp(0, 3);
                        if !(0..=65535).contains(&half)
                            || !(0..=65535).contains(&radius)
                            || !(-(1 << 25)..(1 << 25)).contains(&spin)
                        {
                            valid = false;
                        }
                        [
                            (half as u32 & 65535) | ((radius as u32 & 65535) << 16),
                            (spin as u32 & 0x3ffffff) | ((level as u32) << 26),
                        ]
                    });
                    if valid {
                        fast.tracks[i] = tracks.len() as u16 + 1;
                        tracks.push(track);
                    }
                }
            }
        }
        if cached {
            assert!(
                tracks.len() > 100,
                "cached trajectory must exercise the packed path"
            );
        }
        for tick in 0..650 {
            let coverage = if (70..100).contains(&tick) {
                [-2 * ONE, -ONE, 2 * ONE, ONE]
            } else {
                COVER
            };
            let edge = |i| {
                if i >= edges.len() || (tick >= 140 && i == 0) {
                    [0; 4]
                } else {
                    edges[i]
                }
            };
            let cached = core::array::from_fn::<_, 6, _>(edge);
            let scene = if tick % 53 == 0 { 1 } else { 0 };
            tick_specs_with_tracks(
                &mut fast,
                scene,
                &styles,
                coverage,
                edges.len() + padding,
                |i| if i < cached.len() { cached[i] } else { [0; 4] },
                &tracks,
            );
            tick_reference(
                &mut reference,
                scene,
                &styles,
                coverage,
                edges.len() + padding,
                edge,
            );
            assert_eq!(
                format!("{:?}", fast.particles),
                format!("{:?}", reference.particles),
                "tick{tick}"
            );
            for (i, slot) in fast.particles.iter().enumerate() {
                if let Some(p) = slot {
                    let s = &styles[(p.kind - 2) as usize];
                    let words = cached_frame(fast.tracks[i], phase(p), &tracks);
                    assert_eq!(draw_shape(p, s, words), draw_shape(p, s, None));
                    assert_eq!(tick_shape(p, s, words), tick_shape(p, s, None));
                    let half = draw_shape(p, s, words).0;
                    super::super::perspective::assert_vertices(
                        p.position,
                        p.angle,
                        half * 2,
                        (tick * 7919, -tick * 3571),
                    );
                }
            }
            assert_eq!(
                (fast.spawned, fast.dropped, fast.drawn),
                (reference.spawned, reference.dropped, reference.drawn)
            );
        }
        assert_eq!(fast.active(), 0);
    }
    #[test]
    fn cached_sweeps_preserve_sparse_pending_slots_and_age_varying_curves() {
        const fn curve() -> [Sample; 33] {
            let mut out = [Sample {
                size: [ONE; 2],
                alpha: [255; 2],
                spin: [0; 2],
            }; 33];
            let mut i = 0;
            while i < 33 {
                out[i].size = [ONE / 4 + i as i32 * 1577, 2 * ONE - i as i32 * 1139];
                i += 1;
            }
            out
        }
        static CURVE: [Sample; 33] = curve();
        // One emitter fills every slot, so the loop below can rewrite them all.
        let styles = [
            Style {
                count: super::super::CAPACITY as u16,
                life: [60; 2],
                speed: [ONE, 10 * ONE],
                collision: true,
                force: [[0; 2]; 3],
                radius_scale: 41288,
                samples: &CURVE,
                ..STYLE
            },
            Style {
                collision: false,
                ..STYLE
            },
        ];
        let (mut fast, mut reference) = (Pool::new(), Pool::new());
        for pool in [&mut fast, &mut reference] {
            spawn_specs(pool, 0, 7, 0, 1, &[EMITTER], &styles);
            for (i, slot) in pool.particles.iter_mut().enumerate() {
                let p = slot.as_mut().unwrap();
                p.age = (i % 60) as u16;
                p.delay = (i % 7) as u8;
                p.scene = if i % 11 == 0 { 1 } else { 0 };
                p.kind = if i % 5 == 0 { 3 } else { 2 };
                p.position = [
                    if i % 13 == 0 {
                        101 * ONE
                    } else {
                        (i as i32 % 9 - 4) * ONE
                    },
                    ONE / 2,
                    0,
                ];
                p.velocity = [(i as i32 - 64) * 1577, -3 * ONE, i as i32 * 19];
            }
        }
        for tick in 0..75 {
            let edges = [[-20 * ONE, 0, 20 * ONE, 0], [3 * ONE, 0, 3 * ONE, 5 * ONE]];
            tick_specs(&mut fast, 0, &styles, COVER, edges.len(), |i| edges[i]);
            tick_reference(&mut reference, 0, &styles, COVER, edges.len(), |i| edges[i]);
            assert_eq!(
                format!("{:?}", fast.particles),
                format!("{:?}", reference.particles),
                "tick{tick}"
            );
            assert_eq!(fast.active(), reference.active());
        }
    }
    #[test]
    fn authored_count_capacity_and_owner_are_distinct_from_pool_limit() {
        // STYLE authors 30 particles, so seven bursts fit the 224-slot pool and
        // the eighth is the one the pool limit truncates.
        let mut p = Pool::new();
        spawn_specs(&mut p, 0, 8, 0, 1, &[EMITTER], &[STYLE]);
        assert_eq!(p.active(), 0);
        for _ in 0..7 {
            spawn_specs(&mut p, 0, 7, 0, 1, &[EMITTER], &[STYLE]);
        }
        assert_eq!(p.active(), 210);
        spawn_specs(&mut p, 0, 7, 0, 1, &[EMITTER], &[STYLE]);
        assert_eq!(p.active(), 224);
        assert_eq!(p.spawned, 224);
        assert_eq!(p.dropped, 16);
        assert!(p
            .particles
            .iter()
            .flatten()
            .all(|p| p.delay == 0 && p.kind == 2 && p.cell < 4));
    }
    #[test]
    fn long_source_lifetime_and_scene_changes_do_not_restart_or_truncate() {
        let s = Style {
            life: [600; 2],
            count: 1,
            ..STYLE
        };
        let mut p = Pool::new();
        spawn_specs(&mut p, 0, 7, 0, 1, &[EMITTER], &[s]);
        let cell = p.particles[0].unwrap().cell;
        for _ in 0..300 {
            tick_specs(&mut p, 0, &[s], COVER, 0, |_| [0; 4]);
        }
        assert_eq!(p.particles[0].unwrap().age, 300);
        assert_eq!(p.particles[0].unwrap().cell, cell);
        tick_specs(&mut p, 1, &[s], COVER, 0, |_| [0; 4]);
        assert_eq!(p.particles[0].unwrap().age, 300);
        p.clear_scene(1);
        assert_eq!(p.active(), 1);
        for _ in 0..300 {
            tick_specs(&mut p, 0, &[s], COVER, 0, |_| [0; 4]);
        }
        assert_eq!(p.active(), 0);
    }
    #[test]
    fn deterministic_force_collision_and_removed_floor_change_flight() {
        let s = Style {
            count: 1,
            force: [[0, 0], [-60 * ONE; 2], [0, 0]],
            collision: true,
            life: [600; 2],
            ..STYLE
        };
        let (mut floor, mut missing) = (Pool::new(), Pool::new());
        for p in [&mut floor, &mut missing] {
            spawn_specs(p, 0, 7, 0, 1, &[EMITTER], &[s]);
        }
        for _ in 0..60 {
            tick_specs(&mut floor, 0, &[s], COVER, 1, |_| {
                [-10 * ONE, 0, 10 * ONE, 0]
            });
            tick_specs(&mut missing, 0, &[s], COVER, 1, |_| [0; 4]);
        }
        assert!(floor.particles[0].unwrap().position[1] >= ONE / 2);
        assert!(missing.particles[0].unwrap().position[1] < 0);
        let last = floor.particles[0].unwrap().position[1];
        for _ in 0..60 {
            tick_specs(&mut floor, 0, &[s], COVER, 0, |_| [0; 4]);
        }
        assert!(floor.particles[0].unwrap().position[1] < last);
    }
    #[test]
    fn cached_active_count_tracks_mixed_families_scenes_delays_drops_and_reuse() {
        use super::super::{Bank, EmitterSpec, Sample as LegacySample, Style as LegacyStyle};
        static CURVES: [LegacySample; 65] = [LegacySample {
            size: ONE,
            alpha: [255; 2],
        }; 65];
        // Authored counts are the previous 128-slot fixture scaled by 224/128,
        // which keeps every burst's overflow and every scene's timing intact.
        static LEGACY: [LegacyStyle; 2] = [LegacyStyle {
            life: [3; 2],
            speed: [0; 2],
            size: [ONE; 2],
            force: [0; 2],
            dampen: 0,
            rotation: 0,
            count: 70,
            duration: 6,
            uv_scale: ONE,
            colors: [[128; 3]; 2],
            curves: &CURVES,
        }; 2];
        let bank = Bank {
            frames: [&[], &[]],
            styles: &LEGACY,
            death_offset: [0; 3],
        };
        let source = EmitterSpec {
            state: 0,
            source: 13,
            origin: [0; 3],
            basis: [[ONE, 0, 0], [0, ONE, 0], [0, 0, ONE]],
            direction: [0, 0, ONE],
        };
        let style = Style {
            count: 70,
            life: [3; 2],
            rate: 60 * ONE,
            ..STYLE
        };
        let scan = |p: &Pool| {
            assert_eq!(p.active(), p.particles.iter().flatten().count());
            assert!(p.active() <= super::super::CAPACITY);
        };
        let mut pool = Pool::new();
        scan(&pool);
        pool.spawn_grass(0, source, bank);
        scan(&pool);
        spawn_specs(
            &mut pool,
            1,
            7,
            0,
            1,
            &[Emitter {
                scene: 1,
                ..EMITTER
            }],
            &[style],
        );
        scan(&pool);
        spawn_specs(
            &mut pool,
            0,
            7,
            0,
            1,
            &[EMITTER],
            &[Style {
                count: 105,
                ..style
            }],
        );
        scan(&pool);
        assert_eq!((pool.active(), pool.spawned, pool.dropped), (224, 224, 21));
        spawn_specs(
            &mut pool,
            0,
            7,
            0,
            1,
            &[EMITTER],
            &[Style {
                count: 105,
                ..style
            }],
        );
        scan(&pool);
        assert_eq!((pool.active(), pool.spawned, pool.dropped), (224, 224, 126));
        assert!(pool.particles.iter().flatten().any(|p| p.delay > 0));
        pool.clear_scene(2);
        scan(&pool);
        assert_eq!(pool.active(), 224);
        pool.tick(0, None);
        scan(&pool);
        assert_eq!(pool.active(), 224);
        for tick in 0..72 {
            pool.tick(0, Some(bank));
            scan(&pool);
            pool.tick(2, Some(bank));
            scan(&pool);
            for scene in [0, 1] {
                tick_specs(&mut pool, scene, &[style], COVER, 0, |_| [0; 4]);
                scan(&pool);
            }
            if tick == 2 {
                pool.clear_scene(1);
                scan(&pool);
                let count = pool.active();
                pool.clear_scene(1);
                scan(&pool);
                assert_eq!(pool.active(), count);
            }
            if tick == 8 {
                pool.spawn_death(2, 17, [0; 3], bank);
                scan(&pool);
            }
            if tick == 10 {
                pool.clear_scene(0);
                scan(&pool);
            }
        }
        assert_eq!((pool.active(), pool.spawned, pool.dropped), (0, 294, 126));
    }
    #[test]
    fn collision_kill_and_life_loss_remove_exactly_one_cached_slot() {
        for life_loss in [0, ONE] {
            let mut pool = Pool::new();
            let style = Style {
                count: 1,
                life: [60; 2],
                size: [ONE; 2],
                speed: [0; 2],
                force: [[0; 2]; 3],
                collision: true,
                kill_speed: if life_loss == 0 { i32::MAX } else { 0 },
                life_loss,
                radius_scale: ONE,
                ..STYLE
            };
            spawn_specs(&mut pool, 0, 7, 0, 1, &[EMITTER], &[style]);
            assert_eq!(pool.active(), 1);
            let p = pool.particles[0].as_mut().unwrap();
            p.delay = 0;
            p.position = [0, ONE / 2 + 1, 0];
            p.velocity = [0, -60 * ONE, 0];
            tick_specs(&mut pool, 0, &[style], COVER, 1, |_| {
                [-10 * ONE, 0, 10 * ONE, 0]
            });
            assert_eq!(pool.active(), 0);
            assert_eq!(pool.particles.iter().flatten().count(), 0);
            tick_specs(&mut pool, 0, &[style], COVER, 1, |_| {
                [-10 * ONE, 0, 10 * ONE, 0]
            });
            assert_eq!(pool.active(), 0);
        }
    }
    #[test]
    fn positive_lifetime_eliminates_option_tag_without_reducing_capacity() {
        // The zero niche still removes the tag; the per-particle start alpha is
        // what takes the slot from 48 to 52, a whole word for one byte.
        assert_eq!(core::mem::size_of::<Particle>(), 52);
        assert_eq!(core::mem::size_of::<Option<Particle>>(), 52);
        assert_eq!(super::super::CAPACITY, 224);
        // 224 slots plus one u16 track ID each, the scan bound (padded to a
        // word), then the four cached counters: 11648+448+4+16.
        assert_eq!(core::mem::size_of::<Pool>(), 12116);
    }
    #[test]
    fn positive_lifetime_storage_preserves_full_u16_range_and_expiry() {
        for life in [1, 255, 256, 600, u16::MAX] {
            let s = Style {
                life: [life; 2],
                count: 1,
                ..STYLE
            };
            let mut p = Pool::new();
            spawn_specs(&mut p, 0, 7, 0, 1, &[EMITTER], &[s]);
            let particle = p.particles[0].as_mut().unwrap();
            assert_eq!(particle.life.get(), life);
            particle.delay = 0;
            particle.age = life - 1;
            tick_specs(&mut p, 0, &[s], COVER, 0, |_| [0; 4]);
            assert_eq!(
                p.active(),
                0,
                "lifetime {life} must expire on its original tick"
            );
            spawn_specs(&mut p, 0, 7, 0, 1, &[EMITTER], &[s]);
            assert_eq!(p.active(), 1, "the zero niche must remain reusable");
            assert_eq!(p.dropped, 0);
        }
    }
    #[test]
    fn source_break_angle_offset_changes_with_nail_direction_only() {
        let s = Style {
            count: 1,
            speed: [ONE; 2],
            arc: 0,
            ..STYLE
        };
        let e = Emitter {
            angle_offset: 45 * ONE,
            ..EMITTER
        };
        let launch = |kind, facing| {
            let mut p = Pool::new();
            spawn_specs(&mut p, 0, 7, kind, facing, &[e], &[s]);
            p.particles[0].unwrap().velocity
        };
        let right = launch(0, 1);
        let left = launch(0, -1);
        let up = launch(2, 1);
        assert_eq!(right[0], left[0]);
        assert_eq!(right[1], -left[1]);
        assert!(right[1] > 0);
        assert_eq!(up, [ONE, 0, 0]);
    }
    #[test]
    fn delayed_emission_obeys_source_rate_and_legacy_tick_cannot_double_advance() {
        let s = Style {
            count: 3,
            rate: 150 * ONE,
            ..STYLE
        };
        let mut p = Pool::new();
        spawn_specs(&mut p, 0, 7, 0, 1, &[EMITTER], &[s]);
        let delays: Vec<_> = p.particles.iter().flatten().map(|p| p.delay).collect();
        assert_eq!(delays, vec![0, 0, 1]);
        p.tick(0, None);
        assert!(p.particles.iter().flatten().all(|p| p.age == 0));
        tick_specs(&mut p, 0, &[s], COVER, 0, |_| [0; 4]);
        assert_eq!(
            p.particles
                .iter()
                .flatten()
                .map(|p| p.age)
                .collect::<Vec<_>>(),
            vec![1, 1, 0]
        );
        p.clear_scene(0);
        assert_eq!(p.active(), 0);
    }
    #[test]
    fn soft_styles_are_the_faint_ones_and_draw_through_a_free_palette_copy() {
        static OPAQUE: [Sample; 33] = [Sample {
            size: [ONE; 2],
            alpha: [255; 2],
            spin: [0; 2],
        }; 33];
        static FAINT: [Sample; 33] = [Sample {
            size: [ONE; 2],
            alpha: [0, 181],
            spin: [0; 2],
        }; 33];
        // The effects share palettes across textures: rock and dust art on one.
        static ROCK_FRAMES: [[u16; 3]; 1] = [[0, 1, 2]];
        static DUST_FRAMES: [[u16; 3]; 1] = [[0, 1, 2]];
        let mut r = Resident {
            scene: 0,
            styles: [EMPTY_STYLE; FX_MAX_STYLES],
            style_count: 3,
            emitters: [EMPTY_EMITTER; FX_MAX_EMITTERS],
            emitter_count: 0,
            art: [Art {
                u: 0,
                v: 0,
                w: 0,
                h: 0,
                clut: 0,
                tpage: 0,
            }; FX_MAX_ART],
            art_count: 3,
            frames: [[0; 3]; FX_MAX_FRAMES],
            soft: [false; FX_MAX_STYLES],
            additive: [false; FX_MAX_STYLES],
            soft_cluts: [(0, 0); SOFT_CLUT_SLOTS.len()],
            soft_clut_count: 0,
            hero_dust: NO_STYLE,
        };
        for (i, clut) in [
            (0, clut_word(368, 62)),
            (1, clut_word(368, 63)),
            (2, clut_word(368, 194)),
        ] {
            r.art[i].clut = clut;
        }
        r.styles[0] = Style {
            samples: &OPAQUE,
            frames: &ROCK_FRAMES,
            ..EMPTY_STYLE
        };
        r.styles[1] = Style {
            samples: &FAINT,
            frames: &DUST_FRAMES,
            ..EMPTY_STYLE
        };
        // Full curve, but a random start alpha below 255 (Break Dust's 83..255).
        r.styles[2] = Style {
            samples: &OPAQUE,
            start_alpha: [83, 255],
            frames: &DUST_FRAMES,
            ..EMPTY_STYLE
        };
        let uploads = [[368, 194, 16, 1], [352, 176, 5, 20]];
        soft_styles(&mut r, 3, &uploads);
        assert_eq!(&r.soft[..3], &[false, true, true]);
        // One copy for the densest art's palette, in the first free slot.
        assert_eq!(
            &r.soft_cluts[..r.soft_clut_count],
            &[(clut_word(368, 194), clut_word(368, 239))]
        );
        assert_eq!(
            soft_clut(&r, clut_word(368, 194)),
            Some(clut_word(368, 239))
        );
        assert_eq!(soft_clut(&r, clut_word(368, 62)), None);
        // A scene whose uploads cover every slot keeps everything on coverage.
        let full = [[368, 236, 16, 4]];
        soft_styles(&mut r, 3, &full);
        assert_eq!(r.soft_clut_count, 0);
        assert!(r.soft[..3].iter().all(|s| !s));
    }
}
