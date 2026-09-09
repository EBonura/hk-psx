//! Bounded animation working set on the pinned SDK's slot cache.
#![no_std]
#[cfg(test)] extern crate std;
pub mod residency;
use psx_cache::SlotCache;

pub const SLOTS: usize = 8;
pub const MAX_REQUESTS: usize = 8;
/// Global texture IDs in an admitted resident scene.
pub const MAX_KEYS: usize = 2048;
pub const MAX_UPLOAD_BYTES: u32 = 16384;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub hits: u32,
    pub misses: u32,
    pub upload_bytes: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Phase,
    WorkingSet,
    Upload,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Prepared,
    InFlight,
}
pub struct Cache {
    slots: SlotCache<u8, SLOTS, MAX_KEYS>,
    keys: [u16; MAX_REQUESTS],
    key_count: usize,
    phase: Phase,
}
impl Cache {
    pub const fn new() -> Self {
        Self {
            slots: SlotCache::new(),
            keys: [0; MAX_REQUESTS],
            key_count: 0,
            phase: Phase::Idle,
        }
    }
    /// Upload closure receives key and physical slot. Its successful byte count
    /// must be 1..=2048. Caller validates dimensions before issuing GPU writes.
    /// Failure leaves no usable reference to a partially uploaded slot.
    pub fn prepare(
        &mut self,
        keys: &[u16],
        mut upload: impl FnMut(u16, usize) -> Option<u32>,
    ) -> Result<Stats, Error> {
        if self.phase != Phase::Idle {
            return Err(Error::Phase);
        }
        let mut unique = [0; MAX_REQUESTS];
        let mut count = 0;
        for &key in keys {
            if key as usize >= MAX_KEYS {
                return Err(Error::WorkingSet);
            }
            if !unique[..count].contains(&key) {
                if count == MAX_REQUESTS {
                    return Err(Error::WorkingSet);
                }
                unique[count] = key;
                count += 1;
            }
        }
        self.slots.bump_epoch();
        // Pin every resident request BEFORE the first miss can evict anything.
        self.slots.set_pinned(&unique[..count]);
        let mut stats = Stats::default();
        for &key in &unique[..count] {
            if self.slots.get(key).is_some() {
                stats.hits += 1;
                continue;
            }
            let slot = self.slots.reserve(key).ok_or(Error::WorkingSet)?;
            match upload(key, slot) {
                Some(bytes) if (1..=2048).contains(&bytes) => {
                    self.slots.mark_ready(slot, slot as u8);
                    self.slots.pin(key);
                    stats.misses += 1;
                    stats.upload_bytes += bytes;
                }
                _ => {
                    self.slots.evict(key);
                    self.slots.unpin_all();
                    return Err(Error::Upload);
                }
            }
        }
        self.keys = unique;
        self.key_count = count;
        self.phase = Phase::Prepared;
        Ok(stats)
    }
    /// Only a member of the declared frame working set may be drawn.
    pub fn slot(&self, key: u16) -> Option<usize> {
        if self.phase == Phase::Idle || !self.keys[..self.key_count].contains(&key) {
            return None;
        }
        self.slots.peek(key).map(|&slot| slot as usize)
    }
    pub fn submit(&mut self) -> Result<(), Error> {
        if self.phase != Phase::Prepared {
            return Err(Error::Phase);
        }
        self.phase = Phase::InFlight;
        Ok(())
    }
    /// Caller must observe DMA completion AND GPU raster completion first.
    pub fn complete(&mut self) -> Result<(), Error> {
        if self.phase != Phase::InFlight {
            return Err(Error::Phase);
        }
        self.slots.unpin_all();
        self.phase = Phase::Idle;
        self.key_count = 0;
        Ok(())
    }
}
impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(c: &mut Cache, keys: &[u16]) -> Stats {
        let stats = c.prepare(keys, |_, _| Some(2048)).unwrap();
        c.submit().unwrap();
        c.complete().unwrap();
        stats
    }
    #[test]
    fn repeated_frames_and_duplicate_requests_do_not_upload() {
        let mut c = Cache::new();
        assert_eq!(frame(&mut c, &[1, 1]).misses, 1);
        assert_eq!(frame(&mut c, &[1]).hits, 1);
        assert_eq!(frame(&mut c, &[1]).upload_bytes, 0);
    }
    #[test]
    fn global_scene_keys_remain_distinct_and_reusable_across_frame_sets() {
        let mut c=Cache::new();
        assert_eq!(frame(&mut c,&[0,640,1114,2047]).misses,4);
        let stats=c.prepare(&[2047,640,0,1114],|_,_|panic!("resident global key uploaded again")).unwrap();
        assert_eq!(stats.hits,4);assert_eq!(stats.upload_bytes,0);
        let slots=[c.slot(0),c.slot(640),c.slot(1114),c.slot(2047)];
        for i in 0..slots.len(){for j in 0..i{assert_ne!(slots[i],slots[j]);}}
        c.submit().unwrap();c.complete().unwrap();
        assert_eq!(c.prepare(&[u16::MAX],|_,_|panic!()),Err(Error::WorkingSet));
        assert_eq!(frame(&mut c,&[2047]).hits,1);
        // A real scene replacement clears global IDs before new bytes reuse them.
        c=Cache::new();assert_eq!(frame(&mut c,&[2047]).misses,1);
    }
    #[test]
    fn incoming_hit_is_pinned_before_lru_miss() {
        let mut c = Cache::new();
        frame(&mut c, &[1, 2]);
        frame(&mut c, &[3, 4]);
        frame(&mut c, &[6, 7, 8, 9]);
        // 1 is the oldest slot, and appears AFTER the miss in this request.
        let stats = c.prepare(&[5, 1], |_, _| Some(2048)).unwrap();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_ne!(c.slot(5), c.slot(1));
        assert_eq!(c.slot(2), None); // undeclared texture cannot be drawn
        c.submit().unwrap();
        c.complete().unwrap();
        assert_eq!(frame(&mut c, &[2]).misses, 1); // 2 was the LRU victim
    }
    #[test]
    fn no_inflight_upload_or_double_prepare() {
        let mut c = Cache::new();
        c.prepare(&[1], |_, _| Some(2048)).unwrap();
        assert_eq!(c.prepare(&[2], |_, _| panic!()), Err(Error::Phase));
        c.submit().unwrap();
        assert_eq!(c.prepare(&[2], |_, _| panic!()), Err(Error::Phase));
        c.complete().unwrap();
        assert_eq!(c.complete(), Err(Error::Phase));
        assert_eq!(c.slot(1), None);
    }
    #[test]
    fn invalid_working_set_rejected_before_any_upload() {
        let mut c = Cache::new();
        assert_eq!(
            c.prepare(&[1, 2, 3, 4, 5, 6, 7, 8, 9], |_, _| panic!()),
            Err(Error::WorkingSet)
        );
        assert_eq!(c.prepare(&[2048], |_, _| panic!()), Err(Error::WorkingSet));
        assert_eq!(frame(&mut c, &[1, 2, 3, 4, 5, 6, 7, 8]).upload_bytes, MAX_UPLOAD_BYTES);
    }
    #[test]
    fn failed_upload_is_not_resident_and_can_be_retried() {
        let mut c = Cache::new();
        assert_eq!(c.prepare(&[1], |_, _| None), Err(Error::Upload));
        assert_eq!(c.slot(1), None);
        assert_eq!(c.prepare(&[1], |_, _| Some(2049)), Err(Error::Upload));
        assert_eq!(frame(&mut c, &[1]).misses, 1);
    }
}
