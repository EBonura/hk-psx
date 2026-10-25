//! Run-length coded texels (host/ability_art.py `packbits`) decoded to the words a GPU upload writes.
//!
//! A token below 0x80 is a literal of token + 1 bytes; from 0x80 it is a run of (token & 0x7f) + 2
//! copies of the next byte.

/// The first `len` bytes of a run-length coded frame (`Unpack`'s format), as little-endian words handed
/// to `sink` in order: a run of one byte is written a word at a time, which is most of an effect frame
/// (long runs of the transparent index). `len` is a multiple of four or the last word is padded with zeros.
#[inline]
pub fn decode_words(src: &[u8], len: usize, mut sink: impl FnMut(u32)) {
    let (mut at, mut left) = (0usize, len);
    let (mut word, mut fill) = (0u32, 0u32);
    while left > 0 {
        let token = src[at] as usize;
        at += 1;
        if token < 0x80 {
            let n = (token + 1).min(left);
            let bytes = &src[at..at + n];
            let mut i = 0;
            // Top up the word in progress, then whole words four bytes at a time, then the rest.
            while fill != 0 && i < n {
                word |= u32::from(bytes[i]) << (fill * 8);
                fill += 1;
                i += 1;
                if fill == 4 {
                    sink(word);
                    (word, fill) = (0, 0);
                }
            }
            while i + 4 <= n {
                sink(u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]));
                i += 4;
            }
            while i < n {
                word |= u32::from(bytes[i]) << (fill * 8);
                fill += 1;
                i += 1;
            }
            at += token + 1;
            left -= n;
        } else {
            let mut n = ((token & 0x7f) + 2).min(left);
            let value = u32::from(src[at]);
            at += 1;
            left -= n;
            // Top up the word in progress, then whole words of the byte, then the rest.
            while fill != 0 && n > 0 {
                word |= value << (fill * 8);
                fill += 1;
                n -= 1;
                if fill == 4 {
                    sink(word);
                    (word, fill) = (0, 0);
                }
            }
            let whole = value * 0x0101_0101;
            for _ in 0..n / 4 {
                sink(whole);
            }
            for _ in 0..n % 4 {
                word |= value << (fill * 8);
                fill += 1;
            }
        }
    }
    if fill != 0 {
        sink(word);
    }
}

