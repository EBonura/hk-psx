//! Brightness and screen position, set on the title's Options page.
//!
//! The GPU has no gamma, so brightness is one semi-transparent grey quad over
//! the finished frame (the HUD and panels included): subtracted to darken,
//! added to brighten, five steps either way. At step 0 nothing is added, so
//! the default frame is the frame as drawn and the setting costs nothing.
//!
//! The screen position moves the picture in the video signal, through the GPU's
//! display range (GP1 06h and 07h), the way period games recentred an image in
//! a television's overscan. VRAM and the draw offset are untouched, so nothing
//! is cropped.
//!
//! Neither is saved: like the volumes and cheats beside them they last for the
//! session (the title's page starts from what is set now). The save record is
//! per profile and written only at a bench; a display setting belongs to the
//! television, not to a save.
use psx_display::{Brightness, BrightnessOverlay};
use psx_gpu::{
    self as gpu,
    display::{DisplayConfig, Resolution, VideoMode},
    ot::OrderingTable,
    MAX_NODE_WORDS,
};

/// Brightness steps either side of the picture as drawn.
pub const BRIGHT_STEPS: i8 = Brightness::STEP_COUNT;
/// The screen position's reach either side of centre, in pixels.
pub const SCREEN_RANGE: i8 = 16;

static mut LEVEL: i8 = 0;
static mut SCREEN: (i8, i8) = (0, 0);
static mut OVERLAY: BrightnessOverlay =
    BrightnessOverlay::new(Brightness::DEFAULT, Resolution::R320X240);

/// The brightness step in force: -BRIGHT_STEPS (darker) to BRIGHT_STEPS.
pub fn brightness() -> i8 {
    unsafe { LEVEL }
}

pub fn set_brightness(level: i8) {
    unsafe { LEVEL = Brightness::new(level).level() };
}

/// Link `prim` into depth slot `z` of `ot`, `words` payload words after its tag
/// word. The table's frame is continued, not cleared.
///
/// # Safety
///
/// `prim` must stay live and unmodified, except for its tag word, until the
/// walk of `ot` has finished. Every caller passes a packet in a static.
#[inline(always)]
pub unsafe fn ot_add<T, const N: usize>(
    ot: &mut OrderingTable<N>,
    z: usize,
    prim: &mut T,
    words: u8,
) {
    const {
        assert!(
            core::mem::size_of::<T>() <= 4 * (MAX_NODE_WORDS + 1),
            "primitive larger than one GPU DMA node"
        )
    };
    // SAFETY: forwarded contract; the table is only ever walked after the
    // caller has built the whole frame.
    unsafe {
        ot.resume_frame()
            .add_raw(z, core::ptr::from_mut(prim).cast::<u32>(), words)
    };
}

/// The overlay at the front of the frame's final list, behind only the fade to
/// black: insertion prepends, so call this right after the fade and before the
/// HUD and the panels.
#[inline(never)]
pub fn append(ot: &mut OrderingTable<1>) {
    let Some(overlay) = Brightness::new(brightness()).overlay(Resolution::R320X240) else {
        return;
    };
    unsafe {
        OVERLAY = overlay;
        ot_add(ot, 0, &mut *(&raw mut OVERLAY), BrightnessOverlay::WORDS);
    }
}

/// The overlay drawn straight to the command port, for the title's screens
/// (which do not build a list). `gain` is the screen's fade, so a fade to
/// black still ends black.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn draw_direct(gain: u8) {
    let Some(overlay) = Brightness::new(brightness()).faded_overlay(Resolution::R320X240, gain) else {
        return;
    };
    psx_io::gpu::wait_command_ready();
    for word in overlay.words() {
        psx_io::gpu::write_command(word);
    }
}

/// Where the picture sits now: pixels right and down of centre.
pub fn screen() -> (i8, i8) {
    unsafe { SCREEN }
}

/// Put the picture `x` pixels right and `y` down of centre.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn set_screen(x: i8, y: i8) {
    let (x, y) = (
        x.clamp(-SCREEN_RANGE, SCREEN_RANGE),
        y.clamp(-SCREEN_RANGE, SCREEN_RANGE),
    );
    unsafe { SCREEN = (x, y) };
    let display =
        DisplayConfig::new(VideoMode::Ntsc, Resolution::R320X240).with_offset((x as i16, y as i16));
    // SAFETY: this crate predates the peripheral tokens and, like the SDK's own
    // compatibility layer, takes the GPU token per call. Only GP1 is written, from
    // the main loop, never while a list is walking.
    let mut dma = unsafe { psx_io::periph::GpuDma::steal() };
    gpu::Gpu::from_dma_mut(&mut dma).set_display(display);
}
