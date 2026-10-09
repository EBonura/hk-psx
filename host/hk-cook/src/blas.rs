//! The two matrix products host/quantize.py takes through numpy: `(2 * x) @ c.T` and
//! `x @ LUMA`, rounded as numpy's BLAS rounds them.
//!
//! numpy routes both through the platform BLAS (Accelerate on macOS arm64 wheels), and
//! the rounding of a three-term dot product depends on which kernel the BLAS picks for
//! the shape, so the portable fallback is only an approximation of it. On macOS the
//! same `cblas_dgemm` / `cblas_dgemv` entry points are called with the arguments numpy
//! passes, which reproduces numpy's results exactly there.

#[cfg(target_os = "macos")]
mod accelerate {
    #[link(name = "Accelerate", kind = "framework")]
    extern "C" {
        pub fn cblas_dgemm(
            order: i32,
            ta: i32,
            tb: i32,
            m: i32,
            n: i32,
            k: i32,
            alpha: f64,
            a: *const f64,
            lda: i32,
            b: *const f64,
            ldb: i32,
            beta: f64,
            c: *mut f64,
            ldc: i32,
        );
        pub fn cblas_dgemv(
            order: i32,
            ta: i32,
            m: i32,
            n: i32,
            alpha: f64,
            a: *const f64,
            lda: i32,
            x: *const f64,
            incx: i32,
            beta: f64,
            y: *mut f64,
            incy: i32,
        );
    }
    pub const ROW_MAJOR: i32 = 101;
    pub const NO_TRANS: i32 = 111;
    pub const TRANS: i32 = 112;
}

fn flatten(rows: &[[f64; 3]]) -> Vec<f64> {
    rows.iter().flatten().copied().collect()
}

/// `a @ c.T` for (n, 3) and (k, 3) arrays: an (n, k) array in row-major order.
pub fn matmul_abt(a: &[[f64; 3]], c: &[[f64; 3]]) -> Vec<f64> {
    let (n, k) = (a.len(), c.len());
    let mut out = vec![0.0; n * k];
    if n == 0 || k == 0 {
        return out;
    }
    #[cfg(target_os = "macos")]
    {
        use accelerate::*;
        let (fa, fc) = (flatten(a), flatten(c));
        // numpy: a lone output column or a lone row is a matrix-vector product, anything else a gemm.
        unsafe {
            if k == 1 {
                cblas_dgemv(
                    ROW_MAJOR,
                    NO_TRANS,
                    n as i32,
                    3,
                    1.0,
                    fa.as_ptr(),
                    3,
                    fc.as_ptr(),
                    1,
                    0.0,
                    out.as_mut_ptr(),
                    1,
                );
            } else if n == 1 {
                cblas_dgemv(
                    ROW_MAJOR,
                    NO_TRANS,
                    k as i32,
                    3,
                    1.0,
                    fc.as_ptr(),
                    3,
                    fa.as_ptr(),
                    1,
                    0.0,
                    out.as_mut_ptr(),
                    1,
                );
            } else {
                cblas_dgemm(
                    ROW_MAJOR,
                    NO_TRANS,
                    TRANS,
                    n as i32,
                    k as i32,
                    3,
                    1.0,
                    fa.as_ptr(),
                    3,
                    fc.as_ptr(),
                    3,
                    0.0,
                    out.as_mut_ptr(),
                    k as i32,
                );
            }
        }
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        for i in 0..n {
            for j in 0..k {
                out[i * k + j] = crate::numpy_math::matmul_entry(a[i], c[j], k, j);
            }
        }
        out
    }
}

/// `x @ v` for an (n, 3) array and a length-3 vector.
pub fn matvec(x: &[[f64; 3]], v: [f64; 3]) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![0.0; n];
    if n == 0 {
        return out;
    }
    #[cfg(target_os = "macos")]
    {
        use accelerate::*;
        let fx = flatten(x);
        unsafe {
            cblas_dgemv(
                ROW_MAJOR,
                NO_TRANS,
                n as i32,
                3,
                1.0,
                fx.as_ptr(),
                3,
                v.as_ptr(),
                1,
                0.0,
                out.as_mut_ptr(),
                1,
            )
        };
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        for (o, r) in out.iter_mut().zip(x) {
            *o = crate::numpy_math::dot3_plain(*r, v);
        }
        out
    }
}
