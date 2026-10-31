//! The `CODED` coder of an art bank (shared/hk-format `boss.rs`, module `code`): a part's pixels as a stream of tokens (single
//! pixels, runs of transparent pixels, runs of copies of the previous pixel or of the row above), each coded with a static
//! Huffman code chosen by the pixel before it, one set of codes for the whole bank. The format is documented where the decoder
//! is. This file is the encoder and nothing else; it has no dependency on the rest of the cooker.
use hk_format::boss::code::{CONTEXTS, LEFT_RUN, MAX_LEN, SHORT_BITS, SYMBOLS, TABLE_HEAD, UP_RUN, ZERO_RUN};
use std::collections::BinaryHeap;



/// One part to code: its size in pixels and its texel bytes, two pixels a byte, rows padded to `Part::stride`.
pub struct Plane<'a> {
    pub width: usize,
    pub height: usize,
    pub texels: &'a [u8],
}
impl Plane<'_> {
    /// Pixels in a padded row.
    pub fn row(&self) -> usize {
        hk_format::boss::Part::stride(self.width) * 2
    }
    fn pixels(&self) -> Vec<u8> {
        let row = self.row();
        (0..row * self.height).map(|i| (self.texels[i >> 1] >> ((i & 1) * 4)) & 15).collect()
    }
}

/// The coded bank: the table to store after the part table and each plane's stream.
pub struct Encoded {
    pub table: Vec<u8>,
    pub streams: Vec<Vec<u8>>,
}

/// Which tokens the coder may use. `up_runs` adds the copies of the row above.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub up_runs: bool,
}
impl Default for Options {
    fn default() -> Self {
        Options { up_runs: true }
    }
}

#[derive(Clone, Copy)]
struct Token {
    context: u8,
    symbol: u8,
    extra: u16,
    extra_bits: u8,
}

fn bucket(run: usize) -> usize {
    (usize::BITS - 1 - run.leading_zeros()) as usize
}

fn run_token(context: usize, first: usize, run: usize, shift: usize) -> Token {
    let k = bucket(run);
    Token { context: context as u8, symbol: (first + k - shift) as u8, extra: (run - (1 << k)) as u16, extra_bits: k as u8 }
}

fn tokens(px: &[u8], row: usize, options: Options) -> Vec<Token> {
    let total = px.len();
    let mut out = vec![];
    let (mut i, mut left) = (0usize, 0usize);
    while i < total {
        let v = px[i] as usize;
        let same = |value: u8| px[i..].iter().take_while(|&&p| p == value).count();
        let zeros = if v == 0 { same(0) } else { 0 };
        let copies = if v != 0 && v == left { same(v as u8) } else { 0 };
        let ups = if options.up_runs && i >= row && px[i] == px[i - row] {
            (0..total - i).take_while(|&k| px[i + k] == px[i + k - row]).count()
        } else {
            0
        };
        let best = zeros.max(copies).max(ups);
        if best >= 2 || zeros == 1 {
            let (token, next_left) = if zeros == best {
                (run_token(left, ZERO_RUN, best, 0), 0)
            } else if copies == best {
                (run_token(left, LEFT_RUN, best, 1), left)
            } else {
                (run_token(left, UP_RUN, best, 1), px[i + best - 1] as usize)
            };
            out.push(token);
            left = next_left;
            i += best;
        } else {
            out.push(Token { context: left as u8, symbol: (v - 1) as u8, extra: 0, extra_bits: 0 });
            left = v;
            i += 1;
        }
    }
    out
}

/// Code lengths of `counts` (0 for an unused symbol), none longer than `MAX_LEN`.
fn lengths(counts: &[u64; SYMBOLS]) -> [u8; SYMBOLS] {
    let mut counts = *counts;
    loop {
        let used: Vec<usize> = (0..SYMBOLS).filter(|&s| counts[s] > 0).collect();
        let mut out = [0u8; SYMBOLS];
        if used.len() == 1 {
            out[used[0]] = 1;
            return out;
        }
        // Nodes 0..SYMBOLS are the symbols; merges are numbered after them. Ties break on the node number so the result is
        // the same on every run.
        let mut parent = vec![usize::MAX; 2 * SYMBOLS];
        let mut heap = BinaryHeap::new();
        for &s in &used {
            heap.push(std::cmp::Reverse((counts[s], s)));
        }
        let mut next = SYMBOLS;
        while heap.len() > 1 {
            let std::cmp::Reverse((a, x)) = heap.pop().unwrap();
            let std::cmp::Reverse((b, y)) = heap.pop().unwrap();
            parent[x] = next;
            parent[y] = next;
            heap.push(std::cmp::Reverse((a + b, next)));
            next += 1;
        }
        for &s in &used {
            let (mut depth, mut node) = (0, s);
            while parent[node] != usize::MAX {
                node = parent[node];
                depth += 1;
            }
            out[s] = depth;
        }
        if out.iter().all(|&l| l as usize <= MAX_LEN) {
            return out;
        }
        // Too long: flatten the counts and build again.
        for c in counts.iter_mut().filter(|c| **c > 0) {
            *c = (*c).div_ceil(2);
        }
    }
}

/// The canonical code: for each symbol its code (right aligned) and length, and the record stored in the bank (the number of
/// codes of each length, then the symbols in code order; see shared/hk-format `boss.rs`, module `code`).
fn canonical(lens: &[u8; SYMBOLS]) -> ([(u16, u8); SYMBOLS], Vec<u8>) {
    let mut order: Vec<usize> = (0..SYMBOLS).filter(|&s| lens[s] > 0).collect();
    order.sort_by_key(|&s| (lens[s], s));
    let mut record = vec![0u8; MAX_LEN];
    for &s in &order {
        record[lens[s] as usize - 1] += 1;
    }
    let mut codes = [(0u16, 0u8); SYMBOLS];
    let mut short = [0xffu8; 1 << SHORT_BITS];
    let (mut code, mut at) = (0u32, 0usize);
    for len in 1..=MAX_LEN {
        let count = record[len - 1] as usize;
        for &s in &order[at..at + count] {
            codes[s] = (code as u16, len as u8);
            if len <= SHORT_BITS {
                let (from, to) = ((code as usize) << (SHORT_BITS - len), (code as usize + 1) << (SHORT_BITS - len));
                short[from..to].fill(s as u8 | (len as u8 - 1) << 6);
            }
            code += 1;
        }
        at += count;
        code <<= 1;
    }
    record.extend(short);
    record.extend(order.iter().map(|&s| s as u8));
    (codes, record)
}

struct BitWriter {
    bytes: Vec<u8>,
    acc: u32,
    count: u32,
}
impl BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        for b in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((value >> b) & 1);
            self.count += 1;
            if self.count == 8 {
                self.bytes.push(self.acc as u8);
                self.acc = 0;
                self.count = 0;
            }
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.bytes.push((self.acc << (8 - self.count)) as u8);
        }
        self.bytes
    }
}

/// How many tokens `plane` takes and how many of them are literals (for measurements).
pub fn token_counts(plane: &Plane, options: Options) -> (usize, usize) {
    let t = tokens(&plane.pixels(), plane.row(), options);
    (t.len(), t.iter().filter(|t| (t.symbol as usize) < ZERO_RUN).count())
}

/// Code `planes` with one table. An empty plane list gives an empty table.
pub fn encode(planes: &[Plane], options: Options) -> Encoded {
    let streams_of: Vec<Vec<Token>> = planes.iter().map(|p| tokens(&p.pixels(), p.row(), options)).collect();
    let mut counts = vec![[0u64; SYMBOLS]; CONTEXTS];
    for t in streams_of.iter().flatten() {
        counts[t.context as usize][t.symbol as usize] += 1;
    }
    let mut codes = vec![];
    let mut records = vec![];
    for c in &counts {
        let (code, record) = canonical(&lengths(c));
        codes.push(code);
        records.push(record);
    }
    let mut table = vec![0u8; TABLE_HEAD];
    for (c, record) in records.iter().enumerate() {
        let at = table.len() as u16;
        table[2 + 2 * c..4 + 2 * c].copy_from_slice(&at.to_le_bytes());
        table.extend(record);
    }
    let length = table.len() as u16;
    table[..2].copy_from_slice(&length.to_le_bytes());
    let streams = streams_of
        .iter()
        .map(|stream| {
            let mut w = BitWriter { bytes: vec![], acc: 0, count: 0 };
            for t in stream {
                let (code, len) = codes[t.context as usize][t.symbol as usize];
                w.put(code as u32, len as u32);
                w.put(t.extra as u32, t.extra_bits as u32);
            }
            w.finish()
        })
        .collect();
    Encoded { table, streams }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hk_format::boss::{decode_part, Part, CODED};

    /// A plane from pixel rows: texels packed two a byte, rows padded.
    fn pack(width: usize, rows: &[Vec<u8>]) -> Vec<u8> {
        let stride = Part::stride(width);
        let mut out = vec![0u8; stride * rows.len()];
        for (y, row) in rows.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                out[y * stride + x / 2] |= v << ((x & 1) * 4);
            }
        }
        out
    }

    /// Pixel rows with the kinds of structure sprites have: empty margins, flat fills, an outline, a row repeated.
    fn sprite(width: usize, height: usize, seed: usize) -> Vec<Vec<u8>> {
        let mut state = (seed as u32).wrapping_mul(2654435761).wrapping_add(12345);
        let mut next = move || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            (state >> 16) as usize
        };
        let mut rows: Vec<Vec<u8>> = vec![];
        for y in 0..height {
            if y > 2 && next() % 3 == 0 {
                rows.push(rows[y - 1].clone());
                continue;
            }
            let mut row = vec![0u8; width];
            let half = (width / 2).max(1);
            let (a, b) = (next() % half, (width / 2 + next() % half).min(width));
            for x in a..b {
                row[x] = match x - a {
                    0 => 1,
                    n if n + 1 == b - a => 1,
                    _ => 2 + (x / 7 + y / 5) as u8 % 3,
                };
                if next() % 17 == 0 {
                    row[x] = 1 + (next() % 15) as u8;
                }
            }
            rows.push(row);
        }
        rows
    }

    fn round_trip(options: Options) {
        let shapes = [(64, 64), (37, 11), (5, 5), (1, 1), (3, 40), (64, 3), (8, 8)];
        let texels: Vec<Vec<u8>> = shapes.iter().enumerate().map(|(i, &(w, h))| pack(w, &sprite(w, h, i + 1))).collect();
        let planes: Vec<Plane> = shapes.iter().zip(&texels).map(|(&(w, h), t)| Plane { width: w, height: h, texels: t }).collect();
        let coded = encode(&planes, options);
        // The table, then each stream, as a bank lays them out (the part table is not needed to decode).
        let mut blob = coded.table.clone();
        let mut parts = vec![];
        for (i, s) in coded.streams.iter().enumerate() {
            parts.push(Part { x: 0, y: 0, width: shapes[i].0 as u8, height: shapes[i].1 as u8, coded: s.len() as u16 | CODED, sprite: 0, offset: blob.len() as u32 });
            blob.extend(s);
        }
        let blob = blob.as_slice();
        for (i, part) in parts.iter().enumerate() {
            let mut out = vec![0xaa; part.texel_bytes()];
            decode_part(&blob, 0, part, &mut out).unwrap();
            assert_eq!(out, texels[i], "plane {i} {options:?}");
        }
    }

    #[test]
    fn coded_parts_decode_to_the_texels_they_came_from() {
        round_trip(Options { up_runs: true });
        round_trip(Options { up_runs: false });
    }

    #[test]
    fn a_plane_of_one_colour_and_an_empty_one_survive() {
        let flat = vec![vec![7u8; 16]; 16];
        let empty = vec![vec![0u8; 16]; 16];
        let (a, b) = (pack(16, &flat), pack(16, &empty));
        let planes = [Plane { width: 16, height: 16, texels: &a }, Plane { width: 16, height: 16, texels: &b }];
        let coded = encode(&planes, Options::default());
        let mut blob = coded.table.clone();
        let mut offsets = vec![];
        for s in &coded.streams {
            offsets.push(blob.len() as u32);
            blob.extend(s);
        }
        for (i, want) in [&a, &b].into_iter().enumerate() {
            let part = Part { x: 0, y: 0, width: 16, height: 16, coded: coded.streams[i].len() as u16 | CODED, sprite: 0, offset: offsets[i] };
            let mut out = vec![0xaa; part.texel_bytes()];
            decode_part(&blob.as_slice(), 0, &part, &mut out).unwrap();
            assert_eq!(&out, want);
        }
    }

    #[test]
    fn code_lengths_stay_within_the_limit() {
        // A geometric spread of counts needs 20 bits for the rarest symbol unflattened.
        let mut counts = [0u64; SYMBOLS];
        for (s, c) in counts.iter_mut().enumerate().take(24) {
            *c = 1u64 << (23 - s).min(40);
        }
        let lens = lengths(&counts);
        assert!(lens.iter().all(|&l| l as usize <= MAX_LEN));
        let kraft: f64 = lens.iter().filter(|&&l| l > 0).map(|&l| 0.5f64.powi(l as i32)).sum();
        assert!(kraft <= 1.0 + 1e-12);
    }
}
