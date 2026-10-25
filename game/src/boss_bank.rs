//! A boss's art bank (shared/hk-format `boss.rs`, docs/BOSS-ART.md): the sprites of a fight, as parts of at most 64x64 texels
//! held run-length coded in the boss's scene and decoded into the animation cache's slots when a frame first needs them.
//!
//! The bank rides in its scene as 2 KiB streamed textures (the chunks); nothing here is loaded from disc, so a fight never reads
//! the drive for art. A part's cache key is `KEY_BASE + part`, above every other art set's, so the 704-texture table, the
//! 416 CLUT slots and the 640-texture views keep their sizes whatever a boss shows.
use core::cell::Cell;
use hk_format::boss::{self as fmt, Bank, Blob};
use hk_format::Room;

/// Animation-cache keys, above the dialogue panel's tiles (which sit above the props').
pub const KEY_BASE: u16 =
    crate::dialogue_art::KEY_BASE + crate::dialogue_art::DIALOGUE_TILE_COUNT as u16;
/// Parts a bank may have: the keys that are left.
pub const MAX_PARTS: usize = 640;
const _: () = assert!(KEY_BASE as usize + MAX_PARTS <= hk_cache::MAX_KEYS);

#[no_mangle]
pub static mut HK_BOSS_PARTS_DRAWN: u32 = 0;
#[no_mangle]
pub static mut HK_BOSS_UPLOADS: u32 = 0;
/// Parts a frame had no room for in the working set (pop-in: must stay 0).
#[no_mangle]
pub static mut HK_BOSS_DROPS: u32 = 0;
/// Animated objects whose clip the bank does not have (an object the cooker's clip list missed: must stay 0).
#[no_mangle]
pub static mut HK_BOSS_CLIP_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_BOSS_BANK_FOUND: u32 = 0;

struct Index {
    chunks: [u16; fmt::MAX_CHUNKS],
    count: u8,
    id: u16,
    /// The scene's texture count when the index was made, which tells a different scene's table.
    textures: u16,
}
static mut INDEX: Index = Index { chunks: [0; fmt::MAX_CHUNKS], count: 0, id: 0, textures: 0 };

fn chunk_of(room: &Room, texture: usize) -> Option<(u16, usize)> {
    let t = room.texture(texture);
    if !t.is_streamed() || t.width != 64 || t.height != 64 {
        return None;
    }
    let bytes = room.stream_pixels(t)?;
    if u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != fmt::CHUNK_MAGIC {
        return None;
    }
    Some((u16::from_le_bytes([bytes[4], bytes[5]]), u16::from_le_bytes([bytes[6], bytes[7]]) as usize))
}

/// Find bank `id`'s chunks among the scene's textures, once per scene (a second call is a handful of reads). False if the scene
/// carries no such bank.
pub fn attach(room: &Room, id: u16) -> bool {
    unsafe {
        let ix = &mut *(&raw mut INDEX);
        if ix.count != 0 && ix.id == id && ix.textures as usize == room.counts[1] {
            if chunk_of(room, ix.chunks[0] as usize) == Some((id, 0)) {
                return true;
            }
        }
        ix.count = 0;
        let mut found = 0u8;
        for t in 0..room.counts[1] {
            if let Some((bank, at)) = chunk_of(room, t) {
                if bank == id && at < fmt::MAX_CHUNKS {
                    ix.chunks[at] = t as u16;
                    found = found.max(at as u8 + 1);
                }
            }
        }
        if found == 0 {
            return false;
        }
        ix.id = id;
        ix.textures = room.counts[1] as u16;
        ix.count = found;
        core::ptr::write_volatile(&raw mut HK_BOSS_BANK_FOUND, found as u32);
        true
    }
}

/// The bank's logical bytes through the scene's chunk textures. The chunk last read is remembered, so a run of bytes costs one
/// lookup per 2 KiB.
pub struct View<'a, 'r> {
    room: &'a Room<'r>,
    cur: Cell<(usize, &'r [u8])>,
}
impl<'r> View<'_, 'r> {
    /// Chunk `chunk`'s bytes, the last one read being remembered.
    fn chunk(&self, chunk: usize) -> Option<&'r [u8]> {
        let (have, bytes) = self.cur.get();
        if have == chunk {
            return Some(bytes);
        }
        let ix = unsafe { &*(&raw const INDEX) };
        if chunk >= ix.count as usize {
            return None;
        }
        let bytes = self.room.stream_pixels(self.room.texture(ix.chunks[chunk] as usize))?;
        self.cur.set((chunk, bytes));
        Some(bytes)
    }
}
impl Blob for View<'_, '_> {
    fn byte(&self, at: usize) -> u8 {
        let (chunk, offset) = fmt::locate(at);
        self.chunk(chunk).and_then(|b| b.get(offset).copied()).unwrap_or(0)
    }
    fn read(&self, at: usize, out: &mut [u8]) {
        let (mut at, mut done) = (at, 0);
        while done < out.len() {
            let (chunk, offset) = fmt::locate(at);
            let n = (out.len() - done).min(fmt::CHUNK_BYTES - offset);
            match self.chunk(chunk).and_then(|b| b.get(offset..offset + n)) {
                Some(run) => out[done..done + n].copy_from_slice(run),
                None => out[done..done + n].fill(0),
            }
            at += n;
            done += n;
        }
    }
}

/// Run `f` on the attached bank.
pub fn with<R>(room: &Room, f: impl FnOnce(&Bank<View<'_, '_>>) -> R) -> Option<R> {
    let bank = Bank::open(View { room, cur: Cell::new((usize::MAX, &[])) })?;
    Some(f(&bank))
}

/// The CLUT word of the bank's palette `k`: the palette of the chunk texture that carries it.
pub fn clut(room: &Room, palette: u8) -> u16 {
    let ix = unsafe { &*(&raw const INDEX) };
    let chunk = palette as usize % ix.count.max(1) as usize;
    crate::render::clut_word(room.texture(ix.chunks[chunk] as usize).palette)
}

#[repr(align(4))]
struct Buf([u8; 2048]);

/// The decoded part `index`, uploaded into the rectangle `dest` names for its size. Returns the bytes uploaded.
pub fn upload(room: &Room, index: usize, dest: impl FnOnce(u16, u16) -> psx_vram::VramRect) -> Option<u32> {
    with(room, |bank| {
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
        crate::texture_upload::upload(dest(part.width as u16, part.height as u16), &buf.0[..len]);
        unsafe { HK_BOSS_UPLOADS = HK_BOSS_UPLOADS.wrapping_add(1) };
        Some(len as u32)
    })?
}

/// One part to draw this frame.
#[derive(Clone, Copy)]
struct Queued {
    part: u16,
    clut: u16,
    width: u8,
    height: u8,
    flash: u8,
    verts: [(i16, i16); 4],
}
const QUEUE: usize = 24;
static mut QUEUE_PARTS: [Queued; QUEUE] = [Queued { part: 0, clut: 0, width: 0, height: 0, flash: 0, verts: [(0, 0); 4] }; QUEUE];
static mut QUEUE_LEN: usize = 0;

/// Start a frame's draws.
pub fn begin() {
    unsafe { QUEUE_LEN = 0 };
}

/// Queue part `part` of `bank` at `verts`. False if the queue is full.
pub fn queue(part: usize, palette_clut: u16, width: u8, height: u8, verts: [(i16, i16); 4], flash: u8) -> bool {
    unsafe {
        if QUEUE_LEN == QUEUE {
            HK_BOSS_DROPS = HK_BOSS_DROPS.wrapping_add(1);
            return false;
        }
        QUEUE_PARTS[QUEUE_LEN] = Queued { part: part as u16, clut: palette_clut, width, height, flash, verts };
        QUEUE_LEN += 1;
    }
    true
}

/// The queue's keys, added to the frame's working set as far as it has room.
pub fn append_needed(needed: &mut [u16], len: &mut usize) {
    unsafe {
        let mut kept = 0;
        for i in 0..QUEUE_LEN {
            let key = KEY_BASE + QUEUE_PARTS[i].part;
            if !needed[..*len].contains(&key) {
                if *len == needed.len() {
                    // No room: the parts from here on are not drawn this frame.
                    HK_BOSS_DROPS = HK_BOSS_DROPS.wrapping_add((QUEUE_LEN - i) as u32);
                    break;
                }
                needed[*len] = key;
                *len += 1;
            }
            kept = i + 1;
        }
        QUEUE_LEN = kept;
    }
}

/// Draw the queue (its keys are in this frame's prepared working set) and empty it.
pub fn draw() -> u32 {
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        prim::QuadTextured,
    };
    unsafe {
        let n = QUEUE_LEN;
        QUEUE_LEN = 0;
        for i in 0..n {
            let q = QUEUE_PARTS[i];
            let key = KEY_BASE + q.part;
            let (u, v) = crate::render::animation_uv(key);
            let right = (u as u16 + q.width as u16 - 1) as u8;
            let bottom = (v as u16 + q.height as u16 - 1) as u8;
            let template = QuadTextured::with_material(
                [(0, 0); 4],
                [(u, v), (right, v), (u, bottom), (right, bottom)],
                TextureMaterial::blended(q.clut, crate::render::animation_tpage_word(key), (128, 128, 128), BlendMode::Add),
            );
            crate::render::resident_quad_tinted(&template, q.verts, (128, 128, 128));
            if q.flash != 0 {
                crate::render::resident_quad_flash(&template, q.verts, crate::enemies::flash_tint(q.flash));
            }
        }
        HK_BOSS_PARTS_DRAWN = HK_BOSS_PARTS_DRAWN.wrapping_add(n as u32);
        n as u32
    }
}
