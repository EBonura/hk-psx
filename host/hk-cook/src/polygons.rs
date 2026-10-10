//! Split simple polygons into bounded-vertex pieces with an identical union. Ported
//! from host/polygons.py.
//!
//! Guest hazard and checkpoint records hold at most 16 vertices per polygon while
//! source PolygonCollider2D paths may have many more. Ear clipping triangulates the
//! polygon (with the exact arithmetic Python's `Fraction` gave it), the dual tree of
//! the triangulation is partitioned into connected groups of at most limit-2
//! triangles, and each group's outer boundary is one simple polygon.
//!
//! Which piece starts where, and the order the pieces come out in, followed from the
//! iteration order of Python sets of small ints and int pairs, so `PySet` models
//! CPython's open-addressing table (set_add_entry, set_merge, set_table_resize) and
//! the tuple hash. Nothing is ever removed from those sets, so no dummy entries.

use crate::common::{err, Result};
use std::cmp::Ordering;

/// `POLYGON_VERTEX_LIMIT`.
pub const POLYGON_VERTEX_LIMIT: usize = 16;

// --- exact arithmetic -------------------------------------------------------------------

/// A signed arbitrary-size integer (magnitude in little-endian u32 limbs).
#[derive(Clone, Debug, PartialEq)]
struct Big {
    neg: bool,
    mag: Vec<u32>,
}

impl Big {
    fn zero() -> Big {
        Big {
            neg: false,
            mag: Vec::new(),
        }
    }

    fn from_u128(v: u128, neg: bool) -> Big {
        let mut mag = Vec::new();
        let mut v = v;
        while v != 0 {
            mag.push(v as u32);
            v >>= 32;
        }
        Big {
            neg: neg && !mag.is_empty(),
            mag,
        }
    }

    fn shl(mut self, bits: u32) -> Big {
        if self.mag.is_empty() {
            return self;
        }
        let (limbs, rem) = ((bits / 32) as usize, bits % 32);
        let mut out = vec![0u32; limbs];
        let mut carry = 0u32;
        for &w in &self.mag {
            if rem == 0 {
                out.push(w);
            } else {
                out.push((w << rem) | carry);
                carry = w >> (32 - rem);
            }
        }
        if carry != 0 {
            out.push(carry);
        }
        self.mag = out;
        self
    }

    fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
        if a.len() != b.len() {
            return a.len().cmp(&b.len());
        }
        for i in (0..a.len()).rev() {
            if a[i] != b[i] {
                return a[i].cmp(&b[i]);
            }
        }
        Ordering::Equal
    }

    fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut out = Vec::with_capacity(a.len().max(b.len()) + 1);
        let mut carry = 0u64;
        for i in 0..a.len().max(b.len()) {
            let s = *a.get(i).unwrap_or(&0) as u64 + *b.get(i).unwrap_or(&0) as u64 + carry;
            out.push(s as u32);
            carry = s >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        out
    }

    /// a - b for |a| >= |b|.
    fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut out = Vec::with_capacity(a.len());
        let mut borrow = 0i64;
        for (i, &ai) in a.iter().enumerate() {
            let mut d = ai as i64 - *b.get(i).unwrap_or(&0) as i64 - borrow;
            borrow = 0;
            if d < 0 {
                d += 1 << 32;
                borrow = 1;
            }
            out.push(d as u32);
        }
        while out.last() == Some(&0) {
            out.pop();
        }
        out
    }

    fn add(&self, other: &Big) -> Big {
        if self.neg == other.neg {
            return Big {
                neg: self.neg && !self.mag.is_empty(),
                mag: Big::add_mag(&self.mag, &other.mag),
            };
        }
        match Big::cmp_mag(&self.mag, &other.mag) {
            Ordering::Equal => Big::zero(),
            Ordering::Greater => Big {
                neg: self.neg,
                mag: Big::sub_mag(&self.mag, &other.mag),
            },
            Ordering::Less => Big {
                neg: other.neg,
                mag: Big::sub_mag(&other.mag, &self.mag),
            },
        }
    }

    fn negate(&self) -> Big {
        Big {
            neg: !self.neg && !self.mag.is_empty(),
            mag: self.mag.clone(),
        }
    }

    fn sub(&self, other: &Big) -> Big {
        self.add(&other.negate())
    }

    fn mul(&self, other: &Big) -> Big {
        if self.mag.is_empty() || other.mag.is_empty() {
            return Big::zero();
        }
        let mut out = vec![0u32; self.mag.len() + other.mag.len()];
        for (i, &a) in self.mag.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.mag.iter().enumerate() {
                let t = out[i + j] as u64 + a as u64 * b as u64 + carry;
                out[i + j] = t as u32;
                carry = t >> 32;
            }
            let mut k = i + other.mag.len();
            while carry != 0 {
                let t = out[k] as u64 + carry;
                out[k] = t as u32;
                carry = t >> 32;
                k += 1;
            }
        }
        while out.last() == Some(&0) {
            out.pop();
        }
        Big {
            neg: self.neg != other.neg,
            mag: out,
        }
    }

    /// -1, 0 or 1.
    fn sign(&self) -> i32 {
        if self.mag.is_empty() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }
}

/// `value = mant * 2^exp`, exactly, with the sign; zero has no exponent.
fn decompose(v: f64) -> Option<(u64, i32, bool)> {
    if v == 0.0 {
        return None;
    }
    let bits = v.to_bits();
    let (exp, frac) = (((bits >> 52) & 0x7ff) as i32, bits & ((1u64 << 52) - 1));
    let (mant, e) = if exp == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp - 1075)
    };
    Some((mant, e, v < 0.0))
}

/// Points as integers scaled by a common power of two, so every cross product the
/// triangulation tests is an exact sign over integers.
struct Exact {
    pts: Vec<(Big, Big)>,
}

impl Exact {
    fn new(points: &[(f64, f64)]) -> Result<Exact> {
        if points.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
            return err("polygon has a non-finite vertex");
        }
        let min_exp = points
            .iter()
            .flat_map(|p| [p.0, p.1])
            .filter_map(decompose)
            .map(|d| d.1)
            .min()
            .unwrap_or(0);
        let scaled = |v: f64| match decompose(v) {
            None => Big::zero(),
            Some((m, e, neg)) => Big::from_u128(m as u128, neg).shl((e - min_exp) as u32),
        };
        Ok(Exact {
            pts: points.iter().map(|p| (scaled(p.0), scaled(p.1))).collect(),
        })
    }

    /// `_area2(a, b, c)`: twice the signed area, as an exact integer (scaled).
    fn area2(&self, a: usize, b: usize, c: usize) -> Big {
        let (pa, pb, pc) = (&self.pts[a], &self.pts[b], &self.pts[c]);
        pb.0.sub(&pa.0)
            .mul(&pc.1.sub(&pa.1))
            .sub(&pb.1.sub(&pa.1).mul(&pc.0.sub(&pa.0)))
    }

    /// `_area2((0, 0), b, c)`.
    fn area_from_origin(&self, b: usize, c: usize) -> Big {
        let (pb, pc) = (&self.pts[b], &self.pts[c]);
        pb.0.mul(&pc.1).sub(&pb.1.mul(&pc.0))
    }

    fn same(&self, a: usize, b: usize) -> bool {
        self.pts[a] == self.pts[b]
    }

    /// `_inside_triangle(p, a, b, c)`.
    fn inside(&self, p: usize, a: usize, b: usize, c: usize) -> bool {
        let d = [
            self.area2(a, b, p).sign(),
            self.area2(b, c, p).sign(),
            self.area2(c, a, p).sign(),
        ];
        !(d.iter().any(|&v| v < 0) && d.iter().any(|&v| v > 0))
    }
}

/// `triangulate(points)`: ear-clip a simple polygon into counter-clockwise index triangles.
pub fn triangulate(points: &[(f64, f64)]) -> Result<Vec<[usize; 3]>> {
    let ex = Exact::new(points)?;
    let n = points.len();
    // `pts[i] != pts[i - 1]`, where i - 1 wraps to the last point for i = 0.
    let mut idx: Vec<usize> = (0..n).filter(|&i| !ex.same(i, (i + n - 1) % n)).collect();
    if idx.len() < 3 {
        return err("polygon needs three distinct vertices");
    }
    let mut area = Big::zero();
    for i in 0..idx.len() {
        area = area.add(&ex.area_from_origin(idx[i], idx[(i + 1) % idx.len()]));
    }
    match area.sign() {
        0 => return err("degenerate polygon"),
        -1 => idx.reverse(),
        _ => {}
    }
    let mut triangles = Vec::new();
    while idx.len() > 3 {
        let n = idx.len();
        let mut clipped = false;
        for k in 0..n {
            let (a, b, c) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let cross = ex.area2(a, b, c).sign();
            if cross < 0 {
                continue; // reflex vertex
            }
            if cross == 0 {
                idx.remove(k); // collinear vertex adds no area
                clipped = true;
                break;
            }
            if idx
                .iter()
                .any(|&o| o != a && o != b && o != c && ex.inside(o, a, b, c))
            {
                continue;
            }
            triangles.push([a, b, c]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            return err("polygon is not simple");
        }
    }
    if ex.area2(idx[0], idx[1], idx[2]).sign() > 0 {
        triangles.push([idx[0], idx[1], idx[2]]);
    }
    Ok(triangles)
}

use crate::pyset::PySet;

/// CPython's tuple hash (xxHash-based, 3.8+) of a pair of small non-negative ints.
fn pair_hash(a: usize, b: usize) -> u64 {
    const P1: u64 = 11400714785074694791;
    const P2: u64 = 14029467366897019727;
    const P5: u64 = 2870177450012600261;
    let mut acc = P5;
    for lane in [a as u64, b as u64] {
        acc = acc.wrapping_add(lane.wrapping_mul(P2));
        acc = acc.rotate_left(31);
        acc = acc.wrapping_mul(P1);
    }
    acc = acc.wrapping_add(2 ^ (P5 ^ 3527539));
    if acc == u64::MAX {
        1546275796
    } else {
        acc
    }
}

// --- bounded_polygons ------------------------------------------------------------------

/// `bounded_polygons(points, limit)`: pieces of at most `limit` vertices whose union is `points`.
pub fn bounded_polygons(points: &[(f64, f64)], limit: usize) -> Result<Vec<Vec<(f64, f64)>>> {
    if points.len() <= limit {
        return Ok(vec![points.to_vec()]);
    }
    let triangles = triangulate(points)?;
    // `edges`: unordered vertex pair -> the triangles on it, in first-seen order.
    let mut edges: Vec<((usize, usize), Vec<usize>)> = Vec::new();
    for (t, tri) in triangles.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            match edges.iter_mut().find(|e| e.0 == key) {
                Some(e) => e.1.push(t),
                None => edges.push((key, vec![t])),
            }
        }
    }
    let mut neighbours: Vec<PySet<usize>> = (0..triangles.len()).map(|_| PySet::new()).collect();
    for (_, owners) in &edges {
        for &t in owners {
            for &o in owners.iter().filter(|&&o| o != t) {
                neighbours[t].add(o, o as u64);
            }
        }
    }
    // The dual tree, breadth first from triangle 0.
    let mut parent: Vec<Option<Option<usize>>> = vec![None; triangles.len()];
    parent[0] = Some(None);
    let mut order = vec![0usize];
    let mut at = 0;
    while at < order.len() {
        let t = order[at];
        at += 1;
        for &n in neighbours[t].iter() {
            if parent[n].is_none() {
                parent[n] = Some(Some(t));
                order.push(n);
            }
        }
    }
    if order.len() != triangles.len() {
        return err("triangulation is not connected");
    }
    let mut group: Vec<usize> = (0..triangles.len()).collect();
    let mut members: Vec<Option<PySet<usize>>> = (0..triangles.len())
        .map(|t| {
            let mut s = PySet::new();
            s.add(t, t as u64);
            Some(s)
        })
        .collect();
    for &t in order.iter().rev() {
        let Some(Some(p)) = parent[t] else { continue };
        let (mine, theirs) = (group[t], group[p]);
        let (a, b) = (
            members[mine].as_ref().unwrap().len(),
            members[theirs].as_ref().unwrap().len(),
        );
        if a + b <= limit - 2 {
            let moved = members[mine].take().unwrap();
            for &m in moved.iter() {
                group[m] = theirs;
            }
            members[theirs].as_mut().unwrap().merge(&moved);
        }
    }
    let mut pieces = Vec::new();
    for tris in members.iter().flatten() {
        let mut directed: PySet<(usize, usize)> = PySet::new();
        for &t in tris.iter() {
            for k in 0..3 {
                let pair = (triangles[t][k], triangles[t][(k + 1) % 3]);
                directed.add(pair, pair_hash(pair.0, pair.1));
            }
        }
        let mut boundary: Vec<(usize, usize)> = Vec::new();
        for &(a, b) in directed.iter() {
            if !directed.contains(&(b, a), pair_hash(b, a)) {
                match boundary.iter_mut().find(|x| x.0 == a) {
                    Some(slot) => slot.1 = b,
                    None => boundary.push((a, b)),
                }
            }
        }
        if boundary.len() != tris.len() + 2 {
            return err("piece boundary is not simple");
        }
        let next = |a: usize| boundary.iter().find(|x| x.0 == a).map(|x| x.1);
        let start = boundary[0].0;
        let mut cycle = vec![start];
        while next(*cycle.last().unwrap()) != Some(start) {
            match next(*cycle.last().unwrap()) {
                Some(n) => cycle.push(n),
                // Python indexes the boundary dict here: a missing key is a KeyError, not a ValueError.
                None => return err("KeyError: piece boundary has no continuation"),
            }
            if cycle.len() > boundary.len() {
                return err("piece boundary is not one cycle");
            }
        }
        if cycle.len() != boundary.len() {
            return err("piece boundary is not one cycle");
        }
        pieces.push(cycle.iter().map(|&i| points[i]).collect());
    }
    Ok(pieces)
}
