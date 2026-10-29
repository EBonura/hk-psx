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

/// The vertical fit of `font` at `em`: x-height, cap height, ascender and descender lines of
/// `zones` (the glyphs that define them) moved onto whole pixels.
fn fit_for(font: &Font, em: f32, zones: &[char]) -> Result<Fit> {
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
    Ok(Fit(knots))
}

/// Cook `chars` of `font` at `em` into cells of `w` x `h`: baseline at `base`, grid fitted to the
/// lines of `lines` (the glyphs whose heights define x-height, cap height, ascender and descender).
fn cells(font: &Font, chars: &[char], em: f32, w: usize, h: usize, base: f32, tracking: u8, zones: &[char]) -> Result<(Vec<Vec<u8>>, Vec<u8>)> {
    let fit = fit_for(font, em, zones)?;
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

/// The opening poem's cells (host/hk-build/intro.rs): the verse in the dialogue's own 14 px cells but three
/// rows taller, so that the descenders the dialogue's 14 row cells cut (the `g`'s lower bowl) are whole,
/// and the author's line at the original's smaller size (3.85 against 5), rasterised and grid fitted at
/// its own em instead of the verse's cells scaled down.
pub const POEM_BODY_W: usize = CELL_W;
pub const POEM_BODY_H: usize = CELL_H + 3;
pub const POEM_AUTHOR_W: usize = 11;
pub const POEM_AUTHOR_H: usize = 14;
pub const POEM_AUTHOR_EM: f32 = EM * 3.85 / 5.0;

/// The poem's cells for ASCII 32..127 (one byte of palette index a texel, row by row).
pub struct PoemFont {
    pub body: Vec<Vec<u8>>,
    pub author: Vec<Vec<u8>>,
    /// The author's advances at its em (the verse's are the dialogue's, data/font-metrics.json).
    pub author_advances: Vec<u8>,
}

impl PoemFont {
    /// The file: the two cell sizes, the author's 95 advances, then the verse's and the author's 95 cells.
    pub fn blob(&self) -> Vec<u8> {
        let mut out = vec![POEM_BODY_W as u8, POEM_BODY_H as u8, POEM_AUTHOR_W as u8, POEM_AUTHOR_H as u8];
        out.extend_from_slice(&self.author_advances);
        for cell in self.body.iter().chain(&self.author) {
            out.extend_from_slice(cell);
        }
        out
    }
}

pub fn cook_poem(body_bytes: &[u8]) -> Result<PoemFont> {
    let body = Font::new(body_bytes)?;
    let chars: Vec<char> = (32u8..127).map(char::from).collect();
    let zones = ['x', 'H', 'l', 'p'];
    // The verse: the same em, baseline and fit as the dialogue's cells, so its rows match them exactly.
    let (verse, _) = cells(&body, &chars, EM, POEM_BODY_W, POEM_BODY_H, (EM * 0.75).round(), 0, &zones)?;
    // The author: a baseline row lower, as the cell keeps its first row blank.
    let (author, author_advances) =
        cells(&body, &chars, POEM_AUTHOR_EM, POEM_AUTHOR_W, POEM_AUTHOR_H, (POEM_AUTHOR_EM * 0.75).round() + 1.0, 0, &zones)?;
    Ok(PoemFont { body: verse, author, author_advances })
}

/// The area and boss cards' main lines, which the card strip carries the glyphs of
/// (host/title_cards.py asserts every cooked main line is drawable from them).
pub const CARD_MAINS: [&str; 5] = ["Crossroads", "Dirtmouth", "Greenpath", "King's Pass", "Mound"];
/// Every line of a boss card (`Titles` sheet: `_SUPER`, `_MAIN`, `_SUB`). They are drawn in small
/// capitals, as the original's boss title is: a capital of the source text is a full capital, a
/// lowercase letter is the same capital at the small size.
pub const BOSS_LINES: [&str; 5] = ["Brooding", "False Knight", "Mawlek", "Gruz", "Mother"];
/// The boss title's capital height in texels, full and small. The original's is 3.7 % and 2.6 % of
/// the screen height (hkref frames of Crossroads_10 at 960x540: 20 and 14 rows of 540).
pub const BOSS_BIG_CAP: f32 = 9.0;
pub const BOSS_SMALL_CAP: f32 = 6.0;
/// The cards' em in pixels: Perpetua rasterised at this size, grid fitted, instead of the 14 px
/// glyphs doubled.
pub const CARD_EM: f32 = 40.0;
/// The strip's VRAM rectangle, x, y, width and height in halfwords: the twelve rows below the
/// scenery CLUT rows that held the font before the font image moved, outside both frame buffers.
/// Texels are 4bpp, so it is 1280 texels wide: five texture pages of 256 at y 256, rows 244..255.
pub const CARD_RECT: (u16, u16, u16, u16) = (0, 500, 320, 12);
pub const CARD_ROWS: usize = 12;
pub const CARD_PAGE_W: usize = 256;
pub const CARD_PAGES: usize = 5;
pub const CARD_STRIP_BYTES: usize = CARD_PAGES * CARD_PAGE_W * CARD_ROWS / 2;
/// Rows from the top of a glyph's canvas to its baseline, and the canvas height.
const CARD_BASE: usize = 31;
const CARD_CANVAS_H: usize = 52;

/// One piece of a card glyph, at most `CARD_ROWS` rows tall so it fits the strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardBand {
    /// Texel column in the strip's 1280, and row in the strip.
    pub x: u16,
    pub v: u8,
    pub w: u8,
    pub h: u8,
    /// Where it draws, from the glyph's pen position and the top of the line.
    pub dx: i8,
    pub dy: u8,
}
pub struct CardGlyph {
    pub ch: char,
    pub advance: u8,
    pub bands: Vec<CardBand>,
}
pub struct Cards {
    /// `CARD_STRIP_BYTES` of 4bpp texels, uploaded at `CARD_RECT`.
    pub blob: Vec<u8>,
    /// Sorted by character.
    pub glyphs: Vec<CardGlyph>,
    /// The boss title's glyphs (`BOSS_LINES`), sorted by character: a capital is the full size
    /// capital and a lowercase letter its small capital. Their bands share the flat band table.
    pub boss: Vec<CardGlyph>,
    /// Texels used out of the strip's 15,360.
    pub used: usize,
}

struct Free {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}
/// Guillotine packer over the five 256 x 12 pages: the best fit by leftover area, split into the
/// part right of the piece and the part under it. A piece never crosses a page.
fn pack(sizes: &[(usize, usize)]) -> Result<Vec<(usize, usize)>> {
    let mut free: Vec<Free> =
        (0..CARD_PAGES).map(|p| Free { x: p * CARD_PAGE_W, y: 0, w: CARD_PAGE_W, h: CARD_ROWS }).collect();
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse((sizes[i].1, sizes[i].0)));
    let mut at = vec![(0, 0); sizes.len()];
    for i in order {
        let (w, h) = sizes[i];
        let best = free
            .iter()
            .enumerate()
            .filter(|(_, f)| f.w >= w && f.h >= h)
            .min_by_key(|(_, f)| (f.w * f.h - w * h, f.x))
            .map(|(k, _)| k);
        let Some(k) = best else {
            return err(format!("the card strip is full: a {w}x{h} piece has no room"));
        };
        let f = free.swap_remove(k);
        at[i] = (f.x, f.y);
        if f.w > w {
            free.push(Free { x: f.x + w, y: f.y, w: f.w - w, h: f.h });
        }
        if f.h > h {
            free.push(Free { x: f.x, y: f.y + h, w, h: f.h - h });
        }
    }
    Ok(at)
}

/// Cook the glyphs of `CARD_MAINS` at `CARD_EM` (Perpetua) and of `BOSS_LINES` in two capital
/// sizes (TrajanPro-Bold): each grid fitted, trimmed to its ink and cut into bands of at most
/// `CARD_ROWS` rows, packed into the strip.
pub fn cook_cards(body_bytes: &[u8], caps_bytes: &[u8]) -> Result<Cards> {
    let font = Font::new(body_bytes)?;
    let trajan = Font::new(caps_bytes)?;
    let mut chars: Vec<char> = CARD_MAINS.iter().flat_map(|m| m.chars()).filter(|&c| c != ' ').collect();
    chars.sort_unstable();
    chars.dedup();
    let fit = fit_for(&font, CARD_EM, &['x', 'H', 'l', 'p'])?;
    struct Raster {
        ch: char,
        advance: u8,
        dx: i32,
        top: usize,
        rows: Vec<Vec<u8>>,
    }
    const PAD: usize = 4;
    /// One glyph of `font` at `em`: its ink trimmed from a canvas whose baseline is `CARD_BASE`.
    fn raster(font: &Font, key: char, glyph: char, em: f32, fit: &Fit, tracking: u8) -> Result<Raster> {
        let a = font.advance(glyph, em)?;
        let advance = a.round().max(1.0) as usize + usize::from(tracking);
        let shift = PAD as f32 + (a.round() - a) / 2.0;
        let w = advance + 2 * PAD + 4;
        let cov = font.coverage(glyph, em, fit, shift, CARD_BASE as f32, w, CARD_CANVAS_H);
        let lv: Vec<u8> = cov.iter().map(|&c| level(c)).collect();
        let ink = |x: usize, y: usize| lv[y * w + x] != 0;
        let rows: Vec<usize> = (0..CARD_CANVAS_H).filter(|&y| (0..w).any(|x| ink(x, y))).collect();
        let cols: Vec<usize> = (0..w).filter(|&x| (0..CARD_CANVAS_H).any(|y| ink(x, y))).collect();
        let (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) =
            (rows.first(), rows.last(), cols.first(), cols.last())
        else {
            return err(format!("card glyph {key:?} has no ink"));
        };
        if y1 + 1 >= CARD_CANVAS_H || y0 == 0 {
            return err(format!("card glyph {key:?} leaves the {CARD_CANVAS_H} row canvas"));
        }
        let strip: Vec<Vec<u8>> = (y0..=y1).map(|y| lv[y * w + x0..y * w + x1 + 1].to_vec()).collect();
        Ok(Raster { ch: key, advance: advance as u8, dx: x0 as i32 - PAD as i32, top: y0, rows: strip })
    }
    let mut rasters = Vec::new();
    for &ch in &chars {
        rasters.push(raster(&font, ch, ch, CARD_EM, &fit, 0)?);
    }
    let area_count = rasters.len();
    let mut boss_chars: Vec<char> = BOSS_LINES.iter().flat_map(|m| m.chars()).filter(|&c| c != ' ').collect();
    boss_chars.sort_unstable();
    boss_chars.dedup();
    let cap_unit = trajan.extent('H', 1.0)?.1;
    for &ch in &boss_chars {
        let cap = if ch.is_ascii_uppercase() { BOSS_BIG_CAP } else { BOSS_SMALL_CAP };
        let em = cap / cap_unit;
        let fit = fit_for(&trajan, em, &['H'])?;
        rasters.push(raster(&trajan, ch, ch.to_ascii_uppercase(), em, &fit, 1)?);
    }
    // Bands: equal heights, as few as the 12 row limit allows.
    struct Piece {
        glyph: usize,
        top: usize,
        h: usize,
        w: usize,
    }
    let mut pieces = Vec::new();
    for (g, r) in rasters.iter().enumerate() {
        let height = r.rows.len();
        let count = height.div_ceil(CARD_ROWS);
        let mut y = 0;
        for k in 0..count {
            let h = (height - y).div_ceil(count - k);
            pieces.push(Piece { glyph: g, top: y, h, w: r.rows[0].len() });
            y += h;
        }
    }
    let at = pack(&pieces.iter().map(|p| (p.w, p.h)).collect::<Vec<_>>())?;
    let mut image = vec![0u8; CARD_PAGES * CARD_PAGE_W * CARD_ROWS];
    let width = CARD_PAGES * CARD_PAGE_W;
    let mut glyphs: Vec<CardGlyph> = rasters
        .iter()
        .map(|r| CardGlyph { ch: r.ch, advance: r.advance, bands: Vec::new() })
        .collect();
    let mut used = 0;
    for (p, &(x, y)) in pieces.iter().zip(&at) {
        let r = &rasters[p.glyph];
        for row in 0..p.h {
            for col in 0..p.w {
                image[(y + row) * width + x + col] = r.rows[p.top + row][col];
            }
        }
        used += p.w * p.h;
        glyphs[p.glyph].bands.push(CardBand {
            x: x as u16,
            v: y as u8,
            w: p.w as u8,
            h: p.h as u8,
            dx: i8::try_from(r.dx).map_err(|_| "card glyph offset")?,
            dy: u8::try_from(r.top + p.top).map_err(|_| "card glyph row")?,
        });
    }
    let blob: Vec<u8> = image.chunks_exact(2).map(|t| t[0] | (t[1] << 4)).collect();
    assert_eq!(blob.len(), CARD_STRIP_BYTES);
    let boss = glyphs.split_off(area_count);
    Ok(Cards { blob, glyphs, boss, used })
}

/// The card strip as a grey-ramp image, 1280 x 12, for a preview.
pub fn cards_preview(cards: &Cards) -> Vec<u8> {
    let mut out = Vec::with_capacity(cards.blob.len() * 2);
    for b in &cards.blob {
        out.push((b & 15) * 17);
        out.push((b >> 4) * 17);
    }
    out
}

/// `font_layout.rs`' card glyph table: a flat band table and, for each character in `CARD_CHARS`,
/// the index of its first band (and one past the last), so a glyph costs no pointer.
pub fn cards_rust(cards: &Cards) -> String {
    let chars: String = cards.glyphs.iter().map(|g| g.ch).collect();
    let boss_chars: String = cards.boss.iter().map(|g| g.ch).collect();
    let mut first = vec![0usize];
    let mut boss_first = Vec::new();
    let mut bands = String::new();
    let mut n = 0;
    for (k, g) in cards.glyphs.iter().chain(&cards.boss).enumerate() {
        if k == cards.glyphs.len() {
            boss_first.push(n);
        }
        for b in &g.bands {
            bands += &format!(
                "    CardBand {{ x: {}, v: {}, w: {}, h: {}, dx: {}, dy: {} }},\n",
                b.x, b.v, b.w, b.h, b.dx, b.dy
            );
            n += 1;
        }
        if k < cards.glyphs.len() {
            first.push(n);
        } else {
            boss_first.push(n);
        }
    }
    let list = |v: &[String]| v.join(", ");
    let nums = |v: &[usize]| list(&v.iter().map(|f| f.to_string()).collect::<Vec<_>>());
    format!(
        "/// The area cards' main lines at {em} px (font_strip.rs `cook_cards`): the glyphs the\n\
         /// cards use, each cut into bands of at most {rows} rows that sit in the {bytes} byte strip\n\
         /// uploaded at ({rx}, {ry}) as {rw}x{rh} halfwords (texture pages x 0..256 at y 256, rows 244..255).\n\
         pub const CARD_BYTES: usize = {bytes};\n\
         pub const CARD_PAGE_Y: u16 = 256;\n\
         pub const CARD_V0: u8 = {v0};\n\
         pub const CARD_CHARS: &[u8] = b\"{chars}\";\n\
         pub const CARD_ADVANCES: [u8; {count}] = [{adv}];\n\
         pub const CARD_FIRST: [u8; {firsts}] = [{first}];\n\
         /// The boss cards' small capitals (`BOSS_LINES`): the glyphs by the source text's letter, a\n\
         /// capital at {big} rows and a lowercase letter as the same capital at {small}. Their bands\n\
         /// follow the area glyphs' in `CARD_BANDS`; `BOSS_FIRST` indexes them as `CARD_FIRST` does.\n\
         pub const BOSS_CHARS: &[u8] = b\"{boss_chars}\";\n\
         pub const BOSS_ADVANCES: [u8; {bcount}] = [{badv}];\n\
         pub const BOSS_FIRST: [u16; {bfirsts}] = [{bfirst}];\n\
         /// Ink position in the strip, size, and where it draws from the pen and the line's top.\n\
         pub struct CardBand {{ pub x: u16, pub v: u8, pub w: u8, pub h: u8, pub dx: i8, pub dy: u8 }}\n\
         pub static CARD_BANDS: [CardBand; {nbands}] = [\n{bands}];\n\
         /// Rows from the top of a card line to its baseline.\n\
         pub const CARD_BASE: i16 = {base};\n",
        em = CARD_EM,
        big = BOSS_BIG_CAP,
        small = BOSS_SMALL_CAP,
        rows = CARD_ROWS,
        bytes = CARD_STRIP_BYTES,
        rx = CARD_RECT.0,
        ry = CARD_RECT.1,
        rw = CARD_RECT.2,
        rh = CARD_RECT.3,
        v0 = 256 - CARD_ROWS,
        chars = chars,
        count = cards.glyphs.len(),
        adv = list(&cards.glyphs.iter().map(|g| g.advance.to_string()).collect::<Vec<_>>()),
        firsts = first.len(),
        first = nums(&first),
        boss_chars = boss_chars,
        bcount = cards.boss.len(),
        badv = list(&cards.boss.iter().map(|g| g.advance.to_string()).collect::<Vec<_>>()),
        bfirsts = boss_first.len(),
        bfirst = nums(&boss_first),
        nbands = n,
        bands = bands,
        base = CARD_BASE,
    )
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
