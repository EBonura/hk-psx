//! Memory card diagnostics for a console, built only with the `card-diag`
//! feature (`HK_GUEST_FEATURES=card-diag`). The shipping build has none of this.
//!
//! The bench Save has never been seen to write a card on silicon: the pad and
//! the card share SIO0, the card's flash commit is slow, and the emulator ACKs
//! instantly. A burned disc of this build answers on the screen, so one
//! photograph is enough:
//!
//! * After the title, a CARD DIAG page: the first card transaction of the boot
//!   (what the chain-load left SIO0 in), a fresh frame 0 read, the directory,
//!   and a write-back of frame 63 (the BIOS's own write-test frame, written
//!   with the bytes it already holds). A failed write tries other timings and
//!   names the first that works. X continues to the game.
//! * In the game, two lines at the bottom of every frame: the vblank counter,
//!   game mode, region, save notice state, writes and errors, retries, the last
//!   card fault and frame, how long the last save took in video periods, and
//!   GPUSTAT and the GPU DMA control word (to see a stuck wait at a glance).
//!
//! Each figure is also an `HK_CARD_*` symbol for emulator replays.
use crate::save::Fault;
use psx_gpu::framebuf::FrameBuffer;
use psx_mc::hardware::Timing;
use psx_mc::{Block, ControllerPort, Error, HardwareCard, Slot, TransportFault, TransportTrace};

/// The first transaction of the boot, and the latest one.
static mut FIRST: Option<TransportTrace> = None;
static mut FIRST_RETRIES: u32 = 0;
static mut LAST: Option<TransportTrace> = None;
/// The boot-time card gate: zero for a usable card, else `Fault` plus one.
#[no_mangle]
pub static mut HK_CARD_GATE: u32 = 0;
/// The write-back probe: 0 not run, 1 passed, 2 failed, 3 passed only with
/// another timing (which one is `HK_CARD_PROBE_TIMING`).
#[no_mangle]
pub static mut HK_CARD_PROBE: u32 = 0;
#[no_mangle]
pub static mut HK_CARD_PROBE_TIMING: u32 = 0;

/// Called by the card layer after every frame transaction.
#[inline(never)]
#[optimize(size)]
pub fn note(trace: &TransportTrace, retries: u32) {
    unsafe {
        if (*(&raw const FIRST)).is_none() {
            FIRST = Some(*trace);
            FIRST_RETRIES = retries;
        }
        LAST = Some(*trace);
    }
}
/// Called by `survey` once its first frame has been read.
#[inline(never)]
#[optimize(size)]
pub fn boot(gate: Option<Fault>) {
    unsafe { HK_CARD_GATE = gate.map_or(0, |f| f as u32 + 1) }
}

struct Line {
    bytes: [u8; 46],
    len: usize,
}
impl Line {
    #[inline(never)]
    #[optimize(size)]
    fn new() -> Self {
        Self {
            bytes: [b' '; 46],
            len: 0,
        }
    }
    #[inline(never)]
    #[optimize(size)]
    fn s(mut self, text: &str) -> Self {
        for b in text.bytes() {
            if self.len < 46 {
                self.bytes[self.len] = b;
                self.len += 1;
            }
        }
        self
    }
    #[inline(never)]
    #[optimize(size)]
    fn n(self, value: u32) -> Self {
        let mut d = [0u8; psx_math::fmt::U32_DEC_MAX];
        self.s(psx_math::fmt::u32_dec(&mut d, value))
    }
    #[inline(never)]
    #[optimize(size)]
    fn x(mut self, value: u32, digits: usize) -> Self {
        for i in (0..digits).rev() {
            let nibble = (value >> (4 * i)) & 15;
            let b = if nibble < 10 {
                b'0' + nibble as u8
            } else {
                b'a' + nibble as u8 - 10
            };
            if self.len < 46 {
                self.bytes[self.len] = b;
                self.len += 1;
            }
        }
        self
    }
    #[inline(never)]
    #[optimize(size)]
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}
#[inline(never)]
#[optimize(size)]
fn hex_bytes(mut line: Line, bytes: &[u8]) -> Line {
    for b in bytes {
        line = line.x(*b as u32, 2).s(" ");
    }
    line
}
#[inline(never)]
#[optimize(size)]
fn fault_name(f: TransportFault) -> &'static str {
    match f {
        TransportFault::None => "-",
        TransportFault::TxTimeout => "TX",
        TransportFault::RxTimeout => "RX",
        TransportFault::AckTimeout => "ACK",
        TransportFault::AckReleaseTimeout => "ACKREL",
    }
}
#[inline(never)]
#[optimize(size)]
fn error_name(e: Error) -> &'static str {
    match e {
        Error::NoCard => "NoCard",
        Error::Protocol => "Protocol",
        Error::BadChecksum => "BadChk",
        Error::OutOfRange => "Range",
        Error::NotFormatted => "NotFmt",
        Error::NotFound => "NotFound",
        Error::NoSpace => "NoSpace",
        Error::Corrupt => "Corrupt",
        _ => "Other",
    }
}
#[inline(never)]
#[optimize(size)]
fn result_name(r: &Result<(), Error>) -> &'static str {
    match r {
        Ok(()) => "ok",
        Err(e) => error_name(*e),
    }
}
/// `exch N ack N fault KIND@EXCHANGE` for a trace.
#[inline(never)]
#[optimize(size)]
fn trace_line(prefix: &str, t: &TransportTrace) -> Line {
    let mut line = Line::new()
        .s(prefix)
        .s(" ex")
        .n(t.exchanges as u32)
        .s(" ak")
        .n(t.acknowledgements as u32)
        .s(" f ")
        .s(fault_name(t.fault));
    if t.fault != TransportFault::None {
        line = line.s("@").n(t.fault_exchange as u32);
    }
    line
}

#[inline(never)]
#[optimize(size)]
fn port() -> ControllerPort {
    // SAFETY: a token is a logic guard (psx_io::periph); the pad sampler has
    // not started where this runs, so nothing else drives SIO0.
    unsafe { ControllerPort::steal() }
}
#[inline(never)]
#[optimize(size)]
fn now() -> u32 {
    psx_rt::interrupts::vblank_count()
}
/// One frame 63 write-back with `timing`: read it, write the same bytes, read
/// it again. Returns the result of each step and the video periods the write
/// took.
#[inline(never)]
#[optimize(size)]
fn write_back(timing: Timing) -> (Result<(), Error>, TransportTrace, u32, bool) {
    let mut card = HardwareCard::on_port_with_timing(port(), Slot::One, timing);
    let mut before = [0u8; 128];
    if let Err(e) = card.read_frame(63, &mut before) {
        return (Err(e), card.last_trace(), 0, false);
    }
    let began = now();
    let wrote = card.write_frame(63, &before);
    let took = now().wrapping_sub(began);
    let trace = card.last_trace();
    let mut after = [0u8; 128];
    let same = card.read_frame(63, &mut after).is_ok() && after == before;
    (wrote, trace, took, same)
}

/// The probe page. After the title and before the pad sampler starts, the
/// same place the audio probe runs.
#[inline(never)]
#[optimize(size)]
pub fn run(fb: &mut FrameBuffer) {
    let mut lines: [Line; 14] = core::array::from_fn(|_| Line::new());
    let mut n = 0;
    let mut put = |line: Line| {
        if n < lines.len() {
            lines[n] = line;
            n += 1;
        }
    };
    put(Line::new().s("CARD DIAG slot 1, hk-psx card-diag build"));
    // What the very first access of the boot saw.
    unsafe {
        let gate = HK_CARD_GATE;
        put(Line::new()
            .s("boot gate ")
            .n(gate)
            .s(" retries ")
            .n(FIRST_RETRIES));
        if let Some(t) = *(&raw const FIRST) {
            put(trace_line("boot", &t));
            put(hex_bytes(Line::new().s("  rx "), &t.response_prefix));
        }
    }
    // A fresh read of frame 0 now.
    let mut card = HardwareCard::on_port(port(), Slot::One);
    let mut head = [0u8; 128];
    let read = card.read_frame(0, &mut head);
    let t = card.last_trace();
    put(trace_line("now ", &t).s(" ").s(result_name(&read)));
    put(hex_bytes(Line::new().s("  rx "), &t.response_prefix));
    let formatted = read.is_ok() && head[0] == b'M' && head[1] == b'C';
    put(Line::new()
        .s("MC header ")
        .s(if formatted { "yes" } else { "NO" }));
    drop(card);
    // The write-back, with the card's default timing and then others.
    if formatted {
        let (wrote, trace, took, same) = write_back(Timing::default());
        put(trace_line("wr63", &trace)
            .s(" ")
            .s(result_name(&wrote))
            .s(" v")
            .n(took));
        put(hex_bytes(Line::new().s("  rx "), &trace.response_prefix));
        put(Line::new().s("readback ").s(if same { "same" } else { "DIFF" }));
        unsafe { HK_CARD_PROBE = if wrote.is_ok() && same { 1 } else { 2 } };
        if wrote.is_err() || !same {
            // Which other timing works, if any: the card's own setup delay,
            // then a much longer /ACK wait.
            let base = Timing::default();
            let alternatives = [
                Timing {
                    setup_spins: 8192,
                    ..base
                },
                Timing {
                    ack_spins: base.ack_spins * 10,
                    ..base
                },
                Timing {
                    setup_spins: 8192,
                    ack_spins: base.ack_spins * 10,
                    ..base
                },
            ];
            for (i, timing) in alternatives.into_iter().enumerate() {
                let (wrote, trace, took, same) = write_back(timing);
                put(trace_line("alt", &trace)
                    .n(i as u32 + 1)
                    .s(" ")
                    .s(result_name(&wrote))
                    .s(" v")
                    .n(took)
                    .s(if same { " same" } else { " DIFF" }));
                if wrote.is_ok() && same {
                    unsafe {
                        HK_CARD_PROBE = 3;
                        HK_CARD_PROBE_TIMING = i as u32 + 1;
                    }
                    break;
                }
            }
        }
    }
    put(Line::new().s("X: continue"));
    let mut text: [&str; 14] = [""; 14];
    for (slot, line) in text.iter_mut().zip(lines.iter()) {
        *slot = line.as_str();
    }
    loop {
        crate::menu::probe_screen(fb, &text[..n]);
        if crate::input::poll_bits() & psx_pad::button::CROSS != 0 {
            break;
        }
    }
    // Let go of the button before the title's own consume loop is gone.
    while crate::input::poll_bits() & psx_pad::button::CROSS != 0 {
        psx_rt::interrupts::wait_vblank();
    }
}

/// The two bottom lines of every gameplay frame, drawn with the panel font.
#[inline(never)]
#[optimize(size)]
pub fn overlay() {
    let (gpustat, dma2) = unsafe {
        (
            core::ptr::read_volatile(0x1f80_1814 as *const u32),
            core::ptr::read_volatile(0x1f80_10a8 as *const u32),
        )
    };
    let a = unsafe {
        Line::new()
            .s("v")
            .n(now())
            .s(" m")
            .n(crate::HK_GAME_MODE)
            .s(" r")
            .n(crate::HK_REGION_ID)
            .s(" u")
            .n(crate::save_ui::HK_SAVE_UI)
            .s(" w")
            .n(crate::save::HK_SAVE_WRITES)
            .s("/")
            .n(crate::save::HK_SAVE_ERRORS)
            .s(" rt")
            .n(crate::save::HK_CARD_RETRIES)
    };
    let b = unsafe {
        Line::new()
            .s("c")
            .n(crate::save::HK_CARD_FAULT)
            .s("@")
            .n(crate::save::HK_CARD_FAULT_EXCHANGE.min(999))
            .s(" f")
            .x(crate::save::HK_CARD_LAST, 5)
            .s(" t")
            .n(crate::save::HK_CARD_WRITE_VBLANKS)
            .s(" g")
            .x(gpustat, 8)
            .s(" d")
            .x(dma2, 8)
    };
    crate::dialogue::panel_text(2, 210, a.as_str());
    crate::dialogue::panel_text(2, 224, b.as_str());
}
