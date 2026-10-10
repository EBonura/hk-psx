//! Bounded row-run visibility scissors over a conservative16px owner grid.
//! This is a child of scenery_occlusion, using its initialized-prefix Pieces.
use super::{Pieces, MAX_PIECES};
pub const WIDTH: usize = 20;
pub const HEIGHT: usize = 15;
pub const CELLS: usize = WIDTH * HEIGHT;

/// Whether any certified hidden tile intersects a nonempty input clip.
/// A false result lets the caller retain its original partition without
/// constructing an identical output or summing its area again.
#[inline(never)]
pub fn any_hidden(input: &[[i16; 4]], rows: &[u32; HEIGHT]) -> bool {
    debug_assert!(input.len() <= MAX_PIECES);
    for &[left, top, right, bottom] in input {
        debug_assert!(
            0 <= left
                && left <= right
                && right <= 320
                && 0 <= top
                && top <= bottom
                && bottom <= 240
        );
        if left == right || top == bottom {
            continue;
        }
        let first = left as usize >> 4;
        let end = (right as usize + 15) >> 4;
        let mask = ((1u32 << end) - 1) ^ ((1u32 << first) - 1);
        for row in (top as usize >> 4)..((bottom as usize + 15) >> 4) {
            if rows[row] & mask != 0 {
                return true;
            }
        }
    }
    false
}

/// Keep every source pixel whose tile has owner<=rank. Caller supplies disjoint,
/// half-open screen clips and a grid whose larger ranks prove later opacity.
/// Only clip rectangles change; the original source quad/UVs remain untouched.
/// Returns false with empty output if more than MAX_PIECES row-run rectangles are needed.
/// Input is never mutated, so a false return always permits exact fallback.
/// Empty visibility is successful with zero output rectangles.
#[cfg(test)]
#[inline(never)]
pub fn visible_runs(
    input: &[[i16; 4]],
    owners: &[u16; CELLS],
    rank: u16,
    out: &mut Pieces,
) -> bool {
    debug_assert!(input.len() <= MAX_PIECES);
    out.clear();
    for &[left, top, right, bottom] in input {
        debug_assert!(
            0 <= left
                && left <= right
                && right <= 320
                && 0 <= top
                && top <= bottom
                && bottom <= 240
        );
        if left == right || top == bottom {
            continue;
        }
        let base = out.len();
        let mut y = top;
        while y < bottom {
            let row = (y as usize >> 4) * WIDTH;
            let next_y = (((y as i32 >> 4) + 1) << 4).min(bottom as i32) as i16;
            let mut x = left;
            let mut run = right;
            while x < right {
                let hidden = owners[row + (x as usize >> 4)] > rank;
                let next_x = (((x as i32 >> 4) + 1) << 4).min(right as i32) as i16;
                if hidden {
                    if run < x && !append_run(out, base, run, x, y, next_y) {
                        out.clear();
                        return false;
                    }
                    run = right;
                } else if run == right {
                    run = x;
                }
                x = next_x;
            }
            if run < right && !append_run(out, base, run, right, y, next_y) {
                out.clear();
                return false;
            }
            y = next_y;
        }
    }
    true
}
/// Rows contain exactly the screen tiles hidden by strictly later opaque draws.
#[inline(never)]
pub fn visible_runs_rows(input: &[[i16; 4]], rows: &[u32; HEIGHT], out: &mut Pieces) -> bool {
    debug_assert!(input.len() <= MAX_PIECES);
    out.clear();
    for &[left, top, right, bottom] in input {
        debug_assert!(
            0 <= left
                && left <= right
                && right <= 320
                && 0 <= top
                && top <= bottom
                && bottom <= 240
        );
        if left == right || top == bottom {
            continue;
        }
        let base = out.len();
        let first = left as usize >> 4;
        let end = (right as usize + 15) >> 4;
        let mask = ((1u32 << end) - 1) ^ ((1u32 << first) - 1);
        let mut y = top;
        while y < bottom {
            let row = y as usize >> 4;
            let bits = rows[row] & mask;
            let mut next_row = row + 1;
            // Equal relevant masks have exactly the same clipped horizontal
            // runs. Emit/extend them once for their whole consecutive height.
            while (next_row << 4) < bottom as usize && rows[next_row] & mask == bits {
                next_row += 1;
            }
            let next_y = ((next_row << 4).min(bottom as usize)) as i16;
            if bits == 0 {
                if !append_run(out, base, left, right, y, next_y) {
                    out.clear();
                    return false;
                }
            } else if bits != mask {
                let mut visible = (!bits & mask) >> first;
                let mut tile = first;
                while visible != 0 {
                    let skip = trailing_zeros_nonzero(visible);
                    visible >>= skip;
                    tile += skip as usize;
                    let start = tile;
                    let length = trailing_zeros_nonzero(!visible);
                    visible >>= length;
                    tile += length as usize;
                    let l = ((start << 4) as i16).max(left);
                    let r = ((tile << 4) as i16).min(right);
                    if !append_run(out, base, l, r, y, next_y) {
                        out.clear();
                        return false;
                    }
                }
            }
            y = next_y;
        }
    }
    true
}

#[inline]
fn append_run(out: &mut Pieces, base: usize, left: i16, right: i16, top: i16, bottom: i16) -> bool {
    // Every existing rectangle is final except a matching run in the preceding
    // row. No merges cross an input clip, preserving its exact boundary.
    for i in base..out.len() {
        let r = unsafe { out.rects[i].assume_init_mut() };
        if r[0] == left && r[2] == right && r[3] == top {
            r[3] = bottom;
            return true;
        }
    }
    if out.len() == MAX_PIECES {
        return false;
    }
    out.push([left, top, right, bottom]);
    true
}

/// Packed two-bit CTZ values for each nonzero nibble. The zero nibble is
/// skipped four bits at a time; the nonzero input contract bounds this loop.
/// Keeping the16-entry table in a register avoids RAM loads on the R3000.
#[inline]
fn trailing_zeros_nonzero(mut value: u32) -> u32 {
    let mut count = 0;
    while value & 15 == 0 {
        value >>= 4;
        count += 4;
    }
    count + ((0x12131210u32 >> ((value & 15) * 2)) & 3)
}

#[cfg(test)]
#[test]
fn packed_nibble_trailing_counts_cover_every_shift_and_nonzero_nibble() {
    for shift in (0..32).step_by(4) {
        for nibble in 1u32..16 {
            for high in [0u32, 0x55555555, 0xaaaaaaaa, u32::MAX] {
                let v = if shift == 28 {
                    nibble << shift
                } else {
                    (high << (shift + 4)) | (nibble << shift)
                };
                assert_eq!(trailing_zeros_nonzero(v), v.trailing_zeros());
            }
        }
    }
}
