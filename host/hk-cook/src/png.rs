//! Debug sheet PNGs (Pillow's `Image.save(path)` for RGBA). Pixels match
//! Pillow's; the compressed bytes are this encoder's, not zlib's at level 6.

use hk_pil::{Image, Mode};
use std::path::Path;

pub fn save_rgba(path: &Path, im: &Image) -> Result<(), String> {
    assert_eq!(im.mode, Mode::Rgba);
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(file),
        im.width as u32,
        im.height as u32,
    );
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&im.data).map_err(|e| e.to_string())
}
