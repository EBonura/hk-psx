//! Hardware-independent six-stem residency and source cue lifecycle.
pub const STEMS: usize = 6;
pub const ALL: u8 = (1 << STEMS) - 1;

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
        }
    }
    pub fn finish(&mut self) -> bool {
        self.ready = self.loaded == ALL;
        self.ready
    }
    /// Returns newly started stems. Shared stems and repeated grid cues never restart.
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
        let start = self.enabled & !self.playing;
        self.playing |= start;
        start
    }
    /// Returns stems stopped after the transition, matching ApplyAtmosCue's wait.
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
