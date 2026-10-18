//! Edge-triggered pause navigation, independent of render cadence.
const UP: u16 = 0x10;
const RIGHT: u16 = 0x20;
const DOWN: u16 = 0x40;
const LEFT: u16 = 0x80;
const CIRCLE: u16 = 0x2000;
const CROSS: u16 = 0x4000;
// Two panels, one pad, as the tablet reader and the NPC box already are: the
// charm board names the bits it answers, so a divergence would give one of them
// the wrong button rather than failing loudly.
const _: () = assert!(
    UP == crate::charms::UP
        && DOWN == crate::charms::DOWN
        && CIRCLE == crate::charms::CIRCLE
        && CROSS == crate::charms::CROSS
);
/// Rows on the pause list. Charms is last so the existing rows keep their
/// index; the cheats page returns to its own row by number.
pub const ROWS: usize = 7;
const CHARMS_ROW: usize = 6;
pub struct State {
    pub row: usize,
    pub controls: bool,
    pub cheats: bool,
    pub charms: crate::charms::Screen,
    pub action: crate::cheats::Action,
    /// The cheats page's Reset session row has been pressed and waits for X (yes) or O (no).
    /// The development reset used to be the Select button, which is the original's
    /// inventory button now.
    pub confirm_reset: bool,
    /// Set for the step that confirmed the reset; the caller takes it.
    pub reset_requested: bool,
    previous: u16,
}
impl State {
    pub const fn new() -> Self {
        Self {
            row: 0,
            controls: false,
            cheats: false,
            charms: crate::charms::Screen::new(),
            action: crate::cheats::Action::None,
            confirm_reset: false,
            reset_requested: false,
            previous: 0,
        }
    }
    pub fn enter(&mut self, bits: u16) {
        self.row = 0;
        self.controls = false;
        self.cheats = false;
        self.charms = crate::charms::Screen::new();
        self.action = crate::cheats::Action::None;
        self.confirm_reset = false;
        self.reset_requested = false;
        self.previous = bits;
    }
    /// The inventory button: the pause list with the charm board already open.
    pub fn enter_inventory(&mut self, bits: u16) {
        self.enter(bits);
        self.row = CHARMS_ROW;
        self.charms.enter();
    }
    /// Return true to resume. The caller freezes gameplay while handling this.
    pub fn step(
        &mut self,
        bits: u16,
        sfx: &mut u8,
        ambience: &mut u8,
        music: &mut u8,
        cheats: &mut crate::cheats::Settings,
    ) -> bool {
        self.action = crate::cheats::Action::None;
        let press = bits & !self.previous;
        self.previous = bits;
        if self.confirm_reset {
            if press & CROSS != 0 {
                self.confirm_reset = false;
                self.reset_requested = true;
            } else if press & CIRCLE != 0 {
                self.confirm_reset = false;
            }
            return false;
        }
        // The charm board owns every button while it is up, including Circle,
        // so closing it cannot also resume the game.
        if self.charms.open {
            if self.charms.step(press) {
                self.row = CHARMS_ROW;
            }
            return false;
        }
        if self.cheats {
            if press & CIRCLE != 0 {
                self.cheats = false;
                self.row = 5;
                return false;
            }
            // Toggles, then Restore, Add Lifeblood, Reset All, Reset session and Back. Derived
            // so a new ability row cannot silently land on one of the actions.
            const TOGGLES: usize = crate::cheats::CHEAT_ROWS;
            const ROWS: usize = TOGGLES + 5;
            if press & UP != 0 {
                self.row = (self.row + ROWS - 1) % ROWS;
            }
            if press & DOWN != 0 {
                self.row = (self.row + 1) % ROWS;
            }
            let delta = i8::from(press & RIGHT != 0) - i8::from(press & LEFT != 0);
            if delta != 0 {
                cheats.adjust(self.row, delta);
            }
            if press & CROSS != 0 {
                match self.row {
                    r if r < TOGGLES => {
                        let delta = if cheats.enabled(r) { -1 } else { 1 };
                        cheats.adjust(r, delta);
                    }
                    r if r == TOGGLES => self.action = crate::cheats::Action::Restore,
                    r if r == TOGGLES + 1 => self.action = crate::cheats::Action::AddBlue,
                    r if r == TOGGLES + 2 => {
                        *cheats = crate::cheats::Settings::new();
                        self.action = crate::cheats::Action::Reset;
                    }
                    r if r == TOGGLES + 3 => self.confirm_reset = true,
                    _ => {
                        self.cheats = false;
                        self.row = 5;
                    }
                }
            }
            return false;
        }
        if self.controls {
            if press & (CIRCLE | CROSS) != 0 {
                self.controls = false;
            }
            return false;
        }
        if press & CIRCLE != 0 {
            return true;
        }
        if press & UP != 0 {
            self.row = (self.row + ROWS - 1) % ROWS;
        }
        if press & DOWN != 0 {
            self.row = (self.row + 1) % ROWS;
        }
        let delta = i16::from(press & RIGHT != 0) - i16::from(press & LEFT != 0);
        let value = match self.row {
            1 => Some(sfx),
            2 => Some(ambience),
            3 => Some(music),
            _ => None,
        };
        if let Some(value) = value {
            *value = (i16::from(*value) + delta).clamp(0, 10) as u8;
        }
        if press & CROSS != 0 {
            if self.row == 0 {
                return true;
            }
            if self.row == 4 {
                self.controls = true;
            }
            if self.row == 5 {
                self.cheats = true;
                self.row = 0;
            }
            if self.row == CHARMS_ROW {
                self.charms.enter();
            }
        }
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_inputs_do_not_repeat_and_volume_clamps() {
        let mut p = State::new();
        let (mut a, mut b) = (10, 0);
        assert!(!p.step(
            DOWN,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new()
        ));
        assert_eq!(p.row, 1);
        p.step(
            DOWN,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert_eq!(p.row, 1);
        p.step(
            RIGHT,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert_eq!(a, 10);
        p.step(
            LEFT,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert_eq!(a, 9);
        p.step(
            LEFT,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert_eq!(a, 9);
        p.step(
            DOWN,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert_eq!(p.row, 2);
        p.step(
            LEFT,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert_eq!(b, 0);
    }
    #[test]
    fn controls_back_and_resume_are_separate() {
        let mut p = State::new();
        let (mut a, mut b) = (10, 10);
        // Up from Resume wraps onto Charms, so Controls is three rows back.
        for _ in 0..3 {
            p.step(
                UP,
                &mut a,
                &mut b,
                &mut 10,
                &mut crate::cheats::Settings::new(),
            );
            p.step(
                0,
                &mut a,
                &mut b,
                &mut 10,
                &mut crate::cheats::Settings::new(),
            );
        }
        assert_eq!(p.row, 4);
        assert!(!p.step(
            CROSS,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new()
        ));
        assert!(p.controls);
        assert!(!p.step(
            CIRCLE,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new()
        ));
        assert!(!p.controls);
        p.step(
            0,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new(),
        );
        assert!(p.step(
            CIRCLE,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new()
        ));
        p.enter(CROSS);
        assert!(!p.step(
            CROSS,
            &mut a,
            &mut b,
            &mut 10,
            &mut crate::cheats::Settings::new()
        ));
    }
    #[test]
    fn the_charm_board_takes_circle_instead_of_resuming() {
        let mut p = State::new();
        let (mut a, mut b) = (10, 10);
        let mut cheats = crate::cheats::Settings::new();
        p.step(UP, &mut a, &mut b, &mut 10, &mut cheats);
        assert_eq!(p.row, 6);
        p.step(0, &mut a, &mut b, &mut 10, &mut cheats);
        assert!(!p.step(CROSS, &mut a, &mut b, &mut 10, &mut cheats));
        assert!(p.charms.open);
        // Down inside the board moves its cursor, not the pause row.
        p.step(0, &mut a, &mut b, &mut 10, &mut cheats);
        p.step(DOWN, &mut a, &mut b, &mut 10, &mut cheats);
        assert_eq!(p.row, 6);
        assert_eq!(p.charms.row, 1);
        p.step(0, &mut a, &mut b, &mut 10, &mut cheats);
        assert!(!p.step(CIRCLE, &mut a, &mut b, &mut 10, &mut cheats));
        assert!(!p.charms.open);
        assert_eq!(p.row, 6);
    }
    #[test]
    fn the_reset_asks_first_and_only_x_confirms() {
        let mut p = State::new();
        let (mut a, mut b) = (10, 10);
        let mut c = crate::cheats::Settings::new();
        let mut tap = |p: &mut State, bits: u16| {
            p.step(0, &mut a, &mut b, &mut 10, &mut c);
            p.step(bits, &mut a, &mut b, &mut 10, &mut c)
        };
        // Up twice from Resume is Cheats; in it Up from the first toggle wraps onto Back,
        // and Reset session is the row above.
        tap(&mut p, UP);
        tap(&mut p, UP);
        tap(&mut p, CROSS);
        assert!(p.cheats);
        tap(&mut p, UP);
        tap(&mut p, UP);
        assert_eq!(p.row, crate::cheats::CHEAT_ROWS + 3);
        assert!(!tap(&mut p, CROSS));
        assert!(p.confirm_reset && !p.reset_requested);
        // O backs out of the question, to the cheats page, and does not resume.
        assert!(!tap(&mut p, CIRCLE));
        assert!(!p.confirm_reset && !p.reset_requested && p.cheats);
        tap(&mut p, CROSS);
        assert!(p.confirm_reset);
        assert!(!tap(&mut p, CROSS));
        assert!(p.reset_requested && !p.confirm_reset);
        // A fresh pause forgets a request nobody took.
        p.enter(0);
        assert!(!p.reset_requested);
    }
    #[test]
    fn the_inventory_button_opens_the_charm_board() {
        let mut p = State::new();
        p.enter_inventory(0);
        assert!(p.charms.open);
        assert_eq!(p.row, 6);
    }
}
