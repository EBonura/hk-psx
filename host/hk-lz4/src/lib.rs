//! LZ4 block compression and decompression, written from the published LZ4
//! block format description (one block, no frame, no dictionary).
//!
//! The compressor is a shortest-path parser of our own. A binary search tree
//! of suffixes, one tree per hash of the next four bytes, yields the longest
//! earlier match at every position; since a match costs the same bytes at any
//! distance, a dynamic program over positions then picks the cheapest cut of
//! the input into literals and matches.
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
/// Tree nodes visited per position at most.
const SEARCH_DEPTH: usize = 1024;
/// Matches this long are taken whole, and the tree stops comparing here.
const NICE_LEN: usize = 256;
const HASH_BITS: u32 = 16;
const NIL: u32 = u32::MAX;

fn read32(src: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([src[i], src[i + 1], src[i + 2], src[i + 3]])
}

fn hash4(v: u32) -> usize {
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// Suffix trees over the input. Every indexed position is a node whose left
/// subtree holds the earlier suffixes that sort below it and whose right
/// subtree holds those above; inserting a position walks the tree, which
/// meets the longest matching earlier suffix on the way down.
struct Matcher<'a> {
    src: &'a [u8],
    /// Root of the tree for each hash.
    head: Vec<u32>,
    /// `kids[2 * p]` and `kids[2 * p + 1]`: left and right child of `p`.
    kids: Vec<u32>,
    /// One past the last position where a match may start.
    match_end: usize,
}

impl<'a> Matcher<'a> {
    fn new(src: &'a [u8]) -> Self {
        let match_end = if src.len() > LAST_MATCH_START { src.len() - LAST_MATCH_START + 1 } else { 0 };
        Matcher { src, head: vec![NIL; 1 << HASH_BITS], kids: vec![NIL; 2 * src.len()], match_end }
    }

    /// Index position `cur` (positions must come in order) and return the
    /// longest match behind it as (length, distance), the nearest one on a
    /// tie. Lengths are exact.
    fn insert(&mut self, cur: usize) -> Option<(usize, usize)> {
        if cur >= self.match_end {
            return None;
        }
        let src = self.src;
        let room = src.len() - TRAILING_LITERALS - cur;
        let cap = room.min(NICE_LEN);
        let h = hash4(read32(src, cur));
        let mut cand = self.head[h];
        self.head[h] = cur as u32;
        // The slots waiting for the root of everything below / above `cur`.
        let (mut below, mut above) = (2 * cur, 2 * cur + 1);
        let (mut below_len, mut above_len) = (0, 0);
        let mut best = (0, 0);
        for _ in 0..SEARCH_DEPTH {
            if cand == NIL || cur - cand as usize > MAX_OFFSET {
                break;
            }
            let c = cand as usize;
            let mut len = below_len.min(above_len);
            while len < cap && src[c + len] == src[cur + len] {
                len += 1;
            }
            if len > best.0 {
                best = (len, cur - c);
            }
            if len >= cap {
                // `cur` takes this node's place: its children become ours.
                self.kids[below] = self.kids[2 * c];
                self.kids[above] = self.kids[2 * c + 1];
                return self.finish(cur, best, room);
            }
            if src[c + len] < src[cur + len] {
                self.kids[below] = cand;
                below = 2 * c + 1;
                below_len = len;
                cand = self.kids[below];
            } else {
                self.kids[above] = cand;
                above = 2 * c;
                above_len = len;
                cand = self.kids[above];
            }
        }
        self.kids[below] = NIL;
        self.kids[above] = NIL;
        self.finish(cur, best, room)
    }

    /// Settle the reported match: below `NICE_LEN` it is exact already, at
    /// `NICE_LEN` it is measured out to its real end.
    fn finish(&self, cur: usize, (mut len, dist): (usize, usize), room: usize) -> Option<(usize, usize)> {
        if len < MIN_MATCH {
            return None;
        }
        if len >= NICE_LEN {
            while len < room && self.src[cur - dist + len] == self.src[cur + len] {
                len += 1;
            }
        }
        Some((len, dist))
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

/// Bytes a length field costs beyond its nibble: nothing below 15, then one
/// byte per 255 more (a final byte under 255 ends the field).
fn extra_bytes(n: usize) -> usize {
    if n < 15 { 0 } else { (n - 15) / 255 + 1 }
}

const INF: u32 = u32::MAX / 2;

/// How a position was reached by a match: where it started, how long it was
/// and how far back it copied.
#[derive(Clone, Copy)]
struct Arrival {
    start: u32,
    len: u32,
    dist: u32,
}

/// Compress `src` into one LZ4 block (no size header).
///
/// A match costs the same bytes whatever its distance, so for each position
/// only the longest match matters, and every shorter cut of it is usable too.
/// The parse is a shortest path over positions: `reach[i]` is the cheapest
/// byte count of a block prefix that ends with a match finishing at `i`, and
/// `open[i]` the cheapest prefix that has arrived at `i` and may start a
/// match there (the bytes since the last match being literals). Costs leave
/// out the final sequence's token.
pub fn compress_hc(src: &[u8]) -> Vec<u8> {
    let n = src.len();
    let mut matcher = Matcher::new(src);
    let mut reach = vec![INF; n + 1];
    let mut came = vec![Arrival { start: 0, len: 0, dist: 0 }; n + 1];
    let mut open = vec![INF; n + 1];
    // Literal run that `open[i]` ends with (zero when it comes from a match).
    let mut run = vec![0u32; n + 1];
    reach[0] = 0;
    // Matches taken whole leave their interior without match starts.
    let mut taken_until = 0;
    for i in 0..=n {
        open[i] = reach[i];
        if i > 0 {
            let r = run[i - 1] + 1;
            let step = 1 + (r >= 15 && (r - 15) % 255 == 0) as u32;
            if open[i - 1] + step < open[i] {
                open[i] = open[i - 1] + step;
                run[i] = r;
            }
        }
        // Every position goes into the trees, whether or not it can start a match.
        let found = if i < n { matcher.insert(i) } else { None };
        if open[i] >= INF || i < taken_until {
            continue;
        }
        let Some((len, dist)) = found else { continue };
        if len >= NICE_LEN {
            taken_until = i + len;
        }
        let base = open[i] + 3;
        let shortest = if len >= NICE_LEN { len } else { MIN_MATCH };
        for l in shortest..=len {
            let cost = base + extra_bytes(l - MIN_MATCH) as u32;
            if cost < reach[i + l] {
                reach[i + l] = cost;
                came[i + l] = Arrival { start: i as u32, len: l as u32, dist: dist as u32 };
            }
        }
    }
    // Walk back from the end collecting the matches and their literal runs.
    let mut seqs: Vec<(usize, Arrival)> = Vec::new();
    let mut at = n - run[n] as usize;
    while at > 0 {
        let a = came[at];
        let before = a.start as usize - run[a.start as usize] as usize;
        seqs.push((before, a));
        at = before;
    }
    let mut out = Vec::with_capacity(n / 2 + 16);
    let mut anchor = 0;
    for &(before, a) in seqs.iter().rev() {
        put_sequence(&mut out, &src[before..a.start as usize], Some((a.len as usize, a.dist as usize)));
        anchor = a.start as usize + a.len as usize;
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
