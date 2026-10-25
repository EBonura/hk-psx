//! The title as a boot overlay: game/src/menu.rs is the `title` code module (host/code_modules.py), which the
//! disc carries in a chunk of its own right before the title art. The title is the first thing the game shows
//! and never comes back (the pause menu's reset does not reload it), so its code is read and installed
//! into the module pool for the title and the loading screen, then evicted before the first scene is
//! admitted: its code is not linked into the resident image.
//!
//! These functions are the only resident callers into the module (module_publish.rs, code_modules.py
//! `CALLERS`). Each is a plain forwarder; calling one while the module is not installed would land on
//! `__hk_module_trap`, so `main` brackets them with `open` and `close`.
use crate::{disc::Cache, menu, modules};
use psx_gpu::framebuf::FrameBuffer;

/// Disc reads `open` tries before it gives up (each a whole read of one 20 KB chunk).
const ATTEMPTS: usize = 4;

/// Read the title's code and install it, after the title art was staged (`Cache::menu_art`). False when
/// the disc would not give it, in which case nothing of the title can be shown.
#[inline(never)]
pub fn open(cache: &mut Cache) -> bool {
    for _ in 0..ATTEMPTS {
        if modules::loaded(modules::TITLE) || cache.title_code().is_ok() {
            return true;
        }
        psx_rt::interrupts::wait_vblank();
    }
    false
}

/// Evict the title's code: the pool is the rooms' again.
#[inline(never)]
pub fn close() {
    modules::evict_boot(modules::TITLE);
}

/// `menu::run`: the title, its pages and the save slot chooser.
#[inline(never)]
pub fn run(
    fb: &mut FrameBuffer,
    art: Option<&[u8]>,
    slots: &[&str; 4],
    fault: &str,
) -> (menu::Settings, usize) {
    menu::run(fb, art, slots, fault)
}

/// `menu::loading`: the title art with its loading prompt, while the first scene is read.
#[inline(never)]
pub fn loading(fb: &mut FrameBuffer) {
    menu::loading(fb);
}

/// `menu::retry`: the title art with its retry prompt, until START or Cross is pressed again.
#[inline(never)]
pub fn retry(fb: &mut FrameBuffer) {
    menu::retry(fb);
}

/// `menu::restore`: the title art and glyphs back in video memory after a failed scene load.
#[inline(never)]
pub fn restore(art: Option<&[u8]>) {
    menu::restore(art);
}
