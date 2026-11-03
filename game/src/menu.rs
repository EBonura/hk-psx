//! Boot-only functional menu with original title assets and Perpetua glyphs.
//! Scene admission reuses its title texture storage; no gameplay allocation.
//! Everything here but `skip_drop` and the statics is the `title` code module (host/code_modules.py): the
//! boot installs it from the disc for the title and the game calls it only through title.rs.
use crate::input::poll_bits;
use psx_gpu::{
    self as gpu,
    framebuf::FrameBuffer,
    material::{BlendMode, TextureMaterial},
};
use psx_pad::button;
use psx_vram::{upload_bytes, Clut, TextureDepth, TexturePage, VramRect};

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
// The styles' logos (host/hk-cook/src/title_style.rs): per style two halves of 4bpp coverage.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/menu_logos.rs"));
const _: () = {
    // The cooker's table of styles is the one the options page lists.
    let mut i = 0;
    while i < state::STYLE_NAMES.len() {
        let (a, b) = (state::STYLE_NAMES[i].as_bytes(), COOKED_NAMES[i].as_bytes());
        assert!(a.len() == b.len());
        let mut k = 0;
        while k < a.len() {
            assert!(a[k] == b[k]);
            k += 1;
        }
        i += 1;
    }
    assert!(state::DEFAULT_STYLE == DEFAULT_STYLE);
};
/// The style the title shows (`MenuStyles.currentSettings.styleIndex`): kept across visits to the
/// title in a session. The options page changes it (`state::STYLE_NAMES`).
pub static mut STYLE: u8 = DEFAULT_STYLE;
/// The extras (particles, animated sprites) of the style on screen: its blob in the scene arena
/// (host/hk-cook/src/title_extra.rs), empty when the title is not up.
static mut EXTRA: &[u8] = &[];
/// Where the halves of the logo sit in VRAM: two 4bpp pages of their own beside the title strip,
/// free while the title is up and overwritten by the scenes after it.
const LOGO_X: [u16; 2] = [640, 704];
/// The logo's vertical centre.
const LOGO_Y: i16 = 97;
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
const fn halfwords(width: u8) -> u16 {
    (width as u16).div_ceil(4)
}
const _: () = assert!(halfwords(TITLE_FLEUR_TOP.1) <= 64);
const _: () = assert!(ORN_POINTER as usize + TITLE_POINTER.2 as usize <= 256);
const LEVELS: [&str; 11] = [
    "0%", "10%", "20%", "30%", "40%", "50%", "60%", "70%", "80%", "90%", "100%",
];

fn draw(gain: u8) {
    if !unsafe { ART } {
        return;
    }
    let clut = Clut::new(768, 480).uv_word();
    for (screen_x, vram_x, width) in [(0, 384, 256), (256, 512, 64)] {
        let page = TexturePage::new(vram_x, 0, TextureDepth::Bit8).uv_word(0);
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
                    Clut::new(320, 481).uv_word(),
                    TexturePage::new(state::FONT_X, state::FONT_PAGE_Y, TextureDepth::Bit4).uv_word(0),
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
                Clut::new(320, 481).uv_word(),
                TexturePage::new(state::FONT_X, state::FONT_PAGE_Y, TextureDepth::Bit4).uv_word(0),
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
            Clut::new(320, 481).uv_word(),
            TexturePage::new(ORN_X, 0, TextureDepth::Bit4).uv_word(0),
            (gain, gain, gain),
        ),
    );
}
/// The style's logo (`LogoTitle`: the sprite `MenuStyleTitle` picks), centred, added to the picture at
/// `gain` of 128.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn logo(gain: u8) {
    if gain == 0 || !unsafe { ART } {
        return;
    }
    let halves = LOGOS[usize::from(STYLE_LOGO[unsafe { STYLE } as usize])];
    let total = i16::from(halves[0].1) + i16::from(halves[1].1);
    let mut x = 160 - total / 2;
    for (half, &page_x) in halves.iter().zip(&LOGO_X) {
        let (w, h) = (i16::from(half.1), i16::from(half.2));
        let y = LOGO_Y - h / 2;
        gpu::draw_quad_textured_material(
            [(x, y), (x + w, y), (x, y + h), (x + w, y + h)],
            [(0, 0), (w as u8 - 1, 0), (0, h as u8 - 1), (w as u8 - 1, h as u8 - 1)],
            TextureMaterial::blended(
                Clut::new(320, 481).uv_word(),
                TexturePage::new(page_x, 0, TextureDepth::Bit4).uv_word(0),
                (gain, gain, gain),
                BlendMode::Add,
            ),
        );
        x += w;
    }
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
/// The style's particles and animated sprites over its picture, `gain` of 128 strong. The blob is
/// laid out and explained in host/hk-cook/src/title_extra.rs, whose `play` does the same on a
/// canvas. Everything is stateless: a particle slot's track and age follow from the vblank count.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn extras(gain: u8) {
    let x: &[u8] = unsafe { EXTRA };
    if x.len() < 8 || gain == 0 {
        return;
    }
    let now = psx_rt::interrupts::vblank_count();
    let mut at = 8 + usize::from(x[4]) * 12;
    for _ in 0..x[5] {
        at = emitter(x, at, now, gain);
    }
    for _ in 0..x[6] {
        at = layer(x, at, now, gain);
    }
}
fn rd(x: &[u8], at: usize) -> i32 {
    i32::from(i16::from_le_bytes([x[at], x[at + 1]]))
}
/// A nine-point lookup at `u` of 0 to 255.
fn lerp9(lut: &[u8], u: i32) -> i32 {
    let (i, f) = ((u >> 5).min(7) as usize, u & 31);
    (i32::from(lut[i]) * (32 - f) + i32::from(lut[i + 1]) * f) >> 5
}
/// How a piece meets the picture: added to it, taken away from it, or opaque (its own palette).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Meet {
    Add,
    Take,
    Opaque,
}
/// A quad showing the `(u, v, w, h)` window of a texture page of the title's extras (`page` 0 and 1 are
/// 4bpp coverage tinted by `tint`, 2 is 8bpp with its palette), `gain` of 128 strong.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn piece(corners: [(i16, i16); 4], (u, v, w, h): (i32, i32, i32, i32), page: i32, meet: Meet, tint: (u8, u8, u8)) {
    let eight = page == 2;
    let tpage = TexturePage::new(768 + 64 * page as u16, 0, if eight { TextureDepth::Bit8 } else { TextureDepth::Bit4 }).uv_word(0);
    let material = match meet {
        Meet::Opaque => TextureMaterial::opaque(Clut::new(768, 479).uv_word(), tpage, tint),
        Meet::Take => TextureMaterial::blended(Clut::new(320, 481).uv_word(), tpage, tint, BlendMode::Subtract),
        Meet::Add => TextureMaterial::blended(Clut::new(320, 481).uv_word(), tpage, tint, BlendMode::Add),
    };
    let (u1, v1) = ((u + w - 1) as u8, (v + h - 1) as u8);
    gpu::draw_quad_textured_material(corners, [(u as u8, v as u8), (u1, v as u8), (u as u8, v1), (u1, v1)], material);
}
/// One emitter: its slots, each playing one of four tracks per life. Returns where the next record starts.
fn emitter(x: &[u8], at: usize, now: u32, gain: u8) -> usize {
    let h: &[u8; 28] = x[at..at + 28].try_into().unwrap();
    let slots = usize::from(h[7]);
    let (w, hh, frames, cols) = (i32::from(h[3]), i32::from(h[4]), i32::from(h[5]).max(1), i32::from(h[26]).max(1));
    let meet = if h[6] != 0 { Meet::Take } else { Meet::Add };
    for i in 0..slots {
        let s = at + 28 + i * 65;
        let slot: &[u8; 65] = x[s..s + 65].try_into().unwrap();
        let life = i32::from(slot[0]) * 2;
        let t = now.wrapping_add(i as u32 * 37) as i32;
        let (cycle, age) = (t / life, t % life);
        let k = 1 + (mix(i as u32, cycle as u32) & 3) as usize * 16;
        let tr: &[u8; 16] = slot[k..k + 16].try_into().unwrap();
        let u = age * 256 / life;
        let word = |p: usize| i32::from(i16::from_le_bytes([tr[p], tr[p + 1]]));
        let at_u = |p: usize| (word(p) + ((word(p + 4) * u) >> 8) + ((((word(p + 8) * u) >> 8) * u) >> 8)) >> 2;
        let (px, py) = (at_u(0), at_u(2));
        let size = (i32::from(tr[12]) * lerp9(&h[17..26], u)) >> 8;
        let alpha = lerp9(&h[8..17], u);
        if size <= 0 || px + size < 0 || px - size > 320 || py + size < 0 || py - size > 240 {
            continue;
        }
        let g = |c: usize| ((i32::from(tr[13 + c]) * alpha >> 8) * i32::from(gain) >> 7).min(255) as u8;
        let tint = (g(0), g(1), g(2));
        // Under two of 128 a tinted piece changes nothing the display can show.
        if tint.0.max(tint.1).max(tint.2) < 2 {
            continue;
        }
        let frame = ((u * frames) >> 8).min(frames - 1);
        let (u0, v0) = (i32::from(h[1]) + frame % cols * w, i32::from(h[2]) + frame / cols * hh);
        let (x0, y0) = ((px - size / 2) as i16, (py - size / 2) as i16);
        let (x1, y1) = (x0 + size as i16, y0 + size as i16);
        piece([(x0, y0), (x1, y0), (x0, y1), (x1, y1)], (u0, v0, w, hh), i32::from(h[0]), meet, tint);
    }
    at + 28 + slots * 65
}
/// One animated layer: a quad whose frame, strength and offset follow its table of samples.
fn layer(x: &[u8], at: usize, now: u32, gain: u8) -> usize {
    let h: &[u8; 32] = x[at..at + 32].try_into().unwrap();
    let samples = usize::from(u16::from_le_bytes([h[28], h[29]]));
    let k = at + 32 + ((now / u32::from(h[30].max(1))) as usize % samples) * 4;
    let sample: &[u8; 4] = x[k..k + 4].try_into().unwrap();
    let (frame, strength, dx, dy) = (i32::from(sample[0]), i32::from(sample[1]), i32::from(sample[2] as i8), i32::from(sample[3] as i8));
    if strength != 0 {
        let (w, hh, cols, per) = (i32::from(h[4]), i32::from(h[5]), i32::from(h[6]).max(1), i32::from(h[7]));
        let (page, i) = if per > 0 { (i32::from(h[0]) + frame / per, frame % per) } else { (i32::from(h[0]), frame) };
        let (u0, v0) = (i32::from(h[2]) + i % cols * w, i32::from(h[3]) + i / cols * hh);
        let c = |n: usize| (i32::from(h[24 + n]) * strength * i32::from(gain) >> 14).min(255) as u8;
        let meet = [Meet::Add, Meet::Take, Meet::Add, Meet::Opaque][usize::from(h[1] & 3)];
        let tint = if meet == Meet::Opaque { (gain, gain, gain) } else { (c(0), c(1), c(2)) };
        let p = |n: usize| ((rd(h, 8 + n * 4) + dx) as i16, (rd(h, 10 + n * 4) + dy) as i16);
        piece([p(0), p(1), p(2), p(3)], (u0, v0, w, hh), page, meet, tint);
    }
    at + 32 + samples * 4
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
    /// How much of the style shows, 0 to 128: the style fades through black when the options page
    /// changes it (`MenuStyles.Fade`, 0.25 s); the page's own text stays.
    bg: u8,
    step: u8,
}
#[cfg_attr(not(test), optimize(size))]
fn draw_menu(menu: &State, v: View, slots: &[&str; 4], fault: &str) {
    let View { page, row, lv, step, .. } = v;
    let scaled = |level: u8| (u16::from(v.gain) * u16::from(level) / 128) as u8;
    let (gain, title) = (scaled(lv.content), scaled(lv.title));
    let bg = (u16::from(v.gain) * u16::from(v.bg) / 128) as u8;
    draw(bg);
    extras(bg);
    match page {
        Page::Main => {
            // The logo is the style's own sprite, fading with the page's heading.
            logo((u16::from(title) * u16::from(v.bg) / 128) as u8);
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
            // A page of its own, as the source's Select Game is: the style behind it, no logo.
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
                    state::STYLE_ROW => state::STYLE_NAMES[usize::from(s.style)],
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
            flourishes(24, 224, lv, v.gain);
            centered_caps(51, "CONTROLS", title);
            for (i, line) in state::CONTROL_LINES.iter().enumerate() {
                centered(68 + i as i16 * 13, line, gain);
            }
        }
        Page::Cheats => {
            // The same flourishes as Options: a window of eight rows follows the cursor through the
            // twelve cheats and Back.
            flourishes(24, 224, lv, v.gain);
            centered_caps(51, "CHEATS", title);
            const WINDOW: usize = 8;
            const TOP: i16 = 68;
            const PITCH: i16 = 14;
            let total = state::CHEAT_BACK + 1;
            let first = row.saturating_sub(WINDOW / 2).min(total - WINDOW);
            for (i, label) in state::CHEAT_ITEMS
                .iter()
                .chain(["Unlock all menu styles", "Back"].iter())
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
                } else if i == state::UNLOCK_ROW {
                    // The action's state: every style key set, on the card for the next visit.
                    text(248, y, if menu.settings.unlocked == state::ALL_STYLE_KEYS { "DONE" } else { "" }, g);
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
            centered(194, "Toggles last the session. More in Pause > Cheats", gain);
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
    psx_rt::interrupts::queue_display_control_at_vblank(fb.begin_deferred_swap());
    let mut waited = 0u32;
    while psx_rt::interrupts::is_display_control_queued() {
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
    menu.settings.style = unsafe { STYLE };
    menu.settings.unlocked = crate::save::unlocked_styles();
    // What is on screen lags what the buttons chose: a page that is left plays its leaving sequence
    // (`levels`) and the chosen page then plays its own, as the source does. Input is not held back
    // meanwhile (the source's is): the recorded routes press again on their own timing.
    let mut view = View { page: menu.page, row: 0, lv: FULL, age: 60, gain: 0, bg: 128, step: 5 };
    // A style the options page chose: SetStyle fades the old style to black over 0.25 s
    // (`MenuStyles.fadeTime`), replaces its art and logo in VRAM, and the new one comes up from black;
    // the page's text stays.
    let mut pending: Option<u8> = None;
    let mut hiding = false;
    // Frames since the chosen row changed, for the pointer's opening.
    let mut moved = 0u16;
    loop {
        gain = gain.saturating_add(8).min(128);
        view.gain = gain;
        view.bg = match pending {
            Some(next) if view.bg == 0 => {
                let before = unsafe { STYLE };
                unsafe {
                    STYLE = next;
                }
                if let Some(data) = art {
                    // A style the disc cannot give leaves the one on screen, as it was.
                    if !upload_style(data) {
                        unsafe {
                            STYLE = before;
                        }
                        menu.settings.style = before;
                    }
                }
                pending = None;
                9
            }
            Some(_) => view.bg.saturating_sub(9),
            None => (view.bg + 9).min(128),
        };
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
            if old.style != menu.settings.style {
                pending = Some(menu.settings.style);
            }
            // The Cheats page's unlock: onto the card, and the selector sees the styles at once.
            if old.unlocked != menu.settings.unlocked {
                crate::save::unlock_styles(menu.settings.unlocked);
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
    // The arena is the next scene's now.
    unsafe {
        EXTRA = &[];
    }
    // Consume the activating hold so Cross cannot also cause a gameplay jump.
    while poll_bits() & (button::START | button::CROSS) != 0 {
        psx_rt::interrupts::wait_vblank();
    }
    (menu.settings, menu.profile.unwrap_or(0))
}

fn status(fb: &mut FrameBuffer, patch: u8) -> u16 {
    draw(128);
    logo(128);
    gpu::draw_sprite_material(
        64,
        177,
        192,
        32,
        (64, patch * 32),
        TextureMaterial::opaque(
            Clut::new(768, 480).uv_word(),
            TexturePage::new(512, 0, TextureDepth::Bit8).uv_word(0),
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
#[inline(never)]
pub fn loading(fb: &mut FrameBuffer) {
    let _ = status(fb, 0);
}

/// A failed read can be retried after releasing the previous activating press.
#[inline(never)]
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
#[inline(never)]
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
    if !upload_style(data) {
        // The style asked for cannot be read: the default one is in this chunk.
        unsafe {
            STYLE = DEFAULT_STYLE;
        }
        upload_style(data);
    }
}
/// The style `STYLE` names: its background pages, palette and loading prompts, its logo and its extras.
/// The default style rides the boot chunk; any other is read from the disc into the arena after it,
/// with the title's song stopped for the read (the drive cannot do both). Returns false, changing
/// nothing in video memory, when that read fails.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn upload_style(chunk: &[u8]) -> bool {
    let style = unsafe { STYLE } as usize;
    let (start, extra_at, extra_len, _) = crate::boot_art::STYLE_ART[style];
    let (start, extra_at, extra_len) = (start as usize, extra_at as usize, extra_len as usize);
    let own;
    let (data, extra): (&[u8], &[u8]) = if style == usize::from(DEFAULT_STYLE) {
        (&chunk[..MENU_BYTES], &chunk[extra_at..extra_at + extra_len])
    } else {
        let resume = crate::music::suspend();
        let read = crate::disc::style_art(style);
        if resume {
            crate::music::begin();
        }
        let Some(read) = read else {
            unsafe {
                EXTRA = &[];
            }
            return false;
        };
        own = read;
        (&own[..MENU_BYTES], &own[extra_at - start..extra_at - start + extra_len])
    };
    // The chunk's checksum was verified when it was read and the blob's size is `MENU_BYTES` (a const
    // check above); the header and the glyph advances are checked by the cooker and the tests.
    assert!(&data[..8] == b"HKMENU03");
    upload_bytes(
        VramRect::new(768, 480, 256, 1),
        &data[HEADER..HEADER + PALETTE],
    );
    let pixels = &data[HEADER + PALETTE..METRICS];
    upload_bytes(VramRect::new(384, 0, 128, 240), &pixels[..LEFT]);
    upload_bytes(VramRect::new(512, 0, 32, 240), &pixels[LEFT..LEFT + RIGHT]);
    upload_bytes(VramRect::new(544, 0, 96, 64), &pixels[LEFT + RIGHT..]);
    let (logos, len) = crate::boot_art::LOGO_ART;
    assert_eq!(len, LOGO_BYTES);
    let logos = &chunk[logos..logos + len];
    for (half, &x) in LOGOS[usize::from(STYLE_LOGO[style])].iter().zip(&LOGO_X) {
        let (offset, w, h) = *half;
        let w = halfwords(w);
        upload_bytes(VramRect::new(x, 0, w, u16::from(h)), &logos[offset..offset + w as usize * h as usize * 2]);
    }
    // The extras' pieces (host/hk-cook/src/title_extra.rs): a table of rectangles, then their rows.
    assert!(&extra[..4] == b"HKX1");
    for k in 0..usize::from(extra[4]) {
        let e = 8 + k * 12;
        let (x, y, w, h) = (rd(extra, e) as u16, rd(extra, e + 2) as u16, rd(extra, e + 4) as u16, rd(extra, e + 6) as u16);
        let at = u32::from_le_bytes([extra[e + 8], extra[e + 9], extra[e + 10], extra[e + 11]]) as usize;
        upload_bytes(VramRect::new(x, y, w, h), &extra[at..at + usize::from(w) * usize::from(h) * 2]);
    }
    // SAFETY: the extras live in the boot chunk or the arena behind it, which nothing writes while
    // the title is up; `run` clears this before the arena is the next scene's.
    unsafe {
        EXTRA = core::slice::from_raw_parts(extra.as_ptr(), extra.len());
    }
    // GP0(01h): the texture and palette caches still hold the previous style's texels and colours.
    psx_io::gpu::write_command(0x0100_0000);
    true
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
