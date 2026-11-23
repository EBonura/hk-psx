//! The Grubfather's `King Control` and his 46 rewards (host/grubfather.py
//! documents the FSM this follows and asserts it against the install).
//!
//! He has no conversation: he beckons while the Knight is away, and once the
//! Knight comes within his range he cheers (the first time a scene load), then
//! hands over one reward per grub saved that he has not yet paid for, with a
//! pause between gifts. `grubRewards` is the number paid so far, and the reward
//! paid is `Reward <grubRewards>`. Grubs saved is the port's own count of freed
//! grub jars (`persist::Kind::Grub`), which is what the source keeps as
//! `grubsCollected`.
//!
//! A Geo reward flings its Geo as single coins through the coin pool, each a
//! weighted draw of 1, 5 or 25 that falls back to the smaller coin when what is
//! left cannot pay it (`Remaining?`). An item reward is handed over at once
//! with its message (pickups' grant): the source flings the item out of
//! `Loose Rewards` to be picked up, and those items have no art in this room.
//!
//! Not reproduced: his art, clips and sounds (he is not drawn), `Fat` (nothing
//! in the port sets `fatGrubKing`), the dream dialogue, `StoryRecord_visited`.
use crate::pickups::Grant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reward {
    /// Geo, and the y it flings from (Q16 world units; the x is `REWARD_X`).
    Geo(u16, i32),
    /// An item and the message that names it ("" for none).
    Item(Grant, &'static str),
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/grubfather.rs"));

/// `King Control`'s states, as numbers so the step is a few compares and two
/// small tables rather than a `match` (a jump table, which a room module cannot
/// carry: host/code_modules.py rule 2). 0 is out of his room.
pub const PAUSE: u8 = 1;
pub const ALONE: u8 = 2;
pub const ALL_GIVEN: u8 = 3;
pub const BECKON: u8 = 4;
/// `Cheer?` playing `Cheer` to its end.
pub const CHEER: u8 = 5;
pub const CHEER_PAUSE: u8 = 6;
/// `Gift Anim` until the `Gift` trigger frame.
pub const GIFT: u8 = 7;
/// `Recover`: the rest of `Gift`.
pub const RECOVER: u8 = 8;
pub const RECOVER_PAUSE: u8 = 9;
/// `Final Reward?` at the last reward: nothing leaves it.
pub const DONE: u8 = 10;
/// Each state's length in ticks; 0 is untimed.
const TICKS: [u16; 11] =
    [0, 0, 0, 0, 0, CHEER_TICKS, CHEER_PAUSE_TICKS, GIFT_TRIGGER_TICKS, GIFT_REST_TICKS, RECOVER_PAUSE_TICKS, 0];
/// Where a timed state goes when it runs out, for the ones that only move on.
const NEXT: [u8; 11] = [0, 0, 0, 0, 0, CHEER_PAUSE, GIFT, 0, RECOVER_PAUSE, 0, 0];

/// The PlayerData the FSM reads and writes, so the state machine runs on the
/// host against plain values.
pub trait Player {
    /// `grubsCollected`.
    fn grubs(&self) -> i32;
    /// `grubRewards`.
    fn rewards(&self) -> i32;
    fn set_rewards(&mut self, value: i32);
    /// `finalGrubRewardCollected`.
    fn set_final(&mut self);
}

pub struct King {
    pub state: u8,
    /// Ticks left in a timed state.
    ticks: u16,
    /// `Given`: an FSM variable, so it lasts while the scene does.
    given: bool,
}
impl King {
    pub const fn new() -> Self {
        Self { state: PAUSE, ticks: 0, given: false }
    }
    #[inline(always)]
    fn go(&mut self, state: u8) {
        self.state = state;
        self.ticks = TICKS[state as usize];
    }
    /// `Init Check` and `Recheck`'s two counts: no grub, every grub paid, or
    /// `otherwise`. `Recheck`'s ALONE has no target, so it stays where it is.
    #[inline(always)]
    fn check(pd: &impl Player, alone: u8, otherwise: u8) -> u8 {
        let grubs = pd.grubs();
        if grubs == 0 {
            alone
        } else if grubs == pd.rewards() {
            ALL_GIVEN
        } else {
            otherwise
        }
    }
    /// One 60 Hz tick. `hero_x` is the Knight's x (Q16 world units); `at_cap`
    /// is CheckGeoCap, the coin pool full. Returns the reward to pay, 1-based,
    /// on the tick `Activate Reward` runs.
    #[inline(always)]
    pub fn step(&mut self, pd: &mut impl Player, hero_x: i32, at_cap: bool) -> Option<usize> {
        if self.ticks > 1 {
            self.ticks -= 1;
            return None;
        }
        let state = self.state;
        if self.ticks == 0 {
            // The untimed states: Pause's frame, then Init Check; Beckon's range.
            if state == PAUSE {
                self.go(Self::check(pd, ALONE, BECKON));
            } else if state == BECKON && (hero_x < 0 || (BECKON_X[0] <= hero_x && hero_x <= BECKON_X[1])) {
                // Cheer?: the cheer once a scene load, then Cheer Pause.
                self.go(if self.given { CHEER_PAUSE } else { CHEER });
                self.given = true;
            }
            return None;
        }
        if state == GIFT {
            // Activate Reward, then Final Reward?.
            let given = pd.rewards() + 1;
            pd.set_rewards(given);
            if given < REWARDS.len() as i32 {
                self.go(RECOVER);
            } else {
                pd.set_final();
                self.go(DONE);
            }
            return usize::try_from(given).ok().filter(|&n| n >= 1 && n <= REWARDS.len());
        }
        if state == RECOVER_PAUSE {
            // Recheck, then At cap?: a full coin pool waits another Recover Pause.
            let next = Self::check(pd, RECOVER_PAUSE, GIFT);
            self.go(if next == GIFT && at_cap { RECOVER_PAUSE } else { next });
        } else {
            self.go(NEXT[state as usize]);
        }
        None
    }
}

/// `Remaining?` to `End`: how many of each coin (small, medium, large) a Geo
/// reward flings. `seed` drives the draws.
pub fn coins(mut geo: u16, mut seed: u32) -> [u16; 3] {
    let mut counts = [0u16; 3];
    while geo > 0 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let roll = ((seed >> 16) % 100) as u8;
        let mut kind = if roll < DRAW_PERCENT[0] {
            0
        } else if roll < DRAW_PERCENT[0] + DRAW_PERCENT[1] {
            1
        } else {
            2
        };
        // Large and Med fall through to the smaller coin on LESS.
        while geo < COIN_VALUES[kind] {
            kind -= 1;
        }
        geo -= COIN_VALUES[kind];
        counts[kind] += 1;
    }
    counts
}

/// The guest half: the one King, his telemetry and the reward's payout. Out of
/// the host test build, which runs `King` against a plain `Player`.
#[cfg(not(test))]
mod live {
    use super::*;
    use crate::player_data::{self as pd, Field};
    const GRUB_REWARDS: Field = pd::GRUB_REWARDS;
    const FINAL_GRUB_REWARD: Field = pd::FINAL_GRUB_REWARD_COLLECTED;
    const _: () = {
        let mut i = 0;
        while i < 3 {
            assert!(COIN_VALUES[i] as u32 == crate::geo::GEO_PARAMS.coins[i].value);
            i += 1;
        }
    };
    struct Store;
    impl Player for Store {
        fn grubs(&self) -> i32 {
            crate::persist::count(crate::persist::Kind::Grub) as i32
        }
        fn rewards(&self) -> i32 {
            pd::store().get(GRUB_REWARDS)
        }
        fn set_rewards(&mut self, value: i32) {
            pd::store().set(GRUB_REWARDS, value);
        }
        fn set_final(&mut self) {
            pd::store().put(FINAL_GRUB_REWARD, true);
        }
    }
    static mut KING: King = King::new();
    /// Rewards paid this session, for routes.
    #[no_mangle]
    pub static mut HK_GRUB_REWARDS_PAID: u32 = 0;
    /// The King's state (`PAUSE` to `DONE`), 0 out of his room.
    #[no_mangle]
    pub static mut HK_GRUBFATHER_PHASE: u32 = 0;
    /// Out of his room: the next entry is a fresh scene load, `Given` and all.
    pub fn idle() {
        unsafe {
            KING = King::new();
            HK_GRUBFATHER_PHASE = 0;
        }
    }
    /// One tick in his room.
    #[inline(never)]
    pub fn tick(hero_x: i32, geo: &mut crate::geo::World) {
        let king = unsafe { &mut *(&raw mut KING) };
        let at_cap = geo.active() >= crate::geo::MAX_COINS;
        if let Some(n) = king.step(&mut Store, hero_x, at_cap) {
            match REWARDS[n - 1] {
                Reward::Geo(value, y) => {
                    let fling = crate::geo::Fling { speed: REWARD_SPEED, angle: REWARD_ANGLE, spread: [0, 0] };
                    let counts = coins(value, (n as u32).wrapping_mul(0x9e37_79b9));
                    geo.spawn_chest(GRUBFATHER_SCENE, 0x4000_0000 | n as u32, [REWARD_X, y], fling, counts);
                }
                // The heart piece is taken on touch in the source, the shinies on a kneel: their sounds.
                Reward::Item(grant, name) => crate::grant_item(grant, name, grant == Grant::MaskShard),
            }
            crate::persist::publish();
            unsafe { HK_GRUB_REWARDS_PAID += 1 };
        }
        unsafe { HK_GRUBFATHER_PHASE = u32::from(king.state) };
    }
}
#[cfg(not(test))]
pub use live::*;

#[cfg(test)]
mod tests {
    use super::*;
    struct Pd {
        grubs: i32,
        rewards: i32,
        last: bool,
    }
    impl Player for Pd {
        fn grubs(&self) -> i32 {
            self.grubs
        }
        fn rewards(&self) -> i32 {
            self.rewards
        }
        fn set_rewards(&mut self, value: i32) {
            self.rewards = value;
        }
        fn set_final(&mut self) {
            self.last = true;
        }
    }
    const NEAR: i32 = 36 << 16;
    const FAR: i32 = 10 << 16;
    /// Ticks until the next reward, or None within `limit`.
    fn until_paid(k: &mut King, pd: &mut Pd, x: i32, limit: u32) -> Option<(u32, usize)> {
        (1..=limit).find_map(|t| k.step(pd, x, false).map(|n| (t, n)))
    }
    #[test]
    fn no_grub_cries_and_every_grub_paid_idles() {
        let mut pd = Pd { grubs: 0, rewards: 0, last: false };
        let mut k = King::new();
        k.step(&mut pd, NEAR, false);
        assert_eq!(k.state, ALONE);
        pd = Pd { grubs: 3, rewards: 3, last: false };
        k = King::new();
        k.step(&mut pd, NEAR, false);
        assert_eq!(k.state, ALL_GIVEN);
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000), None);
    }
    #[test]
    fn he_beckons_until_the_knight_comes_then_pays_each_grub_in_turn() {
        let mut pd = Pd { grubs: 2, rewards: 0, last: false };
        let mut k = King::new();
        assert_eq!(until_paid(&mut k, &mut pd, FAR, 1000), None);
        assert_eq!(k.state, BECKON);
        // Pause is spent; Cheer, Cheer Pause, then the Gift trigger.
        let first = CHEER_TICKS + CHEER_PAUSE_TICKS + GIFT_TRIGGER_TICKS + 1;
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000), Some((first as u32, 1)));
        assert_eq!(pd.rewards, 1);
        // The rest of Gift, Recover Pause, Recheck, then the next Gift: the
        // Knight walking away no longer matters.
        let next = GIFT_REST_TICKS + RECOVER_PAUSE_TICKS + GIFT_TRIGGER_TICKS;
        assert_eq!(until_paid(&mut k, &mut pd, FAR, 1000), Some((next as u32, 2)));
        assert_eq!(until_paid(&mut k, &mut pd, FAR, 1000), None);
        assert_eq!(k.state, ALL_GIVEN);
        assert!(!pd.last);
    }
    #[test]
    fn the_cheer_is_once_a_scene_load() {
        let mut pd = Pd { grubs: 1, rewards: 0, last: false };
        let mut k = King::new();
        until_paid(&mut k, &mut pd, NEAR, 1000).unwrap();
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000), None);
        assert_eq!(k.state, ALL_GIVEN);
        // Another grub freed once he idles is not seen until the next load.
        pd.grubs = 2;
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000), None);
        k = King { state: PAUSE, ticks: 0, given: k.given };
        let quick = CHEER_PAUSE_TICKS + GIFT_TRIGGER_TICKS + 1;
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000), Some((quick as u32 + 1, 2)));
    }
    #[test]
    fn a_full_coin_pool_holds_the_next_gift() {
        let mut pd = Pd { grubs: 3, rewards: 0, last: false };
        let mut k = King::new();
        until_paid(&mut k, &mut pd, NEAR, 1000).unwrap();
        for _ in 0..1000 {
            assert_eq!(k.step(&mut pd, NEAR, true), None);
        }
        assert_eq!(k.state, RECOVER_PAUSE);
        assert!(until_paid(&mut k, &mut pd, NEAR, 1000).is_some());
    }
    #[test]
    fn the_last_reward_sets_the_final_flag_and_stops() {
        let n = REWARDS.len() as i32;
        let mut pd = Pd { grubs: n, rewards: n - 1, last: false };
        let mut k = King::new();
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000).map(|p| p.1), Some(REWARDS.len()));
        assert!(pd.last);
        assert_eq!(k.state, DONE);
        assert_eq!(until_paid(&mut k, &mut pd, NEAR, 1000), None);
    }
    #[test]
    fn the_rewards_are_the_originals() {
        assert_eq!(REWARDS.len(), 46);
        assert_eq!(REWARDS[0], Reward::Geo(10, REWARDS_Y0));
        assert_eq!(REWARDS[4], Reward::Item(Grant::MaskShard, ""));
        assert_eq!(REWARDS[9], Reward::Item(Grant::Charm(3), "Grubsong"));
        assert_eq!(REWARDS[15], Reward::Item(Grant::RancidEgg, "Rancid Egg"));
        assert_eq!(REWARDS[22], Reward::Item(Grant::Trinket(2), "Hallownest Seal"));
        assert_eq!(REWARDS[30], Reward::Item(Grant::Trinket(14), "Pale Ore"));
        assert_eq!(REWARDS[37], Reward::Item(Grant::Trinket(3), "King's Idol"));
        assert_eq!(REWARDS[45], Reward::Item(Grant::Charm(35), "Grubberfly's Elegy"));
    }
    const REWARDS_Y0: i32 = match REWARDS[0] {
        Reward::Geo(_, y) => y,
        _ => 0,
    };
    #[test]
    fn a_geo_reward_pays_its_geo_exactly() {
        for (i, r) in REWARDS.iter().enumerate() {
            if let Reward::Geo(value, _) = *r {
                let c = coins(value, (i as u32 + 1).wrapping_mul(0x9e37_79b9));
                let sum: u32 = c.iter().zip(COIN_VALUES).map(|(&n, v)| n as u32 * v as u32).sum();
                assert_eq!(sum, value as u32);
                // Every draw is one coin, so a reward never asks for more than its Geo in coins.
                assert!(c.iter().sum::<u16>() <= value);
            }
        }
    }
}
