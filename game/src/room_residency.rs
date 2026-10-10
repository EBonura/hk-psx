//! Bounded RAM residency policy, independent of CD transport and GPU calls.
pub const SLOT_COUNT: usize = 5;
pub const EMPTY: usize = usize::MAX;
#[derive(Clone, Copy)]
pub struct Slot {
    pub region: usize,
    pub len: usize,
    used: u32,
}
impl Slot {
    const fn new() -> Self {
        Self {
            region: EMPTY,
            len: 0,
            used: 0,
        }
    }
}
pub struct Residency {
    pub slots: [Slot; SLOT_COUNT],
    pub current: usize,
    protected: usize,
    loading: [usize; SLOT_COUNT],
    wanted: [usize; SLOT_COUNT - 1],
    clock: u32,
    decode_ready: usize,
}
impl Residency {
    pub const fn new() -> Self {
        Self {
            slots: [Slot::new(); SLOT_COUNT],
            current: 0,
            protected: EMPTY,
            loading: [EMPTY; SLOT_COUNT],
            wanted: [EMPTY; SLOT_COUNT - 1],
            clock: 0,
            decode_ready: EMPTY,
        }
    }
    pub fn find(&self, region: usize) -> Option<usize> {
        if region == EMPTY {
            None
        } else {
            self.slots
                .iter()
                .position(|s| s.region == region && s.len != 0)
        }
    }
    /// A completed CD payload is not yet a Room. len=0 is exclusively stored
    /// data; only a private decoder may turn it into a verified resident entry.
    pub fn stored(&self, region: usize) -> Option<usize> {
        if region == EMPTY {
            None
        } else {
            self.slots
                .iter()
                .position(|s| s.region == region && s.len == 0)
        }
    }
    fn refresh_decode(&mut self) {
        self.decode_ready = self
            .wanted
            .iter()
            .find_map(|&r| self.stored(r))
            .unwrap_or(EMPTY);
    }
    pub fn next_decode(&self, demand: usize) -> Option<usize> {
        self.stored(demand).or(if self.decode_ready == EMPTY {
            None
        } else {
            Some(self.decode_ready)
        })
    }
    /// Preserve speculative read/decode overlap. Only an explicit unread
    /// urgent demand can hold the idle decoder while transport is live.
    pub fn next_decode_during_read(&self, demand: usize, reading: usize) -> Option<usize> {
        let slot = self.next_decode(demand)?;
        if self.slots[slot].region == demand || !self.loading(reading) {
            return Some(slot);
        }
        // The demand may await the current transfer's Pause/cancel. Mere
        // wishlist priority is not urgency and must not serialize speculation.
        if demand != EMPTY && self.find(demand).is_none() {
            None
        } else {
            Some(slot)
        }
    }
    /// Caller must have observed transport Done, including Pause retirement.
    /// This releases the IRQ lease but does not publish or pin unverified bytes.
    pub fn park_completed(&mut self, i: usize, region: usize) {
        assert_eq!(self.loading[i], region);
        assert_ne!(region, EMPTY);
        assert_ne!(i, self.current);
        assert_ne!(i, self.protected);
        self.loading[i] = EMPTY;
        self.slots[i].region = region;
        self.slots[i].len = 0;
        self.touch(i);
        self.refresh_decode();
    }
    pub fn claim_stored(&mut self, i: usize) -> usize {
        let region = self.slots[i].region;
        assert_ne!(region, EMPTY);
        assert_eq!(self.slots[i].len, 0);
        assert_eq!(self.loading[i], EMPTY);
        assert_ne!(i, self.current);
        assert_ne!(i, self.protected);
        self.slots[i] = Slot::new();
        self.loading[i] = region;
        self.refresh_decode();
        region
    }
    pub fn protect_upload(&mut self, region: Option<usize>) -> bool {
        match region {
            None => {
                self.protected = EMPTY;
                true
            }
            Some(region) => match self.find(region) {
                Some(i) => {
                    self.protected = i;
                    true
                }
                None => false,
            },
        }
    }
    pub fn set_wanted(&mut self, regions: &[usize]) {
        let previous = self.wanted;
        self.wanted = [EMPTY; SLOT_COUNT - 1];
        let mut count = 0;
        for &region in regions {
            if region == EMPTY
                || region == self.slots[self.current].region
                || self.wanted.contains(&region)
            {
                continue;
            }
            self.wanted[count] = region;
            count += 1;
            if count == self.wanted.len() {
                break;
            }
        }
        if self.wanted != previous {
            self.refresh_decode();
        }
    }
    /// Drop completed or parked payloads outside the current directed edge
    /// keep-set. Current, protected and in-flight slots remain untouched.
    pub fn evict_unwanted(&mut self, keep: &[usize]) -> usize {
        let mut removed = 0;
        for i in 0..SLOT_COUNT {
            if i == self.current || i == self.protected || self.loading[i] != EMPTY {
                continue;
            }
            let region = self.slots[i].region;
            if region == EMPTY || keep.contains(&region) {
                continue;
            }
            self.slots[i] = Slot::new();
            removed += 1;
        }
        if removed != 0 {
            self.refresh_decode();
        }
        removed
    }
    pub fn loading(&self, region: usize) -> bool {
        region != EMPTY && self.loading.contains(&region)
    }
    pub fn release(&mut self, i: usize) {
        self.loading[i] = EMPTY;
    }
    pub fn next_request(&self) -> Option<usize> {
        self.wanted.iter().copied().find(|&r| {
            r != EMPTY
                && self.find(r).is_none()
                && self.stored(r).is_none()
                && !self.loading(r)
                && self.victim(r, false).is_some()
        })
    }
    fn touch(&mut self, i: usize) {
        // Rebase before wraparound; preserve recent ordering over long sessions.
        self.clock = self.clock.saturating_add(1);
        if self.clock == u32::MAX {
            for slot in &mut self.slots {
                slot.used = slot.used.saturating_sub(u32::MAX / 2);
            }
            self.clock -= u32::MAX / 2;
        }
        self.slots[i].used = self.clock;
    }
    pub fn select(&mut self, region: usize) -> Option<bool> {
        let i = self.find(region)?;
        let changed = i != self.current;
        self.current = i;
        self.touch(i);
        Some(changed)
    }
    /// Reserve one private arena. Current and an in-progress GPU upload are
    /// never evicted; prefer empty, then no-longer-requested, then least recent.
    fn victim(&self, region: usize, urgent: bool) -> Option<usize> {
        if self.loading(region) || self.stored(region).is_some() {
            return None;
        }
        let priority = self
            .wanted
            .iter()
            .position(|&r| r == region)
            .unwrap_or(usize::MAX);
        let mut victim = None;
        for i in 0..SLOT_COUNT {
            if i == self.current || i == self.protected || self.loading[i] != EMPTY {
                continue;
            }
            let s = &self.slots[i];
            // A lower-priority speculative target must not displace an earlier
            // wishlist entry when a pinned upload temporarily reduces capacity.
            if !urgent
                && s.region != EMPTY
                && self
                    .wanted
                    .iter()
                    .position(|&r| r == s.region)
                    .is_some_and(|p| p <= priority)
            {
                continue;
            }
            let score = (s.region != EMPTY, self.wanted.contains(&s.region), s.used);
            if victim.map_or(true, |(_, best)| score < best) {
                victim = Some((i, score));
            }
        }
        victim.map(|(i, _)| i)
    }
    pub fn can_reserve(&self, region: usize, urgent: bool) -> bool {
        self.victim(region, urgent).is_some()
    }
    pub fn reserve(&mut self, region: usize, urgent: bool) -> Option<usize> {
        let i = self.victim(region, urgent)?;
        self.slots[i] = Slot::new();
        self.loading[i] = region;
        self.refresh_decode();
        Some(i)
    }
    pub fn admit(&mut self, i: usize, region: usize, len: usize) {
        assert_eq!(self.loading[i], region);
        assert_ne!(len, 0);
        self.loading[i] = EMPTY;
        self.slots[i].region = region;
        self.slots[i].len = len;
        self.touch(i);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn load(r: &mut Residency, region: usize) -> usize {
        let i = r.reserve(region, true).unwrap();
        r.admit(i, region, 100);
        i
    }
    #[test]
    fn decode_admission_only_waits_for_an_urgent_unread_demand_with_live_transport() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10);
        let stored = r.reserve(12, true).unwrap();
        r.park_completed(stored, 12);
        r.set_wanted(&[11, 12]);
        // Missing11, and even a stale reading ID without a private lease, are
        // insufficient reasons to hold a complete candidate indefinitely.
        assert_eq!(r.next_decode_during_read(EMPTY, EMPTY), Some(stored));
        assert_eq!(r.next_decode_during_read(EMPTY, 11), Some(stored));
        let read = r.reserve(11, true).unwrap();
        assert_eq!(r.next_decode_during_read(EMPTY, 11), Some(stored));
        assert_eq!(r.next_decode_during_read(11, 11), None);
        assert_eq!(r.next_decode_during_read(12, 11), Some(stored));
        r.set_wanted(&[12, 11]);
        assert_eq!(r.next_decode_during_read(EMPTY, 11), Some(stored));
        // An unread urgent destination may await the live transport's cancel.
        assert_eq!(r.next_decode_during_read(13, 11), None);
        r.release(read);
        assert_eq!(r.next_decode_during_read(13, EMPTY), Some(stored));
        // A read removed from the plan no longer outranks a wanted candidate.
        r.reserve(11, true).unwrap();
        r.set_wanted(&[12]);
        assert_eq!(r.next_decode_during_read(EMPTY, 11), Some(stored));
    }
    #[test]
    fn obsolete_completed_payload_is_evictable_but_never_visible_as_a_room() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        load(&mut r, 12);
        load(&mut r, 13);
        r.protect_upload(Some(12));
        let old = r.reserve(14, true).unwrap();
        r.set_wanted(&[11, 12, 13, 15]);
        assert_eq!(r.next_request(), None);
        r.park_completed(old, 14);
        assert!(!r.loading(14));
        assert_eq!(r.stored(14), Some(old));
        assert!(r.find(14).is_none());
        assert_eq!(r.select(14), None);
        assert!(!r.protect_upload(Some(14)));
        assert_eq!(r.next_decode(EMPTY), None);
        assert_eq!(r.next_request(), Some(15));
        let next = r.reserve(15, false).unwrap();
        assert_eq!(next, old);
        assert!(r.stored(14).is_none());
        assert_eq!(r.next_decode(EMPTY), None);
        for id in [10, 11, 12, 13] {
            assert!(r.find(id).is_some());
        }
    }
    #[test]
    fn brief_wishlist_reversal_reuses_completed_bytes_without_another_cd_request() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        let read = r.reserve(11, true).unwrap();
        let decoder = r.reserve(12, true).unwrap();
        r.set_wanted(&[13]);
        r.park_completed(read, 11);
        assert!(r.loading(12));
        assert_eq!(r.next_decode(EMPTY), None);
        r.set_wanted(&[11, 13]);
        assert_eq!(r.next_decode(EMPTY), Some(read));
        assert_eq!(r.next_request(), Some(13));
        assert!(!r.can_reserve(11, true));
        assert_eq!(r.claim_stored(read), 11);
        assert!(r.stored(11).is_none());
        assert!(r.loading(11) && r.loading(12));
        assert_eq!(r.next_decode(EMPTY), None);
        // Bytes remain private until the actual decoder confirms both hashes.
        assert!(r.find(11).is_none());
        r.admit(read, 11, 100);
        assert!(r.find(11).is_some());
        r.release(decoder);
        assert!(r.find(12).is_none());
    }
    #[test]
    fn decoder_priority_follows_demand_then_latest_wishlist_without_reordering_cd_leases() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        let a = r.reserve(11, true).unwrap();
        let b = r.reserve(12, true).unwrap();
        r.park_completed(a, 11);
        r.park_completed(b, 12);
        r.set_wanted(&[11, 12]);
        assert_eq!(r.next_decode(EMPTY), Some(a));
        assert_eq!(r.next_decode(12), Some(b));
        assert_eq!(r.claim_stored(b), 12);
        assert_eq!(r.next_decode(EMPTY), Some(a));
        r.set_wanted(&[13]);
        assert_eq!(r.next_decode(EMPTY), None);
        assert_eq!(r.next_decode(11), Some(a)); // Urgent, even outside the wishlist.
        r.release(b);
        assert!(r.find(12).is_none() && r.stored(12).is_none());
        assert_eq!(r.next_request(), Some(13));
    }
    #[test]
    fn wanted_completed_payload_survives_lower_priority_capacity_pressure() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        let pinned = load(&mut r, 99);
        r.protect_upload(Some(99));
        let decode = r.reserve(12, true).unwrap();
        let read = r.reserve(11, true).unwrap();
        load(&mut r, 13);
        r.set_wanted(&[11, 12, 13, 14]);
        r.park_completed(read, 11);
        assert_eq!(r.next_request(), None);
        assert!(!r.can_reserve(14, false));
        assert_eq!(r.next_decode(EMPTY), Some(read));
        r.protect_upload(None);
        let next = r.reserve(14, false).unwrap();
        assert_eq!(next, pinned);
        assert_ne!(next, read);
        assert_ne!(next, decode);
        assert_eq!(r.next_decode(EMPTY), Some(read));
    }
    #[test]
    fn concurrent_decode_and_read_keep_distinct_private_arenas() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        r.protect_upload(Some(11));
        load(&mut r, 14);
        r.set_wanted(&[14, 12, 13, 15]);
        let decode = r.reserve(12, false).unwrap();
        let read = r.reserve(13, false).unwrap();
        assert_ne!(decode, read);
        assert!(r.loading(12) && r.loading(13));
        assert_eq!(r.reserve(12, true), None);
        assert!(!r.can_reserve(13, true));
        assert!(r.find(12).is_none() && r.find(13).is_none());
        assert_eq!(r.next_request(), None);
        assert_eq!(r.reserve(15, false), None);
        assert!(r.can_reserve(15, true)); // Urgent demand may replace unpinned14.
        r.admit(decode, 12, 100);
        assert!(r.find(12).is_some());
        assert!(r.loading(13));
        r.release(read);
        assert!(!r.loading(13));
        assert_eq!(r.next_request(), Some(13));
        assert!(r.find(10).is_some() && r.find(11).is_some());
    }
    #[test]
    fn cancelled_read_cannot_release_completed_decode_or_gpu_pin() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        let decode = r.reserve(11, true).unwrap();
        let read = r.reserve(12, true).unwrap();
        r.admit(decode, 11, 100);
        r.protect_upload(Some(11));
        r.release(read);
        let replacement = r.reserve(13, true).unwrap();
        assert_ne!(replacement, decode);
        assert!(r.find(11).is_some());
        assert!(r.loading(13));
    }
    #[test]
    fn keeps_current_and_background_upload_while_loading_two_ahead() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        assert!(r.protect_upload(Some(11)));
        load(&mut r, 12);
        load(&mut r, 13);
        load(&mut r, 14);
        load(&mut r, 15);
        assert!(r.find(10).is_some());
        assert!(r.find(11).is_some());
        assert!(r.find(12).is_none());
        assert!(r.find(13).is_some());
        assert!(r.find(14).is_some());
    }
    #[test]
    fn queue_deduplicates_and_skips_already_resident_neighbors() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        r.set_wanted(&[10, 11, 11, 12, 13, 14]);
        assert_eq!(r.next_request(), Some(11));
        load(&mut r, 11);
        assert_eq!(r.next_request(), Some(12));
        load(&mut r, 12);
        assert_eq!(r.next_request(), Some(13));
        load(&mut r, 13);
        assert_eq!(r.next_request(), Some(14));
        load(&mut r, 14);
        assert_eq!(r.next_request(), None);
    }
    #[test]
    fn replaces_unwanted_newer_neighbor_before_requested_older_one() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        load(&mut r, 12);
        load(&mut r, 13);
        load(&mut r, 14);
        r.set_wanted(&[11, 12, 15]);
        load(&mut r, 15);
        assert!(r.find(11).is_some());
        assert!(r.find(12).is_some());
        assert!(r.find(13).is_none());
    }
    #[test]
    fn protected_stale_upload_cannot_make_requested_neighbors_thrash() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        r.protect_upload(Some(11));
        r.set_wanted(&[12, 13, 14, 15]);
        for expected in [12, 13, 14] {
            assert_eq!(r.next_request(), Some(expected));
            let i = r.reserve(expected, false).unwrap();
            r.admit(i, expected, 100);
        }
        assert_eq!(r.next_request(), None);
        assert!(r.reserve(15, false).is_none());
        r.protect_upload(None);
        assert_eq!(r.next_request(), Some(15));
        let i = r.reserve(15, false).unwrap();
        r.admit(i, 15, 100);
        for region in [10, 12, 13, 14, 15] {
            assert!(r.find(region).is_some());
        }
    }
    #[test]
    fn missing_protection_request_preserves_prior_pin() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        assert!(r.protect_upload(Some(11)));
        assert!(!r.protect_upload(Some(99)));
        for region in 12..20 {
            load(&mut r, region);
        }
        assert!(r.find(11).is_some());
        assert!(r.protect_upload(None));
        for region in 20..24 {
            load(&mut r, region);
        }
        assert!(r.find(11).is_none());
    }
    #[test]
    fn directed_eviction_keeps_current_upload_and_requested_payloads() {
        let mut r = Residency::new();
        load(&mut r, 10);
        r.select(10).unwrap();
        load(&mut r, 11);
        load(&mut r, 12);
        let parked = r.reserve(13, true).unwrap();
        r.park_completed(parked, 13);
        r.protect_upload(Some(11));
        assert_eq!(r.evict_unwanted(&[12]), 1);
        assert!(r.find(10).is_some());
        assert!(r.find(11).is_some());
        assert!(r.find(12).is_some());
        assert!(r.stored(13).is_none());
    }
}
