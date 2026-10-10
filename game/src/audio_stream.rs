//! Continuous ADPCM ring for the area music voice, fed from a main-RAM FIFO
//! that the CD refills between room loads.
//!
//! PSX-SPX: https://psx-spx.consoledev.net/soundprocessingunitspu/
//! ENDX is read-only. Playback boundaries instead use the SPU IRQ-address
//! latch, polled with CPU IRQ9 masked. DMA also triggers that latch: disable
//! IRQ detection throughout refill, then arm the next playback boundary.
//! This module exclusively owns SPU IRQ address/control while running.
//! No IRQ handler, other voice, reverb buffer or unrelated DMA may use this
//! ring. Checkpoints must run independently of simulation pause/catch-up.
//!
//! The FIFO holds whole 2,048-byte sectors, so a CD read lands in it directly
//! and each SPU half is four of its sectors, uploaded straight from it once
//! the ring's transport flags are written into the block headers. It used to
//! hold cave_noises whole, looping in place; that loop is SPU-resident now.

pub const PITCH: u16 = super::MUSIC_PITCH;
pub const SECTOR: usize = 2048;
pub const HALF_BYTES: usize = 8_192;
pub const RING_BYTES: usize = HALF_BYTES * 2;
pub const HALF_SECTORS: usize = HALF_BYTES / SECTOR;
/// 98,304 bytes: 7.8 s at 22,050 Hz mono, longer than any room load measured.
pub const FIFO_SECTORS: usize = 48;
pub const FIFO_BYTES: usize = FIFO_SECTORS * SECTOR;
/// What one gameplay refill reads: 2.6 s of music for one seek.
pub const CHUNK_SECTORS: usize = 16;
const _: () = assert!(FIFO_SECTORS % HALF_SECTORS == 0 && CHUNK_SECTORS <= FIFO_SECTORS);
pub const VOICE: u8 = super::MUSIC_VOICE;
// 512 blocks * 28 samples at 44100*PITCH/4096 Hz per half.
// Eight missed checkpoints invalidate synchronization; no attempt is made to
// guess a current cursor.
pub const MAX_POLL_GAP: u32 = 8;
/// One half is 512 blocks of 28 samples; how many VBlanks that lasts depends on
/// the cooked pitch, so the sanity window around each boundary is derived from
/// it rather than fixed.
pub const HALF_SAMPLES: u64 = (HALF_BYTES as u64 / 16) * 28;
pub const BOUNDARY_TICKS: u32 = (HALF_SAMPLES * 60 * 4096 / (44_100 * PITCH as u64)) as u32;
/// The window around each boundary is a quarter/eighth of a half, or a poll
/// gap either side, whichever is wider: at 22,050 Hz a half is only 39 ticks,
/// and an eighth of that would fault a boundary seen five checkpoints late.
const SLACK: u32 = MAX_POLL_GAP + 2;
pub const MIN_BOUNDARY_TICKS: u32 = {
    let quarter = BOUNDARY_TICKS * 3 / 4;
    let slack = BOUNDARY_TICKS.saturating_sub(SLACK);
    if quarter < slack {
        quarter
    } else {
        slack
    }
};
pub const MAX_BOUNDARY_TICKS: u32 = {
    let eighth = BOUNDARY_TICKS * 9 / 8;
    let slack = BOUNDARY_TICKS + SLACK;
    if eighth > slack {
        eighth
    } else {
        slack
    }
};
const _: () = assert!(
    MIN_BOUNDARY_TICKS < BOUNDARY_TICKS * 5 / 6,
    "a PAL half must not look early"
);

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
    pub fn running(&self) -> bool {
        self.running
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

/// Sector bookkeeping for the RAM FIFO. The CD writes whole sectors at
/// `write`; the ring takes whole halves at `read`. Both wrap at FIFO_SECTORS,
/// which a half divides, so a half never straddles the wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fifo {
    pub filled: usize,
    pub read: usize,
    pub write: usize,
}
impl Fifo {
    pub const fn new() -> Self {
        Self {
            filled: 0,
            read: 0,
            write: 0,
        }
    }
    pub fn free(&self) -> usize {
        FIFO_SECTORS - self.filled
    }
    /// How many sectors one read may write, contiguously, at `write`.
    pub fn writable(&self, most: usize) -> usize {
        most.min(self.free()).min(FIFO_SECTORS - self.write)
    }
    pub fn commit(&mut self, sectors: usize) -> bool {
        if sectors == 0 || sectors > self.writable(sectors) {
            return false;
        }
        self.write = (self.write + sectors) % FIFO_SECTORS;
        self.filled += sectors;
        true
    }
    /// The first sector of the next half, which the caller must upload before
    /// the CD may reuse it, or None when less than a half is buffered.
    pub fn take_half(&mut self) -> Option<usize> {
        if self.filled < HALF_SECTORS {
            return None;
        }
        let at = self.read;
        self.read = (self.read + HALF_SECTORS) % FIFO_SECTORS;
        self.filled -= HALF_SECTORS;
        Some(at)
    }
}

/// Where the refill reads next in one looping track, whole sectors at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub next: u32,
    pub sectors: u32,
}
impl Cursor {
    pub const fn new(sectors: u32) -> Self {
        Self { next: 0, sectors }
    }
    /// Sectors the next read may take: never past the loop, the FIFO's wrap or
    /// its free space.
    pub fn span(&self, fifo: &Fifo, most: usize) -> usize {
        if self.sectors == 0 {
            return 0;
        }
        fifo.writable(most).min((self.sectors - self.next) as usize)
    }
    /// Returns whether the read wrapped the loop.
    pub fn advance(&mut self, sectors: usize) -> bool {
        self.next += sectors as u32;
        if self.next >= self.sectors {
            self.next = 0;
            return true;
        }
        false
    }
}

/// Install the ring's transport flags in one half, in place: A starts the
/// permanent ring, B ends and repeats it. The cook leaves every flag zero, and
/// any other value the CD delivered is cleared, so a stray loop flag cannot
/// send the voice elsewhere. Sample nibbles and headers are untouched.
pub fn flag_half(bytes: &mut [u8], half: usize) -> bool {
    if bytes.len() != HALF_BYTES || half > 1 {
        return false;
    }
    for block in (0..HALF_BYTES).step_by(16) {
        bytes[block + 1] = if half == 0 && block == 0 {
            4
        } else if half == 1 && block == HALF_BYTES - 16 {
            3
        } else {
            0
        };
    }
    true
}

#[cfg(target_arch = "mips")]
mod hardware {
    use super::*;
    use psx_spu::{Pitch, SpuAddr, Voice};
    const IRQ_MASK: u32 = 1 << psx_io::irq::source::SPU;
    #[repr(C, align(4))]
    pub struct Buffer(pub [u8; FIFO_BYTES]);
    /// The FIFO's bytes. The CD writes sectors into it from its interrupt and
    /// the ring reads halves out of it; `Fifo` says which sectors are whose.
    #[no_mangle]
    pub static mut HK_MUSIC_FIFO: Buffer = Buffer([0; FIFO_BYTES]);
    static mut STATE: State = State::new();
    static mut BASE: u32 = 0;
    static mut FIRST_ARM_TICK: u32 = 0;
    static mut ARMED: bool = false;
    /// Half B of a start is uploaded at the first service of a later VBlank.
    static mut SECOND_HALF_DUE: bool = false;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_STARTS: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_REFILLS: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_MAX_SERVICE_GAP: u32 = 0;
    #[no_mangle]
    pub static mut HK_AUDIO_STREAM_UNDERRUNS: u32 = 0;

    pub fn fifo_sector(sector: usize) -> *mut u32 {
        unsafe {
            (&raw mut HK_MUSIC_FIFO.0)
                .cast::<u8>()
                .add(sector * SECTOR)
                .cast::<u32>()
        }
    }
    fn disable_irq() {
        psx_spu::enable_irq(false);
        psx_io::irq::ack(IRQ_MASK);
    }
    fn arm(address: u32) {
        psx_spu::set_irq_address(SpuAddr::new(address));
        psx_spu::enable_irq(true);
    }
    fn upload(fifo: &mut Fifo, half: usize) -> bool {
        let Some(at) = fifo.take_half() else {
            return false;
        };
        let bytes =
            unsafe { core::slice::from_raw_parts_mut(fifo_sector(at).cast::<u8>(), HALF_BYTES) };
        if !flag_half(bytes, half) {
            return false;
        }
        // In 2 KiB slices, taking a pad poll that comes due between them: this
        // runs from inside a checkpoint, and 8 KiB on top of a heavy tick has
        // carried a poll over a VBlank (journey-crossroads, Crossroads_07).
        for (slice, part) in bytes.chunks(SECTOR).enumerate() {
            psx_spu::upload_adpcm(
                unsafe { SpuAddr::new(BASE + (half * HALF_BYTES + slice * SECTOR) as u32) },
                part,
            );
            crate::input::poll_only();
        }
        true
    }
    pub fn running() -> bool {
        unsafe { (*(&raw const STATE)).running() }
    }
    /// Main-thread only; no nested service. The caller has set the voice's
    /// envelope and volume. Takes the first two halves out of the FIFO.
    pub fn start(fifo: &mut Fifo, base: u32) -> bool {
        stop();
        if fifo.filled < 2 * HALF_SECTORS
            || base < 0x1010
            || base & 15 != 0
            || base > 0x80000 - RING_BYTES as u32
            || psx_io::irq::mask() & IRQ_MASK != 0
        {
            return false;
        }
        unsafe {
            BASE = base;
        }
        // Half A now, half B at the next VBlank's service: both at once was a
        // 16 KiB upload that carried a boss-song start past two VBlanks
        // (mawlek-fight). The voice plays A for 0.37 s before it reaches B, and
        // the sectors of B stay buffered (unread, so not free) until it is up.
        if !upload(fifo, 0) {
            return false;
        }
        unsafe {
            SECOND_HALF_DUE = true;
        }
        let voice = Voice::new(VOICE);
        voice.set_start_addr(SpuAddr::new(base));
        voice.set_loop_addr(SpuAddr::new(base));
        voice.set_pitch(Pitch::raw(PITCH));
        Voice::key_on(1 << VOICE);
        // Even a keyed-off voice fetches blocks, and KON takes effect at an
        // SPU sample boundary. Service waits two VBlank transitions before
        // arming B, well before playback can reach B.
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
            SECOND_HALF_DUE = false;
        }
    }
    fn fault() -> Service {
        stop();
        unsafe {
            HK_AUDIO_STREAM_UNDERRUNS = HK_AUDIO_STREAM_UNDERRUNS.wrapping_add(1);
        }
        Service::Fault
    }
    pub fn service(fifo: &mut Fifo, now: u32) -> Service {
        let action = unsafe {
            // Checkpoints can run many times per VBlank. Only one hardware
            // status read per tick is needed for a 0.65-second half-buffer.
            if !(*(&raw const STATE)).running || now == (*(&raw const STATE)).last_poll {
                return Service::Idle;
            }
            if SECOND_HALF_DUE {
                SECOND_HALF_DUE = false;
                if !upload(fifo, 1) {
                    return fault();
                }
            }
            let irq = ARMED && psx_spu::irq_pending();
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
                if !upload(fifo, half) {
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
pub use hardware::{
    fifo_sector, running, service, start, stop, HK_AUDIO_STREAM_MAX_SERVICE_GAP,
    HK_AUDIO_STREAM_UNDERRUNS,
};
