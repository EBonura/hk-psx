//! A boss's own art bank, as host/false_knight_art.py's decomposition cooks it
//! into the boss's scene (the False Knight's, and host/mawlek_art.py's).
//!
//! Every frame of every clip is a run of parts: trimmed bands that live in the
//! scene's own texture pages, or trimmed cells of at most one animation slot
//! that stream from the scene arena. A part is an ordinary frame record (a
//! texture and its share of the world box), appended to every view of the scene
//! in the same order, and the bank's anchor clip's first frame is part 0 in
//! whichever view is live. The tables say which parts make up which frame of
//! which clip; nothing here knows about any fight.
use hk_format::Room;

pub struct Bank {
    pub anchor: u16,
    /// Per sprite: first part (frames after the anchor's first), parts, streamed.
    pub sprites: &'static [(u16, u8, bool)],
    /// Per art clip: first sequence entry, frames, fps (Q16), wrap, loop start.
    pub clips: &'static [(u16, u8, u32, u8, u8)],
    pub sequence: &'static [u16],
}
impl Bank {
    /// The sprite `clip` shows `tick` 60 Hz ticks after it started, with the
    /// guest's usual loop / loop-section / once rules. None for a clip cooked
    /// with no frames (the Mawlek's `Dummy Blank`), which draws nothing.
    pub fn frame(&self, clip: usize, tick: u32) -> Option<usize> {
        let (first, count, fps, wrap, loop_start) = self.clips[clip];
        if count == 0 {
            return None;
        }
        let count = count as u32;
        let start = loop_start as u32;
        let elapsed = (tick as u64 * fps as u64 / (60 * 65536)) as u32;
        let frame = if wrap == 0 {
            elapsed % count
        } else if wrap == 1 && elapsed >= count {
            start + (elapsed - start) % (count - start)
        } else {
            elapsed.min(count - 1)
        };
        Some(self.sequence[first as usize + frame as usize] as usize)
    }
    pub fn sprite(&self, clip: usize, tick: u32) -> usize {
        self.frame(clip, tick).expect("a clip with frames")
    }
    /// The sprite a clip ends on, for art that holds its last pose.
    pub fn last_sprite(&self, clip: usize) -> usize {
        let (first, count, ..) = self.clips[clip];
        self.sequence[first as usize + count as usize - 1] as usize
    }
    pub fn parts(&self, sprite: usize) -> usize {
        self.sprites[sprite].1 as usize
    }
    /// One part of a sprite: its scene texture and world box, relative to the
    /// object the sprite hangs off, in the frame record's `[x0, y0, x1, y1]` order.
    pub fn part(&self, room: &Room, sprite: usize, index: usize) -> (u16, [i32; 4]) {
        let base = room.clip(self.anchor as usize)[0] as usize;
        let f = room.frame(base + self.sprites[sprite].0 as usize + index);
        let word = |at: usize| u32::from_le_bytes([f[at], f[at + 1], f[at + 2], f[at + 3]]);
        (
            word(0) as u16,
            core::array::from_fn(|k| word(4 + k * 4) as i32),
        )
    }
}
