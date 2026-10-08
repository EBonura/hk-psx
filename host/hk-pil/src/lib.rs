//! The Pillow 12.3.0 operations hk-psx's cookers use, ported from Pillow's C
//! (libImaging) so that every pixel comes out the same. Pillow is under the
//! MIT-CMU (HPND) licence, Copyright (c) 1997-2011 Secret Labs AB, (c) 1995-2011
//! Fredrik Lundh and contributors, (c) 2010 Jeffrey A. Clark and contributors;
//! BcnDecode.c is CC0. Where Pillow's arm64 build fuses a multiply and add
//! (clang's default contraction), the port uses `mul_add` in the same place.

pub mod bcn;
pub mod draw;
pub mod quant;
pub mod resample;

/// Pillow modes in use. Multi-band modes store four bytes per pixel, as
/// libImaging does (RGB carries a 255 pad byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// "1": one byte per pixel, 0 or nonzero.
    One,
    L,
    P,
    Rgb,
    Rgba,
    /// Premultiplied RGBA.
    RgbaPre,
}

impl Mode {
    pub fn pixel_size(self) -> usize {
        match self {
            Mode::One | Mode::L | Mode::P => 1,
            _ => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub mode: Mode,
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// Python's `round` (half to even) on an f64, then `int`.
pub fn py_round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

impl Image {
    /// `Image.new(mode, size)`: zero filled.
    pub fn new(mode: Mode, width: usize, height: usize) -> Image {
        Image { mode, width, height, data: vec![0; width * height * mode.pixel_size()] }
    }

    pub fn pixel(&self, x: usize, y: usize) -> &[u8] {
        let n = self.mode.pixel_size();
        let at = (y * self.width + x) * n;
        &self.data[at..at + n]
    }

    /// `Image.crop(box)`: the box is rounded half to even, and the area outside
    /// the source is zero.
    pub fn crop(&self, b: [f64; 4]) -> Image {
        let [x0, y0, x1, y1] = b.map(py_round);
        self.crop_int(x0, y0, x1, y1)
    }

    pub fn crop_int(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> Image {
        let w = (x1 - x0).max(0) as usize;
        let h = (y1 - y0).max(0) as usize;
        let mut out = Image::new(self.mode, w, h);
        let n = self.mode.pixel_size();
        for y in 0..h as i64 {
            let sy = y + y0;
            if sy < 0 || sy >= self.height as i64 {
                continue;
            }
            for x in 0..w as i64 {
                let sx = x + x0;
                if sx < 0 || sx >= self.width as i64 {
                    continue;
                }
                let src = (sy as usize * self.width + sx as usize) * n;
                let dst = (y as usize * w + x as usize) * n;
                out.data[dst..dst + n].copy_from_slice(&self.data[src..src + n]);
            }
        }
        out
    }

    pub fn flip_top_bottom(&self) -> Image {
        let row = self.width * self.mode.pixel_size();
        let mut out = Image::new(self.mode, self.width, self.height);
        for y in 0..self.height {
            let dst = (self.height - 1 - y) * row;
            out.data[dst..dst + row].copy_from_slice(&self.data[y * row..(y + 1) * row]);
        }
        out
    }

    pub fn flip_left_right(&self) -> Image {
        let n = self.mode.pixel_size();
        let mut out = Image::new(self.mode, self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                let s = (y * self.width + x) * n;
                let d = (y * self.width + (self.width - 1 - x)) * n;
                out.data[d..d + n].copy_from_slice(&self.data[s..s + n]);
            }
        }
        out
    }

    /// `paste(src, (x, y), mask)` for a same-mode source and a "1" or "L" mask
    /// of the source's size: a pixel is copied where the mask is nonzero ("1")
    /// or blended by it ("L", Pillow's paste_mask_L).
    pub fn paste(&mut self, src: &Image, x0: i64, y0: i64, mask: Option<&Image>) {
        assert_eq!(self.mode.pixel_size(), src.mode.pixel_size());
        let n = self.mode.pixel_size();
        for y in 0..src.height as i64 {
            let dy = y + y0;
            if dy < 0 || dy >= self.height as i64 {
                continue;
            }
            for x in 0..src.width as i64 {
                let dx = x + x0;
                if dx < 0 || dx >= self.width as i64 {
                    continue;
                }
                let s = (y as usize * src.width + x as usize) * n;
                let d = (dy as usize * self.width + dx as usize) * n;
                match mask {
                    None => self.data[d..d + n].copy_from_slice(&src.data[s..s + n]),
                    Some(m) if m.mode == Mode::One => {
                        if m.data[y as usize * m.width + x as usize] != 0 {
                            self.data[d..d + n].copy_from_slice(&src.data[s..s + n]);
                        }
                    }
                    Some(m) => {
                        let a = m.data[y as usize * m.width + x as usize] as u32;
                        for k in 0..n {
                            // BLEND(mask, out, in) = DIV255(out * (255 - mask) + in * mask)
                            let o = self.data[d + k] as u32;
                            let i = src.data[s + k] as u32;
                            let tmp = o * (255 - a) + i * a + 128;
                            self.data[d + k] = ((tmp + (tmp >> 8)) >> 8) as u8;
                        }
                    }
                }
            }
        }
    }

    /// `convert("RGBA")` from RGB or RGBA (RGB gains alpha 255).
    pub fn to_rgba(&self) -> Image {
        match self.mode {
            Mode::Rgba => self.clone(),
            Mode::Rgb => {
                let mut out = self.clone();
                out.mode = Mode::Rgba;
                for px in out.data.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                out
            }
            other => panic!("to_rgba from {other:?} is not ported"),
        }
    }
}
