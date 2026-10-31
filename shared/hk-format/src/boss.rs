//! HKBOSS: a boss's art bank, carried as streamed textures of its scene (docs/BOSS-ART.md).
//!
//! The bank is one logical byte string (`Blob`): a header, clip, sequence, sprite and part tables, the bank's code table
//! (if any part uses it), and the parts' texels, each stored raw, packbits or Huffman coded (`RAW`, `CODED`, `code`). The cooker cuts the string into 2,048-byte chunks, each led by an 8-byte header, and appends every chunk to
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
/// A part's coded length has this bit set when the texels are Huffman coded with the bank's code table (`code` below). A part
/// with neither bit is packbits, the first coder, so a bank cooked before the table existed decodes unchanged.
pub const CODED: u16 = 0x4000;
/// A part's stored length is the low 14 bits of its `coded` word.
pub const LEN_MASK: u16 = 0x3fff;

/// The bank's logical bytes. Reads past the end give 0.
pub trait Blob {
    fn byte(&self, at: usize) -> u8;
    /// `out.len()` bytes from `at`. A blob that can copy a run at once should say so: a guest decoding a part reads thousands.
    fn read(&self, at: usize, out: &mut [u8]) {
        for (i, b) in out.iter_mut().enumerate() {
            *b = self.byte(at + i);
        }
    }
}
impl Blob for &[u8] {
    fn byte(&self, at: usize) -> u8 {
        self.get(at).copied().unwrap_or(0)
    }
    fn read(&self, at: usize, out: &mut [u8]) {
        let have = self.get(at..).unwrap_or(&[]);
        let n = have.len().min(out.len());
        out[..n].copy_from_slice(&have[..n]);
        out[n..].fill(0);
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
    // `clip` and `sprite` stay out of line, like `part`: each is a dozen table reads, and one copy of each keeps the Hornet
    // module, which draws a boss, under its 64 KiB.
    #[inline(never)]
    pub fn clip(&self, i: usize) -> Clip {
        let p = self.off[0] as usize + i * CLIP_BYTES;
        Clip { first: self.u16_at(p), count: self.u16_at(p + 2), fps: self.u32_at(p + 4), wrap: self.blob.byte(p + 8), loop_start: self.blob.byte(p + 9) }
    }
    /// The sprite the `i`th frame of the sequence table shows.
    pub fn seq_sprite(&self, i: usize) -> usize {
        self.u16_at(self.off[1] as usize + i * 2) as usize
    }
    #[inline(never)]
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
    /// Where the bank's code table starts: right after the part table. Only a bank with a `CODED` part has one.
    pub fn table_at(&self) -> usize {
        self.off[3] as usize + self.parts as usize * PART_BYTES
    }
    /// The part's decoded texel bytes into `out` (at least `texel_bytes` long).
    pub fn decode_into(&self, part: &Part, out: &mut [u8]) -> Option<()> {
        decode_part(&self.blob, self.table_at(), part, out)
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

/// The part's decoded texel bytes into `out` (at least `texel_bytes` long), for any of the three coders. `table_at` is where
/// the bank's code table starts (`Bank::table_at`); a bank without `CODED` parts never reads it. One decoder serves the boss
/// bank, which reads the bank through its chunk textures, and the actor banks, which read it from two pieces of memory.
pub fn decode_part<B: Blob>(blob: &B, table_at: usize, part: &Part, out: &mut [u8]) -> Option<()> {
    let n = part.texel_bytes();
    let out = out.get_mut(..n)?;
    let mut at = part.offset as usize;
    if part.coded & RAW != 0 {
        blob.read(at, out);
        return Some(());
    }
    if part.coded & CODED != 0 {
        return code::decode(blob, table_at, at, part, out);
    }
    // Packbits: a token below 0x80 is a literal of token + 1 bytes; from 0x80 it is a run of (token & 0x7f) + 2 copies of the
    // next byte (the tokens `game/src/ability_art.rs` decodes).
    let mut done = 0;
    while done < n {
        let token = blob.byte(at);
        at += 1;
        if token < 0x80 {
            let len = (token as usize + 1).min(n - done);
            blob.read(at, &mut out[done..done + len]);
            at += token as usize + 1;
            done += len;
        } else {
            let len = ((token & 0x7f) as usize + 2).min(n - done);
            out[done..done + len].fill(blob.byte(at));
            at += 1;
            done += len;
        }
    }
    Some(())
}

/// The `CODED` coder: a static Huffman code per context over a stream of tokens, decoded at a part's upload.
///
/// A part is the pixels of its padded rectangle (`Part::stride(width) * 2` pixels a row, `height` rows) in reading order, each
/// pixel 4 bits (index 0 is transparent). The stream is a sequence of tokens; the context of a token is the value of the pixel
/// before it (the last pixel of the previous token, 0 at the start; the end of one row is the pixel before the next row's
/// first). A token is one of:
///
/// * a literal pixel 1 to 15 (symbols 0 to 14);
/// * a run of 2^k to 2^(k+1) - 1 transparent pixels, k raw extra bits for the length (symbols 15 to 27 for k = 0 to 12);
/// * a run of copies of the previous pixel, which is not 0 (symbols 28 to 39 for k = 1 to 12);
/// * a run of pixels each equal to the pixel one row above (symbols 40 to 51 for k = 1 to 12).
///
/// Runs cross row ends and stop at the end of the part. The code of a context is canonical, at most `MAX_LEN` bits a symbol,
/// and the bank's code is stored once, after the part table:
///
/// ```text
/// u16 length of the table in bytes, this field included
/// u16 x CONTEXTS   offset of each context's record from the start of the table
/// record: u8 x MAX_LEN   the number of codes of length 1 to MAX_LEN
///         u8 x 2^SHORT_BITS   for each SHORT_BITS-bit window, the symbol (low 6 bits) and its code length less one (top 2
///                        bits) if the window starts a code of SHORT_BITS bits or fewer, else 0xff
///         u8 x (sum of the counts)   the symbols in code order (by length, then by symbol)
/// ```
///
/// The stream is read most significant bit first, a symbol's extra bits following its code.
///
/// Speed is the point of the decoder's shape. The console has no data cache, so a load or store of RAM costs about 6 cycles
/// (measured on the emulator, `docs/ACTOR-BANK.md`) and a decoder that keeps its state in memory runs at 240 cycles a token.
/// `walk` therefore has no call inside it and few live values, so its state stays in registers; it hands back to the caller when
/// it needs input or a context's code. The caller derives that code on first use (for each length, the first 12-bit window
/// that is not a code of that length or shorter, and what to add to the window's leading bits to index the symbols), so a part
/// pays for the contexts it meets and nothing else.
pub mod code {
    use super::{Blob, Part};

    pub const CONTEXTS: usize = 16;
    pub const MAX_LEN: usize = 12;
    pub const SYMBOLS: usize = 52;
    /// The first symbol of each kind.
    pub const ZERO_RUN: usize = 15;
    pub const LEFT_RUN: usize = 28;
    pub const UP_RUN: usize = 40;
    pub const TABLE_HEAD: usize = 2 + 2 * CONTEXTS;
    /// The largest table: every context with a full alphabet.
    /// Windows of this many bits index a context's `short` table: codes of this length or less take one look-up.
    /// (The entry holds the length less one in two bits, so this cannot pass 4.)
    pub const SHORT_BITS: usize = 4;
    const SHORT_LEN: usize = 1 << SHORT_BITS;
    /// Lengths beyond `SHORT_BITS`, which the slow path walks.
    const LONG_LENS: usize = MAX_LEN - SHORT_BITS;
    pub const RECORD_MAX: usize = MAX_LEN + SHORT_LEN + SYMBOLS;
    pub const TABLE_MAX: usize = TABLE_HEAD + CONTEXTS * RECORD_MAX;
    /// Bytes of the stream held at once.
    const WINDOW: usize = 64;

    /// One context's code, derived from its record. `limit[0] == 0` is the mark of a code not yet derived.
    /// 128 bytes, so that a context's code is found by a shift; 64 symbol bytes, so that a corrupt stream indexes inside it.
    #[derive(Clone, Copy)]
    #[repr(C, align(128))]
    struct Code {
        /// For each length beyond `SHORT_BITS`, the first MAX_LEN-bit window that is not a code of that length or shorter,
        /// and what to add to the window's leading bits to index `symbols`.
        limit: [u16; LONG_LENS],
        base: [i16; LONG_LENS],
        short: [u8; SHORT_LEN],
        symbols: [u8; 64],
    }
    const NO_CODE: Code = Code { limit: [0; LONG_LENS], base: [0; LONG_LENS], short: [0xff; SHORT_LEN], symbols: [0; 64] };
    const _: () = assert!(core::mem::size_of::<Code>() == 128);

    /// Where `walk` stands. Everything it needs to carry on after it hands back.
    struct State {
        pixel: usize,
        left: usize,
        /// The next bits of the stream, most significant first, and how many there are.
        bits: u32,
        have: u32,
        /// The next unread byte of the window.
        used: usize,
    }

    enum Step {
        Done,
        /// The window is empty.
        Input,
        /// The context's code is not derived yet.
        Code(usize),
        Corrupt,
    }

    /// Read `ctx`'s record and derive its code.
    #[inline(never)]
    fn derive<B: Blob>(blob: &B, table_at: usize, record: usize, code: &mut Code) -> Option<()> {
        let mut raw = [0u8; RECORD_MAX];
        blob.read(table_at + record, &mut raw);
        let (mut first, mut index) = (0u32, 0u32);
        for len in 0..MAX_LEN {
            let count = raw[len] as u32;
            if len >= SHORT_BITS {
                code.limit[len - SHORT_BITS] = ((first + count) << (MAX_LEN - 1 - len)) as u16;
                code.base[len - SHORT_BITS] = (index as i32 - first as i32) as i16;
            }
            index += count;
            first = (first + count) << 1;
        }
        let used = index as usize;
        if used == 0 || used > SYMBOLS {
            return None;
        }
        code.short.copy_from_slice(&raw[MAX_LEN..MAX_LEN + SHORT_LEN]);
        code.symbols[..used].copy_from_slice(&raw[MAX_LEN + SHORT_LEN..MAX_LEN + SHORT_LEN + used]);
        // Mark the code derived: the first long length is never 0 for a derived code that has a long code, and a code
        // without one never reaches the slow path, so the mark is only read there.
        code.limit[0] = code.limit[0].max(1);
        Some(())
    }

    /// Decode tokens until the part is done or `walk` needs something. `out` is the part's zeroed texel bytes.
    ///
    /// The hot loop of the bank reader: no call, no bounds check it can do without. `window[..WINDOW]` holds the stream from
    /// `state.used`.
    ///
    /// Inlined into `decode`, which is generic over the blob: every reader of a bank gets its own copy of the decoder, so a
    /// reader that lives in a code module (the boss's, the actor banks') carries it with it.
    #[inline(always)]
    fn walk(codes: &[Code; CONTEXTS], window: &[u8; WINDOW], state: &mut State, out: &mut [u8], total: usize, row: usize) -> Step {
        let (mut pixel, mut left, mut bits, mut have, mut used) = (state.pixel, state.left, state.bits, state.have, state.used);
        let out_ptr = out.as_mut_ptr();
        let step = loop {
            if pixel >= total {
                break Step::Done;
            }
            // At least 25 bits: a symbol is at most 12 code bits and 12 extra bits.
            while have < 25 {
                if used == WINDOW {
                    break;
                }
                bits |= (window[used] as u32) << (24 - have);
                used += 1;
                have += 8;
            }
            if have < 25 {
                break Step::Input;
            }
            let code = &codes[left & (CONTEXTS - 1)];
            let short = code.short[(bits >> (32 - SHORT_BITS)) as usize];
            let symbol;
            if short & 63 != 63 {
                symbol = (short & 63) as usize;
                let len = (short >> 6) as u32 + 1;
                bits <<= len;
                have -= len;
            } else {
                // A code of more than SHORT_BITS bits. An underived context has `limit[0] == 0` and every `short` entry
                // 0xff, which comes here and passes the compare at every length.
                let peek = (bits >> (32 - MAX_LEN)) as u16;
                let mut long = 0;
                while long < LONG_LENS && peek >= code.limit[long] {
                    long += 1;
                }
                if long == LONG_LENS {
                    break if code.limit[0] == 0 { Step::Code(left & (CONTEXTS - 1)) } else { Step::Corrupt };
                }
                let index = (peek >> (LONG_LENS - 1 - long)) as i32 + code.base[long] as i32;
                // Slots past the alphabet are zero, a literal: a corrupt stream decodes to something, never out of bounds.
                symbol = code.symbols[index as usize & 63] as usize;
                let len = (long + SHORT_BITS + 1) as u32;
                bits <<= len;
                have -= len;
            }
            if symbol < ZERO_RUN {
                left = symbol + 1;
                // SAFETY: `pixel < total` and `total / 2` is the length of `out` (`texel_bytes`), so the byte is inside it.
                unsafe { *out_ptr.add(pixel >> 1) |= (left as u8) << ((pixel & 1) << 2) };
                pixel += 1;
                continue;
            }
            let (first, shift) = if symbol < LEFT_RUN {
                (ZERO_RUN, 0)
            } else if symbol < UP_RUN {
                (LEFT_RUN, 1)
            } else {
                (UP_RUN, 1)
            };
            let k = (symbol - first + shift) as u32;
            let mut run = 1usize << k;
            if k != 0 {
                run += (bits >> (32 - k)) as usize;
                bits <<= k;
                have -= k;
            }
            let end = (pixel + run).min(total);
            if symbol < LEFT_RUN {
                left = 0;
            } else if symbol < UP_RUN {
                // `left` is not 0 here, and stays.
                let value = left as u8;
                let mut at = pixel;
                // SAFETY (this block): `at < end <= total`, and pixels below `total` are bytes of `out` (see above).
                unsafe {
                    if at & 1 != 0 {
                        *out_ptr.add(at >> 1) |= value << 4;
                        at += 1;
                    }
                    while at + 2 <= end {
                        *out_ptr.add(at >> 1) = value * 17;
                        at += 2;
                    }
                    if at < end {
                        *out_ptr.add(at >> 1) |= value;
                    }
                }
            } else {
                if pixel < row {
                    break Step::Corrupt;
                }
                let mut at = pixel;
                // SAFETY (this block): `row <= pixel`, so every source pixel `at - row` is a byte of `out` below the pixel
                // being written, and `at < end <= total`.
                unsafe {
                    if at & 1 != 0 {
                        let up = (*out_ptr.add((at - row) >> 1) >> (((at - row) & 1) << 2)) & 15;
                        *out_ptr.add(at >> 1) |= up << 4;
                        at += 1;
                    }
                    // `row` is a multiple of 4 pixels, so a byte copies from the byte `row / 2` before it.
                    while at + 2 <= end {
                        *out_ptr.add(at >> 1) = *out_ptr.add((at >> 1) - row / 2);
                        at += 2;
                    }
                    if at < end {
                        *out_ptr.add(at >> 1) |= *out_ptr.add((at >> 1) - row / 2) & 15;
                    }
                    left = ((*out_ptr.add((end - 1) >> 1) >> (((end - 1) & 1) << 2)) & 15) as usize;
                }
            }
            pixel = end;
        };
        *state = State { pixel, left, bits, have, used };
        step
    }

    pub(super) fn decode<B: Blob>(blob: &B, table_at: usize, mut at: usize, part: &Part, out: &mut [u8]) -> Option<()> {
        let mut head = [0u8; TABLE_HEAD];
        blob.read(table_at, &mut head);
        let table_len = u16::from_le_bytes([head[0], head[1]]) as usize;
        if !(TABLE_HEAD..=TABLE_MAX).contains(&table_len) {
            return None;
        }
        out.fill(0);
        let row = super::Part::stride(part.width as usize) * 2;
        let total = row * part.height as usize;
        if out.len() * 2 != total {
            return None;
        }
        let mut codes = [NO_CODE; CONTEXTS];
        let mut window = [0u8; WINDOW];
        let mut state = State { pixel: 0, left: 0, bits: 0, have: 0, used: WINDOW };
        loop {
            match walk(&codes, &window, &mut state, out, total, row) {
                Step::Done => return Some(()),
                Step::Input => {
                    blob.read(at, &mut window);
                    at += WINDOW;
                    state.used = 0;
                }
                Step::Code(ctx) => {
                    let record = u16::from_le_bytes([head[2 + 2 * ctx], head[3 + 2 * ctx]]) as usize;
                    if record < TABLE_HEAD || record + MAX_LEN > table_len {
                        return None;
                    }
                    derive(blob, table_at, record, &mut codes[ctx])?;
                }
                Step::Corrupt => return None,
            }
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
        let mut whole = [0u8; 8];
        bank.decode_into(&part, &mut whole).unwrap();
        assert_eq!(whole, [0x11, 0x11, 0x11, 0x11, 0x11, 0xa, 0xb, 0xc]);
        let raw = [1, 2, 3, 4, 5, 6, 7, 8];
        let bytes = bank_bytes(&raw, true, 8, 2);
        let bank = Bank::open(bytes.as_slice()).unwrap();
        let mut whole = [0u8; 8];
        bank.decode_into(&bank.part(0), &mut whole).unwrap();
        assert_eq!(whole, raw);
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
