//! host/region_delta.py in Rust: the HKROOM02 room layout every cooker and
//! census reads rooms through.

use crate::common::{err, Result};

pub const MAGIC: &[u8; 8] = b"HKROOM02";

/// A room's section sizes and where its parts begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// pages, textures, draws, frames, clips, edges
    pub counts: [u32; 6],
    /// The end of the prefix tables (texture palettes start here).
    pub prefix: usize,
    pub pages: usize,
    pub stream: usize,
}

fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]])
}

/// `layout`: validate the header and the total length, and locate the
/// palettes, the atlas pages and the texel stream.
pub fn layout(raw: &[u8]) -> Result<Layout> {
    if raw.len() < 40 || &raw[..8] != MAGIC {
        return err("Invalid room header");
    }
    if word(raw, 36) & !1 != 0 {
        return err("Unknown room features");
    }
    let counts: [u32; 6] = core::array::from_fn(|k| word(raw, 8 + 4 * k));
    let [pages, textures, draws, frames, clips, edges] = counts.map(|c| c as usize);
    let prefix = 40 + textures * 16 + draws * 44 + frames * 20 + clips * 16 + edges * 16;
    let page_start = prefix + textures * 32;
    let stream = page_start + pages * 32768;
    if stream + word(raw, 32) as usize != raw.len() {
        return err("Invalid room length");
    }
    Ok(Layout {
        counts,
        prefix,
        pages: page_start,
        stream,
    })
}

/// `read_row`: one row of compact texels, even when the image begins on an odd nibble.
fn read_row(raw: &[u8], at: usize, odd: bool, width: usize) -> Vec<u8> {
    let pairs = width / 2;
    if odd {
        let mut row: Vec<u8> = (0..pairs)
            .map(|i| (raw[at + i] >> 4) | ((raw[at + i + 1] & 15) << 4))
            .collect();
        if width & 1 != 0 {
            row.push(raw[at + pairs] >> 4);
        }
        row
    } else {
        let mut row = raw[at..at + pairs].to_vec();
        if width & 1 != 0 {
            row.push(raw[at + pairs] & 15);
        }
        row
    }
}

/// `textures(raw)`: every texture as `width, height, palette, texels`, extracted from the pages and the stream.
pub fn textures(raw: &[u8]) -> Result<Vec<Vec<u8>>> {
    let l = layout(raw)?;
    let mut result = Vec::new();
    for i in 0..l.counts[1] as usize {
        let at = 40 + i * 16;
        let h = |k: usize| u16::from_le_bytes([raw[at + 2 * k], raw[at + 2 * k + 1]]) as usize;
        let (page, u, v, width, height, palette) = (h(0), h(1), h(2), h(3), h(4), h(5));
        let offset = word(raw, at + 12) as usize;
        if width == 0 || height == 0 || palette >= l.counts[1] as usize {
            return err("Invalid texture");
        }
        let (stride, base, x) = if page == 65535 {
            (((width + 3) & !3) / 2, l.stream + offset, 0)
        } else {
            if page >= l.counts[0] as usize || u + width > 256 || v + height > 256 {
                return err("Invalid atlas texture");
            }
            (128, l.pages + page * 32768 + v * 128, u)
        };
        let mut blob = Vec::new();
        blob.extend_from_slice(&(width as u16).to_le_bytes());
        blob.extend_from_slice(&(height as u16).to_le_bytes());
        blob.extend_from_slice(&raw[l.prefix + palette * 32..l.prefix + (palette + 1) * 32]);
        for yy in 0..height {
            blob.extend(read_row(raw, base + yy * stride + x / 2, x & 1 != 0, width));
        }
        result.push(blob);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(pages: u32, textures: u32, draws: u32, stream: u32) -> Vec<u8> {
        let mut raw = vec![0u8; 40];
        raw[..8].copy_from_slice(MAGIC);
        for (k, v) in [pages, textures, draws, 0, 0, 0].into_iter().enumerate() {
            raw[8 + 4 * k..12 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        raw[32..36].copy_from_slice(&stream.to_le_bytes());
        raw.resize(
            40 + textures as usize * 48
                + draws as usize * 44
                + pages as usize * 32768
                + stream as usize,
            0,
        );
        raw
    }
    #[test]
    fn the_sections_follow_the_counts() {
        let l = layout(&room(1, 2, 3, 5)).unwrap();
        assert_eq!(l.counts, [1, 2, 3, 0, 0, 0]);
        assert_eq!(l.prefix, 40 + 2 * 16 + 3 * 44);
        assert_eq!(l.pages, l.prefix + 2 * 32);
        assert_eq!(l.stream, l.pages + 32768);
    }
    #[test]
    fn a_bad_header_feature_or_length_is_refused() {
        let mut raw = room(1, 1, 1, 0);
        raw.push(0);
        assert!(layout(&raw).is_err());
        let mut raw = room(1, 1, 1, 0);
        raw[36] = 2;
        assert!(layout(&raw).is_err());
        assert!(layout(&[0; 39]).is_err());
        let mut raw = room(1, 1, 1, 0);
        raw[0] = b'X';
        assert!(layout(&raw).is_err());
    }
}
