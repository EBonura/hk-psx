//! Deterministic disjoint covers of visible PSX 4bpp texels; no pixel edits. Ported from
//! host/alpha_covers.py (the per-process disk cache is not carried over: a cover is a pure
//! function of the texel words).

use crate::common::{err, Result};

pub const HAS_ALPHA_COVERS: u32 = 1;
pub const RECORD_BYTES: usize = 20;
const MAX_RECTS: usize = 4;

type Rect = (usize, usize, usize, usize);

/// A row of up to 256 columns as bits.
type Row = [u64; 4];

fn bits_between(left: usize, right: usize) -> Row {
    let mut out = [0u64; 4];
    for x in left..right {
        out[x / 64] |= 1 << (x % 64);
    }
    out
}

fn and(a: &Row, b: &Row) -> Row {
    [a[0] & b[0], a[1] & b[1], a[2] & b[2], a[3] & b[3]]
}

fn is_zero(a: &Row) -> bool {
    a.iter().all(|&w| w == 0)
}

fn lowest(a: &Row) -> usize {
    for (i, w) in a.iter().enumerate() {
        if *w != 0 {
            return i * 64 + w.trailing_zeros() as usize;
        }
    }
    0
}

fn bit_length(a: &Row) -> usize {
    for i in (0..4).rev() {
        if a[i] != 0 {
            return i * 64 + 64 - a[i].leading_zeros() as usize;
        }
    }
    0
}

/// `support(width, height, palette, pixels)`: which texels' colour words are non-zero.
pub fn support(
    width: usize,
    height: usize,
    palette: &[u8],
    pixels: &[u8],
) -> Result<Vec<Vec<bool>>> {
    if !(1..=256).contains(&width)
        || !(1..=256).contains(&height)
        || palette.len() != 32
        || pixels.len() != width.div_ceil(2) * height
    {
        return err("invalid alpha cover texture");
    }
    let colours: Vec<u16> = palette
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let stride = width.div_ceil(2);
    Ok((0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    colours[((pixels[y * stride + x / 2] >> ((x & 1) * 4)) & 15) as usize] != 0
                })
                .collect()
        })
        .collect())
}

fn area(r: &Rect) -> usize {
    (r.2 - r.0) * (r.3 - r.1)
}

/// `cover(mask)`: up to four disjoint rectangles covering every visible texel.
pub fn cover(mask: &[Vec<bool>]) -> Result<Vec<Rect>> {
    let height = mask.len();
    let width = mask.first().map_or(0, Vec::len);
    if !(1..=256).contains(&height)
        || !(1..=256).contains(&width)
        || mask.iter().any(|r| r.len() != width)
    {
        return err("invalid alpha support mask");
    }
    let rows: Vec<Row> = mask
        .iter()
        .map(|row| {
            let mut out = [0u64; 4];
            for (x, &v) in row.iter().enumerate() {
                if v {
                    out[x / 64] |= 1 << (x % 64);
                }
            }
            out
        })
        .collect();
    let bounds = |left: usize, top: usize, right: usize, bottom: usize| -> Option<Rect> {
        let bits = bits_between(left, right);
        let mut union = [0u64; 4];
        let (mut first, mut last) = (bottom, top);
        for (y, source_row) in rows.iter().enumerate().take(bottom).skip(top) {
            let row = and(source_row, &bits);
            if !is_zero(&row) {
                for k in 0..4 {
                    union[k] |= row[k];
                }
                first = first.min(y);
                last = y + 1;
            }
        }
        if is_zero(&union) {
            return None;
        }
        Some((lowest(&union), first, bit_length(&union), last))
    };
    let Some(first) = bounds(0, 0, width, height) else {
        return Ok(Vec::new());
    };
    let mut rects = vec![first];
    while rects.len() < MAX_RECTS {
        let mut best: Option<(usize, usize, Vec<Rect>)> = None;
        for (index, &(left, top, right, bottom)) in rects.iter().enumerate() {
            for axis in 0..2 {
                let (lo, hi) = if axis == 0 {
                    (left + 1, right)
                } else {
                    (top + 1, bottom)
                };
                for cut in lo..hi {
                    let ranges = if axis == 0 {
                        [(left, top, cut, bottom), (cut, top, right, bottom)]
                    } else {
                        [(left, top, right, cut), (left, cut, right, bottom)]
                    };
                    let children: Vec<Rect> = ranges
                        .iter()
                        .filter_map(|r| bounds(r.0, r.1, r.2, r.3))
                        .collect();
                    let saved = area(&rects[index]) as i64
                        - children.iter().map(area).sum::<usize>() as i64;
                    // Stable ties match the reference's first rectangle/axis/cut.
                    if saved > 0 && best.as_ref().is_none_or(|b| saved as usize > b.0) {
                        best = Some((saved as usize, index, children));
                    }
                }
            }
        }
        let Some((_, index, children)) = best else {
            break;
        };
        rects.splice(index..index + 1, children);
    }
    let mut covered = vec![[0u64; 4]; height];
    for &(left, top, right, bottom) in &rects {
        let bits = bits_between(left, right);
        for seen in covered.iter_mut().take(bottom).skip(top) {
            if !is_zero(&and(seen, &bits)) {
                return err("alpha cover rectangles overlap");
            }
            for k in 0..4 {
                seen[k] |= bits[k];
            }
        }
    }
    if rows
        .iter()
        .zip(&covered)
        .any(|(r, s)| (0..4).any(|k| r[k] & !s[k] != 0))
    {
        return err("alpha cover omitted a visible PSX texel");
    }
    Ok(rects)
}

/// `compute_record(width, height, palette, pixels)`: the 20-byte alpha cover record.
pub fn compute_record(
    width: usize,
    height: usize,
    palette: &[u8],
    pixels: &[u8],
) -> Result<[u8; RECORD_BYTES]> {
    let rects = cover(&support(width, height, palette, pixels)?)?;
    let mut out = [0u8; RECORD_BYTES];
    out[0] = rects.len() as u8;
    for (index, &(left, top, right, bottom)) in rects.iter().enumerate() {
        out[4 + index * 4..8 + index * 4].copy_from_slice(&[
            left as u8,
            top as u8,
            (right - left - 1) as u8,
            (bottom - top - 1) as u8,
        ]);
    }
    Ok(out)
}
