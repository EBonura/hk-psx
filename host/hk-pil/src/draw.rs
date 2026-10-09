//! Filled polygons on a one-byte-per-pixel image, by scanline conversion.
//!
//! Vertices are truncated to integer pixel positions. For every pixel row
//! from the polygon's top to its bottom, the crossings of the non-horizontal
//! edges with that row are sorted and the pixels between each pair of
//! crossings, ends included, are set.

use crate::Image;

/// One non-horizontal edge: its row range, and the x position along it from
/// the vertex the edge starts at, in single precision.
struct Edge {
    y_top: i64,
    y_bottom: i64,
    x_start: f32,
    y_start: i64,
    dx: f32,
}

impl Edge {
    /// Crossing of this edge with pixel row `y`: a fused multiply-add of the
    /// per-row slope from the start vertex.
    fn at(&self, y: i64) -> f32 {
        ((y - self.y_start) as f32).mul_add(self.dx, self.x_start)
    }
}

/// Integer pixel run for a span from crossing `a` to crossing `b`. Pixel p
/// sits at coordinate p: `a` rounds half up, `b` rounds half toward zero.
fn run(a: f32, b: f32) -> (i64, i64) {
    let lo = (a + 0.5).floor() as i64;
    let hi = if b >= 0.0 {
        (b - 0.5).ceil()
    } else {
        (b + 0.5).floor()
    } as i64;
    (lo, hi)
}

/// Fill the polygon whose vertices are `xy` (truncated toward zero) with `ink`.
pub fn polygon_fill(im: &mut Image, xy: &[(f64, f64)], ink: u8) {
    let mut pts: Vec<(i64, i64)> = xy.iter().map(|&(x, y)| (x as i64, y as i64)).collect();
    // Repeated vertices (including the last against the first) add nothing.
    pts.dedup();
    while pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let n = pts.len();
    if n == 0 {
        return;
    }
    // Edge i runs from vertex i to vertex i + 1; flat edges are `None`.
    let by_vertex: Vec<Option<Edge>> = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            if a.1 == b.1 {
                return None;
            }
            let (top, bottom) = if a.1 < b.1 { (a, b) } else { (b, a) };
            Some(Edge {
                y_top: top.1,
                y_bottom: bottom.1,
                x_start: a.0 as f32,
                y_start: a.1,
                dx: (b.0 - a.0) as f32 / (b.1 - a.1) as f32,
            })
        })
        .collect();
    let edges: Vec<&Edge> = by_vertex.iter().flatten().collect();
    // Vertices where the outline turns back: the neighbouring rows (skipping
    // flat edges) are both above (`bottom` tips) or both below (`top` tips).
    let mut tips: Vec<(i64, i64, bool, usize)> = Vec::new();
    for i in 0..n {
        // A vertex on a flat edge is covered by that edge's run instead.
        if pts[(i + n - 1) % n].1 == pts[i].1 || pts[(i + 1) % n].1 == pts[i].1 {
            continue;
        }
        let prev = (1..=n)
            .map(|k| pts[(i + n - k) % n])
            .find(|p| p.1 != pts[i].1);
        let next = (1..=n).map(|k| pts[(i + k) % n]).find(|p| p.1 != pts[i].1);
        if let (Some(p), Some(q)) = (prev, next) {
            if p.1 < pts[i].1 && q.1 < pts[i].1 {
                tips.push((pts[i].0, pts[i].1, false, i));
            } else if p.1 > pts[i].1 && q.1 > pts[i].1 {
                tips.push((pts[i].0, pts[i].1, true, i));
            }
        }
    }
    // Pixel runs of one row: the crossing pairs of the edges active in it
    // (an edge covers its top row but not its bottom row), a tip contributing
    // a zero-length pair, and flat edges.
    let rows_runs = |y: i64| -> Vec<(i64, i64)> {
        let mut xs: Vec<f32> = edges
            .iter()
            .filter(|e| e.y_top <= y && y < e.y_bottom)
            .map(|e| e.at(y))
            .collect();
        for &(tx, ty, top, _) in &tips {
            if ty == y && !top {
                xs.push(tx as f32);
                xs.push(tx as f32);
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut runs: Vec<(i64, i64)> = xs.chunks_exact(2).map(|p| run(p[0], p[1])).collect();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            if a.1 == y && b.1 == y {
                runs.push(run(a.0.min(b.0) as f32, a.0.max(b.0) as f32));
            }
        }
        runs
    };
    let (w, h) = (im.width as i64, im.height as i64);
    let y_min = pts.iter().map(|p| p.1).min().unwrap().max(0);
    let y_max = pts.iter().map(|p| p.1).max().unwrap().min(h - 1);
    for y in y_min..=y_max {
        let mut runs = rows_runs(y);
        // A tip row also reaches along the outline until it touches the
        // neighbouring row's run, so a sharp corner stays connected to the body.
        for &(tx, ty, top, v) in &tips {
            if ty != y {
                continue;
            }
            let towards = if top { y + 1 } else { y - 1 };
            let at = run(tx as f32, tx as f32);
            let Some(r) = runs.iter_mut().find(|r| **r == at) else {
                continue;
            };
            // Where the two edges at this vertex stand in the neighbouring row.
            let (Some(e1), Some(e2)) = (&by_vertex[(v + n - 1) % n], &by_vertex[v]) else {
                continue;
            };
            let (x1, x2) = (e1.at(towards), e2.at(towards));
            let tx = tx as f32;
            if x1.max(x2) < tx {
                r.0 = r.0.min((x1.max(x2) + 0.5).floor() as i64 + 1);
            } else if x1.min(x2) > tx {
                r.1 = r.1.max((x1.min(x2) + 0.5).floor() as i64 - 1);
            }
        }
        for (lo, hi) in runs {
            for x in lo.max(0)..=hi.min(w - 1) {
                im.data[(y * w + x) as usize] = ink;
            }
        }
    }
}
