//! The Knight's ability clips, held in linked RAM and reaching VRAM through
//! the shared animation slots, exactly as the Hollow Shade's art does.
//!
//! An ability is usable in any view, and the tightest admitted region has about
//! 19 free texture slots against the 416-slot CLUT budget, so these frames
//! cannot ride in a per-region atlas. Their keys sit above the Shade's.
pub use crate::shade::{Clip, Frame};
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/ability-art.rs"));
static DATA: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/ability-art.hk"));

/// Clip indices, in the order `host/ability_art.py::ORDER` cooks them.
pub const DASH: usize = 0;
pub const WALL_SLIDE: usize = 1;
pub const WALLJUMP: usize = 2;
pub const DOUBLE_JUMP: usize = 3;
pub const DN_START: usize = 4;
pub const FIREBALL_ANTIC: usize = 8;
pub const FIREBALL_CAST: usize = 9;
pub const SD_CHARGE_GROUND: usize = 10;
pub const SD_WALL_CHARGE: usize = 11;
pub const SD_DASH: usize = 12;
pub const SD_HIT_WALL: usize = 13;
/// Vengeful Spirit's projectile, from its own sprite collection.
pub const BALL: usize = 14;
pub const BALL_END: usize = 15;
/// The Focus effects (additive): Lines Anim's Focus Effect and Focus Effect End, then
/// Heal Anim's Burst Effect, appended by `host/ability_art.py` after the ball clips.
pub const FOCUS_EFFECT: usize = 16;
pub const FOCUS_EFFECT_END: usize = 17;
pub const BURST_EFFECT: usize = 18;

/// Above every Shade key, which are themselves above every room texture table.
pub const KEY_BASE: u16 = crate::shade::KEY_BASE + crate::shade::SHADE_FRAMES.len() as u16;

/// The frame of `clip` at a 60 Hz age, honouring loop / loop-section / once.
pub fn frame_index(clip: usize, age: u32) -> usize {
    let c = ABILITY_CLIPS[clip];
    let frame = (u64::from(age) * u64::from(c.fps) / 60) as usize;
    c.start + if c.wrap == 0 { frame % c.count } else { frame.min(c.count - 1) }
}
/// Width and height of one ability frame's texels.
pub fn size(index: usize) -> Option<(u16, u16)> {
    let frame = ABILITY_FRAMES.get(index)?;
    Some((frame.width, frame.height))
}
/// Decoded texel bytes of one ability frame: 4 bpp, rows padded to whole halfwords.
pub fn texel_bytes(index: usize) -> Option<usize> {
    let frame = ABILITY_FRAMES.get(index)?;
    Some((frame.width as usize + 3) / 4 * 2 * frame.height as usize)
}
/// The frame's run-length coded texels (host/ability_art.py `packbits`), running to the
/// end of the blob at worst: the decoder stops at `texel_bytes`.
fn coded(index: usize) -> Option<&'static [u8]> {
    Some(&DATA[PALETTE_BYTES + ABILITY_FRAMES.get(index)?.offset..])
}
/// Bytes of a frame in order, decoded from its run-length code. A token below 0x80 is a
/// literal of token + 1 bytes; from 0x80 it is a run of (token & 0x7f) + 2 copies of the
/// next byte.
struct Unpack { src: &'static [u8], at: usize, literal: usize, run: usize, value: u8 }
impl Unpack {
    fn new(src: &'static [u8]) -> Self { Unpack { src, at: 0, literal: 0, run: 0, value: 0 } }
    fn next(&mut self) -> u8 {
        if self.run == 0 && self.literal == 0 {
            let token = self.src[self.at];
            self.at += 1;
            if token < 0x80 { self.literal = token as usize + 1 } else { self.run = (token & 0x7f) as usize + 2; self.value = self.src[self.at]; self.at += 1 }
        }
        if self.run != 0 { self.run -= 1; self.value } else { self.literal -= 1; let b = self.src[self.at]; self.at += 1; b }
    }
}

#[cfg(not(test))]
mod presentation {
    use super::*;
    use psx_gpu::{material::{BlendMode, TextureMaterial}, prim::QuadTextured};
    use psx_vram::{upload_bytes, Clut, VramRect};
    #[no_mangle]
    pub static mut HK_ABILITY_DRAWN: u32 = 0;
    /// One resident CLUT row per cooked palette, inside the block shade.py
    /// reserved at y482 and does not use.
    /// Decode one frame's texels into `rect` of VRAM, straight into the GPU's data port,
    /// so no staging buffer is needed. The caller has drained the GPU's DMA channel.
    /// Returns the decoded byte count.
    pub fn upload_frame(index: usize, rect: VramRect) -> Option<u32> {
        let len = texel_bytes(index)?;
        let mut stream = Unpack::new(coded(index)?);
        psx_io::gpu::wait_cmd_ready();
        psx_io::gpu::write_gp0(psx_hw::gpu::gp0::COPY_CPU_TO_VRAM);
        psx_io::gpu::write_gp0(psx_hw::gpu::pack_xy(rect.x, rect.y));
        psx_io::gpu::write_gp0(psx_hw::gpu::pack_xy(rect.w, rect.h));
        let mut left = len;
        while left > 0 {
            let mut word = 0u32;
            for shift in 0..4 {
                if left == 0 { break; }
                word |= u32::from(stream.next()) << (shift * 8);
                left -= 1;
            }
            psx_io::gpu::write_gp0(word);
        }
        Some(len as u32)
    }
    pub fn upload() {
        assert!(DATA.len() >= PALETTE_BYTES && PALETTE_BYTES == PALETTE_COUNT * 32);
        for i in 0..PALETTE_COUNT {
            upload_bytes(VramRect::new(CLUT_RECT.0, CLUT_RECT.1 + i as u16, CLUT_RECT.2, CLUT_RECT.3),
                &DATA[i * 32..i * 32 + 32]);
        }
    }
    /// Draw one ability frame at the Knight, mirrored by its facing.
    #[inline(never)]
    pub fn draw(index: usize, x: i32, y: i32, facing: i32, camera: (i32, i32), tint: u8) -> u32 {
        draw_with(index, x, y, facing, camera, tint, BlendMode::Average)
    }
    /// A Focus effect frame: the original's Screen blend is the GPU's Add.
    #[inline(never)]
    pub fn draw_additive(index: usize, x: i32, y: i32, facing: i32, camera: (i32, i32), tint: u8) -> u32 {
        draw_with(index, x, y, facing, camera, tint, BlendMode::Add)
    }
    #[inline(always)]
    fn draw_with(index: usize, x: i32, y: i32, facing: i32, camera: (i32, i32), tint: u8, blend: BlendMode) -> u32 {
        let Some(frame) = ABILITY_FRAMES.get(index) else { return 0 };
        let (u, v) = crate::render::animation_uv(KEY_BASE + index as u16);
        let b = frame.bounds;
        let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
        let vertices = world.map(|[wx, wy]| {
            // The atlas faces left, as the room body frames do.
            let px = x - wx * facing;
            let py = y + wy;
            (160 + (((i64::from(px) - i64::from(camera.0)) * i64::from(crate::KNIGHT_SCALE)) >> 28) as i32,
             120 - (((i64::from(py) - i64::from(camera.1)) * i64::from(crate::KNIGHT_SCALE)) >> 28) as i32)
        });
        if vertices.iter().all(|p| p.0 < 0) || vertices.iter().all(|p| p.0 >= 320)
            || vertices.iter().all(|p| p.1 < 0) || vertices.iter().all(|p| p.1 >= 240) {
            return 0;
        }
        let right = (u16::from(u) + frame.width - 1) as u8;
        let bottom = (u16::from(v) + frame.height - 1) as u8;
        let clut = Clut::new(CLUT_RECT.0, CLUT_RECT.1 + frame.clut as u16).uv_clut_word();
        let tpage = crate::render::animation_tpage_word(KEY_BASE + index as u16);
        let template = QuadTextured::with_material([(0, 0); 4],
            [(u, v), (right, v), (u, bottom), (right, bottom)],
            TextureMaterial::blended(clut, tpage, (tint, tint, tint), blend));
        crate::render::resident_quad(&template, vertices.map(|(x, y)| (x as i16, y as i16)));
        unsafe { HK_ABILITY_DRAWN = HK_ABILITY_DRAWN.saturating_add(1); }
        1
    }
}
#[cfg(not(test))]
pub use presentation::{draw, draw_additive, upload, upload_frame};
