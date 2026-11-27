//! Sly's rescue in Room_ruinhouse (host/sly.py documents the FSM this follows
//! and asserts it against the install).
//!
//! Sly sits dazed in the ruined house off Crossroads_04. His `Conversation
//! Control` makes one choice, on `slyRescued`: unset, `Meet` sets it as the box
//! comes up and he speaks SLY_RESCUE_1 to SLY_RESCUE_3; set, `Repeat` speaks
//! SLY_RESCUE_REPEAT. `slyRescued` is what Dirtmouth's `Check Opened` reads to
//! show his shop and its door_sly (`world::gate_takes`), and his own `Active?`
//! destroys him on any later load of the room once it is set (`enter_scene`).
//!
//! The panel is the dialogue panel's: X pages on, O closes. The pages are one
//! byte string with line and page tables, so the whole text travels in the
//! `shop` room module, which both of Sly's rooms carry, with the code that
//! reads it (host/code_modules.py).
//!
//! Not reproduced: his `Wake` clip and the steam stopping (he stays on `Dazed`),
//! the conversation's sounds, the dream dialogue.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/sly.rs"));

const UP: u16 = 0x10;
const DOWN: u16 = 0x40;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;
const SQUARE: u16 = 0x8000;
/// The buttons the conversation owns while it is up.
const ACTIONS: u16 = UP | DOWN | CROSS | CIRCLE | SQUARE;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Event {
    pub opened: bool,
    /// `Meet` ran: slyRescued went from unset to set.
    pub rescued: bool,
}

pub struct Convo {
    /// The page up (an index into the page table), or None.
    page: Option<u8>,
    /// The page after the conversation's last.
    end: u8,
    pub prompt: bool,
    previous: u16,
    release: bool,
}
impl Convo {
    pub const fn new() -> Self {
        Self { page: None, end: 0, prompt: false, previous: 0, release: false }
    }
    pub fn open(&self) -> bool {
        self.page.is_some()
    }
    pub fn page(&self) -> Option<u8> {
        self.page
    }
    pub fn more(&self) -> bool {
        self.page.is_some_and(|p| p + 1 < self.end)
    }
    pub fn consumes_actions(&self) -> bool {
        self.open() || self.release
    }
    /// A tick the conversation is not stepped (its room's module is not loaded): see `mapper::Convo::idle`.
    pub fn idle(&mut self, bits: u16) {
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
    }
    pub fn cancel(&mut self) {
        self.page = None;
        self.prompt = false;
        self.release = self.previous & ACTIONS != 0;
    }
    /// One tick. `in_range` is whether the hero body is inside Sly's talk
    /// trigger in `scene`; `rescued` is slyRescued, which `Meet` sets.
    #[inline(always)]
    pub fn step(&mut self, rescued: &mut bool, scene: usize, in_range: bool, eligible: bool, bits: u16) -> Event {
        let pressed = bits & !self.previous;
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
        let mut event = Event::default();
        let Some(page) = self.page else {
            self.prompt = eligible && !self.release && in_range && scene == SLY_SCENE;
            if self.prompt && pressed & UP != 0 {
                event.opened = true;
                // Convo Choice. `Meet` and `Repeat` both write slyRescued as they are entered, before their
                // first line is spoken.
                let first = if *rescued { SLY_RESCUE_PAGES } else { 0 };
                self.end = if *rescued { SLY_PAGE_COUNT } else { SLY_RESCUE_PAGES };
                event.rescued = !*rescued;
                *rescued = true;
                self.page = Some(first);
                self.prompt = false;
            }
            return event;
        };
        if scene != SLY_SCENE || pressed & CIRCLE != 0 {
            self.cancel();
        } else if pressed & CROSS != 0 {
            if page + 1 < self.end {
                self.page = Some(page + 1);
            } else {
                self.cancel();
            }
        }
        event
    }
}

/// One page's lines out of the text tables, into `out`; returns how many.
#[inline(always)]
pub fn page_lines(page: u8, out: &mut [&'static str; MAX_LINES]) -> usize {
    let first = SLY_PAGES[page as usize] as usize;
    let last = SLY_PAGES[page as usize + 1] as usize;
    let mut n = 0;
    while n < last - first && n < MAX_LINES {
        let (a, b) = (SLY_LINES[first + n] as usize, SLY_LINES[first + n + 1] as usize);
        // The cook writes printable ASCII only (host/npc_dialogue.py `_quote`'s rule).
        out[n] = unsafe { core::str::from_utf8_unchecked(&SLY_TEXT[a..b]) };
        n += 1;
    }
    n
}

/// The guest half: the one conversation, its telemetry, the scene-load latch
/// and the drawing. `tick` and `prepare` are in the `shop` room module; the rest is
/// resident and touches none of the module's tables.
#[cfg(not(test))]
mod live {
    use super::*;
    use crate::player_data::{self as pd, SLY_RESCUED};
    static mut CONVO: Convo = Convo::new();
    /// Sly was gone when this scene loaded (his `Active?`): slyRescued was set.
    static mut GONE: bool = false;
    /// Conversations opened, for routes.
    #[no_mangle]
    pub static mut HK_SLY_TALKS: u32 = 0;
    /// The page up, plus one; 0 closed.
    #[no_mangle]
    pub static mut HK_SLY_PAGE: u32 = 0;
    /// PlayerData slyRescued, as of the last scene load or conversation.
    #[no_mangle]
    pub static mut HK_SLY_RESCUED: u32 = 0;
    pub fn prompting() -> bool {
        unsafe { (*(&raw const CONVO)).prompt }
    }
    pub fn open() -> bool {
        unsafe { (*(&raw const CONVO)).open() }
    }
    pub fn consumes_actions() -> bool {
        unsafe { (*(&raw const CONVO)).consumes_actions() }
    }
    pub fn idle(bits: u16) {
        unsafe { (*(&raw mut CONVO)).idle(bits) }
    }
    pub fn cancel() {
        unsafe {
            (*(&raw mut CONVO)).cancel();
            HK_SLY_PAGE = 0;
        }
    }
    pub fn speaking() -> Option<u32> {
        open().then_some(SLY_SOURCE)
    }
    pub fn owns(source: u32) -> bool {
        source == SLY_SOURCE
    }
    /// A scene has loaded: `Active?` reads slyRescued once, so a Sly rescued on
    /// this visit stays until the Knight leaves.
    pub fn enter_scene(scene: usize) {
        let rescued = pd::store().flag(SLY_RESCUED);
        unsafe {
            GONE = scene == SLY_SCENE && rescued;
            HK_SLY_RESCUED = u32::from(rescued);
        }
    }
    /// Whether this cooked NPC is Sly and gone from the room as it loaded.
    pub fn gone(scene: usize, source: u32) -> bool {
        (unsafe { GONE }) && scene == SLY_SCENE && source == SLY_SOURCE
    }
    /// One tick.
    #[inline(never)]
    pub fn tick(scene: usize, in_range: bool, eligible: bool, bits: u16) {
        let convo = unsafe { &mut *(&raw mut CONVO) };
        let mut rescued = pd::store().flag(SLY_RESCUED);
        let event = convo.step(&mut rescued, scene, in_range, eligible, bits);
        pd::store().put(SLY_RESCUED, rescued);
        unsafe {
            HK_SLY_TALKS += u32::from(event.opened);
            HK_SLY_PAGE = convo.page().map_or(0, |p| u32::from(p) + 1);
            HK_SLY_RESCUED = u32::from(rescued);
        }
        if event.opened {
            crate::audio::ui_confirm();
        }
    }
    /// Compose the panel or the prompt label. False when there is nothing of
    /// Sly's to draw.
    #[inline(never)]
    pub fn prepare(camera: (i32, i32)) -> bool {
        use crate::dialogue::{panel_lines, panel_prompt};
        let convo = unsafe { &*(&raw const CONVO) };
        if let Some(page) = convo.page() {
            let mut lines = [""; MAX_LINES];
            let n = page_lines(page, &mut lines);
            panel_lines(&lines[..n], convo.more());
            return true;
        }
        if !convo.prompt {
            return false;
        }
        panel_prompt(camera, SLY_MARKER, SLY_LABEL);
        true
    }
}
#[cfg(not(test))]
pub use live::*;
