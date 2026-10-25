//! The Knight's ability clips, held in linked RAM and reaching VRAM through
//! the shared animation slots, exactly as the Hollow Shade's art does.
//!
//! An ability is usable in any view, and the tightest admitted region has about
//! 19 free texture slots against the 416-slot CLUT budget, so these frames
//! cannot ride in a per-region atlas. Their keys sit above the Shade's.
//!
//! The last frames are not linked: the death clip and the respawn clips are
//! packs on the disc (`STREAMS`), carried into the room pool only while a moment
//! needs them (`ready`, frame.rs `carry_streamed_art`). Their keys, table rows and
//! palettes are where they always were; only the texels and the frame records
//! travel. Frames of a pack that is not resident must not be requested.
pub use crate::shade::{Clip, Frame};
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/ability-art.rs"
));
static DATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/ability-art.hk"
));
/// The carried module of each streamed pack, in `STREAM_PACKS` order
/// (host/ability_art.py): the Death clip, the respawn clips, then the ability effects.
#[cfg(not(test))]
const STREAMS: [usize; 3] = [
    crate::modules::ART_DEATH,
    crate::modules::ART_WAKE,
    crate::modules::ART_FX,
];
/// Bytes of one streamed frame record at the head of a pack: the guest `Frame` field for field
/// (offset u32, width and height u16, palette u32, four bounds i32).
const RECORD_BYTES: usize = 28;

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
/// The Soul Burst: the star at the Knight when the soul orb can heal (`Can Heal 2`).
pub const SOUL_BURST: usize = 19;
/// The heal's `White Flash R`: one frame of greys, faded by the draw's tint.
pub const HEAL_FLASH: usize = 20;

/// The Knight standing and looking up or down, then the clips that end a released look
/// (`host/ability_art.py` LOOK_CLIPS, after the Focus clips).
pub const LOOK_UP: usize = 21;
pub const LOOK_DOWN: usize = 22;
pub const LOOK_UP_END: usize = 23;
pub const LOOK_DOWN_END: usize = 24;
/// The crouch of a hard landing (`HardLand`, `host/ability_art.py` HARD_LAND_CLIP, after the look clips).
pub const HARD_LAND: usize = 25;
/// King's Pass's opening crash: the Knight face down after the first fall, and getting up from it
/// (`host/ability_art.py` CRASH_CLIPS).
pub const SUPER_HARD_LAND: usize = 26;
pub const GET_UP: usize = 27;
/// The two Slash Effect GhostDark clips the Knight Damage and Hero Death FSMs fling
/// (`host/ability_art.py` EFFECT_LIBRARY_CLIPS), then the Knight's death (the Hero Death object's
/// `Death` clip) and the two bench-respawn clips after it (`DEATH_CLIPS`). The slashes are linked; from
/// `DEATH` on the clips are the streamed packs (`host/ability_art.py` STREAM_PACKS).
pub const SLASH_GHOST_1: usize = 28;
pub const SLASH_GHOST_2: usize = 29;
pub const DEATH: usize = 30;
pub const SIT_FALL_ASLEEP: usize = 31;
pub const WAKE_TO_SIT: usize = 32;
/// The respawn at the new-game spawn: the lying pose, then getting up (`Wake Up Ground`).
pub const WAKE_GROUND_LIE: usize = 33;
pub const WAKE_GROUND_RISE: usize = 34;
/// The ability effects, the last streamed pack (`host/ability_art.py` FX_CLIPS, `art_fx`): the Dash
/// Burst, the double jump's wings and the wall jump's puff, each drawn at the Knight
/// (`ability_fx.rs`).
pub const DASH_EFFECT: usize = 35;
pub const WINGS_EFFECT: usize = 36;
pub const WALL_PUFF: usize = 37;
/// Vengeful Spirit's cast (`Fireball Top`'s blast) and the ball's wall impact.
pub const FIREBALL_BLAST: usize = 38;
pub const WALL_IMPACT: usize = 39;
/// The double jump's `White Flash DJ`.
pub const FLASH_EFFECT: usize = 40;

/// Above every Shade key, which are themselves above every room texture table.
pub const KEY_BASE: u16 = crate::shade::KEY_BASE + crate::shade::SHADE_FRAMES.len() as u16;

/// The frame of `clip` at a 60 Hz age, honouring loop / loop-section / once.
pub fn frame_index(clip: usize, age: u32) -> usize {
    let c = ABILITY_CLIPS[clip];
    let frame = hk_sim::fixed::mul_div_floor(u32::from(age), u32::from(c.fps), 60) as usize;
    c.start
        + match c.wrap {
            0 => frame % c.count,
            // tk2d LoopSection: play through once, then repeat from `loopStart`.
            1 => {
                let start = ABILITY_LOOP[clip] as usize;
                if frame < c.count || start >= c.count {
                    frame.min(c.count - 1)
                } else {
                    start + (frame - start) % (c.count - start)
                }
            }
            _ => frame.min(c.count - 1),
        }
}
/// Ticks one play of a clip takes (a once clip's whole run), at the 60 Hz tick.
pub fn clip_ticks(clip: usize) -> u32 {
    let c = ABILITY_CLIPS[clip];
    (c.count as u32 * 60).div_ceil(u32::from(c.fps))
}
/// The bytes of streamed pack `k`, when it is resident (the pool holds it), without counting a miss.
fn pack(k: usize) -> Option<&'static [u8]> {
    #[cfg(not(test))]
    {
        crate::modules::bytes(STREAMS[k])
    }
    #[cfg(test)]
    {
        [
            &include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/art-death.hk"))[..],
            &include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/art-wake.hk"))[..],
            &include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/art-fx.hk"))[..],
        ]
        .get(k)
        .copied()
    }
}
/// The streamed pack holding frame `index`: its number, and the frame's place in it.
fn stream_of(index: usize) -> Option<(usize, usize)> {
    STREAM_PACKS
        .iter()
        .position(|&(first, count)| (first..first + count).contains(&index))
        .map(|k| (k, index - STREAM_PACKS[k].0))
}
/// The little-endian word of a pack's frame table at byte `at`. One copy: seven inline reads of four
/// bounds-checked bytes each were most of `locate`.
#[inline(never)]
fn record_word(blob: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([blob[at], blob[at + 1], blob[at + 2], blob[at + 3]])
}
/// Frame `index`'s record and the coded texels its `offset` counts from. None for a frame past the table
/// and for one whose pack is not resident.
fn locate(index: usize) -> Option<(Frame, &'static [u8])> {
    if let Some(&frame) = ABILITY_FRAMES.get(index) {
        return Some((frame, &DATA[PALETTE_BYTES..]));
    }
    let (k, at) = stream_of(index)?;
    let blob = pack(k)?;
    let word = |n: usize| record_word(blob, at * RECORD_BYTES + n * 4);
    let dims = word(1);
    let bounds = |n: usize| word(n) as i32;
    let frame = Frame {
        offset: word(0) as usize,
        width: dims as u16,
        height: (dims >> 16) as u16,
        clut: word(2) as usize,
        bounds: [bounds(3), bounds(4), bounds(5), bounds(6)],
    };
    Some((frame, &blob[STREAM_PACKS[k].1 * RECORD_BYTES..]))
}
/// The frame's record, if its art is here.
fn frame_record(index: usize) -> Option<Frame> {
    locate(index).map(|(frame, _)| frame)
}
/// Whether `clip` can be drawn now: always for a linked clip, for a streamed one once its pack is resident.
#[inline(never)]
pub fn ready(clip: usize) -> bool {
    let first = ABILITY_CLIPS[clip].start;
    first < ABILITY_FRAMES.len() || stream_of(first).is_some_and(|(k, _)| pack(k).is_some())
}
/// Width and height of one ability frame's texels.
pub fn size(index: usize) -> Option<(u16, u16)> {
    let frame = frame_record(index)?;
    Some((frame.width, frame.height))
}
/// Decoded texel bytes of one ability frame: 4 bpp, rows padded to whole halfwords.
pub fn texel_bytes(index: usize) -> Option<usize> {
    let frame = frame_record(index)?;
    Some((frame.width as usize + 3) / 4 * 2 * frame.height as usize)
}
/// The frame's run-length coded texels (host/ability_art.py `packbits`), running to the
/// end of the blob at worst: the decoder stops at `texel_bytes`.
fn coded(index: usize) -> Option<&'static [u8]> {
    let (frame, base) = locate(index)?;
    Some(&base[frame.offset..])
}
/// Bytes of a frame in order, decoded from its run-length code. A token below 0x80 is a
/// literal of token + 1 bytes; from 0x80 it is a run of (token & 0x7f) + 2 copies of the
/// next byte.
struct Unpack {
    src: &'static [u8],
    at: usize,
    literal: usize,
    run: usize,
    value: u8,
}
impl Unpack {
    fn new(src: &'static [u8]) -> Self {
        Unpack {
            src,
            at: 0,
            literal: 0,
            run: 0,
            value: 0,
        }
    }
    fn next(&mut self) -> u8 {
        if self.run == 0 && self.literal == 0 {
            let token = self.src[self.at];
            self.at += 1;
            if token < 0x80 {
                self.literal = token as usize + 1
            } else {
                self.run = (token & 0x7f) as usize + 2;
                self.value = self.src[self.at];
                self.at += 1
            }
        }
        if self.run != 0 {
            self.run -= 1;
            self.value
        } else {
            self.literal -= 1;
            let b = self.src[self.at];
            self.at += 1;
            b
        }
    }
}

#[cfg(not(test))]
mod presentation {
    use super::*;
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        prim::QuadTextured,
    };
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
                if left == 0 {
                    break;
                }
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
            upload_bytes(
                VramRect::new(CLUT_RECT.0, CLUT_Y[i], CLUT_RECT.2, CLUT_RECT.3),
                &DATA[i * 32..i * 32 + 32],
            );
        }
    }
    /// Draw one ability frame at the Knight, mirrored by its facing.
    #[inline]
    pub fn draw(index: usize, x: i32, y: i32, facing: i32, camera: (i32, i32), tint: u8) -> u32 {
        draw_with(index, x, y, facing, camera, (tint, tint, tint), BlendMode::Average)
    }
    /// A Focus effect frame: the original's Screen blend is the GPU's Add.
    #[inline]
    pub fn draw_additive(
        index: usize,
        x: i32,
        y: i32,
        facing: i32,
        camera: (i32, i32),
        tint: u8,
    ) -> u32 {
        draw_with(index, x, y, facing, camera, (tint, tint, tint), BlendMode::Add)
    }
    /// An ability effect (`ability_fx.rs`): additive light of the colour `rgb`, 128 being the
    /// texel's own level.
    #[inline]
    pub fn draw_light(
        index: usize,
        x: i32,
        y: i32,
        facing: i32,
        camera: (i32, i32),
        rgb: (u8, u8, u8),
    ) -> u32 {
        draw_with(index, x, y, facing, camera, rgb, BlendMode::Add)
    }
    /// One slash frame at a world point, turned `sin`/`cos` (Q12) about its own origin and grown by
    /// `scale` sixteenths. The frame's bounds are in the sprite's own orientation, so this is not
    /// mirrored by a facing.
    #[inline(never)]
    pub fn draw_slash(
        index: usize,
        at: [i32; 2],
        turn: (i32, i32),
        scale: i32,
        camera: (i32, i32),
        tint: u8,
    ) -> u32 {
        let Some(frame) = frame_record(index) else {
            return 0;
        };
        let b = frame.bounds;
        let (sin, cos) = turn;
        let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
        let vertices = world.map(|[wx, wy]| {
            // Each term is cut to 20 bits before the Q12 product, so none of them can overflow.
            let (wx, wy) = ((wx * scale) >> 4, (wy * scale) >> 4);
            let rx = (((wx >> 4) * cos) - ((wy >> 4) * sin)) >> 8;
            let ry = (((wx >> 4) * sin) + ((wy >> 4) * cos)) >> 8;
            (
                160 + hk_sim::fixed::project(at[0] + rx, camera.0, crate::KNIGHT_SCALE as i32, 28),
                120 - hk_sim::fixed::project(at[1] + ry, camera.1, crate::KNIGHT_SCALE as i32, 28),
            )
        });
        submit(index, frame, vertices, BlendMode::Add, (tint, tint, tint))
    }
    #[inline(never)]
    fn draw_with(
        index: usize,
        x: i32,
        y: i32,
        facing: i32,
        camera: (i32, i32),
        tint: (u8, u8, u8),
        blend: BlendMode,
    ) -> u32 {
        let Some(frame) = frame_record(index) else {
            return 0;
        };
        let b = frame.bounds;
        let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
        let vertices = world.map(|[wx, wy]| {
            // The atlas faces left, as the room body frames do.
            let px = x - wx * facing;
            let py = y + wy;
            (
                160 + hk_sim::fixed::project(px, camera.0, crate::KNIGHT_SCALE as i32, 28),
                120 - hk_sim::fixed::project(py, camera.1, crate::KNIGHT_SCALE as i32, 28),
            )
        });
        submit(index, frame, vertices, blend, tint)
    }
    /// The quad of frame `index` at the four projected corners (top-left, top-right, bottom-left,
    /// bottom-right of the sprite), unless every corner is off one side of the screen. One copy
    /// for the three draws above.
    #[inline(never)]
    fn submit(
        index: usize,
        frame: Frame,
        vertices: [(i32, i32); 4],
        blend: BlendMode,
        tint: (u8, u8, u8),
    ) -> u32 {
        if vertices.iter().all(|p| p.0 < 0)
            || vertices.iter().all(|p| p.0 >= 320)
            || vertices.iter().all(|p| p.1 < 0)
            || vertices.iter().all(|p| p.1 >= 240)
        {
            return 0;
        }
        let (u, v) = crate::render::animation_uv(KEY_BASE + index as u16);
        let right = (u16::from(u) + frame.width - 1) as u8;
        let bottom = (u16::from(v) + frame.height - 1) as u8;
        let clut = Clut::new(CLUT_RECT.0, CLUT_Y[frame.clut as usize]).uv_clut_word();
        let tpage = crate::render::animation_tpage_word(KEY_BASE + index as u16);
        let template = QuadTextured::with_material(
            [(0, 0); 4],
            [(u, v), (right, v), (u, bottom), (right, bottom)],
            TextureMaterial::blended(clut, tpage, tint, blend),
        );
        // `resident_quad` draws at the texel's own level whatever the template's tint: only the
        // additive (Focus, slash) draws pass theirs on, since the flash fades through it.
        let level = if matches!(blend, BlendMode::Add) {
            tint
        } else {
            (128, 128, 128)
        };
        crate::render::resident_quad_tinted(
            &template,
            vertices.map(|(x, y)| (x as i16, y as i16)),
            level,
        );
        unsafe {
            HK_ABILITY_DRAWN = HK_ABILITY_DRAWN.saturating_add(1);
        }
        1
    }
}
#[cfg(not(test))]
pub use presentation::{draw, draw_additive, draw_light, draw_slash, upload, upload_frame};
