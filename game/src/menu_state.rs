//! Bounded title menu state. Button bits match the SDK's active-high PS1 map.
//! Session settings are returned to gameplay; no save or memory-card claim.
pub const UP: u16 = 1 << 4;
pub const RIGHT: u16 = 1 << 5;
pub const DOWN: u16 = 1 << 6;
pub const LEFT: u16 = 1 << 7;
pub const START: u16 = 1 << 3;
pub const CIRCLE: u16 = 1 << 13;
pub const CROSS: u16 = 1 << 14;
/// Brightness steps either side of the picture as drawn, and the screen
/// position's reach either side of centre in pixels (display.rs).
// The font image's layout (host/hk-cook/src/font_strip.rs): where it sits in VRAM, the body glyph
// cells, the capitals and the Geo coin's cell.
include!("../../data/font_layout.rs");
pub const BRIGHT_STEPS: i8 = 5;
pub const SCREEN_RANGE: i8 = 16;
/// Word aligned so that the opening movies' mailbox (intro.rs) moves it in whole words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(align(4))]
pub struct Settings {
    pub sfx: u8,
    pub ambience: u8,
    pub music: u8,
    pub cheats: crate::cheats::Settings,
    /// Rows 3 to 5: brightness (-BRIGHT_STEPS darker to BRIGHT_STEPS brighter, 0 as drawn), and the
    /// picture's position right and down of centre. They reach the screen through display.rs.
    pub brightness: i8,
    pub screen_x: i8,
    pub screen_y: i8,
    /// Row 6: the title style (`MenuStyles.currentSettings.styleIndex`, as an index into
    /// `STYLE_NAMES`).
    pub style: u8,
    /// The profile-wide unlock keys the endings and the Mighty Zote fight have set (`STYLE_KEY` bits),
    /// which the Cheats page's last action sets all at once.
    pub unlocked: u8,
}
impl Settings {
    pub const fn new() -> Self {
        Self {
            sfx: 10,
            ambience: 10,
            music: 10,
            cheats: crate::cheats::Settings::new(),
            brightness: 0,
            screen_x: 0,
            screen_y: 0,
            style: DEFAULT_STYLE,
            unlocked: 0,
        }
    }
    /// Whether style `i` can be chosen: it has no unlock key, or its key is set (`MenuStyle.IsAvailable`).
    pub fn available(&self, i: usize) -> bool {
        STYLE_KEY[i] == 0 || self.unlocked & STYLE_KEY[i] != 0
    }
    /// Rows 0/1/2 adjust sound effects/ambience/music (0 to 10), rows 3/4/5 brightness and the screen
    /// position. Returns whether the value changed.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    pub fn adjust(&mut self, row: usize, delta: i8) -> bool {
        let (value, low, high) = match row {
            0 => (&mut self.sfx, 0, 10),
            1 => (&mut self.ambience, 0, 10),
            2 => (&mut self.music, 0, 10),
            3 => return step(&mut self.brightness, delta, BRIGHT_STEPS),
            4 => return step(&mut self.screen_x, delta, SCREEN_RANGE),
            5 => return step(&mut self.screen_y, delta, SCREEN_RANGE),
            // The style list wraps and skips the styles still locked (`MenuStyleSetting.OnMove`).
            6 => {
                let (n, dir) = (STYLE_NAMES.len() as i16, i16::from(delta.signum()));
                let mut next = i16::from(self.style);
                for _ in 0..if dir == 0 { 0 } else { n } {
                    next = (next + dir).rem_euclid(n);
                    if self.available(next as usize) {
                        break;
                    }
                }
                let changed = next as u8 != self.style;
                self.style = next as u8;
                return changed;
            }
            _ => return false,
        };
        let next = (*value as i16 + delta as i16).clamp(low, high) as u8;
        let changed = next != *value;
        *value = next;
        changed
    }
}
/// Step a value kept in -limit..=limit, saturating. Returns whether it moved.
fn step(value: &mut i8, delta: i8, limit: i8) -> bool {
    let next = (*value as i16 + delta as i16).clamp(-(limit as i16), limit as i16) as i8;
    let changed = next != *value;
    *value = next;
    changed
}
/// `value` as the options page prints it: `0`, `+3`, `-2`.
pub fn signed(value: i8, out: &mut [u8; 3]) -> &str {
    let n = value.unsigned_abs();
    let mut at = 0;
    if value != 0 {
        out[at] = if value > 0 { b'+' } else { b'-' };
        at += 1;
    }
    if n >= 10 {
        out[at] = b'0' + n / 10;
        at += 1;
    }
    out[at] = b'0' + n % 10;
    at += 1;
    core::str::from_utf8(&out[..at]).unwrap_or("")
}
impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Options,
    Controls,
    Cheats,
    Profiles,
}
pub const MAIN_ITEMS: [&str; 4] = ["Start", "Options", "Controls", "Cheats"];
const _: () = assert!(CHEAT_ITEMS.len() == crate::cheats::CHEAT_ROWS);
/// The title's Cheats page after the toggles: the action that unlocks every menu style, then Back.
pub const UNLOCK_ROW: usize = CHEAT_ITEMS.len();
pub const CHEAT_BACK: usize = UNLOCK_ROW + 1;
pub const CHEAT_ITEMS: [&str; 12] = [
    "Invincibility",
    "Pure Nail (21 damage)",
    "Infinite SOUL",
    "Max masks (9)",
    "Mothwing Cloak (R2 dash)",
    "Mantis Claw (wall jump)",
    "Monarch Wings (double jump)",
    "Crystal Heart (hold L2)",
    "Shade Cloak (dash i-frames)",
    "Dream Nail (hold Triangle)",
    "Vengeful Spirit (tap O)",
    "All charms found",
];
pub const OPTION_ITEMS: [&str; 8] = [
    "Sound effects",
    "Ambience",
    "Music",
    "Brightness",
    "Screen X",
    "Screen Y",
    "Menu style",
    "Back",
];
/// The options row that picks the title style.
pub const STYLE_ROW: usize = 6;
/// The title styles, in the order of `MenuStyles.styles` (docs/MENU_STYLES.md): the names the options row
/// prints. Modern is the 1.5.0 default (`MenuStyles.StyleDefault`).
pub const STYLE_NAMES: [&str; 10] = [
    "Classic", "HD", "Grimm", "GG", "Infected", "Shade", "Steel", "Eternal", "Voidheart", "Modern",
];
/// The style the title starts on.
pub const DEFAULT_STYLE: u8 = 9;
/// The unlock key each style waits for, as a bit of `Settings::unlocked` (0: available from the start):
/// 1 `infectedMenu` (the endings A and B), 2 `shadeMenu` (ending C), 4 `steelSoulMenu` (an ending in
/// Steel Soul mode), 8 `eternalOrdealMenu` (the Mighty Zote fight).
pub const STYLE_KEY: [u8; 10] = [0, 0, 0, 0, 1, 2, 4, 8, 0, 0];
/// Every key at once, what the Cheats page's last action unlocks (`unlockAllMenuStyles`).
pub const ALL_STYLE_KEYS: u8 = 15;
/// The row that leaves the options page.
pub const OPTION_BACK: usize = OPTION_ITEMS.len() - 1;
pub const CONTROL_LINES: [&str; 10] = [
    "D-pad or stick: move",
    "X: jump    Square: nail",
    "Up + Square: nail up",
    "Air Down + Square: pogo",
    "O: cast (tap), Focus (hold)",
    "R1: quick cast    R2: dash",
    "Hold L2: Crystal Heart",
    "Hold Triangle: Dream Nail",
    "L1: quick map    Select: charms",
    "Start: pause    Up: inspect",
];
pub struct State {
    pub page: Page,
    pub selected: usize,
    pub settings: Settings,
    /// The profile the player confirmed on the save screen, once chosen.
    pub profile: Option<usize>,
    previous: u16,
    direction: u16,
    repeat: u8,
}
impl State {
    pub const fn new() -> Self {
        Self {
            page: Page::Main,
            selected: 0,
            settings: Settings::new(),
            profile: None,
            previous: 0,
            direction: 0,
            repeat: 0,
        }
    }
    /// Consume exactly one actual pad sample. Navigation repeats after20 held
    /// polls then every5 polls. Confirm/back require fresh edges. Confirm has
    /// priority over a simultaneous direction, preserving direct first START/X.
    pub fn step(&mut self, bits: u16) -> bool {
        let edge = bits & !self.previous;
        self.previous = bits;
        let dir = bits & (UP | DOWN | LEFT | RIGHT);
        let dir = if dir.count_ones() == 1 { dir } else { 0 };
        let navigate = if dir != self.direction {
            self.direction = dir;
            self.repeat = 20;
            dir
        } else if dir != 0 {
            self.repeat = self.repeat.saturating_sub(1);
            if self.repeat == 0 {
                self.repeat = 5;
                dir
            } else {
                0
            }
        } else {
            0
        };
        if edge & CIRCLE != 0 {
            if self.page != Page::Main {
                self.selected = match self.page {
                    Page::Options => 1,
                    Page::Cheats => 3,
                    Page::Profiles => 0,
                    _ => 2,
                };
                self.page = Page::Main;
            }
            return false;
        }
        if edge & (START | CROSS) != 0 {
            match self.page {
                Page::Main => match self.selected {
                    // Start Game opens the save screen, as the source does;
                    // an empty profile begins a new game and a used one resumes.
                    0 => {
                        self.page = Page::Profiles;
                        self.selected = 0;
                    }
                    1 => {
                        self.page = Page::Options;
                        self.selected = 0;
                    }
                    2 => {
                        self.page = Page::Controls;
                        self.selected = 0;
                    }
                    _ => {
                        self.page = Page::Cheats;
                        self.selected = 0;
                    }
                },
                Page::Profiles => {
                    self.profile = Some(self.selected);
                    return true;
                }
                Page::Options => {
                    if self.selected == OPTION_BACK {
                        self.page = Page::Main;
                        self.selected = 1;
                    } else if self.selected == STYLE_ROW {
                        // `MenuStyleSetting.OnSubmit` steps to the next style.
                        self.settings.adjust(STYLE_ROW, 1);
                    }
                }
                Page::Controls => {
                    self.page = Page::Main;
                    self.selected = 2;
                }
                Page::Cheats => {
                    if self.selected == CHEAT_BACK {
                        self.page = Page::Main;
                        self.selected = 3;
                    } else if self.selected == UNLOCK_ROW {
                        self.settings.unlocked = ALL_STYLE_KEYS;
                    } else {
                        let delta = if self.settings.cheats.enabled(self.selected) {
                            -1
                        } else {
                            1
                        };
                        self.settings.cheats.adjust(self.selected, delta);
                    }
                }
            }
            return false;
        }
        if self.page == Page::Controls {
            return false;
        }
        let count = match self.page {
            Page::Cheats => CHEAT_BACK + 1,
            Page::Profiles => crate::save::PROFILES,
            Page::Options => OPTION_ITEMS.len(),
            _ => 4,
        };
        if navigate == UP {
            self.selected = (self.selected + count - 1) % count;
        } else if navigate == DOWN {
            self.selected = (self.selected + 1) % count;
        } else if self.page == Page::Options {
            if navigate == LEFT {
                self.settings.adjust(self.selected, -1);
            } else if navigate == RIGHT {
                self.settings.adjust(self.selected, 1);
            }
        } else if self.page == Page::Cheats {
            if navigate == LEFT {
                self.settings.cheats.adjust(self.selected, -1);
            } else if navigate == RIGHT {
                self.settings.cheats.adjust(self.selected, 1);
            }
        }
        false
    }
}
