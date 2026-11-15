#![allow(dead_code)] // includes game modules by path and exercises part of each
#![allow(static_mut_refs)]
extern crate self as psx_pad;
extern crate self as psx_rt;
extern crate self as psx_io;
use std::sync::atomic::{AtomicU32, Ordering};
static CLOCK: AtomicU32 = AtomicU32::new(100);
static BUTTONS: AtomicU32 = AtomicU32::new(0);
static POLLS: AtomicU32 = AtomicU32::new(0);
/// The stub pad's left stick, x in the low byte and y in the next (0x80 centred).
static STICKS: AtomicU32 = AtomicU32::new(0x8080);
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
// Every checkpoint also takes a CD event the interrupt wrapper missed while a
// scene's sectors stream in (disc::stream_checkpoint).
static STREAM_CHECKPOINTS: AtomicU32 = AtomicU32::new(0);
mod disc {
    pub fn stream_checkpoint() {
        super::STREAM_CHECKPOINTS.fetch_add(1, super::Ordering::SeqCst);
    }
}
pub mod interrupts {
    pub fn vblank_count() -> u32 {
        super::CLOCK.load(super::Ordering::SeqCst)
    }
}
#[derive(Clone, Copy)]
pub struct Buttons(u16);
impl Buttons {
    pub fn bits(self) -> u16 {
        self.0
    }
}
// The SDK's bit positions (psx-pad `button`), the ones input.rs names.
pub mod button {
    pub const SELECT: u16 = 1 << 0;
    pub const L3: u16 = 1 << 1;
    pub const START: u16 = 1 << 3;
    pub const UP: u16 = 1 << 4;
    pub const RIGHT: u16 = 1 << 5;
    pub const DOWN: u16 = 1 << 6;
    pub const LEFT: u16 = 1 << 7;
    pub const L2: u16 = 1 << 8;
    pub const R2: u16 = 1 << 9;
    pub const L1: u16 = 1 << 10;
    pub const R1: u16 = 1 << 11;
    pub const TRIANGLE: u16 = 1 << 12;
    pub const CIRCLE: u16 = 1 << 13;
    pub const CROSS: u16 = 1 << 14;
    pub const SQUARE: u16 = 1 << 15;
}
/// The left stick as the pad reports it: 0x80 centred.
#[derive(Clone, Copy)]
pub struct Sticks {
    pub left_x: u8,
    pub left_y: u8,
}
impl Sticks {
    pub const CENTERED: Sticks = Sticks { left_x: 0x80, left_y: 0x80 };
    pub fn left_centered(self) -> (i16, i16) {
        (self.left_x as i16 - 0x80, self.left_y as i16 - 0x80)
    }
}
#[derive(Clone, Copy)]
pub struct Pad {
    pub buttons: Buttons,
    pub sticks: Sticks,
}
pub type PadState = Pad;
pub fn poll_port1() -> Pad {
    POLLS.fetch_add(1, Ordering::SeqCst);
    CLOCK.fetch_add(
        POLL_CROSS_VBLANK.swap(0, Ordering::SeqCst),
        Ordering::SeqCst,
    );
    Pad {
        buttons: Buttons(BUTTONS.load(Ordering::SeqCst) as u16),
        sticks: {
            let packed = STICKS.load(Ordering::SeqCst);
            Sticks { left_x: packed as u8, left_y: (packed >> 8) as u8 }
        },
    }
}
/// The SDK's `stick_bits_of` for the stub pad (psx-pad `stick.rs`: strictly beyond the threshold on an
/// axis sets that direction; the stub pad always reports sticks).
pub fn stick_bits_of<const THRESHOLD: i16>(pad: &Pad) -> u16 {
    let (x, y) = pad.sticks.left_centered();
    let mut bits = 0;
    if x < -THRESHOLD {
        bits |= button::LEFT;
    } else if x > THRESHOLD {
        bits |= button::RIGHT;
    }
    if y < -THRESHOLD {
        bits |= button::UP;
    } else if y > THRESHOLD {
        bits |= button::DOWN;
    }
    bits
}
// The reader the game polls through; the stub hardware never rejects a poll.
pub mod periph {
    pub struct ControllerPort;
    impl ControllerPort {
        pub const unsafe fn steal() -> Self {
            Self
        }
    }
}
pub enum Port {
    One,
}
pub struct Controllers {
    last: Pad,
}
impl Controllers {
    pub const fn new() -> Self {
        Self {
            last: Pad { buttons: Buttons(0), sticks: Sticks::CENTERED },
        }
    }
    pub fn poll(&mut self, _port: &mut periph::ControllerPort, _which: Port) -> &Pad {
        self.last = poll_port1();
        &self.last
    }
}
#[allow(non_upper_case_globals)]
pub static mut HK_PAD_POLL_MAX_VBLANK_GAP: u32 = 0;
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/input.rs"]
mod input;
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
#[path = "../../../game/src/input_queue.rs"]
mod input_queue;
#[allow(clippy::all, unexpected_cfgs)] // game source, linted with the game
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
    assert_eq!(STREAM_CHECKPOINTS.load(Ordering::SeqCst), 10);
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
#[test]
fn the_left_stick_is_a_second_d_pad_and_digital_pads_are_untouched() {
    use button::*;
    // The game's own poll: the buttons the stub pad holds with its stick folded in by the SDK's rule.
    let with_stick = |buttons: u16, x: u8, y: u8| {
        BUTTONS.store(u32::from(buttons), Ordering::SeqCst);
        STICKS.store(u32::from(x) | u32::from(y) << 8, Ordering::SeqCst);
        input::poll_bits()
    };
    // Centred, and the SCPH-110's measured rest offset (LX 6C, LY 73): nothing.
    assert_eq!(with_stick(CROSS, 0x80, 0x80), CROSS);
    assert_eq!(with_stick(0, 0x6C, 0x73), 0);
    // The threshold is 48 of 127 per axis, edges included.
    assert_eq!(with_stick(0, 0x80 - 47, 0x80), 0);
    assert_eq!(with_stick(0, 0x80 - 48, 0x80), LEFT);
    assert_eq!(with_stick(0, 0x80 + 48, 0x80), RIGHT);
    assert_eq!(with_stick(0, 0x80, 0x80 - 48), UP);
    assert_eq!(with_stick(0, 0x80, 0x80 + 48), DOWN);
    // Full deflection, a diagonal, and a stick beside a pressed button.
    assert_eq!(with_stick(0, 0x00, 0xFF), LEFT | DOWN);
    assert_eq!(with_stick(SQUARE | UP, 0xFF, 0x80), SQUARE | UP | RIGHT);
    BUTTONS.store(0, Ordering::SeqCst);
    STICKS.store(0x8080, Ordering::SeqCst);
}
