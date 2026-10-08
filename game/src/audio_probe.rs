//! Console probes for the area music transport, built only with the
//! `audio-probe` feature (host/build_guest.py --audio-probe). The disc boots
//! through the title as usual, then runs these instead of the game and shows
//! the results on screen, so a burned disc can be filmed. Every figure is also
//! a HK_PROBE_* symbol for emulator replays.
//!
//! A. Ring under room reads (120 s): Crossroads_01's music streams through
//!    the SPU ring while the drive reads a scene group every 2 to 4 s, picked
//!    pseudo-randomly so the seeks are long. Counts underruns, the FIFO's low
//!    point, the room and music reads, read errors and the longest service gap.
//! B. XA pause, far read, resume (8 cycles): Boss1 plays as an XA song,
//!    pauses, the drive reads a scene chunk at the far end of WORLD.PAK, then
//!    the song resumes from the sector where it paused. Times each step in
//!    VBlanks and records how far from the pause point the resumed position
//!    is. This is the cost of sharing the drive between XA music and room
//!    reads: what a gate load would cut out of an area song.
use psx_io::cdrom;
use psx_spu::{self as spu, CdVolume};
use psx_math::fmt::{u32_dec, U32_DEC_MAX};

const PROBE_A_TICKS: u32 = 120 * 60;
const CYCLES: usize = 8;
const SPINS: u32 = 131072;
/// Crossroads_01, whose SceneManager plays the Crossroads cue under Normal.
const MUSIC_SCENE: usize = 2;

#[no_mangle] pub static mut HK_PROBE_PHASE: u32 = 0;
#[no_mangle] pub static mut HK_PROBE_A_ROOM_READS: u32 = 0;
#[no_mangle] pub static mut HK_PROBE_A_ROOM_SECTORS: u32 = 0;
#[no_mangle] pub static mut HK_PROBE_A_ROOM_ERRORS: u32 = 0;
#[no_mangle] pub static mut HK_PROBE_A_TICKS: u32 = 0;
/// Per cycle: VBlanks for Pause to complete, for the far read, for resumed
/// playback to report playing, and the resumed position minus the paused one
/// in sectors (signed), plus a failure bitmask.
#[no_mangle] pub static mut HK_PROBE_B_PAUSE: [u32; CYCLES] = [0; CYCLES];
#[no_mangle] pub static mut HK_PROBE_B_READ: [u32; CYCLES] = [0; CYCLES];
#[no_mangle] pub static mut HK_PROBE_B_RESUME: [u32; CYCLES] = [0; CYCLES];
#[no_mangle] pub static mut HK_PROBE_B_DRIFT: [i32; CYCLES] = [0; CYCLES];
#[no_mangle] pub static mut HK_PROBE_B_FAIL: [u32; CYCLES] = [0; CYCLES];

fn now() -> u32 { psx_rt::interrupts::vblank_count() }
/// Keep the music ring and the drive serviced for `ticks` VBlanks.
fn idle(ticks: u32) {
    let end = now().wrapping_add(ticks);
    while now().wrapping_sub(end) > u32::MAX / 2 { crate::input::checkpoint(); }
}

struct Line { bytes: [u8; 40], len: usize }
impl Line {
    fn new() -> Self { Self { bytes: [b' '; 40], len: 0 } }
    fn s(mut self, text: &str) -> Self {
        for b in text.bytes() { if self.len < 40 { self.bytes[self.len] = b; self.len += 1; } }
        self
    }
    fn n(self, value: u32) -> Self { let mut d = [0u8; U32_DEC_MAX]; let t = u32_dec(&mut d, value); self.s(t) }
    fn i(self, value: i32) -> Self { if value < 0 { self.s("-").n(value.unsigned_abs()) } else { self.n(value as u32) } }
    fn as_str(&self) -> &str { core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("") }
}

fn show(fb: &mut psx_gpu::framebuf::FrameBuffer, lines: &[Line]) {
    let mut text: [&str; 14] = [""; 14];
    for (slot, line) in text.iter_mut().zip(lines.iter()) { *slot = line.as_str(); }
    crate::menu::probe_screen(fb, &text[..lines.len().min(14)]);
}

fn read_u32(value: *const u32) -> u32 { unsafe { core::ptr::read_volatile(value) } }

fn probe_a(cache: &mut crate::disc::Cache, fb: &mut psx_gpu::framebuf::FrameBuffer) {
    unsafe { HK_PROBE_PHASE = 1; }
    crate::music::enter_scene(MUSIC_SCENE);
    let start = now();
    let mut next = start.wrapping_add(180);
    let mut seed = 0x2545_f491u32;
    let mut shown = start;
    while now().wrapping_sub(start) < PROBE_A_TICKS {
        crate::input::checkpoint();
        if now().wrapping_sub(next) < u32::MAX / 2 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            crate::disc::begin_load();
            match cache.probe_read_group((seed >> 8) as usize) {
                Ok(sectors) => unsafe { HK_PROBE_A_ROOM_READS += 1; HK_PROBE_A_ROOM_SECTORS += sectors; },
                Err(_) => unsafe { HK_PROBE_A_ROOM_ERRORS += 1; },
            }
            crate::disc::end_load();
            next = now().wrapping_add(120 + (seed >> 20) % 120);
        }
        if now().wrapping_sub(shown) >= 30 {
            shown = now();
            unsafe { HK_PROBE_A_TICKS = now().wrapping_sub(start); }
            show(fb, &[
                Line::new().s("A: music ring under room reads"),
                Line::new().s("seconds ").n(now().wrapping_sub(start) / 60).s(" of 120"),
                Line::new().s("underruns ").n(read_u32(&raw const crate::music::stream::HK_AUDIO_STREAM_UNDERRUNS)),
                Line::new().s("fifo min ").n(read_u32(&raw const crate::music::HK_MUSIC_FIFO_MIN)).s(" now ").n(read_u32(&raw const crate::music::HK_MUSIC_FIFO_FILL)),
                Line::new().s("room reads ").n(read_u32(&raw const HK_PROBE_A_ROOM_READS)).s(" err ").n(read_u32(&raw const HK_PROBE_A_ROOM_ERRORS)),
                Line::new().s("music reads ").n(read_u32(&raw const crate::music::HK_MUSIC_READS)).s(" in-load ").n(read_u32(&raw const crate::music::HK_MUSIC_LOAD_READS)),
                Line::new().s("music read err ").n(read_u32(&raw const crate::music::HK_MUSIC_READ_ERRORS)),
                Line::new().s("max gap ").n(read_u32(&raw const crate::music::stream::HK_AUDIO_STREAM_MAX_SERVICE_GAP)),
            ]);
        }
    }
    crate::music::enter_scene(0);
    idle(90);
}

/// Absolute LBA the head is reading (the drive counts from the lead-in, 150 sectors early).
fn head() -> Option<i32> {
    cdrom::try_get_loc_p(SPINS).and_then(|r| cdrom::PlayPosition::parse(&r)).map(|p|
        (p.absolute_min as i32 * 60 + p.absolute_sec as i32) * 75 + p.absolute_frame as i32 - 150)
}
/// Start the XA channel streaming from `lba` (an absolute LBA, already shifted).
fn xa_start(lba: u32, channel: u8) -> bool {
    cdrom::try_demute(SPINS).is_some()
        && cdrom::try_set_mode(0x40 | 0x08, SPINS).is_some()
        && cdrom::try_command(0x0D, &[crate::music::XA_FILE_NUMBER, channel], SPINS).is_some()
        && cdrom::try_set_loc_lba(lba, SPINS).is_some()
        && cdrom::try_command(0x1B, &[], SPINS).is_some()
}
/// VBlanks until `ready` holds, polling it every other VBlank, or u32::MAX.
fn wait(limit: u32, mut ready: impl FnMut() -> bool) -> u32 {
    let start = now();
    loop {
        if ready() { return now().wrapping_sub(start); }
        if now().wrapping_sub(start) > limit { return u32::MAX; }
        idle(2);
    }
}

fn probe_b(cache: &mut crate::disc::Cache, fb: &mut psx_gpu::framebuf::FrameBuffer) {
    unsafe { HK_PROBE_PHASE = 2; }
    while !crate::disc::cdda_acquire() { idle(1); }
    let gain = CdVolume(crate::music::BOSS_GAIN);
    spu::set_cd_volume(gain, gain);
    spu::enable_cd_audio(true);
    let top = psx_io::disc_base::shift_lba(crate::music::xa_lba());
    let mut fail = 0u32;
    if !xa_start(top, crate::music::BOSS_TRACK) { fail |= 4; }
    if wait(600, || head().is_some_and(|h| h >= top as i32 + 8)) == u32::MAX { fail |= 8; }
    for cycle in 0..CYCLES {
        idle(180);
        let paused_at = head();
        let t = wait(600, || cdrom::try_pause_until_complete(4_000_000));
        crate::disc::cdda_release();
        let started = now();
        let read = cache.probe_far_read();
        let read_ticks = now().wrapping_sub(started);
        while !crate::disc::cdda_acquire() { idle(1); }
        let mut f = if paused_at.is_none() { 16 } else { 0 } | if read.is_err() { 32 } else { 0 };
        let resume_ticks = match paused_at {
            Some(at) => {
                if !xa_start(at.max(top as i32) as u32, crate::music::BOSS_TRACK) { f |= 64; }
                wait(600, || head().is_some_and(|h| h >= at + 8))
            }
            None => u32::MAX,
        };
        let drift = match (paused_at, head()) { (Some(a), Some(b)) => b - a, _ => i32::MIN };
        unsafe {
            HK_PROBE_B_PAUSE[cycle] = t; HK_PROBE_B_READ[cycle] = read_ticks;
            HK_PROBE_B_RESUME[cycle] = resume_ticks; HK_PROBE_B_DRIFT[cycle] = drift; HK_PROBE_B_FAIL[cycle] = f | fail;
        }
        fail = 0;
        // A's result stays on screen above B's, so one photo holds both.
        let mut lines = [Line::new().s("A underrun ").n(read_u32(&raw const crate::music::stream::HK_AUDIO_STREAM_UNDERRUNS))
                .s(" min ").n(read_u32(&raw const crate::music::HK_MUSIC_FIFO_MIN)).s(" rd ").n(read_u32(&raw const HK_PROBE_A_ROOM_READS))
                .s(" err ").n(read_u32(&raw const HK_PROBE_A_ROOM_ERRORS) + read_u32(&raw const crate::music::HK_MUSIC_READ_ERRORS)),
            Line::new().s("B: pause read resume drift fail"),
            Line::new(), Line::new(), Line::new(), Line::new(), Line::new(), Line::new(), Line::new(), Line::new()];
        for c in 0..=cycle {
            let at = |v: &[u32; CYCLES]| unsafe { core::ptr::read_volatile(&v[c]) };
            lines[2 + c] = Line::new().n(at(unsafe { &*(&raw const HK_PROBE_B_PAUSE) })).s(" ")
                .n(at(unsafe { &*(&raw const HK_PROBE_B_READ) })).s(" ").n(at(unsafe { &*(&raw const HK_PROBE_B_RESUME) })).s(" ")
                .i(unsafe { HK_PROBE_B_DRIFT[c] }).s(" ").n(at(unsafe { &*(&raw const HK_PROBE_B_FAIL) }));
        }
        show(fb, &lines[..3 + cycle]);
    }
    spu::set_cd_volume(CdVolume::SILENCE, CdVolume::SILENCE);
    let _ = cdrom::try_pause_until_complete(4_000_000);
    crate::disc::cdda_release();
    unsafe { HK_PROBE_PHASE = 3; }
}

/// Runs both probes, then holds the last screen.
pub fn run(cache: &mut crate::disc::Cache, fb: &mut psx_gpu::framebuf::FrameBuffer) -> ! {
    while cache.prepare_ambience().is_err() { idle(60); }
    probe_a(cache, fb);
    probe_b(cache, fb);
    loop { idle(60); }
}
