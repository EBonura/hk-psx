//! A compact art bank for ordinary actors (docs/BOSS-ART.md, generalised past one boss): the sprites of an actor family as
//! parts of at most 64x64 texels held packbits-coded in one contiguous byte string, decoded into the animation cache's slots
//! when a frame first needs them. A room carries the bytes outside its scene stream (a pool data package, by whatever loader
//! the build wires in: `set`), and only the bank's palettes ride in the room as tiny carrier textures.
//!
//! A frame of a bank-drawn actor is an ordinary frame record whose texture is the first carrier texture (`marker`) and whose
//! first two box words carry the sprite's index in their low bytes (`frame_sprite`); every table the region has about clips and
//! frames stays what it always was, so clip lengths, completion and every validator keep working. A scene without a bank has
//! no marker (`NO_MARKER`) and no frame can match it.
//!
//! The bank's animation-cache keys are the boss bank's (`boss_bank::KEY_BASE`): a scene carries one or the other.
use hk_format::boss::{self as fmt, Bank};
use hk_format::Room;

/// The bank's animation-cache keys: part `i` is key `KEY_BASE + i`.
pub const KEY_BASE: u16 = crate::dialogue_art::KEY_BASE + crate::dialogue_art::DIALOGUE_TILE_COUNT as u16;
#[cfg(feature = "hornet")]
const _: () = assert!(KEY_BASE == crate::boss_bank::KEY_BASE);
/// Parts a bank may have: the keys that are left.
pub const MAX_PARTS: usize = 640;
const _: () = assert!(KEY_BASE as usize + MAX_PARTS <= hk_cache::MAX_KEYS);
/// What `marker` holds when the room has no bank: above every texture id.
pub const NO_MARKER: u16 = u16::MAX;
/// A carrier texture's stream starts with this, the bank id and the palette it carries.
pub const CARRIER_MAGIC: u32 = u32::from_le_bytes(*b"HKBP");
/// Palettes a bank has (the boss bank's limit).
pub const MAX_PALETTES: usize = 16;

#[no_mangle]
pub static mut HK_ACTOR_BANK_UPLOADS: u32 = 0;
#[no_mangle]
pub static mut HK_ACTOR_BANK_PARTS_DRAWN: u32 = 0;
/// Sprites asked for that the bank does not have (must stay 0).
#[no_mangle]
pub static mut HK_ACTOR_BANK_MISSES: u32 = 0;

/// The bank's bytes while its room is resident; empty otherwise.
static mut BLOB: &[u8] = &[];
/// The first carrier texture of the bound room, or `NO_MARKER`.
static mut MARKER: u16 = NO_MARKER;
/// The CLUT word of each palette, from the carrier textures of the bound room.
static mut CLUTS: [u16; MAX_PALETTES] = [0; MAX_PALETTES];

/// Hand the loader's bytes to the reader (the room's data package), or take them away with an empty slice.
pub fn set(bytes: &'static [u8]) {
    unsafe {
        BLOB = bytes;
        MARKER = NO_MARKER;
    }
}

/// Whether a bank is loaded.
#[inline(always)]
pub fn active() -> bool {
    unsafe { !BLOB.is_empty() }
}

/// The texture id that marks a bank frame in the bound room.
#[inline(always)]
pub fn marker() -> u16 {
    unsafe { MARKER }
}

/// Find the bank's carrier textures among `room`'s, once per room bind (`render::init`).
pub fn bind(room: &Room) {
    unsafe {
        MARKER = NO_MARKER;
        if BLOB.is_empty() {
            return;
        }
        let Some(bank) = Bank::open(BLOB) else { return };
        for t in 0..room.counts[1] {
            let tex = room.texture(t);
            if !tex.is_streamed() {
                continue;
            }
            let Some(bytes) = room.stream_pixels(tex) else { continue };
            if bytes.len() < 8 || u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != CARRIER_MAGIC {
                continue;
            }
            let id = u16::from_le_bytes([bytes[4], bytes[5]]);
            let k = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
            if id != bank.id || k >= MAX_PALETTES {
                continue;
            }
            CLUTS[k] = crate::render::clut_word(tex.palette);
            if MARKER == NO_MARKER {
                MARKER = t as u16;
            }
        }
    }
}

/// The sprite a bank frame stands for: its index rides in the low bytes of the frame box's first two words.
#[inline(always)]
pub fn frame_sprite(b: [i32; 4]) -> (usize, [i32; 4]) {
    let sprite = (b[0] & 0xff) as usize | ((b[1] & 0xff) as usize) << 8;
    (sprite, [b[0] & !0xff, b[1] & !0xff, b[2], b[3]])
}

/// One part of a sprite, ready to draw.
#[derive(Clone, Copy)]
pub struct Piece {
    pub key: u16,
    pub clut: u16,
    pub width: u8,
    pub height: u8,
    /// The part's world box in a frame record's `[x0, y0, x1, y1]` order.
    pub bounds: [i32; 4],
}

/// The parts of sprite `sprite`, each to `f`. False if the bank has no such sprite.
pub fn pieces(sprite: usize, mut f: impl FnMut(Piece)) -> bool {
    let Some(bank) = (unsafe { Bank::open(BLOB) }) else { return false };
    if sprite >= bank.sprites as usize {
        unsafe { HK_ACTOR_BANK_MISSES = HK_ACTOR_BANK_MISSES.wrapping_add(1) };
        return false;
    }
    let s = bank.sprite(sprite);
    for p in s.first_part as usize..(s.first_part + s.parts) as usize {
        let part = bank.part(p);
        f(Piece {
            key: KEY_BASE + p as u16,
            clut: unsafe { CLUTS[(s.palette as usize).min(MAX_PALETTES - 1)] },
            width: part.width,
            height: part.height,
            bounds: Bank::<&[u8]>::part_bounds(&s, &part),
        });
    }
    true
}

#[repr(align(4))]
struct Buf([u8; 2048]);

/// The decoded part `index`, uploaded into cache slot `slot` (its rectangle sized to the part). The bytes uploaded.
/// Out of line on purpose: inlined into the cache's upload closure it would put the decode in the resident hot path.
#[inline(never)]
pub fn upload(index: usize, slot: usize) -> Option<u32> {
    let bank = Bank::open(unsafe { BLOB })?;
    if index >= bank.parts as usize {
        return None;
    }
    let part = bank.part(index);
    let len = part.texel_bytes();
    if len == 0 || len > 2048 {
        return None;
    }
    let mut buf = Buf([0; 2048]);
    bank.decode_into(&part, &mut buf.0)?;
    crate::texture_upload::upload(crate::animation_cache::dest(slot, part.width as u16, part.height as u16), &buf.0[..len]);
    unsafe { HK_ACTOR_BANK_UPLOADS = HK_ACTOR_BANK_UPLOADS.wrapping_add(1) };
    crate::input::poll_only();
    Some(len as u32)
}

/// Draw one part through the cache slot its key holds: the quad the boss bank draws, with the hit flash on top.
pub fn draw_piece(key: u16, clut: u16, width: u8, height: u8, verts: [(i16, i16); 4], flash: u8) {
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        prim::QuadTextured,
    };
    let (u, v) = crate::render::animation_uv(key);
    let right = (u as u16 + width as u16 - 1) as u8;
    let bottom = (v as u16 + height as u16 - 1) as u8;
    let template = QuadTextured::with_material(
        [(0, 0); 4],
        [(u, v), (right, v), (u, bottom), (right, bottom)],
        TextureMaterial::blended(clut, crate::render::animation_tpage_word(key), (128, 128, 128), BlendMode::Add),
    );
    crate::render::resident_quad_tinted(&template, verts, (128, 128, 128));
    if flash != 0 {
        crate::render::resident_quad_flash(&template, verts, crate::enemies::flash_tint(flash));
    }
    unsafe { HK_ACTOR_BANK_PARTS_DRAWN = HK_ACTOR_BANK_PARTS_DRAWN.wrapping_add(1) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sprite_index_survives_the_frame_box_it_rides_in() {
        let b = [0x0012_3400, -0x0045_6700, 0x0100_0000, 0x0200_0000];
        for sprite in [0usize, 1, 98, 255, 256, 4095, 65535] {
            let coded = [(b[0] & !0xff) | (sprite & 0xff) as i32, (b[1] & !0xff) | (sprite >> 8) as i32, b[2], b[3]];
            let (got, clean) = frame_sprite(coded);
            assert_eq!(got, sprite);
            assert_eq!(clean, [b[0] & !0xff, b[1] & !0xff, b[2], b[3]]);
        }
    }

    #[test]
    fn no_marker_matches_no_texture_id() {
        assert_eq!(marker(), NO_MARKER);
        assert!(NO_MARKER as usize >= 2048);
        assert!(!active());
    }
}
