//! Python's `repr(float)`: the shortest round-trip digits, fixed notation for
//! decimal exponents in [-4, 16), otherwise `d.ddde+XX`.

pub fn repr(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // Rust's LowerExp prints the shortest round-trip digits: "d.ddde-N".
    // Of the strings that short, Python's dtoa takes the one nearest the
    // exact value (ties to even), which the shortest search need not: for
    // 226.350006103515625 it gives ...563 where Python gives ...562. The exact
    // formatter rounds to that length correctly, so keep it when it still
    // reads back as `v`.
    let shortest = format!("{v:e}");
    let n = shortest
        .split_once('e')
        .unwrap()
        .0
        .chars()
        .filter(char::is_ascii_digit)
        .count();
    let exact = format!("{v:.prec$e}", prec = n - 1);
    let e = if exact.parse::<f64>() == Ok(v) {
        exact
    } else {
        shortest
    };
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if neg { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let point = exp + 1; // digits before the decimal point
        let s = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        format!("{sign}{s}")
    } else {
        let m = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.clone()
        };
        format!(
            "{sign}{m}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}

/// CPython's `math.hypot(x, y)` (3.10 and later): the Euclidean norm computed with an extended-precision
/// accumulation, which differs from a C library `hypot` in the last place for some inputs.
pub fn hypot(a: f64, b: f64) -> f64 {
    let (x, y) = (a.abs(), b.abs());
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let max = x.max(y);
    if max == 0.0 {
        return max;
    }
    // frexp: max = m * 2^e with 0.5 <= m < 1.
    let bits = max.to_bits();
    let exp_field = ((bits >> 52) & 0x7ff) as i64;
    if exp_field == 0 {
        // Subnormal inputs: no scaling trick; fall back to the library.
        return x.hypot(y);
    }
    let e = exp_field - 1022;
    let scale = f64::from_bits(((1023 - e) as u64) << 52);
    let mul = |p: f64, q: f64| -> (f64, f64) {
        let hi = p * q;
        (hi, p.mul_add(q, -hi))
    };
    let fast_sum = |p: f64, q: f64| -> (f64, f64) {
        let s = p + q;
        (s, (p - s) + q)
    };
    let (mut csum, mut frac1, mut frac2) = (1.0f64, 0.0f64, 0.0f64);
    for v in [x, y] {
        let v = v * scale;
        let (phi, plo) = mul(v, v);
        let (shi, slo) = fast_sum(csum, phi);
        csum = shi;
        frac1 += plo;
        frac2 += slo;
    }
    let mut h = (csum - 1.0 + (frac1 + frac2)).sqrt();
    let (phi, plo) = mul(-h, h);
    let (shi, slo) = fast_sum(csum, phi);
    csum = shi;
    frac1 += plo;
    frac2 += slo;
    let x2 = csum - 1.0 + (frac1 + frac2);
    h += x2 / (2.0 * h);
    h / scale
}

#[cfg(test)]
mod tests {
    use super::repr;
    #[test]
    fn matches_python() {
        for (v, s) in [
            (0.1, "0.1"),
            (1.0, "1.0"),
            (1e-5, "1e-05"),
            (1e16, "1e+16"),
            (123456.789, "123456.789"),
            (0.0001, "0.0001"),
            (-2.5e-7, "-2.5e-07"),
            (1.5e300, "1.5e+300"),
            (9999999999999998.0, "9999999999999998.0"),
            (0.20000000298023224, "0.20000000298023224"),
            (226.350006103515625, "226.35000610351562"),
        ] {
            assert_eq!(repr(v), s);
        }
    }
}
