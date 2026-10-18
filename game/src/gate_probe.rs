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
#[cfg(feature = "gate-log")]
mod imp {
    use psx_io::timers::{self, Timer};
    /// Gate records kept: the whole 164-step tour by default; diagnosis builds that stop
    /// early (HK_TOUR_STOP) ask for fewer (HK_GATE_RECORDS) to leave BSS for the event ring.
    pub const RECORDS: usize = match option_env!("HK_GATE_RECORDS") {
        Some(s) => parse(s),
        None => 164,
    };
    const fn parse(s: &str) -> usize {
        let b = s.as_bytes();
        let mut i = 0;
        let mut v = 0;
        while i < b.len() {
            v = v * 10 + (b[i] - b'0') as usize;
            i += 1;
        }
        v
    }
    pub const WORDS: usize = 23;
    /// Phases p = 2..=17 accumulate as u16 HBlank halves of words 11..=18.
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
        if unsafe { OPEN } && cur >= 2 {
            if let Some(r) = record() {
                let i = cur - 2;
                let w = &mut r[ACC + i / 2];
                let sh = (i % 2) * 16;
                let old = (*w >> sh) & 0xffff;
                let v = (old + delta).min(0xffff);
                *w = (*w & !(0xffff << sh)) | v << sh;
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
        event(11, src as u32, dst as u32);
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
        if r[6] != 0 && r[19] == 0 && shade < 255 {
            r[19] = vbl();
        }
        if r[6] != 0 && r[7] == 0 && shade == 0 {
            r[7] = vbl();
            unsafe {
                OPEN = false;
                HK_GATE_PHASE = 0;
            }
        }
    }
    /// Drive events for the gate study: (vblank << 8 | kind, a << 16 | b & 0xffff).
    #[cfg(feature = "gate-events")]
    pub const EVENTS: usize = 160;
    #[cfg(not(feature = "gate-events"))]
    pub const EVENTS: usize = 1;
    #[no_mangle]
    pub static mut HK_EVENT_LOG: [[u32; 2]; EVENTS] = [[0; 2]; EVENTS];
    #[no_mangle]
    pub static mut HK_EVENT_N: u32 = 0;
    #[no_mangle]
    pub static mut HK_GAP_LOG: [[u32; 2]; 24] = [[0; 2]; 24];
    #[no_mangle]
    pub static mut HK_GAP_N: u32 = 0;
    #[cfg(not(feature = "gate-events"))]
    pub fn event(_: u8, _: u32, _: u32) {}
    #[cfg(feature = "gate-events")]
    pub fn event(kind: u8, a: u32, b: u32) {
        if kind == 16 {
            // Poll gaps are kept apart: the first 24 of the whole run.
            unsafe {
                let n = HK_GAP_N;
                HK_GAP_N = n + 1;
                if (n as usize) < 24 {
                    (*(&raw mut HK_GAP_LOG))[n as usize] = [vbl(), a << 16 | (b & 0xffff)];
                }
            }
        }
        unsafe {
            let n = HK_EVENT_N;
            HK_EVENT_N = n.wrapping_add(1);
            (*(&raw mut HK_EVENT_LOG))[n as usize % EVENTS] =
                [vbl() << 8 | kind as u32, (a << 16) | (b & 0xffff)];
        }
    }
    /// What ran last among the long operations, for attributing a stall (set by `ctx`).
    static mut CTX: u32 = 0;
    pub fn ctx(c: u32) {
        unsafe {
            CTX = c;
        }
    }
    pub fn stall_ctx() -> u32 {
        unsafe { CTX << 8 | HK_GATE_PHASE }
    }
    pub fn note(word: usize, value: u32) {
        if let Some(r) = record() {
            if (19..WORDS).contains(&word) {
                r[word] = value;
            }
        }
    }
}
#[cfg(feature = "gate-log")]
pub use imp::{ctx, display_on, event, frame, load_start, note, read_started, set, stall_ctx, trigger};
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn set(_: u8) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn trigger(_: usize, _: usize) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn load_start(_: u32) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn display_on() {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn frame(_: u8) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn read_started() {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn note(_: usize, _: u32) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn event(_: u8, _: u32, _: u32) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn ctx(_: u32) {}
#[cfg(not(feature = "gate-log"))]
#[inline(always)]
pub fn stall_ctx() -> u32 {
    0
}
