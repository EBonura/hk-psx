//! Pillow's BCn decoder (libImaging/BcnDecode.c, CC0) for BC1 (DXT1),
//! BC3 (DXT5) and BC7, decoding whole 4x4 blocks row by row into RGBA.

type Px = [u8; 4];

fn decode_565(x: u16) -> Px {
    let x = x as u32;
    let mut r = (x & 0xf800) >> 8;
    r |= r >> 5;
    let mut g = (x & 0x7e0) >> 3;
    g |= g >> 6;
    let mut b = (x & 0x1f) << 3;
    b |= b >> 5;
    [r as u8, g as u8, b as u8, 0xff]
}

fn bc1_color(dst: &mut [Px; 16], src: &[u8], separate_alpha: bool) {
    let c0 = src[0] as u16 | (src[1] as u16) << 8;
    let c1 = src[2] as u16 | (src[3] as u16) << 8;
    let mut p = [decode_565(c0), decode_565(c1), [0; 4], [0; 4]];
    let (r0, g0, b0) = (p[0][0] as u32, p[0][1] as u32, p[0][2] as u32);
    let (r1, g1, b1) = (p[1][0] as u32, p[1][1] as u32, p[1][2] as u32);
    if c0 > c1 || separate_alpha {
        p[2] = [((2 * r0 + r1) / 3) as u8, ((2 * g0 + g1) / 3) as u8, ((2 * b0 + b1) / 3) as u8, 0xff];
        p[3] = [((r0 + 2 * r1) / 3) as u8, ((g0 + 2 * g1) / 3) as u8, ((b0 + 2 * b1) / 3) as u8, 0xff];
    } else {
        p[2] = [((r0 + r1) / 2) as u8, ((g0 + g1) / 2) as u8, ((b0 + b1) / 2) as u8, 0xff];
        p[3] = [0, 0, 0, 0];
    }
    for n in 0..4 {
        for o in 0..4 {
            let cw = 3 & (src[4 + n] >> (2 * o));
            dst[n * 4 + o] = p[cw as usize];
        }
    }
}

fn bc3_alpha(dst: &mut [Px; 16], src: &[u8]) {
    let (a0, a1) = (src[0] as u32, src[1] as u32);
    let lut1 = src[2] as u32 | (src[3] as u32) << 8 | (src[4] as u32) << 16;
    let lut2 = src[5] as u32 | (src[6] as u32) << 8 | (src[7] as u32) << 16;
    let mut a = [a0 as u8, a1 as u8, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for k in 0..6u32 {
            a[2 + k as usize] = (((6 - k) * a0 + (1 + k) * a1) / 7) as u8;
        }
    } else {
        for k in 0..4u32 {
            a[2 + k as usize] = (((4 - k) * a0 + (1 + k) * a1) / 5) as u8;
        }
        a[6] = 0;
        a[7] = 0xff;
    }
    for n in 0..8 {
        dst[n][3] = a[(7 & (lut1 >> (3 * n))) as usize];
        dst[8 + n][3] = a[(7 & (lut2 >> (3 * n))) as usize];
    }
}

fn get_bits(src: &[u8], bit: usize, count: usize) -> u8 {
    if count == 0 {
        return 0;
    }
    let by = bit >> 3;
    let bit = bit & 7;
    if bit + count <= 8 {
        (src[by] >> bit) & ((1u16 << count) - 1) as u8
    } else {
        let x = src[by] as u32 | (src.get(by + 1).copied().unwrap_or(0) as u32) << 8;
        ((x >> bit) & ((1 << count) - 1)) as u8
    }
}

// ns, pb, rb, isb, cb, ab, epb, spb, ib, ib2
const MODES: [[usize; 10]; 8] = [
    [3, 4, 0, 0, 4, 0, 1, 0, 3, 0],
    [2, 6, 0, 0, 6, 0, 0, 1, 3, 0],
    [3, 6, 0, 0, 5, 0, 0, 0, 2, 0],
    [2, 6, 0, 0, 7, 0, 1, 0, 2, 0],
    [1, 0, 2, 1, 5, 6, 0, 0, 2, 3],
    [1, 0, 2, 0, 7, 8, 0, 0, 2, 2],
    [1, 0, 0, 0, 7, 7, 1, 0, 4, 0],
    [2, 6, 0, 0, 5, 5, 1, 0, 2, 0],
];

const SI2: [u16; 64] = [
    0xcccc, 0x8888, 0xeeee, 0xecc8, 0xc880, 0xfeec, 0xfec8, 0xec80, 0xc800, 0xffec, 0xfe80, 0xe800, 0xffe8, 0xff00, 0xfff0, 0xf000,
    0xf710, 0x008e, 0x7100, 0x08ce, 0x008c, 0x7310, 0x3100, 0x8cce, 0x088c, 0x3110, 0x6666, 0x366c, 0x17e8, 0x0ff0, 0x718e, 0x399c,
    0xaaaa, 0xf0f0, 0x5a5a, 0x33cc, 0x3c3c, 0x55aa, 0x9696, 0xa55a, 0x73ce, 0x13c8, 0x324c, 0x3bdc, 0x6996, 0xc33c, 0x9966, 0x0660,
    0x0272, 0x04e4, 0x4e40, 0x2720, 0xc936, 0x936c, 0x39c6, 0x639c, 0x9336, 0x9cc6, 0x817e, 0xe718, 0xccf0, 0x0fcc, 0x7744, 0xee22,
];
const SI3: [u32; 64] = [
    0xaa685050, 0x6a5a5040, 0x5a5a4200, 0x5450a0a8, 0xa5a50000, 0xa0a05050, 0x5555a0a0, 0x5a5a5050, 0xaa550000, 0xaa555500,
    0xaaaa5500, 0x90909090, 0x94949494, 0xa4a4a4a4, 0xa9a59450, 0x2a0a4250, 0xa5945040, 0x0a425054, 0xa5a5a500, 0x55a0a0a0,
    0xa8a85454, 0x6a6a4040, 0xa4a45000, 0x1a1a0500, 0x0050a4a4, 0xaaa59090, 0x14696914, 0x69691400, 0xa08585a0, 0xaa821414,
    0x50a4a450, 0x6a5a0200, 0xa9a58000, 0x5090a0a8, 0xa8a09050, 0x24242424, 0x00aa5500, 0x24924924, 0x24499224, 0x50a50a50,
    0x500aa550, 0xaaaa4444, 0x66660000, 0xa5a0a5a0, 0x50a050a0, 0x69286928, 0x44aaaa44, 0x66666600, 0xaa444444, 0x54a854a8,
    0x95809580, 0x96969600, 0xa85454a8, 0x80959580, 0xaa141414, 0x96960000, 0xaaaa1414, 0xa05050a0, 0xa0a5a5a0, 0x96000000,
    0x40804080, 0xa9a8a9a8, 0xaaaaaa44, 0x2a4a5254,
];
const AI0: [usize; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8, 2, 2, 8, 8, 2, 2, 15, 15, 6, 8,
    2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2, 2, 15, 15, 15, 15, 15, 2, 2, 15,
];
const AI1: [usize; 64] = [
    3, 3, 15, 15, 8, 3, 15, 15, 8, 8, 6, 6, 6, 5, 3, 3, 3, 3, 8, 15, 3, 3, 6, 10, 5, 8, 8, 6, 8, 5, 15, 15, 8, 15, 3, 5, 6, 10, 8,
    15, 15, 3, 15, 5, 15, 15, 15, 15, 3, 15, 5, 5, 5, 8, 5, 10, 5, 10, 8, 13, 15, 12, 3, 3,
];
const AI2: [usize; 64] = [
    15, 8, 8, 3, 15, 15, 3, 8, 15, 15, 15, 15, 15, 15, 15, 8, 15, 8, 15, 3, 15, 8, 15, 8, 3, 15, 6, 10, 15, 15, 10, 8, 15, 3, 15,
    10, 10, 8, 9, 10, 6, 15, 8, 15, 3, 6, 6, 8, 15, 3, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 3, 15, 15, 8,
];
const W2: [u32; 4] = [0, 21, 43, 64];
const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weights(n: usize) -> &'static [u32] {
    match n {
        2 => &W2,
        3 => &W3,
        _ => &W4,
    }
}

fn subset(ns: usize, partition: usize, n: usize) -> usize {
    match ns {
        2 => (1 & (SI2[partition] >> n)) as usize,
        3 => (3 & (SI3[partition] >> (2 * n))) as usize,
        _ => 0,
    }
}

fn expand(v: u8, bits: usize) -> u8 {
    let v = ((v as u32) << (8 - bits)) as u8;
    v | (v >> bits)
}

fn lerp(e0: Px, e1: Px, s0: u32, s1: u32) -> Px {
    let (t0, t1) = (64 - s0, 64 - s1);
    let c = |i: usize, t: u32, s: u32| ((t * e0[i] as u32 + s * e1[i] as u32 + 32) >> 6) as u8;
    [c(0, t0, s0), c(1, t0, s0), c(2, t0, s0), c(3, t1, s1)]
}

fn bc7(col: &mut [Px; 16], src: &[u8]) {
    let first = src[0];
    if first == 0 {
        for p in col.iter_mut() {
            *p = [0, 0, 0, 255];
        }
        return;
    }
    let mode = first.trailing_zeros() as usize;
    let mut bit = mode + 1;
    let [ns, pb, rb, isb, mut cb, mut ab, epb, spb, ib0, ib20] = MODES[mode];
    let cw = weights(ib0);
    let aw = weights(if ab != 0 && ib20 != 0 { ib20 } else { ib0 });
    let mut load = |n: usize| {
        let v = get_bits(src, bit, n);
        bit += n;
        v
    };
    let partition = load(pb) as usize;
    let rotation = load(rb);
    let index_sel = load(isb);
    let numep = ns << 1;
    let mut ep = [[0u8; 4]; 6];
    for c in 0..3 {
        for e in ep.iter_mut().take(numep) {
            e[c] = load(cb);
        }
    }
    for e in ep.iter_mut().take(numep) {
        e[3] = if ab != 0 { load(ab) } else { 255 };
    }
    let assign = |x: &mut u8, v: u8| *x = (((*x as u32) << 1) | v as u32) as u8;
    if epb != 0 {
        cb += 1;
        if ab != 0 {
            ab += 1;
        }
        for e in ep.iter_mut().take(numep) {
            let v = load(1);
            for c in e.iter_mut().take(3) {
                assign(c, v);
            }
            if ab != 0 {
                assign(&mut e[3], v);
            }
        }
    }
    if spb != 0 {
        cb += 1;
        if ab != 0 {
            ab += 1;
        }
        for i in (0..numep).step_by(2) {
            let v = load(1);
            for e in ep.iter_mut().skip(i).take(2) {
                for c in e.iter_mut().take(3) {
                    assign(c, v);
                }
                if ab != 0 {
                    assign(&mut e[3], v);
                }
            }
        }
    }
    for e in ep.iter_mut().take(numep) {
        for c in e.iter_mut().take(3) {
            *c = expand(*c, cb);
        }
        if ab != 0 {
            e[3] = expand(e[3], ab);
        }
    }
    let mut cibit = bit;
    let mut aibit = cibit + 16 * ib0 - ns;
    for (i, out) in col.iter_mut().enumerate() {
        let s = subset(ns, partition, i) << 1;
        let mut ib = ib0;
        if i == 0 || (ns == 2 && i == AI0[partition]) || (ns == 3 && (i == AI1[partition] || i == AI2[partition])) {
            ib -= 1;
        }
        let i0 = get_bits(src, cibit, ib) as usize;
        cibit += ib;
        let px = if ab != 0 && ib20 != 0 {
            let ib2 = if i == 0 { ib20 - 1 } else { ib20 };
            let i1 = get_bits(src, aibit, ib2) as usize;
            aibit += ib2;
            if index_sel != 0 {
                lerp(ep[s], ep[s + 1], aw[i1], cw[i0])
            } else {
                lerp(ep[s], ep[s + 1], cw[i0], aw[i1])
            }
        } else {
            lerp(ep[s], ep[s + 1], cw[i0], cw[i0])
        };
        let mut px = px;
        match rotation {
            1 => px.swap(0, 3),
            2 => px.swap(1, 3),
            3 => px.swap(2, 3),
            _ => {}
        }
        *out = px;
    }
}

/// Decode `n` (1, 3 or 7) blocks into a `width` x `height` RGBA buffer whose
/// sides are multiples of 4, as Image.frombuffer(mode, size, data, "bcn", n) does.
/// None when the data runs out first ("not enough image data").
pub fn decode(n: u8, data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let mut out = vec![0u8; width * height * 4];
    let block = if n == 1 { 8 } else { 16 };
    let (mut x, mut y) = (0, 0);
    for src in data.chunks_exact(block) {
        if y >= height {
            break;
        }
        let mut col = [[0u8; 4]; 16];
        match n {
            1 => bc1_color(&mut col, src, false),
            3 => {
                bc1_color(&mut col, &src[8..], true);
                bc3_alpha(&mut col, src);
            }
            7 => bc7(&mut col, src),
            _ => panic!("bcn mode {n} is not ported"),
        }
        for j in 0..4 {
            for i in 0..4 {
                let at = ((y + j) * width + x + i) * 4;
                out[at..at + 4].copy_from_slice(&col[j * 4 + i]);
            }
        }
        x += 4;
        if x >= width {
            x = 0;
            y += 4;
        }
    }
    (y >= height).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn bc1_four_colour_block() {
        // c0 = pure red (0xf800) > c1 = pure blue (0x001f); indices 0,1,2,3 on row 0.
        let block = [0x00, 0xf8, 0x1f, 0x00, 0b11_10_01_00, 0, 0, 0];
        let px = decode(1, &block, 4, 4).unwrap();
        assert_eq!(&px[0..4], &[255, 0, 0, 255]);
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
        assert_eq!(&px[8..12], &[170, 0, 85, 255]); // (2*255+0)/3, (2*0+255)/3
        assert_eq!(&px[12..16], &[85, 0, 170, 255]);
        assert!(decode(1, &block[..4], 4, 4).is_none());
    }

    #[test]
    fn bc7_all_zero_mode_byte_is_opaque_black() {
        let px = decode(7, &[0u8; 16], 4, 4).unwrap();
        assert!(px.chunks(4).all(|p| p == [0, 0, 0, 255]));
    }
}
