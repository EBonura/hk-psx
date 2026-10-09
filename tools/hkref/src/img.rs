//! Tiny RGB image helpers: PPM/PNG load, resize, blit, a 3x5 label font, PNG save,
//! and a two-series line plot. Enough for side-by-side sheets and trace plots.

use std::fs;
use std::path::Path;

#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Img {
    pub fn new(w: usize, h: usize, c: [u8; 3]) -> Img {
        let mut px = Vec::with_capacity(w * h * 3);
        for _ in 0..w * h {
            px.extend_from_slice(&c);
        }
        Img { w, h, px }
    }
    pub fn load_ppm(p: &Path) -> Result<Img, String> {
        let d = fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut pos = 0;
        let mut tok = vec![];
        while tok.len() < 4 {
            while pos < d.len() && d[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let s = pos;
            while pos < d.len() && !d[pos].is_ascii_whitespace() {
                pos += 1;
            }
            tok.push(String::from_utf8_lossy(&d[s..pos]).to_string());
        }
        pos += 1;
        if tok[0] != "P6" {
            return Err("not P6".into());
        }
        let (w, h): (usize, usize) = (
            tok[1].parse().map_err(|_| "w")?,
            tok[2].parse().map_err(|_| "h")?,
        );
        if d.len() < pos + w * h * 3 {
            return Err("short ppm".into());
        }
        Ok(Img {
            w,
            h,
            px: d[pos..pos + w * h * 3].to_vec(),
        })
    }
    pub fn load_png(p: &Path) -> Result<Img, String> {
        let f = fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut r = png::Decoder::new(f)
            .read_info()
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0; r.output_buffer_size()];
        let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let n = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Grayscale => 1,
            _ => return Err("unsupported png".into()),
        };
        let mut px = Vec::with_capacity(info.width as usize * info.height as usize * 3);
        for c in buf[..info.buffer_size()].chunks(n) {
            if n == 1 {
                px.extend_from_slice(&[c[0]; 3]);
            } else {
                px.extend_from_slice(&c[..3]);
            }
        }
        Ok(Img {
            w: info.width as usize,
            h: info.height as usize,
            px,
        })
    }
    pub fn save_png(&self, p: &Path) -> Result<(), String> {
        let f = fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut e = png::Encoder::new(f, self.w as u32, self.h as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&self.px)
            .map_err(|e| e.to_string())
    }
    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.px[i], self.px[i + 1], self.px[i + 2]]
    }
    pub fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }
    /// Box-filter / bilinear resize.
    pub fn resize(&self, nw: usize, nh: usize) -> Img {
        let mut o = Img::new(nw, nh, [0; 3]);
        for y in 0..nh {
            for x in 0..nw {
                let (fx, fy) = (
                    (x as f64 + 0.5) * self.w as f64 / nw as f64 - 0.5,
                    (y as f64 + 0.5) * self.h as f64 / nh as f64 - 0.5,
                );
                let (x0, y0) = (fx.floor().max(0.0) as usize, fy.floor().max(0.0) as usize);
                let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
                let (ax, ay) = (
                    (fx - x0 as f64).clamp(0.0, 1.0),
                    (fy - y0 as f64).clamp(0.0, 1.0),
                );
                let mut c = [0u8; 3];
                for k in 0..3 {
                    let v = self.get(x0, y0)[k] as f64 * (1.0 - ax) * (1.0 - ay)
                        + self.get(x1, y0)[k] as f64 * ax * (1.0 - ay)
                        + self.get(x0, y1)[k] as f64 * (1.0 - ax) * ay
                        + self.get(x1, y1)[k] as f64 * ax * ay;
                    c[k] = v.round() as u8;
                }
                o.put(x as i64, y as i64, c);
            }
        }
        o
    }
    pub fn blit(&mut self, s: &Img, ox: i64, oy: i64) {
        for y in 0..s.h {
            for x in 0..s.w {
                self.put(ox + x as i64, oy + y as i64, s.get(x, y));
            }
        }
    }
    pub fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, c: [u8; 3]) {
        for j in 0..h {
            for i in 0..w {
                self.put(x + i, y + j, c);
            }
        }
    }
    pub fn line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 3]) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut e) = (x0, y0, dx + dy);
        loop {
            self.put(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * e;
            if e2 >= dy {
                e += dy;
                x += sx;
            }
            if e2 <= dx {
                e += dx;
                y += sy;
            }
        }
    }
    pub fn text(&mut self, x: i64, y: i64, s: &str, scale: i64, c: [u8; 3]) {
        let mut cx = x;
        for ch in s.chars() {
            if let Some(g) = glyph(ch) {
                for (r, row) in g.iter().enumerate() {
                    for col in 0..3 {
                        if row >> (2 - col) & 1 == 1 {
                            self.rect(
                                cx + col as i64 * scale,
                                y + r as i64 * scale,
                                scale,
                                scale,
                                c,
                            );
                        }
                    }
                }
            }
            cx += 4 * scale;
        }
    }
}

fn glyph(c: char) -> Option<[u8; 5]> {
    Some(match c.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'O' => [7, 5, 5, 5, 7],
        'R' => [6, 5, 6, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'G' => [7, 4, 5, 5, 7],
        'P' => [6, 5, 6, 4, 4],
        'T' => [7, 2, 2, 2, 2],
        'N' => [5, 7, 7, 7, 5],
        'A' => [2, 5, 7, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        ':' => [0, 2, 0, 2, 0],
        '-' => [0, 0, 7, 0, 0],
        '.' => [0, 0, 0, 0, 2],
        ' ' => [0; 5],
        'H' => [5, 5, 7, 5, 5],
        'E' => [7, 4, 6, 4, 7],
        'C' => [7, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'S' => [7, 4, 7, 1, 7],
        'F' => [7, 4, 6, 4, 4],
        _ => return None,
    })
}

/// Two series against tick, one panel. Original blue, port orange; gaps are skipped.
pub fn plot(title: &str, o: &[Option<f64>], p: &[Option<f64>], t0: i64) -> Img {
    let (w, h, m) = (900usize, 220usize, 28usize);
    let mut im = Img::new(w, h, [250, 250, 250]);
    let vals: Vec<f64> = o.iter().chain(p.iter()).flatten().copied().collect();
    if vals.is_empty() {
        return im;
    }
    let (mut lo, mut hi) = (
        vals.iter().cloned().fold(f64::MAX, f64::min),
        vals.iter().cloned().fold(f64::MIN, f64::max),
    );
    if hi - lo < 1e-6 {
        lo -= 0.5;
        hi += 0.5;
    }
    let n = o.len().max(p.len()).max(2);
    let xy = |i: usize, v: f64| {
        (
            m as i64 + (i as f64 / (n - 1) as f64 * (w - 2 * m) as f64) as i64,
            (h - m) as i64 - ((v - lo) / (hi - lo) * (h - 2 * m) as f64) as i64,
        )
    };
    im.rect(m as i64, m as i64, (w - 2 * m) as i64, 1, [200, 200, 200]);
    im.rect(
        m as i64,
        (h - m) as i64,
        (w - 2 * m) as i64,
        1,
        [120, 120, 120],
    );
    for (series, col) in [(o, [30, 90, 220]), (p, [230, 120, 20])] {
        let mut prev: Option<(i64, i64)> = None;
        for (i, v) in series.iter().enumerate() {
            match v {
                Some(v) => {
                    let q = xy(i, *v);
                    if let Some(a) = prev {
                        im.line(a.0, a.1, q.0, q.1, col);
                    }
                    prev = Some(q);
                }
                None => prev = None,
            }
        }
    }
    im.text(m as i64, 6, title, 2, [20, 20, 20]);
    im.text(
        m as i64,
        (h - 18) as i64,
        &format!("T{} - {}   LO {:.2} HI {:.2}", t0, t0 + n as i64, lo, hi),
        1,
        [60, 60, 60],
    );
    im.rect((w - 190) as i64, 8, 12, 8, [30, 90, 220]);
    im.text((w - 174) as i64, 8, "ORIGINAL", 2, [20, 20, 20]);
    im.rect((w - 90) as i64, 8, 12, 8, [230, 120, 20]);
    im.text((w - 74) as i64, 8, "PORT", 2, [20, 20, 20]);
    im
}
