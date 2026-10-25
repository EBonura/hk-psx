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
//! The pause menu and the charm board share the pieces: the pause screen's
//! top and bottom fleurs are the last frames of the `pause_top_fleur` and
//! `bottom_fleur` sprites (resources.assets sprites 3137 and 1697), cut the
//! same way.
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

/// Where a piece's pixels come from.
#[derive(Clone, Copy)]
enum Origin {
    /// A frame of the `Dialogue Cln` tk2d collection.
    Tk(usize),
    /// A Unity `Sprite` of resources.assets, by path id.
    Sprite(i64),
    /// A frame of another tk2d collection of resources.assets: (collection path id, frame).
    TkIn(i64, usize),
    /// No source art: a soft disc, solid at the middle and fading to nothing at the rim (the
    /// glow behind the charm board's soul vessel).
    Orb,
}

/// A piece: its name, where it comes from, its size on the PS1 screen. The
/// fleurs keep the source's proportions to within a pixel; the arrow and the
/// stop are drawn larger than their share of the fleur so that they read at
/// 320x240. The pause screen's two fleurs are the final frames of their
/// animations (`pause_top_fleur0008`, `bottom_fleur0008`).
const PIECES: [(&str, Origin, usize, usize); 11] = [
    ("fleur_top", Origin::Tk(35), 192, 25),
    ("fleur_bottom", Origin::Tk(44), 128, 14),
    ("arrow", Origin::Tk(11), 15, 17),
    ("stop", Origin::Tk(15), 14, 17),
    ("pause_top", Origin::Sprite(3137), 160, 46),
    ("pause_bottom", Origin::Sprite(1697), 112, 24),
    // The charm board's own parts (the source's `Charms` pane: the slot backboard of `Collected Charms`
    // and the unlit and lit notch circles of `Notches`; its cursor is four corner flourishes, drawn
    // as flat corners here because a rectangle cannot be flipped), after everything the
    // other screens ask for: the guest asks for this range alone.
    ("board_slot", Origin::Sprite(3731), 18, 18),
    ("board_notch", Origin::Sprite(2531), 10, 10),
    ("board_notch_lit", Origin::Sprite(2221), 10, 10),
    // The soul vessel's glow on the board: the source's is a pink disc about 70 of 960 canvas texels
    // across, 23 texels at this screen's width. The guest tints it pink and adds it.
    ("board_orb", Origin::Orb, 24, 24),
    // The prompt marker's flourish (`Arrow Prompt New` of the `Prompts Cln` collection, resources.assets
    // 22054, frame 89 = prompt_idle0000): curls above, a chevron below, the label between. It spans about
    // 2.4 by 2.2 world units, which are 14.8 px each on this screen. After the board's pieces: the guest
    // asks for it alone.
    ("prompt", Origin::TkIn(22054, 89), 36, 34),
];

/// The orb: coverage 1 out to a third of the radius, then a smoothstep to nothing at the rim.
fn orb_image(size: usize) -> Image {
    let mut image = Image::new(Mode::Rgba, size, size);
    let (c, r) = ((size as f64 - 1.0) / 2.0, size as f64 / 2.0);
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt();
            let t = ((r - d) / (r * 0.6)).clamp(0.0, 1.0);
            let a = (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8;
            image.data[(y * size + x) * 4..(y * size + x) * 4 + 4].copy_from_slice(&[255, 255, 255, a]);
        }
    }
    image
}

/// Coverage to one of 16 levels. Thin strokes lose their contrast when the
/// source is shrunk by a factor of three, so the ramp is steepened around the
/// middle: a stroke stays a stroke instead of turning into a grey haze.
pub(crate) fn level(alpha: u8, luma: u8) -> u8 {
    let t = f64::from(alpha) / 255.0 * f64::from(luma) / 255.0;
    let t = ((t - 0.08) / 0.84).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t);
    // Blend the smoothstep with the straight ramp: half way keeps the tone
    // of anti-aliased curves and still firms up the edges.
    let v = 0.5 * s + 0.5 * t;
    (v * 15.0).round() as u8
}

/// The pieces the title draws whole, in the order its VRAM strip keeps them, and the menu
/// pointer's source sprite and size.
const TITLE_PIECES: [&str; 2] = ["fleur_top", "fleur_bottom"];
const TITLE_ORDER: [&str; 4] = ["fleur_top", "fleur_bottom", "pointer", "pointer_mirrored"];
const POINTER_SPRITE: i64 = 101;
/// The frames of `Menu Fleur Reveal` (sharedassets0.assets clip 58) the pointer shows while it
/// opens, sampled as a 60 Hz screen does: the clip runs at 120 frames a second, so each presented
/// frame advances two sprite frames (anim0000, 0002, 0004, 0006, 0008), and the sixth is the final
/// sprite above.
const POINTER_STEPS: [i64; 5] = [118, 131, 128, 96, 147];
const POINTER_W: usize = 20;
const POINTER_H: usize = 14;

/// `grey` (one index per pixel) as 4bpp rows padded to whole halfwords, first pixel in the low nibble.
pub(crate) fn whole_rows(grey: &[u8], width: usize, height: usize) -> Vec<u8> {
    let padded = width.div_ceil(4) * 4;
    let mut out = Vec::with_capacity(padded / 2 * height);
    for y in 0..height {
        for x in (0..padded).step_by(2) {
            let at = |xx: usize| if xx < width { grey[y * width + xx] } else { 0 };
            out.push(at(x) | (at(x + 1) << 4));
        }
    }
    out
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
pub(crate) fn indices(image: &Image) -> Vec<u8> {
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

/// A source font by name: its serialized id and the TrueType bytes.
fn font_data(source: &Source, file: &std::sync::Arc<hk_unity::serialized::SerializedFile>, name: &str) -> Result<(String, Vec<u8>)> {
    for info in file.objects.iter().filter(|i| i.class_id == 128) {
        let object = source.object(file, info.path_id).map_err(|e| e.to_string())?;
        let value = source.read(&object).map_err(|e| e.to_string())?;
        if value.get("m_Name").and_then(|n| n.str()).as_deref() != Some(name) {
            continue;
        }
        let bytes = match value.get("m_FontData") {
            Some(hk_unity::Value::Bytes(b)) => b.clone(),
            Some(hk_unity::Value::List(l)) => l.iter().map(|v| v.int().unwrap_or(0) as u8).collect(),
            _ => return err(format!("{name} has no font data")),
        };
        return Ok((object.sid(), bytes));
    }
    err(format!("no {name} font"))
}

fn hk_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
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
    // The title's ornaments are the same pieces, whole instead of cut to cache tiles, plus the
    // menu pointer (see `title_pieces`): rows padded to halfwords, in the order the guest uploads.
    let mut title: Vec<(&str, usize, usize, Vec<u8>)> = Vec::new();
    let pointer_steps: (Vec<Vec<u8>>, Vec<Vec<u8>>);
    for (name, from, width, height) in PIECES {
        let (full, source_name) = match from {
            Origin::Tk(id) => (
                tk_sprite(&source, &resources, &collection, id, &mut textures)?.0,
                format!("resources.assets:{COLLECTION} sprite {id}"),
            ),
            Origin::TkIn(collection_id, frame) => {
                let object = source
                    .object(&resources, collection_id)
                    .map_err(|e| e.to_string())?;
                let other = source.read(&object).map_err(|e| e.to_string())?;
                (
                    tk_sprite(&source, &resources, &other, frame, &mut textures)?.0,
                    format!("resources.assets:{collection_id} sprite {frame}"),
                )
            }
            Origin::Sprite(id) => {
                let object = source.object(&resources, id).map_err(|e| e.to_string())?;
                (
                    hk_unity::texture::sprite_image(&source, &object)
                        .map_err(|e| e.to_string())?
                        .0,
                    format!("resources.assets Sprite {id}"),
                )
            }
            Origin::Orb => (orb_image(width), "drawn".to_string()),
        };
        let shrunk = if full.width == width && full.height == height {
            full.clone()
        } else {
            full.resize(width, height, Filter::Lanczos)
        };
        let grey = indices(&shrunk);
        if TITLE_PIECES.contains(&name) {
            title.push((name, width, height, whole_rows(&grey, width, height)));
        }
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
            "source": source_name,
            "source_size": [full.width, full.height],
            "size": [width, height],
            "tiles": tiles.len() - first,
        }));
    }
    // The menu pointer: the final frame of the title menu's `main_menu_pointer_anim`
    // (sharedassets0.assets Sprite 101), as it points left of the chosen row and mirrored for the right.
    {
        let shared = source.file("sharedassets0.assets").map_err(|e| e.to_string())?;
        let object = source.object(&shared, POINTER_SPRITE).map_err(|e| e.to_string())?;
        let (full, _) = hk_unity::texture::sprite_image(&source, &object).map_err(|e| e.to_string())?;
        let small = full.resize(POINTER_W, POINTER_H, Filter::Lanczos);
        // The opening frames, left leaf then mirrored: the strip's slots 2 to 6 and 7 to 11 (menu.rs).
        let mut steps: Vec<Vec<u8>> = Vec::new();
        let mut steps_mirrored: Vec<Vec<u8>> = Vec::new();
        for id in POINTER_STEPS {
            let object = source.object(&shared, id).map_err(|e| e.to_string())?;
            let (full, _) = hk_unity::texture::sprite_image(&source, &object).map_err(|e| e.to_string())?;
            let small = full.resize(POINTER_W, POINTER_H, Filter::Lanczos);
            steps.push(whole_rows(&indices(&small), POINTER_W, POINTER_H));
            steps_mirrored.push(whole_rows(&indices(&small.flip_left_right()), POINTER_W, POINTER_H));
        }
        pointer_steps = (steps, steps_mirrored);
        for (name, image) in [("pointer", small.clone()), ("pointer_mirrored", small.flip_left_right())] {
            let grey = indices(&image);
            let mut shown = Image::new(Mode::Rgba, POINTER_W, POINTER_H);
            for (i, &v) in grey.iter().enumerate() {
                let g = (u32::from(v) * 255 / 15) as u8;
                shown.data[i * 4..i * 4 + 4].copy_from_slice(&[g, g, g, 255]);
            }
            save_rgba(&preview_dir.join(format!("{name}.png")), &shown)?;
            title.push((name, POINTER_W, POINTER_H, whole_rows(&grey, POINTER_W, POINTER_H)));
        }
    }
    {
        let mut ornaments: Vec<u8> = Vec::new();
        let mut rust = String::from("// Generated by hk-cook dialogue-art (host/hk-cook/src/dialogue_art.rs).\n");
        rust += "/// offset into the title ornaments, width and height in pixels; rows padded to halfwords.\n";
        for wanted in TITLE_ORDER {
            let (_, w, h, rows) = title.iter().find(|t| t.0 == wanted).ok_or("title piece missing")?;
            rust += &format!(
                "pub const TITLE_{}: (usize, u8, u8) = ({}, {w}, {h});\n",
                wanted.to_uppercase(),
                ornaments.len()
            );
            ornaments.extend_from_slice(rows);
        }
        // The pointer's opening frames follow the pieces: left leaf, then the mirrored leaf (the
        // leaves are the strip's slots 2 to 6 and 7 to 11 of POINTER_W texels each).
        rust += "/// Offsets of the pointer's twelve slots, each the size of TITLE_POINTER (see menu.rs `leaf`).\n";
        let mut steps_at: (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        for (set, at) in [(&pointer_steps.0, &mut steps_at.0), (&pointer_steps.1, &mut steps_at.1)] {
            for rows in set {
                at.push(ornaments.len());
                ornaments.extend_from_slice(rows);
            }
        }
        // The pointer's twelve slots in the order the strip holds them (`menu.rs`): final left leaf
        // (the cooked sprite, pointing left), final right leaf (its mirror), the left leaf's five
        // opening frames, then the right leaf's.
        let pointer_at = |name: &str| rust.lines().find(|l| l.contains(&format!("TITLE_{}:", name.to_uppercase()))).and_then(|l| l.split('(').nth(2)).and_then(|t| t.split(',').next()).and_then(|n| n.trim().parse::<usize>().ok());
        let mut slots = vec![pointer_at("pointer").ok_or("pointer slot")?, pointer_at("pointer_mirrored").ok_or("mirrored slot")?];
        slots.extend(steps_at.0.iter());
        slots.extend(steps_at.1.iter());
        rust += &format!("pub const TITLE_POINTER_SLOTS: [usize; {}] = {slots:?};\n", slots.len());
        rust += &format!("pub const TITLE_ORNAMENT_BYTES: usize = {};\n", ornaments.len());
        std::fs::write(root.join("data/menu-ornaments.hk"), &ornaments).map_err(|e| e.to_string())?;
        std::fs::write(root.join("data/menu_ornaments.rs"), rust).map_err(|e| e.to_string())?;
    }
    // The Geo icon, cut to one font cell. read_points.py pastes it into the
    // glyph strip's first unused cell (95), where the guest draws it as a glyph:
    // no VRAM of its own, drawn 1:1 beside the Geo count.
    {
        let collection = source
            .read(&source.object(&resources, UI_COLLECTION).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let (full, _) = tk_sprite(&source, &resources, &collection, COIN_SPRITE, &mut textures)?;
        // An area average: Lanczos rings on the stones' dark gaps and speckled the 12 px coin.
        let coin = full.resize(COIN_PX, COIN_PX, Filter::Box);
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
        // The font image: Perpetua at 14 px grid fitted, TrajanPro-Bold capitals and the coin
        // (font_strip.rs). It sits at (320, 360) in the texture page of the CLUT bank no scene uses.
        let body = font_data(&source, &resources, "Perpetua")?;
        let shared = source.file("sharedassets0.assets").map_err(|e| e.to_string())?;
        let caps = font_data(&source, &shared, "TrajanPro-Bold")?;
        let strip = crate::font_strip::cook(&body.1, &caps.1, &cell)?;
        std::fs::write(root.join("data/read_font.hk"), &strip.blob).map_err(|e| e.to_string())?;
        let list = |v: &[u8]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
        std::fs::write(
            root.join("data/font-metrics.json"),
            format!(
                "{{\n  \"font\": \"Perpetua\",\n  \"source\": \"{}\",\n  \"font_sha256\": \"{}\",\n  \"caps_font\": \"TrajanPro-Bold\",\n  \"caps_source\": \"{}\",\n  \"caps_font_sha256\": \"{}\",\n  \"em_px\": {},\n  \"advances\": [{}],\n  \"caps_chars\": \"{}\",\n  \"caps_advances\": [{}]\n}}\n",
                body.0,
                hk_sha256(&body.1),
                caps.0,
                hk_sha256(&caps.1),
                crate::font_strip::EM,
                list(&strip.advances),
                crate::font_strip::CAPS,
                list(&strip.caps_advances)
            ),
        )
        .map_err(|e| e.to_string())?;
        let fs = crate::font_strip::CAPS.len();
        let per_row = crate::font_strip::caps_per_row(strip.caps_w);
        let cards = crate::font_strip::cook_cards(&body.1)?;
        std::fs::write(root.join("data/card-font.hk"), &cards.blob).map_err(|e| e.to_string())?;
        let mut layout = format!(
                "// Generated by hk-cook dialogue-art (host/hk-cook/src/font_strip.rs).\n\
                 /// The font image: palette, then {rows} rows of 256 4bpp texels, uploaded at ({x}, {y}).\n\
                 pub const FONT_BYTES: usize = {bytes};\n\
                 pub const FONT_X: u16 = 320;\n\
                 pub const FONT_Y: u16 = 360;\n\
                 /// The texture page it is sampled from, and the texel row of its first row in that page.\n\
                 pub const FONT_PAGE_Y: u16 = 256;\n\
                 pub const FONT_V0: usize = 104;\n\
                 pub const FONT_ROWS: u16 = {rows};\n\
                 /// Perpetua's cells: size, cells to a row, rows apart, and where cells start in the row.\n\
                 pub const GLYPH_W: usize = {gw};\n\
                 pub const GLYPH_H: usize = {gh};\n\
                 pub const GLYPH_PER_ROW: usize = {gp};\n\
                 pub const GLYPH_ROW_PITCH: usize = {gr};\n\
                 pub const GLYPH_U0: usize = {u0};\n\
                 /// The capitals: cell size, cells to a row, the first row's texel row, the glyphs and their advances.\n\
                 pub const CAPS_W: usize = {cw};\n\
                 pub const CAPS_H: usize = {ch};\n\
                 pub const CAPS_PER_ROW: usize = {cp};\n\
                 pub const CAPS_V0: usize = {cv};\n\
                 pub const CAPS_CHARS: &[u8] = b\"{chars}\";\n\
                 pub const CAPS_ADVANCES: [u8; {fs}] = [{adv}];\n\
                 /// The Geo coin's cell, counted after the capitals (12x12 in a capital cell).\n\
                 pub const COIN_CELL: usize = {fs};\n",
                rows = crate::font_strip::IMAGE_ROWS,
                x = 320,
                y = 360,
                bytes = strip.blob.len(),
                gw = crate::font_strip::CELL_W,
                gh = crate::font_strip::CELL_H,
                gp = crate::font_strip::PER_ROW,
                gr = crate::font_strip::ROW_PITCH,
                u0 = crate::font_strip::GLYPH_U0,
                cw = strip.caps_w,
                ch = crate::font_strip::CAPS_H,
                cp = per_row,
                cv = crate::font_strip::CAPS_V0,
                chars = crate::font_strip::CAPS,
                fs = fs,
                adv = list(&strip.caps_advances),
            );
        layout += &crate::font_strip::cards_rust(&cards);
        std::fs::write(root.join("data/font_layout.rs"), layout).map_err(|e| e.to_string())?;
        let preview = crate::font_strip::cards_preview(&cards);
        let mut shown = Image::new(Mode::Rgba, 1280, crate::font_strip::CARD_ROWS);
        for (i, g) in preview.iter().enumerate() {
            shown.data[i * 4..i * 4 + 4].copy_from_slice(&[*g, *g, *g, 255]);
        }
        save_rgba(&preview_dir.join("card-font.png"), &shown)?;
        println!(
            "Card font: {} glyphs, {} bands, {} of {} texels used",
            cards.glyphs.len(),
            cards.glyphs.iter().map(|g| g.bands.len()).sum::<usize>(),
            cards.used,
            crate::font_strip::CARD_PAGES * crate::font_strip::CARD_PAGE_W * crate::font_strip::CARD_ROWS
        );
        let mut shown = Image::new(Mode::Rgba, 256, crate::font_strip::IMAGE_ROWS);
        for (i, g) in strip.preview.iter().enumerate() {
            shown.data[i * 4..i * 4 + 4].copy_from_slice(&[*g, *g, *g, 255]);
        }
        save_rgba(&preview_dir.join("font.png"), &shown)?;
        println!("Font image: {} bytes", strip.blob.len());
    }
    if blob.len() > 16384 {
        return err(format!(
            "dialogue art is {} bytes, the resident budget is 16384",
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
