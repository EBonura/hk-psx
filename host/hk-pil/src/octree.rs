//! The "fast octree" colour quantiser for RGBA images: a coarse and a fine
//! colour histogram, with the most populated fine cells taking palette slots
//! and the coarse cells that remain sharing the rest.
//!
//! This is a clean-room fit to the behaviour of Pillow's `FASTOCTREE`
//! quantiser, checked entry for entry and index for index against it on
//! several thousand random and mask-like images. Everything that decides the
//! output is spelled out here:
//!
//! * The fine histogram has 3, 4, 3 and 3 bits for R, G, B and A (8192
//!   cells), the coarse one 2 bits each (256 cells). A cell's palette colour
//!   is the integer part of its channel means, divided in `f32`.
//! * Every fully transparent pixel is first replaced by the first fully
//!   transparent pixel in raster order, so they all share one colour.
//! * Cells are ranked by pixel count, largest first, with the platform
//!   `qsort` of macOS. Its order among equal counts shows in the palette, so
//!   the C library's own routine is called ([`sort_desc`]); a hand-written
//!   copy of its algorithm, built from FreeBSD's, differed from it on about
//!   one random array in a thousand.
//! * The palette is the coarse leftovers followed by the fine cells. The
//!   split is settled by repetition: each round gives the fine side
//!   `colors - coarse` cells and takes them out of the coarse histogram,
//!   which can free coarse slots for more fine cells.
//! * A pixel takes the index of its fine cell when that cell has a palette
//!   slot, otherwise its coarse cell's slot, otherwise index 0.

/// R, G, B, A bits of the fine histogram.
const FINE_BITS: [u32; 4] = [3, 4, 3, 3];
/// R, G, B, A bits of the coarse histogram.
const COARSE_BITS: [u32; 4] = [2, 2, 2, 2];

#[derive(Clone, Copy, Default)]
struct Cell {
    count: u32,
    sum: [u64; 4],
}

impl Cell {
    fn add(&mut self, c: [u8; 4]) {
        self.count += 1;
        for (s, v) in self.sum.iter_mut().zip(c) {
            *s += v as u64;
        }
    }
    fn remove(&mut self, o: &Cell) {
        self.count -= o.count;
        for (s, v) in self.sum.iter_mut().zip(o.sum) {
            *s -= v;
        }
    }
    /// Channel means, truncated; `f32` arithmetic as the reference does.
    fn mean(&self) -> [u8; 4] {
        if self.count == 0 {
            return [0; 4];
        }
        let n = self.count as f32;
        let mut out = [0u8; 4];
        for (o, s) in out.iter_mut().zip(self.sum) {
            *o = (s as f32 / n) as i32 as u8;
        }
        out
    }
}

fn cell_index(c: [u8; 4], bits: [u32; 4]) -> usize {
    let [rb, gb, bb, ab] = bits;
    ((c[0] as usize >> (8 - rb)) << (gb + bb + ab))
        | ((c[1] as usize >> (8 - gb)) << (bb + ab))
        | ((c[2] as usize >> (8 - bb)) << ab)
        | (c[3] as usize >> (8 - ab))
}

/// A histogram cell reference as the C sort sees it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rec {
    count: u32,
    /// The cell number, or -1 for an empty cell.
    cell: i32,
}

/// Sort by descending count with the platform's `qsort`. Which of several
/// equal counts comes first is whatever that routine leaves, and Pillow's
/// palette order shows it, so on macOS (where the reference output comes
/// from) this calls the C library itself. Elsewhere equal counts keep their
/// cell order, which is not what the reference does.
fn sort_desc(a: &mut [Rec]) {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn qsort(
                base: *mut core::ffi::c_void,
                n: usize,
                size: usize,
                cmp: extern "C" fn(*const core::ffi::c_void, *const core::ffi::c_void) -> i32,
            );
        }
        // The reference comparator: `b.count - a.count` as an `int`.
        extern "C" fn cmp(x: *const core::ffi::c_void, y: *const core::ffi::c_void) -> i32 {
            let (x, y) = unsafe { (&*(x as *const Rec), &*(y as *const Rec)) };
            y.count.wrapping_sub(x.count) as i32
        }
        // SAFETY: `a` is a live slice of `repr(C)` records and `cmp` reads
        // two of them.
        unsafe {
            qsort(
                a.as_mut_ptr() as *mut core::ffi::c_void,
                a.len(),
                core::mem::size_of::<Rec>(),
                cmp,
            )
        };
    }
    #[cfg(not(target_os = "macos"))]
    a.sort_by(|x, y| y.count.cmp(&x.count));
}

/// Cell numbers by descending count, every cell of the histogram included
/// (empty cells last, in the order the sort leaves them); `None` marks an
/// empty cell.
fn ranked(cells: &[Cell]) -> Vec<Option<usize>> {
    let mut a: Vec<Rec> = cells
        .iter()
        .enumerate()
        .map(|(i, c)| Rec {
            count: c.count,
            cell: if c.count > 0 { i as i32 } else { -1 },
        })
        .collect();
    sort_desc(&mut a);
    a.into_iter()
        .map(|r| {
            if r.cell >= 0 {
                Some(r.cell as usize)
            } else {
                None
            }
        })
        .collect()
}

/// Quantise RGBA pixels to at most `colors` palette entries (at most 255).
/// Returns the palette, padded with transparent black to `colors` entries,
/// and one palette index per pixel.
pub fn fast_octree(pixels: &[[u8; 4]], colors: usize) -> (Vec<[u8; 4]>, Vec<u8>) {
    let transparent = pixels.iter().find(|p| p[3] == 0).copied();
    let px: Vec<[u8; 4]> = pixels
        .iter()
        .map(|&p| match transparent {
            Some(t) if p[3] == 0 => t,
            _ => p,
        })
        .collect();
    let mut fine = vec![Cell::default(); 1 << FINE_BITS.iter().sum::<u32>()];
    let mut coarse = vec![Cell::default(); 1 << COARSE_BITS.iter().sum::<u32>()];
    for &p in &px {
        fine[cell_index(p, FINE_BITS)].add(p);
        coarse[cell_index(p, COARSE_BITS)].add(p);
    }
    let fine_rank = ranked(&fine);
    let used = |cells: &[Cell]| cells.iter().filter(|c| c.count > 0).count();
    let mut n_coarse = used(&coarse).min(colors);
    let mut n_fine = colors - n_coarse;
    let mut subtracted = 0;
    loop {
        for &cell in fine_rank.iter().take(n_fine).skip(subtracted) {
            if let Some(i) = cell {
                let slot = cell_index(fine[i].mean(), COARSE_BITS);
                let taken = fine[i];
                coarse[slot].remove(&taken);
            }
        }
        subtracted = n_fine;
        n_coarse = used(&coarse).min(colors);
        let wanted = colors - n_coarse;
        if wanted <= subtracted {
            break;
        }
        n_fine = wanted;
    }
    let coarse_rank = ranked(&coarse);
    let mut palette: Vec<[u8; 4]> = Vec::with_capacity(colors);
    let mut coarse_slot = vec![0u8; coarse.len()];
    let mut fine_slot: Vec<Option<u8>> = vec![None; fine.len()];
    for (i, &cell) in coarse_rank.iter().take(n_coarse).enumerate() {
        match cell {
            Some(c) => {
                coarse_slot[c] = i as u8;
                palette.push(coarse[c].mean());
            }
            None => palette.push([0; 4]),
        }
    }
    for (i, &cell) in fine_rank.iter().take(n_fine).enumerate() {
        match cell {
            Some(c) => {
                fine_slot[c] = Some((n_coarse + i) as u8);
                palette.push(fine[c].mean());
            }
            None => palette.push([0; 4]),
        }
    }
    palette.truncate(colors);
    palette.resize(colors, [0; 4]);
    let indices = px
        .iter()
        .map(|&p| {
            fine_slot[cell_index(p, FINE_BITS)].unwrap_or(coarse_slot[cell_index(p, COARSE_BITS)])
        })
        .collect();
    (palette, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_colour_takes_one_slot() {
        let (palette, indices) = fast_octree(&[[0, 0, 0, 7]; 4], 15);
        assert_eq!(palette[0], [0, 0, 0, 7]);
        assert!(palette[1..].iter().all(|p| *p == [0; 4]));
        assert_eq!(indices, [0; 4]);
    }

    #[test]
    fn alpha_cells_are_32_wide_and_merge_inside() {
        let px = [
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 16],
            [0, 0, 0, 16],
        ];
        let (palette, indices) = fast_octree(&px, 15);
        assert_eq!(palette[0], [0, 0, 0, 6]);
        assert_eq!(indices, [0; 5]);
        let px = [[0, 0, 0, 0], [0, 0, 0, 32]];
        let (_, indices) = fast_octree(&px, 15);
        assert_eq!(indices.len(), 2);
        assert_ne!(indices[0], indices[1]);
    }

    #[test]
    fn transparent_pixels_share_the_first_one_s_colour() {
        let px = [[7, 8, 9, 255], [4, 5, 6, 0], [7, 8, 9, 255], [1, 2, 3, 0]];
        let (palette, indices) = fast_octree(&px, 15);
        assert_eq!(indices, [0, 1, 0, 1]);
        assert_eq!(palette[1], [4, 5, 6, 0]);
    }
}
