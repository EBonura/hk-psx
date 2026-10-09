//! LZ4 block compression and decompression, written from the published LZ4
//! block format description (one block, no frame, no dictionary).
//!
//! The compressor is a high-compression hash-chain parser of our own: every
//! position is indexed by its next four bytes, each candidate chain is walked
//! to a fixed depth for the longest (then nearest) match, and a match is
//! deferred by one byte when the next position holds a longer one.
//!
//! The block format only fixes what a valid block looks like, not how the
//! input is cut into sequences, so the compressed bytes are this crate's own
//! choice; every block decodes to the input under any conforming decoder.

/// Shortest match a sequence can encode.
const MIN_MATCH: usize = 4;
/// A match must start at least this many bytes before the end of the block.
const LAST_MATCH_START: usize = 12;
/// The last bytes of a block are always literals.
const TRAILING_LITERALS: usize = 5;
/// Largest back-reference distance (the offset field is 16 bits, zero invalid).
const MAX_OFFSET: usize = 65535;
/// Candidates examined per position.
const CHAIN_DEPTH: usize = 512;
const HASH_BITS: u32 = 15;

fn read32(src: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([src[i], src[i + 1], src[i + 2], src[i + 3]])
}

fn hash4(v: u32) -> usize {
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// Hash chains over the input: `head[h]` is the latest indexed position with
/// hash `h` plus one (zero is empty), `prev[p]` the earlier position it chains to.
struct Chains<'a> {
    src: &'a [u8],
    head: Vec<u32>,
    prev: Vec<u32>,
    /// Positions below this are indexed.
    indexed: usize,
    /// One past the last position where a match may start.
    match_end: usize,
}

impl<'a> Chains<'a> {
    fn new(src: &'a [u8]) -> Self {
        let match_end = if src.len() > LAST_MATCH_START { src.len() - LAST_MATCH_START + 1 } else { 0 };
        Chains { src, head: vec![0; 1 << HASH_BITS], prev: vec![0; src.len()], indexed: 0, match_end }
    }

    fn index_up_to(&mut self, end: usize) {
        let end = end.min(self.match_end);
        while self.indexed < end {
            let p = self.indexed;
            let h = hash4(read32(self.src, p));
            self.prev[p] = self.head[h];
            self.head[h] = p as u32 + 1;
            self.indexed += 1;
        }
    }

    /// Longest match for position `i` (longer than `floor`, nearest on ties):
    /// (length, distance).
    fn best(&mut self, i: usize, floor: usize) -> Option<(usize, usize)> {
        if i >= self.match_end {
            return None;
        }
        self.index_up_to(i);
        let max_len = self.src.len() - TRAILING_LITERALS - i;
        let mut best_len = floor.max(MIN_MATCH - 1);
        let mut best = None;
        let mut cand = self.head[hash4(read32(self.src, i))];
        let mut left = CHAIN_DEPTH;
        while cand != 0 && left > 0 {
            let c = cand as usize - 1;
            if i - c > MAX_OFFSET {
                break;
            }
            // A longer match must agree at the byte just past the current best.
            if best_len < max_len && self.src[c + best_len] == self.src[i + best_len] {
                let mut l = 0;
                while l < max_len && self.src[c + l] == self.src[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best = Some((l, i - c));
                }
            }
            cand = self.prev[c];
            left -= 1;
        }
        best
    }
}

fn put_length(out: &mut Vec<u8>, mut rest: usize) {
    while rest >= 255 {
        out.push(255);
        rest -= 255;
    }
    out.push(rest as u8);
}

/// Append one sequence: `literals`, then (when `m` is given) a match of
/// (length, distance).
fn put_sequence(out: &mut Vec<u8>, literals: &[u8], m: Option<(usize, usize)>) {
    let lit_nibble = literals.len().min(15) as u8;
    let match_nibble = m.map_or(0, |(len, _)| (len - MIN_MATCH).min(15) as u8);
    out.push(lit_nibble << 4 | match_nibble);
    if literals.len() >= 15 {
        put_length(out, literals.len() - 15);
    }
    out.extend_from_slice(literals);
    if let Some((len, dist)) = m {
        out.extend_from_slice(&(dist as u16).to_le_bytes());
        if len - MIN_MATCH >= 15 {
            put_length(out, len - MIN_MATCH - 15);
        }
    }
}

/// Compress `src` into one LZ4 block (no size header).
pub fn compress_hc(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / 2 + 16);
    let mut chains = Chains::new(src);
    let (mut anchor, mut i) = (0, 0);
    while i < src.len() {
        let Some(first) = chains.best(i, 0) else {
            i += 1;
            continue;
        };
        // Defer by a byte while the next position starts a longer match.
        let (mut at, mut found) = (i, first);
        while let Some(next) = chains.best(at + 1, found.0) {
            at += 1;
            found = next;
        }
        put_sequence(&mut out, &src[anchor..at], Some(found));
        anchor = at + found.0;
        i = anchor;
    }
    put_sequence(&mut out, &src[anchor..], None);
    out
}

/// Decode one LZ4 block that expands to exactly `size` bytes; `None` when the
/// block is malformed or expands to a different size.
pub fn decompress(block: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut i = 0;
    let read_len = |i: &mut usize, mut len: usize| -> Option<usize> {
        if len == 15 {
            loop {
                let b = *block.get(*i)?;
                *i += 1;
                len += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        Some(len)
    };
    loop {
        let token = *block.get(i)?;
        i += 1;
        let lit = read_len(&mut i, (token >> 4) as usize)?;
        out.extend_from_slice(block.get(i..i.checked_add(lit)?)?);
        i += lit;
        if i == block.len() {
            break;
        }
        let dist = u16::from_le_bytes([*block.get(i)?, *block.get(i + 1)?]) as usize;
        i += 2;
        let len = read_len(&mut i, (token & 15) as usize)? + MIN_MATCH;
        if dist == 0 || dist > out.len() || out.len() + len > size {
            return None;
        }
        // Overlapping copies repeat the pattern, so go byte by byte.
        let from = out.len() - dist;
        for k in 0..len {
            let b = out[from + k];
            out.push(b);
        }
    }
    (out.len() == size).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pseudo(n: usize, seed: u32, alphabet: u32) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((s >> 24) % alphabet) as u8
            })
            .collect()
    }

    fn round_trip(data: &[u8]) -> usize {
        let z = compress_hc(data);
        assert_eq!(decompress(&z, data.len()).as_deref(), Some(data), "len {}", data.len());
        z.len()
    }

    #[test]
    fn empty_and_tiny() {
        assert_eq!(compress_hc(&[]), vec![0]);
        for n in 0..40 {
            round_trip(&vec![7u8; n]);
            round_trip(&pseudo(n, n as u32, 256));
        }
    }

    #[test]
    fn repeats_compress() {
        assert!(round_trip(&vec![0u8; 100_000]) < 500);
        let pattern: Vec<u8> = (0..3000).map(|i| (i % 7) as u8).collect();
        assert!(round_trip(&pattern) < 100);
    }

    #[test]
    fn mixed_inputs_round_trip() {
        for (n, seed, alpha) in [(70_000, 1, 4), (70_000, 2, 256), (200_000, 3, 2), (1000, 4, 16)] {
            round_trip(&pseudo(n, seed, alpha));
        }
        // Matches beyond the 64 KB window must not be used.
        let mut far = pseudo(1000, 9, 256);
        far.extend(pseudo(70_000, 10, 256));
        far.extend_from_slice(&far.clone()[..1000]);
        round_trip(&far);
    }

    #[test]
    fn rejects_malformed() {
        assert!(decompress(&[], 0).is_none());
        assert!(decompress(&[0x10], 1).is_none());
        let z = compress_hc(&pseudo(500, 5, 3));
        assert!(decompress(&z, 499).is_none());
        assert!(decompress(&z[..z.len() - 1], 500).is_none());
    }
}
