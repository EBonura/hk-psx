//! Approval sheet: the original | PS1 before | PS1 after at one matched moment.
//!
//! The original column is a reference-run frame (`frames/fKKKKK.png`, 640x360,
//! input frame K after the anchor), centre-cropped from 16:9 to the PS1's 4:3
//! and scaled to 320x240. Each PS1 column is the first frame the emulator
//! displayed after the tick that consumed tape sample ANCHOR+K: the tick count
//! (`HK_SIM_TICKS`) is tied to the tape by the median offset between the
//! frontend's poll count and the tick count around the anchor, which is the
//! first step of tools/og_compare.py `align_ticks` without its button check.
//!
//! With `--zoom X,Y` (world units) a box around that point, projected with each
//! PS1 run's own camera (`HK_CAMERA_X/Y`), is outlined and shown 3x below.
//!
//! Usage:
//!   hk-parity-sheet --out SHEET.png --k K --anchor A --orig REF_RUN
//!       --before RUN --before-map MAP --after RUN --after-map MAP
//!       [--zoom X,Y] [--label TEXT]
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// KNIGHT_SCALE in data/params.rs: pixels per unit = 60693 / 4096.
const PX_PER_UNIT: f64 = 60693.0 / 4096.0;

struct Image {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Image {
    fn new(w: usize, h: usize, fill: [u8; 3]) -> Self {
        Self { w, h, rgb: fill.iter().cycle().take(w * h * 3).cloned().collect() }
    }
    fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.w + x) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
    fn set(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x < self.w && y < self.h {
            let i = (y * self.w + x) * 3;
            self.rgb[i..i + 3].copy_from_slice(&c);
        }
    }
    /// Box-filtered resample of a source rectangle into a new image.
    fn resample(&self, x0: f64, y0: f64, sw: f64, sh: f64, w: usize, h: usize, nearest: bool) -> Image {
        let mut out = Image::new(w, h, [0, 0, 0]);
        for y in 0..h {
            for x in 0..w {
                let (fx0, fy0) = (x0 + sw * x as f64 / w as f64, y0 + sh * y as f64 / h as f64);
                let (fx1, fy1) = (x0 + sw * (x + 1) as f64 / w as f64, y0 + sh * (y + 1) as f64 / h as f64);
                if nearest {
                    let sx = (((fx0 + fx1) / 2.0) as isize).clamp(0, self.w as isize - 1) as usize;
                    let sy = (((fy0 + fy1) / 2.0) as isize).clamp(0, self.h as isize - 1) as usize;
                    out.set(x, y, self.get(sx, sy));
                    continue;
                }
                let mut acc = [0u32; 3];
                let mut n = 0u32;
                for sy in fy0.floor() as isize..(fy1.ceil() as isize).max(fy0.floor() as isize + 1) {
                    for sx in fx0.floor() as isize..(fx1.ceil() as isize).max(fx0.floor() as isize + 1) {
                        if sx < 0 || sy < 0 || sx >= self.w as isize || sy >= self.h as isize { continue; }
                        let c = self.get(sx as usize, sy as usize);
                        for k in 0..3 { acc[k] += c[k] as u32; }
                        n += 1;
                    }
                }
                if n > 0 { out.set(x, y, [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]); }
            }
        }
        out
    }
    fn paste(&mut self, other: &Image, x0: usize, y0: usize) {
        for y in 0..other.h {
            for x in 0..other.w { self.set(x0 + x, y0 + y, other.get(x, y)); }
        }
    }
    fn rect(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, c: [u8; 3]) {
        for x in x0..=x1 {
            for y in [y0, y1] { if x >= 0 && y >= 0 { self.set(x as usize, y as usize, c); } }
        }
        for y in y0..=y1 {
            for x in [x0, x1] { if x >= 0 && y >= 0 { self.set(x as usize, y as usize, c); } }
        }
    }
    fn text(&mut self, x0: usize, y0: usize, text: &str, scale: usize, c: [u8; 3]) {
        let mut x = x0;
        for ch in text.chars() {
            let rows = glyph(ch.to_ascii_uppercase());
            for (ry, bits) in rows.iter().enumerate() {
                for rx in 0..5 {
                    if bits & (0x10 >> rx) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale { self.set(x + rx * scale + dx, y0 + ry * scale + dy, c); }
                        }
                    }
                }
            }
            x += 6 * scale;
        }
    }
}

/// A 5x7 bitmap font: digits, capitals and the punctuation the labels use.
fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [14, 17, 19, 21, 25, 17, 14], '1' => [4, 12, 4, 4, 4, 4, 14], '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14], '4' => [2, 6, 10, 18, 31, 2, 2], '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14], '7' => [31, 1, 2, 4, 8, 8, 8], '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12], 'A' => [14, 17, 17, 31, 17, 17, 17], 'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14], 'D' => [28, 18, 17, 17, 17, 18, 28], 'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16], 'G' => [14, 17, 16, 23, 17, 17, 15], 'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14], 'J' => [7, 2, 2, 2, 2, 18, 12], 'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31], 'M' => [17, 27, 21, 21, 17, 17, 17], 'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14], 'P' => [30, 17, 17, 30, 16, 16, 16], 'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17], 'S' => [15, 16, 16, 14, 1, 1, 30], 'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14], 'V' => [17, 17, 17, 17, 17, 10, 4], 'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17], 'Y' => [17, 17, 17, 10, 4, 4, 4], 'Z' => [31, 1, 2, 4, 8, 16, 31],
        '.' => [0, 0, 0, 0, 0, 12, 12], ',' => [0, 0, 0, 0, 12, 4, 8], ':' => [0, 12, 12, 0, 12, 12, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0], '(' => [2, 4, 8, 8, 8, 4, 2], ')' => [8, 4, 2, 2, 2, 4, 8],
        '/' => [1, 1, 2, 4, 8, 16, 16], '|' => [4, 4, 4, 4, 4, 4, 4], '+' => [0, 4, 4, 31, 4, 4, 0],
        '=' => [0, 0, 31, 0, 31, 0, 0], '_' => [0, 0, 0, 0, 0, 0, 31],
        _ => [0; 7],
    }
}

fn read_ppm(path: &Path) -> Image {
    let data = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while data[i].is_ascii_whitespace() { i += 1; }
        if data[i] == b'#' { while data[i] != b'\n' { i += 1; } continue; }
        let s = i;
        while !data[i].is_ascii_whitespace() { i += 1; }
        fields.push(String::from_utf8_lossy(&data[s..i]).to_string());
    }
    assert_eq!(fields[0], "P6", "{}: not a binary PPM", path.display());
    let (w, h): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    Image { w, h, rgb: data[i + 1..i + 1 + w * h * 3].to_vec() }
}

fn read_png(path: &Path) -> Image {
    let decoder = png::Decoder::new(fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = info.line_size / w;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let p = y * info.line_size + x * channels;
            rgb.extend_from_slice(&buf[p..p + 3]);
        }
    }
    Image { w, h, rgb }
}

fn write_png(path: &Path, img: &Image) {
    let file = fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), img.w as u32, img.h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(&img.rgb).unwrap();
}

/// Symbol -> route.csv column (`ram_xxxxxxxx`) from a link map's 5-field lines.
fn columns(map: &Path) -> HashMap<String, String> {
    fs::read_to_string(map).unwrap().lines().filter_map(|l| {
        let p: Vec<&str> = l.split_whitespace().collect();
        (p.len() == 5).then(|| (p[4].to_string(), format!("ram_{:0>8}", p[0].to_lowercase())))
    }).collect()
}

fn signed(v: &str) -> i64 {
    let v: i64 = v.parse().unwrap();
    if v >= 1 << 31 { v - (1 << 32) } else { v }
}

/// (shot path, camera in units, route tick) for tape sample `sample`.
fn ps1_frame(run: &Path, map: &Path, anchor: i64, sample: i64) -> (PathBuf, (f64, f64), i64) {
    let cols = columns(map);
    let text = fs::read_to_string(run.join("route.csv")).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let index = |name: &str| header.iter().position(|h| *h == name).unwrap_or_else(|| panic!("route.csv lacks {name}"));
    let at = |sym: &str| index(&cols[sym]);
    let (tick_i, poll_i, flip_i) = (index("route_tick"), index("port1_polls"), index("display_start_changed"));
    // Without a tick count (main has none) the poll count is the sample.
    let ticks_i = cols.get("HK_SIM_TICKS").map(|c| index(c)).unwrap_or(poll_i);
    let (cx_i, cy_i) = (at("HK_CAMERA_X"), at("HK_CAMERA_Y"));
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let mut offsets: Vec<i64> = rows.iter()
        .filter(|r| { let p: i64 = r[poll_i].parse().unwrap(); (anchor - 30..=anchor + 300).contains(&p) })
        .map(|r| r[poll_i].parse::<i64>().unwrap() - r[ticks_i].parse::<i64>().unwrap()).collect();
    offsets.sort();
    let offset = if ticks_i == poll_i { 0 } else { offsets[offsets.len() / 2] };
    let row = rows.iter().find(|r| offset + r[ticks_i].parse::<i64>().unwrap() >= sample).expect("sample past the run");
    let tick: i64 = row[tick_i].parse().unwrap();
    let shown = rows.iter().filter(|r| r[flip_i] == "1").map(|r| r[tick_i].parse::<i64>().unwrap())
        .find(|&t| t > tick).unwrap_or(tick + 2);
    let mut shots: Vec<i64> = fs::read_dir(run.join("shots")).unwrap().filter_map(|e| {
        let name = e.ok()?.file_name().to_string_lossy().to_string();
        name.strip_prefix("tick-")?.strip_suffix(".ppm")?.parse().ok()
    }).collect();
    shots.sort();
    let best = *shots.iter().find(|&&s| s >= shown).expect("no shot after the frame");
    let camera = (signed(row[cx_i]) as f64 / 65536.0, signed(row[cy_i]) as f64 / 65536.0);
    (run.join("shots").join(format!("tick-{best:06}.ppm")), camera, tick)
}

fn main() {
    let mut a: HashMap<String, String> = HashMap::new();
    let mut args = std::env::args().skip(1);
    while let Some(k) = args.next() {
        a.insert(k.trim_start_matches("--").to_string(), args.next().expect("flag needs a value"));
    }
    let arg = |k: &str| a.get(k).unwrap_or_else(|| panic!("--{k} is required")).clone();
    let (k, anchor): (i64, i64) = (arg("k").parse().unwrap(), arg("anchor").parse().unwrap());
    // Without `--orig` the first column is left blank: a moment no reference
    // run covers yet, shown PS1 before | after only.
    let orig_path = a.get("orig").map(|o| PathBuf::from(o).join("frames").join(format!("f{k:05}.png")));
    let orig = match &orig_path {
        Some(path) => read_png(path),
        None => Image::new(640, 360, [14, 14, 18]),
    };
    let crop_w = orig.h as f64 * 4.0 / 3.0;
    // `--orig-x0` moves the 4:3 window inside the 16:9 frame (default centred),
    // for a moment where the cameras differ and the subject sits off-centre.
    let x0 = a.get("orig-x0").map_or((orig.w as f64 - crop_w) / 2.0, |v| v.parse().unwrap());
    let orig = orig.resample(x0, 0.0, crop_w, orig.h as f64, 320, 240, false);
    let (bp, bcam, btick) = ps1_frame(Path::new(&arg("before")), Path::new(&arg("before-map")), anchor, anchor + k);
    let (ap, acam, atick) = ps1_frame(Path::new(&arg("after")), Path::new(&arg("after-map")), anchor, anchor + k);
    let (before, after) = (read_ppm(&bp), read_ppm(&ap));
    let before = before.resample(0.0, 0.0, before.w as f64, before.h as f64, 320, 240, true);
    let after = after.resample(0.0, 0.0, after.w as f64, after.h as f64, 320, 240, true);
    let zoom: Option<(f64, f64)> = a.get("zoom").map(|z| {
        let v: Vec<f64> = z.split(',').map(|s| s.parse().unwrap()).collect();
        (v[0], v[1])
    });
    const S: usize = 2;
    let zoom_h = if zoom.is_some() { 300 } else { 0 };
    let mut sheet = Image::new(3 * 320 * S, 240 * S + zoom_h + 60, [14, 14, 18]);
    let label = a.get("label").cloned().unwrap_or_else(|| "this branch".into());
    let titles = [if orig_path.is_some() { "ORIGINAL (WINDOWS, UNITY)" } else { "NO REFERENCE RUN FOR THIS MOMENT" }.to_string(), "PS1 BEFORE".to_string(), format!("PS1 AFTER: {label}")];
    let cams = [acam, bcam, acam];
    let orig_zoom: Option<(f64, f64)> = a.get("orig-zoom").map(|z| {
        let v: Vec<f64> = z.split(',').map(|s| s.parse().unwrap()).collect();
        (v[0], v[1])
    });
    for (i, img) in [&orig, &before, &after].into_iter().enumerate() {
        let mut big = img.resample(0.0, 0.0, 320.0, 240.0, 320 * S, 240 * S, true);
        // The reference runs record no camera, so the original's box is placed
        // by hand (screen pixels of the 320x240 crop) or left out.
        let centre = match (i, zoom) {
            (_, None) => None,
            (0, Some(_)) => orig_zoom,
            (_, Some((wx, wy))) => {
                let (cx, cy) = cams[i];
                Some((160.0 + (wx - cx) * PX_PER_UNIT, 120.0 - (wy - cy) * PX_PER_UNIT))
            }
        };
        if let Some((px, py)) = centre {
            let r = (px as isize - 40, py as isize - 50, px as isize + 40, py as isize + 50);
            big.rect(r.0 * S as isize, r.1 * S as isize, r.2 * S as isize, r.3 * S as isize, [255, 220, 0]);
            let crop = img.resample(r.0 as f64, r.1 as f64, 80.0, 100.0, 240, 300, true);
            sheet.paste(&crop, i * 320 * S + 200, 240 * S);
        }
        sheet.paste(&big, i * 320 * S, 0);
        sheet.text(i * 320 * S + 6, 6, &titles[i], 2, [255, 255, 255]);
    }
    let foot = format!("K {k}  SAMPLE {}  TICKS {btick}/{atick}  CAMERA {:.2},{:.2} / {:.2},{:.2}{}",
        anchor + k, bcam.0, bcam.1, acam.0, acam.1,
        zoom.map_or(String::new(), |(x, y)| format!("  ZOOM {x:.2},{y:.2}")));
    sheet.text(6, 240 * S + zoom_h + 20, &foot, 2, [220, 220, 220]);
    write_png(Path::new(&arg("out")), &sheet);
    println!("{} <- {} | {} | {}", arg("out"), orig_path.map_or("-".into(), |p| p.display().to_string()), bp.display(), ap.display());
}
