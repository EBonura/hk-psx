//! The panel font: Perpetua rasterised once, at the size the PS1 draws it, and a trimmed Trajan caps strip.
//!
//! Every menu and dialogue screen draws text from one 4bpp image of 256 x 100 texels (64 x 100
//! halfwords, uploaded at (320, 256) in VRAM, the texture page of the unused CLUT banks 1 and 2):
//!
//! - rows 0..74: the 95 ASCII glyphs of Perpetua at 14 px, in cells of 13 x 14 texels, 19 to a row,
//!   five rows 15 texels apart (the spare row keeps the texel above a cell blank, which a doubled
//!   glyph's polygon samples), cells starting 4 texels into the row;
//! - rows 76..99: the capitals, digits and % of TrajanPro-Bold, in cells of the widest capital by
//!   12 texels, two rows, then the Geo coin in the cell after them.
//!
//! The image follows a 16 entry palette (data/read_font.hk, 12,832 bytes). Menu buttons, headings
//! and save-slot and option labels use the capitals, as the original's do. This writes the image,
//! the advances the layout code shares (data/font-metrics.json) and the constants the guest links
//! (data/font_layout.rs).
//!
//! The glyphs are the source fonts' own outlines, anti-aliased to 16 coverage levels:
//!
//! - Grid fitted vertically. The outlines are scaled to the cell and their baseline, x-height, cap
//!   height and ascender and descender lines moved onto whole pixels (piecewise linear between), so
//!   the horizontal strokes and the tops of letters land on pixel rows instead of straddling two.
//!   There is no hinter here, so stems are not snapped sideways.
//! - Coverage is lifted by a gamma below one: the glyphs are drawn as white added to the screen, and
//!   a display shows half coverage darker than half brightness.
//! - The advance is the real advance rounded to the nearest pixel (a Trajan capital gets one more,
//!   for the original's letter spacing), the glyph sits in it by half the rounding error, one
//!   baseline for every glyph.
//!
//! The palette's entries other than 0 carry the semi-transparency bit (bit 15), so a glyph drawn with a
//! semi-transparent command adds its grey to the screen. Index 0 stays fully transparent.
use crate::common::{err, Result};
use ab_glyph_rasterizer::{point, Rasterizer};
use ttf_parser::{Face, OutlineBuilder};

/// Perpetua's pixels per em and the strip's cell: the 95 ASCII glyphs in 13x14 cells, 19 to a row.
pub const EM: f32 = 14.0;
pub const CELL_W: usize = 13;
pub const CELL_H: usize = 14;
pub const PER_ROW: usize = 19;
pub const ROWS: usize = 5;
/// Texels between the tops of two rows of body cells: the cell and a blank row.
pub const ROW_PITCH: usize = 15;
/// Where cells start inside the 256 texel row (game/src/menu_state.rs `GLYPH_U0`).
pub const GLYPH_U0: usize = 4;
pub const GLYPHS: usize = 95;
/// The Trajan capitals: 12 texel tall cells, two rows of them below the body cells.
pub const CAPS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789%";
pub const CAPS_H: usize = 12;
pub const CAPS_ROWS: usize = 2;
pub const CAPS_V0: usize = ROWS * ROW_PITCH + 1;
/// The image's height in texel rows, and the file: palette, then 256 texels a row at 4bpp.
pub const IMAGE_ROWS: usize = CAPS_V0 + CAPS_ROWS * CAPS_H;
pub const STRIP_BYTES: usize = 32 + IMAGE_ROWS * 128;
/// Coverage exponent and a small gain on top.
const GAMMA: f32 = 0.6;
const GAIN: f32 = 1.1;

/// The 16 palette entries: transparent, then the grey ramp with the semi-transparency bit.
pub fn palette() -> [u16; 16] {
    let mut p = [0u16; 16];
    for (v, e) in p.iter_mut().enumerate().skip(1) {
        *e = 0x8000 | (((v as u16 * 17) >> 3) * 0x421);
    }
    p
}

/// Coverage 0..1 to a palette index.
pub fn level(coverage: f32) -> u8 {
    (15.0 * (coverage * GAIN).clamp(0.0, 1.0).powf(GAMMA)).round() as u8
}

#[derive(Default)]
struct Outline(Vec<Seg>);
enum Seg {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}
impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::M(x, y))
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::L(x, y))
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.0.push(Seg::Q(a, b, x, y))
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.0.push(Seg::C(a, b, c, d, x, y))
    }
    fn close(&mut self) {
        self.0.push(Seg::Z)
    }
}

/// Piecewise linear map of a height in pixels (y up) through its knots, slope 1 outside them.
struct Fit(Vec<(f32, f32)>);
impl Fit {
    fn map(&self, y: f32) -> f32 {
        let k = &self.0;
        if k.len() < 2 {
            return y;
        }
        if y <= k[0].0 {
            return k[0].1 + (y - k[0].0);
        }
        for w in k.windows(2) {
            if y <= w[1].0 {
                return w[0].1 + (y - w[0].0) / (w[1].0 - w[0].0) * (w[1].1 - w[0].1);
            }
        }
        let l = k[k.len() - 1];
        l.1 + (y - l.0)
    }
}

struct Font<'a> {
    face: Face<'a>,
    upem: f32,
}
impl<'a> Font<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self> {
        let face = Face::parse(bytes, 0).map_err(|e| format!("font: {e}"))?;
        let upem = f32::from(face.units_per_em());
        Ok(Font { face, upem })
    }
    fn advance(&self, ch: char, em: f32) -> Result<f32> {
        let id = self.face.glyph_index(ch).ok_or_else(|| format!("no glyph for {ch:?}"))?;
        Ok(f32::from(self.face.glyph_hor_advance(id).unwrap_or(0)) * em / self.upem)
    }
    /// Top and bottom of a glyph's bounding box, in pixels at `em`.
    fn extent(&self, ch: char, em: f32) -> Result<(f32, f32)> {
        let id = self.face.glyph_index(ch).ok_or_else(|| format!("no glyph for {ch:?}"))?;
        let b = self.face.glyph_bounding_box(id).ok_or("glyph without a box")?;
        Ok((f32::from(b.y_min) * em / self.upem, f32::from(b.y_max) * em / self.upem))
    }
    /// One glyph's coverage in a `w` x `h` cell, baseline at row `base`, ink shifted by `shift`.
    fn coverage(&self, ch: char, em: f32, fit: &Fit, shift: f32, base: f32, w: usize, h: usize) -> Vec<f32> {
        let mut out = vec![0f32; w * h];
        let Some(id) = self.face.glyph_index(ch) else { return out };
        let mut o = Outline::default();
        if self.face.outline_glyph(id, &mut o).is_none() {
            return out;
        }
        let s = em / self.upem;
        let tx = |x: f32| x * s + shift;
        let ty = |y: f32| base - fit.map(y * s);
        let mut r = Rasterizer::new(w, h);
        let (mut cur, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
        for seg in &o.0 {
            match *seg {
                Seg::M(x, y) => {
                    cur = (tx(x), ty(y));
                    start = cur;
                }
                Seg::L(x, y) => {
                    let p = (tx(x), ty(y));
                    r.draw_line(point(cur.0, cur.1), point(p.0, p.1));
                    cur = p;
                }
                Seg::Q(a, b, x, y) => {
                    let p = (tx(x), ty(y));
                    r.draw_quad(point(cur.0, cur.1), point(tx(a), ty(b)), point(p.0, p.1));
                    cur = p;
                }
                Seg::C(a, b, c, d, x, y) => {
                    let p = (tx(x), ty(y));
                    r.draw_cubic(point(cur.0, cur.1), point(tx(a), ty(b)), point(tx(c), ty(d)), point(p.0, p.1));
                    cur = p;
                }
                Seg::Z => {
                    r.draw_line(point(cur.0, cur.1), point(start.0, start.1));
                    cur = start;
                }
            }
        }
        r.for_each_pixel(|i, c| out[i] = c.min(1.0));
        out
    }
}

/// Cook `chars` of `font` at `em` into cells of `w` x `h`: baseline at `base`, grid fitted to the
/// lines of `lines` (the glyphs whose heights define x-height, cap height, ascender and descender).
fn cells(font: &Font, chars: &[char], em: f32, w: usize, h: usize, base: f32, tracking: u8, zones: &[char]) -> Result<(Vec<Vec<u8>>, Vec<u8>)> {
    let mut knots = vec![(0.0f32, 0.0f32)];
    let mut last = 0.0f32;
    for &z in zones {
        let (bottom, top) = font.extent(z, em)?;
        if z == 'p' {
            knots.push((bottom, bottom.round().min(-1.0)));
        } else {
            let to = top.round().max(last + 1.0);
            last = to;
            knots.push((top, to));
        }
    }
    knots.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let fit = Fit(knots);
    let mut out = Vec::new();
    let mut advances = Vec::new();
    for &ch in chars {
        let a = font.advance(ch, em)?;
        let ia = (a.round() as i32).max(1) as usize + usize::from(tracking);
        if ia > w {
            return err(format!("glyph {ch:?} advance {ia} exceeds the {w} texel cell"));
        }
        advances.push(ia as u8);
        let shift = (a.round() - a) / 2.0;
        let cov = font.coverage(ch, em, &fit, shift, base, w, h);
        let mut cell = vec![0u8; w * h];
        for (j, v) in cov.iter().enumerate() {
            // Row 0 stays blank.
            cell[j] = if j < w { 0 } else { level(*v) };
        }
        out.push(cell);
    }
    Ok((out, advances))
}

/// Lay `cells` (each `cw` x `ch`) out `per_row` to a row, rows `pitch` apart from row `v0` of `image`.
fn place(image: &mut [u8], cells: &[Vec<u8>], cw: usize, ch: usize, per_row: usize, rows: usize, pitch: usize, v0: usize) {
    for row in 0..rows {
        for col in 0..per_row {
            if let Some(cell) = cells.get(row * per_row + col) {
                for y in 0..ch {
                    for x in 0..cw {
                        image[(v0 + row * pitch + y) * 256 + GLYPH_U0 + col * cw + x] = cell[y * cw + x];
                    }
                }
            }
        }
    }
}

pub struct Strip {
    /// The disc/VRAM image: palette, then `IMAGE_ROWS` rows of 256 4bpp texels.
    pub blob: Vec<u8>,
    /// Integer advance of ASCII 32..126 (Perpetua).
    pub advances: Vec<u8>,
    /// Integer advance of `CAPS` (Trajan, tracking included).
    pub caps_advances: Vec<u8>,
    /// The widest capital's cell, which every capital cell shares.
    pub caps_w: usize,
    /// The image as one grey byte per texel, 256 wide, for a preview.
    pub preview: Vec<u8>,
}
pub fn caps_per_row(cw: usize) -> usize {
    (256 - GLYPH_U0) / cw
}
/// Cook the image: Perpetua body cells, TrajanPro-Bold capitals at the em that gives the body font's
/// cap height, and the Geo coin (12x12, 17 times its palette index per texel) in the cell after them.
pub fn cook(body_bytes: &[u8], caps_bytes: &[u8], coin: &[u8]) -> Result<Strip> {
    let body = Font::new(body_bytes)?;
    let chars: Vec<char> = (32u8..127).map(char::from).collect();
    let base = (EM * 0.75).round();
    let (body_cells, advances) = cells(&body, &chars, EM, CELL_W, CELL_H, base, 0, &['x', 'H', 'l', 'p'])?;
    let caps = Font::new(caps_bytes)?;
    let cap = body.extent('H', EM)?.1.round();
    let em = cap / caps.extent('H', 1.0)?.1;
    let cap_chars: Vec<char> = CAPS.chars().collect();
    let mut widest = 0;
    for &c in &cap_chars {
        widest = widest.max(caps.advance(c, em)?.round() as usize + 1);
    }
    let cw = widest.max(12);
    let per_row = caps_per_row(cw);
    if per_row * CAPS_ROWS < cap_chars.len() + 1 {
        return err(format!("the capitals and the coin need {} cells, {} fit", cap_chars.len() + 1, per_row * CAPS_ROWS));
    }
    let (mut cap_cells, caps_advances) = cells(&caps, &cap_chars, em, cw, CAPS_H, 10.0, 1, &['H'])?;
    if coin.len() != 12 * 12 {
        return err("the coin cell is not 12x12");
    }
    let mut coin_cell = vec![0u8; cw * CAPS_H];
    for y in 0..12 {
        for x in 0..12.min(cw) {
            coin_cell[y * cw + x] = ((u16::from(coin[y * 12 + x]) + 8) / 17) as u8;
        }
    }
    cap_cells.push(coin_cell);
    let mut image = vec![0u8; 256 * IMAGE_ROWS];
    place(&mut image, &body_cells, CELL_W, CELL_H, PER_ROW, ROWS, ROW_PITCH, 0);
    place(&mut image, &cap_cells, cw, CAPS_H, per_row, CAPS_ROWS, CAPS_H, CAPS_V0);
    let mut blob = Vec::with_capacity(STRIP_BYTES);
    for e in palette() {
        blob.extend_from_slice(&e.to_le_bytes());
    }
    blob.extend(image.chunks_exact(2).map(|p| p[0] | (p[1] << 4)));
    assert_eq!(blob.len(), STRIP_BYTES);
    let preview = image.iter().map(|t| t * 17).collect();
    Ok(Strip { blob, advances, caps_advances, caps_w: cw, preview })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_is_transparent_then_a_semi_transparent_ramp() {
        let p = palette();
        assert_eq!(p[0], 0);
        for (v, e) in p.iter().enumerate().skip(1) {
            assert_eq!(e & 0x8000, 0x8000, "entry {v} keeps the semi-transparency bit");
        }
        assert_eq!(p[15], 0x8000 | 0x7fff);
        assert!(p.windows(2).all(|w| (w[1] & 0x7fff) > (w[0] & 0x7fff)));
    }

    #[test]
    fn coverage_is_a_monotone_ramp_that_lifts_the_middle() {
        assert_eq!(level(0.0), 0);
        assert_eq!(level(1.0), 15);
        let mut last = 0;
        for i in 0..=100 {
            let v = level(i as f32 / 100.0);
            assert!(v >= last);
            last = v;
        }
        assert!(level(0.5) > 8, "half coverage is drawn brighter than half");
    }

    #[test]
    fn strips_match_the_guest_reservations() {
        assert_eq!(STRIP_BYTES, 12_832);
        assert_eq!(IMAGE_ROWS, 100);
        assert_eq!(PER_ROW * ROWS, GLYPHS);
        // The image has to fit the 64 x 256 halfword page the unused CLUT banks sit on.
        assert!(IMAGE_ROWS <= 104);
        assert!(GLYPH_U0 + PER_ROW * CELL_W <= 256);
    }

    #[test]
    fn the_fit_snaps_its_knots_and_keeps_slope_between() {
        let fit = Fit(vec![(-3.4, -3.0), (0.0, 0.0), (5.3, 5.0), (7.8, 8.0)]);
        assert_eq!(fit.map(0.0), 0.0);
        assert_eq!(fit.map(5.3), 5.0);
        assert_eq!(fit.map(7.8), 8.0);
        let mid = fit.map(2.65);
        assert!(mid > 2.0 && mid < 3.0);
    }
}
