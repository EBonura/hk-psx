//! HKBOSS: a boss's art bank, carried as streamed textures of its scene (docs/BOSS-ART.md).
//!
//! The bank is one logical byte string (`Blob`): a header, clip, sequence, sprite and part tables, and the parts' texels,
//! run-length coded. The cooker cuts the string into 2,048-byte chunks, each led by an 8-byte header, and appends every chunk to
//! the boss's scene as one ordinary 64x64 streamed texture. The scene bank therefore stores, loads, deduplicates and (with
//! the scene) frees the art with no new machinery; the guest finds the chunks by their magic and addresses the string through
//! them. The first `palettes` chunks also carry the bank's shared palettes as their own texture palettes, which is how the
//! palettes reach VRAM with the scene's CLUTs.
//!
//! Everything is little endian. Nothing here allocates.

pub const CHUNK_BYTES: usize = 2048;
pub const CHUNK_HEADER: usize = 8;
pub const CHUNK_PAYLOAD: usize = CHUNK_BYTES - CHUNK_HEADER;
/// Leads every chunk: "HKBC", the bank id (u16) and the chunk's index (u16).
pub const CHUNK_MAGIC: u32 = u32::from_le_bytes(*b"HKBC");
pub const BANK_MAGIC: u32 = u32::from_le_bytes(*b"HKBN");
pub const HEADER_BYTES: usize = 40;
pub const CLIP_BYTES: usize = 12;
pub const SPRITE_BYTES: usize = 28;
pub const PART_BYTES: usize = 14;
/// Most chunks one bank may have, and so the most the guest indexes.
pub const MAX_CHUNKS: usize = 96;
/// A part's coded length has this bit set when the texels are stored as they are.
pub const RAW: u16 = 0x8000;

/// The bank's logical bytes. Reads past the end give 0.
pub trait Blob {
    fn byte(&self, at: usize) -> u8;
}
impl Blob for &[u8] {
    fn byte(&self, at: usize) -> u8 {
        self.get(at).copied().unwrap_or(0)
    }
}

/// Where logical offset `at` lives: the chunk and the byte inside the chunk.
pub const fn locate(at: usize) -> (usize, usize) {
    (at / CHUNK_PAYLOAD, CHUNK_HEADER + at % CHUNK_PAYLOAD)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clip {
    pub first: u16,
    pub count: u16,
    pub fps: u32,
    pub wrap: u8,
    pub loop_start: u8,
}
impl Clip {
    /// The index into the clip's frames `age` 60 Hz ticks after it started, with the guest's loop / loop-section / once rules.
    pub fn frame(&self, age: u32) -> usize {
        let count = self.count.max(1) as u32;
        // floor(age * fps / (60 * 65536)) in 32 bits: the whole frames per second first, then the fraction's share.
        let age = age.min(0xffff);
        let (whole, frac) = (self.fps >> 16, self.fps & 0xffff);
        let elapsed = (age * whole + (age * frac >> 16)) / 60;
        let start = self.loop_start as u32;
        (if self.wrap == 0 {
            elapsed % count
        } else if self.wrap == 1 && elapsed >= count {
            start + (elapsed - start) % (count - start).max(1)
        } else {
            elapsed.min(count - 1)
        }) as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub width: u16,
    pub height: u16,
    /// The world box `[x0, y0, x1, y1]` in Q16.16, in a room frame record's order.
    pub bounds: [i32; 4],
    pub first_part: u16,
    pub parts: u16,
    pub palette: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    /// The part's place in its sprite, in pixels from the top left, and its size.
    pub x: u16,
    pub y: u16,
    pub width: u8,
    pub height: u8,
    /// Stored length with `RAW` set when the texels are not coded.
    pub coded: u16,
    pub sprite: u16,
    pub offset: u32,
}
impl Part {
    /// Bytes per row of a part `width` pixels wide, rows padded to a halfword as the slot upload wants.
    pub const fn stride(width: usize) -> usize {
        ((width + 3) & !3) / 2
    }
    /// The decoded texel bytes.
    pub fn texel_bytes(&self) -> usize {
        Self::stride(self.width as usize) * self.height as usize
    }
}

pub struct Bank<B: Blob> {
    pub blob: B,
    pub id: u16,
    pub palettes: u16,
    pub sprites: u32,
    pub parts: u32,
    pub clips: u32,
    pub seq: u32,
    off: [u32; 4],
}
impl<B: Blob> Bank<B> {
    pub fn u16_at(&self, at: usize) -> u16 {
        self.blob.byte(at) as u16 | (self.blob.byte(at + 1) as u16) << 8
    }
    pub fn u32_at(&self, at: usize) -> u32 {
        self.u16_at(at) as u32 | (self.u16_at(at + 2) as u32) << 16
    }
    pub fn open(blob: B) -> Option<Self> {
        let mut bank = Bank { blob, id: 0, palettes: 0, sprites: 0, parts: 0, clips: 0, seq: 0, off: [0; 4] };
        if bank.u32_at(0) != BANK_MAGIC {
            return None;
        }
        bank.id = bank.u16_at(4);
        bank.palettes = bank.u16_at(6);
        bank.sprites = bank.u32_at(8);
        bank.parts = bank.u32_at(12);
        bank.clips = bank.u32_at(16);
        bank.seq = bank.u32_at(20);
        for i in 0..4 {
            bank.off[i] = bank.u32_at(24 + i * 4);
        }
        Some(bank)
    }
    pub fn clip(&self, i: usize) -> Clip {
        let p = self.off[0] as usize + i * CLIP_BYTES;
        Clip { first: self.u16_at(p), count: self.u16_at(p + 2), fps: self.u32_at(p + 4), wrap: self.blob.byte(p + 8), loop_start: self.blob.byte(p + 9) }
    }
    /// The sprite the `i`th frame of the sequence table shows.
    pub fn seq_sprite(&self, i: usize) -> usize {
        self.u16_at(self.off[1] as usize + i * 2) as usize
    }
    pub fn sprite(&self, i: usize) -> Sprite {
        let p = self.off[2] as usize + i * SPRITE_BYTES;
        Sprite {
            width: self.u16_at(p),
            height: self.u16_at(p + 2),
            bounds: [self.u32_at(p + 4) as i32, self.u32_at(p + 8) as i32, self.u32_at(p + 12) as i32, self.u32_at(p + 16) as i32],
            first_part: self.u16_at(p + 20),
            parts: self.u16_at(p + 22),
            palette: self.blob.byte(p + 24),
        }
    }
    pub fn part(&self, i: usize) -> Part {
        let p = self.off[3] as usize + i * PART_BYTES;
        Part {
            x: self.u16_at(p),
            y: self.u16_at(p + 2),
            width: self.blob.byte(p + 4),
            height: self.blob.byte(p + 5),
            coded: self.u16_at(p + 6),
            sprite: self.u16_at(p + 8),
            offset: self.u32_at(p + 10),
        }
    }
    /// The part's decoded texel bytes, in order.
    pub fn decode(&self, part: &Part) -> Decode<'_, B> {
        Decode {
            bank: self,
            at: part.offset as usize,
            raw: part.coded & RAW != 0,
            left: part.texel_bytes(),
            literal: 0,
            run: 0,
            value: 0,
        }
    }
    /// The part's world box inside its sprite's, in a frame record's `[x0, y0, x1, y1]` order (art row 0 is the top, `b[3]`).
    pub fn part_bounds(sprite: &Sprite, part: &Part) -> [i32; 4] {
        let (w, h) = (sprite.width.max(1) as u32, sprite.height.max(1) as u32);
        let b = sprite.bounds;
        let (px, py) = (part.x as u32, part.y as u32);
        [
            super::lerp_tile(b[0], b[2], px, w),
            super::lerp_tile(b[3], b[1], py + part.height as u32, h),
            super::lerp_tile(b[0], b[2], px + part.width as u32, w),
            super::lerp_tile(b[3], b[1], py, h),
        ]
    }
}

/// The decoded bytes of one part. A token below 0x80 is a literal of token + 1 bytes; from 0x80 it is a run of
/// (token & 0x7f) + 2 copies of the next byte (the tokens `game/src/ability_art.rs` decodes).
pub struct Decode<'a, B: Blob> {
    bank: &'a Bank<B>,
    at: usize,
    raw: bool,
    left: usize,
    literal: usize,
    run: usize,
    value: u8,
}
impl<B: Blob> Decode<'_, B> {
    pub fn remaining(&self) -> usize {
        self.left
    }
    pub fn next_byte(&mut self) -> Option<u8> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        if self.raw {
            self.at += 1;
            return Some(self.bank.blob.byte(self.at - 1));
        }
        if self.run == 0 && self.literal == 0 {
            let token = self.bank.blob.byte(self.at);
            self.at += 1;
            if token < 0x80 {
                self.literal = token as usize + 1;
            } else {
                self.run = (token & 0x7f) as usize + 2;
                self.value = self.bank.blob.byte(self.at);
                self.at += 1;
            }
        }
        if self.run != 0 {
            self.run -= 1;
            Some(self.value)
        } else {
            self.literal -= 1;
            self.at += 1;
            Some(self.bank.blob.byte(self.at - 1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    #[test]
    fn locate_skips_the_chunk_headers() {
        assert_eq!(locate(0), (0, 8));
        assert_eq!(locate(CHUNK_PAYLOAD - 1), (0, CHUNK_BYTES - 1));
        assert_eq!(locate(CHUNK_PAYLOAD), (1, 8));
    }

    #[test]
    fn clip_frames_follow_the_loop_rules() {
        let clip = |wrap, count, loop_start| Clip { first: 0, count, fps: 12 << 16, wrap, loop_start };
        // 12 fps: a frame every 5 ticks.
        assert_eq!(clip(0, 4, 0).frame(5 * 5), 1);
        assert_eq!(clip(2, 4, 0).frame(5 * 9), 3);
        // Loop section: after the last frame it returns to `loop_start`.
        assert_eq!(clip(1, 4, 2).frame(5 * 4), 2);
        assert_eq!(clip(1, 4, 2).frame(5 * 5), 3);
        assert_eq!(clip(1, 4, 2).frame(5 * 6), 2);
    }

    fn bank_bytes(coded: &[u8], raw: bool, width: u8, height: u8) -> Vec<u8> {
        let mut v = Vec::new();
        let put32 = |v: &mut Vec<u8>, n: u32| v.extend(n.to_le_bytes());
        put32(&mut v, BANK_MAGIC);
        put32(&mut v, 1 | 1 << 16);
        for n in [1u32, 1, 0, 0, 40, 40, 40, 40 + SPRITE_BYTES as u32] {
            put32(&mut v, n);
        }
        // Sprite 0.
        v.extend((width as u16).to_le_bytes());
        v.extend((height as u16).to_le_bytes());
        for b in [0i32, 0, 65536, 65536] {
            v.extend(b.to_le_bytes());
        }
        v.extend(0u16.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend([0u8; 4]);
        // Part 0.
        v.extend(0u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend([width, height]);
        v.extend((coded.len() as u16 | if raw { RAW } else { 0 }).to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(((40 + SPRITE_BYTES + PART_BYTES) as u32).to_le_bytes());
        v.extend(coded);
        v
    }

    #[test]
    fn a_coded_part_decodes_and_a_raw_part_is_copied() {
        // 8x2 pixels: stride 4, 8 bytes. A run of five 0x11, then a literal of three.
        let coded = [0x80 | 3, 0x11, 2, 0xa, 0xb, 0xc];
        let bytes = bank_bytes(&coded, false, 8, 2);
        let bank = Bank::open(bytes.as_slice()).unwrap();
        let (sprite, part) = (bank.sprite(0), bank.part(0));
        assert_eq!((sprite.width, part.sprite, part.texel_bytes()), (8, 0, 8));
        let mut d = bank.decode(&part);
        let out: Vec<u8> = core::iter::from_fn(|| d.next_byte()).collect();
        assert_eq!(out, [0x11, 0x11, 0x11, 0x11, 0x11, 0xa, 0xb, 0xc]);
        let raw = [1, 2, 3, 4, 5, 6, 7, 8];
        let bytes = bank_bytes(&raw, true, 8, 2);
        let bank = Bank::open(bytes.as_slice()).unwrap();
        let mut d = bank.decode(&bank.part(0));
        let out: Vec<u8> = core::iter::from_fn(|| d.next_byte()).collect();
        assert_eq!(out, raw);
    }

    #[test]
    fn a_part_box_is_its_share_of_the_sprite_box() {
        let sprite = Sprite { width: 100, height: 50, bounds: [0, 0, 100 << 16, 50 << 16], first_part: 0, parts: 1, palette: 0 };
        let part = Part { x: 25, y: 10, width: 50, height: 20, coded: 0, sprite: 0, offset: 0 };
        let b = Bank::<&[u8]>::part_bounds(&sprite, &part);
        // Art row 0 is the top of the box, so y counts down from b[3].
        assert_eq!(b, [25 << 16, 20 << 16, 75 << 16, 40 << 16]);
    }
}
