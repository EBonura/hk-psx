//! Separable filtered resizing and bilinear affine sampling of 8-bit images.
//!
//! Resizing runs a horizontal pass and then a vertical pass (the other way
//! round for very tall images that shrink), each an independent 1-D
//! convolution whose kernel is the chosen filter stretched by the shrink
//! factor and normalised to unit sum. Weights are held in fixed
//! point and each pass rounds back to 8 bits. Colour images with alpha are
//! premultiplied before either operation and divided back out afterwards.

use crate::{div255, Image, Mode};

/// Fixed-point fraction bits of a weight.
const WEIGHT_BITS: u32 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Box,
    Bilinear,
    Lanczos,
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

impl Filter {
    /// Half-width of the kernel in output-scale pixels.
    fn support(self) -> f64 {
        match self {
            Filter::Box => 0.5,
            Filter::Bilinear => 1.0,
            Filter::Lanczos => 3.0,
        }
    }

    /// Kernel value at distance `x` from the centre; the box covers (-0.5, 0.5].
    fn eval(self, x: f64) -> f64 {
        match self {
            Filter::Box => {
                if x > -0.5 && x <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Filter::Bilinear => {
                let x = x.abs();
                if x < 1.0 {
                    1.0 - x
                } else {
                    0.0
                }
            }
            Filter::Lanczos => {
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
        }
    }
}

/// The taps of one output sample: first source index and fixed-point weights.
struct Taps {
    first: usize,
    weights: Vec<i32>,
}

/// Taps for every output sample of a pass from `n_in` samples to `n_out`.
fn taps(n_in: usize, n_out: usize, filter: Filter) -> Vec<Taps> {
    let scale = n_in as f64 / n_out as f64;
    let stretch = scale.max(1.0);
    let support = filter.support() * stretch;
    (0..n_out)
        .map(|o| {
            let center = (o as f64 + 0.5) * scale;
            // Source pixels whose centres lie in (center - support, center + support].
            let first = (center - support + 0.5).floor().max(0.0) as usize;
            let end = ((center + support + 0.5).floor() as usize).min(n_in);
            let raw: Vec<f64> = (first..end).map(|i| filter.eval((i as f64 + 0.5 - center) / stretch)).collect();
            let sum: f64 = raw.iter().sum();
            let weights = raw
                .iter()
                .map(|&w| {
                    let w = if sum != 0.0 { w / sum } else { w };
                    // Round half away from zero.
                    (w * (1u64 << WEIGHT_BITS) as f64).round() as i32
                })
                .collect();
            Taps { first, weights }
        })
        .collect()
}

fn to_u8(acc: i64) -> u8 {
    (acc >> WEIGHT_BITS).clamp(0, 255) as u8
}

/// One resampling pass along x (`horizontal`) or y.
fn pass(src: &[u8], width: usize, height: usize, bands: usize, n_out: usize, horizontal: bool, filter: Filter) -> Vec<u8> {
    let n_in = if horizontal { width } else { height };
    let taps = taps(n_in, n_out, filter);
    let (out_w, out_h) = if horizontal { (n_out, height) } else { (width, n_out) };
    let mut out = vec![0u8; out_w * out_h * bands];
    let half = 1i64 << (WEIGHT_BITS - 1);
    for y in 0..out_h {
        for x in 0..out_w {
            let (t, fixed) = if horizontal { (&taps[x], y) } else { (&taps[y], x) };
            for b in 0..bands {
                let mut acc = half;
                for (k, &w) in t.weights.iter().enumerate() {
                    let at = if horizontal { (fixed * width + t.first + k) * bands + b } else { ((t.first + k) * width + fixed) * bands + b };
                    acc += w as i64 * src[at] as i64;
                }
                out[(y * out_w + x) * bands + b] = to_u8(acc);
            }
        }
    }
    out
}

impl Image {
    /// RGBA to premultiplied: each colour byte becomes `c * a / 255`, rounded.
    pub fn premultiply(&self) -> Image {
        let mut out = self.clone();
        out.mode = Mode::RgbaPre;
        for px in out.data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = div255(*c as u32 * a) as u8;
            }
        }
        out
    }

    /// Premultiplied back to RGBA: each colour byte becomes `c * 255 / a`,
    /// rounded and capped at 255; pixels with zero alpha keep their bytes.
    pub fn unpremultiply(&self) -> Image {
        let mut out = self.clone();
        out.mode = Mode::Rgba;
        for px in out.data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a == 0 {
                continue;
            }
            for c in &mut px[..3] {
                *c = (*c as u32 * 255 / a).min(255) as u8;
            }
        }
        out
    }

    /// Resize to `w` x `h` for L, RGB and RGBA images.
    pub fn resize(&self, w: usize, h: usize, filter: Filter) -> Image {
        if (w, h) == (self.width, self.height) {
            return self.clone();
        }
        let alpha = self.mode == Mode::Rgba;
        let src = if alpha { self.premultiply() } else { self.clone() };
        let bands = self.mode.pixel_size();
        // Rows first, except when the image is more than 100 times taller than
        // wide and loses height: then columns first.
        let columns_first = h < src.height && src.height > 100 * src.width;
        let data = if columns_first {
            let tall = pass(&src.data, src.width, src.height, bands, h, false, filter);
            pass(&tall, src.width, h, bands, w, true, filter)
        } else {
            let wide = pass(&src.data, src.width, src.height, bands, w, true, filter);
            pass(&wide, w, src.height, bands, h, false, filter)
        };
        let mut out = Image { mode: src.mode, width: w, height: h, data };
        out.clear_pad();
        if alpha {
            out.unpremultiply()
        } else {
            out
        }
    }

    /// Sample this image into a `w` x `h` image through the affine map
    /// `a = [a, b, c, d, e, f]`: output pixel (x, y), taken at its centre
    /// (x + 0.5, y + 0.5), reads the source at
    /// (a*x + b*y + c, d*x + e*y + f), source pixel centres at integer + 0.5,
    /// interpolating bilinearly. Positions outside the source give zero.
    pub fn affine_bilinear(&self, w: usize, h: usize, a: [f64; 6]) -> Image {
        let alpha = self.mode == Mode::Rgba;
        let src = if alpha { self.premultiply() } else { self.clone() };
        let bands = self.mode.pixel_size();
        let mut out = Image::new(src.mode, w, h);
        let (sw, sh) = (src.width as f64, src.height as f64);
        let at = |x: i64, y: i64, b: usize| -> f64 {
            let x = x.clamp(0, src.width as i64 - 1) as usize;
            let y = y.clamp(0, src.height as i64 - 1) as usize;
            src.data[(y * src.width + x) * bands + b] as f64
        };
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                // Per axis: one fused multiply-add of the x term onto the y term, then the offset.
                let xin = a[0].mul_add(px, a[1] * py) + a[2];
                let yin = a[3].mul_add(px, a[4] * py) + a[5];
                if xin < 0.0 || xin >= sw || yin < 0.0 || yin >= sh {
                    continue;
                }
                let (u, v) = (xin - 0.5, yin - 0.5);
                let (x0, y0) = (u.floor(), v.floor());
                let (fx, fy) = (u - x0, v - y0);
                let (x0, y0) = (x0 as i64, y0 as i64);
                for b in 0..bands {
                    let (p00, p10, p01, p11) = (at(x0, y0, b), at(x0 + 1, y0, b), at(x0, y0 + 1, b), at(x0 + 1, y0 + 1, b));
                    // Two fused linear interpolations: p0 + t * (p1 - p0).
                    let top = fx.mul_add(p10 - p00, p00);
                    let bottom = fx.mul_add(p11 - p01, p01);
                    let value = fy.mul_add(bottom - top, top);
                    out.data[(y * w + x) * bands + b] = value.clamp(0.0, 255.0) as u8;
                }
            }
        }
        out.clear_pad();
        if alpha {
            out.unpremultiply()
        } else {
            out
        }
    }

    /// RGB's fourth byte carries no data and resampling leaves it zero.
    fn clear_pad(&mut self) {
        if self.mode == Mode::Rgb {
            for px in self.data.chunks_exact_mut(4) {
                px[3] = 0;
            }
        }
    }
}
