//! Continuous ADPCM ring for the fixed 743-pitch cave ambience voice.
//!
//! PSX-SPX: https://psx-spx.consoledev.net/soundprocessingunitspu/
//! ENDX is read-only. Playback boundaries instead use the SPU IRQ-address
//! latch, polled with CPU IRQ9 masked. DMA also triggers that latch: disable
//! IRQ detection throughout refill, then arm the next playback boundary.
//! This module exclusively owns SPU IRQ address/control while running.
//! No IRQ handler, other voice, reverb buffer or unrelated DMA may use this
//! ring. Checkpoints must run independently of simulation pause/catch-up.

// Both taken from the cooked bank so the guest cannot drift from the payload
// the host actually wrote; the cave bed's rate is not the other loops'.
pub const SOURCE_BYTES: usize = super::AMBIENCE_STREAM_SOURCE_BYTES;
pub const HALF_BYTES: usize = 8_192;
pub const RING_BYTES: usize = HALF_BYTES * 2;
pub const VOICE: u8 = super::AMBIENCE_CLIPS[super::STREAM_INDEX].voice;
pub const PITCH: u16 = super::AMBIENCE_CLIPS[super::STREAM_INDEX].pitch;
// 512 blocks * 28 samples at 44100*PITCH/4096 Hz per half.
// PAL and NTSC give about 89 and 107 VBlanks. Eight missed checkpoints
// invalidate synchronization; no attempt is made to guess a current cursor.
pub const MAX_POLL_GAP: u32 = 8;
/// One half is 512 blocks of 28 samples; how many VBlanks that lasts depends on
/// the cooked pitch, so the sanity window around each boundary is derived from
/// it rather than fixed. At the 8 kHz pitch this is the 107 ticks the previous
/// literal 80 and 120 bracketed.
pub const HALF_SAMPLES: u64 = (HALF_BYTES as u64 / 16) * 28;
pub const BOUNDARY_TICKS: u32 = (HALF_SAMPLES * 60 * 4096 / (44_100 * PITCH as u64)) as u32;
pub const MIN_BOUNDARY_TICKS: u32 = BOUNDARY_TICKS * 3 / 4;
pub const MAX_BOUNDARY_TICKS: u32 = BOUNDARY_TICKS * 9 / 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Idle,
    Refilled,
    Fault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    Idle,
    Refill(usize),
    Fault,
}

pub struct State {
    running: bool,
    pending: bool,
    active: usize,
    last_poll: u32,
    boundary: u32,
    pub max_gap: u32,
}
impl State {
    pub const fn new() -> Self {
        Self {
            running: false,
            pending: false,
            active: 0,
            last_poll: 0,
            boundary: 0,
            max_gap: 0,
        }
    }
    pub fn start(&mut self, now: u32) {
        self.running = true;
        self.pending = false;
        self.active = 0;
        self.last_poll = now;
        self.boundary = now;
    }
    pub fn stop(&mut self) {
        self.running = false;
        self.pending = false;
    }
    pub fn poll(&mut self, now: u32, irq: bool) -> Poll {
        if !self.running {
            return Poll::Idle;
        }
        let gap = now.wrapping_sub(self.last_poll);
        self.max_gap = self.max_gap.max(gap);
        let age = now.wrapping_sub(self.boundary);
        if self.pending
            || gap >= MAX_POLL_GAP
            || age > MAX_BOUNDARY_TICKS
            || (irq && age < MIN_BOUNDARY_TICKS)
        {
            self.stop();
            return Poll::Fault;
        }
        self.last_poll = now;
        if !irq {
            return Poll::Idle;
        }
        let inactive = self.active;
        self.active ^= 1;
        self.boundary = now;
        self.pending = true;
        Poll::Refill(inactive)
    }
    /// Only after synchronous DMA completion, before arming its next boundary.
    pub fn complete(&mut self, now: u32) -> bool {
        let gap = now.wrapping_sub(self.last_poll);
        self.max_gap = self.max_gap.max(gap);
        if !self.running || !self.pending || gap >= MAX_POLL_GAP {
            self.stop();
            return false;
        }
        self.pending = false;
        self.last_poll = now;
        true
    }
}

/// Copy unchanged ADPCM headers/payload in source order, wrapping at the true
/// source end. Only the transport byte changes: A starts the permanent ring,
/// B ends/repeats it. Source wrap does not key-on/reset predictor history.
/// The caller validates original flags/header/checksum once at boot.
pub fn fill_half(
    source: &[u8],
    cursor: &mut usize,
    half: usize,
    out: &mut [u8; HALF_BYTES],
) -> Option<u32> {
    if source.is_empty()
        || source.len() % 16 != 0
        || *cursor >= source.len()
        || *cursor % 16 != 0
        || half > 1
    {
        return None;
    }
    let mut done = 0;
    let mut wraps = 0;
    while done < HALF_BYTES {
        let count = (source.len() - *cursor).min(HALF_BYTES - done);
        out[done..done + count].copy_from_slice(&source[*cursor..*cursor + count]);
        done += count;
        *cursor += count;
        if *cursor == source.len() {
            *cursor = 0;
            wraps += 1;
        }
    }
    for block in (0..HALF_BYTES).step_by(16) {
        out[block + 1] = if half == 0 && block == 0 {
            4
        } else if half == 1 && block == HALF_BYTES - 16 {
            3
        } else {
            0
        };
    }
    Some(wraps)
}

#[cfg(target_arch = "mips")]
mod hardware {
    use super::*;
    use psx_spu::{Pitch, SpuAddr, Voice};
    const CONTROL: u32 = 0x1f80_1daa;
    const STATUS: u32 = 0x1f80_1dae;
    const IRQ_ADDRESS: u32 = 0x1f80_1da4;
    const IRQ_ENABLE: u16 = 1 << 6;
    const IRQ_MASK: u32 = 1 << psx_io::irq::source::SPU;
    #[repr(C, align(4))]
    struct Scratch([u8; HALF_BYTES]);
    static mut SCRATCH: Scratch = Scratch([0; HALF_BYTES]);
    static mut STATE: State = State::new();
    static mut CURSOR: usize = 0;
    static mut BASE: u32 = 0;
    static mut FIRST_ARM_TICK: u32 = 0;
    static mut ARMED: bool = false;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_STARTS: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_REFILLS: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_SOURCE_WRAPS: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_MAX_SERVICE_GAP: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_UNDERRUNS: u32 = 0;

    fn disable_irq() {
        unsafe {
            psx_io::write16(CONTROL, psx_io::read16(CONTROL) & !IRQ_ENABLE);
        }
        psx_io::irq::ack(IRQ_MASK);
    }
    fn arm(address: u32) {
        unsafe {
            psx_io::write16(IRQ_ADDRESS, (address >> 3) as u16);
            psx_io::write16(CONTROL, psx_io::read16(CONTROL) | IRQ_ENABLE);
        }
    }
    fn upload(source: &[u8], half: usize) -> bool {
        unsafe {
            let Some(wraps) = fill_half(
                source,
                &mut *(&raw mut CURSOR),
                half,
                &mut (*(&raw mut SCRATCH)).0,
            ) else {
                return false;
            };
            HK_AUDIO_STREAM_SOURCE_WRAPS = HK_AUDIO_STREAM_SOURCE_WRAPS.wrapping_add(wraps);
            psx_spu::upload_adpcm(
                SpuAddr::new(BASE + (half * HALF_BYTES) as u32),
                &(*(&raw const SCRATCH)).0,
            );
        }
        true
    }
    /// Main-thread only; no nested service. Root configures ADSR/volume before
    /// calling. Owns voice9 KON/pitch/start/loop: exclude it from generic KON.
    /// `source` is the immutable, checksum-validated boot cache.
    pub fn start(source: &[u8], base: u32, _now: u32) -> bool {
        stop();
        if source.len() != SOURCE_BYTES
            || base < 0x1010
            || base & 15 != 0
            || base > 0x80000 - RING_BYTES as u32
            || psx_io::irq::mask() & IRQ_MASK != 0
        {
            return false;
        }
        unsafe {
            BASE = base;
            CURSOR = 0;
        }
        if !upload(source, 0) || !upload(source, 1) {
            return false;
        }
        let voice = Voice::new(VOICE);
        voice.set_start_addr(SpuAddr::new(base));
        voice.set_loop_addr(SpuAddr::new(base));
        voice.set_pitch(Pitch::raw(PITCH));
        Voice::key_on(1 << VOICE);
        // Even a keyed-off voice fetches blocks, and KON takes effect at an
        // SPU sample boundary. Service waits two VBlank transitions before
        // arming B, well before playback can reach B (~89 PAL ticks).
        let now = psx_rt::interrupts::vblank_count();
        unsafe {
            (*(&raw mut STATE)).start(now);
            FIRST_ARM_TICK = now;
            HK_AUDIO_STREAM_STARTS = HK_AUDIO_STREAM_STARTS.wrapping_add(1);
        }
        true
    }
    pub fn stop() {
        disable_irq();
        Voice::new(VOICE).set_pitch(Pitch::raw(0));
        Voice::key_off(1 << VOICE);
        unsafe {
            (*(&raw mut STATE)).stop();
            ARMED = false;
        }
    }
    fn fault() -> Service {
        stop();
        unsafe {
            HK_AUDIO_STREAM_UNDERRUNS = HK_AUDIO_STREAM_UNDERRUNS.wrapping_add(1);
        }
        Service::Fault
    }
    pub fn service(source: &[u8], now: u32) -> Service {
        let action = unsafe {
            // Checkpoints can run many times per VBlank. Only one hardware
            // status read per tick is needed for a 1.79-second half-buffer.
            if !(*(&raw const STATE)).running || now == (*(&raw const STATE)).last_poll {
                return Service::Idle;
            }
            let irq = ARMED && psx_io::read16(STATUS) & IRQ_ENABLE != 0;
            let action = (*(&raw mut STATE)).poll(now, irq);
            HK_AUDIO_STREAM_MAX_SERVICE_GAP = (*(&raw const STATE)).max_gap;
            if action == Poll::Idle
                && (*(&raw const STATE)).running
                && !ARMED
                && now.wrapping_sub(FIRST_ARM_TICK) >= 2
            {
                arm(BASE + HALF_BYTES as u32);
                ARMED = true;
            }
            action
        };
        match action {
            Poll::Idle => Service::Idle,
            Poll::Fault => fault(),
            Poll::Refill(half) => {
                disable_irq();
                if source.len() != SOURCE_BYTES || !upload(source, half) {
                    return fault();
                }
                let now = psx_rt::interrupts::vblank_count();
                unsafe {
                    if !(*(&raw mut STATE)).complete(now) {
                        return fault();
                    }
                    HK_AUDIO_STREAM_MAX_SERVICE_GAP = (*(&raw const STATE)).max_gap;
                    HK_AUDIO_STREAM_REFILLS = HK_AUDIO_STREAM_REFILLS.wrapping_add(1);
                    arm(BASE + (half * HALF_BYTES) as u32);
                }
                Service::Refilled
            }
        }
    }
}
#[cfg(target_arch = "mips")]
pub use hardware::{service, start, stop};
