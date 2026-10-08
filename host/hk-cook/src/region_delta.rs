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
    Ok(Layout { counts, prefix, pages: page_start, stream })
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
        raw.resize(40 + textures as usize * 48 + draws as usize * 44 + pages as usize * 32768 + stream as usize, 0);
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
