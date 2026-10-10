//! CPU-only metadata for a packet prefix prepared after the previous DMA.
//! The packet words remain in their original pool; no GPU command is issued.
pub const SNAPSHOT_CAPACITY: usize = 64;
pub struct Prefix {
    camera: (i32, i32),
    framebuffer_y: u16,
    count: u16,
    valid: bool,
    state: [u32; SNAPSHOT_CAPACITY],
    pub next: usize,
    pub packets: usize,
    pub extra: usize,
    pub emitted: u32,
    pub max_rank: u16,
    pub rows: [u32; 15],
}
impl Prefix {
    pub const fn new() -> Self {
        Self {
            camera: (0, 0),
            framebuffer_y: 0,
            count: 0,
            valid: false,
            state: [0; SNAPSHOT_CAPACITY],
            next: 0,
            packets: 0,
            extra: 0,
            emitted: 0,
            max_rank: 0,
            rows: [0; 15],
        }
    }
    #[inline]
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
    #[inline]
    pub fn matches(&self, camera: (i32, i32), framebuffer_y: u16) -> bool {
        self.valid && self.camera == camera && self.framebuffer_y == framebuffer_y
    }
    pub fn state(&self) -> &[u32] {
        &self.state[..self.count as usize]
    }
    pub fn state_mut(&mut self) -> &mut [u32; SNAPSHOT_CAPACITY] {
        self.valid = false;
        &mut self.state
    }
    pub fn save(
        &mut self,
        camera: (i32, i32),
        framebuffer_y: u16,
        count: usize,
        next: usize,
        packets: usize,
        extra: usize,
        emitted: u32,
        max_rank: u16,
        rows: &[u32; 15],
    ) {
        assert!(count <= SNAPSHOT_CAPACITY);
        self.camera = camera;
        self.framebuffer_y = framebuffer_y;
        self.count = count as u16;
        self.next = next;
        self.packets = packets;
        self.extra = extra;
        self.emitted = emitted;
        self.max_rank = max_rank;
        self.rows.copy_from_slice(rows);
        self.valid = true;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_key_full_and_partial_state_and_explicit_invalidation() {
        let mut p = Prefix::new();
        assert!(!p.matches((0, 0), 0));
        p.state_mut()[..3].copy_from_slice(&[3, 5, 7]);
        p.save((123, -456), 240, 3, 17, 31, 9, 31, 1042, &[0x12345; 15]);
        assert!(p.matches((123, -456), 240));
        assert_eq!(p.state(), &[3, 5, 7]);
        assert_eq!(
            (p.next, p.packets, p.extra, p.emitted, p.max_rank),
            (17, 31, 9, 31, 1042)
        );
        assert_eq!(p.rows, [0x12345; 15]);
        assert!(!p.matches((124, -456), 240));
        assert!(!p.matches((123, -455), 240));
        assert!(!p.matches((123, -456), 0));
        p.invalidate();
        assert!(!p.matches((123, -456), 240));
        p.save((0, 0), 0, 0, 480, 80, 4, 80, 0, &[0; 15]);
        assert!(p.matches((0, 0), 0));
        assert!(p.state().is_empty());
        let _ = p.state_mut();
        assert!(!p.matches((0, 0), 0));
    }
    #[test]
    fn each_complete_source_boundary_resumes_identical_packet_and_row_order() {
        for stop in 0..=64 {
            let counts: [usize; 64] = core::array::from_fn(|i| (i * 7) % 9);
            let full: Vec<_> = counts
                .iter()
                .enumerate()
                .flat_map(|(i, &n)| (0..n).map(move |k| (i, k)))
                .collect();
            let mut words = Vec::new();
            let mut rows = [u32::MAX; 15];
            for (i, &n) in counts[..stop].iter().enumerate() {
                rows[i % 15] &= !(1 << (i % 20));
                words.extend((0..n).map(|k| (i, k)));
            }
            let mut p = Prefix::new();
            p.save(
                (0, 0),
                240,
                0,
                stop,
                words.len(),
                0,
                words.len() as u32,
                1,
                &rows,
            );
            rows.fill(0);
            rows.copy_from_slice(&p.rows);
            for i in p.next..64 {
                rows[i % 15] &= !(1 << (i % 20));
                words.extend((0..counts[i]).map(|k| (i, k)));
            }
            assert_eq!(words, full);
            let mut expected = [u32::MAX; 15];
            for i in 0..64 {
                expected[i % 15] &= !(1 << (i % 20));
            }
            assert_eq!(rows, expected);
        }
    }
}
