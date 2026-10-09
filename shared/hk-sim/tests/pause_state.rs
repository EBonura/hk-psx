// cheats composes the equipped charms into the live parameters and reconciles
// the all-charms grant. The real module links the cooked catalogue this harness
// has no manifest directory for, and an empty board composes to the base
// anyway, so the contract is stubbed here; tests/charms_runtime.rs exercises
// the real one.
// The real charm module `include!`s the cooked catalogue relative to its own
// crate, which this one is not, so the board is stubbed down to the contract
// `pause` actually depends on: a screen that opens, moves and closes. The board
// itself is exercised for real in tests/charms_runtime.rs, and
// tests/test_charms.py holds these constants and this contract to the real
// module so the stub cannot drift away from what it stands in for. The button
// values matter: pause.rs asserts at compile time that its own match the charm
// screen's, and a stub with different ones would silently defeat that.
mod charms {
    pub const UP: u16 = 0x10;
    pub const DOWN: u16 = 0x40;
    pub const CIRCLE: u16 = 0x2000;
    pub const CROSS: u16 = 0x4000;
    pub struct Screen {
        pub open: bool,
        pub row: usize,
    }
    impl Screen {
        pub const fn new() -> Self {
            Self {
                open: false,
                row: 0,
            }
        }
        pub fn enter(&mut self) {
            self.open = true;
            self.row = 0;
        }
        /// True when the press closed the board, which is what hands the row back.
        pub fn step(&mut self, press: u16) -> bool {
            if press & CIRCLE != 0 {
                self.open = false;
                return true;
            }
            if press & DOWN != 0 {
                self.row += 1;
            }
            if press & UP != 0 {
                self.row = self.row.saturating_sub(1);
            }
            false
        }
    }
    pub fn vitals(base: hk_sim::VitalParams) -> hk_sim::VitalParams {
        base
    }
    pub fn grant_all(_: bool) {}
    pub fn on_damage(_: &mut hk_sim::Vitals, _: hk_sim::VitalParams) {}
}
// And the shop's half of PlayerData, which `cheats::params` composes under
// the charms. The real module `include!`s the cooked stock relative to its own
// crate, which this one is not, and a shop nobody has visited composes to the
// base anyway; tests/shop_runtime.rs exercises the real one.
mod shop {
    pub fn vitals(base: hk_sim::VitalParams) -> hk_sim::VitalParams {
        base
    }
}
#[path = "../../../game/src/cheats.rs"]
mod cheats;
#[path = "../../../game/src/pause.rs"]
mod pause;
fn step(p: &mut pause::State, c: &mut cheats::Settings, b: u16) -> bool {
    p.step(b, &mut 10, &mut 10, &mut 10, c)
}
fn tap(p: &mut pause::State, c: &mut cheats::Settings, b: u16) -> bool {
    step(p, c, 0);
    step(p, c, b)
}
#[test]
fn actions_are_edges_and_back_does_not_resume() {
    let mut p = pause::State::new();
    let mut c = cheats::Settings::new();
    // Charms joined the menu at row 6, so a single Up from Resume now lands
    // there and Cheats is one further back.
    tap(&mut p, &mut c, 0x10);
    assert_eq!(p.row, 6);
    tap(&mut p, &mut c, 0x10);
    assert_eq!(p.row, 5);
    tap(&mut p, &mut c, 0x4000);
    assert!(p.cheats);
    tap(&mut p, &mut c, 0x4000);
    assert!(c.invincible);
    step(&mut p, &mut c, 0x4000);
    assert!(c.invincible);
    // Down from the first toggle to the first action row.
    for _ in 0..cheats::CHEAT_ROWS {
        tap(&mut p, &mut c, 0x40);
    }
    tap(&mut p, &mut c, 0x4000);
    assert_eq!(p.action, cheats::Action::Restore);
    step(&mut p, &mut c, 0x4000);
    assert_eq!(p.action, cheats::Action::None);
    tap(&mut p, &mut c, 0x40);
    tap(&mut p, &mut c, 0x4000);
    assert_eq!(p.action, cheats::Action::AddBlue);
    tap(&mut p, &mut c, 0x40);
    tap(&mut p, &mut c, 0x4000);
    assert_eq!(p.action, cheats::Action::Reset);
    assert_eq!(c, cheats::Settings::new());
    assert!(!tap(&mut p, &mut c, 0x2000));
    assert!(!p.cheats);
    assert_eq!(p.row, 5);
    assert!(tap(&mut p, &mut c, 0x2000));
}

#[test]
fn music_volume_and_adjacent_pages_preserve_each_other() {
    let mut p = pause::State::new();
    let mut c = cheats::Settings::new();
    let (mut sfx, mut ambience, mut music) = (7, 6, 10);
    let mut tap = |p: &mut pause::State, c: &mut cheats::Settings, b| {
        assert!(!p.step(0, &mut sfx, &mut ambience, &mut music, c));
        p.step(b, &mut sfx, &mut ambience, &mut music, c)
    };
    for _ in 0..3 {
        tap(&mut p, &mut c, 0x40);
    }
    assert_eq!(p.row, 3);
    for _ in 0..15 {
        tap(&mut p, &mut c, 0x80);
    }
    tap(&mut p, &mut c, 0x20);
    assert!(!tap(&mut p, &mut c, 0x4000));
    assert!(!p.controls && !p.cheats);
    tap(&mut p, &mut c, 0x40);
    assert_eq!(p.row, 4);
    tap(&mut p, &mut c, 0x4000);
    assert!(p.controls);
    assert!(!tap(&mut p, &mut c, 0x2000));
    assert!(!p.controls);
    assert_eq!(p.row, 4);
    tap(&mut p, &mut c, 0x40);
    assert_eq!(p.row, 5);
    tap(&mut p, &mut c, 0x4000);
    assert!(p.cheats);
    // Up from row 0 wraps to the last row, which is Back.
    tap(&mut p, &mut c, 0x10);
    assert_eq!(p.row, cheats::CHEAT_ROWS + 3);
    assert!(!tap(&mut p, &mut c, 0x4000));
    // Down from Cheats passes through Charms before wrapping to Resume.
    assert!(!p.cheats);
    assert_eq!(p.row, 5);
    tap(&mut p, &mut c, 0x40);
    assert_eq!(p.row, 6);
    tap(&mut p, &mut c, 0x40);
    assert_eq!(p.row, 0);
    assert!(tap(&mut p, &mut c, 0x4000));
    drop(tap);
    assert_eq!((sfx, ambience, music), (7, 6, 1));
    assert_eq!(c, cheats::Settings::new());
}
