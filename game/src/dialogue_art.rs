//! The original dialogue box's ornaments (host/hk-cook/src/dialogue_art.rs).
//!
//! A tablet or a conversation is not a bare rectangle in the source: a fleur
//! above the text, a fleur below it, and under the last line an arrow while
//! there is more to read or a stop once there is not. The cooker cuts each to
//! its final pixel size, so the guest draws them 1:1 as textured rectangles.
//!
//! The pieces stay in linked RAM and reach VRAM through the animation cache's
//! 64x64 slots, exactly as the charm board's icons do: a piece wider than a
//! slot is several tiles, each its own cache key, and a frame with no panel up
//! asks the cache for nothing.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/dialogue_art.rs"));
#[cfg(not(test))]
pub use presentation::{append_needed, prepared, texels, KEY_BASE};

/// Pixels between the arrow or stop and the bottom fleur, and the width of the
/// panel's backboard on either side of the fleur: layout shared with the
/// panel that places the pieces.
pub const ICON_W: i16 = 16;
pub const ICON_H: i16 = 17;

#[cfg(not(test))]
mod presentation {
    use super::*;
    static ART: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../data/dialogue-art.hk"
    ));
    /// Animation-cache keys above the props'.
    pub const KEY_BASE: u16 = crate::props::KEY_BASE + crate::props::PARTS.len() as u16;
    const _: () = assert!(KEY_BASE as usize + DIALOGUE_TILE_COUNT <= hk_cache::MAX_KEYS);
    const _: () = assert!(ART.len() == DIALOGUE_ART_BYTES);
    // A tile is one slot upload, and a slot is 64x64 4bpp.
    const _: () = assert!(DIALOGUE_ART_BYTES <= 4096);
    /// Whether this frame's working set carries the panel's tiles. `panel`
    /// draws ornaments only for a frame that asked for them, so a panel that
    /// opened after the set was fixed draws plain for one frame instead of
    /// asking the cache for a slot it was never given.
    static mut PREPARED: bool = false;
    /// Frames the panel wanted its ornaments and the view behind it had
    /// already claimed every request the cache takes.
    #[no_mangle]
    pub static mut HK_DIALOGUE_ART_CROWDED: u32 = 0;

    /// The panel's tiles for the frame's working set; nothing when no panel is up.
    #[inline(never)]
    pub fn append_needed(open: bool, needed: &mut [u16], len: &mut usize) {
        unsafe { PREPARED = false };
        if !open {
            return;
        }
        if *len + DIALOGUE_TILE_COUNT > needed.len() {
            unsafe { HK_DIALOGUE_ART_CROWDED = HK_DIALOGUE_ART_CROWDED.saturating_add(1) };
            return;
        }
        for i in 0..DIALOGUE_TILE_COUNT {
            needed[*len] = KEY_BASE + i as u16;
            *len += 1;
        }
        unsafe { PREPARED = true };
    }
    pub fn prepared() -> bool {
        unsafe { PREPARED }
    }
    /// Texels of one tile, for the animation cache's upload closure.
    pub fn texels(index: usize) -> Option<(&'static [u8], u16, u16)> {
        let &(offset, w, h, _) = DIALOGUE_TILES.get(index)?;
        let len = (usize::from(w) + 3) / 4 * 2 * usize::from(h);
        let start = usize::from(offset);
        ART.get(start..start + len).map(|t| (t, u16::from(w), u16::from(h)))
    }
}
