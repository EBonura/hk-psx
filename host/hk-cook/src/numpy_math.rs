//! The float reductions host/quantize.py leans on, in the order numpy performs them, so
//! that rounding agrees: pairwise summation, short row sums, weighted averages and the
//! three-term dot products behind `@`.

/// numpy's `pairwise_sum` over a contiguous run, as `np.sum` of a 1-D array uses it.
pub fn pairwise_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = 0.0;
        for &v in a {
            res += v;
        }
        res
    } else if n <= 128 {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
    }
}

/// `np.sum(a)` of a 1-D float array.
pub fn sum(a: &[f64]) -> f64 {
    pairwise_sum(a)
}

/// `a.sum(1)` over three columns.
pub fn row_sum3(r: [f64; 3]) -> f64 {
    (r[0] + r[1]) + r[2]
}

/// `np.average(x, axis=0, weights=w)` for an (n, 3) array.
pub fn average3(x: &[[f64; 3]], w: &[f64]) -> [f64; 3] {
    let scl = sum(w);
    let mut acc = [0.0f64; 3];
    for (i, row) in x.iter().enumerate() {
        for c in 0..3 {
            let v = row[c] * w[i];
            if i == 0 {
                acc[c] = v;
            } else {
                acc[c] += v;
            }
        }
    }
    [acc[0] / scl, acc[1] / scl, acc[2] / scl]
}

/// Exact `a * b + c` with a single rounding.
pub fn fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}

/// The fused chain `fma(a2, b2, fma(a1, b1, a0 * b0))`.
pub fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    fma(a[2], b[2], fma(a[1], b[1], a[0] * b[0]))
}

/// The blocked kernel's other rounding: `fma(a2, b2, a0 * b0 + a1 * b1)`.
pub fn dot3_blocked(a: [f64; 3], b: [f64; 3]) -> f64 {
    fma(a[2], b[2], a[0] * b[0] + a[1] * b[1])
}

/// Output column `j` of `x @ c.T` for a `k`-row `c`, rounded as the BLAS used by numpy rounds it
/// (observed: a single column is a plain dot, two or three columns use the fused chain, four
/// to seven put the first four columns in a block with the other rounding, eight and up are chains).
pub fn matmul_entry(a: [f64; 3], b: [f64; 3], k: usize, j: usize) -> f64 {
    match k {
        1 => dot3_plain(a, b),
        2 | 3 => dot3(a, b),
        4..=7 if j < 4 => dot3_blocked(a, b),
        _ => dot3(a, b),
    }
}

/// `x @ LUMA` for one row, plain left to right.
pub fn dot3_plain(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
