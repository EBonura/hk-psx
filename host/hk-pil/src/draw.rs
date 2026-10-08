//! `ImageDraw.polygon(xy, fill=ink)` on a one-byte-per-pixel image
//! (libImaging/Draw.c: ImagingDrawPolygon with fill, polygon_generic, hline8).
//! The arm64 build fuses `(y - y0) * dx + x0` into one fmadd; so does this.

use crate::Image;

#[derive(Clone, Copy)]
struct Edge {
    x0: i32,
    y0: i32,
    xmin: i32,
    ymin: i32,
    xmax: i32,
    ymax: i32,
    dx: f32,
}

fn add_edge(x0: i32, y0: i32, x1: i32, y1: i32) -> Edge {
    let (xmin, xmax) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (ymin, ymax) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    let dx = if y0 == y1 { 0.0 } else { (x1 - x0) as f32 / (y1 - y0) as f32 };
    Edge { x0, y0, xmin, ymin, xmax, ymax, dx }
}

/// `(y - e.y0) * e.dx + e.x0` as the fused multiply-add the binary computes.
fn edge_x(e: &Edge, y: i32) -> f32 {
    ((y - e.y0) as f32).mul_add(e.dx, e.x0 as f32)
}

fn round_up(f: f32) -> i32 {
    if f >= 0.0 {
        (f + 0.5f32).floor() as i32
    } else {
        -((f.abs() as f64 + 0.5).floor()) as i32
    }
}

fn round_down(f: f32) -> i32 {
    if f >= 0.0 {
        (f - 0.5f32).ceil() as i32
    } else {
        -((f.abs() as f64 - 0.5).ceil()) as i32
    }
}

fn hline8(im: &mut Image, mut x0: i32, y0: i32, mut x1: i32, ink: u8) {
    let (w, h) = (im.width as i32, im.height as i32);
    if y0 < 0 || y0 >= h {
        return;
    }
    if x0 < 0 {
        x0 = 0;
    } else if x0 >= w {
        return;
    }
    if x1 < 0 {
        return;
    } else if x1 >= w {
        x1 = w - 1;
    }
    if x0 <= x1 {
        let row = y0 as usize * im.width;
        im.data[row + x0 as usize..=row + x1 as usize].fill(ink);
    }
}

fn polygon_generic(im: &mut Image, e: &[Edge], ink: u8) {
    if e.is_empty() {
        return;
    }
    let mut ymin = im.height as i32 - 1;
    let mut ymax = 0;
    let mut table: Vec<&Edge> = Vec::new();
    for edge in e {
        ymin = ymin.min(edge.ymin);
        ymax = ymax.max(edge.ymax);
        if edge.ymin == edge.ymax {
            hline8(im, edge.xmin, edge.ymin, edge.xmax, ink);
            continue;
        }
        table.push(edge);
    }
    ymin = ymin.max(0);
    ymax = ymax.min(im.height as i32);
    let mut xx: Vec<f32> = Vec::with_capacity(table.len() * 2);
    while ymin <= ymax {
        xx.clear();
        for (i, current) in table.iter().enumerate() {
            if ymin < current.ymin || ymin > current.ymax {
                continue;
            }
            xx.push(edge_x(current, ymin));
            if ymin == current.ymax && ymin < ymax {
                let last = *xx.last().unwrap();
                xx.push(last);
            } else if (ymin == current.ymin || ymin == current.ymax) && current.dx != 0.0 {
                for other in &table[..i] {
                    if (ymin != other.ymin && ymin != other.ymax) || other.dx == 0.0 {
                        continue;
                    }
                    let last = *xx.last().unwrap();
                    if last.round() == edge_x(other, ymin).round() {
                        let offset = if ymin == current.ymax { -1 } else { 1 };
                        let adjacent = edge_x(current, ymin + offset);
                        if ymin + offset >= other.ymin && ymin + offset <= other.ymax {
                            let adjacent_other = edge_x(other, ymin + offset);
                            let n = xx.len() - 1;
                            if xx[n] > adjacent + 1.0 && xx[n] > adjacent_other + 1.0 {
                                xx[n] = adjacent.max(adjacent_other).round() + 1.0;
                            } else if xx[n] < adjacent - 1.0 && xx[n] < adjacent_other - 1.0 {
                                xx[n] = adjacent.min(adjacent_other).round() - 1.0;
                            }
                            break;
                        }
                    }
                }
            }
        }
        // qsort with a float comparison; ties are equal values, so order is moot.
        xx.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 1;
        while i < xx.len() {
            hline8(im, round_up(xx[i - 1]), ymin, round_down(xx[i]), ink);
            i += 2;
        }
        ymin += 1;
    }
}

/// Fill the polygon whose vertices are `xy` (truncated toward zero, as
/// `_draw_polygon` does) with `ink`.
pub fn polygon_fill(im: &mut Image, xy: &[(f64, f64)], ink: u8) {
    assert_eq!(im.mode.pixel_size(), 1, "polygon fill is ported for 8-bit images only");
    let p: Vec<(i32, i32)> = xy.iter().map(|&(x, y)| (x as i32, y as i32)).collect();
    let count = p.len();
    if count == 0 {
        return;
    }
    let mut e: Vec<Edge> = Vec::with_capacity(count);
    let mut i = 0;
    while i + 1 < count {
        let (x0, y0) = p[i];
        let (x1, y1) = p[i + 1];
        if y0 == y1 && i != 0 && y0 == p[i - 1].1 {
            let prev_x = p[i - 1].0;
            let last = e.last_mut().unwrap();
            if x1 > x0 && x0 > prev_x {
                last.xmax = x1;
                i += 1;
                continue;
            } else if x1 < x0 && x0 < prev_x {
                last.xmin = x1;
                i += 1;
                continue;
            }
        }
        e.push(add_edge(x0, y0, x1, y1));
        i += 1;
    }
    if p[i] != p[0] {
        e.push(add_edge(p[i].0, p[i].1, p[0].0, p[0].1));
    }
    polygon_generic(im, &e, ink);
}

#[cfg(test)]
mod tests {
    use super::polygon_fill;
    use crate::{Image, Mode};

    #[test]
    fn axis_aligned_square_fills_its_closed_bounds() {
        let mut im = Image::new(Mode::One, 6, 6);
        polygon_fill(&mut im, &[(1.0, 1.0), (4.0, 1.0), (4.0, 4.0), (1.0, 4.0)], 1);
        for y in 0..6 {
            for x in 0..6 {
                let inside = (1..=4).contains(&x) && (1..=4).contains(&y);
                assert_eq!(im.data[y * 6 + x] != 0, inside, "({x}, {y})");
            }
        }
    }

    #[test]
    fn coordinates_truncate_toward_zero() {
        let mut a = Image::new(Mode::One, 4, 4);
        let mut b = Image::new(Mode::One, 4, 4);
        polygon_fill(&mut a, &[(0.9, 0.9), (2.9, 0.9), (0.9, 2.9)], 1);
        polygon_fill(&mut b, &[(0.0, 0.0), (2.0, 0.0), (0.0, 2.0)], 1);
        assert_eq!(a, b);
    }
}
