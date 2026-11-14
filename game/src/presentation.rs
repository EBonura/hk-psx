//! Main-thread presentation through the SDK's presenter (psx_rt::present, sdk/docs/PRESENT.md).
//!
//! A frame is handed over as DMA chains: every chunk the renderer kicks while it builds is a
//! `Part::More`, the final list (HUD and dialogue on top, ending on GP0(1Fh)) the `Part::Final`
//! that carries the display word. The presenter links the draw area, offset and clear in front of
//! the first part, starts a part as soon as the frame before is on screen and the channel is idle
//! (here from `checkpoint`, else the VBlank handler), and flips at the first edge the 30 fps cap
//! allows. The main thread never waits for it: cooperative checkpoints kick, simulation ticks
//! and input run in the waits (`submit_final`, `present_spin`).
//!
//! Phase of the frame in hand: not submitted, final list handed over (its walk may still run),
//! and drained (the walk has ended, so CPU-only work may reuse packet RAM while the GPU finishes
//! and the flip waits for its edge).
use psx_gpu::display::DoubleBuffer;
use psx_rt::present::{self, Part, PresentCap, Presenter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Submitted,
    Drained,
}
static mut PHASE: Phase = Phase::Idle;
static mut PRESENTER: Option<Presenter> = None;
static mut FB: *mut DoubleBuffer = core::ptr::null_mut();

/// Take over presentation: once, after `install_vblank_counter`, with the GPU idle. `fb` is the
/// program's double buffer; the presenter is the only thing that moves its draw side. Every
/// frame is cleared to the sky colour in front of its first part. Hollow Knight runs at 30 fps by
/// the owner's decision: a cap of two VBlanks, set by name.
pub fn start(fb: &mut DoubleBuffer) {
    unsafe {
        FB = fb;
        PRESENTER = Some(Presenter::new(Some((9, 13, 20))));
    }
    present::set_cap(PresentCap::HZ30);
}
fn presenter() -> &'static mut Presenter {
    // SAFETY: set once by `start`; the main thread is the only caller.
    unsafe { (*(&raw mut PRESENTER)).as_mut().expect("presentation not started") }
}
/// The program's double buffer.
fn buffers() -> &'static mut DoubleBuffer {
    unsafe { &mut *FB }
}

/// A frame or a display word is handed over and not on screen yet: the replacement of "a display
/// word is queued", for the places that need nothing of the frame in flight touched.
#[inline(always)]
pub fn pending() -> bool {
    present::is_slot_full() || present::is_flip_pending()
}
/// Hand over one chunk of the frame being built. The caller checked that the slot is empty.
pub fn submit_more(head: *const u32) {
    // SAFETY: the chunk's table and packets stay untouched until its walk has ended
    // (`render::begin_frame` runs after the frame is drained).
    unsafe { presenter().submit(buffers(), head, Part::More) };
}
/// Hand over the final list. Waits only while an earlier chunk still waits in the slot, running
/// the checkpoints.
pub fn submit_final(head: *const u32) {
    while present::is_slot_full() {
        crate::input::checkpoint();
        present::kick();
    }
    unsafe {
        PHASE = Phase::Submitted;
        KICK_NOTED = false;
        presenter().submit(buffers(), head, Part::Final);
    }
}
/// Start the part in the slot when it can go, and note when the final list has drained.
#[inline(always)]
pub fn checkpoint() {
    if present::is_slot_full() {
        present::kick();
    }
    if unsafe { PHASE == Phase::Submitted } {
        service();
    }
}
static mut KICK_NOTED: bool = false;
#[inline(never)]
fn service() {
    // The final list leaves the slot when it starts: the line it started on (the policy of
    // `render::begin_frame` reads the walk's length from it).
    if unsafe { !KICK_NOTED } && !present::is_slot_full() {
        unsafe {
            KICK_NOTED = true;
            crate::render::HK_FRAME_LAST_KICK_LINES = crate::render::frame_lines();
        }
    }
    // Not drained while a part waits or the channel still walks: an earlier linked list can own
    // packet memory even when the GPU's FIFO reports ready.
    if present::is_slot_full() || crate::render::dma_pending() {
        return;
    }
    unsafe {
        PHASE = Phase::Drained;
        let now = psx_rt::interrupts::vblank_count();
        // The vblanks between this frame's drain and the last one's.
        HK_FRAME_VBLANKS = now.wrapping_sub(QUEUED_AT);
        QUEUED_AT = now;
        crate::render::HK_FRAME_FLIP_LINES = crate::render::frame_lines();
    }
}
/// The final list has drained: the frame's walk has ended and its flip waits (or has landed).
#[inline(always)]
pub fn queued() -> bool {
    unsafe { PHASE == Phase::Drained }
}
/// Retire the frame once its flip has landed.
pub fn finish() {
    unsafe {
        assert!(PHASE == Phase::Drained && !present::is_flip_pending(), "presentation incomplete");
        PHASE = Phase::Idle;
    }
}
/// VBlanks a flip may wait for its GPU before it is written by hand. The presenter recovers a
/// stalled walk and a missing GP0(1Fh) itself; this stays for a frame that heavy.
pub const FLIP_TIMEOUT_VBLANKS: u32 = 30;
#[no_mangle]
pub static mut HK_FLIP_TIMEOUTS: u32 = 0;
static mut QUEUED_AT: u32 = 0;
/// Vblanks the last presented frame took (drain to drain); the renderer reads it to see which
/// resource a slow frame ran out of (render::begin_frame).
#[no_mangle]
pub static mut HK_FRAME_VBLANKS: u32 = 0;
/// The main loop's flip has waited FLIP_TIMEOUT_VBLANKS for its GPU.
pub fn flip_overdue() -> bool {
    queued()
        && present::is_flip_pending()
        && psx_rt::interrupts::vblank_count().wrapping_sub(unsafe { QUEUED_AT }) >= FLIP_TIMEOUT_VBLANKS
}
/// Take the pending display word back from the presenter and write it now. Racing the VBlank
/// handler is harmless: it writes the same idempotent GP1 word.
pub fn force_flip() {
    let word = present::cancel_flip();
    if word != 0 {
        psx_io::gpu::write_display_control(word);
        unsafe {
            HK_FLIP_TIMEOUTS = HK_FLIP_TIMEOUTS.wrapping_add(1);
        }
    }
}

/// Turn the display on or off at a gate. The presenter's own call waits for the slot and the last
/// flip with no checkpoints, which would miss pad polls; the checkpoints run here instead.
pub fn set_display_enabled(on: bool) {
    while pending() {
        crate::input::checkpoint();
        present::kick();
    }
    present::set_display_enabled(on);
}

// Menus, title, loading screens and the gates draw through the command port.
/// Start a frame drawn through the port: wait until the one handed over last is on screen, then
/// point the draw area at the draw side. Once before a screen's first draw, and after each
/// `present_drawn`.
pub fn begin_drawn() {
    presenter().begin_frame(buffers());
}
/// Present a frame drawn through the port and return at once; the flip lands at its edge.
pub fn present_drawn() {
    presenter().present_drawn(buffers());
}
