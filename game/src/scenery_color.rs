//! Exact color-command preparation for a scenery draw's gain and material.
//! UVs, CLUTs, texture pages and geometry remain in the existing packet path.

/// Preserve the command byte while applying the renderer's original integer
/// gain, black-sentinel modulation and faded-mask color in the same order.
/// Live setters clamp gain/opacity to128; the arithmetic also matches the old
/// renderer outside that range, including the post-division u8 truncation.
#[inline]
pub fn command(template_command: u32, tint: (u8, u8, u8), gain: u8, opacity: u16) -> u32 {
    let red = (tint.0 as u16 * gain as u16 / 128) as u8;
    let green = (tint.1 as u16 * gain as u16 / 128) as u8;
    let blue = (tint.2 as u16 * gain as u16 / 128) as u8;
    let mut color = (template_command & 0xff00_0000)
        | red as u32
        | ((green as u32) << 8)
        | ((blue as u32) << 16);
    if opacity & 256 != 0 {
        color = (color & !255) | 127;
    }
    let alpha = (opacity & 255) as u32;
    if alpha < 128 {
        color = (color & 0xff00_0000) | alpha | (alpha << 8) | (alpha << 16);
    }
    color
}

/// Apply cached RGB while retaining live template command/UV words. In
/// particular, streamed template updates cannot leave a stale GPU command.
/// The non-color CLUT and texture-page branches match quad_words unchanged.
#[inline]
pub fn with_material(mut words: [u32; 9], color: u32, opacity: u16, fade_clut: u32) -> [u32; 9] {
    words[0] = (words[0] & 0xff00_0000) | color;
    if opacity & 256 != 0 {
        words[4] &= !(3 << 21);
    }
    if opacity & 255 < 128 {
        words[2] = (words[2] & 0xffff) | (fade_clut << 16);
        words[4] = (words[4] & !(3 << 21)) | (2 << 21);
    }
    words
}

#[cfg(test)]
#[path = "../../tests/scenery_color_runtime.rs"]
mod tests;
