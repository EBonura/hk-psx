//! Conservative mandatory packet admission for the native scenery grid repair
//! (host/scenery_geometry.py): how many packets a draw needs so every child stays
//! inside the safe screen extent, with the exact rational arithmetic the Python used.

use crate::common::{err, py_round, Result};

pub const CAP: i64 = 1032;
/// 128 particles + 32 impacts/debris + 8 actors + 80 Geo + 16 Lifeblood parts.
pub const ACTOR_RESERVE: i64 = 264;
pub const CHILD_CAPACITY: i64 = 64;
pub const MAX_DIVISIONS: i64 = 64;
pub const MAX_COORDINATE: i64 = 1 << 20;
pub const SAFE_WIDTH: i64 = 704;
pub const SAFE_HEIGHT: i64 = 511;

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// A reduced fraction, denominator positive (Python's `Fraction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frac {
    n: i128,
    d: i128,
}

impl Frac {
    fn new(n: i128, d: i128) -> Frac {
        let g = gcd(n, d).max(1);
        let s = if d < 0 { -1 } else { 1 };
        Frac {
            n: s * n / g,
            d: s * d / g,
        }
    }
    fn int(n: i128) -> Frac {
        Frac { n, d: 1 }
    }
    fn mul_int(self, k: i128) -> Frac {
        Frac::new(self.n * k, self.d)
    }
    fn add(self, o: Frac) -> Frac {
        Frac::new(self.n * o.d + o.n * self.d, self.d * o.d)
    }
    /// `math.ceil`.
    fn ceil(self) -> i128 {
        -((-self.n).div_euclid(self.d))
    }
    /// Python's `round(Fraction)`: nearest, ties to even.
    fn round(self) -> i128 {
        let floor = self.n.div_euclid(self.d);
        let rem = self.n.rem_euclid(self.d);
        if rem * 2 < self.d {
            floor
        } else if rem * 2 > self.d {
            floor + 1
        } else if floor % 2 == 0 {
            floor
        } else {
            floor + 1
        }
    }
}

/// `axis_offsets(span, n)`.
fn axis_offsets(span: i64, n: i64) -> Vec<i64> {
    if span == 0 {
        return vec![0, 0];
    }
    let mut v: Vec<i64> = (0..=n)
        .map(|i| Frac::new((i * span) as i128, n as i128).round() as i64)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// What `packet_bound` returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub packets: i64,
    pub divisions: i64,
    pub child_extent_bound: [i64; 2],
}

/// `packet_bound(xy, width, height)`: `xy` are the cooked signed Q24.8 screen
/// coordinates of a draw's four corners before the camera.
pub fn packet_bound(xy: &[[i64; 2]; 4], width: i64, height: i64) -> Result<Bound> {
    if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
        return err("invalid texture dimensions");
    }
    let projected: [i64; 2] = [0, 1].map(|k| {
        let hi = xy.iter().map(|p| p[k]).max().unwrap();
        let lo = xy.iter().map(|p| p[k]).min().unwrap();
        (hi - lo + 255).div_euclid(256)
    });
    if projected[0] <= SAFE_WIDTH && projected[1] <= SAFE_HEIGHT {
        return Ok(Bound {
            packets: 1,
            divisions: 1,
            child_extent_bound: projected,
        });
    }
    let dx: [i64; 2] = [0, 1].map(|k| {
        ((xy[1][k] - xy[0][k]).abs() + 255)
            .div_euclid(256)
            .max(((xy[3][k] - xy[2][k]).abs() + 255).div_euclid(256))
    });
    let dy: [i64; 2] = [0, 1].map(|k| {
        ((xy[2][k] - xy[0][k]).abs() + 255)
            .div_euclid(256)
            .max(((xy[3][k] - xy[1][k]).abs() + 255).div_euclid(256))
    });
    let mut n = 2;
    while n <= MAX_DIVISIONS {
        let us = axis_offsets(width - 1, n);
        let vs = axis_offsets(height - 1, n);
        let step = |o: &[i64]| o.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
        let du = if width > 1 {
            Frac::new(step(&us) as i128, (width - 1) as i128)
        } else {
            Frac::int(1)
        };
        let dv = if height > 1 {
            Frac::new(step(&vs) as i128, (height - 1) as i128)
        } else {
            Frac::int(1)
        };
        let extent: [i64; 2] = [0, 1].map(|k| {
            du.mul_int(dx[k] as i128)
                .add(dv.mul_int(dy[k] as i128))
                .ceil() as i64
                + 1
        });
        if extent[0] <= SAFE_WIDTH && extent[1] <= SAFE_HEIGHT {
            return Ok(Bound {
                packets: (us.len() as i64 - 1) * (vs.len() as i64 - 1),
                divisions: n,
                child_extent_bound: extent,
            });
        }
        n *= 2;
    }
    err(format!(
        "no safe bounded grid for texture {width}x{height}, source bbox [{}, {}]",
        projected[0], projected[1]
    ))
}

/// `texture_draws_safe(raw, texture, width, height)`: true when every draw of
/// `texture` in a cooked HKROOM02 pack keeps a bounded packet grid at the new size.
pub fn texture_draws_safe(raw: &[u8], texture: u16, width: i64, height: i64) -> bool {
    let word = |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap());
    let textures = word(12) as usize;
    let draws = word(16) as usize;
    let draw_start = 40 + textures * 16;
    for index in 0..draws {
        let pos = draw_start + index * 44;
        if u16::from_le_bytes([raw[pos], raw[pos + 1]]) != texture {
            continue;
        }
        let coords: Vec<i64> = (0..8)
            .map(|i| {
                i32::from_le_bytes(raw[pos + 8 + 4 * i..pos + 12 + 4 * i].try_into().unwrap())
                    as i64
            })
            .collect();
        let xy = [
            [coords[0], coords[1]],
            [coords[2], coords[3]],
            [coords[4], coords[5]],
            [coords[6], coords[7]],
        ];
        match packet_bound(&xy, width, height) {
            Err(_) => return false,
            Ok(b) if b.packets > CHILD_CAPACITY => return false,
            Ok(_) => {}
        }
    }
    true
}

/// `check_camera_arithmetic(xy, scale, camera)`: verify the post-shift and
/// subtraction bounds across the region camera box; returns (largest projected
/// coordinate, largest camera product).
pub fn check_camera_arithmetic(
    xy: &[[i64; 2]; 4],
    scale: i64,
    camera: [f64; 4],
) -> Result<(i64, i64)> {
    let (mut largest, mut largest_product) = (0i64, 0i64);
    for k in 0..2 {
        for endpoint in [camera[k], camera[k + 2]] {
            let q = py_round(endpoint * 65536.0);
            let product = (q >> 8) * scale;
            largest_product = largest_product.max(product.abs());
            let offset = product >> 12;
            for p in xy {
                let difference = p[k] - offset;
                if !(-(1i64 << 31)..(1i64 << 31)).contains(&difference) {
                    return err("camera subtraction exceeds native i32");
                }
                let projected = if k == 0 {
                    160 + (difference >> 8)
                } else {
                    120 - (difference >> 8)
                };
                largest = largest.max(projected.abs());
                if projected.abs() > MAX_COORDINATE {
                    return err("projection exceeds native geometry arithmetic bound");
                }
            }
        }
    }
    Ok((largest, largest_product))
}

/// `mandatory_packets(packets, groups)`: a view's mandatory packet total: every
/// draw's packets, except that each decor group counts only its largest frame.
pub fn mandatory_packets(packets: &[i64], groups: &[Vec<usize>]) -> i64 {
    let mut grouped = std::collections::BTreeSet::new();
    let mut total = 0;
    for group in groups {
        if !group.is_empty() {
            total += group.iter().map(|&i| packets[i]).max().unwrap();
            grouped.extend(group.iter().copied());
        }
    }
    total
        + packets
            .iter()
            .enumerate()
            .filter(|(i, _)| !grouped.contains(i))
            .map(|(_, p)| *p)
            .sum::<i64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_draw_is_one_packet() {
        let xy = [
            [0, 0],
            [256 * 100, 0],
            [0, 256 * 100],
            [256 * 100, 256 * 100],
        ];
        let b = packet_bound(&xy, 64, 64).unwrap();
        assert_eq!((b.packets, b.divisions), (1, 1));
        assert_eq!(b.child_extent_bound, [100, 100]);
    }

    #[test]
    fn a_wide_draw_is_split_until_each_child_is_safe() {
        let xy = [
            [0, 0],
            [256 * 1500, 0],
            [0, 256 * 100],
            [256 * 1500, 256 * 100],
        ];
        let b = packet_bound(&xy, 256, 64).unwrap();
        assert!(b.packets > 1 && b.child_extent_bound[0] <= SAFE_WIDTH);
    }

    #[test]
    fn rounding_a_half_goes_to_the_even_neighbour() {
        assert_eq!(Frac::new(5, 2).round(), 2);
        assert_eq!(Frac::new(7, 2).round(), 4);
        assert_eq!(Frac::new(-5, 2).round(), -2);
        assert_eq!(axis_offsets(0, 4), vec![0, 0]);
    }

    #[test]
    fn decor_groups_count_only_their_largest_frame() {
        assert_eq!(mandatory_packets(&[1, 2, 3, 4], &[vec![1, 2]]), 1 + 4 + 3);
    }
}
