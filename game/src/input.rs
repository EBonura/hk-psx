//! Main-thread-only cooperative pad service; never called from interrupt context.
use crate::input_sampler::{Fault, Sampler};
static mut SAMPLER: Sampler = Sampler::new();
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
/// Samples polled but not yet consumed by the simulation, at the last publish.
#[no_mangle]
pub static mut HK_INPUT_PENDING: u32 = 0;
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
    crate::ambience::service();
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
            let bits = psx_pad::poll_port1().buttons.bits();
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
pub fn consume(tick: u32) -> u16 {
    // The main-loop clock read can cross VBlank after its preceding checkpoint.
    // Observe actual current time before consuming; never poll a historical tick.
    checkpoint();
    unsafe {
        match SAMPLER.consume(tick) {
            Ok(bits) => bits,
            Err(fault) => failed(fault),
        }
    }
}
