//! Main-thread-only cooperative pad service; never called from interrupt context.
use crate::input_sampler::{Fault, Sampler};
static mut SAMPLER: Sampler = Sampler::new();
/// Port 1 through one reader for the whole program, menus and gameplay alike:
/// a poll the driver rejects reads as the last clean state instead of every
/// button released, so a held button cannot fire twice. See PAD-ANALOG.md.
static mut PAD: psx_pad::PadReader = psx_pad::PadReader::port1();
/// One poll of port 1 through the reader, as button bits.
pub fn poll_bits() -> u16 {
    unsafe { (*(&raw mut PAD)).poll().buttons.bits() }
}
#[no_mangle]
pub static mut HK_INPUT_POLLS: u32 = 0;
#[no_mangle]
pub static mut HK_INPUT_MISSED_VBLANKS: u32 = 0;
#[no_mangle]
pub static mut HK_INPUT_QUEUE_PEAK: u32 = 0;
#[no_mangle]
pub static mut HK_INPUT_FAULT: u32 = 0;
#[no_mangle]
pub static mut HK_INPUT_LOADING_SAMPLES: u32 = 0;
#[no_mangle]
pub static mut HK_INPUT_LOADING_TICKS: u32 = 0;
#[no_mangle]
pub static mut HK_INPUT_BLOCKED_VBLANKS: u32 = 0;
/// Samples a full input queue dropped (the simulation was 16 ticks behind).
#[no_mangle]
pub static mut HK_INPUT_DROPPED_SAMPLES: u32 = 0;
/// Simulation ticks skipped because more than `MAX_LAG` waited for it.
#[no_mangle]
pub static mut HK_INPUT_SKIPPED_TICKS: u32 = 0;
/// Most observed ticks the simulation may trail its input by before ticks are
/// skipped. Half the queue: every validated route peaks at 4 or less, and
/// the other half absorbs one render or tick of up to eight VBlanks before
/// the queue itself has to drop a sample.
pub const MAX_LAG: u32 = 8;
/// Samples polled but not yet consumed by the simulation, at the last publish.
#[no_mangle]
pub static mut HK_INPUT_PENDING: u32 = 0;
/// Samples consumed so far (one per simulated tick) and the last one's
/// buttons, for main to publish with the tick's results (HK_SIM_TICKS).
pub static mut CONSUMED: u32 = 0;
pub static mut CONSUMED_PAD: u32 = 0;
fn publish_stats() {
    unsafe {
        let stats = SAMPLER.stats();
        HK_INPUT_PENDING = SAMPLER.pending_count() as u32;
        HK_INPUT_POLLS = stats.polls;
        HK_INPUT_MISSED_VBLANKS = stats.missed_vblanks;
        HK_INPUT_QUEUE_PEAK = stats.pending_peak;
        HK_INPUT_LOADING_SAMPLES = stats.loading_samples;
        HK_INPUT_LOADING_TICKS = stats.loading_ticks;
        HK_INPUT_BLOCKED_VBLANKS = stats.blocked_vblanks;
        HK_INPUT_DROPPED_SAMPLES = stats.dropped_samples;
        HK_INPUT_SKIPPED_TICKS = stats.skipped_ticks;
        crate::HK_PAD_POLL_MAX_VBLANK_GAP = stats.max_poll_gap;
    }
}
/// Explicitly pause input consumption for a scene replacement. Enter before
/// blocking work; all subsequent checkpoints still service pad/audio/display.
pub fn begin_scene_load(sim_clock: u32) {
    unsafe {
        if let Err(fault) = SAMPLER.begin_scene_load(sim_clock) { failed(fault); }
    }
    publish_stats();
    checkpoint();
}
/// Memory card frames own SIO0 for longer than a VBlank; count their poll
/// gaps as blocked rather than missed until the scene load ends.
pub fn begin_blocking_transfer() {
    unsafe {
        if let Err(fault) = SAMPLER.begin_blocking_transfer() { failed(fault); }
    }
}
/// Return the observed resume boundary, never an unpolled hardware timestamp.
pub fn end_scene_load() -> u32 {
    checkpoint();
    unsafe {
        match SAMPLER.end_scene_load() {
            Ok(tick) => tick,
            Err(fault) => failed(fault),
        }
    }
}
/// Use after end_scene_load to seed edge detectors from the last loading poll.
pub fn held_buttons() -> u16 {
    unsafe { SAMPLER.held_buttons() }
}
fn failed(fault: Fault) -> ! {
    unsafe {
        HK_INPUT_FAULT = fault.code();
    }
    panic!("input sampler contract");
}
pub fn start(tick: u32) {
    unsafe {
        if let Err(fault) = SAMPLER.start(tick) {
            failed(fault);
        }
    }
}
#[inline]
pub fn checkpoint() {
    crate::presentation::checkpoint();
    crate::exit_fade::service();
    crate::music::service();
    let now = psx_rt::interrupts::vblank_count();
    if unsafe { SAMPLER.poll_due(now) } {
        poll_slow(now);
    }
}
/// Only the pad half of `checkpoint`, for code that runs inside one (an SPU
/// upload from the audio service): take a poll that has come due, and nothing
/// else, so the services are never re-entered.
#[inline]
pub fn poll_only() {
    let now = psx_rt::interrupts::vblank_count();
    if unsafe { SAMPLER.poll_due(now) } {
        poll_slow(now);
    }
}
// Keep the hardware transaction and stats publication out of idle checkpoints.
#[inline(never)]
fn poll_slow(now: u32) {
    unsafe {
        let result = SAMPLER.checkpoint(now, || {
            let bits = poll_bits();
            (psx_rt::interrupts::vblank_count(), bits)
        });
        match result {
            Ok(false) => {}
            Ok(true) => {
                publish_stats();
            }
            Err(fault) => failed(fault),
        }
    }
}
/// Before a catch-up run to `now`: if more than `MAX_LAG` ticks wait, skip
/// the oldest so the run starts `MAX_LAG` behind, and return the tick to
/// continue from (the simulation clock after the skip).
pub fn bound_lag(now: u32) -> Option<u32> {
    let skipped = unsafe {
        match SAMPLER.bound_lag(now, MAX_LAG) {
            Ok(skipped) => skipped,
            Err(fault) => failed(fault),
        }
    };
    if skipped.is_some() {
        publish_stats();
    }
    skipped
}
/// The original reads input in Update and moves the Hero in the next
/// FixedUpdate, so a walk or jump starts one frame after the press there,
/// while its menus, dialogue and shop answer in Update, at once. With
/// `original-latency` (on by default) the Knight's simulation takes LEFT,
/// RIGHT and CROSS from the tick before (`hero_latency`) while no menu, panel
/// or prompt reads the pad (frame::menu_reads_pad); everything else, and the
/// Knight while one does, reads the pad as polled. Side by side, the port's walking
/// lead over the original falls from 0.25 units to the 50 Hz physics phase
/// (0.11 units). After a scene load the previous tick is the last loading
/// poll (`seed_latency`).
#[cfg(feature = "original-latency")]
const ORIGINAL_LATE: u16 = psx_pad::button::LEFT | psx_pad::button::RIGHT | psx_pad::button::CROSS;
#[cfg(feature = "original-latency")]
static mut LATE_PREVIOUS: u16 = 0;
/// The Knight's view of this tick's pad: `raw` with the late buttons of the
/// previous consumed tick. Call once per consumed tick, before anything skips it.
#[inline]
pub fn hero_latency(raw: u16) -> u16 {
    #[cfg(feature = "original-latency")]
    unsafe {
        let out = (raw & !ORIGINAL_LATE) | (LATE_PREVIOUS & ORIGINAL_LATE);
        LATE_PREVIOUS = raw;
        return out;
    }
    #[cfg(not(feature = "original-latency"))]
    raw
}
/// After a scene load or card write: the tick before the next one is the last
/// loading poll, whose hold the sampler resumes from.
#[inline]
pub fn seed_latency() {
    #[cfg(feature = "original-latency")]
    unsafe {
        LATE_PREVIOUS = SAMPLER.held_buttons();
    }
}
/// Route re-encoding telemetry (`tick-log`): per consumed tick, the poll
/// ordinal of its sample and whether the Knight read it late (bit 31).
#[cfg(feature = "tick-log")]
#[no_mangle]
pub static mut HK_TICK_LOG: [u32; 16] = [0; 16];
#[cfg(feature = "tick-log")]
#[no_mangle]
pub static mut HK_TICK_LOG_COUNT: u32 = 0;
#[cfg(feature = "tick-log")]
pub fn log_tick(hero: bool) {
    unsafe {
        let ordinal = SAMPLER.stats().polls.wrapping_sub(SAMPLER.pending_count() as u32);
        HK_TICK_LOG[(HK_TICK_LOG_COUNT & 15) as usize] = ordinal | (u32::from(hero) << 31);
        HK_TICK_LOG_COUNT = HK_TICK_LOG_COUNT.wrapping_add(1);
    }
}
pub fn consume(tick: u32) -> u16 {
    // The main-loop clock read can cross VBlank after its preceding checkpoint.
    // Observe actual current time before consuming; never poll a historical tick.
    checkpoint();
    unsafe {
        match SAMPLER.consume(tick) {
            Ok(bits) => {
                CONSUMED = CONSUMED.wrapping_add(1);
                CONSUMED_PAD = bits as u32;
                bits
            }
            Err(fault) => failed(fault),
        }
    }
}
