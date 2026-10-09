#![allow(static_mut_refs)]
extern crate self as psx_pad;
extern crate self as psx_rt;
use std::sync::atomic::{AtomicU32, Ordering};
static CLOCK: AtomicU32 = AtomicU32::new(100);
static BUTTONS: AtomicU32 = AtomicU32::new(0);
static POLLS: AtomicU32 = AtomicU32::new(0);
static POLL_CROSS_VBLANK: AtomicU32 = AtomicU32::new(0);
// This harness exercises real input sampling with stub hardware. Presentation
// state ordering has its own native tests; count calls here to verify input
// checkpoints still service it even when no new pad transaction is due.
static PRESENT_CHECKPOINTS: AtomicU32 = AtomicU32::new(0);
static AUDIO_CHECKPOINTS: AtomicU32 = AtomicU32::new(0);
mod music {
    pub fn service() {
        super::AUDIO_CHECKPOINTS.fetch_add(1, super::Ordering::SeqCst);
    }
}
mod presentation {
    pub fn checkpoint() {
        super::PRESENT_CHECKPOINTS.fetch_add(1, super::Ordering::SeqCst);
    }
}
// The exit fade steps once per VBlank from every checkpoint during a gate
// load; it has no pad state, so it is counted like the other services.
static FADE_CHECKPOINTS: AtomicU32 = AtomicU32::new(0);
mod exit_fade {
    pub fn service() {
        super::FADE_CHECKPOINTS.fetch_add(1, super::Ordering::SeqCst);
    }
}
pub mod interrupts {
    pub fn vblank_count() -> u32 {
        super::CLOCK.load(super::Ordering::SeqCst)
    }
}
pub struct Buttons(u16);
impl Buttons {
    pub fn bits(self) -> u16 {
        self.0
    }
}
pub struct Pad {
    pub buttons: Buttons,
}
pub fn poll_port1() -> Pad {
    POLLS.fetch_add(1, Ordering::SeqCst);
    CLOCK.fetch_add(
        POLL_CROSS_VBLANK.swap(0, Ordering::SeqCst),
        Ordering::SeqCst,
    );
    Pad {
        buttons: Buttons(BUTTONS.load(Ordering::SeqCst) as u16),
    }
}
// The reader the game polls through; the stub hardware never rejects a poll.
pub struct PadReader;
impl PadReader {
    pub const fn port1() -> Self {
        Self
    }
    pub fn poll(&mut self) -> Pad {
        poll_port1()
    }
}
#[allow(non_upper_case_globals)]
pub static mut HK_PAD_POLL_MAX_VBLANK_GAP: u32 = 0;
#[path = "../../../game/src/input.rs"]
mod input;
#[path = "../../../game/src/input_queue.rs"]
mod input_queue;
#[path = "../../../game/src/input_sampler.rs"]
mod input_sampler;
#[test]
fn hardware_wrapper_closes_clock_read_race_and_preserves_pending_pulse() {
    input::checkpoint(); // Boot never owns pad polling.
    input::start(100);
    input::checkpoint();
    assert_eq!(POLLS.load(Ordering::SeqCst), 0);
    assert_eq!(PRESENT_CHECKPOINTS.load(Ordering::SeqCst), 2);
    // VBlank arrived after main's last checkpoint and before reading `now`.
    CLOCK.store(101, Ordering::SeqCst);
    BUTTONS.store(0x4000, Ordering::SeqCst);
    assert_eq!(input::consume(101), 0x4000);
    assert_eq!(POLLS.load(Ordering::SeqCst), 1);
    input::checkpoint();
    assert_eq!(POLLS.load(Ordering::SeqCst), 1);
    CLOCK.store(102, Ordering::SeqCst);
    BUTTONS.store(0, Ordering::SeqCst);
    input::checkpoint();
    CLOCK.store(103, Ordering::SeqCst);
    BUTTONS.store(0x1000, Ordering::SeqCst);
    input::checkpoint();
    CLOCK.store(104, Ordering::SeqCst);
    BUTTONS.store(0, Ordering::SeqCst);
    input::checkpoint();
    assert_eq!(input::consume(102), 0);
    assert_eq!(input::consume(103), 0x1000);
    assert_eq!(input::consume(104), 0);
    assert_eq!(POLLS.load(Ordering::SeqCst), 4);
    assert_eq!(PRESENT_CHECKPOINTS.load(Ordering::SeqCst), 10);
    assert_eq!(AUDIO_CHECKPOINTS.load(Ordering::SeqCst), 10);
    assert_eq!(FADE_CHECKPOINTS.load(Ordering::SeqCst), 10);
    unsafe {
        assert_eq!(input::HK_INPUT_POLLS, 4);
        assert_eq!(input::HK_INPUT_MISSED_VBLANKS, 0);
        assert_eq!(input::HK_INPUT_QUEUE_PEAK, 3);
        assert_eq!(input::HK_INPUT_FAULT, 0);
        assert_eq!(HK_PAD_POLL_MAX_VBLANK_GAP, 1);
    }
    // Exercise the real wrapper with a load much longer than the 16-entry FIFO.
    input::begin_scene_load(104);
    for tick in 105..=704 {
        CLOCK.store(tick, Ordering::SeqCst);
        BUTTONS.store(if tick == 704 { 0x4000 } else { 0 }, Ordering::SeqCst);
        input::checkpoint();
    }
    // Final checkpoint's transaction crosses VBlank. The returned boundary is
    // the completion tick, with its real held state, not the stale entry read.
    CLOCK.store(705, Ordering::SeqCst);
    POLL_CROSS_VBLANK.store(1, Ordering::SeqCst);
    assert_eq!(input::end_scene_load(), 706);
    assert_eq!(input::held_buttons(), 0x4000);
    // A VBlank after return is normal input, including an immediate new button.
    CLOCK.store(707, Ordering::SeqCst);
    BUTTONS.store(0x1000, Ordering::SeqCst);
    assert_eq!(input::consume(707), 0x1000);
    unsafe {
        assert_eq!(input::HK_INPUT_POLLS, 606);
        assert_eq!(input::HK_INPUT_MISSED_VBLANKS, 1);
        assert_eq!(input::HK_INPUT_LOADING_SAMPLES, 601);
        assert_eq!(input::HK_INPUT_LOADING_TICKS, 602);
        assert_eq!(input::HK_INPUT_QUEUE_PEAK, 3);
        assert_eq!(input::HK_INPUT_FAULT, 0);
    }
    assert_eq!(AUDIO_CHECKPOINTS.load(Ordering::SeqCst), 613);
    assert_eq!(PRESENT_CHECKPOINTS.load(Ordering::SeqCst), 613);
}
