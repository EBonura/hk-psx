//! A compact art bank for ordinary actors (docs/BOSS-ART.md, generalised past one boss): the sprites of an actor family as
//! parts of at most 64x64 texels held packbits-coded in contiguous byte strings, decoded into the animation cache's slots
//! when a frame first needs them. A scene's bank is a list of up to `MAX_SLICES` slices (each a `hk_format::boss::Bank` blob
//! with its own palettes): sprites and parts are numbered globally, the first slice's first, so a frame names a sprite
//! without knowing which slice holds it. A scene opts in by data (`data/actor_banks.rs`, written by host/actor_bank.py from
//! host/actor_banks.json): its first slice is a data package in the module pool (`modules::bytes`) and the rest are linked
//! read-only data. Only the slices' palettes ride in the views that draw the actors, as tiny carrier textures.
//!
//! A frame of a bank-drawn actor is an ordinary frame record whose texture is the first carrier texture (`marker`) and whose
//! first two box words carry the sprite's index in their low bytes (`frame_sprite`); every table the region has about clips and
//! frames stays what it always was, so clip lengths, completion and every validator keep working. A scene without a bank has
//! no marker (`NO_MARKER`) and no frame can match it.
//!
//! The bank's animation-cache keys are the boss bank's (`boss_bank::KEY_BASE`): a scene carries one or the other.
use hk_format::boss::{self as fmt, Bank};
use hk_format::Room;

/// A scene that draws actors from a bank: its manifest index, the module id of its core slice's data package (a
/// cook-generated `modules::ART_*` constant) and its late slice, which is linked read-only data.
pub struct SceneBank {
    pub scene: usize,
    pub package: usize,
    pub late: &'static [u8],
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/actor_banks.rs"));

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
/// Palettes a slice has (the boss bank's limit).
pub const MAX_PALETTES: usize = 16;
/// Slices a scene's bank has: the core, then the late art (Death and Fall clips) that does not fit the pool package.
pub const MAX_SLICES: usize = 2;

#[no_mangle]
pub static mut HK_ACTOR_BANK_UPLOADS: u32 = 0;
#[no_mangle]
pub static mut HK_ACTOR_BANK_PARTS_DRAWN: u32 = 0;
/// Sprites asked for that the bank does not have (must stay 0).
#[no_mangle]
pub static mut HK_ACTOR_BANK_MISSES: u32 = 0;
/// Views bound in a scene that has a bank whose package was not resident (must stay 0).
#[no_mangle]
pub static mut HK_ACTOR_BANK_UNBOUND: u32 = 0;

/// The slices of the bound scene's bank; empty ones are unused.
static mut SLICES: [&[u8]; MAX_SLICES] = [&[]; MAX_SLICES];
/// The first carrier texture of the bound room, or `NO_MARKER`.
static mut MARKER: u16 = NO_MARKER;
/// The CLUT word of each slice's palettes, from the carrier textures of the bound room.
static mut CLUTS: [[u16; MAX_PALETTES]; MAX_SLICES] = [[0; MAX_PALETTES]; MAX_SLICES];

/// Hand the loader's bytes to the reader: the slices in numbering order, or none to take the bank away. A list that
/// would pass `MAX_PARTS` parts or `MAX_SLICES` slices is refused whole.
pub fn set(slices: &[&'static [u8]]) {
    unsafe {
        SLICES = [&[]; MAX_SLICES];
        MARKER = NO_MARKER;
        if slices.len() > MAX_SLICES {
            return;
        }
        let mut parts = 0;
        for s in slices {
            parts += Bank::open(*s).map_or(0, |b| b.parts as usize);
        }
        if parts > MAX_PARTS {
            return;
        }
        for (i, s) in slices.iter().enumerate() {
            SLICES[i] = s;
        }
    }
}

/// Whether a bank is loaded.
#[inline(always)]
pub fn active() -> bool {
    unsafe { !SLICES[0].is_empty() }
}

/// The texture id that marks a bank frame in the bound room.
#[inline(always)]
pub fn marker() -> u16 {
    unsafe { MARKER }
}

/// The slices' `(sprites, parts)` counts, in numbering order (an empty slice counts as none).
fn counts() -> impl Iterator<Item = (u32, u32)> {
    (0..MAX_SLICES).map_while(|i| Bank::open(unsafe { SLICES[i] }).map(|b| (b.sprites, b.parts)))
}

/// Where global sprite `sprite` lives: its slice's bank, the slice, its number in the slice and the global number of
/// the slice's first part.
fn locate_sprite(sprite: usize) -> Option<(Bank<&'static [u8]>, usize, usize, usize)> {
    let (slice, local, parts) = fmt::locate_sprite(counts(), sprite)?;
    Some((Bank::open(unsafe { SLICES[slice] })?, slice, local, parts))
}

/// Bind the bank of manifest scene `scene` (or none) to the view `room`: hand its slices to the reader and find the
/// carrier textures among the room's, once per view bind (`render::init`). A scene without an entry in
/// `data/actor_banks.rs` costs this one search; a scene with one whose package is not resident leaves the frames that name
/// the bank drawing nothing (`HK_ACTOR_BANK_UNBOUND`, and `HK_ACTOR_BANK_MISSES` per frame).
pub fn bind(room: &Room, scene: usize) {
    set(&[]);
    let Some(entry) = SCENE_BANKS.iter().find(|e| e.scene == scene) else { return };
    match crate::modules::bytes(entry.package) {
        Some(core) => set(&[core, entry.late]),
        None => unsafe { HK_ACTOR_BANK_UNBOUND = HK_ACTOR_BANK_UNBOUND.wrapping_add(1) },
    }
    unsafe {
        for t in 0..room.counts[1] {
            let tex = room.texture(t);
            if !tex.is_streamed() {
                continue;
            }
            let Some(bytes) = room.stream_pixels(tex) else { continue };
            if bytes.len() < 8 || u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != CARRIER_MAGIC {
                continue;
            }
            if MARKER == NO_MARKER {
                MARKER = t as u16;
            }
            let id = u16::from_le_bytes([bytes[4], bytes[5]]);
            let k = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
            if k >= MAX_PALETTES {
                continue;
            }
            for slice in 0..MAX_SLICES {
                if Bank::open(SLICES[slice]).is_some_and(|bank| bank.id == id) {
                    CLUTS[slice][k] = crate::render::clut_word(tex.palette);
                }
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

/// The parts of global sprite `sprite`, each to `f`. False if the bank has no such sprite.
pub fn pieces(sprite: usize, mut f: impl FnMut(Piece)) -> bool {
    let Some((bank, slice, local, first)) = locate_sprite(sprite) else {
        unsafe { HK_ACTOR_BANK_MISSES = HK_ACTOR_BANK_MISSES.wrapping_add(1) };
        return false;
    };
    let s = bank.sprite(local);
    for p in s.first_part as usize..(s.first_part + s.parts) as usize {
        let part = bank.part(p);
        f(Piece {
            key: KEY_BASE + (first + p) as u16,
            clut: unsafe { CLUTS[slice][(s.palette as usize).min(MAX_PALETTES - 1)] },
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
    let (slice, index) = fmt::locate_part(counts(), index)?;
    let bank = Bank::open(unsafe { SLICES[slice] })?;
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
    extern crate std;
    use super::*;
    use hk_format::boss::{BANK_MAGIC, PART_BYTES, SPRITE_BYTES};

    /// A bank `id` of `sprites` one-part sprites, sprite `i` being 4x2 texels of byte `fill + i`, stored raw.
    fn bank(id: u16, sprites: u32, fill: u8) -> &'static [u8] {
        let mut v = std::vec::Vec::new();
        let put = |v: &mut std::vec::Vec<u8>, n: u32| v.extend(n.to_le_bytes());
        let sprite_at = 40u32;
        let part_at = sprite_at + sprites * SPRITE_BYTES as u32;
        let texels = part_at + sprites * PART_BYTES as u32;
        put(&mut v, BANK_MAGIC);
        put(&mut v, id as u32 | 1 << 16);
        for n in [sprites, sprites, 0, 0, 40, 40, sprite_at, part_at] {
            put(&mut v, n);
        }
        for i in 0..sprites {
            v.extend(4u16.to_le_bytes());
            v.extend(2u16.to_le_bytes());
            for b in [0i32, 0, 65536, 65536] {
                v.extend(b.to_le_bytes());
            }
            v.extend((i as u16).to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend([0u8; 4]);
        }
        for i in 0..sprites {
            v.extend(0u16.to_le_bytes());
            v.extend(0u16.to_le_bytes());
            v.extend([4u8, 2]);
            v.extend((4u16 | hk_format::boss::RAW).to_le_bytes());
            v.extend((i as u16).to_le_bytes());
            v.extend((texels + i * 4).to_le_bytes());
        }
        for i in 0..sprites {
            v.extend([fill + i as u8; 4]);
        }
        std::boxed::Box::leak(v.into_boxed_slice())
    }

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
    }

    #[test]
    fn two_slices_are_numbered_as_one() {
        set(&[bank(1, 3, 10), bank(2, 2, 50)]);
        assert!(active());
        let mut keys = std::vec::Vec::new();
        for sprite in 0..5 {
            assert!(pieces(sprite, |p| keys.push(p.key)));
        }
        let want: std::vec::Vec<u16> = (0..5).map(|i| KEY_BASE + i).collect();
        assert_eq!(keys, want);
        assert!(!pieces(5, |_| panic!("no such sprite")));
        // The slice a global part belongs to: parts 3 and 4 are the second slice's.
        let (b, slice, local, first) = locate_sprite(4).unwrap();
        assert_eq!((b.id, slice, local, first), (2, 1, 1, 3));
        set(&[]);
        assert!(!active());
    }

    #[test]
    fn a_list_past_the_limits_is_refused_whole() {
        set(&[bank(1, 1, 0), bank(2, 1, 0), bank(3, 1, 0)]);
        assert!(!active());
        set(&[bank(1, MAX_PARTS as u32 / 2 + 1, 0), bank(2, MAX_PARTS as u32 / 2 + 1, 0)]);
        assert!(!active());
    }
}
