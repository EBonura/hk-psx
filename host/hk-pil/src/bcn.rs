//! Block-compressed texture decoding for BC1 (DXT1), BC3 (DXT5) and BC7
//! (BPTC), from the formats' published layouts. Each 4x4 block expands to
//! RGBA; a whole image is decoded block by block in row-major order.

use crate::bcn_tables::{ANCHOR2, ANCHOR3, PARTITIONS2, PARTITIONS3};

type Px = [u8; 4];

/// LSB-first bit reader over one 16-byte block.
struct Bits {
    v: u128,
    pos: u32,
}

impl Bits {
    fn new(block: &[u8]) -> Self {
        let mut b = [0u8; 16];
        b.copy_from_slice(&block[..16]);
        Bits { v: u128::from_le_bytes(b), pos: 0 }
    }

    fn take(&mut self, n: u32) -> u32 {
        let out = ((self.v >> self.pos) & ((1u128 << n) - 1)) as u32;
        self.pos += n;
        out
    }
}

/// RGB565 to 8-bit channels, replicating the high bits into the low ones.
fn expand565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) as u32 & 31, (c >> 5) as u32 & 63, c as u32 & 31);
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

/// The four colours of a colour block (eight bytes). With the first endpoint
/// above the second, or when `four_colors` forces it (the colour half of a
/// BC3 block), there are two interpolated thirds; otherwise one midpoint and a
/// transparent black.
fn bc1_palette(block: &[u8], four_colors: bool) -> [Px; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (expand565(c0), expand565(c1));
    let mix = |wa: u32, wb: u32, div: u32| -> Px {
        let f = |i: usize| ((a[i] as u32 * wa + b[i] as u32 * wb) / div) as u8;
        [f(0), f(1), f(2), 255]
    };
    if c0 > c1 || four_colors {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(2, 1, 3), mix(1, 2, 3)]
    } else {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(1, 1, 2), [0, 0, 0, 0]]
    }
}

fn decode_bc1(block: &[u8], four_colors: bool) -> [Px; 16] {
    let palette = bc1_palette(block, four_colors);
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    std::array::from_fn(|i| palette[(indices >> (2 * i)) as usize & 3])
}

/// The eight alpha values of a BC3 alpha block (first eight bytes' endpoints).
fn bc3_alphas(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (a0 as u32, a1 as u32);
    let mut out = [a0, a1, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for k in 1..7 {
            out[k + 1] = (((7 - k as u32) * x + k as u32 * y) / 7) as u8;
        }
    } else {
        for k in 1..5 {
            out[k + 1] = (((5 - k as u32) * x + k as u32 * y) / 5) as u8;
        }
        out[6] = 0;
        out[7] = 255;
    }
    out
}

fn decode_bc3(block: &[u8]) -> [Px; 16] {
    let alphas = bc3_alphas(block[0], block[1]);
    let mut bits = 0u64;
    for (i, &b) in block[2..8].iter().enumerate() {
        bits |= (b as u64) << (8 * i);
    }
    let colors = decode_bc1(&block[8..16], true);
    std::array::from_fn(|i| {
        let mut px = colors[i];
        px[3] = alphas[(bits >> (3 * i)) as usize & 7];
        px
    })
}

/// Interpolation weights (out of 64) for 2-, 3- and 4-bit indices.
fn weight(bits: u32, index: u32) -> u32 {
    const W2: [u32; 4] = [0, 21, 43, 64];
    const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
    const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];
    match bits {
        2 => W2[index as usize],
        3 => W3[index as usize],
        _ => W4[index as usize],
    }
}

fn lerp64(a: u8, b: u8, w: u32) -> u8 {
    ((a as u32 * (64 - w) + b as u32 * w + 32) >> 6) as u8
}

#[derive(Clone, Copy, PartialEq)]
enum PBits {
    None,
    PerEndpoint,
    PerSubset,
}

/// Layout of one BPTC mode.
struct Mode {
    subsets: usize,
    partition_bits: u32,
    rotation: bool,
    index_select: bool,
    color_bits: u32,
    alpha_bits: u32,
    pbits: PBits,
    index_bits: u32,
    /// Bits of the second index set (modes 4 and 5).
    index2_bits: u32,
}

const MODES: [Mode; 8] = [
    Mode { subsets: 3, partition_bits: 4, rotation: false, index_select: false, color_bits: 4, alpha_bits: 0, pbits: PBits::PerEndpoint, index_bits: 3, index2_bits: 0 },
    Mode { subsets: 2, partition_bits: 6, rotation: false, index_select: false, color_bits: 6, alpha_bits: 0, pbits: PBits::PerSubset, index_bits: 3, index2_bits: 0 },
    Mode { subsets: 3, partition_bits: 6, rotation: false, index_select: false, color_bits: 5, alpha_bits: 0, pbits: PBits::None, index_bits: 2, index2_bits: 0 },
    Mode { subsets: 2, partition_bits: 6, rotation: false, index_select: false, color_bits: 7, alpha_bits: 0, pbits: PBits::PerEndpoint, index_bits: 2, index2_bits: 0 },
    Mode { subsets: 1, partition_bits: 0, rotation: true, index_select: true, color_bits: 5, alpha_bits: 6, pbits: PBits::None, index_bits: 2, index2_bits: 3 },
    Mode { subsets: 1, partition_bits: 0, rotation: true, index_select: false, color_bits: 7, alpha_bits: 8, pbits: PBits::None, index_bits: 2, index2_bits: 2 },
    Mode { subsets: 1, partition_bits: 0, rotation: false, index_select: false, color_bits: 7, alpha_bits: 7, pbits: PBits::PerEndpoint, index_bits: 4, index2_bits: 0 },
    Mode { subsets: 2, partition_bits: 6, rotation: false, index_select: false, color_bits: 5, alpha_bits: 5, pbits: PBits::PerEndpoint, index_bits: 2, index2_bits: 0 },
];

/// Widen an `n`-bit value to 8 bits by replicating its high bits.
fn widen(v: u32, n: u32) -> u8 {
    if n >= 8 {
        v as u8
    } else {
        ((v << (8 - n)) | (v >> (2 * n - 8))) as u8
    }
}

/// Which texels hold an anchor (one fewer index bit) for the given partition.
fn anchor_texels(subsets: usize, partition: usize) -> [usize; 3] {
    match subsets {
        1 => [0, usize::MAX, usize::MAX],
        2 => [0, ANCHOR2[partition] as usize, usize::MAX],
        _ => [0, ANCHOR3[partition][0] as usize, ANCHOR3[partition][1] as usize],
    }
}

fn decode_bc7(block: &[u8]) -> [Px; 16] {
    let mut bits = Bits::new(block);
    let mode_number = block[0].trailing_zeros() as usize;
    // No mode bit set: reserved, decoded as opaque black.
    if mode_number >= 8 {
        return [[0, 0, 0, 255]; 16];
    }
    bits.take(mode_number as u32 + 1);
    let m = &MODES[mode_number];
    let partition = bits.take(m.partition_bits) as usize;
    let rotation = if m.rotation { bits.take(2) } else { 0 };
    let index_select = if m.index_select { bits.take(1) == 1 } else { false };

    let endpoints = m.subsets * 2;
    let mut raw = [[0u32; 4]; 6];
    for channel in 0..3 {
        for e in raw.iter_mut().take(endpoints) {
            e[channel] = bits.take(m.color_bits);
        }
    }
    if m.alpha_bits > 0 {
        for e in raw.iter_mut().take(endpoints) {
            e[3] = bits.take(m.alpha_bits);
        }
    }
    let mut pbit = [0u32; 6];
    match m.pbits {
        PBits::None => {}
        PBits::PerEndpoint => {
            for p in pbit.iter_mut().take(endpoints) {
                *p = bits.take(1);
            }
        }
        PBits::PerSubset => {
            for s in 0..m.subsets {
                let p = bits.take(1);
                pbit[s * 2] = p;
                pbit[s * 2 + 1] = p;
            }
        }
    }
    let mut ends = [[0u8; 4]; 6];
    for e in 0..endpoints {
        for c in 0..4 {
            let (n, v) = if c < 3 { (m.color_bits, raw[e][c]) } else { (m.alpha_bits, raw[e][3]) };
            ends[e][c] = if c == 3 && n == 0 {
                255
            } else if m.pbits != PBits::None {
                widen((v << 1) | pbit[e], n + 1)
            } else {
                widen(v, n)
            };
        }
    }

    let subset_of = |texel: usize| -> usize {
        match m.subsets {
            1 => 0,
            2 => PARTITIONS2[partition][texel] as usize,
            _ => PARTITIONS3[partition][texel] as usize,
        }
    };
    let anchors = anchor_texels(m.subsets, partition);
    let is_anchor = |texel: usize| -> bool { anchors.contains(&texel) };
    let mut read_indices = |n: u32| -> [u32; 16] {
        std::array::from_fn(|t| bits.take(if is_anchor(t) { n - 1 } else { n }))
    };
    let first = read_indices(m.index_bits);
    let second = if m.index2_bits > 0 { Some(read_indices(m.index2_bits)) } else { None };

    std::array::from_fn(|t| {
        let s = subset_of(t);
        let (lo, hi) = (ends[s * 2], ends[s * 2 + 1]);
        // Choose which index set drives colour and which drives alpha.
        let (color_idx, color_bits, alpha_idx, alpha_bits) = match second {
            None => (first[t], m.index_bits, first[t], m.index_bits),
            Some(second) => {
                if index_select {
                    (second[t], m.index2_bits, first[t], m.index_bits)
                } else {
                    (first[t], m.index_bits, second[t], m.index2_bits)
                }
            }
        };
        let (wc, wa) = (weight(color_bits, color_idx), weight(alpha_bits, alpha_idx));
        let mut px = [lerp64(lo[0], hi[0], wc), lerp64(lo[1], hi[1], wc), lerp64(lo[2], hi[2], wc), lerp64(lo[3], hi[3], wa)];
        if rotation != 0 {
            px.swap(3, rotation as usize - 1);
        }
        px
    })
}

/// Decode `n` (1, 3 or 7) blocks into a `width` x `height` RGBA buffer whose
/// sides are multiples of 4. `None` when the data runs out first.
pub fn decode(n: u8, data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let size = if n == 1 { 8 } else { 16 };
    let (bw, bh) = (width / 4, height / 4);
    if data.len() < bw * bh * size {
        return None;
    }
    let mut out = vec![0u8; width * height * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let block = &data[(by * bw + bx) * size..][..size];
            let texels = match n {
                1 => decode_bc1(block, false),
                3 => decode_bc3(block),
                _ => decode_bc7(block),
            };
            for (i, px) in texels.iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                out[(y * width + x) * 4..][..4].copy_from_slice(px);
            }
        }
    }
    Some(out)
}
