//! Boot-only functional menu with original title assets and Perpetua glyphs.
//! Scene admission reuses its title texture storage; no gameplay allocation.
use crate::input::poll_bits;
use psx_gpu::{
    self as gpu,
    framebuf::FrameBuffer,
    material::{BlendMode, TextureMaterial},
};
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
/// Test discs only (`route-input`): held together while the save slot is chosen, the four shoulder
/// buttons start a new game on the ledge instead of with the drop from the top gate (main.rs). The
/// recorded routes (tools/tapes) press them so they begin where they were recorded. A shipping build
/// does not read them.
#[cfg(feature = "route-input")]
const SKIP_DROP: u16 = button::L1 | button::R1 | button::L2 | button::R2;
#[cfg(feature = "route-input")]
static mut SKIP_DROP_HELD: bool = false;
#[cfg(feature = "route-input")]
pub fn skip_drop() -> bool {
    unsafe { SKIP_DROP_HELD }
}
#[cfg(not(feature = "route-input"))]
pub const fn skip_drop() -> bool {
    false
}
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
// The title's ornaments (host/hk-cook/src/dialogue_art.rs): the two flourishes the text
// panel uses and the menu pointer, uploaded with the title art as one 4bpp strip that the
// scene art overwrites afterwards.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/menu_ornaments.rs"));
/// Where the strip sits: the title art's own x range, below its status patches, on the
/// 4bpp page at x 576. Texel rows of each piece are whole halfwords.
const ORN_X: u16 = 576;
const ORN_TOP: u16 = 64;
const ORN_BOTTOM: u16 = ORN_TOP + TITLE_FLEUR_TOP.2 as u16 + 2;
const ORN_POINTER: u16 = ORN_BOTTOM + TITLE_FLEUR_BOTTOM.2 as u16 + 2;
/// The background particles' smoke frames and soft dot, under the pointer's row.
const ORN_SMOKE: u16 = ORN_POINTER + TITLE_POINTER.2 as u16 + 2;
const ORN_DOT: u16 = ORN_SMOKE + TITLE_SMOKE.2 as u16 + 2;
const fn halfwords(width: u8) -> u16 {
    (width as u16).div_ceil(4)
}
const _: () = assert!(halfwords(TITLE_FLEUR_TOP.1) <= 64);
const _: () = assert!(ORN_DOT as usize + TITLE_DOT.2 as usize <= 256);
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
#[inline(never)]
fn text(x: i16, y: i16, value: &str, gain: u8) {
    let mut x = x;
    for b in value.bytes() {
        assert!((32..=126).contains(&b));
        let i = (b - 32) as usize;
        if b != b' ' {
            gpu::draw_sprite_material(
                x,
                y,
                state::GLYPH_W as u16,
                state::GLYPH_H as u16,
                (
                    (state::GLYPH_U0 + i % state::GLYPH_PER_ROW * state::GLYPH_W) as u8,
                    (state::FONT_V0 + i / state::GLYPH_PER_ROW * state::GLYPH_ROW_PITCH) as u8,
                ),
                // Adding: the glyph palette's entries are semi-transparent (font_strip.rs).
                TextureMaterial::blended(
                    Clut::new(320, 481).uv_clut_word(),
                    Tpage::new(state::FONT_X, state::FONT_PAGE_Y, TexDepth::Bit4).uv_tpage_word(0),
                    (gain, gain, gain),
                    BlendMode::Add,
                ),
            );
        }
        x += MENU_METRICS[i] as i16;
    }
}
#[inline(never)]
fn centered(y: i16, value: &str, gain: u8) {
    text(160 - text_width(value) / 2, y, value, gain);
}
/// A space between capitals.
const CAPS_SPACE: i16 = 6;
fn caps_index(b: u8) -> Option<usize> {
    state::CAPS_CHARS.iter().position(|&c| c == b.to_ascii_uppercase())
}
/// The width of `value` in capitals (TrajanPro-Bold, as the source's buttons and headings).
fn caps_width(value: &str) -> i16 {
    value
        .bytes()
        .map(|b| caps_index(b).map_or(CAPS_SPACE, |i| state::CAPS_ADVANCES[i] as i16))
        .sum()
}
/// `value` upper cased in the capitals; the cell is 12 texels tall and its cap line sits where a
/// 14 texel body glyph's does when drawn one row higher.
#[inline(never)]
fn caps(x: i16, y: i16, value: &str, gain: u8) {
    let mut x = x;
    for b in value.bytes() {
        let Some(i) = caps_index(b) else {
            x += CAPS_SPACE;
            continue;
        };
        let (col, row) = (i % state::CAPS_PER_ROW, i / state::CAPS_PER_ROW);
        gpu::draw_sprite_material(
            x,
            y,
            state::CAPS_W as u16,
            state::CAPS_H as u16,
            (
                (state::GLYPH_U0 + col * state::CAPS_W) as u8,
                (state::FONT_V0 + state::CAPS_V0 + row * state::CAPS_H) as u8,
            ),
            TextureMaterial::blended(
                Clut::new(320, 481).uv_clut_word(),
                Tpage::new(state::FONT_X, state::FONT_PAGE_Y, TexDepth::Bit4).uv_tpage_word(0),
                (gain, gain, gain),
                BlendMode::Add,
            ),
        );
        x += state::CAPS_ADVANCES[i] as i16;
    }
}
#[inline(never)]
fn centered_caps(y: i16, value: &str, gain: u8) {
    caps(160 - caps_width(value) / 2, y, value, gain);
}
/// One ornament from the strip, drawn 1:1 at (`x`, `y`): `u`, `v` are its texel position on
/// the strip's 4bpp page.
#[inline(never)]
fn ornament(x: i16, y: i16, size: (u8, u8), uv: (u8, u16), gain: u8) {
    gpu::draw_sprite_material(
        x,
        y,
        size.0 as u16,
        size.1 as u16,
        (uv.0, uv.1 as u8),
        TextureMaterial::opaque(
            Clut::new(320, 481).uv_clut_word(),
            Tpage::new(ORN_X, 0, TexDepth::Bit4).uv_tpage_word(0),
            (gain, gain, gain),
        ),
    );
}
/// The menu pointer, as the source's title menu shows it: a leaf left of the chosen row pointing
/// right at it and its mirror right of the row pointing left. The cooked sprite points left, so the
/// left leaf is the mirrored half of the strip.
/// The left leaf alone, for the lists whose rows start at a fixed column.
fn pointer(x: i16, y: i16, gain: u8, step: u8) {
    leaf(x, y, gain, step, true);
}
fn pointers(left: i16, right: i16, y: i16, gain: u8, step: u8) {
    leaf(left - TITLE_POINTER.1 as i16 - 4, y, gain, step, true);
    leaf(right + 4, y, gain, step, false);
}
/// One leaf: the left one (pointing right) or the right one (pointing left).
#[inline(never)]
fn leaf(x: i16, y: i16, gain: u8, step: u8, left: bool) {
    if step == HIDDEN {
        return;
    }
    let (w, h) = (TITLE_POINTER.1, TITLE_POINTER.2);
    ornament(x, y - 1, (w, h), (pointer_u(left, step), ORN_POINTER), gain);
}
/// A pointer step for "not shown yet".
const HIDDEN: u8 = 255;
/// A smoke frame's side in texels.
const SMOKE_FRAME_PX: u8 = 48;
/// U of the pointer's frame `step` on the strip: 0 to 4 are the opening frames of the source's
/// `Menu Fleur Reveal` (every second sprite frame, a 60 Hz screen's share of its 120 fps), 5 and
/// more the final leaf. The left leaf is the mirrored half, which points right.
fn pointer_u(left: bool, step: u8) -> u8 {
    let w = halfwords(TITLE_POINTER.1) * 4;
    let slot = match (left, step) {
        (true, 0..=4) => 7 + u16::from(step),
        (false, 0..=4) => 2 + u16::from(step),
        (true, _) => 1,
        (false, _) => 0,
    };
    (w * slot) as u8
}
/// A well mixed word of two.
fn mix(a: u32, b: u32) -> u32 {
    let h = (a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b)).wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 13)
}
/// One particle slot's moment: how far into its life (frames), its life, and its random word. A slot
/// is born again as soon as it dies with a new word, so nothing is kept between frames.
fn moment(slot: u32, base: u32, step: u32, now: u32) -> (i32, i32, u32) {
    let life = base + slot * step;
    let t = now + slot * 37;
    ((t % life) as i32, life as i32, mix(slot, t / life))
}
/// Strength 0 to 256 over a life: up over the first `rise` of 256, down over the last after `fall`.
fn envelope(age: i32, life: i32, rise: i32, fall: i32) -> i32 {
    let a = age * 256 / life;
    if a < rise {
        a * 256 / rise
    } else if a > fall {
        (256 - a) * 256 / (256 - fall)
    } else {
        256
    }
}
/// One soft blob of the particle strip taken away from the picture: a `w` by `h` quad whose texture
/// is `size` texels from (`u`, `v`), `gain` of 128 strong.
#[inline(never)]
fn blob(x: i32, y: i32, w: i32, h: i32, uv: (u8, u8), size: (u8, u8), gain: i32) {
    if gain <= 0 || w <= 0 || h <= 0 || x + w <= 0 || x >= 320 || y + h <= 0 || y >= 240 {
        return;
    }
    let g = gain.min(255) as u8;
    let (x0, y0, x1, y1) = (x as i16, y as i16, (x + w) as i16, (y + h) as i16);
    let (u0, v0) = uv;
    let (u1, v1) = (u0 + size.0 - 1, v0 + size.1 - 1);
    gpu::draw_quad_textured_material(
        [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        TextureMaterial::blended(
            Clut::new(320, 481).uv_clut_word(),
            Tpage::new(ORN_X, 0, TexDepth::Bit4).uv_tpage_word(0),
            (g, g, g),
            BlendMode::Subtract,
        ),
    );
}
/// The Voidheart title's background (level1 Menu_Styles/VHeart_menu_Style/abyss particles), through the
/// camera at (13.99, 11.86, -38.1) with a 24 degree field: `wispy smoke` (8 puffs of 2 to 5 s, 8 to 24
/// units wide, black smoke whose frame advances over its life, rising from below the screen and
/// drifting left at 0.3 to 4 units a second, faded in over the first eighth and out over the last
/// sixth), `FG` (32 specks of 2 to 4 s, 0.2 to 0.6 units, rising on a gravity of -0.05 near the
/// bottom centre) and `immediate_BG` (46 specks 2 px across further back). Black and Multiply in the
/// source, here subtracted from the picture in proportion to their coverage. Positions follow from
/// the projection (5.5 px a unit at the smoke's depth, 14 at the FG's, 5.3 at the BG's) and each slot
/// is stateless (`moment`).
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn particles(gain: u8) {
    let now = psx_rt::interrupts::vblank_count();
    let strength = |level: i32, base: i32| level * base * i32::from(gain) / (256 * 128);
    // The wispy smoke.
    for i in 0..8u32 {
        let (age, life, h) = moment(i, 150, 20, now);
        let r = |shift: u32| ((h >> shift) & 255) as i32;
        // Depth: 0.77 to 1.43 of the screen's scale at the emitter's.
        let f = 197 + r(8) * 169 / 256;
        let size = (44 + r(0) * 89 / 256) * f >> 8;
        let speed = (6 + r(24) * 88 / 256) * f >> 8;
        let x = 160 + ((10 + (r(16) - 128) * 20 / 128) * f >> 8) - (speed * age >> 8);
        let y = 120 + (135 * f >> 8);
        let frame = (age * 5 / life) as u8;
        blob(
            x - size / 2,
            y - size / 2,
            size,
            size,
            (frame * SMOKE_FRAME_PX, ORN_SMOKE as u8),
            (SMOKE_FRAME_PX, SMOKE_FRAME_PX),
            strength(envelope(age, life, 31, 215), 80),
        );
    }
    // The specks: the foreground ones (`FG`) and the back ones (`immediate_BG`), both on the source's
    // size and strength curves. Per group: first slot, count, life step, then the start point and its
    // spread in x and y, the sideways speed (1/256 px a frame), the rise (1/65536 px a frame squared)
    // and the size in px (least, spread; 0 spread is a fixed 2 px fleck).
    const SPECKS: [[i32; 11]; 2] = [
        [8, 32, 4, 150, 30, 252, 18, 12, 63, 3, 6],
        [40, 46, 3, 147, 11, 189, 7, 0, 14, 2, 0],
    ];
    for [first, count, step, x0, xr, y0, yr, vx, rise, least, spread] in SPECKS {
        for i in 0..count as u32 {
            let (age, life, h) = moment(i + first as u32, 120, step as u32, now);
            let r = |shift: u32| ((h >> shift) & 255) as i32 - 128;
            let a = age * 256 / life;
            let grow = if spread == 0 {
                256
            } else if a < 36 {
                a * 256 / 36
            } else if a > 230 {
                (256 - a) * 256 / 26
            } else {
                256
            };
            let size = (least + (r(24) + 128) * spread / 256) * grow >> 8;
            let x = x0 + r(0) * xr / 128 + (r(8) * vx / 128 * age >> 8);
            let y = y0 + r(16) * yr / 128 - (age * age * rise >> 16);
            blob(x - size / 2, y - size / 2, size, size, (0, ORN_DOT as u8), (8, 8), strength(envelope(age, life, 20, 240), 96));
        }
    }
}
/// The flourishes above and below a page, centred, each unfurled to `top` / `bottom` of 128. The
/// source swaps nine sprite frames for the unfurl (a bud, then the wings spreading, then the long
/// lines); here the finished flourish is shown through a window that widens from the middle,
/// which keeps the one frame in video memory.
fn flourishes(top: i16, bottom: i16, lv: Levels, gain: u8) {
    let (t, b) = (TITLE_FLEUR_TOP, TITLE_FLEUR_BOTTOM);
    unfurl(top, (t.1, t.2), ORN_TOP, lv.top, gain);
    unfurl(bottom, (b.1, b.2), ORN_BOTTOM, lv.bottom, gain);
}
#[inline(never)]
fn unfurl(y: i16, size: (u8, u8), v: u16, level: u8, gain: u8) {
    let w = (u16::from(size.0) * u16::from(level) / 128) as u8;
    if w == 0 {
        return;
    }
    let cut = (size.0 - w) / 2;
    ornament(160 - i16::from(size.0) / 2 + i16::from(cut), y, (w, size.1), (cut, v), gain);
}
pub const SLOT_LABELS: [&str; 4] = ["Slot 1", "Slot 2", "Slot 3", "Slot 4"];
/// How far each part of a page has come, 0 to 128: the heading, the body, and
/// the two flourishes (as `UIManager.ShowMenu` and `HideMenu` order them, at 60 frames a second).
#[derive(Clone, Copy)]
struct Levels {
    title: u8,
    content: u8,
    top: u8,
    bottom: u8,
}
const FULL: Levels = Levels { title: 128, content: 128, top: 128, bottom: 128 };
/// 0 to 128 over `len` frames from `start`.
#[inline(never)]
fn ramp(age: u16, start: u16, len: u16) -> u8 {
    (age.saturating_sub(start).min(len) * 128 / len) as u8
}
/// The source's page sequence, in frames (its waits are 0.1 s, its fade 0.19 s at 5 a second, a
/// flourish nine sprites at 30 a second): the heading fades first, the top flourish unfurls from
/// 0.1 s, the body fades and the bottom flourish unfurls from 0.2 s, and input returns at 0.3 s.
/// The title page has no flourishes: its buttons fade as one group.
#[inline(never)]
fn levels(page: Page, hiding: bool, age: u16) -> Levels {
    let flip = |v: u8| if hiding { 128 - v } else { v };
    if page == Page::Main {
        let g = flip(ramp(age, 0, 12));
        return Levels { title: g, content: g, top: 0, bottom: 0 };
    }
    Levels {
        title: flip(ramp(age, 0, 12)),
        content: flip(ramp(age, 12, 12)),
        top: flip(ramp(age, 6, 16)),
        bottom: flip(ramp(age, if hiding { 18 } else { 12 }, 16)),
    }
}
/// Frames a page takes to leave, and until it takes input.
fn hide_frames(page: Page) -> u16 {
    if page == Page::Main { 12 } else { 34 }
}
fn input_at(page: Page) -> u16 {
    if page == Page::Main { 12 } else { 18 }
}
// For size, like `run`: the title is not a gameplay frame, and the resident code
// budget (docs/BUDGET.md) is what the Options rows would otherwise be spent from.
/// What a frame shows: page `page` with `row` chosen, `age` frames into its sequence at the levels
/// `lv`, the pointer at frame `step`, under the screen's overall `gain`.
#[derive(Clone, Copy)]
struct View {
    page: Page,
    row: usize,
    lv: Levels,
    age: u16,
    gain: u8,
    step: u8,
}
#[cfg_attr(not(test), optimize(size))]
fn draw_menu(menu: &State, v: View, slots: &[&str; 4], fault: &str) {
    let View { page, row, lv, step, .. } = v;
    let scaled = |level: u8| (u16::from(v.gain) * u16::from(level) / 128) as u8;
    let (gain, title) = (scaled(lv.content), scaled(lv.title));
    draw(v.gain);
    particles(v.gain);
    match page {
        Page::Main => {
            for (i, label) in state::MAIN_ITEMS.iter().enumerate() {
                let y = 152 + i as i16 * 18;
                let g = if i == row { gain } else { gain - gain / 4 };
                centered_caps(y, label, g);
                if i == row {
                    let half = caps_width(label) / 2;
                    pointers(160 - half, 160 + half, y, gain, step);
                }
            }
        }
        Page::Profiles => {
            // A page of its own, as the source's Select Game is: no logo behind it.
            gpu::draw_rect_flat(18, 41, 284, 188, 0, 0, 0);
            flourishes(24, 224, lv, v.gain);
            centered_caps(51, "SELECT GAME", title);
            for i in 0..crate::save::PROFILES {
                let y = 80 + i as i16 * 26;
                // The slots appear one after another, 0.165 s apart.
                let shown = scaled(ramp(v.age, 12 + 10 * i as u16, 12));
                let g = if i == row { shown } else { shown - shown / 4 };
                caps(78, y, SLOT_LABELS[i], g);
                text(150, y - 1, slots[i], g);
                if i == row {
                    pointer(52, y, shown, step);
                }
            }
            // The page has no hint row, as the source's has none; a failed read says so here.
            if !fault.is_empty() {
                centered(206, fault, gain);
            }
        }
        Page::Options => {
            gpu::draw_rect_flat(18, 41, 284, 188, 0, 0, 0);
            flourishes(24, 224, lv, v.gain);
            centered_caps(51, "OPTIONS", title);
            for (i, label) in state::OPTION_ITEMS.iter().enumerate() {
                let y = 70 + i as i16 * 18;
                let g = if i == row { gain } else { gain - gain / 4 };
                caps(70, y, label, g);
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
                    text(220, y - 1, value, g);
                }
                if i == row {
                    pointer(46, y, gain, step);
                }
            }
        }
        Page::Controls => {
            gpu::draw_rect_flat(18, 41, 284, 188, 0, 0, 0);
            flourishes(24, 224, lv, v.gain);
            centered_caps(51, "CONTROLS", title);
            for (i, line) in state::CONTROL_LINES.iter().enumerate() {
                centered(68 + i as i16 * 13, line, gain);
            }
        }
        Page::Cheats => {
            // The same panel and flourishes as Options: a window of eight rows follows the
            // cursor through the twelve cheats and Back.
            gpu::draw_rect_flat(18, 41, 284, 188, 0, 0, 0);
            flourishes(24, 224, lv, v.gain);
            centered_caps(51, "CHEATS", title);
            const WINDOW: usize = 8;
            const TOP: i16 = 68;
            const PITCH: i16 = 14;
            let total = state::CHEAT_ITEMS.len() + 1;
            let first = row.saturating_sub(WINDOW / 2).min(total - WINDOW);
            for (i, label) in state::CHEAT_ITEMS
                .iter()
                .chain(["Back"].iter())
                .enumerate()
                .skip(first)
                .take(WINDOW)
            {
                let y = TOP + (i - first) as i16 * PITCH;
                let g = if i == row { gain } else { gain - gain / 4 };
                text(52, y, label, g);
                if i < state::CHEAT_ITEMS.len() {
                    text(
                        248,
                        y,
                        if menu.settings.cheats.enabled(i) {
                            "ON"
                        } else {
                            "OFF"
                        },
                        g,
                    );
                }
                if i == row {
                    pointer(30, y, gain, step);
                }
            }
            // More rows above or below the window.
            if first > 0 {
                text(288, TOP - 2, "^", gain);
            }
            if first + WINDOW < total {
                text(288, TOP + (WINDOW as i16 - 1) * PITCH, "v", gain);
            }
            centered(194, "Session only. More in Pause > Cheats", gain);
        }
    }
    // BRIGHTNESS over the whole screen, text included (display.rs).
    crate::display::draw_direct(v.gain);
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
    // What is on screen lags what the buttons chose: a page that is left plays its leaving sequence
    // (`levels`) and the chosen page then plays its own, as the source does. Input is not held back
    // meanwhile (the source's is): the recorded routes press again on their own timing.
    let mut view = View { page: menu.page, row: 0, lv: FULL, age: 60, gain: 0, step: 5 };
    let mut hiding = false;
    // Frames since the chosen row changed, for the pointer's opening.
    let mut moved = 0u16;
    loop {
        gain = gain.saturating_add(8).min(128);
        view.gain = gain;
        view.lv = levels(view.page, hiding, view.age);
        view.step = if view.age < input_at(view.page) || hiding { HIDDEN } else { moved.min(5) as u8 };
        draw_menu(&menu, view, slots, fault);
        let mut start = false;
        present_samples(fb, |bits| {
            crate::music::tick();
            if start {
                return;
            }
            let old = menu.settings;
            let (page, row) = (menu.page, menu.selected);
            start = menu.step(bits);
            #[cfg(feature = "route-input")]
            if start {
                unsafe { SKIP_DROP_HELD = bits & SKIP_DROP == SKIP_DROP };
            }
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
        view.age = view.age.saturating_add(1);
        moved = moved.saturating_add(1);
        if hiding {
            if view.age >= hide_frames(view.page) {
                (view.page, view.row, view.age, hiding, moved) = (menu.page, menu.selected, 0, false, 0);
            }
        } else if menu.page != view.page {
            (hiding, view.age) = (true, 0);
        } else if menu.selected != view.row {
            (view.row, moved) = (menu.selected, 0);
        }
    }
    view.lv = FULL;
    view.step = 5;
    while gain > 0 {
        gain = gain.saturating_sub(16);
        crate::music::set_fade(gain);
        view.gain = gain;
        draw_menu(&menu, view, slots, fault);
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
#[cfg(any(feature = "audio-probe", feature = "card-diag"))]
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
    let at = crate::boot_art::TITLE_ORNAMENTS.0;
    upload_ornaments(&data[at..at + TITLE_ORNAMENT_BYTES]);
    let (at, len) = crate::boot_art::CARD_FONT;
    assert_eq!(len, state::CARD_BYTES);
    upload_bytes(VramRect::new(0, 500, 320, 12), &data[at..at + len]);
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
            .all(|&n| (1..=state::GLYPH_W as u8).contains(&n))
            && data[METRICS + 95] == 0
    );
    let pixels = &data[HEADER + PALETTE..METRICS];
    upload_bytes(VramRect::new(384, 0, 128, 240), &pixels[..LEFT]);
    upload_bytes(VramRect::new(512, 0, 32, 240), &pixels[LEFT..LEFT + RIGHT]);
    upload_bytes(VramRect::new(544, 0, 96, 64), &pixels[LEFT + RIGHT..]);
}
fn upload_ornaments(rows: &[u8]) {
    let mut put = |piece: (usize, u8, u8), x: u16, y: u16| {
        let (offset, w, h) = piece;
        let w = halfwords(w);
        let len = w as usize * h as usize * 2;
        upload_bytes(VramRect::new(x, y, w, h as u16), &rows[offset..offset + len]);
    };
    put(TITLE_FLEUR_TOP, ORN_X, ORN_TOP);
    put(TITLE_FLEUR_BOTTOM, ORN_X, ORN_BOTTOM);
    put(TITLE_SMOKE, ORN_X, ORN_SMOKE);
    put(TITLE_DOT, ORN_X, ORN_DOT);
    // The pointer's slots (`pointer_u`): final left leaf and right leaf, then the five opening
    // frames of each.
    for (slot, &offset) in TITLE_POINTER_SLOTS.iter().enumerate() {
        let w = halfwords(TITLE_POINTER.1);
        put((offset, TITLE_POINTER.1, TITLE_POINTER.2), ORN_X + w * slot as u16, ORN_POINTER);
    }
}
/// The font image's size (Perpetua and the capitals), which the dialogue panels share.
pub use state::FONT_BYTES;
fn upload_font(font: &[u8]) {
    // The palette, then the image in the texture page of the CLUT banks the scenes never use.
    assert_eq!(font.len(), FONT_BYTES);
    upload_bytes(VramRect::new(320, 481, 16, 1), &font[..32]);
    upload_bytes(
        VramRect::new(state::FONT_X, state::FONT_Y, 64, state::FONT_ROWS),
        &font[32..],
    );
}
