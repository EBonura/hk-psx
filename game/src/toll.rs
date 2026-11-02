//! The Toll Machines of the rooms that place one (host/tolls.py documents the cook, host/world_links.py
//! `toll_record` the FSM it follows): UP inside a machine's range opens the source's `Text YN` box with the
//! price beside the question; `Yes` takes the Geo and, half a second later, sets the scene's toll bit, which
//! removes every Toll Gate of the scene (the terrain and the art bound to the bit, like the Metal Gate's).
//! `Yes` is not selectable while the wallet is short, so a short wallet can only say `No` and nothing is
//! taken. All the machines of a scene share the bit (the source broadcasts `TOLL GATE OPEN` to every gate),
//! and once it is set the machines answer nothing.
//!
//! The state machine is `Convo`, run on the host in tests/toll_runtime.rs; the guest half (`live`: the
//! one box, its telemetry and its drawing) is the `toll` room module (host/code_modules.py).
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/toll.rs"));

const UP: u16 = 0x10;
const DOWN: u16 = 0x40;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;
const SQUARE: u16 = 0x8000;
/// The buttons the box owns while it is up.
const ACTIONS: u16 = UP | DOWN | CROSS | CIRCLE | SQUARE;
const NONE: u8 = u8::MAX;

/// One tick's outcome: the box opened, and the Geo a `Yes` took with the bit it will set.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Event {
    pub opened: bool,
    pub paid: u32,
    pub id: usize,
}

pub struct Convo {
    /// The box's cursor (0 Yes, 1 No) while it is up.
    row: Option<u8>,
    /// The machine in range, whose prompt is up, or the one the box belongs to.
    machine: u8,
    prompt: bool,
    /// The bit a `Yes` is about to set: its machines answer nothing from the payment on.
    paying: usize,
    previous: u16,
    release: bool,
}

fn overlaps(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

impl Convo {
    pub const fn new() -> Self {
        Self { row: None, machine: NONE, prompt: false, paying: usize::MAX, previous: 0, release: false }
    }
    pub fn open(&self) -> bool {
        self.row.is_some()
    }
    pub fn consumes_actions(&self) -> bool {
        self.open() || self.release
    }
    pub fn prompting(&self) -> bool {
        self.prompt
    }
    /// A tick the box is not stepped (its room's module is not loaded): the pad of this tick is the
    /// previous one, and `release` goes once the actions are up, so a `cancel` never latches from a pad
    /// seen rooms ago.
    pub fn idle(&mut self, bits: u16) {
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
    }
    pub fn cancel(&mut self) {
        self.row = None;
        self.prompt = false;
        self.machine = NONE;
        self.release = self.previous & ACTIONS != 0;
    }
    pub fn cursor(&self) -> u8 {
        self.row.unwrap_or(0)
    }
    pub fn machine(&self) -> Option<&'static Machine> {
        MACHINES.get(self.machine as usize)
    }
    /// One tick. `body` is the hero body, `geo` the wallet, `retired(id)` whether the bit `id` is set.
    pub fn step(
        &mut self,
        scene: usize,
        body: [i32; 4],
        eligible: bool,
        bits: u16,
        geo: u32,
        retired: impl Fn(usize) -> bool,
    ) -> Event {
        let pressed = bits & !self.previous;
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
        let mut event = Event::default();
        let done = |paying: usize, id: usize| id == paying || retired(id);
        let Some(row) = self.row else {
            let near = MACHINES
                .iter()
                .position(|m| m.scene == scene && overlaps(m.bounds, body) && !done(self.paying, m.id));
            self.prompt = eligible && !self.release && near.is_some();
            if self.prompt {
                self.machine = near.map_or(NONE, |i| i as u8);
                if pressed & UP != 0 {
                    let affordable = geo >= MACHINES[near.unwrap_or(0)].price;
                    self.row = Some(if affordable { 0 } else { 1 });
                    self.prompt = false;
                    event.opened = true;
                }
            }
            return event;
        };
        let Some(machine) = self.machine() else {
            self.cancel();
            return event;
        };
        if machine.scene != scene || done(self.paying, machine.id) {
            self.cancel();
            return event;
        }
        let affordable = geo >= machine.price;
        if pressed & (UP | DOWN) != 0 && affordable {
            self.row = Some(1 - row);
        } else if pressed & CROSS != 0 && row == 0 && affordable {
            event.paid = machine.price;
            event.id = machine.id;
            self.paying = machine.id;
            self.cancel();
        } else if pressed & CIRCLE != 0 || (pressed & CROSS != 0 && row == 1) {
            self.cancel();
        }
        event
    }
}

/// The guest half: the one box, its telemetry and its drawing. Out of the host test build, which runs
/// `Convo` on its own.
#[cfg(not(test))]
mod live {
    use super::*;
    static mut CONVO: Convo = Convo::new();
    /// Boxes opened, and Geo paid at the machines, for routes.
    #[no_mangle]
    pub static mut HK_TOLL_OPENED: u32 = 0;
    #[no_mangle]
    pub static mut HK_TOLL_PAID: u32 = 0;
    /// The box's state: 0 closed, 1 up with Yes chosen, 2 up with No chosen.
    #[no_mangle]
    pub static mut HK_TOLL_STEP: u32 = 0;
    pub fn prompting() -> bool {
        unsafe { (*(&raw const CONVO)).prompting() }
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
            HK_TOLL_STEP = 0;
        }
    }
    /// One tick; returns the Geo the payment took. The bit it sets is `state`'s, after the source's wait.
    #[inline(never)]
    pub fn tick(
        scene: usize,
        body: [i32; 4],
        eligible: bool,
        bits: u16,
        geo: u32,
        state: &mut crate::world::State,
    ) -> u32 {
        let convo = unsafe { &mut *(&raw mut CONVO) };
        let event = convo.step(scene, body, eligible, bits, geo, |id| state.broken(id));
        unsafe {
            HK_TOLL_OPENED += u32::from(event.opened);
            HK_TOLL_PAID += event.paid;
            HK_TOLL_STEP = if convo.open() { 1 + u32::from(convo.cursor()) } else { 0 };
        }
        if event.opened {
            crate::audio::ui_confirm();
        }
        if event.paid > 0 {
            state.toll_paid(event.id, TOLL_WAIT);
        }
        event.paid
    }
    /// Compose the box or the prompt label. False when there is nothing of the machines' to draw.
    #[inline(never)]
    pub fn prepare(camera: (i32, i32), geo: u32) -> bool {
        use crate::dialogue::{panel_box, panel_price, panel_prompt, panel_text, panel_width};
        // The conversation box's line pitch.
        const ROW: i16 = 16;
        let convo = unsafe { &*(&raw const CONVO) };
        let Some(machine) = convo.machine() else {
            return false;
        };
        if convo.open() {
            let lines = QUESTION.len() as i16;
            let y = panel_box(lines + 3);
            for (i, line) in QUESTION.iter().enumerate() {
                panel_text(160 - panel_width(line) / 2, y + i as i16 * ROW, line);
            }
            // The box's Geo Text and Geo Sprite: the coin, then the toll.
            panel_price(y + lines * ROW, machine.price);
            let affordable = geo >= machine.price;
            for (i, label) in ["Yes", "No"].iter().enumerate() {
                if i == 0 && !affordable {
                    continue;
                }
                let y = y + (lines + 1 + i as i16) * ROW;
                panel_text(146, y, label);
                if convo.cursor() as usize == i {
                    panel_text(130, y, ">");
                }
            }
            return true;
        }
        if !convo.prompting() {
            return false;
        }
        panel_prompt(camera, machine.marker, TOLL_LABEL);
        true
    }
}
#[cfg(not(test))]
pub use live::*;

#[cfg(test)]
mod tests {
    use super::*;
    fn machine() -> &'static Machine {
        &MACHINES[0]
    }
    fn at(m: &Machine) -> [i32; 4] {
        m.bounds
    }
    fn press(c: &mut Convo, bits: u16, geo: u32, open: &[usize]) -> Event {
        let m = machine();
        let e = c.step(m.scene, at(m), true, bits, geo, |id| open.contains(&id));
        c.step(m.scene, at(m), true, 0, geo, |id| open.contains(&id));
        e
    }
    #[test]
    fn up_in_range_opens_the_box_and_yes_takes_the_toll() {
        let mut c = Convo::new();
        let price = machine().price;
        assert!(press(&mut c, UP, price, &[]).opened);
        assert!(c.open());
        let e = press(&mut c, CROSS, price, &[]);
        assert_eq!((e.paid, e.id), (price, machine().id));
        assert!(!c.open());
    }
    #[test]
    fn a_short_wallet_can_only_say_no_and_nothing_is_taken() {
        let mut c = Convo::new();
        let short = machine().price - 1;
        press(&mut c, UP, short, &[]);
        assert!(c.open() && c.cursor() == 1);
        press(&mut c, UP, short, &[]);
        assert_eq!(c.cursor(), 1, "Yes is not selectable");
        let e = press(&mut c, CROSS, short, &[]);
        assert_eq!(e.paid, 0);
        assert!(!c.open());
    }
    #[test]
    fn no_and_circle_close_the_box_without_a_payment() {
        let mut c = Convo::new();
        let price = machine().price;
        press(&mut c, UP, price + 10, &[]);
        press(&mut c, DOWN, price + 10, &[]);
        assert_eq!(press(&mut c, CROSS, price + 10, &[]).paid, 0);
        assert!(!c.open());
        press(&mut c, UP, price + 10, &[]);
        assert_eq!(press(&mut c, CIRCLE, price + 10, &[]).paid, 0);
        assert!(!c.open());
    }
    #[test]
    fn a_paid_or_open_bit_retires_the_machines() {
        let mut c = Convo::new();
        let price = machine().price;
        press(&mut c, UP, price, &[]);
        press(&mut c, CROSS, price, &[]);
        // The bit is still waiting out its half second: a second UP opens nothing.
        assert!(!press(&mut c, UP, price, &[]).opened);
        // A reload with the bit set: nothing answers either.
        let mut fresh = Convo::new();
        assert!(!press(&mut fresh, UP, price, &[machine().id]).opened);
    }
    #[test]
    fn out_of_range_or_not_eligible_nothing_happens() {
        let mut c = Convo::new();
        let m = machine();
        let far = [m.bounds[0] - 1_000_000, m.bounds[1], m.bounds[0] - 900_000, m.bounds[3]];
        assert!(!c.step(m.scene, far, true, UP, 999, |_| false).opened);
        let mut c = Convo::new();
        assert!(!c.step(m.scene, m.bounds, false, UP, 999, |_| false).opened);
        assert!(!c.consumes_actions());
    }
}
