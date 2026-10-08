//! Fixed-capacity FIFO of actual pad polls. This module never polls hardware,
//! invents samples, merges pulses, or silently overwrites an unread sample;
//! dropping one is an explicit call (`drop_oldest`) owned by the sampler.
pub const CAPACITY: usize = 16;
const HALF_RANGE: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub tick: u32,
    pub buttons: u16,
}
const EMPTY: Sample = Sample {
    tick: 0,
    buttons: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Full,
    DuplicateTick,
    OutOfOrder,
    AmbiguousWrap,
}

/// Serial-number order is defined only within a half-u32 timestamp window.
/// Exactly half a wrap is ambiguous; larger forward gaps appear out of order.
fn forward(previous: u32, tick: u32, allow_equal: bool) -> Result<(), Error> {
    match tick.wrapping_sub(previous) {
        0 if !allow_equal => Err(Error::DuplicateTick),
        HALF_RANGE => Err(Error::AmbiguousWrap),
        distance if distance > HALF_RANGE => Err(Error::OutOfOrder),
        _ => Ok(()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Queue {
    samples: [Sample; CAPACITY],
    head: u8,
    count: u8,
    base_tick: u32,
    seen: Option<u32>,
    through_tick: u32,
    consumed_tick: u32,
    hold: u16,
}
impl Queue {
    pub const fn new() -> Self {
        Self {
            samples: [EMPTY; CAPACITY],
            head: 0,
            count: 0,
            base_tick: 0,
            seen: None,
            through_tick: 0,
            consumed_tick: 0,
            hold: 0,
        }
    }
    /// `base_tick` is the preceding boundary, not a hardware poll. The first
    /// pushed timestamp must follow it. Neutral hold is zero until consumption.
    pub fn reset(&mut self, base_tick: u32) {
        *self = Self {
            base_tick,
            through_tick: base_tick,
            consumed_tick: base_tick,
            ..Self::new()
        };
    }
    /// A rejected push leaves all state unchanged, so the caller can explicitly
    /// handle overflow or retry after draining. `seen` means last accepted poll.
    pub fn push(&mut self, tick: u32, buttons: u16) -> Result<(), Error> {
        forward(self.seen.unwrap_or(self.base_tick), tick, false)?;
        // Keep the entire unread history unambiguously after its consumed base.
        forward(self.consumed_tick, tick, false)?;
        if self.count as usize == CAPACITY {
            return Err(Error::Full);
        }
        let tail = (self.head as usize + self.count as usize) & (CAPACITY - 1);
        self.samples[tail] = Sample { tick, buttons };
        self.count += 1;
        self.seen = Some(tick);
        Ok(())
    }
    /// Return at most one actual sample due by `through_tick`, retaining its
    /// original timestamp. Drain repeatedly at the same horizon to preserve
    /// every edge. None means no recorded poll is due; consult the hold getter
    /// separately if simulation needs the last consumed state during a gap.
    /// A horizon may advance by less than half a wrap; it cannot move backward.
    pub fn take(&mut self, through_tick: u32) -> Result<Option<Sample>, Error> {
        forward(self.through_tick, through_tick, true)?;
        forward(self.consumed_tick, through_tick, true)?;
        let sample = if self.count == 0 {
            None
        } else {
            let front = self.samples[self.head as usize];
            match through_tick.wrapping_sub(front.tick) {
                HALF_RANGE => return Err(Error::AmbiguousWrap),
                distance if distance < HALF_RANGE => Some(front),
                _ => None,
            }
        };
        self.through_tick = through_tick;
        if let Some(sample) = sample {
            self.head = (self.head + 1) & (CAPACITY as u8 - 1);
            self.count -= 1;
            self.consumed_tick = sample.tick;
            self.hold = sample.buttons;
        }
        Ok(sample)
    }
    /// Remove the oldest unread sample without consuming it: the consumed
    /// tick and the hold stay where the consumer left them, so its tick later
    /// reads as a gap (the previous hold). The sampler's overflow path; the
    /// caller owns what happens to the returned buttons.
    pub fn drop_oldest(&mut self) -> Option<Sample> {
        if self.count == 0 {
            return None;
        }
        let front = self.samples[self.head as usize];
        self.head = (self.head + 1) & (CAPACITY as u8 - 1);
        self.count -= 1;
        Some(front)
    }
    pub fn pending_count(&self) -> usize {
        self.count as usize
    }
    pub fn latest_seen(&self) -> Option<u32> {
        self.seen
    }
    pub fn last_consumed_hold(&self) -> u16 {
        self.hold
    }
}
