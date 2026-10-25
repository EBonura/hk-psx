//! The word decoder of the ability art's run-length code (game/src/unpack.rs) against a byte-at-a-time one.
#![allow(dead_code)]
#[path = "../../../game/src/unpack.rs"]
mod unpack;

/// The coded bytes one at a time, as the cooker's own `unpackbits` reads them.
fn bytes_of(src: &[u8], len: usize) -> Vec<u8> {
    let (mut out, mut at) = (Vec::new(), 0);
    while out.len() < len {
        let token = src[at] as usize;
        at += 1;
        if token < 0x80 {
            out.extend_from_slice(&src[at..at + token + 1]);
            at += token + 1;
        } else {
            out.extend(std::iter::repeat(src[at]).take((token & 0x7f) + 2));
            at += 1;
        }
    }
    out.truncate(len);
    out
}

/// A deterministic mix of literals and runs of every length around the word boundaries.
fn coded(seed: u32) -> Vec<u8> {
    let mut x = seed;
    let mut next = || {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        x >> 16
    };
    let mut out = Vec::new();
    for _ in 0..120 {
        if next() % 2 == 0 {
            let n = 1 + next() as usize % 20;
            out.push((n - 1) as u8);
            for _ in 0..n {
                out.push(next() as u8);
            }
        } else {
            out.push(0x80 | (next() % 20) as u8);
            out.push(next() as u8);
        }
    }
    out
}

#[test]
fn words_match_bytes_for_every_length_and_mix() {
    for seed in 0..40 {
        let src = coded(seed);
        // The decoded stream is as long as the tokens say; try every prefix length that ends mid-token too.
        let mut total = 0;
        let mut at = 0;
        while at < src.len() {
            let t = src[at] as usize;
            if t < 0x80 {
                total += t + 1;
                at += t + 2;
            } else {
                total += (t & 0x7f) + 2;
                at += 2;
            }
        }
        for len in 0..=total {
            let mut words = Vec::new();
            unpack::decode_words(&src, len, |w| words.push(w));
            let got: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            let mut want = bytes_of(&src, len);
            want.resize(len.div_ceil(4) * 4, 0);
            assert_eq!(got, want, "seed {seed} length {len}");
        }
    }
}
