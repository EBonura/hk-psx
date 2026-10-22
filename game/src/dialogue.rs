//! Original tablet and NPC text/Perpetua font; bounded PS1 panel adaptation.
use psx_gpu::{
    material::BlendMode,
    ot::OrderingTable,
    prim::{QuadGouraudBlended, RectFlat},
};
use crate::menu::state::{
    CAPS_ADVANCES, CAPS_CHARS, CAPS_H, CAPS_PER_ROW, CAPS_V0, CAPS_W, COIN_CELL, FONT_X,
    FONT_PAGE_Y, FONT_V0, GLYPH_H, GLYPH_PER_ROW, GLYPH_ROW_PITCH, GLYPH_U0, GLYPH_W,
};
use psx_vram::{upload_bytes, Clut, TexDepth, Tpage, VramRect};
#[path = "npc_state.rs"]
mod npc_state;
#[path = "read_state.rs"]
mod read_state;
use npc_state::{Convo, Cursors, NpcConversation, NpcLines};
use read_state::{ReadPoint, State};
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/read_points.rs"
));
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/npc_lines.rs"));
// Two panels, one pad. The tablet reader and the NPC panel each name the bits
// they answer, so a divergence would silently give one of them the wrong button.
const _: () = assert!(
    npc_state::UP == read_state::UP
        && npc_state::DOWN == read_state::DOWN
        && npc_state::CROSS == read_state::CROSS
        && npc_state::CIRCLE == read_state::CIRCLE
);
/// The longest chain in a cooked table, for the bound below.
const fn longest_chain(lines: &[NpcLines]) -> usize {
    let (mut i, mut most) = (0, 0);
    while i < lines.len() {
        if lines[i].conversations.len() > most {
            most = lines[i].conversations.len();
        }
        i += 1;
    }
    most
}
// A cursor is CURSOR_BITS wide and the word holds MAX_SLOTS of them, so a table
// past either bound would wrap one NPC's progress into the next one's. The cook
// refuses first; this is what stops a stale generated table from linking.
const _: () = assert!(NPC_LINES.len() <= npc_state::MAX_SLOTS);
const _: () = assert!(longest_chain(NPC_LINES) <= npc_state::MAX_CONVERSATIONS);
static mut STATE: State = State::new();
#[no_mangle]
pub static mut HK_READ_SOURCE: u32 = 0;
#[no_mangle]
pub static mut HK_READ_PAGE: u32 = 0;
#[no_mangle]
pub static mut HK_READ_OPENED: u32 = 0;
#[no_mangle]
pub static mut HK_READ_CLOSED: u32 = 0;
pub(crate) const CAP: usize = 416;
#[repr(C, align(4))]
struct Glyph {
    tag: u32,
    mode: u32,
    color: u32,
    xy: u32,
    uv: u32,
    wh: u32,
}
const EMPTY: Glyph = Glyph {
    tag: 0,
    mode: 0,
    color: 0,
    xy: 0,
    uv: 0,
    wh: 0,
};
static mut GLYPHS: [Glyph; CAP] = [const { EMPTY }; CAP];
static mut BOXES: [RectFlat; 2] = [const { RectFlat::new(0, 0, 0, 0, 0, 0, 0) }; 2];
/// The ornate panel's backboard: a 3x3 grid of translucent quads, flat in the
/// middle and fading out at every edge.
static mut BACKBOARD: [QuadGouraudBlended; BACKBOARDS] = [const {
    QuadGouraudBlended::new([(0, 0); 4], [(0, 0, 0); 4], BlendMode::Subtract)
}; BACKBOARDS];
/// Backboard quads a frame can carry: the charm board's black, nine of purple glow and two of
/// light behind the chosen slot.
const BACKBOARDS: usize = 12;
static mut BACKBOARD_COUNT: usize = 0;
static mut USED: usize = 0;
static mut BOX_COUNT: usize = 0;
pub fn upload() {
    // The glyph sheet itself is the title's (menu::restore uploads it from the
    // boot art chunk), and nothing between the title and play writes it over.
    // The charm board's own resident CLUTs ride with the panel font rather than
    // taking a second call site in `main`, which is at the MIPS branch limit.
    crate::charms::upload();
}
static mut CONVO: Convo = Convo::new();
/// Every cooked NPC's conversation cursor. `save.rs` carries the word so it
/// survives a quit, the way the source's own metElderbug does.
static mut CURSORS: Cursors = Cursors::new();
#[no_mangle]
pub static mut HK_NPC_SOURCE: u32 = 0;
#[no_mangle]
pub static mut HK_NPC_PAGE: u32 = 0;
#[no_mangle]
pub static mut HK_NPC_OPENED: u32 = 0;
/// `CURSORS` as one word, for the replay watches.
#[no_mangle]
pub static mut HK_NPC_MET: u32 = 0;
/// Source PlayerData `metElderbug`, projected from the slot the cook found it
/// in, for the town-elderbug route that asserts it by name.
#[no_mangle]
pub static mut HK_MET_ELDERBUG: u32 = 0;
fn publish_cursors() {
    unsafe {
        HK_NPC_MET = CURSORS.bits();
        HK_MET_ELDERBUG = MET_ELDERBUG_SLOT.map_or(0, |slot| u32::from(CURSORS.get(slot) > 0));
    }
}
/// The whole persistent word, for the bench save record.
pub fn met_bits() -> u32 {
    unsafe { CURSORS.bits() }
}
/// Restore it from a loaded record.
pub fn restore_met(bits: u32) {
    unsafe {
        CURSORS = Cursors::from_bits(bits);
    }
    publish_cursors();
}
/// A panel owns the Knight: a tablet, a conversation, Cornifer's, or the quick
/// map, which the source runs with control relinquished and pause disabled.
pub fn open() -> bool {
    #[cfg(feature = "hornet")]
    if crate::enemies::hero_locked() {
        return true;
    }
    (unsafe { STATE.open() || CONVO.open() })
        || crate::mapper::open()
        || crate::game_map::open()
        || crate::shaman::open()
}
pub fn consumes_actions() -> bool {
    (unsafe { STATE.consumes_actions() || CONVO.consumes_actions() })
        || crate::mapper::consumes_actions()
        || crate::game_map::open()
        || crate::shaman::consumes_actions()
}
pub fn cancel() {
    unsafe {
        if STATE.open() {
            HK_READ_CLOSED += 1;
        }
        STATE.cancel();
        HK_READ_SOURCE = 0;
        CONVO.cancel();
        HK_NPC_SOURCE = 0;
        HK_NPC_PAGE = 0;
    }
    crate::mapper::cancel();
    crate::game_map::cancel();
    crate::shaman::cancel();
}
pub fn reset() {
    unsafe {
        STATE = State::new();
        HK_READ_SOURCE = 0;
        HK_READ_PAGE = 0;
        CONVO = Convo::new();
        CURSORS = Cursors::new();
        HK_NPC_SOURCE = 0;
        HK_NPC_PAGE = 0;
    }
    crate::mapper::cancel();
    crate::game_map::cancel();
    publish_cursors();
}
pub fn tick(
    scene: usize,
    player: &hk_sim::Player,
    params: hk_sim::Params,
    eligible: bool,
    bits: u16,
) {
    let body = [
        player.x - params.half_width,
        player.y + params.bottom,
        player.x + params.half_width,
        player.y + params.top,
    ];
    // One panel at a time: a tablet cannot open behind an open conversation.
    let eligible = eligible && unsafe { CONVO.active.is_none() };
    unsafe {
        let e = STATE.step(POINTS, scene, body, eligible, bits);
        HK_READ_OPENED += u32::from(e.opened);
        HK_READ_CLOSED += u32::from(e.closed);
        HK_READ_SOURCE = STATE.active.map_or(0, |i| POINTS[i].source_id);
        HK_READ_PAGE = STATE.page as u32;
    }
}
/// The NPC whose conversation is open, by its cooked source object id.
pub fn npc_speaking() -> Option<u32> {
    unsafe { CONVO.active.map(|i| NPC_LINES[i].source_id) }.or_else(crate::mapper::speaking)
}
/// One tick of the NPC panel. `in_range` is the cooked NPC whose talk trigger
/// the hero body is inside; the state machine is npc_state.rs.
/// Returns the Geo a purchase in conversation took (Cornifer's map).
pub fn npc_tick(scene: usize, in_range: Option<u32>, eligible: bool, bits: u16, geo: u32) -> u32 {
    // Cornifer speaks through his own module; every other NPC through the chain.
    // Cornifer's conversation is room logic carried by his room (modules.rs).
    let paid = if crate::modules::loaded(crate::modules::CORNIFER) {
        crate::mapper::tick(
            scene,
            in_range.is_some_and(crate::mapper::owns),
            eligible && unsafe { !STATE.open() && CONVO.active.is_none() },
            bits,
            geo,
        )
    } else {
        0
    };
    let in_range = in_range.filter(|&s| !crate::mapper::owns(s));
    unsafe {
        let eligible = eligible && !STATE.open() && !crate::mapper::open();
        let convo = &mut *(&raw mut CONVO);
        let event = convo.step(
            NPC_LINES,
            &mut *(&raw mut CURSORS),
            scene,
            in_range,
            eligible,
            bits,
        );
        HK_NPC_OPENED += u32::from(event.opened);
        HK_NPC_SOURCE = convo.active.map_or(0, |i| NPC_LINES[i].source_id);
        HK_NPC_PAGE = convo.page as u32;
    }
    publish_cursors();
    paid
}
/// One glyph's advance, for the title cards that draw this font doubled.
pub(crate) fn advance(b: u8) -> i16 {
    ADVANCES[(b - 32) as usize] as i16
}
fn width(text: &str) -> i16 {
    text.bytes()
        .map(|b| ADVANCES[(b - 32) as usize] as i16)
        .sum()
}
fn text(x: i16, y: i16, text: &str) {
    text_gain(x, y, text, 0x80);
}
/// `text` at a brightness: 0x80 draws the glyph ramp as it is, less dims it.
fn text_gain(x: i16, y: i16, text: &str, gain: u8) {
    // Semi-transparent, adding: the glyph palette's entries carry the semi-transparency bit, so a
    // glyph's anti-aliased edge brightens what is behind it (host/hk-cook/src/font_strip.rs).
    let color = 0x6600_0000 | u32::from(gain) * 0x0001_0101;
    let mut x = x;
    for b in text.bytes() {
        assert!((32..=126).contains(&b));
        let i = (b - 32) as usize;
        if b != b' ' {
            unsafe {
                assert!(USED < CAP);
                let p = &mut GLYPHS[USED];
                USED += 1;
                p.mode = 0xe100_0000
                    | Tpage::new(FONT_X, FONT_PAGE_Y, TexDepth::Bit4).uv_tpage_word(1) as u32;
                p.color = color;
                p.xy = x as u16 as u32 | ((y as u16 as u32) << 16);
                p.uv = glyph_u(i) as u32
                    | ((glyph_v(i) as u32) << 8)
                    | ((Clut::new(320, 481).uv_clut_word() as u32) << 16);
                p.wh = GLYPH_W as u32 | ((GLYPH_H as u32) << 16);
            }
        }
        x += ADVANCES[i] as i16;
    }
}
/// U and V of glyph cell `i` of the font image: `GLYPH_PER_ROW` cells to a row, rows
/// `GLYPH_ROW_PITCH` apart (the spare texel row keeps the texel above a cell blank).
pub(crate) const fn glyph_u(i: usize) -> usize {
    GLYPH_U0 + i % GLYPH_PER_ROW * GLYPH_W
}
pub(crate) const fn glyph_v(i: usize) -> usize {
    FONT_V0 + i / GLYPH_PER_ROW * GLYPH_ROW_PITCH
}
/// The Geo coin, a 12x12 cell of the font image's capitals rows, drawn 1:1 at (`x`, `y`).
fn coin(x: i16, y: i16) {
    unsafe {
        assert!(USED < CAP);
        let p = &mut GLYPHS[USED];
        USED += 1;
        p.mode = 0xe100_0000 | Tpage::new(FONT_X, FONT_PAGE_Y, TexDepth::Bit4).uv_tpage_word(0) as u32;
        p.color = 0x6480_8080;
        p.xy = x as u16 as u32 | ((y as u16 as u32) << 16);
        let (col, row) = (COIN_CELL % CAPS_PER_ROW, COIN_CELL / CAPS_PER_ROW);
        p.uv = (GLYPH_U0 + col * CAPS_W) as u32
            | (((FONT_V0 + CAPS_V0 + row * CAPS_H) as u32) << 8)
            | ((Clut::new(320, 481).uv_clut_word() as u32) << 16);
        p.wh = 12 | (12 << 16);
    }
}
/// Capitals for the menu buttons and headings (TrajanPro-Bold, as the original's): `value` upper
/// cased, letters, digits and % only, a space as a gap.
pub(crate) fn caps_width(value: &str) -> i16 {
    value
        .bytes()
        .map(|b| match CAPS_CHARS.iter().position(|&c| c == b.to_ascii_uppercase()) {
            Some(i) => CAPS_ADVANCES[i] as i16,
            None => CAPS_SPACE,
        })
        .sum()
}
/// A space between capitals.
pub(crate) const CAPS_SPACE: i16 = 6;
pub(crate) fn caps_text(x: i16, y: i16, value: &str, gain: u8) {
    let color = 0x6600_0000 | u32::from(gain) * 0x0001_0101;
    let mut x = x;
    for b in value.bytes() {
        let Some(i) = CAPS_CHARS.iter().position(|&c| c == b.to_ascii_uppercase()) else {
            x += CAPS_SPACE;
            continue;
        };
        unsafe {
            assert!(USED < CAP);
            let p = &mut GLYPHS[USED];
            USED += 1;
            p.mode = 0xe100_0000 | Tpage::new(FONT_X, FONT_PAGE_Y, TexDepth::Bit4).uv_tpage_word(1) as u32;
            p.color = color;
            p.xy = x as u16 as u32 | ((y as u16 as u32) << 16);
            let (col, row) = (i % CAPS_PER_ROW, i / CAPS_PER_ROW);
            p.uv = (GLYPH_U0 + col * CAPS_W) as u32
                | (((FONT_V0 + CAPS_V0 + row * CAPS_H) as u32) << 8)
                | ((Clut::new(320, 481).uv_clut_word() as u32) << 16);
            p.wh = CAPS_W as u32 | ((CAPS_H as u32) << 16);
        }
        x += CAPS_ADVANCES[i] as i16;
    }
}
fn number(x: i16, y: i16, value: u32) {
    let mut digits = [0u8; psx_math::fmt::U32_DEC_MAX];
    text(x, y, psx_math::fmt::u32_dec(&mut digits, value));
}
// The charm board composes into this same glyph and box budget, so it borrows
// the panel primitives rather than keeping a second copy of the font strip.
pub(crate) fn panel_text(x: i16, y: i16, value: &str) {
    text(x, y, value);
}
pub(crate) fn panel_number(x: i16, y: i16, value: u32) {
    number(x, y, value);
}
pub(crate) fn panel_width(value: &str) -> i16 {
    width(value)
}
/// The shop counter raises the same marker over the same world point an NPC
/// does, so it borrows the marker rather than keeping a second copy of it.
pub(crate) fn panel_prompt(camera: (i32, i32), p: [i32; 2], label: &str) {
    prompt_marker(camera, p, label);
}
pub(crate) fn panel_pages(pages: &[&'static [&'static str]], page: usize) {
    panel(pages, page);
}
pub(crate) fn panel_frame(x: i16, y: i16, w: u16, h: u16) {
    unsafe {
        BOXES[0] = RectFlat::new(x, y, w, h, 150, 150, 150);
        BOXES[1] = RectFlat::new(x + 2, y + 2, w - 4, h - 4, 0, 0, 0);
        BOX_COUNT = 2;
    }
}
/// The Geo count sits right of the coin the HUD draws at `GEO_COIN_X`
/// (geo_render::hud_coin), under the masks as the original's does.
pub(crate) const GEO_COIN_X: i16 = 56;
const GEO_NUMBER_X: i16 = GEO_COIN_X + 14;
#[inline(never)] // UI composition must not expand main beyond MIPS branch reach.
#[cfg_attr(not(test), optimize(size))]
pub fn prepare(
    camera: (i32, i32),
    geo: u32,
    pause: Option<(&crate::pause::State, &crate::menu::Settings)>,
    total_masks: u16,
    save_ui: crate::save_ui::Ui,
) {
    unsafe {
        USED = 0;
        BOX_COUNT = 0;
        BACKBOARD_COUNT = 0;
        let board = pause.is_some_and(|(p, _)| (p.charms.open || p.cheats) && !p.confirm_reset);
        crate::hud::hide(board);
        if !board {
            let geo_y = crate::hud::geo_y(total_masks);
            number(GEO_NUMBER_X, geo_y, geo);
            coin(GEO_COIN_X, geo_y - 1);
        }
        if let Some((p, settings)) = pause {
            // Every pause screen is the game dimmed behind the source's own ornaments, with
            // a soft light behind the chosen row, not a box. The charm board keeps its frame.
            let fleurs = crate::dialogue_art::pause_prepared();
            if p.confirm_reset {
                pause_backdrop(fleurs, LIST_Y + 7 * LIST_PITCH + 3);
                text(160 - width("Reset session?") / 2, 108, "Reset session?");
                text(
                    160 - width("Back to the new game spawn.") / 2,
                    130,
                    "Back to the new game spawn.",
                );
            } else if p.charms.open {
                crate::charms::panel(&p.charms);
            } else if p.cheats {
                // Thirteen rows do not leave room for the crest: the dim and the glow only.
                pause_backdrop(false, 0);
                caps_text(160 - caps_width("Cheats") / 2, 12, "Cheats", 0x80);
                // Seventeen rows at the font's size do not fit: a window of eleven follows the cursor.
                const WINDOW: usize = 11;
                const TOP: i16 = 32;
                const CHEAT_PITCH: i16 = 14;
                let total = crate::menu::state::CHEAT_ITEMS.len() + 5;
                let first = p.row.saturating_sub(WINDOW / 2).min(total - WINDOW);
                row_glow(TOP + (p.row - first) as i16 * CHEAT_PITCH - 1, CHEAT_PITCH);
                for (i, line) in crate::menu::state::CHEAT_ITEMS
                    .iter()
                    .chain(
                        [
                            "Restore health + SOUL",
                            "Add 5 Lifeblood masks",
                            "Turn all cheats off",
                            "Reset session",
                            "Back",
                        ]
                        .iter(),
                    )
                    .enumerate()
                    .skip(first)
                    .take(WINDOW)
                {
                    let y = TOP + (i - first) as i16 * CHEAT_PITCH;
                    text_gain(46, y, line, row_gain(p.row == i));
                    if i < crate::menu::state::CHEAT_ITEMS.len() {
                        text_gain(
                            255,
                            y,
                            if settings.cheats.enabled(i) {
                                "ON"
                            } else {
                                "OFF"
                            },
                            row_gain(p.row == i),
                        );
                    }
                }
                // More rows above or below the window.
                if first > 0 {
                    text(296, 14, "^");
                }
                if first + WINDOW < total {
                    text(157, TOP + WINDOW as i16 * CHEAT_PITCH, "v");
                }
            } else if p.controls {
                pause_backdrop(fleurs, LIST_Y + 8 * 13 + 3);
                for (i, line) in [
                    "D-pad / left stick: Move",
                    "X: Jump    Square: Nail    Up: Inspect",
                    "O: Cast (tap) or Focus (hold)",
                    "R1: Quick Cast    Air Down + Sq: Pogo",
                    "R2: Dash    Hold L2: Crystal Heart",
                    "Hold Triangle: Dream Nail",
                    "L1: Quick Map (needs a map)",
                    "Select: Charms    Start: Pause",
                ]
                .iter()
                .enumerate()
                {
                    text(160 - width(line) / 2, LIST_Y + i as i16 * 13, line);
                }
            } else {
                pause_backdrop(fleurs, LIST_Y + 7 * LIST_PITCH + 3);
                row_glow(LIST_Y + p.row as i16 * LIST_PITCH - 1, LIST_PITCH);
                for (i, line) in [
                    "Resume",
                    "Sound effects",
                    "Ambience",
                    "Music",
                    "Controls",
                    "Cheats",
                    "Charms",
                ]
                .iter()
                .enumerate()
                {
                    let y = LIST_Y + i as i16 * LIST_PITCH;
                    caps_text(LIST_X, y + 1, line, row_gain(p.row == i));
                }
                for (i, volume) in [settings.sfx, settings.ambience, settings.music]
                    .iter()
                    .enumerate()
                {
                    let y = LIST_Y + (i as i16 + 1) * LIST_PITCH;
                    // The source's volume rows read the bare number, 0 to 100.
                    number(VOLUME_X, y, u32::from(*volume) * 10);
                }
            }
        } else if save_ui != crate::save_ui::Ui::Idle {
            save_notice(save_ui);
        } else if let Some(id) = STATE.active {
            panel(POINTS[id].pages, STATE.page);
        } else if let Some(id) = CONVO.active {
            panel(NPC_LINES[id].conversations[CONVO.entry].pages, CONVO.page);
        } else if crate::mapper::prepare(camera, geo) {
        } else if crate::shaman::prepare(camera) {
        } else if let Some(id) = STATE.prompt {
            prompt_marker(camera, POINTS[id].marker, POINTS[id].label);
        } else if let Some(id) = CONVO.prompt {
            prompt_marker(camera, NPC_LINES[id].marker, NPC_LINES[id].label);
        }
        #[cfg(feature = "card-diag")]
        crate::card_diag::overlay();
    }
}
/// The bench's Save prompt and the notices that follow it: Saving, Saved, the
/// format question and a failure, each in the conversation box.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn save_notice(ui: crate::save_ui::Ui) {
    use crate::save_ui::Ui;
    // A line centred on the box, and the Yes and No rows with the cursor.
    let centered = |y: i16, line: &str| text(160 - width(line) / 2, y, line);
    let rows = |row: u8, y: i16| {
        for (i, label) in ["Yes", "No"].iter().enumerate() {
            let y = y + i as i16 * ORNATE_PITCH;
            text(146, y, label);
            if row as usize == i {
                text(130, y, ">");
            }
        }
    };
    match ui {
        Ui::Idle => {}
        // Seated at a bench. The source saves silently; this port asks first.
        Ui::Ask(row) => {
            let y = ornate_frame(3, Under::Nothing, None);
            centered(y, "Save game?");
            rows(row, y + ORNATE_PITCH);
        }
        Ui::Format(row) => {
            let y = ornate_frame(4, Under::Nothing, None);
            centered(y, "Memory card is not formatted.");
            centered(y + ORNATE_PITCH, "Format it and save?");
            rows(row, y + 2 * ORNATE_PITCH);
        }
        Ui::Saving { format, .. } => {
            let y = ornate_frame(2, Under::Nothing, None);
            centered(
                y,
                if format {
                    "Formatting and saving..."
                } else {
                    "Saving..."
                },
            );
            centered(y + ORNATE_PITCH, "Do not remove the memory card.");
        }
        Ui::Saved(_) => {
            // Low on the screen, out of the way of the bench.
            let top = 240 - ornate_height(1, Under::Nothing) - 6;
            let y = ornate_frame(1, Under::Nothing, Some(top));
            centered(y, "Game saved");
        }
        Ui::Failed(fault) => {
            let y = ornate_frame(4, Under::Stop, None);
            centered(y, "Save failed");
            centered(y + ORNATE_PITCH, fault.message());
            centered(y + 2 * ORNATE_PITCH, fault.hint());
            let (hardware, exchange) = unsafe {
                (
                    crate::save::HK_CARD_FAULT,
                    crate::save::HK_CARD_FAULT_EXCHANGE.min(999),
                )
            };
            let mut digits = [0u8; psx_math::fmt::U32_DEC_MAX];
            let code = psx_math::fmt::u32_dec(&mut digits, fault as u32 * 10_000 + hardware * 1000 + exchange);
            let w = width("Code ") + width(code);
            text(160 - w / 2, y + 3 * ORNATE_PITCH, "Code ");
            text(160 - w / 2 + width("Code "), y + 3 * ORNATE_PITCH, code);
        }
    }
}
/// Whether a tablet or an NPC conversation is up, which is when the panel
/// wants its ornaments in the frame's animation working set (`frame.rs`).
#[inline(never)] // `frame::render` is large enough already.
pub(crate) fn panel_up(save_ui: crate::save_ui::Ui) -> bool {
    save_ui != crate::save_ui::Ui::Idle
        || unsafe { STATE.active.is_some() || CONVO.active.is_some() }
        || crate::mapper::open()
        || crate::shaman::open()
}
/// The one text panel, shared by a tablet and an NPC conversation (and, through `panel_box`, by
/// Cornifer, the Snail Shaman and the bench's notices). A frame
/// whose working set carries the ornaments draws the source's box (fleurs
/// above and below the centred text, the arrow or the stop under it, over a
/// dark backboard); any other frame draws the plain box, which is also what
/// a view that has claimed the whole cache leaves room for.
fn panel(pages: &[&'static [&'static str]], page: usize) {
    ornate(pages[page], page + 1 < pages.len());
}
/// One ornament piece at (`x`, `y`): a textured rectangle per tile, 1:1.
fn ornament(piece: (usize, usize, u16, u16), x: i16, y: i16) {
    ornament_tint(piece, x, y, 0x80);
}
/// One art piece of the charm board, drawn 1:1 at (`x`, `y`) at a brightness: 0x80 as it is.
pub(crate) fn board_art(piece: (usize, usize, u16, u16), x: i16, y: i16, tint: u8) {
    ornament_tint(piece, x, y, tint);
}
/// One textured rectangle for a caller that has packed its own words (the area cards' glyphs):
/// `tpage` is the page word, `uv` the texel and CLUT words and `wh` the size as the GPU takes them.
/// A frame with no room left draws without it, as the packet table is the panels' too.
pub(crate) fn raw_rect(x: i16, y: i16, tpage: u16, color: u32, uv: u32, wh: u32) {
    unsafe {
        if USED >= CAP {
            return;
        }
        let p = &mut GLYPHS[USED];
        USED += 1;
        p.mode = 0xe100_0000 | u32::from(tpage);
        p.color = color;
        p.xy = x as u16 as u32 | ((y as u16 as u32) << 16);
        p.uv = uv;
        p.wh = wh;
    }
}
fn ornament_tint(piece: (usize, usize, u16, u16), x: i16, y: i16, tint: u8) {
    ornament_color(piece, x, y, u32::from(tint) * 0x0001_0101);
}
/// One art piece of the charm board, drawn 1:1 at (`x`, `y`) in a colour (r, g, b): the grey
/// ramp times it, 0x80 a channel as it is.
pub(crate) fn board_art_rgb(piece: (usize, usize, u16, u16), x: i16, y: i16, (r, g, b): (u8, u8, u8)) {
    ornament_color(piece, x, y, u32::from(b) << 16 | u32::from(g) << 8 | u32::from(r));
}
fn ornament_color(piece: (usize, usize, u16, u16), x: i16, y: i16, color: u32) {
    let clut = Clut::new(320, 481).uv_clut_word() as u32;
    for tile in piece.0..piece.0 + piece.1 {
        let (_, w, h, dx) = crate::dialogue_art::DIALOGUE_TILES[tile];
        let key = crate::dialogue_art::KEY_BASE + tile as u16;
        let (u, v) = crate::render::animation_uv(key);
        unsafe {
            assert!(USED < CAP);
            let p = &mut GLYPHS[USED];
            USED += 1;
            p.mode = 0xe100_0000 | crate::render::animation_tpage_word(key) as u32;
            p.color = 0x6400_0000 | color;
            p.xy = (x + dx as i16) as u16 as u32 | ((y as u16 as u32) << 16);
            p.uv = u32::from(u) | (u32::from(v) << 8) | (clut << 16);
            p.wh = u32::from(w) | (u32::from(h) << 16);
        }
    }
}
/// Pixels between a line and the next in the ornate panel, and between the top fleur and the
/// first line.
const ORNATE_PITCH: i16 = 16;
const ORNATE_GAP: i16 = 6;
/// What the ornate panel carries under its last line: the arrow while there is more to read, the
/// stop at the end, or nothing for a question or a notice.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Under {
    Nothing,
    Arrow,
    Stop,
}
/// The panel's height for `rows` lines.
fn ornate_height(rows: i16, under: Under) -> i16 {
    use crate::dialogue_art::{DIALOGUE_FLEUR_BOTTOM, DIALOGUE_FLEUR_TOP, ICON_H};
    let below = if under == Under::Nothing { 6 } else { 4 + ICON_H + 4 };
    DIALOGUE_FLEUR_TOP.3 as i16
        + ORNATE_GAP
        + rows * ORNATE_PITCH
        + below
        + DIALOGUE_FLEUR_BOTTOM.3 as i16
}
/// The conversation box around `rows` lines: the fleurs, the arrow or the stop and the backboard,
/// from `top` or, by default, near the top of the screen like the source's, so the speaker below
/// stays in view; clear of the HUD's masks and Geo count above, and a full page of eight lines is
/// exactly what the screen holds from there. Returns the y of the first line, which the caller
/// draws (`ORNATE_PITCH` apart).
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
fn ornate_frame(rows: i16, under: Under, top: Option<i16>) -> i16 {
    use crate::dialogue_art::{
        DIALOGUE_ARROW, DIALOGUE_FLEUR_BOTTOM, DIALOGUE_FLEUR_TOP, DIALOGUE_STOP, ICON_H, ICON_W,
    };
    let top_h = DIALOGUE_FLEUR_TOP.3 as i16;
    let bottom_h = DIALOGUE_FLEUR_BOTTOM.3 as i16;
    let total = ornate_height(rows, under);
    let top = top.unwrap_or_else(|| 52.min(240 - total).max(40));
    // A frame whose working set lacks the ornaments (the view behind has claimed the whole
    // cache) gets a plain box of the same size, so the lines sit where they would.
    if !crate::dialogue_art::prepared() {
        panel_frame(10, top - 4, 300, (total + 8) as u16);
        return top + top_h + ORNATE_GAP;
    }
    let fleur = |piece: (usize, usize, u16, u16)| 160 - piece.2 as i16 / 2;
    ornament(DIALOGUE_FLEUR_TOP, fleur(DIALOGUE_FLEUR_TOP), top);
    let first = top + top_h + ORNATE_GAP;
    let after = first + rows * ORNATE_PITCH;
    let bottom_y = match under {
        Under::Nothing => after + 6,
        Under::Arrow | Under::Stop => {
            let icon_y = after + 4;
            let icon = if under == Under::Arrow { DIALOGUE_ARROW } else { DIALOGUE_STOP };
            ornament(icon, 160 - ICON_W / 2 + (ICON_W - icon.2 as i16) / 2, icon_y);
            icon_y + ICON_H + 4
        }
    };
    ornament(DIALOGUE_FLEUR_BOTTOM, fleur(DIALOGUE_FLEUR_BOTTOM), bottom_y);
    // The backboard: a 3x3 grid of translucent quads that darkens what is behind
    // the panel by a fixed amount inside and fades to nothing over the outer 24
    // pixels sideways and 10 above and below, like the source's soft backboard.
    const LEFT: i16 = 8;
    const RIGHT: i16 = 312;
    const FADE_X: i16 = 24;
    const FADE_Y: i16 = 10;
    const DARK: u8 = 96;
    let (y0, y1) = (top - 6 - FADE_Y, bottom_y + bottom_h + 6 + FADE_Y);
    let xs = [LEFT, LEFT + FADE_X, RIGHT - FADE_X, RIGHT];
    let ys = [y0, y0 + FADE_Y, y1 - FADE_Y, y1];
    let node = |i: usize, j: usize| if i % 3 == 0 || j % 3 == 0 { 0 } else { DARK };
    for r in 0..3 {
        for c in 0..3 {
            let (a, b, d, e) = (node(c, r), node(c + 1, r), node(c, r + 1), node(c + 1, r + 1));
            unsafe {
                BACKBOARD[r * 3 + c] = QuadGouraudBlended::new(
                    [(xs[c], ys[r]), (xs[c + 1], ys[r]), (xs[c], ys[r + 1]), (xs[c + 1], ys[r + 1])],
                    [(a, a, a), (b, b, b), (d, d, d), (e, e, e)],
                    BlendMode::Subtract,
                );
            }
        }
    }
    unsafe { BACKBOARD_COUNT = 9 };
    first
}
/// The ornate panel with one page of text: the page's lines centred, arrow or stop under them.
fn ornate(lines: &[&str], more: bool) {
    let rows = lines.len().max(1) as i16;
    let first = ornate_frame(rows, if more { Under::Arrow } else { Under::Stop }, None);
    for (i, line) in lines.iter().enumerate() {
        text(160 - width(line) / 2, first + i as i16 * ORNATE_PITCH, line);
    }
}
/// The conversation box for a caller that draws its own `rows` lines (Cornifer's question, the
/// map notice, the Shaman's spell): returns the y of the first line, `ORNATE_PITCH` per row.
pub(crate) fn panel_box(rows: i16) -> i16 {
    ornate_frame(rows, Under::Nothing, None)
}
/// Where the pause screens put their pieces: the crest, the list under it and
/// the flourish under that, all centred. The HUD's masks and Geo count sit above.
const FLEUR_Y: i16 = 44;
const LIST_Y: i16 = 92;
const LIST_PITCH: i16 = 14;
const LIST_X: i16 = 64;
const VOLUME_X: i16 = 204;
/// How much the game behind a pause screen is darkened, and how bright the light behind the
/// chosen row is (the source's `menu_backboard_*` and `List_Selection_Glow` sprites).
const PAUSE_DIM: u8 = 150;
const ROW_GLOW: u8 = 56;
/// The chosen row's text is full brightness, the others a step down.
pub(crate) fn row_gain(chosen: bool) -> u8 {
    if chosen {
        0x80
    } else {
        0x58
    }
}
/// The pause screens' backdrop: the whole view darkened and, when the ornaments are in
/// the frame's working set, the crest above the list and the flourish at `bottom`.
fn pause_backdrop(fleurs: bool, bottom: i16) {
    unsafe {
        BACKBOARD[0] = QuadGouraudBlended::new(
            [(0, 0), (320, 0), (0, 240), (320, 240)],
            [(PAUSE_DIM, PAUSE_DIM, PAUSE_DIM); 4],
            BlendMode::Subtract,
        );
        BACKBOARD_COUNT = 1;
        BOX_COUNT = 0;
    }
    if fleurs {
        use crate::dialogue_art::{DIALOGUE_PAUSE_BOTTOM, DIALOGUE_PAUSE_TOP};
        ornament(DIALOGUE_PAUSE_TOP, 160 - DIALOGUE_PAUSE_TOP.2 as i16 / 2, FLEUR_Y);
        ornament(DIALOGUE_PAUSE_BOTTOM, 160 - DIALOGUE_PAUSE_BOTTOM.2 as i16 / 2, bottom);
    }
}
/// A soft light behind the chosen row, brightest in the middle and gone at both ends.
fn row_glow(y: i16, height: i16) {
    glow(56, 264, y, height);
}
fn glow(left: i16, right: i16, y: i16, height: i16) {
    let mid = (left + right) / 2;
    let (bright, dark) = ((ROW_GLOW, ROW_GLOW, ROW_GLOW), (0, 0, 0));
    unsafe {
        let n = BACKBOARD_COUNT;
        BACKBOARD[n] = QuadGouraudBlended::new(
            [(left, y), (mid, y), (left, y + height), (mid, y + height)],
            [dark, bright, dark, bright],
            BlendMode::Add,
        );
        BACKBOARD[n + 1] = QuadGouraudBlended::new(
            [(mid, y), (right, y), (mid, y + height), (right, y + height)],
            [bright, dark, bright, dark],
            BlendMode::Add,
        );
        BACKBOARD_COUNT = n + 2;
    }
}
/// The charm board's backdrop, as the source's `Charms` pane: the view blacked out, a purple glow
/// strongest up and to the left (a 3x3 grid of additive quads shaded from the middle out to
/// nothing at the screen's edge), and a soft light behind the chosen slot.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub(crate) fn board_backdrop(left: i16, right: i16, y: i16, height: i16) {
    // Node colours at the four inner corners of the glow's 4x4 node grid; the outer ring is black.
    const XS: [i16; 4] = [0, 60, 180, 320];
    const YS: [i16; 4] = [0, 30, 120, 240];
    let tone = |i: usize, j: usize| match (i, j) {
        (1, 1) => (140, 52, 196),
        (2, 1) => (96, 36, 140),
        (1, 2) => (84, 30, 124),
        (2, 2) => (44, 16, 66),
        _ => (0, 0, 0),
    };
    unsafe {
        BACKBOARD[0] = QuadGouraudBlended::new(
            [(0, 0), (320, 0), (0, 240), (320, 240)],
            [(255, 255, 255); 4],
            BlendMode::Subtract,
        );
        for r in 0..3 {
            for c in 0..3 {
                BACKBOARD[1 + r * 3 + c] = QuadGouraudBlended::new(
                    [(XS[c], YS[r]), (XS[c + 1], YS[r]), (XS[c], YS[r + 1]), (XS[c + 1], YS[r + 1])],
                    [tone(c, r), tone(c + 1, r), tone(c, r + 1), tone(c + 1, r + 1)],
                    BlendMode::Add,
                );
            }
        }
        BACKBOARD_COUNT = 10;
        BOX_COUNT = 0;
    }
    glow(left, right, y, height);
}
/// A one texel rule, as the source's under its "Collection" label.
pub(crate) fn board_rule(x: i16, y: i16, w: u16) {
    unsafe {
        BOXES[0] = RectFlat::new(x, y, w, 1, 150, 130, 176);
        BOX_COUNT = 1;
    }
}
/// A board art piece tinted (r, g, b) and added to what is behind it: the soul vessel's glow.
pub(crate) fn board_glow_art(piece: (usize, usize, u16, u16), x: i16, y: i16, tint: (u8, u8, u8)) {
    let clut = Clut::new(320, 481).uv_clut_word() as u32;
    let tile = piece.0;
    let (_, w, h, dx) = crate::dialogue_art::DIALOGUE_TILES[tile];
    let key = crate::dialogue_art::KEY_BASE + tile as u16;
    let (u, v) = crate::render::animation_uv(key);
    unsafe {
        assert!(USED < CAP);
        let p = &mut GLYPHS[USED];
        USED += 1;
        // Semi-transparency mode 1 (B+F) in the page word, and the semi-transparent command.
        p.mode = 0xe100_0000 | u32::from(crate::render::animation_tpage_word(key) & !0x60 | 0x20);
        p.color = 0x6600_0000
            | u32::from(tint.2) << 16
            | u32::from(tint.1) << 8
            | u32::from(tint.0);
        p.xy = (x + dx as i16) as u16 as u32 | ((y as u16 as u32) << 16);
        p.uv = u32::from(u) | (u32::from(v) << 8) | (clut << 16);
        p.wh = u32::from(w) | (u32::from(h) << 16);
    }
}
pub(crate) fn panel_text_gain(x: i16, y: i16, value: &str, gain: u8) {
    text_gain(x, y, value, gain);
}
/// The source prompt marker's label, over the marker's own world point.
fn prompt_marker(camera: (i32, i32), p: [i32; 2], label: &str) {
    let x = 160 + psx_math::int32::mul_shr_i32((p[0] - camera.0) >> 8, crate::KNIGHT_SCALE as i32, 20);
    let y = 120 - psx_math::int32::mul_shr_i32((p[1] - camera.1) >> 8, crate::KNIGHT_SCALE as i32, 20);
    let w = width(label);
    let x = (x - w as i32 / 2).clamp(4, 316 - w as i32) as i16;
    let y = y.clamp(32, 220) as i16;
    unsafe {
        BOXES[0] = RectFlat::new(x - 3, y - 2, w as u16 + 6, 18, 0, 0, 0);
        BOX_COUNT = 1;
    }
    text(x, y, label);
}
/// Called before scenery/HUD insertions: prepend ordering puts the panel last.
pub fn append(ot: &mut OrderingTable<1>) {
    unsafe {
        for i in (0..USED).rev() {
            crate::display::ot_add(ot, 0, &mut GLYPHS[i], 5);
        }
        // Insertion prepends, so this lands between the box and the glyphs: a charm
        // icon sits on the board rather than behind it, and under its own row.
        crate::charms::append_icons(ot);
        for i in (0..BOX_COUNT).rev() {
            crate::display::ot_add(ot, 0, &mut BOXES[i], 3);
        }
        for i in (0..BACKBOARD_COUNT).rev() {
            crate::display::ot_add(
                ot,
                0,
                &mut *(&raw mut BACKBOARD[i]),
                QuadGouraudBlended::WORDS,
            );
        }
    }
}
