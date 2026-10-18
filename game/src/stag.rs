//! The stag network: the bell's toll, the stag's conversation and the trip between stations.
//!
//! The props (grate, bell stand, nail-struck bell, the stag and its arrival animations) are machines of
//! `interactables.rs`; this keeps what the source's `Stag Bell` and `Stag Control` FSMs hold beside them:
//! which stations are open (PlayerData `openedCrossroads`; `openedTown` starts true), where the stag
//! stands (`stagPosition`, one number for the whole network), whether the Knight has met it (`metStag`),
//! and the three screens that talk to the Knight: the toll's yes/no box, the stag's first conversation
//! and the list of stations to travel to. A trip leaves through the same exit fade a scene gate uses and
//! arrives at the destination's platform; the source's stag cinematic is not played.
//!
//! `host/hk-cook/src/stags.rs` cooks the table this reads.
use crate::charms::{CIRCLE as BACK, CROSS as CONFIRM, DOWN, UP};
use crate::dialogue::{panel_frame, panel_number, panel_pages, panel_prompt, panel_text, panel_width};
use crate::persist;
use hk_sim::switches::Event;

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/stags.rs"));

/// PlayerData `metStag`: the stag's first conversation has been heard.
const MET_STAG: u32 = persist::MET_STAG;
/// `levels[3]` holds `stagPosition + 1`; zero is the source's -1, the stag at no station.
const POSITION_LEVEL: usize = persist::STAG_POSITION;
const ALWAYS_OPEN: u8 = 255;
const NAIL: u16 = 0x8000;
const ACTIONS: u16 = UP | DOWN | CONFIRM | BACK | NAIL;

#[no_mangle]
pub static mut HK_STAG_MODE: u32 = 0;
/// `stagPosition + 1`.
#[no_mangle]
pub static mut HK_STAG_POSITION: u32 = 0;
/// Geo the bell has taken, trips begun.
#[no_mangle]
pub static mut HK_STAG_PAID: u32 = 0;
#[no_mangle]
pub static mut HK_STAG_TRIPS: u32 = 0;

fn station_of(scene: usize) -> Option<&'static Station> {
    STATIONS.iter().find(|s| s.scene as usize == scene)
}

fn open_of(s: &Station) -> bool {
    s.open_bit == ALWAYS_OPEN || persist::player(s.open_bit as u32)
}

/// Whether `scene` is a stag station that is open.
pub fn open(scene: usize) -> bool {
    station_of(scene).is_some_and(open_of)
}

/// `stagPosition`: the number of the station the stag stands at, if any.
pub fn position() -> Option<u8> {
    persist::store().levels[POSITION_LEVEL].checked_sub(1)
}

fn set_position(number: u8) {
    persist::set_level(POSITION_LEVEL, number + 1);
    unsafe { HK_STAG_POSITION = number as u32 + 1 }
}

/// The stag is at `scene`'s station and the station is open.
pub fn here(scene: usize) -> bool {
    station_of(scene).is_some_and(|s| open_of(s) && position() == Some(s.number))
}

/// The stag comes to `scene`'s station (`Opened?` sets `stagPosition`).
pub fn arrive(scene: usize) {
    if let Some(s) = station_of(scene) {
        set_position(s.number);
    }
}

const MODE_NONE: u8 = 0;
const MODE_TOLL: u8 = 1;
const MODE_MEET: u8 = 2;
const MODE_MENU: u8 = 3;
const PROMPT_NONE: u8 = 0;
const PROMPT_BELL: u8 = 1;
const PROMPT_STAG: u8 = 2;

struct Screen {
    mode: u8,
    prompt: u8,
    /// The yes/no row, the stations' row.
    row: usize,
    page: usize,
    refused: bool,
    previous: u16,
    release: bool,
}

static mut SCREEN: Screen = Screen { mode: MODE_NONE, prompt: PROMPT_NONE, row: 0, page: 0, refused: false, previous: 0, release: false };

fn screen() -> &'static mut Screen {
    unsafe { &mut *(&raw mut SCREEN) }
}

/// A panel or the press that closed it owns the pad.
pub fn open_screen() -> bool {
    screen().mode != MODE_NONE
}
pub fn consumes_actions() -> bool {
    open_screen() || screen().release
}

/// Forget any screen (a scene load or a death takes the box with it).
pub fn reset() {
    let s = screen();
    *s = Screen { mode: MODE_NONE, prompt: PROMPT_NONE, row: 0, page: 0, refused: false, previous: 0, release: false };
}

/// What the stag module asked of the frame this tick.
pub struct Out {
    /// Geo the bell took.
    pub pay: u32,
    /// A trip: the destination's scene, view and arrival point.
    pub depart: Option<(usize, usize, [i32; 2])>,
}

fn overlaps(body: [i32; 4], b: [i32; 4]) -> bool {
    b[0] <= body[2] && b[2] >= body[0] && b[1] <= body[3] && b[3] >= body[1]
}

/// The stations the Knight may ride to from `here`: the open ones but this.
fn destinations(here: &Station) -> impl Iterator<Item = &'static Station> {
    let number = here.number;
    STATIONS.iter().filter(move |s| s.number != number && open_of(s))
}

fn close(s: &mut Screen) {
    s.mode = MODE_NONE;
    s.refused = false;
    s.release = s.previous & ACTIONS != 0;
}

/// One tick of the station the Knight stands in. `eligible` is the grounded-and-unlocked test the NPC
/// prompt answers, `bits` the held pad word (the release hold watches a button stay down).
pub fn tick(scene: usize, body: [i32; 4], eligible: bool, bits: u16, geo: u32) -> Out {
    let mut out = Out { pay: 0, depart: None };
    let s = screen();
    let pressed = bits & !s.previous;
    s.previous = bits;
    if bits & ACTIONS == 0 {
        s.release = false;
    }
    unsafe { HK_STAG_MODE = s.mode as u32 }
    let Some(st) = station_of(scene) else {
        if s.mode != MODE_NONE {
            close(s);
        }
        s.prompt = PROMPT_NONE;
        return out;
    };
    if s.mode == MODE_NONE {
        s.prompt = PROMPT_NONE;
        if eligible && !s.release {
            if !open_of(st) && overlaps(body, st.inspect) {
                s.prompt = PROMPT_BELL;
            } else if crate::interactables::stag_ready(scene) && overlaps(body, st.range) {
                s.prompt = PROMPT_STAG;
            }
        }
        if pressed & UP != 0 {
            match s.prompt {
                PROMPT_BELL => {
                    s.mode = MODE_TOLL;
                    s.row = 0;
                    s.refused = false;
                }
                PROMPT_STAG => {
                    s.row = 0;
                    s.page = 0;
                    if persist::player(MET_STAG) {
                        s.mode = MODE_MENU;
                    } else {
                        // `Meeting`: metStag is written as the conversation starts.
                        persist::set_player(MET_STAG);
                        s.mode = MODE_MEET;
                    }
                }
                _ => {}
            }
            if s.mode != MODE_NONE {
                s.prompt = PROMPT_NONE;
            }
        }
        unsafe { HK_STAG_MODE = s.mode as u32 }
        return out;
    }
    match s.mode {
        MODE_TOLL => {
            if !overlaps(body, st.inspect) || pressed & BACK != 0 {
                close(s);
            } else if pressed & (UP | DOWN) != 0 {
                s.row ^= 1;
                s.refused = false;
            } else if pressed & CONFIRM != 0 {
                if s.row == 1 {
                    close(s);
                } else if geo >= st.cost as u32 {
                    // `Yes`: the bool, the count of stations, the box falls away and the grate opens.
                    out.pay = st.cost as u32;
                    unsafe { HK_STAG_PAID += st.cost as u32 }
                    if st.open_bit != ALWAYS_OPEN {
                        persist::set_player(st.open_bit as u32);
                    }
                    crate::interactables::send_to(scene, crate::interactables::M_STAG_BELL_BOX, Event::Paid);
                    close(s);
                } else {
                    s.refused = true;
                }
            }
        }
        MODE_MEET => {
            if !overlaps(body, st.range) || pressed & BACK != 0 {
                close(s);
            } else if pressed & CONFIRM != 0 {
                if s.page + 1 < MEET.len() {
                    s.page += 1;
                } else {
                    // `CONVO_FINISH` goes on to the map of stations.
                    s.mode = MODE_MENU;
                    s.row = 0;
                }
            }
        }
        _ => {
            let rows = destinations(st).count();
            if rows == 0 || !overlaps(body, st.range) || pressed & BACK != 0 {
                close(s);
            } else {
                s.row = s.row.min(rows - 1);
                if pressed & DOWN != 0 {
                    s.row = (s.row + 1) % rows;
                } else if pressed & UP != 0 {
                    s.row = (s.row + rows - 1) % rows;
                } else if pressed & CONFIRM != 0 {
                    if let Some(to) = destinations(st).nth(s.row) {
                        // `Go To Stag Cutscene` writes `stagPosition` before the scene is left.
                        set_position(to.number);
                        unsafe { HK_STAG_TRIPS += 1 }
                        out.depart = Some((to.scene as usize, to.region as usize, to.spawn));
                        close(s);
                    }
                }
            }
        }
    }
    unsafe { HK_STAG_MODE = s.mode as u32 }
    out
}

const FOOTER_MENU: &str = "X: travel    O: leave";
const FOOTER_TOLL: &str = "X: choose    O: leave";

mod presentation {
    use super::*;

    /// What the screen draws: the prompt marker over the bell or the stag, or one of the three panels.
    pub fn prepare(scene: usize, camera: (i32, i32), paused: bool) {
        if paused {
            return;
        }
        let Some(st) = station_of(scene) else { return };
        let s = screen();
        match s.mode {
            MODE_NONE => match s.prompt {
                PROMPT_BELL => panel_prompt(camera, st.bell_marker, INSPECT),
                PROMPT_STAG => panel_prompt(camera, st.stag_marker, if persist::player(MET_STAG) { TRAVEL } else { LISTEN }),
                _ => {}
            },
            MODE_TOLL => {
                panel_frame(40, 70, 240, 100);
                for (i, line) in TOLL.iter().enumerate() {
                    panel_text(160 - panel_width(line) / 2, 82 + i as i16 * 11, line);
                }
                panel_number(160 - 12, 100, st.cost as u32);
                panel_text(160 + 8, 100, "Geo");
                for (i, label) in [YES, NO].iter().enumerate() {
                    let y = 122 + i as i16 * 12;
                    if s.row == i {
                        panel_text(120, y, ">");
                    }
                    panel_text(134, y, label);
                }
                let footer = if s.refused { NOT_ENOUGH_GEO } else { FOOTER_TOLL };
                panel_text(160 - panel_width(footer) / 2, 154, footer);
            }
            MODE_MEET => panel_pages(MEET, s.page),
            _ => {
                panel_frame(60, 60, 200, 110);
                panel_text(160 - panel_width(TRAVEL) / 2, 70, TRAVEL);
                for (i, to) in destinations(st).take(6).enumerate() {
                    let y = 94 + i as i16 * 12;
                    if s.row == i {
                        panel_text(76, y, ">");
                    }
                    panel_text(90, y, to.name);
                }
                panel_text(160 - panel_width(FOOTER_MENU) / 2, 152, FOOTER_MENU);
            }
        }
    }
}
pub use presentation::prepare;
