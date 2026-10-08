//! `Image.resize` (libImaging/Resample.c, 8 bits per channel) and
//! `Image.transform(AFFINE, BILINEAR)` (Geometry.c), with the RGBA
//! premultiply round trip Image.py wraps both in.

#![allow(clippy::needless_range_loop)] // kept in the shape of the C it ports

use crate::{Image, Mode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Box,
    Bilinear,
    Lanczos,
}

impl Filter {
    fn support(self) -> f64 {
        match self {
            Filter::Box => 0.5,
            Filter::Bilinear => 1.0,
            Filter::Lanczos => 3.0,
        }
    }
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
                fn sinc(x: f64) -> f64 {
                    if x == 0.0 {
                        return 1.0;
                    }
                    let x = x * std::f64::consts::PI;
                    x.sin() / x
                }
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
        }
    }
}

const PRECISION_BITS: u32 = 32 - 8 - 2;

fn clip8(v: i32) -> u8 {
    (v >> PRECISION_BITS).clamp(0, 255) as u8
}

/// precompute_coeffs + normalize_coeffs_8bpc: (bounds, fixed-point weights, ksize).
fn coeffs(in_size: usize, in0: f32, in1: f32, out_size: usize, filter: Filter) -> (Vec<(usize, usize)>, Vec<i32>, usize) {
    // (in1 - in0) is a float subtraction in the C, widened after.
    let scale = (in1 - in0) as f64 / out_size as f64;
    let filterscale = if scale < 1.0 { 1.0 } else { scale };
    let support = filter.support() * filterscale;
    let ksize = support.ceil() as usize * 2 + 1;
    let inv = 1.0 / filterscale;
    let mut bounds = Vec::with_capacity(out_size);
    let mut kk = vec![0i32; out_size * ksize];
    for xx in 0..out_size {
        let center = (xx as f64 + 0.5).mul_add(scale, in0 as f64);
        let mut xmin = (center - support + 0.5) as i32;
        if xmin < 0 {
            xmin = 0;
        }
        let mut xmax = (center + support + 0.5) as i32;
        if xmax > in_size as i32 {
            xmax = in_size as i32;
        }
        xmax -= xmin;
        let mut k = vec![0f64; ksize];
        let mut ww = 0.0;
        for x in 0..xmax.max(0) as usize {
            let w = filter.eval(((x as i32 + xmin) as f64 - center + 0.5) * inv);
            k[x] = w;
            ww += w;
        }
        if ww != 0.0 {
            for v in k.iter_mut().take(xmax.max(0) as usize) {
                *v /= ww;
            }
        }
        for (x, v) in k.iter().enumerate() {
            let scaled = v * (1u32 << PRECISION_BITS) as f64;
            kk[xx * ksize + x] = if *v < 0.0 { (-0.5 + scaled) as i32 } else { (0.5 + scaled) as i32 };
        }
        bounds.push((xmin as usize, xmax.max(0) as usize));
    }
    (bounds, kk, ksize)
}

fn bands(mode: Mode) -> usize {
    match mode {
        Mode::Rgb => 3,
        Mode::Rgba | Mode::RgbaPre => 4,
        _ => 1,
    }
}

fn horizontal(im: &Image, out_w: usize, rows: std::ops::Range<usize>, bounds: &[(usize, usize)], kk: &[i32], ksize: usize) -> Image {
    let n = im.mode.pixel_size();
    let b = bands(im.mode);
    let mut out = Image::new(im.mode, out_w, rows.len());
    for (yy, y) in rows.enumerate() {
        let line = &im.data[y * im.width * n..(y + 1) * im.width * n];
        for (xx, &(xmin, xmax)) in bounds.iter().enumerate() {
            let k = &kk[xx * ksize..];
            for c in 0..n {
                let o = (yy * out_w + xx) * n + c;
                if c >= b {
                    out.data[o] = 0;
                    continue;
                }
                let mut ss: i32 = 1 << (PRECISION_BITS - 1);
                for x in 0..xmax {
                    ss = ss.wrapping_add(line[(x + xmin) * n + c] as i32 * k[x]);
                }
                out.data[o] = clip8(ss);
            }
        }
    }
    out
}

fn vertical(im: &Image, out_h: usize, bounds: &[(usize, usize)], kk: &[i32], ksize: usize) -> Image {
    let n = im.mode.pixel_size();
    let b = bands(im.mode);
    let w = im.width;
    let mut out = Image::new(im.mode, w, out_h);
    for (yy, &(ymin, ymax)) in bounds.iter().enumerate() {
        let k = &kk[yy * ksize..];
        for xx in 0..w {
            for c in 0..n {
                let o = (yy * w + xx) * n + c;
                if c >= b {
                    out.data[o] = 0;
                    continue;
                }
                let mut ss: i32 = 1 << (PRECISION_BITS - 1);
                for y in 0..ymax {
                    ss = ss.wrapping_add(im.data[((y + ymin) * w + xx) * n + c] as i32 * k[y]);
                }
                out.data[o] = clip8(ss);
            }
        }
    }
    out
}

/// ImagingResample with the full box.
fn resample(im: &Image, w: usize, h: usize, filter: Filter) -> Image {
    let need_h = w != im.width;
    let need_v = h != im.height;
    let (mut bv, kv, ksv) = coeffs(im.height, 0.0, im.height as f32, h, filter);
    let first = bv[0].0;
    let last = bv[h - 1].0 + bv[h - 1].1;
    let mut cur: Option<Image> = None;
    if need_h {
        let (bh, kh, ksh) = coeffs(im.width, 0.0, im.width as f32, w, filter);
        for b in bv.iter_mut() {
            b.0 -= first;
        }
        cur = Some(horizontal(im, w, first..last, &bh, &kh, ksh));
    }
    if need_v {
        let src = cur.as_ref().unwrap_or(im);
        return vertical(src, h, &bv, &kv, ksv);
    }
    cur.unwrap_or_else(|| im.clone())
}

impl Image {
    /// RGBA -> RGBa (Convert.c rgbA2rgba).
    pub fn premultiply(&self) -> Image {
        let mut out = self.clone();
        out.mode = Mode::RgbaPre;
        for px in out.data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            for c in px.iter_mut().take(3) {
                let tmp = *c as u32 * a + 128;
                *c = ((tmp + (tmp >> 8)) >> 8) as u8;
            }
        }
        out
    }

    /// RGBa -> RGBA (Convert.c rgba2rgbA).
    pub fn unpremultiply(&self) -> Image {
        let mut out = self.clone();
        out.mode = Mode::Rgba;
        for px in out.data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a != 255 && a != 0 {
                for c in px.iter_mut().take(3) {
                    *c = ((255 * *c as u32) / a).min(255) as u8;
                }
            }
        }
        out
    }

    /// `Image.resize((w, h), filter)` for L, RGB and RGBA.
    pub fn resize(&self, w: usize, h: usize, filter: Filter) -> Image {
        if (w, h) == (self.width, self.height) {
            return self.clone();
        }
        assert!(w >= 1 && h >= 1, "height and width must be > 0");
        if self.mode == Mode::Rgba {
            return self.premultiply().resize(w, h, filter).unpremultiply();
        }
        if self.height > self.width * 100 && h < self.height {
            let step = resample(self, self.width, h, filter);
            return resample(&step, w, h, filter);
        }
        resample(self, w, h, filter)
    }

    /// `Image.transform((w, h), AFFINE, a, BILINEAR)` with the default fill.
    pub fn affine_bilinear(&self, w: usize, h: usize, a: [f64; 6]) -> Image {
        if self.mode == Mode::Rgba {
            return self.premultiply().affine_bilinear(w, h, a).unpremultiply();
        }
        let n = self.mode.pixel_size();
        let b = bands(self.mode);
        let mut out = Image::new(self.mode, w, h);
        let (sw, sh) = (self.width as i32, self.height as i32);
        for y in 0..h {
            for x in 0..w {
                let (xin, yin) = ((x as i32) as f64 + 0.5, (y as i32) as f64 + 0.5);
                // affine_transform: a2 + fma(a0, xin, a1 * yin), as the arm64 build computes it.
                let xx = a[2] + a[0].mul_add(xin, a[1] * yin);
                let yy = a[5] + a[3].mul_add(xin, a[4] * yin);
                let o = (y * w + x) * n;
                if xx < 0.0 || xx >= sw as f64 || yy < 0.0 || yy >= sh as f64 {
                    continue; // fill: zero
                }
                let (xin, yin) = (xx - 0.5, yy - 0.5);
                let floor = |v: f64| if v < 0.0 { v.floor() as i32 } else { v as i32 };
                let (ix, iy) = (floor(xin), floor(yin));
                let (dx, dy) = (xin - ix as f64, yin - iy as f64);
                let xclip = |v: i32| v.clamp(0, sw - 1) as usize;
                let yclip = |v: i32| v.clamp(0, sh - 1) as usize;
                let (x0, x1) = (xclip(ix), xclip(ix + 1));
                for c in 0..b {
                    let px = |row: usize, col: usize| self.data[(row * self.width + col) * n + c] as i32;
                    let r0 = yclip(iy);
                    let lerp_int = |a: i32, bb: i32| ((bb - a) as f64).mul_add(dx, a as f64);
                    let v1 = lerp_int(px(r0, x0), px(r0, x1));
                    let v2 = if iy + 1 >= 0 && iy + 1 < sh { lerp_int(px((iy + 1) as usize, x0), px((iy + 1) as usize, x1)) } else { v1 };
                    let v = (v2 - v1).mul_add(dy, v1);
                    out.data[o + c] = v as i32 as u8;
                }
            }
        }
        out
    }
}
