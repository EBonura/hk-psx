//! Bounded repair of oversized flat textured scenery quads.
//!
//! Coordinates remain framebuffer-local: the SDK supplies GP0(E5)'s draw offset.
//! Shared integer-UV grid vertices use rational round-to-nearest, ties-to-even.
//! Subdivision is a sampling approximation: restarting the GPU's Q12 attribute
//! DDA can change texels and rounded oblique edges. It never clamps source XY.

pub const MAX_DIVISIONS: usize = 64;
/// Bounds all i64 weighted-coordinate intermediates and rejects hostile input.
pub const MAX_COORDINATE: i32 = 1 << 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UvRect {
    pub u: u16,
    pub v: u16,
    pub w: u16,
    pub h: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Quad {
    pub xy: [(i16, i16); 4],
    pub uv: [(u8, u8); 4],
}
impl Quad {
    pub const ZERO: Self = Self {
        xy: [(0, 0); 4],
        uv: [(0, 0); 4],
    };
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidUv,
    /// Coordinates exceed the arithmetic bound or no legal grid was found.
    UnsupportedBounds,
    /// The accepted grid needs more output slots. No partial count is returned.
    Capacity,
}

/// Init-only proof for the renderer's common Q24.8 camera translation.
/// Valid ONLY after rejecting quads wholly x<0/x>320/y<0/y>240, including
/// boundary-touching/zero-area survivors. floor((p-camera)/256) changes any
/// coordinate difference by at most ceil(source range/256), also after y flip.
/// Thus range X<=703 and Y<=511 plus a viewport-overlapping AABB imply
/// x∈[-703,1023], y∈[-511,751], and every original triangle edge is legal.
/// Use the full `legal` check when false; larger geometry is not rejected here.
/// Wide init arithmetic makes even hostile i32 source extents fail safely.
pub fn legal_extent_q8(xy: &[i32; 8]) -> bool {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (xy[0], xy[0], xy[1], xy[1]);
    for k in 1..4 {
        min_x = min_x.min(xy[k * 2]);
        max_x = max_x.max(xy[k * 2]);
        min_y = min_y.min(xy[k * 2 + 1]);
        max_y = max_y.max(xy[k * 2 + 1]);
    }
    max_x.abs_diff(min_x) <= 703 * 256 && max_y.abs_diff(min_y) <= 511 * 256
}

/// Command coordinates are signed 11-bit BEFORE the SDK's drawing offset.
/// Both original triangles must obey all three hardware edge limits.
pub fn legal(xy: &[(i32, i32); 4]) -> bool {
    if xy
        .iter()
        .any(|&(x, y)| !(-1024..=1023).contains(&x) || !(-1024..=1023).contains(&y))
    {
        return false;
    }
    for (a, b) in [(0, 1), (1, 2), (2, 0), (1, 3), (3, 2)] {
        if (xy[a].0 - xy[b].0).abs() > 1023 || (xy[a].1 - xy[b].1).abs() > 511 {
            return false;
        }
    }
    true
}
fn outside(xy: &[(i32, i32); 4]) -> bool {
    xy.iter().all(|p| p.0 <= 0)
        || xy.iter().all(|p| p.0 >= 320)
        || xy.iter().all(|p| p.1 <= 0)
        || xy.iter().all(|p| p.1 >= 240)
}
fn nearest_even(n: i64, d: i64) -> i64 {
    debug_assert!(d > 0);
    let q = n / d;
    let twice_remainder = (n % d).abs() * 2;
    if twice_remainder > d || (twice_remainder == d && q & 1 != 0) {
        q + if n < 0 { -1 } else { 1 }
    } else {
        q
    }
}
// Current cooked rooms have |XY|<=3691 and UV denominator<=65025, so all
// convex weighted sums fit signed32. Keep the wide reference path for larger
// supported inputs; MIPS-I otherwise lowers each i64 division to a costly helper.
#[inline]
fn nearest_even_small(n: i32, d: i32) -> i32 {
    let q = n / d;
    let twice_remainder = (n % d).abs() * 2;
    if twice_remainder > d || (twice_remainder == d && q & 1 != 0) {
        q + if n < 0 { -1 } else { 1 }
    } else {
        q
    }
}
#[inline]
fn point_small(xy: &[(i32, i32); 4], x: i32, y: i32, width: i32, height: i32) -> (i32, i32) {
    let weights = [
        (width - x) * (height - y),
        x * (height - y),
        (width - x) * y,
        x * y,
    ];
    let mut px = 0;
    let mut py = 0;
    for i in 0..4 {
        px += xy[i].0 * weights[i];
        py += xy[i].1 * weights[i];
    }
    let denominator = width * height;
    (
        nearest_even_small(px, denominator),
        nearest_even_small(py, denominator),
    )
}
fn offsets(range: u16, divisions: usize, values: &mut [u16; MAX_DIVISIONS + 1]) -> usize {
    if range == 0 {
        values[0] = 0;
        values[1] = 0;
        return 2;
    }
    let mut count = 0;
    for i in 0..=divisions {
        let value = nearest_even_small(i as i32 * range as i32, divisions as i32) as u16;
        if count == 0 || values[count - 1] != value {
            values[count] = value;
            count += 1;
        }
    }
    count
}
fn point(xy: &[(i32, i32); 4], x: i64, y: i64, width: i64, height: i64) -> (i32, i32) {
    let weights = [
        (width - x) * (height - y),
        x * (height - y),
        (width - x) * y,
        x * y,
    ];
    let mut px = 0;
    let mut py = 0;
    for i in 0..4 {
        px += xy[i].0 as i64 * weights[i];
        py += xy[i].1 as i64 * weights[i];
    }
    let d = width * height;
    (nearest_even(px, d) as i32, nearest_even(py, d) as i32)
}
fn packed(xy: [(i32, i32); 4], uv: [(u8, u8); 4]) -> Quad {
    Quad {
        xy: xy.map(|(x, y)| (x as i16, y as i16)),
        uv,
    }
}

/// Return complete legal child quads, or an explicit error. On error `out` may
/// contain scratch writes and MUST NOT be submitted. Offscreen input returns 0.
/// Legal visible input preserves its original vertices and inclusive UV ends.
/// The caller reserves packet capacity before submitting any returned children.
/// `subdivide`'s offset tables and two grid rows (about 1.3 KiB), owned by
/// the caller so the renderer can keep them off the stack it runs on.
pub struct Grid {
    us: [u16; MAX_DIVISIONS + 1],
    vs: [u16; MAX_DIVISIONS + 1],
    row_a: [(i32, i32); MAX_DIVISIONS + 1],
    row_b: [(i32, i32); MAX_DIVISIONS + 1],
}
impl Grid {
    pub const ZERO: Self = Self {
        us: [0; MAX_DIVISIONS + 1],
        vs: [0; MAX_DIVISIONS + 1],
        row_a: [(0, 0); MAX_DIVISIONS + 1],
        row_b: [(0, 0); MAX_DIVISIONS + 1],
    };
}
#[cfg(test)]
pub fn subdivide(xy: [(i32, i32); 4], uv: UvRect, out: &mut [Quad]) -> Result<usize, Error> {
    let mut grid = Grid::ZERO;
    subdivide_in(xy, uv, out, &mut grid)
}
#[inline(never)]
pub fn subdivide_in(
    xy: [(i32, i32); 4],
    uv: UvRect,
    out: &mut [Quad],
    grid: &mut Grid,
) -> Result<usize, Error> {
    if uv.w == 0 || uv.h == 0 || uv.u as u32 + uv.w as u32 > 256 || uv.v as u32 + uv.h as u32 > 256
    {
        return Err(Error::InvalidUv);
    }
    if xy.iter().any(|&(x, y)| {
        !(-MAX_COORDINATE..=MAX_COORDINATE).contains(&x)
            || !(-MAX_COORDINATE..=MAX_COORDINATE).contains(&y)
    }) {
        return Err(Error::UnsupportedBounds);
    }
    if outside(&xy) {
        return Ok(0);
    }
    let u0 = uv.u as u8;
    let v0 = uv.v as u8;
    let u1 = (uv.u + uv.w - 1) as u8;
    let v1 = (uv.v + uv.h - 1) as u8;
    if legal(&xy) {
        if out.is_empty() {
            return Err(Error::Capacity);
        }
        out[0] = packed(xy, [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]);
        return Ok(1);
    }
    // Two shared rows avoid an unbounded or 65x65 scratch grid. Every entry
    // read below is written first for this call, so the grid needs no clearing.
    let Grid {
        us,
        vs,
        row_a,
        row_b,
    } = grid;
    // Swap these two references, not their 520-byte backing arrays. The row
    // contents remain in place and each new row overwrites only needed entries.
    let mut previous = row_a;
    let mut current = row_b;
    let width = (uv.w - 1).max(1) as i64;
    let height = (uv.h - 1).max(1) as i64;
    // Weights are nonnegative and sum to this denominator. Bounding every
    // |coordinate|*denominator bounds every term AND every partial sum.
    let narrow_limit = i32::MAX / (width * height) as i32;
    let narrow = xy
        .iter()
        .all(|&(x, y)| x.abs() <= narrow_limit && y.abs() <= narrow_limit);
    let mut divisions = 2;
    while divisions <= MAX_DIVISIONS {
        let nu = offsets(uv.w - 1, divisions, us);
        let nv = offsets(uv.h - 1, divisions, vs);
        let mut count = 0;
        let mut bad = false;
        for j in 0..nv {
            let y = if uv.h == 1 { j as i64 } else { vs[j] as i64 };
            for i in 0..nu {
                let x = if uv.w == 1 { i as i64 } else { us[i] as i64 };
                current[i] = if narrow {
                    point_small(&xy, x as i32, y as i32, width as i32, height as i32)
                } else {
                    point(&xy, x, y, width, height)
                };
            }
            if j != 0 {
                for i in 1..nu {
                    let child = [previous[i - 1], previous[i], current[i - 1], current[i]];
                    if outside(&child) {
                        continue;
                    }
                    if !legal(&child) {
                        bad = true;
                        break;
                    }
                    if count < out.len() {
                        let a = (uv.u + us[i - 1]) as u8;
                        let b = (uv.u + us[i]) as u8;
                        let c = (uv.v + vs[j - 1]) as u8;
                        let d = (uv.v + vs[j]) as u8;
                        out[count] = packed(child, [(a, c), (b, c), (a, d), (b, d)]);
                    }
                    count += 1;
                }
            }
            if bad {
                break;
            }
            core::mem::swap(&mut previous, &mut current);
        }
        if !bad {
            return if count > out.len() {
                Err(Error::Capacity)
            } else {
                Ok(count)
            };
        }
        divisions *= 2;
    }
    Err(Error::UnsupportedBounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    const UV: UvRect = UvRect {
        u: 17,
        v: 13,
        w: 47,
        h: 39,
    };
    #[test]
    fn nearest_ties_are_even_for_both_signs() {
        for (n, expected) in [
            (1, 0),
            (3, 2),
            (5, 2),
            (7, 4),
            (-1, 0),
            (-3, -2),
            (-5, -2),
            (-7, -4),
        ] {
            assert_eq!(nearest_even(n, 2), expected);
        }
    }
    #[test]
    fn native_width_interpolation_matches_wide_reference() {
        // Includes positive/negative ties and values at the overflow-proof
        // boundary, plus uneven texture sizes seen in the real source packs.
        for (width, height) in [(1, 1), (46, 38), (47, 47), (255, 255)] {
            let limit = i32::MAX / (width * height);
            for xy in [
                [(-370, -270), (820, -201), (-511, 653), (679, 722)],
                [
                    (-limit, limit),
                    (limit, -limit),
                    (limit, limit),
                    (-limit, -limit),
                ],
            ] {
                for y in [0, 1, height / 2, height] {
                    for x in [0, 1, width / 2, width] {
                        assert_eq!(
                            point_small(&xy, x, y, width, height),
                            point(&xy, x as i64, y as i64, width as i64, height as i64)
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn exact_fast_path_and_edge_limits() {
        let p = [(-704, 0), (319, 0), (-704, 511), (319, 511)];
        assert!(legal(&p));
        let mut out = [Quad::ZERO; 1];
        assert_eq!(subdivide(p, UV, &mut out), Ok(1));
        assert_eq!(out[0].xy, p.map(|(x, y)| (x as i16, y as i16)));
        assert_eq!(out[0].uv, [(17, 13), (63, 13), (17, 51), (63, 51)]);
        assert!(!legal(&[(-705, 0), (319, 0), (-705, 511), (319, 511)]));
        assert!(!legal(&[(0, 0), (100, 0), (0, 512), (100, 512)]));
        assert!(!legal(&[(1024, 0), (1024, 0), (1024, 0), (1024, 0)]));
    }
    #[test]
    fn reflected_grid_is_legal_and_keeps_shared_uv_vertices() {
        let mut out = [Quad::ZERO; 16];
        let p = [(820, -201), (-370, -270), (679, 722), (-511, 653)];
        let count = subdivide(p, UV, &mut out).unwrap();
        assert!(count > 1);
        for q in &out[..count] {
            assert!(legal(&q.xy.map(|(x, y)| (x as i32, y as i32))));
            for (xy, uv) in q.xy.iter().zip(q.uv) {
                for other in &out[..count] {
                    for (other_xy, other_uv) in other.xy.iter().zip(other.uv) {
                        if uv == other_uv {
                            assert_eq!(xy, other_xy);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn capacity_and_unsupported_input_are_explicit() {
        let p = [(-400, -300), (800, -300), (-400, 700), (800, 700)];
        assert_eq!(subdivide(p, UV, &mut []), Err(Error::Capacity));
        assert_eq!(
            subdivide(p, UvRect { w: 0, ..UV }, &mut []),
            Err(Error::InvalidUv)
        );
        assert_eq!(
            subdivide(p, UvRect { u: 250, ..UV }, &mut []),
            Err(Error::InvalidUv)
        );
        assert_eq!(
            subdivide([(i32::MIN, 0); 4], UV, &mut []),
            Err(Error::UnsupportedBounds)
        );
        assert_eq!(
            subdivide(p, UvRect { w: 1, h: 1, ..UV }, &mut [Quad::ZERO; 16]),
            Err(Error::UnsupportedBounds)
        );
    }
    #[test]
    fn offscreen_geometry_needs_no_packets() {
        assert_eq!(subdivide([(-4000, -3000); 4], UV, &mut []), Ok(0));
        assert_eq!(
            subdivide(
                [(320, 0), (2000, 0), (320, 2000), (2000, 2000)],
                UV,
                &mut []
            ),
            Ok(0)
        );
    }
}

#[cfg(test)]
mod legal_extent_tests {
    use super::*;
    fn projected(xy: [i32; 8], camera: (i32, i32)) -> [(i32, i32); 4] {
        core::array::from_fn(|k| {
            (
                160 + ((xy[k * 2] - camera.0) >> 8),
                120 - ((xy[k * 2 + 1] - camera.1) >> 8),
            )
        })
    }
    fn survives(v: &[(i32, i32); 4]) -> bool {
        !(v.iter().all(|p| p.0 < 0)
            || v.iter().all(|p| p.0 > 320)
            || v.iter().all(|p| p.1 < 0)
            || v.iter().all(|p| p.1 > 240))
    }
    #[test]
    fn every_camera_fraction_at_extent_and_viewport_boundaries() {
        let mut checks = 0;
        for sx in [
            0,
            1,
            703 * 256 - 1,
            703 * 256,
            703 * 256 + 1,
            704 * 256,
            1023 * 256,
        ] {
            for sy in [0, 1, 511 * 256 - 1, 511 * 256, 511 * 256 + 1, 512 * 256] {
                let shapes = [
                    [0, 0, sx, 0, 0, sy, sx, sy],
                    [sx, 0, 0, sy, sx, sy, 0, 0],
                    [sx / 3, 0, sx, sy / 3, 0, sy * 2 / 3, sx * 2 / 3, sy],
                ];
                for xy in shapes {
                    let flag = legal_extent_q8(&xy);
                    for f in 0..256 {
                        for edge in [-1024, -703, -511, -1, 0, 239, 240, 319, 320, 511, 703, 1023] {
                            let v = projected(
                                xy,
                                ((160 - edge) * 256 + f, (120 - edge) * 256 + (255 - f)),
                            );
                            if survives(&v) {
                                assert_eq!(
                                    flag || legal(&v),
                                    legal(&v),
                                    "extent{sx},{sy} fraction{f} edge{edge} {v:?}"
                                );
                            }
                            checks += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checks, 387072);
    }
    #[test]
    fn arbitrary_quads_camera_fractions_and_reflections() {
        let mut seed = 0x192af621u32;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut accepted = 0;
        for _ in 0..200000 {
            let sx = (random() % (704 * 256)) as i32;
            let sy = (random() % (512 * 256)) as i32;
            let ox = (random() % 2000000) as i32 - 1000000;
            let oy = (random() % 2000000) as i32 - 1000000;
            let xy = core::array::from_fn(|i| {
                if i & 1 == 0 {
                    ox + (random() % (sx as u32 + 1)) as i32
                } else {
                    oy + (random() % (sy as u32 + 1)) as i32
                }
            });
            let v = projected(
                xy,
                (
                    ox + (random() % 300000) as i32 - 100000,
                    oy + (random() % 200000) as i32 - 100000,
                ),
            );
            if survives(&v) {
                let flag = legal_extent_q8(&xy);
                assert_eq!(flag || legal(&v), legal(&v));
                accepted += usize::from(flag);
            }
        }
        assert!(accepted > 10000);
    }
    #[test]
    fn conservative_threshold_and_hostile_extents() {
        let boundary = [0, 0, 703 * 256, 0, 0, 511 * 256, 703 * 256, 511 * 256];
        assert!(legal_extent_q8(&boundary));
        let mut larger = boundary;
        larger[2] += 1;
        assert!(!legal_extent_q8(&larger));
        let mut larger = boundary;
        larger[5] += 1;
        assert!(!legal_extent_q8(&larger));
        assert!(!legal_extent_q8(&[i32::MIN, 0, i32::MAX, 0, 0, 0, 0, 0]));
        assert!(!legal_extent_q8(&[0, i32::MIN, 0, i32::MAX, 0, 0, 0, 0]));
        let too_wide = projected(
            [0, 0, 704 * 256, 0, 0, 256, 704 * 256, 256],
            (-160 * 256, 120 * 256),
        );
        assert!(survives(&too_wide));
        assert!(!legal(&too_wide)); // x=320..1024
        let too_tall = projected([0, 0, 256, 0, 0, 512 * 256, 256, 512 * 256], (0, 0));
        assert!(survives(&too_tall));
        assert!(!legal(&too_tall)); // y edge512
        let zero = projected([0; 8], (160 * 256, 120 * 256));
        assert!(survives(&zero));
        assert!(legal(&zero));
    }
    #[test]
    fn native_unsigned_extent_matches_wide_reference_for_full_i32_domain() {
        let reference = |xy: &[i32; 8]| {
            let xs = [xy[0], xy[2], xy[4], xy[6]];
            let ys = [xy[1], xy[3], xy[5], xy[7]];
            *xs.iter().max().unwrap() as i64 - *xs.iter().min().unwrap() as i64 <= 703 * 256
                && *ys.iter().max().unwrap() as i64 - *ys.iter().min().unwrap() as i64 <= 511 * 256
        };
        for origin in [
            i32::MIN,
            i32::MIN + 703 * 256,
            -703 * 256,
            -1,
            0,
            1,
            i32::MAX - 703 * 256,
            i32::MAX,
        ] {
            for sx in [0, 1, 703 * 256 - 1, 703 * 256, 703 * 256 + 1, i32::MAX] {
                for sy in [0, 1, 511 * 256 - 1, 511 * 256, 511 * 256 + 1, i32::MAX] {
                    let Some(right) = origin.checked_add(sx) else {
                        continue;
                    };
                    let Some(bottom) = origin.checked_add(sy) else {
                        continue;
                    };
                    for xy in [
                        [origin, origin, right, origin, origin, bottom, right, bottom],
                        [right, bottom, origin, bottom, right, origin, origin, origin],
                    ] {
                        assert_eq!(legal_extent_q8(&xy), reference(&xy));
                    }
                }
            }
        }
        let mut seed = 0x319ff92au32;
        for _ in 0..200000 {
            let xy = core::array::from_fn(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as i32
            });
            assert_eq!(legal_extent_q8(&xy), reference(&xy));
        }
    }
}
