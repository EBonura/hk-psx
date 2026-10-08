use super::segments_intersect;
type Point = [i32; 2];

// Independent exact rational line-parameter reference. i128 keeps test arithmetic
// separate from the production Q16.16/i64 orientation implementation.
fn reference(a: Point, b: Point, c: Point, d: Point) -> bool {
    let sub = |p: Point, q: Point| [p[0] as i128 - q[0] as i128, p[1] as i128 - q[1] as i128];
    let det = |p: [i128; 2], q: [i128; 2]| p[0] * q[1] - p[1] * q[0];
    let r = sub(b, a);
    let s = sub(d, c);
    let offset = sub(c, a);
    let denominator = det(r, s);
    if denominator != 0 {
        let t = det(offset, s);
        let u = det(offset, r);
        let within = |n| {
            if denominator > 0 {
                n >= 0 && n <= denominator
            } else {
                n <= 0 && n >= denominator
            }
        };
        return within(t) && within(u);
    }
    if r == [0, 0] && s == [0, 0] {
        return a == c;
    }
    let direction = if r == [0, 0] { s } else { r };
    if det(offset, direction) != 0 {
        return false;
    }
    // Collinear projections along a nonzero axis suffice, including point edges.
    let axis = if direction[0] != 0 { 0 } else { 1 };
    a[axis].min(b[axis]) <= c[axis].max(d[axis]) && c[axis].min(d[axis]) <= a[axis].max(b[axis])
}

fn check(a: Point, b: Point, c: Point, d: Point) {
    assert_eq!(
        segments_intersect(a, b, c, d),
        reference(a, b, c, d),
        "{a:?}->{b:?} / {c:?}->{d:?}"
    );
}

#[test]
fn exhaustive_small_lattice_including_all_endpoint_orders_and_degeneracies() {
    let mut points = [[0; 2]; 25];
    for (i, p) in points.iter_mut().enumerate() {
        *p = [(i % 5) as i32 - 2, (i / 5) as i32 - 2];
    }
    for &a in &points {
        for &b in &points {
            for &c in &points {
                for &d in &points {
                    check(a, b, c, d);
                }
            }
        }
    }
}

#[test]
fn touching_collinear_parallel_and_one_q16_bit_separation_at_large_coordinates() {
    let unit = 65536;
    let fixtures = [
        (
            [0, 0],
            [400 * unit, 400 * unit],
            [200 * unit, 200 * unit],
            [500 * unit, 500 * unit],
            true,
        ),
        (
            [0, 0],
            [400 * unit, 400 * unit],
            [400 * unit, 400 * unit],
            [500 * unit, 0],
            true,
        ),
        (
            [0, 0],
            [400 * unit, 400 * unit],
            [0, 1],
            [400 * unit, 400 * unit + 1],
            false,
        ),
        (
            [0, 0],
            [0, 400 * unit],
            [0, 400 * unit + 1],
            [0, 500 * unit],
            false,
        ),
        (
            [0, 0],
            [0, 400 * unit],
            [0, 200 * unit],
            [0, 200 * unit],
            true,
        ),
        (
            [0, 0],
            [0, 400 * unit],
            [1, 200 * unit],
            [1, 200 * unit],
            false,
        ),
        (
            [0, 0],
            [400 * unit, 400 * unit],
            [0, 400 * unit],
            [400 * unit, 0],
            true,
        ),
    ];
    for (a, b, c, d, expected) in fixtures {
        for offset in [-512 * unit, i32::MIN, i32::MAX - 512 * unit] {
            let shift = |p: Point| [p[0] + offset, p[1] + offset];
            let (a, b, c, d) = (shift(a), shift(b), shift(c), shift(d));
            assert_eq!(reference(a, b, c, d), expected);
            for (a, b, c, d) in [(a, b, c, d), (b, a, c, d), (a, b, d, c), (c, d, a, b)] {
                check(a, b, c, d);
            }
        }
    }
}

#[test]
fn deterministic_full_room_q16_range_matches_rational_reference() {
    let mut seed = 0x68b39d21u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % (1024 * 65536 + 1)) as i32 - 512 * 65536
    };
    for i in 0..50_000 {
        let a = [next(), next()];
        let b = [next(), next()];
        let c = if i % 3 == 0 { a } else { [next(), next()] };
        let d = if i % 5 == 0 { c } else { [next(), next()] };
        check(a, b, c, d);
        check(b, a, c, d);
        check(c, d, a, b);
    }
}
