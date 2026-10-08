//! Trigger overlap for the script runtime, apart from the cooked bank.
//!
//! This is the half of `script.rs` that decides *when* a `HeroTrigger` op is
//! true, and it is separate so it can be tested without a bank. The sequencing
//! is the whole point: the overlap is sampled once per tick, for every volume,
//! before any instance runs. Unity raises OnTriggerEnter2D from the physics
//! step rather than from the state that listens, so a state entered with the
//! Knight already inside must see Stay and never Enter, and a state re-entered
//! while he stands still must not see Enter again.
use hk_sim::script::{TRIGGER_ENTER, TRIGGER_EXIT, TRIGGER_STAY};

pub fn overlaps(bounds: [i32; 4], body: [i32; 4]) -> bool {
    bounds[0] <= body[2] && bounds[2] >= body[0] && bounds[1] <= body[3] && bounds[3] >= body[1]
}

/// This tick's and last tick's overlap, one bit each per cooked volume.
pub struct Overlaps<const N: usize> {
    now: [bool; N],
    was: [bool; N],
}
impl<const N: usize> Default for Overlaps<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> Overlaps<N> {
    pub const fn new() -> Self {
        Self { now: [false; N], was: [false; N] }
    }
    /// Forget both ticks. A scene load does this, so the volumes it creates
    /// around the arriving Knight read as an entry rather than as a stay.
    pub fn clear(&mut self) {
        self.now = [false; N];
        self.was = [false; N];
    }
    /// Sample every volume, once, at the top of a tick.
    pub fn refresh(&mut self, inside: impl Fn(usize) -> bool) {
        for index in 0..N {
            self.was[index] = self.now[index];
            self.now[index] = inside(index);
        }
    }
    /// Whether volume `index` is in `phase` this tick. An index or a phase the
    /// bank does not have is false rather than a guess; the executor rejects an
    /// unknown phase before it gets here.
    pub fn holds(&self, index: usize, phase: u8) -> bool {
        let (Some(&now), Some(&was)) = (self.now.get(index), self.was.get(index)) else {
            return false;
        };
        match phase {
            TRIGGER_ENTER => now && !was,
            TRIGGER_STAY => now,
            TRIGGER_EXIT => was && !now,
            _ => false,
        }
    }
}
