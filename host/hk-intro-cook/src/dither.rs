//! The picture's reduction to the 5 bits per channel the MDEC's 15-bit output keeps.
//!
//! The MDEC truncates to 5 bits with no dither of its own, so what the encoder is given has to carry the
//! dither: each frame goes through a tone correction (the measured transfer of the whole chain, [`Transfer`])
//! and then to 5 bits against an ordered Bayer threshold, the same for the three channels (so the noise is in
//! the picture's brightness, not its colour) and offset by a different amount for every picture ([`phase`]).
//! Blue noise (void and cluster, 16x16 tile) was tried too: at the same dead zone the busiest frame needs
//! 422,560 B of the scene arena's 415,000 (the Bayer tile 404,000), for a temporal change per picture of
//! 2.2 against 2.6; at a dead zone that fits it has the same banding as the Bayer tile.
//! A dead zone ([`DEAD`]) keeps a tone near a level on that level instead of speckling two; past 0.2 the
//! shallow gradients fall back into contour patches (the teal oval of the Intro's frame 79 returns at 0.3).

/// Where in the matrix picture `n` starts: the matrix is offset by a different amount for every picture, in a
/// low-discrepancy sequence (the plastic number's R2 sequence), so that no pixel keeps the same threshold from
/// one picture to the next. The pattern is then fine grain at 15 pictures a second instead of a mesh fixed
/// to the screen (the shower door that a fixed matrix makes over a moving picture).
pub fn phase(n: usize) -> (usize, usize) {
    let fraction = |step: f64| (n as f64 * step).fract();
    ((fraction(0.754_877_666_2) * SIDE as f64) as usize, (fraction(0.569_840_291_0) * SIDE as f64) as usize)
}

/// The ordered Bayer matrix of side [`SIDE`] as thresholds.
pub struct Bayer {
    rank: Vec<u32>,
}

/// Side of the matrix (a power of two).
pub const SIDE: usize = 16;

impl Bayer {
    pub fn new() -> Bayer {
        Bayer { rank: bayer(SIDE) }
    }

    /// The threshold at pixel (`x`, `y`), in [0, 1): every threshold of the matrix appears once per tile.
    pub fn threshold(&self, x: usize, y: usize) -> f64 {
        (f64::from(self.rank[(y % SIDE) * SIDE + x % SIDE]) + 0.5) / (SIDE * SIDE) as f64
    }
}


/// An ordered Bayer matrix of side `size` (a power of two), thresholds in rank order.
pub fn bayer(size: usize) -> Vec<u32> {
    let mut m = vec![0u32];
    let mut n = 1;
    while n < size {
        let mut next = vec![0u32; 4 * n * n];
        for y in 0..n {
            for x in 0..n {
                let v = 4 * m[y * n + x];
                next[y * 2 * n + x] = v;
                next[y * 2 * n + x + n] = v + 2;
                next[(y + n) * 2 * n + x] = v + 3;
                next[(y + n) * 2 * n + x + n] = v + 1;
            }
        }
        m = next;
        n *= 2;
    }
    m
}

/// as measured points (source value, value the PlayStation shows): the tone correction is its inverse, so
/// that the picture shown has the clip's levels instead of the chain's ~5% extra gain and its green lift.
pub struct Transfer {
    lut: [u8; 256],
}

impl Transfer {
    /// The inverse of `measured` (points in rising order of both values; (0, 0) is implied before the first)
    /// as a table of source value to value to encode.
    pub fn inverse_of(measured: &[(f64, f64)]) -> Transfer {
        let mut points = vec![(0.0, 0.0)];
        points.extend_from_slice(measured);
        let mut lut = [0u8; 256];
        for (x, out) in lut.iter_mut().enumerate() {
            let want = x as f64;
            // Past the last measured point (the chain was not measured where the clip has no pixels) the
            // curve goes on with the slope of its last five points.
            let tail = points.len().saturating_sub(6);
            let (s_end, o_end) = points[points.len() - 1];
            let slope = (o_end - points[tail].1) / (s_end - points[tail].0).max(1.0);
            let mut value = s_end + (want - o_end) / slope.max(0.5);
            for w in points.windows(2) {
                let ((s0, o0), (s1, o1)) = (w[0], w[1]);
                if want <= o1 {
                    let t = if o1 > o0 { ((want - o0) / (o1 - o0)).clamp(0.0, 1.0) } else { 1.0 };
                    value = s0 + t * (s1 - s0);
                    break;
                }
            }
            *out = value.round().clamp(0.0, 255.0) as u8;
        }
        Transfer { lut }
    }

    pub fn apply(&self, v: u8) -> u8 {
        self.lut[usize::from(v)]
    }
}

/// The dead zone [`five_bits`] is given: 0.2 of a level at each end. Measured on the Intro's flat fog (std of a 16x16
/// block against the clip's 0.5): 3.98 with none, 3.36 at 0.15, 2.97 at 0.2, 2.63 at 0.25; the shallow gradients of
/// frames 79 and 180 show no contour at 0.2 and the oval at 0.3.
pub const DEAD: f64 = 0.2;

/// `v` (0..=255) reduced to 5 bits against `threshold` and expanded back by bit replication, the way the
/// display expands the MDEC's 5 bits, so that the mean of the dithered values is `v` (when `dead` is 0).
///
/// `dead` is the share of a level, at each end, within which the value is not dithered but taken to the nearer
/// level: a flat area whose tone lands near a level then keeps one level instead of speckling two, and only
/// a tone between levels (the middle 1 - 2 `dead` of the span) mixes them, over a steeper ramp.
pub fn five_bits(v: u8, threshold: f64, dead: f64) -> u8 {
    let level = f64::from(v) * 31.0 / 255.0;
    let (base, fraction) = (level.floor(), level.fract());
    let mixed = if dead > 0.0 { ((fraction - 0.5) / (1.0 - 2.0 * dead) + 0.5).clamp(0.0, 1.0) } else { fraction };
    let q = ((base + f64::from(u8::from(mixed + threshold >= 1.0))) as i32).clamp(0, 31) as u8;
    (q << 3) | (q >> 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_uses_every_rank_once() {
        let n = Bayer::new();
        let mut seen = vec![false; SIDE * SIDE];
        for &r in &n.rank {
            assert!(!seen[r as usize], "{r} twice");
            seen[r as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn every_picture_starts_somewhere_else_in_the_matrix() {
        let phases: Vec<(usize, usize)> = (0..64).map(phase).collect();
        assert!(phases.iter().all(|&(x, y)| x < SIDE && y < SIDE));
        // Neighbouring pictures differ, and over a few seconds most of the 256 offsets are used.
        assert!(phases.windows(2).all(|w| w[0] != w[1]));
        let distinct: std::collections::HashSet<_> = phases.iter().collect();
        assert!(distinct.len() > 48, "{}", distinct.len());
    }

    #[test]
    fn a_dithered_flat_area_keeps_its_mean() {
        let n = Bayer::new();
        for v in [3u8, 17, 100, 128, 200, 250] {
            let mut sum = 0u64;
            for y in 0..SIDE {
                for x in 0..SIDE {
                    sum += u64::from(five_bits(v, n.threshold(x, y), 0.0));
                }
            }
            let mean = sum as f64 / (SIDE * SIDE) as f64;
            assert!((mean - f64::from(v)).abs() < 1.0, "{v}: {mean}");
        }
    }

    #[test]
    fn the_inverse_of_a_five_percent_gain_takes_five_percent_off() {
        let m: Vec<(f64, f64)> = (1..=51).map(|i| (f64::from(i) * 5.0, f64::from(i) * 5.0 * 1.05)).collect();
        let t = Transfer::inverse_of(&m);
        assert_eq!(t.apply(0), 0);
        assert!((i32::from(t.apply(105)) - 100).abs() <= 1);
        assert!((i32::from(t.apply(210)) - 200).abs() <= 1);
        assert!((i32::from(t.apply(255)) - 243).abs() <= 2);
        // Past the points: a table that stops at 100 (shown 105) goes on at the same slope.
        let short = Transfer::inverse_of(&m[..20]);
        assert!((i32::from(short.apply(210)) - 200).abs() <= 2, "{}", short.apply(210));
    }
}
