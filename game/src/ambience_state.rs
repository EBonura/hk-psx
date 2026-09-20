//! Hardware-independent stem residency, source cue lifecycle and voice pool.
//!
//! How many stems there are, which SPU voice the streamed one keeps and which
//! voices the rest may take are all read from the cooked manifest, so widening
//! the resident channel set is a cook run rather than an edit here.
#[path = "../../data/ambience.rs"]
pub mod data;
use data::{AMBIENCE_CLIPS, AMBIENCE_POOL_VOICES, AMBIENCE_STREAM_VOICE};

/// One stem per resident atmos channel, as many as the cook wrote.
pub const STEMS: usize = AMBIENCE_CLIPS.len();
// Every cue mask, and every mask this module returns, is one bit per stem.
const _: () = assert!(STEMS > 0 && STEMS <= 8, "a cue mask is a u8");
pub const ALL: u8 = ((1u16 << STEMS) - 1) as u8;

/// The one clip held in SPU as a ring refilled from RAM rather than whole.
/// Derived from the cooked table rather than written down, so changing which
/// source channels are resident cannot silently point the stream at another
/// loop's pitch and RAM buffer.
pub const STREAM_INDEX: usize = {
    let mut found = usize::MAX;
    let mut i = 0;
    while i < AMBIENCE_CLIPS.len() {
        if AMBIENCE_CLIPS[i].spu_bytes != AMBIENCE_CLIPS[i].byte_len {
            assert!(found == usize::MAX, "more than one streamed ambience clip");
            found = i;
        }
        i += 1;
    }
    assert!(found != usize::MAX, "no streamed ambience clip");
    found
};

/// Voices a stem takes when it keys on and gives back once it has finished
/// fading out, which is what lets more loops be resident than ambience owns
/// voices. The streamed stem is never one of them: audio_stream.rs owns the
/// single SPU IRQ address register while it runs, so its voice is its own.
pub const POOL: usize = AMBIENCE_POOL_VOICES.len();
const _: () = assert!(POOL > 0 && POOL <= 8, "one bit per pooled voice in a u8");
/// A stem that is not playing drives no SPU register at all.
pub const NO_VOICE: u8 = u8::MAX;

/// Every voice ambience drives, for the paths that silence the whole bank.
pub const VOICE_MASK: u32 = {
    let mut mask = 1u32 << AMBIENCE_STREAM_VOICE;
    let mut i = 0;
    while i < POOL {
        let bit = 1u32 << AMBIENCE_POOL_VOICES[i];
        assert!(mask & bit == 0, "a pooled voice repeats or is the stream's");
        mask |= bit;
        i += 1;
    }
    mask
};

#[derive(Clone, Copy)]
pub struct Mixer {
    pub loaded: u8,
    pub ready: bool,
    pub scene: u8,
    pub playing: u8,
    pub gains: [i16; STEMS],
    from: [i16; STEMS],
    target: [i16; STEMS],
    enabled: u8,
    duration: u16,
    pub elapsed: u16,
    pub transitioning: bool,
    /// The voice each playing stem holds, NO_VOICE when it holds none.
    held: [u8; STEMS],
    /// Which pool entries are out, one bit per AMBIENCE_POOL_VOICES index.
    taken: u8,
    /// Where the next search starts, so a voice just given back is the last
    /// one handed out again rather than the first.
    cursor: usize,
    /// Stems a cue could not key on because every pooled voice was held.
    pub denied: u16,
}
impl Mixer {
    pub const fn new() -> Self {
        Self {
            loaded: 0,
            ready: false,
            scene: u8::MAX,
            playing: 0,
            gains: [0; STEMS],
            from: [0; STEMS],
            target: [0; STEMS],
            enabled: 0,
            duration: 0,
            elapsed: 0,
            transitioning: false,
            held: [NO_VOICE; STEMS],
            taken: 0,
            cursor: 0,
            denied: 0,
        }
    }
    pub fn finish(&mut self) -> bool {
        self.ready = self.loaded == ALL;
        self.ready
    }
    /// The SPU voice a stem is playing on, or NO_VOICE.
    pub fn voice(&self, stem: usize) -> u8 {
        self.held[stem]
    }
    /// Which voices the bank is driving right now, for the published telemetry.
    pub fn voice_mask(&self) -> u32 {
        let mut mask = 0;
        for stem in 0..STEMS {
            if self.held[stem] != NO_VOICE {
                mask |= 1 << self.held[stem] as u32;
            }
        }
        mask
    }
    fn take(&mut self) -> u8 {
        for step in 0..POOL {
            let index = (self.cursor + step) % POOL;
            if self.taken & (1 << index) == 0 {
                self.taken |= 1 << index;
                self.cursor = (index + 1) % POOL;
                return AMBIENCE_POOL_VOICES[index];
            }
        }
        NO_VOICE
    }
    /// Take every pooled voice, for the tests that need the pool dry. A cue
    /// cannot reach this state while the resident set is no longer than the
    /// pool plus the streamed stem, which is why it has to be reached here.
    #[cfg(test)]
    pub fn exhaust_pool(&mut self) -> usize {
        let mut count = 0;
        while self.take() != NO_VOICE {
            count += 1;
        }
        count
    }
    /// Returns newly started stems. Shared stems and repeated grid cues never
    /// restart, and a stem the pool could not serve is left unplayed rather
    /// than taking a voice another stem is still fading out on.
    pub fn cue(&mut self, scene: u8, enabled: u8, gains: [i16; STEMS], ticks: u16) -> u8 {
        if !self.ready || self.scene == scene {
            return 0;
        }
        self.scene = scene;
        self.enabled = enabled & ALL;
        self.from = self.gains;
        self.target = gains;
        self.duration = ticks;
        self.elapsed = 0;
        self.transitioning = true;
        let mut start = 0;
        for stem in 0..STEMS {
            if self.enabled & !self.playing & (1 << stem) == 0 {
                continue;
            }
            let voice = if stem == STREAM_INDEX {
                AMBIENCE_STREAM_VOICE
            } else {
                self.take()
            };
            if voice == NO_VOICE {
                // Every pooled voice belongs to a stem that has not finished
                // fading out. This stem stays out of `playing`, so the next cue
                // that enables it tries again rather than losing it for good.
                self.denied = self.denied.saturating_add(1);
                continue;
            }
            self.held[stem] = voice;
            start |= 1 << stem;
        }
        self.playing |= start;
        start
    }
    /// Returns stems stopped after the transition, matching ApplyAtmosCue's
    /// wait. Their voices are still theirs until `release`.
    pub fn tick(&mut self) -> u8 {
        if !self.ready || !self.transitioning {
            return 0;
        }
        if self.elapsed < self.duration {
            self.elapsed += 1;
        }
        for i in 0..STEMS {
            // Source endpoints/duration; fixed linear SPU-volume interpolation.
            // Unity AudioMixer's native interpolation curve is not reproduced.
            self.gains[i] = if self.duration == 0 {
                self.target[i]
            } else {
                (self.from[i] as i32
                    + (self.target[i] as i32 - self.from[i] as i32) * self.elapsed as i32
                        / self.duration as i32) as i16
            };
        }
        if self.elapsed == self.duration {
            self.transitioning = false;
            let stop = self.playing & !self.enabled;
            self.playing &= self.enabled;
            stop
        } else {
            0
        }
    }
    /// Give back the voices of stems the caller has just keyed off. Separate
    /// from `tick` so a voice cannot be handed to another stem before the SPU
    /// has been told the stem that held it stopped.
    pub fn release(&mut self, stems: u8) {
        for stem in 0..STEMS {
            if stems & (1 << stem) == 0 || self.held[stem] == NO_VOICE {
                continue;
            }
            for index in 0..POOL {
                if AMBIENCE_POOL_VOICES[index] == self.held[stem] {
                    self.taken &= !(1 << index);
                }
            }
            self.held[stem] = NO_VOICE;
        }
    }
}

/// No upload can publish residency until every byte and transport flag agrees.
pub fn valid_clip(bytes: &[u8], length: usize, checksum: u32) -> bool {
    if bytes.len() != length || length == 0 || length % 16 != 0 || bytes[0] >> 4 != 0 {
        return false;
    }
    let mut hash = 0x811c9dc5u32;
    for (index, &byte) in bytes.iter().enumerate() {
        if index % 16 == 0 && (byte >> 4 > 4 || byte & 15 > 12) {
            return false;
        }
        if index % 16 == 1 {
            let expected =
                if index == 1 { 4 } else { 0 } | if index == length - 15 { 3 } else { 0 };
            if byte != expected {
                return false;
            }
        }
        hash = (hash ^ byte as u32).wrapping_mul(0x01000193);
    }
    hash == checksum
}
