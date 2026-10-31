//! A compact art bank for ordinary actors (docs/BOSS-ART.md, generalised past one boss): the sprites of an actor family as
//! parts of at most 64x64 texels held packbits-coded in one logical byte string (a `hk_format::boss::Bank` blob), decoded into
//! the animation cache's slots when a frame first needs them. A scene opts in by data (`data/actor_banks.rs`, written by
//! host/actor_bank.py from host/actor_banks.json). The blob is stored in up to two pieces of memory read as one (`Pieces`): its
//! head is a data package in the module pool (`modules::bytes`), whatever does not fit the room's pool slack follows as linked
//! read-only data. The bank's palettes ride in the views that draw the actors, as tiny carrier textures.
//!
//! A frame of a bank-drawn actor is an ordinary frame record whose texture is the first carrier texture (`marker`) and whose
//! first two box words carry the sprite's index in their low bytes (`frame_sprite`); every table the region has about clips and
//! frames stays what it always was, so clip lengths, completion and every validator keep working. A scene without a bank has
//! no marker (`NO_MARKER`) and no frame can match it.
//!
//! The bank's animation-cache keys are the boss bank's (`boss_bank::KEY_BASE`): a scene carries one or the other.
use hk_format::boss::{Bank, Blob};
use hk_format::Room;

/// A scene that draws actors from a bank: its manifest index, the module id of the data package that holds the head of the
/// bank's blob (a cook-generated `modules::ART_*` constant) and the rest of the blob, which is linked read-only data.
pub struct SceneBank {
    pub scene: usize,
    pub package: usize,
    pub rest: &'static [u8],
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
/// Palettes a bank has (the boss bank's limit).
pub const MAX_PALETTES: usize = 16;

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

/// One byte string stored in two pieces of memory: `head`, then `rest`. Reads past the end give 0 like any `Blob`.
#[derive(Clone, Copy)]
pub struct Pieces {
    head: &'static [u8],
    rest: &'static [u8],
}
impl Blob for Pieces {
    fn byte(&self, at: usize) -> u8 {
        match self.head.get(at) {
            Some(b) => *b,
            None => self.rest.get(at - self.head.len()).copied().unwrap_or(0),
        }
    }
    fn read(&self, at: usize, out: &mut [u8]) {
        let in_head = self.head.get(at..).unwrap_or(&[]);
        let n = in_head.len().min(out.len());
        out[..n].copy_from_slice(&in_head[..n]);
        let from = at.saturating_sub(self.head.len());
        let in_rest = self.rest.get(from..).unwrap_or(&[]);
        let m = in_rest.len().min(out.len() - n);
        out[n..n + m].copy_from_slice(&in_rest[..m]);
        out[n + m..].fill(0);
    }
}

/// The bank of the bound scene; empty pieces when there is none.
static mut BLOB: Pieces = Pieces { head: &[], rest: &[] };
/// The first carrier texture of the bound room, or `NO_MARKER`.
static mut MARKER: u16 = NO_MARKER;
/// The CLUT word of each palette, from the carrier textures of the bound room.
static mut CLUTS: [u16; MAX_PALETTES] = [0; MAX_PALETTES];

/// Hand the loader's bytes to the reader: the blob's head and the rest, or two empty slices to take the bank away. A bank
/// with more than `MAX_PARTS` parts is refused.
pub fn set(head: &'static [u8], rest: &'static [u8]) {
    unsafe {
        BLOB = Pieces { head, rest };
        MARKER = NO_MARKER;
        if Bank::open(BLOB).is_some_and(|b| b.parts as usize > MAX_PARTS) {
            BLOB = Pieces { head: &[], rest: &[] };
        }
    }
}

/// Whether a bank is loaded.
#[inline(always)]
pub fn active() -> bool {
    unsafe { !BLOB.head.is_empty() }
}

/// The texture id that marks a bank frame in the bound room.
#[inline(always)]
pub fn marker() -> u16 {
    unsafe { MARKER }
}

/// Bind the bank of manifest scene `scene` (or none) to the view `room`: hand its bytes to the reader and find the
/// carrier textures among the room's, once per view bind (`render::init`). A scene without an entry in
/// `data/actor_banks.rs` costs this one search; a scene with one whose package is not resident leaves the frames that name
/// the bank drawing nothing (`HK_ACTOR_BANK_UNBOUND`, and `HK_ACTOR_BANK_MISSES` per frame).
pub fn bind(room: &Room, scene: usize) {
    set(&[], &[]);
    let Some(entry) = SCENE_BANKS.iter().find(|e| e.scene == scene) else { return };
    match crate::modules::bytes(entry.package) {
        Some(head) => set(head, entry.rest),
        None => unsafe { HK_ACTOR_BANK_UNBOUND = HK_ACTOR_BANK_UNBOUND.wrapping_add(1) },
    }
    unsafe {
        let id = Bank::open(BLOB).map(|b| b.id);
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
            let k = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
            if k < MAX_PALETTES && id == Some(u16::from_le_bytes([bytes[4], bytes[5]])) {
                CLUTS[k] = crate::render::clut_word(tex.palette);
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
    let Some(bank) = Bank::open(unsafe { BLOB }).filter(|b| sprite < b.sprites as usize) else {
        unsafe { HK_ACTOR_BANK_MISSES = HK_ACTOR_BANK_MISSES.wrapping_add(1) };
        return false;
    };
    let s = bank.sprite(sprite);
    for p in s.first_part as usize..(s.first_part + s.parts) as usize {
        let part = bank.part(p);
        f(Piece {
            key: KEY_BASE + p as u16,
            clut: unsafe { CLUTS[(s.palette as usize).min(MAX_PALETTES - 1)] },
            width: part.width,
            height: part.height,
            bounds: Bank::<Pieces>::part_bounds(&s, &part),
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
    let bank = Bank::open(unsafe { BLOB }).filter(|b| index < b.parts as usize)?;
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
    use hk_format::boss::{BANK_MAGIC, PART_BYTES, RAW, SPRITE_BYTES};

    /// A bank of `sprites` one-part sprites, sprite `i` being 4x2 texels of byte `fill + i`, stored raw.
    fn bank(sprites: u32, fill: u8) -> std::vec::Vec<u8> {
        let mut v = std::vec::Vec::new();
        let put = |v: &mut std::vec::Vec<u8>, n: u32| v.extend(n.to_le_bytes());
        let sprite_at = 40u32;
        let part_at = sprite_at + sprites * SPRITE_BYTES as u32;
        let texels = part_at + sprites * PART_BYTES as u32;
        put(&mut v, BANK_MAGIC);
        put(&mut v, 7 | 1 << 16);
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
            v.extend((4u16 | RAW).to_le_bytes());
            v.extend((i as u16).to_le_bytes());
            v.extend((texels + i * 4).to_le_bytes());
        }
        for i in 0..sprites {
            v.extend([fill + i as u8; 4]);
        }
        v
    }
    fn leak(v: &[u8]) -> &'static [u8] {
        std::boxed::Box::leak(v.to_vec().into_boxed_slice())
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
    fn a_bank_split_anywhere_reads_the_same() {
        let whole = bank(5, 10);
        for cut in 0..=whole.len() {
            set(leak(&whole[..cut]), leak(&whole[cut..]));
            // An empty head means no bank, as an unloaded package does.
            assert_eq!(active(), cut != 0, "cut {cut}");
            if cut == 0 {
                continue;
            }
            let b = Bank::open(unsafe { BLOB }).expect("a bank whose header may straddle the cut");
            assert_eq!((b.sprites, b.parts), (5, 5));
            for i in 0..5 {
                let part = b.part(i);
                let mut got = [0u8; 4];
                b.decode_into(&part, &mut got[..part.texel_bytes().min(4)]).unwrap();
                assert_eq!(got, [10 + i as u8; 4], "cut {cut} part {i}");
            }
            let mut keys = std::vec::Vec::new();
            assert!(pieces(4, |p| keys.push(p.key)));
            assert_eq!(keys, [KEY_BASE + 4]);
            assert!(!pieces(5, |_| panic!("no such sprite")));
        }
        set(&[], &[]);
    }

    #[test]
    fn a_bank_over_the_part_limit_is_refused() {
        set(leak(&bank(MAX_PARTS as u32 + 1, 0)), &[]);
        assert!(!active());
    }
}
