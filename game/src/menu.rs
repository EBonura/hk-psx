//! Boot-only functional menu with original title assets and Perpetua glyphs.
//! Scene admission reuses its title texture storage; no gameplay allocation.
use crate::input::poll_bits;
use psx_gpu::{self as gpu, framebuf::FrameBuffer, material::TextureMaterial};
use psx_pad::button;
use psx_vram::{upload_bytes, Clut, TexDepth, Tpage, VramRect};

#[path = "menu_state.rs"]
pub mod state;
pub use state::Settings;
use state::{Page, State};
#[no_mangle]
pub static mut HK_MENU_PAGE: u32 = 0;
#[no_mangle]
pub static mut HK_MENU_ROW: u32 = 0;
#[no_mangle]
pub static mut HK_MENU_SFX: u32 = 10;
#[no_mangle]
pub static mut HK_MENU_AMBIENCE: u32 = 10;
#[no_mangle]
pub static mut HK_MENU_MUSIC: u32 = 10;

// The title art is a disc chunk (disc::Cache::menu_art), read into the scene
// arena before the title and uploaded to VRAM; only its size, checksum and the
// glyph advances stay linked.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/menu.rs"));
/// Whether the title art reached VRAM: a retry screen whose read failed draws
/// no background rather than whatever a scene left in those pages.
static mut ART: bool = false;
const HEADER: usize = 16;
const PALETTE: usize = 512;
const LEFT: usize = 256 * 240;
const RIGHT: usize = 64 * 240;
const PATCH: usize = 192 * 32;
const METRICS: usize = HEADER + PALETTE + LEFT + RIGHT + 2 * PATCH;
const _: () = assert!(MENU_BYTES == METRICS + 96);
const LEVELS: [&str; 11] = [
    "0%", "10%", "20%", "30%", "40%", "50%", "60%", "70%", "80%", "90%", "100%",
];

fn draw(gain: u8) {
    if !unsafe { ART } {
        return;
    }
    let clut = Clut::new(768, 480).uv_clut_word();
    for (screen_x, vram_x, width) in [(0, 384, 256), (256, 512, 64)] {
        let page = Tpage::new(vram_x, 0, TexDepth::Bit8).uv_tpage_word(0);
        gpu::draw_sprite_material(
            screen_x,
            0,
            width,
            240,
            (0, 0),
            TextureMaterial::opaque(clut, page, (gain, gain, gain)),
        );
    }
}

fn text_width(value: &str) -> i16 {
    value
        .bytes()
        .map(|b| MENU_METRICS[(b - 32) as usize] as i16)
        .sum()
}
fn text(x: i16, y: i16, value: &str, gain: u8) {
    let mut x = x;
    for b in value.bytes() {
        assert!((32..=126).contains(&b));
        let i = (b - 32) as usize;
        if b != b' ' {
            gpu::draw_sprite_material(
                x,
                y,
                12,
                12,
                ((i % 21 * 12) as u8, 244),
                TextureMaterial::opaque(
                    Clut::new(320, 481).uv_clut_word(),
                    Tpage::new((i / 21 * 64) as u16, 256, TexDepth::Bit4).uv_tpage_word(0),
                    (gain, gain, gain),
                ),
            );
        }
        x += MENU_METRICS[i] as i16;
    }
}
fn centered(y: i16, value: &str, gain: u8) {
    text(160 - text_width(value) / 2, y, value, gain);
}
pub const SLOT_LABELS: [&str; 4] = ["Slot 1", "Slot 2", "Slot 3", "Slot 4"];
// For size, like `run`: the title is not a gameplay frame, and the resident code
// budget (docs/BUDGET.md) is what the Options rows would otherwise be spent from.
#[cfg_attr(not(test), optimize(size))]
fn draw_menu(menu: &State, gain: u8, slots: &[&str; 4], fault: &str) {
    draw(gain);
    match menu.page {
        Page::Main => {
            for (i, label) in state::MAIN_ITEMS.iter().enumerate() {
                let y = 145 + i as i16 * 17;
                let g = if i == menu.selected {
                    gain
                } else {
                    gain - gain / 4
                };
                centered(y, label, g);
                if i == menu.selected {
                    text(144 - text_width(label) / 2, y, ">", gain);
                }
            }
            centered(222, "Up/Down: choose    X/START: select", gain);
        }
        Page::Profiles => {
            centered(120, "SELECT SAVE", gain);
            for i in 0..crate::save::PROFILES {
                let y = 142 + i as i16 * 18;
                let g = if i == menu.selected {
                    gain
                } else {
                    gain - gain / 4
                };
                text(78, y, SLOT_LABELS[i], g);
                text(150, y, slots[i], g);
                if i == menu.selected {
                    text(62, y, ">", gain);
                }
            }
            centered(
                218,
                if fault.is_empty() {
                    "X: select    O: back"
                } else {
                    fault
                },
                gain,
            );
        }
        Page::Options => {
            // No heading: the logo's flourish reaches y 142, and seven rows need the space under it.
            for (i, label) in state::OPTION_ITEMS.iter().enumerate() {
                let y = 134 + i as i16 * 12;
                let g = if i == menu.selected {
                    gain
                } else {
                    gain - gain / 4
                };
                text(86, y, label, g);
                let mut sign = [0u8; 3];
                let s = &menu.settings;
                let value = match i {
                    0..=2 => LEVELS[[s.sfx, s.ambience, s.music][i] as usize],
                    3..=5 => {
                        state::signed([s.brightness, s.screen_x, s.screen_y][i - 3], &mut sign)
                    }
                    _ => "",
                };
                if !value.is_empty() {
                    text(215, y, value, g);
                }
                if i == menu.selected {
                    text(70, y, ">", gain);
                }
            }
            centered(221, "Left/Right: adjust    O: back", gain);
        }
        Page::Controls => {
            let border = gain / 2;
            gpu::draw_rect_flat(16, 39, 288, 192, border, border, border);
            gpu::draw_rect_flat(18, 41, 284, 188, 0, 0, 0);
            centered(51, "CONTROLS", gain);
            for (i, line) in state::CONTROL_LINES.iter().enumerate() {
                text(30, 66 + i as i16 * 12, line, gain);
            }
            centered(214, "X / O / START: back", gain);
        }
        Page::Cheats => {
            gpu::draw_rect_flat(16, 45, 288, 187, gain / 2, gain / 2, gain / 2);
            gpu::draw_rect_flat(18, 47, 284, 183, 0, 0, 0);
            centered(65, "CHEATS", gain);
            for (i, label) in state::CHEAT_ITEMS.iter().chain(["Back"].iter()).enumerate() {
                let y = 68 + i as i16 * 11;
                let g = if i == menu.selected {
                    gain
                } else {
                    gain - gain / 4
                };
                text(46, y, label, g);
                if i < state::CHEAT_ITEMS.len() {
                    text(
                        254,
                        y,
                        if menu.settings.cheats.enabled(i) {
                            "ON"
                        } else {
                            "OFF"
                        },
                        g,
                    );
                }
                if i == menu.selected {
                    text(30, y, ">", gain);
                }
            }
            centered(204, "Session only. More in Pause > Cheats", gain);
            centered(218, "X: toggle    Left/Right: off/on    O: back", gain);
        }
    }
    // BRIGHTNESS over the whole screen, text included (display.rs).
    crate::display::draw_direct(gain);
}
fn present_samples(fb: &mut FrameBuffer, mut sample: impl FnMut(u16)) {
    // The menu draws through the command port; GP0(1Fh) after it is what lets
    // psx-rt apply the flip, once the GPU has drawn everything before it.
    gpu::arm_draw_done();
    gpu::signal_draw_done();
    psx_rt::interrupts::queue_gp1_at_vblank(fb.begin_deferred_swap());
    let mut waited = 0u32;
    while psx_rt::interrupts::gp1_queue_pending() {
        psx_rt::interrupts::wait_vblank();
        // Process each real sample in order, even if a frame spans several blanks.
        sample(poll_bits());
        waited += 1;
        if waited >= crate::presentation::FLIP_TIMEOUT_VBLANKS {
            crate::presentation::force_flip();
        }
    }
    fb.apply_draw_target();
}
fn present(fb: &mut FrameBuffer) -> u16 {
    let mut pressed = 0;
    present_samples(fb, |bits| pressed |= bits);
    pressed
}
/// First START/Cross still directly enters gameplay. Other choices are exposed
/// with D-pad navigation. Settings apply immediately and persist for this run.
/// Returns the profile the player confirmed on the save screen.
#[inline(never)] // Keep title rendering outside main's MIPS PC16 branch span.
#[cfg_attr(not(test), optimize(size))]
pub fn run(
    fb: &mut FrameBuffer,
    art: Option<&[u8]>,
    slots: &[&str; 4],
    fault: &str,
) -> (Settings, usize) {
    restore(art);
    crate::music::begin();
    let mut menu = State::new();
    let mut gain = 0u8;
    // The page starts from what is set now: the title comes back after a session reset.
    menu.settings.brightness = crate::display::brightness();
    (menu.settings.screen_x, menu.settings.screen_y) = crate::display::screen();
    loop {
        gain = gain.saturating_add(8).min(128);
        draw_menu(&menu, gain, slots, fault);
        let mut start = false;
        present_samples(fb, |bits| {
            crate::music::tick();
            if start {
                return;
            }
            let old = menu.settings;
            let (page, row) = (menu.page, menu.selected);
            start = menu.step(bits);
            // MenuAudioController: startGame when a save is chosen, submit or
            // cancel (one clip) when a button changes the page, select for the
            // cursor and slider for an option step.
            if start {
                crate::audio::ui_start();
            } else if menu.page != page {
                crate::audio::ui_confirm();
            } else if menu.selected != row {
                crate::audio::ui_select();
            } else if old != menu.settings {
                crate::audio::ui_slider();
            }
            if old.sfx != menu.settings.sfx {
                crate::audio::set_volume(menu.settings.sfx);
            }
            if old.ambience != menu.settings.ambience {
                crate::ambience::set_volume(menu.settings.ambience);
            }
            if old.music != menu.settings.music {
                crate::music::set_volume(menu.settings.music);
            }
            if old.brightness != menu.settings.brightness {
                crate::display::set_brightness(menu.settings.brightness);
            }
            if (old.screen_x, old.screen_y) != (menu.settings.screen_x, menu.settings.screen_y) {
                crate::display::set_screen(menu.settings.screen_x, menu.settings.screen_y);
            }
            unsafe {
                HK_MENU_PAGE = match menu.page {
                    Page::Main => 0,
                    Page::Options => 1,
                    Page::Controls => 2,
                    Page::Cheats => 3,
                    Page::Profiles => 4,
                };
                HK_MENU_ROW = menu.selected as u32;
                HK_MENU_SFX = menu.settings.sfx as u32;
                HK_MENU_AMBIENCE = menu.settings.ambience as u32;
                HK_MENU_MUSIC = menu.settings.music as u32;
            }
        });
        if start {
            break;
        }
    }
    while gain > 0 {
        gain = gain.saturating_sub(16);
        crate::music::set_fade(gain);
        draw_menu(&menu, gain, slots, fault);
        present_samples(fb, |_| crate::music::tick());
    }
    assert!(crate::music::stop(), "CD music did not release drive");
    // Consume the activating hold so Cross cannot also cause a gameplay jump.
    while poll_bits() & (button::START | button::CROSS) != 0 {
        psx_rt::interrupts::wait_vblank();
    }
    (menu.settings, menu.profile.unwrap_or(0))
}

fn status(fb: &mut FrameBuffer, patch: u8) -> u16 {
    draw(128);
    gpu::draw_sprite_material(
        64,
        177,
        192,
        32,
        (64, patch * 32),
        TextureMaterial::opaque(
            Clut::new(768, 480).uv_clut_word(),
            Tpage::new(512, 0, TexDepth::Bit8).uv_tpage_word(0),
            (128, 128, 128),
        ),
    );
    present(fb)
}

/// One screen of probe results over the dimmed title art (audio_probe.rs).
#[cfg(feature = "audio-probe")]
pub fn probe_screen(fb: &mut FrameBuffer, lines: &[&str]) {
    draw(40);
    for (row, line) in lines.iter().enumerate() {
        text(16, 16 + row as i16 * 15, line, 128);
    }
    let _ = present(fb);
}

/// Keep the title visible while the blocking room read completes.
pub fn loading(fb: &mut FrameBuffer) {
    let _ = status(fb, 0);
}

/// A failed read can be retried after releasing the previous activating press.
pub fn retry(fb: &mut FrameBuffer) {
    let mut released = false;
    loop {
        let pressed = status(fb, 1) & (button::START | button::CROSS) != 0;
        if released && pressed {
            return;
        }
        released |= !pressed;
    }
}

/// Upload the title art (`None` when its read failed) and the glyphs, at boot
/// and after a failed in-game region load. The glyph sheet rides the same
/// disc chunk, right after the title art; a failed read leaves the glyphs an
/// earlier upload put in VRAM.
pub fn restore(art: Option<&[u8]>) {
    unsafe {
        ART = art.is_some();
    }
    let Some(data) = art else { return };
    upload_font(&data[MENU_BYTES..MENU_BYTES + FONT_BYTES]);
    let data = &data[..MENU_BYTES];
    assert!(data.len() == METRICS + 96);
    assert!(&data[..8] == b"HKMENU03");
    assert!(data[8..12] == [64, 1, 240, 0]);
    assert!(
        u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize == LEFT + RIGHT + 2 * PATCH
    );
    upload_bytes(
        VramRect::new(768, 480, 256, 1),
        &data[HEADER..HEADER + PALETTE],
    );
    assert!(
        data[METRICS..METRICS + 95]
            .iter()
            .all(|&n| (1..=12).contains(&n))
            && data[METRICS + 95] == 0
    );
    let pixels = &data[HEADER + PALETTE..METRICS];
    upload_bytes(VramRect::new(384, 0, 128, 240), &pixels[..LEFT]);
    upload_bytes(VramRect::new(512, 0, 32, 240), &pixels[LEFT..LEFT + RIGHT]);
    upload_bytes(VramRect::new(544, 0, 96, 64), &pixels[LEFT + RIGHT..]);
}
/// The Perpetua glyph sheet's size, which the dialogue panels share.
pub const FONT_BYTES: usize = 7712;
fn upload_font(font: &[u8]) {
    // Reuse the exact original Perpetua glyph sheet and its gameplay reservation.
    // No new font asset or VRAM region is introduced by the menu.
    assert_eq!(font.len(), FONT_BYTES);
    upload_bytes(VramRect::new(320, 481, 16, 1), &font[..32]);
    for page in 0..5 {
        upload_bytes(
            VramRect::new(page * 64, 500, 64, 12),
            &font[32 + page as usize * 1536..32 + (page as usize + 1) * 1536],
        );
    }
}
