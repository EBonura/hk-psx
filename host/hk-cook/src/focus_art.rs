//! Focus Soul Burst and heal flash from the selected retail source.
//! The five Soul Burst sprites belong to the Knight's Soul_burst000 clip.
//! White Flash R uses the white_light sprite and fades over one second in the guest.
use crate::common::{err, py_round, Result};
use crate::cook::{focal, native_sprite, CAM_Z};
use crate::png::save_rgba;
use hk_pil::resample::Filter;
use hk_pil::{Image, Mode};
use hk_unity::Source;
use serde_json::json;
use std::path::Path;

const SOUL_IDS: [i64; 5] = [3114, 3559, 3434, 1947, 2426];
const FLASH_ID: i64 = 1987;
const SOUL_SCALE: f64 = 1.67;
const SOUL_Y: f64 = -0.48;
const SOUL_SHRINK: f64 = 2.2;
// The flash's scale, vertical offset and gain are fits against a retail capture,
// not recovered authored values. The full-bright disc lifted dark pixels by
// about 60 grey levels; the fit's mean error was 1.8 grey levels.
const FLASH_SCALE: f64 = 11.0;
const FLASH_BELOW: f64 = 1.0;
const FLASH_ALPHA: f64 = 0.5176470875740051;
const FLASH_GAIN: f64 = 1.17;

fn flash_quantized(image: &Image) -> Result<([u8; 32], Vec<u8>)> {
    if image.mode != Mode::Rgba || image.width != 256 || image.height != 256 {
        return err("flash quantizer requires one RGBA 256x256 sheet");
    }
    let values: Vec<f64> = image
        .data
        .chunks_exact(4)
        .map(|p| {
            let rgb = (u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) as f64;
            (((rgb / 3.0) * f64::from(p[3]) / 255.0) * FLASH_ALPHA) * FLASH_GAIN
        })
        .collect();
    let peak = values.iter().copied().fold(0.0_f64, f64::max);
    if peak <= 0.0 {
        return err("flash sprite is empty");
    }
    let mut packed = vec![0_u8; 256 * 256 / 2];
    for (i, &value) in values.iter().enumerate() {
        let index = py_round(value / peak * 15.0) as u8;
        if index > 15 {
            return err("flash palette index exceeds 4bpp");
        }
        packed[i / 2] |= index << ((i & 1) * 4);
    }
    let mut palette = [0_u8; 32];
    for i in 1..16 {
        let grey = ((py_round(peak * i as f64 / 15.0) + 4) >> 3) as u16;
        let word = grey * 0x0421 | 0x8000;
        palette[i * 2..i * 2 + 2].copy_from_slice(&word.to_le_bytes());
    }
    Ok((palette, packed))
}

pub fn main(root: &Path, _args: Option<&[String]>) -> Result<()> {
    let source = Source::from_doctor(root).map_err(|e| e.to_string())?;
    let resources = source.file("resources.assets").map_err(|e| e.to_string())?;
    let scale = focal() / -CAM_Z;
    let out = root.join(".hkpsx/ability-focus-rust");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut frames = Vec::new();
    for (index, id) in SOUL_IDS.into_iter().enumerate() {
        let object = source.object(&resources, id).map_err(|e| e.to_string())?;
        let (image, box_) = native_sprite(&source, &object)?;
        let w = (((box_[2] - box_[0]) * SOUL_SCALE * scale / SOUL_SHRINK).ceil() as usize).max(1);
        let h = (((box_[3] - box_[1]) * SOUL_SCALE * scale / SOUL_SHRINK).ceil() as usize).max(1);
        let image = image.resize(w, h, Filter::Lanczos);
        let name = format!("soul-{index}.png");
        save_rgba(&out.join(&name), &image)?;
        frames.push(json!({"name":name,"source":format!("resources.assets:{id}"),
            "box":[SOUL_SCALE*box_[0],SOUL_Y+SOUL_SCALE*box_[1],SOUL_SCALE*box_[2],SOUL_Y+SOUL_SCALE*box_[3]]}));
    }
    let object = source
        .object(&resources, FLASH_ID)
        .map_err(|e| e.to_string())?;
    let (image, box_) = native_sprite(&source, &object)?;
    let reach = [320.0 / scale, 240.0 / scale];
    let half = [
        reach[0] / FLASH_SCALE * image.width as f64 / (box_[2] - box_[0]),
        reach[1] / FLASH_SCALE * image.height as f64 / (box_[3] - box_[1]),
    ];
    let cx = image.width as f64 / 2.0;
    let cy = image.height as f64 / 2.0
        - FLASH_BELOW / FLASH_SCALE * image.height as f64 / (box_[3] - box_[1]);
    let crop = image.to_rgba().crop([
        py_round(cx - half[0]) as f64,
        py_round(cy - half[1]) as f64,
        py_round(cx + half[0]) as f64,
        py_round(cy + half[1]) as f64,
    ]);
    let flash = crop.resize(64, 48, Filter::Box);
    save_rgba(&out.join("flash.png"), &flash)?;
    frames.push(
        json!({"name":"flash.png","source":format!("resources.assets:{FLASH_ID}"),
        "box":[-reach[0],-reach[1],reach[0],reach[1]]}),
    );
    let mut sheet = Image::new(Mode::Rgba, 256, 256);
    sheet.paste(&flash, 0, 0, None);
    let (palette, packed) = flash_quantized(&sheet)?;
    std::fs::write(out.join("flash-pal.bin"), palette).map_err(|e| e.to_string())?;
    std::fs::write(out.join("flash-sheet.4bpp"), packed).map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("frames.json"),
        serde_json::to_vec(&frames).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
