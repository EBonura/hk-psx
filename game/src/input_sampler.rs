//! Cooperative sampling of actual controller observations; no hardware/IRQ owner.
use crate::input_queue::{Error as QueueError, Queue, CAPACITY};
const HALF: u32 = 1 << 31;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    AlreadyStarted,
    ClockOrder,
    AmbiguousClock,
    QueueFull,
    QueueOrder,
    NonConsecutiveUpdate,
    FutureUpdate,
    LateSample,
    NotStarted,
    SceneLoadActive,
    SceneLoadInactive,
}
impl Fault {
    pub const fn code(self) -> u32 {
        self as u32 + 1
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub polls: u32,
    pub max_poll_gap: u32,
    pub missed_vblanks: u32,
    pub pending_peak: u32,
    pub loading_samples: u32,
    pub loading_ticks: u32,
    /// VBlanks without a poll while a blocking transfer owned the port (a
    /// memory card frame cannot be interrupted by a pad poll). Not a miss.
    pub blocked_vblanks: u32,
}
pub struct Sampler {
    queue: Queue,
    enabled: bool,
    loading: bool,
    blocking: bool,
    observed: u32,
    last_poll: u32,
    consumed: u32,
    stats: Stats,
    fault: Option<Fault>,
}
fn forward(a: u32, b: u32) -> Result<(), Fault> {
    match b.wrapping_sub(a) {
        HALF => Err(Fault::AmbiguousClock),
        n if n > HALF => Err(Fault::ClockOrder),
        _ => Ok(()),
    }
}
impl Sampler {
    pub const fn new() -> Self {
        Self {
            queue: Queue::new(),
            enabled: false,
            loading: false,
            blocking: false,
            observed: 0,
            last_poll: 0,
            consumed: 0,
            stats: Stats {
                polls: 0,
                max_poll_gap: 0,
                missed_vblanks: 0,
                pending_peak: 0,
                loading_samples: 0,
                loading_ticks: 0,
                blocked_vblanks: 0,
            },
            fault: None,
        }
    }
    /// Called once after boot/menu/preload. Region activation is not a reset.
    pub fn start(&mut self, tick: u32) -> Result<(), Fault> {
        if self.enabled {
            return self.fail(Fault::AlreadyStarted);
        }
        self.queue.reset(tick);
        self.enabled = true;
        self.observed = tick;
        self.last_poll = tick;
        self.consumed = tick;
        Ok(())
    }
    /// A scene transition owns this explicit pause. It may acknowledge pending
    /// observations, but may not reset history or start at a different sim tick.
    pub fn begin_scene_load(&mut self, tick: u32) -> Result<(), Fault> {
        if let Some(fault) = self.fault { return Err(fault); }
        if !self.enabled { return self.fail(Fault::NotStarted); }
        if self.loading { return self.fail(Fault::SceneLoadActive); }
        if tick != self.consumed { return self.fail(Fault::NonConsecutiveUpdate); }
        self.loading = true;
        self.acknowledge_load()
    }
    /// Acknowledge only observed time. Never read a newer hardware clock here:
    /// a VBlank arriving after the final checkpoint belongs to normal resume.
    pub fn end_scene_load(&mut self) -> Result<u32, Fault> {
        if let Some(fault) = self.fault { return Err(fault); }
        if !self.loading { return self.fail(Fault::SceneLoadInactive); }
        self.loading = false;
        self.blocking = false;
        Ok(self.consumed)
    }
    /// Inside a scene load only: the port is about to run transfers longer
    /// than a VBlank (memory card frames). Their poll gaps count as blocked,
    /// not missed. Cleared by end_scene_load.
    pub fn begin_blocking_transfer(&mut self) -> Result<(), Fault> {
        if let Some(fault) = self.fault { return Err(fault); }
        if !self.loading { return self.fail(Fault::SceneLoadInactive); }
        self.blocking = true;
        Ok(())
    }
    fn acknowledge_load(&mut self) -> Result<(), Fault> {
        // At most CAPACITY queued observations, even after a long clock gap.
        // Preserve the last observed hold so held buttons do not become edges
        // merely because the destination scene resumed.
        loop {
            match self.queue.take(self.observed) {
                Ok(Some(_)) => self.stats.loading_samples = self.stats.loading_samples.saturating_add(1),
                Ok(None) => break,
                Err(QueueError::AmbiguousWrap) => return self.fail(Fault::AmbiguousClock),
                Err(_) => return self.fail(Fault::QueueOrder),
            }
        }
        self.stats.loading_ticks = self.stats.loading_ticks.saturating_add(self.observed.wrapping_sub(self.consumed));
        self.consumed = self.observed;
        Ok(())
    }
    pub fn held_buttons(&self) -> u16 {
        self.queue.last_consumed_hold()
    }
    fn fail<T>(&mut self, fault: Fault) -> Result<T, Fault> {
        self.fault = Some(fault);
        Err(fault)
    }
    /// Cheap wrapper guard; any changed clock (including a backward clock) must
    /// reach checkpoint's full validation. A latched fault is never hidden.
    #[inline(always)]
    pub fn poll_due(&self, now: u32) -> bool {
        self.enabled && (now != self.last_poll || self.fault.is_some())
    }
    /// The callback performs one real poll, then returns its completion-clock
    /// timestamp and active-high buttons. If the transfer crosses VBlank, that
    /// later timestamp is retained and no second poll is issued in that VBlank.
    /// Boot calls, same-VBlank checkpoints, and latched faults never invoke it.
    pub fn checkpoint(
        &mut self,
        now: u32,
        poll: impl FnOnce() -> (u32, u16),
    ) -> Result<bool, Fault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if !self.enabled {
            return Ok(false);
        }
        if let Err(fault) = forward(self.observed, now) {
            return self.fail(fault);
        }
        self.observed = now;
        if now == self.last_poll {
            return Ok(false);
        }
        if self.queue.pending_count() == CAPACITY {
            return self.fail(Fault::QueueFull);
        }
        let (completed, buttons) = poll();
        self.stats.polls = self.stats.polls.saturating_add(1);
        if let Err(fault) = forward(now, completed) {
            return self.fail(fault);
        }
        let gap = completed.wrapping_sub(self.last_poll);
        if gap >= HALF {
            return self.fail(if gap == HALF {
                Fault::AmbiguousClock
            } else {
                Fault::ClockOrder
            });
        }
        let previous_poll = self.queue.latest_seen();
        if let Err(error) = self.queue.push(completed, buttons) {
            return self.fail(match error {
                QueueError::Full => Fault::QueueFull,
                QueueError::AmbiguousWrap => Fault::AmbiguousClock,
                _ => Fault::QueueOrder,
            });
        }
        // First hardware observation has no earlier actual poll to compare.
        if self.blocking {
            self.stats.blocked_vblanks = self.stats.blocked_vblanks.saturating_add(gap.saturating_sub(1));
        } else {
            if previous_poll.is_some() {
                self.stats.max_poll_gap = self.stats.max_poll_gap.max(gap);
            }
            self.stats.missed_vblanks = self
                .stats
                .missed_vblanks
                .saturating_add(gap.saturating_sub(1));
        }
        self.stats.pending_peak = self
            .stats
            .pending_peak
            .max(self.queue.pending_count() as u32);
        self.last_poll = completed;
        self.observed = completed;
        if self.loading { self.acknowledge_load()?; }
        Ok(true)
    }
    /// Fixed updates consume consecutive real VBlank timestamps. A missing
    /// historical poll uses the previous hold without polling or inventing data.
    /// A skipped consumer tick is an error rather than merging queued pulses.
    pub fn consume(&mut self, tick: u32) -> Result<u16, Fault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if self.loading { return self.fail(Fault::SceneLoadActive); }
        if !self.enabled || tick.wrapping_sub(self.consumed) != 1 {
            return self.fail(Fault::NonConsecutiveUpdate);
        }
        if self.observed.wrapping_sub(tick) >= HALF {
            return self.fail(Fault::FutureUpdate);
        }
        let sample = match self.queue.take(tick) {
            Ok(sample) => sample,
            Err(QueueError::AmbiguousWrap) => return self.fail(Fault::AmbiguousClock),
            Err(_) => return self.fail(Fault::QueueOrder),
        };
        if sample.is_some_and(|sample| sample.tick != tick) {
            return self.fail(Fault::LateSample);
        }
        self.consumed = tick;
        Ok(self.queue.last_consumed_hold())
    }
    pub fn stats(&self) -> Stats {
        self.stats
    }
    pub fn pending_count(&self) -> usize {
        self.queue.pending_count()
    }
    pub fn latest_seen(&self) -> Option<u32> {
        self.queue.latest_seen()
    }
    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }
}
