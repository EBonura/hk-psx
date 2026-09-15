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
                let stats = SAMPLER.stats();
                HK_INPUT_POLLS = stats.polls;
                HK_INPUT_MISSED_VBLANKS = stats.missed_vblanks;
                HK_INPUT_QUEUE_PEAK = stats.pending_peak;
                crate::HK_PAD_POLL_MAX_VBLANK_GAP = stats.max_poll_gap;
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
