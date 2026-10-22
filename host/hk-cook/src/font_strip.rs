//! The panel font: Perpetua rasterised once, at the size the PS1 draws it.
//!
//! Every menu and dialogue screen draws text from one 4bpp glyph strip: 95
//! ASCII cells of 12x12 texels (21 to a 256 texel row, five rows) and a
//! 16 entry palette (data/read_font.hk, 7,712 bytes). This writes that strip
//! and the advances the layout code shares (data/font-metrics.json).
//!
//! The glyphs are the source font's own outlines, unhinted, anti-aliased to
//! 16 coverage levels:
//!
//! - Perpetua's strokes are thinner than a pixel at this size. A 1-bit
//!   hinted raster (what this replaced) turns each stem into a whole pixel
//!   column, so `m` and `w` lose their arches and read as `rn` and `ru`.
//!   Partial coverage keeps the stem weight and the arch.
//! - Coverage is lifted by a gamma below one: the glyphs are drawn as white
//!   added to the screen, and a display shows half coverage darker than half
//!   brightness, so linear coverage makes light text look thin.
//! - The advance is the real advance rounded to the nearest pixel (the
//!   replaced strip took the next whole pixel up, 0.5 px too wide a glyph on
//!   average and by a different amount for each), and the glyph is placed
//!   inside it by half the rounding error, so the gap between letters stays
//!   even instead of drifting with the rounding.
//! - One baseline for every glyph (row 9 of 12), whatever the glyph.
//!
//! The palette's entries other than 0 carry the semi-transparency bit (bit
//! 15), so a glyph drawn with a semi-transparent command adds its grey to the
//! screen: an edge pixel brightens what is behind it instead of overwriting
//! it with a dark grey, and a glyph fades with its tint. Index 0 stays fully
//! transparent. A command without the semi-transparent bit ignores the bit
//! (the ornaments, the Geo coin).
use crate::common::{err, Result};
use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};

/// Pixels per em: the glyph cell is 12 texels.
pub const EM: f32 = 12.0;
/// Rows above the baseline in a cell. Row 0 stays blank (a scaled polygon
/// starts one texel early), the tallest ascender is 8.
pub const BASELINE: f32 = 9.0;
/// Coverage exponent (see the module comment) and a small gain on top.
pub const GAMMA: f32 = 0.6;
pub const GAIN: f32 = 1.1;
pub const CELL: usize = 12;
pub const PER_ROW: usize = 21;
pub const ROWS: usize = 5;
/// Where cells start inside the 256 texel row (game/src/menu_state.rs `GLYPH_U0`).
pub const GLYPH_U0: usize = 4;
pub const GLYPHS: usize = 95;
/// An overhang this strong left of the origin moves the glyph right a pixel.
const OVERHANG: f32 = 0.25;

pub const STRIP_BYTES: usize = 32 + ROWS * 256 * CELL / 2;

/// The 16 palette entries: transparent, then the grey ramp with the
/// semi-transparency bit.
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

/// One glyph's coverage, `w` x `h`, origin at column `origin`, baseline at `BASELINE`.
fn coverage(font: &FontRef, scale: PxScale, ch: char, shift: f32, origin: i32, w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    let g = font
        .glyph_id(ch)
        .with_scale_and_position(scale, point(origin as f32 + shift, BASELINE));
    if let Some(o) = font.outline_glyph(g) {
        let b = o.px_bounds();
        o.draw(|px, py, c| {
            let (x, y) = (b.min.x as i32 + px as i32, b.min.y as i32 + py as i32);
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                let i = y as usize * w + x as usize;
                out[i] = (out[i] + c).min(1.0);
            }
        });
    }
    out
}

pub struct Strip {
    /// The disc/VRAM image: palette then five 256x12 rows of 4bpp texels.
    pub blob: Vec<u8>,
    /// Integer advance of ASCII 32..126.
    pub advances: [u8; GLYPHS],
    /// Glyphs moved right a pixel to keep an overhang.
    pub shifted: Vec<char>,
    /// The strip as one grey byte per texel, 256 x 60, for a preview.
    pub preview: Vec<u8>,
}

/// Cook the strip. `coin` is the Geo icon's 12x12 cell (one byte per texel,
/// 17 times its palette index), pasted into cell 95.
pub fn cook(font_bytes: &[u8], coin: Option<&[u8]>) -> Result<Strip> {
    let font = FontRef::try_from_slice(font_bytes).map_err(|e| format!("font: {e}"))?;
    let upem = font.units_per_em().ok_or("font has no units per em")?;
    let k = font.height_unscaled() / upem;
    let scale = PxScale { x: EM * k, y: EM * k };
    let sf = font.as_scaled(scale);
    let mut advances = [0u8; GLYPHS];
    let mut shifted = Vec::new();
    let mut cells: Vec<[u8; CELL * CELL]> = Vec::new();
    for (i, c) in (32u8..127).enumerate() {
        let ch = c as char;
        let a = sf.h_advance(font.glyph_id(ch));
        let ia = (a.round() as i32).max(1);
        if ia > CELL as i32 {
            return err(format!("glyph {ch:?} advance {ia} exceeds the {CELL} texel cell"));
        }
        advances[i] = ia as u8;
        let shift = (ia as f32 - a) / 2.0;
        // Ink left of the origin: probe one column over and move the glyph right if it is there.
        let probe = coverage(&font, scale, ch, shift, 1, CELL, CELL);
        let left = (0..CELL).map(|y| probe[y * CELL]).fold(0.0f32, f32::max);
        let origin = if left >= OVERHANG {
            shifted.push(ch);
            1
        } else {
            0
        };
        let cov = coverage(&font, scale, ch, shift, origin, CELL, CELL);
        let mut cell = [0u8; CELL * CELL];
        for (j, v) in cov.iter().enumerate() {
            // Row 0 stays blank.
            cell[j] = if j < CELL { 0 } else { level(*v) };
        }
        cells.push(cell);
    }
    if let Some(coin) = coin {
        if coin.len() != CELL * CELL {
            return err("the coin cell is not 12x12");
        }
        let mut cell = [0u8; CELL * CELL];
        for (j, v) in coin.iter().enumerate() {
            cell[j] = ((u16::from(*v) + 8) / 17) as u8;
        }
        cells.push(cell);
    }
    let mut blob = Vec::with_capacity(STRIP_BYTES);
    for e in palette() {
        blob.extend_from_slice(&e.to_le_bytes());
    }
    let mut preview = vec![0u8; 256 * CELL * ROWS];
    for row in 0..ROWS {
        let mut texels = vec![0u8; 256 * CELL];
        for col in 0..PER_ROW {
            if let Some(cell) = cells.get(row * PER_ROW + col) {
                for y in 0..CELL {
                    for x in 0..CELL {
                        texels[y * 256 + GLYPH_U0 + col * CELL + x] = cell[y * CELL + x];
                    }
                }
            }
        }
        for (j, t) in texels.iter().enumerate() {
            preview[row * 256 * CELL + j] = t * 17;
        }
        blob.extend(texels.chunks_exact(2).map(|p| p[0] | (p[1] << 4)));
    }
    assert_eq!(blob.len(), STRIP_BYTES);
    Ok(Strip {
        blob,
        advances,
        shifted,
        preview,
    })
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
    fn strip_size_matches_the_guest_reservation() {
        assert_eq!(STRIP_BYTES, 7712);
    }
}
