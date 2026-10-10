//! DMA-owned flat textured quads with optional scoped drawing areas.
//! E3/E4 bracket the original quad, preserving its UV interpolation and order.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Packet {
    pub tag: u32,
    pub words: [u32; 13],
}
impl Packet {
    pub const ZERO: Self = Self {
        tag: 0,
        words: [0; 13],
    };
    pub fn plain(quad: [u32; 9]) -> Self {
        let mut p = Self::ZERO;
        p.write_plain(quad);
        p
    }
    pub fn write_plain(&mut self, quad: [u32; 9]) {
        self.tag = 9 << 24;
        self.words[..9].copy_from_slice(&quad);
    }
    /// Same polygon coverage as FT4, with no texture fetch for proven solid fill.
    pub fn write_black(&mut self, vertices: [u32; 4]) {
        self.tag = 5 << 24;
        self.words[0] = 0x2800_0000;
        self.words[1..5].copy_from_slice(&vertices);
    }
    pub fn scissored(quad: [u32; 9], rect: [i16; 4], framebuffer_y: u16) -> Self {
        let mut p = Self::ZERO;
        p.write_scissored(quad, rect, framebuffer_y);
        p
    }
    pub fn write_scissored(&mut self, quad: [u32; 9], rect: [i16; 4], framebuffer_y: u16) {
        let [left, top, right, bottom] = rect;
        assert!(
            left >= 0 && top >= 0 && left < right && top < bottom && right <= 320 && bottom <= 240
        );
        assert!(framebuffer_y == 0 || framebuffer_y == 240);
        self.tag = 13 << 24;
        self.words[0] = 0xe300_0000 | left as u32 | ((top as u32 + framebuffer_y as u32) << 10);
        // Drawing-area lower/right limits are inclusive; helper rectangles are not.
        self.words[1] =
            0xe400_0000 | (right - 1) as u32 | ((bottom as u32 - 1 + framebuffer_y as u32) << 10);
        self.words[2..11].copy_from_slice(&quad);
        self.words[11] = 0xe300_0000 | ((framebuffer_y as u32) << 10);
        self.words[12] = 0xe400_0000 | 319 | ((framebuffer_y as u32 + 239) << 10);
    }
    pub fn word_count(&self) -> u8 {
        (self.tag >> 24) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_quad_words_survive_both_scissor_origins() {
        let quad = [0x2e80_8080, 1, 2, 3, 4, 5, 6, 7, 8];
        for y in [0, 240] {
            let p = Packet::scissored(quad, [3, 7, 103, 207], y);
            assert_eq!(p.word_count(), 13);
            assert_eq!(&p.words[2..11], &quad);
            assert_eq!(p.words[0], 0xe300_0000 | 3 | ((y as u32 + 7) << 10));
            assert_eq!(p.words[1], 0xe400_0000 | 102 | ((y as u32 + 206) << 10));
            assert_eq!(p.words[11], 0xe300_0000 | ((y as u32) << 10));
            assert_eq!(p.words[12], 0xe400_0000 | 319 | ((y as u32 + 239) << 10));
        }
    }
    #[test]
    fn plain_length_excludes_unused_scissor_storage() {
        let quad = [0x2e80_8080, 1, 2, 3, 4, 5, 6, 7, 8];
        let p = Packet::plain(quad);
        assert_eq!(p.word_count(), 9);
        assert_eq!(&p.words[..9], &quad);
        assert_eq!(core::mem::size_of::<Packet>(), 56);
        assert_eq!(core::mem::offset_of!(Packet, words), 4);
    }
}
