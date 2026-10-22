//! Area and boss title cards (host/title_cards.py).
//!
//! Entering a scene whose `Area Title Controller` announces an area other than
//! the one last announced arms the card; a controller that waits for its
//! trigger holds it until the Knight is inside that box; then the source's
//! pause (first visit or later visit) runs and the card fades in, holds and
//! fades out. `Only On Revisit` controllers stay silent until the area has been
//! seen once. Areas seen are kept in the world store, as the source's visited
//! bools are. Bosses call `show_boss` with their own title.
//!
//! The text uses the dialogue font the disc already has, main line doubled.
use psx_gpu::ot::OrderingTable;
use psx_gpu::prim::QuadTextured;
use psx_vram::{Clut, TexDepth, Tpage};
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/title_cards.rs"
));
use crate::persist::{self, Kind};

const NONE: u8 = u8::MAX;
const FADE: u16 = 30;
const HOLD: u16 = 150;
// The longest card is 22 letters (Dirtmouth, The Fading Town); a charm name is at most 17.
const GLYPHS: usize = 24;
#[derive(Clone, Copy, PartialEq)]
enum Style {
    Area,
    Boss,
    Item,
}
/// `showing` while an item's name is up rather than a title.
const ITEM: u8 = NONE - 1;
static mut ITEM_NAME: &str = "";
struct State {
    last: u8,
    armed: Option<usize>,
    triggered: bool,
    wait: u16,
    showing: u8,
    style: Style,
    age: u16,
}
static mut STATE: State = State {
    last: NONE,
    armed: None,
    triggered: false,
    wait: 0,
    showing: NONE,
    style: Style::Area,
    age: 0,
};
static mut QUADS: [QuadTextured; GLYPHS] =
    [const { QuadTextured::new([(0, 0); 4], [(0, 0); 4], 0, 0, (0, 0, 0)) }; GLYPHS];
/// A 12x12 textured rectangle with its draw mode in the packet (the panel's glyph).
#[repr(C, align(4))]
struct Rect {
    tag: u32,
    mode: u32,
    color: u32,
    xy: u32,
    uv: u32,
    wh: u32,
}
static mut RECTS: [Rect; GLYPHS] = [const {
    Rect {
        tag: 0,
        mode: 0,
        color: 0,
        xy: 0,
        uv: 0,
        wh: 0,
    }
}; GLYPHS];
/// Whether slot `i` of the frame's glyphs is a rectangle (scale 1) or a quad (scale 2).
static mut KINDS: [bool; GLYPHS] = [false; GLYPHS];
static mut USED: usize = 0;
/// Cards shown, and the title index (plus one) of the last.
#[no_mangle]
pub static mut HK_TITLE_CARDS: u32 = 0;
#[no_mangle]
pub static mut HK_TITLE_CARD: u32 = 0;

fn state() -> &'static mut State {
    unsafe { &mut *(&raw mut STATE) }
}
fn visited(title: u8) -> bool {
    persist::get(Kind::Visited, title as usize, 0).is_some()
}
fn show(title: u8, style: Style) {
    let s = state();
    s.showing = title;
    s.style = style;
    s.age = 0;
    unsafe {
        HK_TITLE_CARDS = HK_TITLE_CARDS.saturating_add(1);
        HK_TITLE_CARD = title as u32 + 1;
    }
}
/// A scene became current (gate, respawn or load).
pub fn enter_scene(scene: usize) {
    let s = state();
    s.armed = None;
    if let Some(i) = AREAS.iter().position(|a| a.scene as usize == scene) {
        if AREAS[i].title != s.last {
            s.armed = Some(i);
            s.triggered = AREAS[i].trigger.is_none();
            s.wait = if visited(AREAS[i].title) {
                AREAS[i].visited_ticks
            } else {
                AREAS[i].unvisited_ticks
            };
        }
    }
}
/// A boss's own name card, from the `Titles` sheet (e.g. `FALSE_KNIGHT`).
pub fn show_boss(title: u8) {
    show(title, Style::Boss);
}
/// A pickup's message: the charm or item name its `Shiny Control` shows.
pub fn show_item(name: &'static str) {
    if name.is_empty() {
        return;
    }
    unsafe {
        ITEM_NAME = name;
    }
    show(ITEM, Style::Item);
}
/// A new session or the development reset: nothing announced yet.
pub fn reset() {
    *state() = State {
        last: NONE,
        armed: None,
        triggered: false,
        wait: 0,
        showing: NONE,
        style: Style::Area,
        age: 0,
    };
}
/// One simulation tick; `body` is the Knight's box.
pub fn tick(body: [i32; 4]) {
    let s = state();
    if s.showing != NONE {
        s.age += 1;
        if s.age >= FADE * 2 + HOLD {
            s.showing = NONE;
        }
    }
    let Some(i) = s.armed else { return };
    let area = AREAS[i];
    if !s.triggered {
        let Some(b) = area.trigger else { return };
        if !(body[0] <= b[2] && body[2] >= b[0] && body[1] <= b[3] && body[3] >= b[1]) {
            return;
        }
        s.triggered = true;
    }
    if s.wait > 0 {
        s.wait -= 1;
        return;
    }
    s.armed = None;
    s.last = area.title;
    let seen = visited(area.title);
    persist::set(Kind::Visited, area.title as usize, 0, 1);
    if area.only_on_revisit && !seen {
        return;
    }
    show(area.title, Style::Area);
}
/// The horizontal centre of a card's lines: mid-screen for an area or item,
/// and for a boss the original's lower left, never so far left that a long
/// name starts off screen (`BOSS_CENTRE`, `BOSS_MARGIN`).
const BOSS_CENTRE: i16 = 56;
const BOSS_MARGIN: i16 = 12;
fn line(text: &str, y: i16, scale: i16, level: u8, centre: i16) {
    let width: i16 = text
        .bytes()
        .map(|b| crate::dialogue::advance(b) * scale)
        .sum();
    let mut x = if centre < 160 {
        (centre - width / 2).max(BOSS_MARGIN)
    } else {
        centre - width / 2
    };
    for b in text.bytes() {
        let i = (b - 32) as usize;
        if b != b' ' && unsafe { USED } < GLYPHS {
            let u = crate::dialogue::glyph_u(i) as u8;
            let page = (i / 21 * 64) as u16;
            let clut = Clut::new(320, 481).uv_clut_word();
            let tpage = Tpage::new(page, 256, TexDepth::Bit4).uv_tpage_word(0);
            unsafe {
                if scale == 1 {
                    // A 12x12 rectangle samples texel for texel: exact.
                    RECTS[USED] = Rect {
                        tag: 0,
                        mode: 0xe100_0000 | tpage as u32,
                        color: 0x6400_0000 | u32::from(level) * 0x0001_0101,
                        xy: x as u16 as u32 | ((y as u16 as u32) << 16),
                        uv: u32::from(u) | (244 << 8) | (u32::from(clut) << 16),
                        wh: 12 | (12 << 16),
                    };
                } else {
                    // A polygon seeds its texture coordinate half a texel in and steps a
                    // fixed amount a pixel, so doubling a glyph exactly means starting one
                    // texel early: the polygon spans 12 texels over 24 pixels from the
                    // blank column before the glyph (cells start 4 into the strip) and
                    // 11 rows from the blank top row over 22 pixels (row 255 is the last
                    // texel there is). That shows 11 of the 12 rows, 2 pixels each, with
                    // only the last row of a descender halved.
                    let (w, h) = (12 * scale, 11 * scale);
                    let y = y + scale / 2;
                    QUADS[USED] = QuadTextured::new(
                        [(x, y), (x + w, y), (x, y + h), (x + w, y + h)],
                        [(u - 1, 244), (u + 11, 244), (u - 1, 255), (u + 11, 255)],
                        clut,
                        tpage,
                        (level, level, level),
                    );
                }
                KINDS[USED] = scale == 1;
                USED += 1;
            }
        }
        x += crate::dialogue::advance(b) * scale;
    }
}
/// Compose this frame's card, if any. A paused game shows none: its clock is stopped, and the
/// pause pages are drawn over the view rather than in an opaque box that used to hide the card.
pub fn prepare(paused: bool) {
    unsafe {
        USED = 0;
    }
    let s = state();
    if s.showing == NONE || paused {
        return;
    }
    let level = if s.age < FADE {
        s.age * 128 / FADE
    } else if s.age < FADE + HOLD {
        128
    } else {
        (FADE * 2 + HOLD - s.age) * 128 / FADE
    } as u16;
    let level = level as u8;
    if s.style == Style::Item {
        line(unsafe { ITEM_NAME }, 104, 1, level, 160);
        return;
    }
    let [sup, main, sub] = TITLES[s.showing as usize];
    // The original's boss card sits at the lower left of the screen; its area
    // card is the one at the top centre.
    let (y, centre) = if s.style == Style::Area {
        (52, 160)
    } else {
        (170, BOSS_CENTRE)
    };
    if !sup.is_empty() {
        line(sup, y, 1, level, centre);
    }
    line(main, y + 14, 2, level, centre);
    if !sub.is_empty() {
        line(sub, y + 42, 1, level, centre);
    }
}
pub fn append(ot: &mut OrderingTable<1>) {
    unsafe {
        for i in (0..USED).rev() {
            if KINDS[i] {
                crate::display::ot_add(ot, 0, &mut RECTS[i], 5);
            } else {
                crate::display::ot_add(ot, 0, &mut QUADS[i], QuadTextured::WORDS);
            }
        }
    }
}
