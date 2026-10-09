//! Deterministic unrotated MaxRects packing into legal 256x256 texture pages. Ported
//! from `dense_pack` in tools/audit_resident_bank.py.

use crate::common::{err, Result};
use std::collections::HashSet;

/// `aligned(n, 4)`.
pub fn aligned(n: i64) -> i64 {
    (n + 3) / 4 * 4
}

/// One free rectangle of a page: (x, y, w, h).
type Free = (i64, i64, i64, i64);

/// A placed rectangle: (index, page, x, y, width, height).
pub type Placement = (usize, usize, i64, i64, i64, i64);

/// `dense_pack(rectangles)`: `(width, height, index)` rectangles to a page count and placements.
pub fn dense_pack(rectangles: &[(i64, i64, usize)]) -> Result<(usize, Vec<Placement>)> {
    let mut pages: Vec<Vec<Free>> = Vec::new();
    let mut placements: Vec<Placement> = Vec::new();
    let mut sorted = rectangles.to_vec();
    // `reverse=True` on (area, longer side, index).
    sorted.sort_by_key(|r| std::cmp::Reverse((r.0 * r.1, r.0.max(r.1), r.2)));
    for (width, height, index) in sorted {
        if !(0 < width && width <= 256) || !(0 < height && height <= 256) || width % 4 != 0 {
            return err("Invalid page rectangle");
        }
        let mut best: Option<((i64, i64, usize, i64, i64), usize, i64, i64)> = None;
        for (pi, free) in pages.iter().enumerate() {
            for &(x, y, w, h) in free {
                if width <= w && height <= h {
                    let key = ((w - width).min(h - height), (w - width).max(h - height), pi, y, x);
                    if best.as_ref().is_none_or(|b| (key, pi, x, y) < (b.0, b.1, b.2, b.3)) {
                        best = Some((key, pi, x, y));
                    }
                }
            }
        }
        let (page, x, y) = match best {
            Some((_, page, x, y)) => (page, x, y),
            None => {
                pages.push(vec![(0, 0, 256, 256)]);
                (pages.len() - 1, 0, 0)
            }
        };
        let mut changed: Vec<Free> = Vec::new();
        for &(fx, fy, fw, fh) in &pages[page] {
            if x >= fx + fw || x + width <= fx || y >= fy + fh || y + height <= fy {
                changed.push((fx, fy, fw, fh));
                continue;
            }
            if fx < x {
                changed.push((fx, fy, x - fx, fh));
            }
            if x + width < fx + fw {
                changed.push((x + width, fy, fx + fw - x - width, fh));
            }
            if fy < y {
                changed.push((fx, fy, fw, y - fy));
            }
            if y + height < fy + fh {
                changed.push((fx, y + height, fw, fy + fh - y - height));
            }
        }
        let kept: Vec<Free> = changed
            .iter()
            .enumerate()
            .filter(|&(j, r)| {
                !changed.iter().enumerate().any(|(k, q)| k != j && q.0 <= r.0 && q.1 <= r.1 && q.0 + q.2 >= r.0 + r.2 && q.1 + q.3 >= r.1 + r.3 && (q != r || k < j))
            })
            .map(|(_, r)| *r)
            .collect();
        pages[page] = kept;
        placements.push((index, page, x, y, width, height));
    }
    let mut occupied: HashSet<(usize, i64, i64)> = HashSet::new();
    for &(_, page, x, y, w, h) in &placements {
        if x % 4 != 0 || x + w > 256 || y + h > 256 {
            return err("placement outside its page");
        }
        for yy in y..y + h {
            let mut xx = x;
            while xx < x + w {
                if !occupied.insert((page, xx / 4, yy)) {
                    return err("Atlas overlap");
                }
                xx += 4;
            }
        }
    }
    Ok((pages.len(), placements))
}
