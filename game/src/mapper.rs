//! Cornifer's Crossroads conversation and the map he sells (host/cornifer.py
//! documents the FSM this follows and asserts it against the install).
//!
//! The panel is the dialogue panel's: same box, same pages, same X to page on
//! and O to close. The yes/no box is the source's `Text YN` with its `Toll
//! Cost`: the price shows beside the question, `Yes` is not selectable while
//! the wallet is short, and Yes takes the Geo before the map is handed over.
//! The first map ever raises the source's first-map prompt (`Get F Map` to
//! `Map Input`), which waits for jump, attack, cast or the map button.
use crate::persist;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/cornifer.rs"));

const UP: u16 = 0x10;
const DOWN: u16 = 0x40;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;
const SQUARE: u16 = 0x8000;
/// The buttons the conversation owns while it is up.
const ACTIONS: u16 = UP | DOWN | CROSS | CIRCLE | SQUARE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Talk {
    Meet,
    Greet,
    Again,
    Refuse,
    NotEnough,
    Bought,
    Intro1,
    Intro2,
    Iselda,
}
impl Talk {
    pub fn pages(self) -> &'static [&'static [&'static str]] {
        match self {
            Talk::Meet => MEET,
            Talk::Greet => GREET,
            Talk::Again => AGAIN,
            Talk::Refuse => REFUSE,
            Talk::NotEnough => NOT_ENOUGH,
            Talk::Bought => BOUGHT,
            Talk::Intro1 => INTRO_1,
            Talk::Intro2 => INTRO_2,
            Talk::Iselda => ISELDA,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Pages(Talk, usize),
    YesNo(u8),
    FirstMap,
}

/// The PlayerData the conversation reads and writes, so the state machine can
/// be run on the host against a plain struct.
pub trait Player {
    fn get(&self, bit: u32) -> bool;
    fn set(&mut self, bit: u32);
}

pub struct Convo {
    pub step: Option<Step>,
    pub prompt: bool,
    /// `Spoken`: an FSM variable, so it lasts while the scene does.
    spoken: bool,
    spoken_scene: usize,
    previous: u16,
    release: bool,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Event {
    pub opened: bool,
    pub paid: u32,
    pub bought: bool,
}
impl Convo {
    pub const fn new() -> Self {
        Self {
            step: None,
            prompt: false,
            spoken: false,
            spoken_scene: usize::MAX,
            previous: 0,
            release: false,
        }
    }
    pub fn open(&self) -> bool {
        self.step.is_some()
    }
    pub fn consumes_actions(&self) -> bool {
        self.open() || self.release
    }
    pub fn cancel(&mut self) {
        self.step = None;
        self.prompt = false;
        self.release = self.previous & ACTIONS != 0;
    }
    /// One tick. `in_range` is whether the hero body is inside Cornifer's talk
    /// trigger in `scene`; `geo` is the wallet.
    pub fn step(
        &mut self,
        pd: &mut impl Player,
        scene: usize,
        in_range: bool,
        eligible: bool,
        bits: u16,
        geo: u32,
        map_button: u16,
    ) -> Event {
        let pressed = bits & !self.previous;
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
        if scene != self.spoken_scene {
            self.spoken = false;
            self.spoken_scene = scene;
        }
        let mut event = Event::default();
        let Some(step) = self.step else {
            self.prompt = eligible && !self.release && in_range && scene == CORNIFER_SCENE;
            if self.prompt && pressed & UP != 0 {
                event.opened = true;
                // Open map Shop, then Convo Choice.
                pd.set(persist::OPENED_MAPPER_SHOP);
                self.step = Some(if !pd.get(persist::MET_CORNIFER) {
                    pd.set(persist::MET_CORNIFER);
                    self.spoken = true;
                    Step::Pages(Talk::Meet, 0)
                } else if pd.get(persist::MAP_CROSSROADS) {
                    if !pd.get(persist::CORNIFER_INTRODUCED) {
                        pd.set(persist::CORNIFER_INTRODUCED);
                        Step::Pages(Talk::Intro1, 0)
                    } else {
                        Step::Pages(Talk::Bought, 0)
                    }
                } else if self.spoken {
                    Step::Pages(Talk::Again, 0)
                } else {
                    self.spoken = true;
                    Step::Pages(Talk::Greet, 0)
                });
                self.prompt = false;
            }
            return event;
        };
        if scene != CORNIFER_SCENE {
            self.cancel();
            return event;
        }
        let affordable = geo >= CORNIFER_PRICE;
        self.step = match step {
            Step::Pages(talk, page) if pressed & CROSS != 0 => {
                if page + 1 < talk.pages().len() {
                    Some(Step::Pages(talk, page + 1))
                } else {
                    match talk {
                        // Box Down 2, Box Up YN: the cursor starts on Yes when it can.
                        Talk::Meet | Talk::Greet | Talk::Again => {
                            Some(Step::YesNo(if affordable { 0 } else { 1 }))
                        }
                        Talk::Intro1 => Some(Step::Pages(Talk::Intro2, 0)),
                        _ => None,
                    }
                }
            }
            Step::Pages(..) if pressed & CIRCLE != 0 => None,
            Step::YesNo(row) => {
                if pressed & (UP | DOWN) != 0 && affordable {
                    Some(Step::YesNo(1 - row))
                } else if pressed & CROSS != 0 && row == 0 && affordable {
                    // Take Geo, then Geo Pause and GetMap and First Map?.
                    event.paid = CORNIFER_PRICE;
                    event.bought = true;
                    pd.set(persist::MAP_CROSSROADS);
                    if !pd.get(persist::HAS_MAP) {
                        pd.set(persist::HAS_MAP);
                        Some(Step::FirstMap)
                    } else {
                        None
                    }
                } else if pressed & CIRCLE != 0 || (pressed & CROSS != 0 && row == 1) {
                    Some(Step::Pages(
                        if affordable {
                            Talk::Refuse
                        } else {
                            Talk::NotEnough
                        },
                        0,
                    ))
                } else {
                    Some(step)
                }
            }
            // Map Input: jump, attack, cast or the map button.
            Step::FirstMap if pressed & (CROSS | SQUARE | CIRCLE | map_button) != 0 => {
                Some(Step::Pages(Talk::Iselda, 0))
            }
            other => Some(other),
        };
        if self.step.is_none() {
            self.cancel();
        }
        event
    }
}

/// The guest half: the one conversation, its telemetry and its drawing. Out
/// of the host test build, which runs `Convo` against a plain `Player`.
#[cfg(not(test))]
mod live {
    use super::*;
    struct Store;
    impl Player for Store {
        fn get(&self, bit: u32) -> bool {
            persist::player(bit)
        }
        fn set(&mut self, bit: u32) {
            persist::set_player(bit)
        }
    }
    static mut CONVO: Convo = Convo::new();
    #[no_mangle]
    pub static mut HK_CORNIFER_TALKS: u32 = 0;
    #[no_mangle]
    pub static mut HK_CORNIFER_SOLD: u32 = 0;
    /// The open step, for routes: 0 closed, 1 pages, 2 yes/no, 3 first-map prompt.
    #[no_mangle]
    pub static mut HK_CORNIFER_STEP: u32 = 0;
    pub fn open() -> bool {
        unsafe { CONVO.open() }
    }
    pub fn consumes_actions() -> bool {
        unsafe { CONVO.consumes_actions() }
    }
    pub fn cancel() {
        unsafe {
            CONVO.cancel();
            HK_CORNIFER_STEP = 0;
        }
    }
    pub fn speaking() -> Option<u32> {
        open().then_some(CORNIFER_SOURCE)
    }
    /// Whether this cooked NPC is the one this module speaks for.
    pub fn owns(source: u32) -> bool {
        source == CORNIFER_SOURCE
    }
    /// One tick; returns the Geo the purchase took.
    pub fn tick(scene: usize, in_range: bool, eligible: bool, bits: u16, geo: u32) -> u32 {
        let convo = unsafe { &mut *(&raw mut CONVO) };
        let event = convo.step(
            &mut Store,
            scene,
            in_range,
            eligible,
            bits,
            geo,
            crate::game_map::MAP_BUTTON,
        );
        unsafe {
            HK_CORNIFER_TALKS += u32::from(event.opened);
            HK_CORNIFER_SOLD += u32::from(event.bought);
            HK_CORNIFER_STEP = match convo.step {
                None => 0,
                Some(Step::Pages(..)) => 1,
                Some(Step::YesNo(_)) => 2,
                Some(Step::FirstMap) => 3,
            };
        }
        if event.opened {
            crate::audio::ui_confirm();
        }
        // Geo Pause and GetMap: UpdateGameMap after the bool is set.
        if event.bought {
            crate::game_map::update_game_map();
        }
        event.paid
    }
    /// Compose the panel, the box or the prompt label. Returns false when there
    /// is nothing of Cornifer's to draw, so the caller can draw something else.
    pub fn prepare(camera: (i32, i32), geo: u32) -> bool {
        use crate::dialogue::{
            panel_frame, panel_number, panel_pages, panel_prompt, panel_text, panel_width,
        };
        let convo = unsafe { &*(&raw const CONVO) };
        match convo.step {
            Some(Step::Pages(talk, page)) => panel_pages(talk.pages(), page),
            Some(Step::YesNo(row)) => {
                panel_frame(40, 70, 240, 100);
                let q = PROMPT[0][0];
                panel_text(160 - panel_width(q) / 2, 82, q);
                // The box's Geo Text: the toll.
                panel_number(146, 102, CORNIFER_PRICE);
                panel_text(174, 102, "Geo");
                let affordable = geo >= CORNIFER_PRICE;
                for (i, label) in ["Yes", "No"].iter().enumerate() {
                    let y = 124 + i as i16 * 18;
                    if i == 0 && !affordable {
                        continue;
                    }
                    panel_text(146, y, label);
                    if row as usize == i {
                        panel_text(130, y, ">");
                    }
                }
            }
            Some(Step::FirstMap) => {
                panel_frame(30, 70, 260, 100);
                panel_text(160 - panel_width(GET_MAP_1) / 2, 84, GET_MAP_1);
                let hold = "L2";
                let line_w = panel_width(HOLD) + 8 + panel_width(hold);
                panel_text(160 - line_w / 2, 112, HOLD);
                panel_text(160 - line_w / 2 + panel_width(HOLD) + 8, 112, hold);
                panel_text(160 - panel_width(GET_MAP_2) / 2, 138, GET_MAP_2);
            }
            None => {
                if !convo.prompt {
                    return false;
                }
                panel_prompt(camera, CORNIFER_MARKER, CORNIFER_LABEL);
            }
        }
        true
    }
}
#[cfg(not(test))]
pub use live::*;

#[cfg(test)]
mod tests {
    use super::*;
    struct Pd(u32);
    impl Player for Pd {
        fn get(&self, b: u32) -> bool {
            self.0 & (1 << b) != 0
        }
        fn set(&mut self, b: u32) {
            self.0 |= 1 << b;
        }
    }
    fn press(c: &mut Convo, pd: &mut Pd, bits: u16, geo: u32) -> Event {
        let e = c.step(pd, CORNIFER_SCENE, true, true, bits, geo, 0x100);
        c.step(pd, CORNIFER_SCENE, true, true, 0, geo, 0x100);
        e
    }
    fn page_through(c: &mut Convo, pd: &mut Pd, geo: u32) {
        while let Some(Step::Pages(..)) = c.step {
            press(c, pd, CROSS, geo);
        }
    }
    #[test]
    fn first_talk_meets_sells_and_mentions_iselda() {
        let (mut c, mut pd) = (Convo::new(), Pd(0));
        assert!(press(&mut c, &mut pd, UP, 100).opened);
        assert_eq!(c.step, Some(Step::Pages(Talk::Meet, 0)));
        assert!(pd.get(persist::MET_CORNIFER) && pd.get(persist::OPENED_MAPPER_SHOP));
        page_through(&mut c, &mut pd, 100);
        assert_eq!(c.step, Some(Step::YesNo(0)));
        let e = press(&mut c, &mut pd, CROSS, 100);
        assert_eq!((e.paid, e.bought), (CORNIFER_PRICE, true));
        assert!(pd.get(persist::MAP_CROSSROADS) && pd.get(persist::HAS_MAP));
        assert_eq!(c.step, Some(Step::FirstMap));
        press(&mut c, &mut pd, 0x100, 100);
        assert_eq!(c.step, Some(Step::Pages(Talk::Iselda, 0)));
        page_through(&mut c, &mut pd, 100);
        assert_eq!(c.step, None);
        // Next talk: the introduction, once, then the bought line.
        press(&mut c, &mut pd, UP, 70);
        assert_eq!(c.step, Some(Step::Pages(Talk::Intro1, 0)));
        page_through(&mut c, &mut pd, 70);
        assert_eq!(c.step, None);
        press(&mut c, &mut pd, UP, 70);
        assert_eq!(c.step, Some(Step::Pages(Talk::Bought, 0)));
    }
    #[test]
    fn a_short_wallet_cannot_say_yes_and_hears_not_enough() {
        let (mut c, mut pd) = (Convo::new(), Pd(0));
        press(&mut c, &mut pd, UP, CORNIFER_PRICE - 1);
        page_through(&mut c, &mut pd, CORNIFER_PRICE - 1);
        assert_eq!(c.step, Some(Step::YesNo(1)));
        press(&mut c, &mut pd, UP, CORNIFER_PRICE - 1);
        assert_eq!(c.step, Some(Step::YesNo(1)));
        let e = press(&mut c, &mut pd, CROSS, CORNIFER_PRICE - 1);
        assert_eq!(e.paid, 0);
        assert!(!pd.get(persist::MAP_CROSSROADS));
        assert_eq!(c.step, Some(Step::Pages(Talk::NotEnough, 0)));
        page_through(&mut c, &mut pd, 0);
        // Spoken this visit: Again, not Greet.
        press(&mut c, &mut pd, UP, 0);
        assert_eq!(c.step, Some(Step::Pages(Talk::Again, 0)));
    }
    #[test]
    fn no_with_the_geo_in_hand_is_a_refusal() {
        let (mut c, mut pd) = (Convo::new(), Pd(1 << persist::MET_CORNIFER));
        press(&mut c, &mut pd, UP, 500);
        assert_eq!(c.step, Some(Step::Pages(Talk::Greet, 0)));
        page_through(&mut c, &mut pd, 500);
        press(&mut c, &mut pd, DOWN, 500);
        press(&mut c, &mut pd, CROSS, 500);
        assert_eq!(c.step, Some(Step::Pages(Talk::Refuse, 0)));
    }
}
