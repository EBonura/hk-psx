//! Scene-gate phase timing for `gate-tour` builds. Every call is a no-op in
//! ordinary builds, so call sites need no cfg of their own.
//!
//! Time is counted in HBlanks (Timer1 runs in HBlank mode for frame-pacing
//! telemetry, see main), about 63.6 us each, and charged to whichever phase
//! was current since the previous call. One 32-word record per gate lands in
//! `HK_GATE_LOG`; tools read it from a final RAM dump.
#![allow(dead_code)]
pub const PREP: u8 = 2;
pub const DRIVE: u8 = 3;
pub const CD: u8 = 4;
pub const AMBIENCE: u8 = 5;
pub const HASH_STORED: u8 = 6;
pub const RELOCATE: u8 = 7;
pub const LZ4: u8 = 8;
pub const HASH_RAW: u8 = 9;
pub const VALIDATE: u8 = 10;
pub const COVERAGE_CHECK: u8 = 11;
pub const UPLOAD: u8 = 12;
pub const SCENE_CHECK: u8 = 13;
pub const META_CHECK: u8 = 14;
pub const DISPLAY: u8 = 15;
pub const REGION: u8 = 16;
pub const OTHER: u8 = 17;
/// The decoder's phase id (room_decode::Decoder::phase_id) as a probe phase.
pub fn decoder(phase_id: u32) -> u8 {
    match phase_id {
        0 => HASH_STORED,
        1 => RELOCATE,
        8 => HASH_RAW,
        9 | 10 => VALIDATE,
        _ => LZ4,
    }
}
#[cfg(feature = "gate-tour")]
mod imp {
    use psx_io::timers::{self, Timer};
    pub const RECORDS: usize = 200;
    pub const WORDS: usize = 32;
    const ACC: usize = 11;
    #[no_mangle]
    pub static mut HK_GATE_LOG: [[u32; WORDS]; RECORDS] = [[0; WORDS]; RECORDS];
    #[no_mangle]
    pub static mut HK_GATE_COUNT: u32 = 0;
    /// The current phase, for --route-watch-u32 (0 outside a gate).
    #[no_mangle]
    pub static mut HK_GATE_PHASE: u32 = 0;
    static mut CUR: u8 = 0;
    static mut LAST: u16 = 0;
    static mut OPEN: bool = false;
    static mut SECTORS0: u32 = 0;
    fn record() -> Option<&'static mut [u32; WORDS]> {
        let n = unsafe { HK_GATE_COUNT } as usize;
        if n == 0 || n > RECORDS {
            return None;
        }
        Some(unsafe { &mut (*(&raw mut HK_GATE_LOG))[n - 1] })
    }
    fn flush() {
        let now = timers::counter(Timer::Timer1);
        let delta = now.wrapping_sub(unsafe { LAST }) as u32;
        unsafe {
            LAST = now;
        }
        let cur = unsafe { CUR } as usize;
        if unsafe { OPEN } && cur != 0 {
            if let Some(r) = record() {
                r[ACC + cur - 1] = r[ACC + cur - 1].wrapping_add(delta);
            }
        }
    }
    pub fn set(phase: u8) {
        flush();
        unsafe {
            CUR = phase;
            HK_GATE_PHASE = phase as u32;
        }
    }
    fn vbl() -> u32 {
        psx_rt::interrupts::vblank_count()
    }
    pub fn trigger(src: usize, dst: usize) {
        // Close whatever phase bootstrap or the last gate left current
        // before this record starts counting.
        set(0);
        unsafe {
            if HK_GATE_COUNT as usize >= RECORDS {
                return;
            }
            HK_GATE_COUNT += 1;
            OPEN = true;
            HK_GATE_PHASE = 1;
        }
        if let Some(r) = record() {
            *r = [0; WORDS];
            r[0] = unsafe { HK_GATE_COUNT };
            r[1] = src as u32;
            r[2] = dst as u32;
            r[3] = vbl();
        }
    }
    pub fn load_start(prefetch: u32) {
        if let Some(r) = record() {
            r[4] = vbl();
            r[8] = prefetch;
        }
        unsafe {
            SECTORS0 = crate::disc::HK_CD_SECTORS_READ;
        }
        set(super::OTHER);
    }
    pub fn display_on() {
        if let Some(r) = record() {
            r[5] = vbl();
            r[9] = unsafe { crate::disc::HK_CD_SECTORS_READ }.wrapping_sub(unsafe { SECTORS0 });
        }
        set(super::REGION);
    }
    pub fn read_started() {
        if let Some(r) = record() {
            r[10] += 1;
        }
    }
    /// Once per presented frame. The first one after a load closes the
    /// load's time; the shade reaching zero closes the transition.
    pub fn frame(shade: u8) {
        if !unsafe { OPEN } {
            return;
        }
        let Some(r) = record() else { return };
        if r[5] != 0 && r[6] == 0 {
            r[6] = vbl();
            set(0);
        }
        // The first presented frame that is not fully black: the fade-in start.
        if r[6] != 0 && r[28] == 0 && shade < 255 {
            r[28] = vbl();
        }
        if r[6] != 0 && r[7] == 0 && shade == 0 {
            r[7] = vbl();
            unsafe {
                OPEN = false;
                HK_GATE_PHASE = 0;
            }
        }
    }
    pub fn note(word: usize, value: u32) {
        if let Some(r) = record() {
            if (28..WORDS).contains(&word) {
                r[word] = value;
            }
        }
    }
}
#[cfg(feature = "gate-tour")]
pub use imp::{display_on, frame, load_start, note, read_started, set, trigger};
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn set(_: u8) {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn trigger(_: usize, _: usize) {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn load_start(_: u32) {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn display_on() {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn frame(_: u8) {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn read_started() {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn note(_: usize, _: u32) {}
