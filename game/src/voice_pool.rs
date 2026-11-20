//! A few SPU voices shared by sounds of different weight, with the lightest one cut first.
//!
//! The port gives most kinds of sound a voice of its own. The room one-shots (a scene bank's
//! clips: the bosses', the enemies', the world's) all used to share one, so a new one cut
//! whatever was still sounding there: in the False Knight fight 53 of 88 starts cut a sound
//! (hk-sound, 2026-11-20). The pool hands a new sound a voice that has finished, and only when
//! every voice is still sounding cuts the one that matters least (the oldest of those),
//! never one that outranks the new sound: a rustle never cuts a roar, it is dropped instead.
//!
//! Pure bookkeeping on a clock the caller supplies (simulation ticks); the caller keys the voice.
//! No floats, no 64-bit arithmetic.

/// How much a sound matters when the pool is full, lowest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Weight {
    /// Grass, drips, an enemy's footsteps: frequent, and the first to go.
    Ambient = 0,
    /// The world answering the Knight: breakables, benches, pickups, a nail tink.
    World = 1,
    /// Enemies' calls, attacks and deaths.
    Enemy = 2,
    /// Bosses and their arenas, and the Knight's own hard landing.
    Boss = 3,
}

/// A held sound (a loop) never finishes by itself: only `release` frees its voice.
pub const HELD: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Slot {
    /// A sound was handed this voice and has not been released.
    live: bool,
    /// Clock at which the sound ends; `HELD` for a loop.
    until: u32,
    started: u32,
    weight: Weight,
    /// Changes on every claim, so a loop can tell whether it still has its voice.
    claim: u32,
}

/// The pool over `N` voices.
pub struct VoicePool<const N: usize> {
    voices: [u8; N],
    slots: [Slot; N],
    claims: u32,
}

/// A voice handed out by [`VoicePool::claim`]: which SPU voice, and the claim it was handed out
/// under ([`VoicePool::holds`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub voice: u8,
    pub claim: u32,
    /// The voice was taken from a sound still playing.
    pub cut: bool,
}

impl<const N: usize> VoicePool<N> {
    /// A pool over `voices`, all of them free.
    pub const fn new(voices: [u8; N]) -> Self {
        Self {
            voices,
            slots: [Slot { live: false, until: 0, started: 0, weight: Weight::Ambient, claim: 0 }; N],
            claims: 0,
        }
    }

    /// The SPU voices the pool hands out.
    pub const fn voices(&self) -> &[u8; N] {
        &self.voices
    }

    /// A voice for a sound of `weight` that starts at `now` and lasts `length` clock ticks
    /// (`HELD` for a loop): a voice whose sound has ended, else the lightest one sounding (the
    /// oldest of equals) when it does not outweigh this one. `None` when every voice carries a
    /// heavier sound: the new sound is not played.
    pub fn claim(&mut self, now: u32, weight: Weight, length: u32) -> Option<Claim> {
        let finished =
            |s: &Slot| !s.live || (s.until != HELD && now.wrapping_sub(s.until) as i32 >= 0);
        let (index, cut) = match self.slots.iter().position(finished) {
            Some(free) => (free, false),
            None => {
                let (index, slot) = self
                    .slots
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, s)| (s.weight, core::cmp::Reverse(now.wrapping_sub(s.started))))?;
                if slot.weight > weight {
                    return None;
                }
                (index, true)
            }
        };
        self.claims = self.claims.wrapping_add(1);
        let until = if length == HELD { HELD } else { now.wrapping_add(length.max(1)) };
        self.slots[index] = Slot { live: true, until, started: now, weight, claim: self.claims };
        Some(Claim { voice: self.voices[index], claim: self.claims, cut })
    }

    /// Whether `claim` still owns its voice (nothing claimed it since).
    pub fn holds(&self, claim: Claim) -> bool {
        self.voices
            .iter()
            .zip(&self.slots)
            .any(|(&v, s)| v == claim.voice && s.claim == claim.claim)
    }

    /// The sound on `claim`'s voice has stopped (a loop's source went away): the voice is free.
    /// Nothing happens when another claim has taken the voice since.
    pub fn release(&mut self, claim: Claim) {
        for (v, s) in self.voices.iter().zip(self.slots.iter_mut()) {
            if *v == claim.voice && s.claim == claim.claim {
                s.live = false;
            }
        }
    }

    /// Every voice is free (the bank the sounds played from is gone). Returns the voices that
    /// were still sounding, as a key-off mask.
    pub fn release_all(&mut self, now: u32) -> u32 {
        let mut mask = 0;
        for (v, s) in self.voices.iter().zip(self.slots.iter_mut()) {
            if s.live && (s.until == HELD || (now.wrapping_sub(s.until) as i32) < 0) {
                mask |= 1 << v;
            }
            s.live = false;
        }
        mask
    }
}

/// Clock ticks (60 a second) a one-shot of `bytes` of SPU ADPCM lasts at
/// pitch register `pitch` (4096 is 44,100 samples a second), rounded up.
pub fn clip_ticks(bytes: u32, pitch: u16) -> u32 {
    let samples = bytes / 16 * 28;
    let pitch = u32::from(pitch.max(1));
    // samples * 4096 stays below 2^32 for any clip SPU RAM can hold (512 KiB is 917,504 samples
    // at most: 3.8e9); dividing by the pitch before the 60 / 44,100 keeps the rest in range.
    let output = (samples.min(1_000_000) * 4096).div_ceil(pitch);
    (output * 60).div_ceil(44_100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_voice_is_taken_before_any_is_cut() {
        let mut pool = VoicePool::new([11, 13]);
        let a = pool.claim(0, Weight::Boss, 100).unwrap();
        let b = pool.claim(10, Weight::Ambient, 5).unwrap();
        assert_eq!((a.voice, b.voice), (11, 13));
        // At 15 the rustle has finished: the next sound takes its voice, the roar plays on.
        let c = pool.claim(15, Weight::Ambient, 5).unwrap();
        assert_eq!(c.voice, 13);
        assert!(pool.holds(a));
    }

    #[test]
    fn a_full_pool_cuts_the_lightest_oldest_and_never_a_heavier_sound() {
        let mut pool = VoicePool::new([11, 13, 14]);
        let roar = pool.claim(0, Weight::Boss, 600).unwrap();
        let old = pool.claim(1, Weight::Enemy, 600).unwrap();
        let young = pool.claim(2, Weight::Enemy, 600).unwrap();
        // A rustle finds every voice heavier than itself: dropped, nothing cut.
        assert_eq!(pool.claim(3, Weight::Ambient, 10), None);
        assert!(!roar.cut);
        assert!(pool.holds(roar) && pool.holds(old) && pool.holds(young));
        // An enemy call cuts the oldest enemy sound, not the boss's.
        let call = pool.claim(4, Weight::Enemy, 10).unwrap();
        assert_eq!((call.voice, call.cut), (old.voice, true));
        assert!(!pool.holds(old) && pool.holds(roar) && pool.holds(young));
        // A boss sound cuts the lightest (the younger enemy sound is now the oldest of them).
        assert_eq!(pool.claim(5, Weight::Boss, 10).unwrap().voice, young.voice);
    }

    #[test]
    fn a_loop_holds_its_voice_until_released_and_knows_when_it_was_cut() {
        let mut pool = VoicePool::new([11]);
        let hum = pool.claim(0, Weight::World, HELD).unwrap();
        assert_eq!(pool.claim(100_000, Weight::Ambient, 1), None);
        let hit = pool.claim(100_001, Weight::Enemy, 1).unwrap();
        assert!(!pool.holds(hum));
        pool.release(hum); // stale: changes nothing
        assert!(pool.holds(hit));
        pool.release(hit);
        assert!(pool.claim(100_002, Weight::Ambient, 1).is_some());
    }

    #[test]
    fn release_all_reports_what_was_still_sounding() {
        let mut pool = VoicePool::new([11, 13, 16]);
        pool.claim(0, Weight::World, 10).unwrap();
        pool.claim(0, Weight::World, HELD).unwrap();
        assert_eq!(pool.release_all(20), 1 << 13);
        assert_eq!(pool.claim(20, Weight::Ambient, 1).unwrap().voice, 11);
    }

    #[test]
    fn the_clock_may_wrap() {
        let mut pool = VoicePool::new([11]);
        pool.claim(u32::MAX - 5, Weight::Enemy, 10).unwrap();
        assert_eq!(pool.claim(2, Weight::Ambient, 1), None);
        assert!(pool.claim(5, Weight::Ambient, 1).is_some());
    }

    #[test]
    fn clip_ticks_follow_bytes_and_pitch() {
        // 3,520 bytes at 22,050 Hz (pitch 2048): 6,160 samples, 0.279 s, 17 ticks.
        assert_eq!(clip_ticks(3520, 2048), 17);
        // The same clip an octave down lasts twice as long.
        assert_eq!(clip_ticks(3520, 1024), 34);
        assert_eq!(clip_ticks(0, 0), 0);
    }
}
