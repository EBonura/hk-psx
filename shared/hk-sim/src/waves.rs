//! The `Battle Control` of an arena that fights in waves of ordinary enemies
//! (Crossroads_22's Hatcher and Spitter arena), as opposed to a boss room.
//!
//! The same FSM family as `boss::Arena` and the same gates, but its middle is a
//! loop: `Wave n` activates that wave's summon objects, broadcasts SUMMON, sets
//! `Battle Enemies` to the wave's size and waits, every frame, for it to reach
//! zero (each member's HealthManager decrements it on death). Then `Pause W n`
//! waits 0.75 s and the next wave starts, so one wave is ever alive at a time.
//! After the last one, `End Pause` writes `Activated` and waits 1 s, `Blob Open`
//! waits 2 s, and `End` sends BG OPEN.
//!
//! Evidence: the `Battle Control` FSM of level59 (host/hk-cook battle_waves.rs
//! checks its shape before it admits a scene).
//!
//! The caller owns the trigger box, the gates, the camera lock, the summon
//! members and the persistent store; this type owns only the order.

/// `Pause W n`: `Wait 0.75`.
pub const WAVE_PAUSE_TICKS: u16 = 45;
/// `End Pause`: `Wait 1.0`.
pub const END_PAUSE_TICKS: u16 = 60;
/// `Blob Open`: `Wait 2.0`.
pub const BLOB_OPEN_TICKS: u16 = 120;
/// Waves an arena can hold (the source has four).
pub const MAX_WAVES: usize = 4;
/// Members one wave can hold, which is also the most pending summons.
pub const MAX_MEMBERS: usize = 4;
/// `summon`'s `Random Pause`: `WaitRandom` 0.25 s .. 1.0 s.
pub const SUMMON_PAUSE_TICKS: (u16, u16) = (15, 60);
/// `summon`'s `Enter`: the fly-in lasts `Tween Time`, `RandomFloat` 0.75 .. 1.2 s.
pub const FLY_IN_TICKS: (u16, u16) = (45, 72);

/// Ticks from SUMMON to the member standing at its place: the random pause,
/// then the fly-in, each uniform over its range. `state` is the caller's own
/// xorshift word and must not be zero.
pub fn summon_delay(state: &mut u32) -> u16 {
    fn draw(state: &mut u32, (lo, hi): (u16, u16)) -> u16 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *state = x;
        lo + (x % u32::from(hi - lo + 1)) as u16
    }
    draw(state, SUMMON_PAUSE_TICKS) + draw(state, FLY_IN_TICKS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Init`'s `Trigger2dEvent`: gates open, waiting for the hero.
    Waiting,
    /// `Wave n`: its members are summoned and `Battle Enemies` counts them down.
    Wave,
    /// `Pause W n`.
    Pause,
    /// `End Pause`, then `Blob Open`: the arena is won, the gates still shut.
    Ending,
    /// `End`, and the `Activated` branch a later visit takes.
    Open,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `BG CLOSE`, broadcast to every gate and to every `Remove on battle start`.
    CloseGates,
    /// `BG OPEN` at the end of this visit.
    OpenGates,
    /// The `Activated` branch of `Init`: gates placed open, nothing fights.
    QuickOpenGates,
    /// `ActivateAllChildren` on `Wave n` and the SUMMON broadcast (1 based).
    StartWave(u8),
    CameraLock(bool),
    /// `End Pause` set `Activated`: it must be saved.
    Persist,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actions {
    values: [Option<Action>; 4],
    count: u8,
}
impl Actions {
    const fn new() -> Self {
        Self {
            values: [None; 4],
            count: 0,
        }
    }
    fn push(&mut self, action: Action) {
        self.values[self.count as usize] = Some(action);
        self.count += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = Action> + '_ {
        self.values[..self.count as usize]
            .iter()
            .map(|a| a.unwrap())
    }
    pub fn contains(&self, action: Action) -> bool {
        self.iter().any(|a| a == action)
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveArena {
    phase: Phase,
    /// 1 based wave in progress (or just finished, while pausing).
    wave: u8,
    waves: u8,
    /// `SetIntValue` of each wave: the members it counts.
    sizes: [u8; MAX_WAVES],
    /// `Battle Enemies`.
    enemies: i32,
    timer: u16,
    /// Which half of `Ending` the timer is in: false `End Pause`, true `Blob Open`.
    blob: bool,
    activated: bool,
}
impl WaveArena {
    /// `Pause` -> `Init`: an arena whose `Activated` survived a save takes the
    /// `Activated` branch and never fights again.
    pub fn new(activated: bool, sizes: &[u8]) -> Self {
        assert!(!sizes.is_empty() && sizes.len() <= MAX_WAVES);
        let mut table = [0; MAX_WAVES];
        table[..sizes.len()].copy_from_slice(sizes);
        Self {
            phase: if activated {
                Phase::Open
            } else {
                Phase::Waiting
            },
            wave: 0,
            waves: sizes.len() as u8,
            sizes: table,
            enemies: 0,
            timer: 0,
            blob: false,
            activated,
        }
    }
    /// What `Init` does before its first idle frame.
    pub fn enter(&self) -> Actions {
        let mut out = Actions::new();
        if self.activated {
            out.push(Action::QuickOpenGates);
        } else {
            out.push(Action::CameraLock(false));
        }
        out
    }
    pub fn phase(self) -> Phase {
        self.phase
    }
    pub fn wave(self) -> u8 {
        self.wave
    }
    pub fn activated(self) -> bool {
        self.activated
    }
    pub fn enemies(self) -> i32 {
        self.enemies
    }
    fn start_wave(&mut self, n: u8, out: &mut Actions) {
        self.phase = Phase::Wave;
        self.wave = n;
        self.enemies = i32::from(self.sizes[n as usize - 1]);
        out.push(Action::StartWave(n));
    }
    /// `Trigger2dEvent` HIT: the hero's collider entered the Battle Scene box.
    pub fn hero_entered(&mut self) -> Actions {
        let mut out = Actions::new();
        if self.phase != Phase::Waiting {
            return out;
        }
        out.push(Action::CameraLock(true));
        out.push(Action::CloseGates);
        self.start_wave(1, &mut out);
        out
    }
    /// A summoned member's `Decrement Battle Enemies`.
    pub fn enemy_died(&mut self) {
        self.enemies -= 1;
    }
    /// One 60 Hz frame of the waits and of the every-frame `IntCompare`.
    pub fn tick(&mut self) -> Actions {
        let mut out = Actions::new();
        match self.phase {
            Phase::Wave => {
                if self.enemies <= 0 {
                    if self.wave < self.waves {
                        self.phase = Phase::Pause;
                        self.timer = WAVE_PAUSE_TICKS;
                    } else {
                        self.phase = Phase::Ending;
                        self.blob = false;
                        self.timer = END_PAUSE_TICKS;
                        self.activated = true;
                        out.push(Action::Persist);
                    }
                }
            }
            Phase::Pause => {
                self.timer -= 1;
                if self.timer == 0 {
                    let next = self.wave + 1;
                    self.start_wave(next, &mut out);
                }
            }
            Phase::Ending => {
                self.timer -= 1;
                if self.timer == 0 {
                    if !self.blob {
                        self.blob = true;
                        self.timer = BLOB_OPEN_TICKS;
                    } else {
                        self.phase = Phase::Open;
                        out.push(Action::OpenGates);
                        out.push(Action::CameraLock(false));
                    }
                }
            }
            Phase::Waiting | Phase::Open => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;
    const SIZES: [u8; 4] = [2, 3, 3, 4];
    fn run(arena: &mut WaveArena, ticks: u16) -> Vec<Action> {
        let mut all = Vec::new();
        for _ in 0..ticks {
            all.extend(arena.tick().iter());
        }
        all
    }
    #[test]
    fn nothing_happens_until_the_hero_crosses_the_trigger() {
        let mut arena = WaveArena::new(false, &SIZES);
        assert_eq!(
            arena.enter().iter().collect::<Vec<_>>(),
            [Action::CameraLock(false)]
        );
        assert!(run(&mut arena, 600).is_empty());
        assert_eq!(arena.phase(), Phase::Waiting);
    }
    #[test]
    fn the_first_wave_closes_the_gates_and_counts_its_members() {
        let mut arena = WaveArena::new(false, &SIZES);
        let started: Vec<_> = arena.hero_entered().iter().collect();
        assert_eq!(
            started,
            [
                Action::CameraLock(true),
                Action::CloseGates,
                Action::StartWave(1)
            ]
        );
        assert_eq!(
            (arena.phase(), arena.wave(), arena.enemies()),
            (Phase::Wave, 1, 2)
        );
        // A second crossing does nothing.
        assert!(arena.hero_entered().is_empty());
    }
    #[test]
    fn a_wave_ends_when_every_member_is_dead_and_the_next_starts_after_the_pause() {
        let mut arena = WaveArena::new(false, &SIZES);
        arena.hero_entered();
        arena.enemy_died();
        assert!(run(&mut arena, 300).is_empty(), "one of two is still alive");
        arena.enemy_died();
        assert!(arena.tick().is_empty());
        assert_eq!(arena.phase(), Phase::Pause);
        let actions = run(&mut arena, WAVE_PAUSE_TICKS);
        assert_eq!(actions, [Action::StartWave(2)]);
        assert_eq!((arena.wave(), arena.enemies()), (2, 3));
    }
    #[test]
    fn the_last_wave_saves_at_once_and_opens_after_three_seconds() {
        let mut arena = WaveArena::new(false, &SIZES);
        arena.hero_entered();
        for (n, size) in SIZES.iter().enumerate() {
            for _ in 0..*size {
                arena.enemy_died();
            }
            let after = arena.tick();
            if n + 1 < SIZES.len() {
                assert!(after.is_empty());
                assert_eq!(
                    run(&mut arena, WAVE_PAUSE_TICKS),
                    [Action::StartWave(n as u8 + 2)]
                );
            } else {
                assert_eq!(after.iter().collect::<Vec<_>>(), [Action::Persist]);
            }
        }
        assert!(arena.activated());
        let rest = run(&mut arena, END_PAUSE_TICKS + BLOB_OPEN_TICKS);
        assert_eq!(rest, [Action::OpenGates, Action::CameraLock(false)]);
        assert_eq!(arena.phase(), Phase::Open);
        assert!(run(&mut arena, 600).is_empty());
    }
    #[test]
    fn a_summon_takes_between_one_and_two_and_a_quarter_seconds() {
        let mut state = 0x1234_5678;
        let (mut lo, mut hi) = (u16::MAX, 0);
        for _ in 0..2000 {
            let d = summon_delay(&mut state);
            lo = lo.min(d);
            hi = hi.max(d);
        }
        assert!(lo >= 60 && hi <= 132, "{lo}..{hi}");
        assert!(lo < 70 && hi > 120, "the range is used: {lo}..{hi}");
    }
    #[test]
    fn an_activated_arena_places_its_gates_open_and_never_fights() {
        let mut arena = WaveArena::new(true, &SIZES);
        assert_eq!(
            arena.enter().iter().collect::<Vec<_>>(),
            [Action::QuickOpenGates]
        );
        assert!(arena.hero_entered().is_empty());
        assert!(run(&mut arena, 600).is_empty());
    }
}
