//! The ornaments of the original dialogue box, for the text panel.
//!
//! The source's `DialogueManager` prefab (resources.assets:4446) is not a
//! square: a `DialogueBox` carries a `Fleur Top` and a `Fleur Bot` sprite
//! above and below the text, an `Arrow` (more to read) and a `Stop` (the last
//! page) under it, all frames of the `Dialogue Cln` tk2d collection
//! (resources.assets:21989), over a dark `backboard`. This cooks the final
//! frame of each ornament for the PS1 panel.
//!
//! Every piece is cut to its final pixel size once, here, so the guest draws
//! it 1:1 as a textured rectangle: no scaling, no texture filtering, no UV
//! arithmetic to get wrong. Coverage becomes the 16 grey levels of the shared
//! font palette (index 0 is transparent, 15 is full white), so the ornaments
//! need no palette of their own. A piece wider than one animation-cache slot
//! (64 pixels) is cut into slot-wide tiles, each streamed to VRAM by the
//! cache like any other animation texture.
//!
//! Writes `data/dialogue-art.hk` (the tiles' 4bpp texels back to back, rows
//! padded to whole halfwords, first pixel in the low nibble), the constants
//! the guest links (`data/dialogue_art.rs`) and a preview under
//! `.hkpsx/dialogue-art/`.
use crate::common::{err, Result};
use crate::cook::tk_sprite;
use crate::png::save_rgba;
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::Source;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

/// The collection every ornament is a frame of.
const COLLECTION: i64 = 21989;
/// Width of an animation-cache slot, which bounds a tile.
const TILE: usize = 64;

/// A piece: its name, the tk2d sprite id of its last frame, its size on the
/// PS1 screen. The fleurs keep the source's proportions to within a pixel; the
/// arrow and the stop are drawn larger than their share of the fleur so that
/// they read at 320x240.
const PIECES: [(&str, usize, usize, usize); 4] = [
    ("fleur_top", 35, 192, 25),
    ("fleur_bottom", 44, 128, 14),
    ("arrow", 11, 15, 17),
    ("stop", 15, 14, 17),
];

/// Coverage to one of 16 levels. Thin strokes lose their contrast when the
/// source is shrunk by a factor of three, so the ramp is steepened around the
/// middle: a stroke stays a stroke instead of turning into a grey haze.
fn level(alpha: u8, luma: u8) -> u8 {
    let t = f64::from(alpha) / 255.0 * f64::from(luma) / 255.0;
    let t = ((t - 0.08) / 0.84).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    // Blend the smoothstep with the straight ramp: half way keeps the tone
    // of anti-aliased curves and still firms up the edges.
    let v = 0.5 * s + 0.5 * t;
    (v * 15.0).round() as u8
}

/// The HUD's Geo icon: `Geo Sprite` of the UI collection (resources.assets:21155
/// sprite 88), a cluster of pale stones with a dark outline.
const UI_COLLECTION: i64 = 21155;
const COIN_SPRITE: usize = 88;
/// A glyph cell of the font strip, the size of the coin.
const COIN_PX: usize = 12;

/// The coin's grey: unlike the ornaments, its dark outline is opaque, so only
/// nearly transparent pixels become index 0 and every other pixel keeps at least
/// index 1 (the ramp's darkest opaque grey).
fn coin_level(alpha: u8, luma: u8) -> u8 {
    if alpha < 64 {
        0
    } else {
        ((u32::from(luma) * 15 + 127) / 255).clamp(1, 15) as u8
    }
}

/// The 4bpp indices of `image`, row-major, one byte per pixel.
fn indices(image: &Image) -> Vec<u8> {
    image
        .data
        .chunks_exact(4)
        .map(|p| level(p[3], p[0].max(p[1]).max(p[2])))
        .collect()
}

/// One tile of the cooked file.
struct Tile {
    offset: usize,
    width: usize,
    height: usize,
    /// Left edge of the tile inside its piece.
    dx: usize,
}

pub fn main(root: &Path, source: Option<&Path>) -> Result<()> {
    let source = match source {
        Some(directory) => Source::new(directory).map_err(|e| e.to_string())?,
        None => Source::from_doctor(root).map_err(|e| e.to_string())?,
    };
    let resources = source.file("resources.assets").map_err(|e| e.to_string())?;
    let object = source
        .object(&resources, COLLECTION)
        .map_err(|e| e.to_string())?;
    let collection = source.read(&object).map_err(|e| e.to_string())?;
    let mut textures = HashMap::new();
    let mut blob: Vec<u8> = Vec::new();
    let mut tiles: Vec<Tile> = Vec::new();
    // (first tile, tile count, width, height) per piece.
    let mut table: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut report = Vec::new();
    let preview_dir = root.join(".hkpsx/dialogue-art");
    std::fs::create_dir_all(&preview_dir).map_err(|e| e.to_string())?;
    let mut sheet = Image::new(Mode::Rgba, 640, 120);
    for p in sheet.data.chunks_exact_mut(4) {
        p.copy_from_slice(&[24, 28, 40, 255]);
    }
    let mut cursor = [4i64, 4i64];
    for (name, id, width, height) in PIECES {
        let (full, _) = tk_sprite(&source, &resources, &collection, id, &mut textures)?;
        let shrunk = full.resize(width, height, Filter::Lanczos);
        let grey = indices(&shrunk);
        let first = tiles.len();
        let mut dx = 0;
        while dx < width {
            let tw = (width - dx).min(TILE);
            // Rows are whole halfwords: four pixels.
            let padded = tw.div_ceil(4) * 4;
            let offset = blob.len();
            for y in 0..height {
                for x in (0..padded).step_by(2) {
                    let at = |xx: usize| {
                        if xx < tw {
                            grey[y * width + dx + xx]
                        } else {
                            0
                        }
                    };
                    blob.push(at(x) | (at(x + 1) << 4));
                }
            }
            tiles.push(Tile {
                offset,
                width: tw,
                height,
                dx,
            });
            dx += tw;
        }
        table.push((first, tiles.len() - first, width, height));
        // The preview shows what the GPU will show: the ramp as grey.
        let mut shown = Image::new(Mode::Rgba, width, height);
        for (i, &v) in grey.iter().enumerate() {
            let g = (u32::from(v) * 255 / 15) as u8;
            shown.data[i * 4..i * 4 + 4].copy_from_slice(&[g, g, g, 255]);
        }
        sheet.paste(&shown, cursor[0], cursor[1], None);
        cursor[1] += height as i64 + 6;
        save_rgba(&preview_dir.join(format!("{name}.png")), &shown)?;
        report.push(json!({
            "piece": name,
            "source": format!("resources.assets:{COLLECTION} sprite {id}"),
            "source_size": [full.width, full.height],
            "size": [width, height],
            "tiles": tiles.len() - first,
        }));
    }
    // The Geo icon, cut to one font cell. read_points.py pastes it into the
    // glyph strip's first unused cell (95), where the guest draws it as a glyph:
    // no VRAM of its own, drawn 1:1 beside the Geo count.
    {
        let collection = source
            .read(&source.object(&resources, UI_COLLECTION).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let (full, _) = tk_sprite(&source, &resources, &collection, COIN_SPRITE, &mut textures)?;
        let coin = full.resize(COIN_PX, COIN_PX, Filter::Lanczos);
        let cell: Vec<u8> = coin
            .data
            .chunks_exact(4)
            .map(|p| coin_level(p[3], p[0].max(p[1]).max(p[2])) * 17)
            .collect();
        std::fs::write(root.join("data/hud-coin.l"), &cell).map_err(|e| e.to_string())?;
        let mut shown = Image::new(Mode::Rgba, COIN_PX, COIN_PX);
        for (i, &g) in cell.iter().enumerate() {
            shown.data[i * 4..i * 4 + 4].copy_from_slice(&[g, g, g, if g == 0 { 0 } else { 255 }]);
        }
        save_rgba(&preview_dir.join("coin.png"), &shown)?;
    }
    if blob.len() > 4096 {
        return err(format!(
            "dialogue art is {} bytes, the resident budget is 4096",
            blob.len()
        ));
    }
    save_rgba(&preview_dir.join("sheet.png"), &sheet)?;
    std::fs::write(root.join("data/dialogue-art.hk"), &blob).map_err(|e| e.to_string())?;
    let mut rust = String::from("// Generated by hk-cook dialogue-art (host/hk-cook/src/dialogue_art.rs).\n");
    rust += &format!("pub const DIALOGUE_ART_BYTES: usize = {};\n", blob.len());
    rust += &format!("pub const DIALOGUE_TILE_COUNT: usize = {};\n", tiles.len());
    rust += "/// offset into the art, width and height in pixels, left edge in the piece.\n";
    rust += "pub static DIALOGUE_TILES: [(u16, u8, u8, u8); DIALOGUE_TILE_COUNT] = [\n";
    for t in &tiles {
        rust += &format!("    ({}, {}, {}, {}),\n", t.offset, t.width, t.height, t.dx);
    }
    rust += "];\n";
    rust += "/// first tile, tile count, width, height in pixels.\n";
    for ((name, ..), (first, count, w, h)) in PIECES.iter().zip(&table) {
        rust += &format!(
            "pub const DIALOGUE_{}: (usize, usize, u16, u16) = ({first}, {count}, {w}, {h});\n",
            name.to_uppercase()
        );
    }
    std::fs::write(root.join("data/dialogue_art.rs"), rust).map_err(|e| e.to_string())?;
    std::fs::write(
        preview_dir.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "pieces": report,
            "bytes": blob.len(),
            "palette": "the shared font ramp (CLUT 320,481): index 0 transparent, 1..15 grey",
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Dialogue art: {} pieces, {} tiles, {} bytes",
        PIECES.len(),
        tiles.len(),
        blob.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_a_monotone_ramp_from_transparent_to_white() {
        assert_eq!(level(0, 255), 0);
        assert_eq!(level(255, 255), 15);
        let mut last = 0;
        for a in 0..=255u8 {
            let v = level(a, 255);
            assert!(v >= last && v <= 15);
            last = v;
        }
    }

    #[test]
    fn a_piece_is_cut_into_slot_wide_tiles() {
        for (_, _, width, _) in PIECES {
            let tiles = width.div_ceil(TILE);
            assert!((1..=3).contains(&tiles));
        }
    }
}
