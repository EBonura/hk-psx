//! Source material admission and the 4bpp alpha-coverage quantizer (host/materials.py).

use hk_pil::{quant, Image, Mode};

/// Ordered coverage is spatial, never a random or frame-dependent fade.
const BAYER4: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Source alpha times lifetime alpha, quantized spatially to 0, 1/2, 1.
pub fn alpha_coverage(alpha: u32, opacity128: u32, x: usize, y: usize) -> u32 {
    assert!(alpha <= 255 && opacity128 <= 128, "alpha range");
    let numerator = alpha * opacity128 * 2;
    let denominator = 255 * 128;
    let (low, remainder) = (numerator / denominator, numerator % denominator);
    (low + (remainder * 32 > denominator * (2 * BAYER4[y & 3][x & 3] + 1)) as u32).min(2)
}

/// The quantizer's result: width, height, 32-byte palette, packed 4bpp texels.
pub struct Quantized {
    pub width: usize,
    pub height: usize,
    pub palette: [u8; 32],
    pub packed: Vec<u8>,
}

/// 4bpp source-over approximation for explicitly admitted coloured sprites:
/// three opacity classes share 15 colours, each class median-cut separately,
/// the GPU's Average blend supplying the half-coverage premultiplication.
pub fn quantize_alpha_coverage(image: &Image, opacity128: u32) -> Result<Quantized, String> {
    if opacity128 > 128 {
        return Err("alpha range".into());
    }
    let image = image.to_rgba();
    let (w, h) = (image.width, image.height);
    if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
        return Err("4bpp texture dimensions".into());
    }
    let px = |i: usize| -> [u8; 4] { image.data[i * 4..i * 4 + 4].try_into().unwrap() };
    let levels: Vec<u32> = (0..w * h).map(|i| alpha_coverage(px(i)[3] as u32, opacity128, i % w, i / w)).collect();
    let groups: [Vec<usize>; 2] = [1u32, 2].map(|level| (0..w * h).filter(|&i| levels[i] == level).collect());
    let budgets = if !groups[0].is_empty() && !groups[1].is_empty() {
        let (n1, n2) = (groups[0].len() as f64, groups[1].len() as f64);
        let half = ((15.0 * n1 / (n1 + n2)).round_ties_even() as i64).clamp(1, 14) as u32;
        [half, 15 - half]
    } else {
        [15, 15]
    };
    let mut palette: Vec<u16> = vec![0];
    let mut indices = vec![0u8; w * h];
    for (g, positions) in groups.iter().enumerate() {
        if positions.is_empty() {
            continue;
        }
        let strip: Vec<[u8; 3]> = positions.iter().map(|&i| [px(i)[0], px(i)[1], px(i)[2]]).collect();
        let (colors, q) = quant::median_cut(&strip, budgets[g]).ok_or("median cut failed")?;
        let mut used: Vec<u8> = q.clone();
        used.sort_unstable();
        used.dedup();
        let mut mapping = [0u8; 256];
        for original in used {
            let [r, gg, b] = colors[original as usize];
            let word = (r as u16 >> 3) | ((gg as u16 >> 3) << 5) | ((b as u16 >> 3) << 10);
            mapping[original as usize] = palette.len() as u8;
            palette.push(if g == 0 { word | 0x8000 } else if word == 0 { 1 } else { word });
        }
        for (&position, &index) in positions.iter().zip(&q) {
            indices[position] = mapping[index as usize];
        }
    }
    if palette.len() > 16 {
        return Err("4bpp palette budget".into());
    }
    let stride = w.div_ceil(2);
    let mut packed = vec![0u8; stride * h];
    for (i, &index) in indices.iter().enumerate() {
        packed[(i / w) * stride + (i % w) / 2] |= index << (((i % w) & 1) * 4);
    }
    let mut bytes = [0u8; 32];
    for (i, word) in palette.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&word.to_le_bytes());
    }
    Ok(Quantized { width: w, height: h, palette: bytes, packed })
}

/// Pixels as `Image.new("RGB", (n, 1)).putdata(...)` would hold them.
pub fn rgb_strip(pixels: &[[u8; 3]]) -> Image {
    let mut im = Image::new(Mode::Rgb, pixels.len(), 1);
    for (i, p) in pixels.iter().enumerate() {
        im.data[i * 4..i * 4 + 4].copy_from_slice(&[p[0], p[1], p[2], 255]);
    }
    im
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_levels() {
        // Opaque at full opacity is level 2, transparent is 0, half alpha dithers.
        assert_eq!(alpha_coverage(255, 128, 0, 0), 2);
        assert_eq!(alpha_coverage(0, 128, 3, 1), 0);
        let half: Vec<u32> = (0..16).map(|i| alpha_coverage(128, 128, i % 4, i / 4)).collect();
        assert!(half.contains(&1) && half.iter().all(|&l| l <= 2));
    }

    #[test]
    fn solid_sprite_quantizes_to_one_opaque_colour() {
        let mut im = Image::new(Mode::Rgba, 3, 2);
        for px in im.data.chunks_exact_mut(4) {
            px.copy_from_slice(&[255, 0, 0, 255]);
        }
        let q = quantize_alpha_coverage(&im, 128).unwrap();
        assert_eq!(&q.palette[..4], &[0, 0, 31, 0]); // index 0 transparent, index 1 red (0x001f)
        assert_eq!(q.packed, vec![0x11, 0x01, 0x11, 0x01]);
    }
}
