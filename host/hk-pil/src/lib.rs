//! The image operations hk-psx's cookers use, written from the textbook
//! definitions of each operation: separable filtered resampling, bilinear
//! affine sampling, S3TC/BPTC block decoding, scanline polygon fill and
//! median-cut colour quantisation.
//!
//! Pixel layout follows the cookers' needs: single-band modes take one byte
//! per pixel and multi-band modes four (RGB carries a 255 pad byte).

pub mod bcn;
mod bcn_tables;
pub mod draw;
pub mod quant;
pub mod resample;

/// Pixel modes in use. Multi-band modes store four bytes per pixel
/// (RGB carries a 255 pad byte).
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

/// Round half to even, as an integer.
pub fn py_round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// `x / 255` rounded to nearest, for `x` up to 255 * 255.
pub(crate) fn div255(x: u32) -> u32 {
    (x + 127) / 255
}

impl Image {
    /// A zero-filled image.
    pub fn new(mode: Mode, width: usize, height: usize) -> Image {
        Image { mode, width, height, data: vec![0; width * height * mode.pixel_size()] }
    }

    /// The bytes of the pixel at (`x`, `y`).
    pub fn pixel(&self, x: usize, y: usize) -> &[u8] {
        let n = self.mode.pixel_size();
        let at = (y * self.width + x) * n;
        &self.data[at..at + n]
    }

    /// The sub-image inside `b` = [left, upper, right, lower]; each edge is
    /// rounded half to even and any area outside the source is zero.
    pub fn crop(&self, b: [f64; 4]) -> Image {
        self.crop_int(py_round(b[0]), py_round(b[1]), py_round(b[2]), py_round(b[3]))
    }

    /// `crop` with integer edges.
    pub fn crop_int(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> Image {
        let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        let n = self.mode.pixel_size();
        let mut out = Image::new(self.mode, w, h);
        for y in 0..h {
            let sy = y0 + y as i64;
            if sy < 0 || sy >= self.height as i64 {
                continue;
            }
            // Columns of this row that fall inside the source.
            let first = (-x0).max(0) as usize;
            let last = ((self.width as i64 - x0).max(0) as usize).min(w);
            if first >= last {
                continue;
            }
            let src = (sy as usize * self.width + (x0 + first as i64) as usize) * n;
            let dst = (y * w + first) * n;
            let len = (last - first) * n;
            out.data[dst..dst + len].copy_from_slice(&self.data[src..src + len]);
        }
        out
    }

    /// Mirror about the horizontal axis.
    pub fn flip_top_bottom(&self) -> Image {
        let row = self.width * self.mode.pixel_size();
        let mut out = self.clone();
        for y in 0..self.height {
            let from = (self.height - 1 - y) * row;
            out.data[y * row..(y + 1) * row].copy_from_slice(&self.data[from..from + row]);
        }
        out
    }

    /// Mirror about the vertical axis.
    pub fn flip_left_right(&self) -> Image {
        let n = self.mode.pixel_size();
        let mut out = self.clone();
        for y in 0..self.height {
            for x in 0..self.width {
                let from = (y * self.width + self.width - 1 - x) * n;
                let to = (y * self.width + x) * n;
                out.data[to..to + n].copy_from_slice(&self.data[from..from + n]);
            }
        }
        out
    }

    /// A quarter turn: counter-clockwise when `counter_clockwise`, clockwise otherwise. The result is
    /// `height` wide and `width` tall, the way an expanding rotation by 90 degrees lays it out.
    pub fn quarter_turn(&self, counter_clockwise: bool) -> Image {
        let n = self.mode.pixel_size();
        let mut out = Image { mode: self.mode, width: self.height, height: self.width, data: vec![0; self.data.len()] };
        for y in 0..self.height {
            for x in 0..self.width {
                // Counter-clockwise sends the right edge to the top; clockwise sends the left edge to the top.
                let (ox, oy) = if counter_clockwise { (y, self.width - 1 - x) } else { (self.height - 1 - y, x) };
                let from = (y * self.width + x) * n;
                let to = (oy * out.width + ox) * n;
                out.data[to..to + n].copy_from_slice(&self.data[from..from + n]);
            }
        }
        out
    }

    /// Paste a same-mode `src` with its top-left corner at (`x0`, `y0`),
    /// clipped to this image. A mask of the source's size selects pixels: a
    /// "1" mask copies where nonzero, an "L" mask blends each byte as
    /// `(src * m + dst * (255 - m)) / 255`, rounded to nearest.
    pub fn paste(&mut self, src: &Image, x0: i64, y0: i64, mask: Option<&Image>) {
        let n = self.mode.pixel_size();
        for sy in 0..src.height {
            let dy = y0 + sy as i64;
            if dy < 0 || dy >= self.height as i64 {
                continue;
            }
            for sx in 0..src.width {
                let dx = x0 + sx as i64;
                if dx < 0 || dx >= self.width as i64 {
                    continue;
                }
                let s = &src.data[(sy * src.width + sx) * n..][..n];
                let d = &mut self.data[(dy as usize * self.width + dx as usize) * n..][..n];
                match mask {
                    None => d.copy_from_slice(s),
                    Some(m) => {
                        let m_value = m.data[sy * m.width + sx];
                        if m.mode == Mode::One {
                            if m_value != 0 {
                                d.copy_from_slice(s);
                            }
                        } else {
                            let m_value = m_value as u32;
                            for (db, &sb) in d.iter_mut().zip(s) {
                                *db = div255(sb as u32 * m_value + *db as u32 * (255 - m_value)) as u8;
                            }
                        }
                    }
                }
            }
        }
    }

    /// RGB or RGBA as RGBA (RGB gains alpha 255).
    pub fn to_rgba(&self) -> Image {
        let mut out = self.clone();
        out.mode = Mode::Rgba;
        if self.mode == Mode::Rgb {
            for px in out.data.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        out
    }
}
