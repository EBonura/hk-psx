//! Stable-ID state split into bounded global, mode and active-scene owners.
//!
//! Save encoding belongs to the persistence package. This module defines the
//! allocation-free mutation/reset contract that gameplay systems share.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct StateId(pub u64);

/// Current installed-source upper bound: 815 PlayerData keys plus 4,468
/// Breakable owners. Sparse saves may contain fewer entries but never remap IDs.
pub const GLOBAL_CATALOG_CAPACITY: usize = 5_283;
pub const MODE_CATALOG_CAPACITY: usize = 512;
pub const ACTIVE_FLAG_WORDS: usize = 32;
pub const ACTIVE_TIMERS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Entry {
    pub id: StateId,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidId,
    Capacity,
    Overflow,
    ConflictingRewardId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Set(i32),
    Add(i32),
    Maximum(i32),
    Minimum(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mutation {
    pub id: StateId,
    pub operation: Operation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub old: i32,
    pub new: i32,
    pub changed: bool,
}

#[derive(Clone, Copy)]
pub struct Store<const N: usize> {
    entries: [Entry; N],
    len: usize,
}

impl<const N: usize> Store<N> {
    pub const fn new() -> Self {
        Self {
            entries: [Entry {
                id: StateId(0),
                value: 0,
            }; N],
            len: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries[..self.len]
    }
    fn position(&self, id: StateId) -> Result<usize, usize> {
        self.entries[..self.len].binary_search_by_key(&id, |entry| entry.id)
    }
    pub fn get(&self, id: StateId) -> i32 {
        self.position(id)
            .ok()
            .map_or(0, |index| self.entries[index].value)
    }
    pub fn contains(&self, id: StateId) -> bool {
        self.position(id).is_ok()
    }
    fn result(old: i32, operation: Operation) -> Result<i32, Error> {
        match operation {
            Operation::Set(value) => Ok(value),
            Operation::Add(value) => old.checked_add(value).ok_or(Error::Overflow),
            Operation::Maximum(value) => Ok(old.max(value)),
            Operation::Minimum(value) => Ok(old.min(value)),
        }
    }
    pub fn apply(&mut self, mutation: Mutation) -> Result<Change, Error> {
        if mutation.id.0 == 0 {
            return Err(Error::InvalidId);
        }
        match self.position(mutation.id) {
            Ok(index) => {
                let old = self.entries[index].value;
                let new = Self::result(old, mutation.operation)?;
                self.entries[index].value = new;
                Ok(Change {
                    old,
                    new,
                    changed: old != new,
                })
            }
            Err(index) => {
                if self.len == N {
                    return Err(Error::Capacity);
                }
                let new = Self::result(0, mutation.operation)?;
                self.entries.copy_within(index..self.len, index + 1);
                self.entries[index] = Entry {
                    id: mutation.id,
                    value: new,
                };
                self.len += 1;
                Ok(Change {
                    old: 0,
                    new,
                    changed: new != 0,
                })
            }
        }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    /// Apply one reward exactly once even when metadata duplicates the owner.
    pub fn grant_once(&mut self, token: StateId, reward: Mutation) -> Result<bool, Error> {
        if token.0 == 0 || reward.id.0 == 0 {
            return Err(Error::InvalidId);
        }
        if token == reward.id {
            return Err(Error::ConflictingRewardId);
        }
        if self.get(token) != 0 {
            return Ok(false);
        }
        let missing = (if self.contains(token) { 0 } else { 1 })
            + (if self.contains(reward.id) { 0 } else { 1 });
        if self.len + missing > N {
            return Err(Error::Capacity);
        }
        let old = self.get(reward.id);
        Self::result(old, reward.operation)?;
        self.apply(reward)?;
        self.apply(Mutation {
            id: token,
            operation: Operation::Set(1),
        })?;
        Ok(true)
    }
}
impl<const N: usize> Default for Store<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Timer {
    pub owner: StateId,
    pub remaining: u16,
    pub event: u16,
}

#[derive(Clone, Copy)]
pub struct ActiveScene<const WORDS: usize, const TIMERS: usize> {
    pub scene: StateId,
    pub generation: u32,
    bits: [u32; WORDS],
    timers: [Timer; TIMERS],
    timer_len: usize,
}
impl<const WORDS: usize, const TIMERS: usize> ActiveScene<WORDS, TIMERS> {
    pub const fn new() -> Self {
        Self {
            scene: StateId(0),
            generation: 0,
            bits: [0; WORDS],
            timers: [Timer {
                owner: StateId(0),
                remaining: 0,
                event: 0,
            }; TIMERS],
            timer_len: 0,
        }
    }
    pub fn enter(&mut self, scene: StateId) {
        self.scene = scene;
        self.generation = self.generation.wrapping_add(1);
        self.bits.fill(0);
        self.timer_len = 0;
    }
    pub fn bit(&self, index: usize) -> bool {
        index < WORDS * 32 && self.bits[index / 32] & (1 << (index % 32)) != 0
    }
    pub fn set_bit(&mut self, index: usize) -> Result<bool, Error> {
        if index >= WORDS * 32 {
            return Err(Error::Capacity);
        }
        let mask = 1 << (index % 32);
        let old = self.bits[index / 32] & mask != 0;
        self.bits[index / 32] |= mask;
        Ok(!old)
    }
    pub fn add_timer(&mut self, timer: Timer) -> Result<(), Error> {
        if timer.owner.0 == 0 {
            return Err(Error::InvalidId);
        }
        if self.timer_len == TIMERS {
            return Err(Error::Capacity);
        }
        self.timers[self.timer_len] = timer;
        self.timer_len += 1;
        Ok(())
    }
    pub fn timers(&self) -> &[Timer] {
        &self.timers[..self.timer_len]
    }
}
impl<const W: usize, const T: usize> Default for ActiveScene<W, T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reset {
    SceneLeave,
    Bench,
    Death,
    QuitLoad,
    DreamReturn,
    Challenge,
    Mode,
}

pub struct AdventureState<
    const GLOBAL: usize,
    const MODE: usize,
    const WORDS: usize,
    const TIMERS: usize,
> {
    pub global: Store<GLOBAL>,
    pub mode: Store<MODE>,
    pub active: ActiveScene<WORDS, TIMERS>,
}
impl<const G: usize, const M: usize, const W: usize, const T: usize> AdventureState<G, M, W, T> {
    pub const fn new() -> Self {
        Self {
            global: Store::new(),
            mode: Store::new(),
            active: ActiveScene::new(),
        }
    }
    pub fn reset(&mut self, reset: Reset) {
        match reset {
            Reset::Challenge => self.mode.clear(),
            Reset::Mode => {
                self.global.clear();
                self.mode.clear()
            }
            _ => {}
        }
        self.active.enter(StateId(0));
    }
    pub fn load_global(&mut self, saved: Store<G>) {
        self.global = saved;
        self.active.enter(StateId(0));
    }
}
impl<const G: usize, const M: usize, const W: usize, const T: usize> Default
    for AdventureState<G, M, W, T>
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type State = AdventureState<8, 4, 2, 2>;
    #[test]
    fn sorted_stable_entries_and_checked_mutation() {
        let mut store = Store::<3>::new();
        store
            .apply(Mutation {
                id: StateId(9),
                operation: Operation::Set(3),
            })
            .unwrap();
        store
            .apply(Mutation {
                id: StateId(2),
                operation: Operation::Add(4),
            })
            .unwrap();
        assert_eq!(store.entries()[0].id, StateId(2));
        assert_eq!(store.entries()[1].id, StateId(9));
        assert_eq!(
            store.apply(Mutation {
                id: StateId(2),
                operation: Operation::Add(i32::MAX)
            }),
            Err(Error::Overflow)
        );
    }
    #[test]
    fn duplicate_occurrences_grant_one_owner_once() {
        let mut store = Store::<4>::new();
        let reward = Mutation {
            id: StateId(20),
            operation: Operation::Add(100),
        };
        assert_eq!(store.grant_once(StateId(10), reward), Ok(true));
        assert_eq!(store.grant_once(StateId(10), reward), Ok(false));
        assert_eq!(store.get(StateId(20)), 100);
    }
    #[test]
    fn scene_death_bench_and_reload_keep_global_but_clear_active() {
        for reset in [
            Reset::SceneLeave,
            Reset::Death,
            Reset::Bench,
            Reset::QuitLoad,
            Reset::DreamReturn,
        ] {
            let mut state = State::new();
            state
                .global
                .apply(Mutation {
                    id: StateId(1),
                    operation: Operation::Set(7),
                })
                .unwrap();
            state.active.enter(StateId(2));
            state.active.set_bit(5).unwrap();
            state
                .active
                .add_timer(Timer {
                    owner: StateId(3),
                    remaining: 4,
                    event: 5,
                })
                .unwrap();
            state.reset(reset);
            assert_eq!(state.global.get(StateId(1)), 7);
            assert_eq!(state.active.scene, StateId(0));
            assert!(!state.active.bit(5));
            assert!(state.active.timers().is_empty());
        }
    }
    #[test]
    fn challenge_and_mode_reset_have_separate_owners() {
        let mut state = State::new();
        state
            .global
            .apply(Mutation {
                id: StateId(1),
                operation: Operation::Set(1),
            })
            .unwrap();
        state
            .mode
            .apply(Mutation {
                id: StateId(2),
                operation: Operation::Set(1),
            })
            .unwrap();
        state.reset(Reset::Challenge);
        assert_eq!(state.global.get(StateId(1)), 1);
        assert_eq!(state.mode.len(), 0);
        state.reset(Reset::Mode);
        assert_eq!(state.global.len(), 0);
    }
    #[test]
    fn reload_replaces_global_snapshot_and_invalidates_active_generation() {
        let mut state = State::new();
        state.active.enter(StateId(4));
        let generation = state.active.generation;
        let mut saved = Store::new();
        saved
            .apply(Mutation {
                id: StateId(7),
                operation: Operation::Set(9),
            })
            .unwrap();
        state.load_global(saved);
        assert_eq!(state.global.get(StateId(7)), 9);
        assert_eq!(state.active.scene, StateId(0));
        assert_ne!(state.active.generation, generation);
    }
    #[test]
    fn measured_whole_catalog_store_does_not_scale_per_scene() {
        assert_eq!(core::mem::size_of::<Entry>(), 16);
        assert_eq!(
            core::mem::size_of::<Store<GLOBAL_CATALOG_CAPACITY>>(),
            84_536
        );
        assert_eq!(core::mem::size_of::<Store<MODE_CATALOG_CAPACITY>>(), 8_200);
        assert_eq!(
            core::mem::size_of::<ActiveScene<ACTIVE_FLAG_WORDS, ACTIVE_TIMERS>>(),
            1_176
        );
    }
}
