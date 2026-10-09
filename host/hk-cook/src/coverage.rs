// SPDX-License-Identifier: GPL-2.0-or-later
//! The opaque coverage proofs the scene cook binds to its banks: a tile
//! certificate for one textured quad (opaque_tiles) and a seam certificate for
//! a group of two to four flat quads (opaque_groups). Both intersect the exact
//! PS1 raster across every source rounding phase, then admit a grid bit only
//! when its whole window is opaque. They were the host/coverage_cert.rs and
//! host/coverage_groups.rs helper binaries, which the Python cookers compiled
//! with rustc at cook time; the raster equations stay in host/coverage_raster.rs.
//!
//! `tile_proofs` and `group_proofs` keep the helpers' HKTCIN01/HKGPIN01 to
//! HKTCOT01 byte formats, so the cache files and their hashes are unchanged.
#[path = "../../coverage_raster.rs"]
mod raster;
use raster::{tri_plane_eval, tri_raster_setup, tri_span_x};

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let end = self.at.checked_add(N).expect("input offset");
        let a = self
            .bytes
            .get(self.at..end)
            .expect("truncated certificate input")
            .try_into()
            .unwrap();
        self.at = end;
        a
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }
    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }
}
fn integral(a: &[u8], w: usize, h: usize) -> Vec<u32> {
    let mut p = vec![0; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0;
        for x in 0..w {
            row += a[y * w + x] as u32;
            p[(y + 1) * (w + 1) + x + 1] = p[y * (w + 1) + x + 1] + row;
        }
    }
    p
}
fn sum(p: &[u32], w: usize, l: usize, t: usize, r: usize, b: usize) -> u32 {
    p[b * (w + 1) + r] + p[t * (w + 1) + l] - p[t * (w + 1) + r] - p[b * (w + 1) + l]
}
type Proof = ([i16; 2], [u16; 2], Vec<u32>);

/// The grid bits whose `window` is wholly set in `ok`, cropped to the set
/// bits, minus any window `refuse` says to leave out.
fn erode(
    bounds: [i32; 4],
    w: usize,
    h: usize,
    shift: u32,
    window: i32,
    ok: &[u32],
    refuse: impl Fn(usize, usize, usize, usize) -> bool,
) -> Proof {
    let step = 1i32 << shift;
    let gx = bounds[0] >> shift;
    let gy = bounds[1] >> shift;
    let gw = (((bounds[2] + step - 1) >> shift) - gx) as usize;
    let gh = (((bounds[3] + step - 1) >> shift) - gy) as usize;
    let mut bits = vec![false; gw * gh];
    let (mut x0, mut y0, mut x1, mut y1) = (gw, gh, 0, 0);
    for y in 0..gh {
        for x in 0..gw {
            let l = (gx + x as i32) * step - bounds[0];
            let t = (gy + y as i32) * step - bounds[1];
            let r = l + window;
            let b = t + window;
            if l >= 0
                && t >= 0
                && r <= w as i32
                && b <= h as i32
                && sum(ok, w, l as usize, t as usize, r as usize, b as usize)
                    == (window * window) as u32
                && !refuse(l as usize, t as usize, r as usize, b as usize)
            {
                bits[y * gw + x] = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x0 >= x1 || y0 >= y1 {
        return ([0, 0], [0, 0], Vec::new());
    }
    let width = x1 - x0;
    let height = y1 - y0;
    let mut packed = vec![0u32; (width * height + 31) / 32];
    for y in y0..y1 {
        for x in x0..x1 {
            if bits[y * gw + x] {
                let bit = (y - y0) * width + x - x0;
                packed[bit / 32] |= 1 << (bit % 32);
            }
        }
    }
    (
        [(gx + x0 as i32) as i16, (gy + y0 as i32) as i16],
        [width as u16, height as u16],
        packed,
    )
}
fn bounds_of<'a>(points: impl Iterator<Item = &'a (i32, i32)>) -> [i32; 4] {
    let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for &(x, y) in points {
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x + 1);
        bounds[3] = bounds[3].max(y + 1);
    }
    bounds
}
fn legal_quad(q: &[(i32, i32)]) {
    assert!(q
        .iter()
        .all(|&(x, y)| (-1024..=1023).contains(&x) && (-1024..=1023).contains(&y)));
    for (a, b) in [(0, 1), (1, 2), (2, 0), (1, 3), (3, 2)] {
        assert!((q[a].0 - q[b].0).abs() <= 1023 && (q[a].1 - q[b].1).abs() <= 511);
    }
}

/// One textured quad's certificate: the 4 px (shift 2) or 8 px (shift 3) grid
/// bits whose 19x19 or 23x23 window samples an opaque, STP-clear texel in
/// every phase. Unknown neighbouring atlas samples never certify.
pub fn certify_tile(
    phases: &[[(i32, i32); 4]],
    tw: usize,
    th: usize,
    words: &[u16],
    shift: u32,
) -> Proof {
    assert!(shift == 2 || shift == 3);
    let window = 16 + (1i32 << shift) - 1;
    assert!(!phases.is_empty() && phases.len() <= 16 && tw > 0 && th > 0 && tw <= 252 && th <= 252);
    assert_eq!(words.len(), tw * th);
    for v in phases {
        assert_eq!(v[0], (0, 0));
        legal_quad(v);
    }
    let bounds = bounds_of(phases.iter().flatten());
    let w = (bounds[2] - bounds[0]) as usize;
    let h = (bounds[3] - bounds[1]) as usize;
    assert!(w <= 1025 && h <= 513);
    let mut opaque = vec![1u8; w * h];
    let uv = [
        (0, 0),
        (tw as i32 - 1, 0),
        (0, th as i32 - 1),
        (tw as i32 - 1, th as i32 - 1),
    ];
    for v in phases {
        let mut here = vec![0u8; w * h];
        for ids in [[0, 1, 2], [1, 3, 2]] {
            let Some(t) =
                tri_raster_setup(ids.map(|i| v[i]), [(0, 0, 0); 3], ids.map(|i| uv[i]), true)
            else {
                continue;
            };
            for (y0, y1, l, dl, r, dr) in t.parts {
                for y in y0..y1 {
                    let k = (y - y0) as i64;
                    for x in tri_span_x(l + k * dl)..tri_span_x(r + k * dr) {
                        assert!(
                            x >= bounds[0] && x < bounds[2] && y >= bounds[1] && y < bounds[3],
                            "raster escaped proven bounds"
                        );
                        let u = tri_plane_eval(t.planes[3], x, y) as usize;
                        let v = tri_plane_eval(t.planes[4], x, y) as usize;
                        if u >= tw || v >= th {
                            continue;
                        }
                        let word = words[v * tw + u];
                        if word != 0 && word & 0x8000 == 0 {
                            here[(y - bounds[1]) as usize * w + (x - bounds[0]) as usize] = 1;
                        }
                    }
                }
            }
        }
        for (a, b) in opaque.iter_mut().zip(here) {
            *a &= b;
        }
    }
    erode(
        bounds,
        w,
        h,
        shift,
        window,
        &integral(&opaque, w, h),
        |_, _, _, _| false,
    )
}

/// One group's seam certificate: the 4 px grid bits whose 19x19 window the
/// union of its flat quads covers in every phase, less any window one member
/// alone already covers (its own tile certificate holds those).
pub fn certify_group(phases: &[Vec<(i32, i32)>], shift: u32) -> Proof {
    assert_eq!(shift, 2);
    let window = 19;
    assert!(!phases.is_empty() && phases.len() <= 256);
    let members = phases[0].len() / 4;
    assert!((2..=4).contains(&members));
    for v in phases {
        assert_eq!(v.len(), members * 4);
        assert_eq!(v[0], (0, 0));
        v.chunks_exact(4).for_each(legal_quad);
    }
    let bounds = bounds_of(phases.iter().flatten());
    let w = (bounds[2] - bounds[0]) as usize;
    let h = (bounds[3] - bounds[1]) as usize;
    assert!(w <= 704 && h <= 512);
    let mut opaque = vec![1u8; w * h];
    let mut single = vec![vec![1u8; w * h]; members];
    for v in phases {
        let mut here = vec![0u8; w * h];
        for (member, q) in v.chunks_exact(4).enumerate() {
            let mut one = vec![0u8; w * h];
            for ids in [[0, 1, 2], [1, 3, 2]] {
                let Some(t) =
                    tri_raster_setup(ids.map(|i| q[i]), [(0, 0, 0); 3], [(0, 0); 3], false)
                else {
                    continue;
                };
                for (y0, y1, l, dl, r, dr) in t.parts {
                    for y in y0..y1 {
                        let k = (y - y0) as i64;
                        for x in tri_span_x(l + k * dl)..tri_span_x(r + k * dr) {
                            assert!(
                                x >= bounds[0] && x < bounds[2] && y >= bounds[1] && y < bounds[3],
                                "group raster escaped bounds"
                            );
                            let at = (y - bounds[1]) as usize * w + (x - bounds[0]) as usize;
                            one[at] = 1;
                            here[at] = 1;
                        }
                    }
                }
            }
            for (a, b) in single[member].iter_mut().zip(one) {
                *a &= b;
            }
        }
        for (a, b) in opaque.iter_mut().zip(here) {
            *a &= b;
        }
    }
    // Store only new seam windows. Removing an optional proof bit is safe;
    // ordinary per-source certificates continue to provide their own coverage.
    let single: Vec<_> = single.iter().map(|mask| integral(mask, w, h)).collect();
    let full = (window * window) as u32;
    erode(
        bounds,
        w,
        h,
        shift,
        window,
        &integral(&opaque, w, h),
        |l, t, r, b| single.iter().any(|p| sum(p, w, l, t, r, b) == full),
    )
}

fn proofs(count: u32, mut next: impl FnMut() -> (u32, Proof)) -> Vec<u8> {
    let mut out = b"HKTCOT01".to_vec();
    out.extend(count.to_le_bytes());
    for _ in 0..count {
        let (id, (origin, size, bits)) = next();
        out.extend(id.to_le_bytes());
        for v in origin {
            out.extend(v.to_le_bytes());
        }
        for v in size {
            out.extend(v.to_le_bytes());
        }
        out.extend((bits.len() as u32).to_le_bytes());
        for v in bits {
            out.extend(v.to_le_bytes());
        }
    }
    out
}

/// The tile proofs for an HKTCIN01 input, as HKTCOT01 (coverage_cert's
/// `main`). The poses are independent, so they run in parallel; the output
/// keeps their order.
pub fn tile_proofs(input: &[u8], shift: u32) -> Vec<u8> {
    use rayon::prelude::*;
    let mut r = Reader {
        bytes: input,
        at: 0,
    };
    assert_eq!(&r.take::<8>(), b"HKTCIN01");
    let count = r.u32();
    assert!(count <= 65535);
    let mut poses = Vec::new();
    for expected in 0..count {
        let id = r.u32();
        assert_eq!(id, expected);
        let n = r.u32() as usize;
        assert!((1..=16).contains(&n));
        let w = r.u16() as usize;
        let h = r.u16() as usize;
        assert!(w > 0 && h > 0 && w <= 252 && h <= 252);
        let phases = (0..n)
            .map(|_| core::array::from_fn(|_| (r.i32(), r.i32())))
            .collect::<Vec<[(i32, i32); 4]>>();
        let words = (0..w * h).map(|_| r.u16()).collect::<Vec<_>>();
        poses.push((id, phases, w, h, words));
    }
    assert_eq!(r.at, r.bytes.len(), "trailing certificate input");
    let done: Vec<(u32, Proof)> = poses
        .par_iter()
        .map(|(id, phases, w, h, words)| (*id, certify_tile(phases, *w, *h, words, shift)))
        .collect();
    let mut it = done.into_iter();
    proofs(count, || it.next().unwrap())
}

/// The group proofs for an HKGPIN01 input, as HKTCOT01 (coverage_groups'
/// `main`).
pub fn group_proofs(input: &[u8]) -> Vec<u8> {
    use rayon::prelude::*;
    let mut r = Reader {
        bytes: input,
        at: 0,
    };
    assert_eq!(&r.take::<8>(), b"HKGPIN01");
    let count = r.u32();
    assert!(count <= 65535);
    let mut poses = Vec::new();
    for expected in 0..count {
        let id = r.u32();
        assert_eq!(id, expected);
        let n = r.u32() as usize;
        let members = r.u32() as usize;
        assert!((1..=256).contains(&n) && (2..=4).contains(&members));
        let phases = (0..n)
            .map(|_| (0..members * 4).map(|_| (r.i32(), r.i32())).collect())
            .collect::<Vec<Vec<_>>>();
        poses.push((id, phases));
    }
    assert_eq!(r.at, r.bytes.len(), "trailing group input");
    let done: Vec<(u32, Proof)> = poses
        .par_iter()
        .map(|(id, phases)| (*id, certify_group(phases, 2)))
        .collect();
    let mut it = done.into_iter();
    proofs(count, || it.next().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> [[(i32, i32); 4]; 1] {
        [[(0, 0), (48, 0), (0, 48), (48, 48)]]
    }
    #[test]
    fn transparent_and_soft_never_certify() {
        for word in [0, 0x8000, 0xffff] {
            assert!(certify_tile(&square(), 2, 2, &[word; 4], 3).2.is_empty());
        }
    }
    #[test]
    fn opaque_zero_rgb_sentinel_certifies() {
        let (o, s, b) = certify_tile(&square(), 2, 2, &[1; 4], 3);
        assert_eq!(o, [0, 0]);
        assert_eq!(s, [4, 4]);
        assert_eq!(b, [0xffff]);
    }
    #[test]
    fn fine_grid_certifies_the_full_nineteen_pixel_window() {
        let (o, s, b) = certify_tile(&square(), 2, 2, &[1; 4], 2);
        assert_eq!(o, [0, 0]);
        assert_eq!(s, [8, 8]);
        assert_eq!(b, [u32::MAX; 2]);
    }
    #[test]
    fn intersection_rejects_missing_geometry() {
        let p = [square()[0], [(0, 0), (16, 0), (0, 16), (16, 16)]];
        assert!(certify_tile(&p, 2, 2, &[1; 4], 3).2.is_empty());
    }
    fn pair() -> Vec<(i32, i32)> {
        vec![
            (0, 0),
            (16, 0),
            (0, 48),
            (16, 48),
            (16, 0),
            (32, 0),
            (16, 48),
            (32, 48),
        ]
    }
    #[test]
    fn union_before_erosion_fills_seam() {
        let c = certify_group(&[pair()], 2);
        assert!(!c.2.is_empty());
        assert!(c.2.iter().any(|&x| x != 0));
    }
    #[test]
    fn disjoint_gap_never_fills() {
        let mut p = pair();
        for q in &mut p[4..] {
            q.0 += 1;
        }
        assert!(certify_group(&[p], 2).2.is_empty());
    }
    #[test]
    fn covered_by_one_member_is_not_new_seam() {
        let mut p = pair();
        p[1].0 = 32;
        p[3].0 = 32;
        assert!(certify_group(&[p], 2).2.is_empty());
    }
    #[test]
    fn every_phase_must_cover_window() {
        let mut p = pair();
        for q in &mut p[4..] {
            q.0 += 1;
        }
        assert!(certify_group(&[pair(), p], 2).2.is_empty());
    }
    #[test]
    fn the_byte_formats_round_trip_an_empty_input() {
        let mut input = b"HKTCIN01".to_vec();
        input.extend(0u32.to_le_bytes());
        assert_eq!(
            tile_proofs(&input, 2),
            [b"HKTCOT01".as_slice(), &[0; 4]].concat()
        );
        let mut input = b"HKGPIN01".to_vec();
        input.extend(0u32.to_le_bytes());
        assert_eq!(
            group_proofs(&input),
            [b"HKTCOT01".as_slice(), &[0; 4]].concat()
        );
    }
}
