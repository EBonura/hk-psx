//! The Snail Shaman and Vengeful Spirit, earned in play (host/shaman.py reads
//! and asserts the FSMs this follows).
//!
//! PlayerData `shaman` is 0..3 in two player bits and `fireballLevel` is
//! `persist`'s own level. The Shaman's frames and the spell's orb are in the
//! quick map's texture page (host/game_map.py packs them, with their two
//! palettes in the page's last row), streamed before the title with the map:
//! no linked RAM, no animation slot.
//!
//! The pickup is the source's sequence cut to what the port can show: the
//! orb goes, the Knight is held while the camera rumbles for the 5 s the
//! cutscene takes, the screen goes black, and on black the Knight is moved to
//! where he wakes with 100 SOUL, the spell and the respawn there, the game is
//! saved, and the Fireball's get-item message waits for a press. Then a 3 s
//! fade back and he is his own again at the first direction, jump or attack.
//! Not shown: the cutscene knight and its orbs, the flash, the Knight lying
//! prostrate and rising (he stands), the Shaman's talk animation (he idles).
use crate::persist;
pub use crate::shade::Clip;
/// One frame in the map page: its texels, palette (index into
/// `game_map::MAP_CLUT_XY`) and world box around the placement, Q16.
pub struct MapFrame {
    pub u: u16,
    pub v: u16,
    pub w: u16,
    pub h: u16,
    pub clut: usize,
    pub bounds: [i32; 4],
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/shaman.rs"));
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/shaman_art.rs"
));

const UP: u16 = 0x10;
const RIGHT: u16 = 0x20;
const DOWN: u16 = 0x40;
const LEFT: u16 = 0x80;
const CROSS: u16 = 0x4000;
const CIRCLE: u16 = 0x2000;
const SQUARE: u16 = 0x8000;
const ACTIONS: u16 = UP | DOWN | CROSS | CIRCLE | SQUARE;
/// `shaman` in two PlayerData bits (persist::SHAMAN_LOW, SHAMAN_HIGH).
pub fn state(pd: &impl Player) -> u8 {
    u8::from(pd.get(persist::SHAMAN_LOW)) | u8::from(pd.get(persist::SHAMAN_HIGH)) << 1
}
fn set_state(pd: &mut impl Player, value: u8) {
    pd.put(persist::SHAMAN_LOW, value & 1 != 0);
    pd.put(persist::SHAMAN_HIGH, value & 2 != 0);
}

/// Tick lengths at 60 Hz, from the FSM waits.
pub const SUMMON_TICKS: u16 = 180; // Summon Anim: Wait 3
pub const APPEAR_TICKS: u16 = 60; // Spell Appear: Wait 1
pub const ABSORB_TICKS: u16 = 300; // Get Fireball: Start 3 + Rumble 2
pub const BLACK_TICKS: u16 = 120; // Check Fall: Black, Wait 2
pub const FADE_TICKS: u16 = 180; // Fade Back: EaseColor 3

pub trait Player {
    fn get(&self, bit: u32) -> bool;
    fn put(&mut self, bit: u32, on: bool);
    fn fireball(&self) -> u8;
    fn set_fireball(&mut self, level: u8);
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Talk {
    Meet,
    Summoned1,
    Summoned2,
    Trapped1,
    Trapped2,
}
impl Talk {
    pub fn pages(self) -> &'static [&'static [&'static str]] {
        match self {
            Talk::Meet => MEET,
            Talk::Summoned1 => SUMMONED1,
            Talk::Summoned2 => SUMMONED2,
            Talk::Trapped1 => TRAPPED1,
            Talk::Trapped2 => TRAPPED2,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Talk(Talk, usize),
    Summon(u16),
    Appear(u16),
    Absorb(u16),
    Black(u16),
    Message,
    Fade(u16),
    Prostrate,
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Event {
    pub opened: bool,
    pub shake: bool,
    pub big_shake: bool,
    pub wake: bool,
    pub risen: bool,
}

pub struct Shaman {
    pub phase: Option<Phase>,
    pub prompt: Option<bool>,
    scene: usize,
    orb: bool,
    previous: u16,
    release: bool,
}
impl Shaman {
    pub const fn new() -> Self {
        Self {
            phase: None,
            prompt: None,
            scene: usize::MAX,
            orb: false,
            previous: 0,
            release: false,
        }
    }
    pub fn open(&self) -> bool {
        self.phase.is_some()
    }
    pub fn consumes_actions(&self) -> bool {
        self.open() || self.release
    }
    pub fn orb(&self) -> bool {
        self.orb
    }
    /// Which Shaman stands in the Mound: the Meeting one before the spell, the
    /// Trapped one after it until `shaman` reaches 4 (`Check Active`).
    pub fn standing(pd: &impl Player) -> Option<bool> {
        if pd.fireball() == 0 {
            Some(false)
        } else if state(pd) < 4 {
            Some(true)
        } else {
            None
        }
    }
    pub fn cancel(&mut self) {
        // A hit only closes a conversation; the pickup has taken control.
        if matches!(self.phase, Some(Phase::Talk(..))) {
            self.phase = None;
        }
        self.prompt = None;
        self.release = self.previous & ACTIONS != 0;
    }
    /// One tick. `body` is the hero box, `eligible` whether he could talk.
    pub fn step(
        &mut self,
        pd: &mut impl Player,
        scene: usize,
        body: [i32; 4],
        eligible: bool,
        bits: u16,
    ) -> Event {
        let pressed = bits & !self.previous;
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
        let mut event = Event::default();
        if scene != self.scene {
            self.scene = scene;
            self.phase = None;
            // Check Summoned, on the Mound's entry.
            if scene == SHAMAN_SCENE && pd.fireball() == 0 {
                let s = state(pd);
                if s == 1 {
                    set_state(pd, 2);
                }
                self.orb = s == 1 || s == 2;
            } else {
                self.orb = false;
            }
        }
        if scene != SHAMAN_SCENE {
            self.prompt = None;
            return event;
        }
        let overlaps =
            |r: [i32; 4]| r[0] <= body[2] && r[2] >= body[0] && r[1] <= body[3] && r[3] >= body[1];
        match self.phase {
            None => {
                self.prompt = None;
                // The orb's own trigger takes the hero whatever he is doing.
                if self.orb && pd.fireball() == 0 && overlaps(ORB_TRIGGER) {
                    self.orb = false;
                    self.phase = Some(Phase::Absorb(0));
                    event.shake = true;
                    return event;
                }
                let Some(trapped) = Self::standing(pd) else {
                    return event;
                };
                let trigger = if trapped {
                    TRAPPED_TRIGGER
                } else {
                    MEETING_TRIGGER
                };
                if eligible && !self.release && overlaps(trigger) {
                    self.prompt = Some(trapped);
                    if pressed & UP != 0 {
                        event.opened = true;
                        self.prompt = None;
                        let talk = if trapped {
                            if state(pd) < 3 {
                                set_state(pd, 3);
                                Talk::Trapped1
                            } else {
                                Talk::Trapped2
                            }
                        } else {
                            match state(pd) {
                                0 => Talk::Meet,
                                1 => Talk::Summoned1,
                                _ => Talk::Summoned2,
                            }
                        };
                        self.phase = Some(Phase::Talk(talk, 0));
                    }
                }
            }
            Some(Phase::Talk(talk, page)) => {
                if pressed & CROSS != 0 {
                    self.phase = if page + 1 < talk.pages().len() {
                        Some(Phase::Talk(talk, page + 1))
                    } else if talk == Talk::Meet {
                        Some(Phase::Summon(0))
                    } else {
                        None
                    };
                } else if pressed & CIRCLE != 0 {
                    // Closing Meet early still ends in the summon, as Box Down does.
                    self.phase = if talk == Talk::Meet {
                        Some(Phase::Summon(0))
                    } else {
                        None
                    };
                }
            }
            Some(Phase::Summon(t)) => {
                self.phase = Some(if t + 1 >= SUMMON_TICKS {
                    // Spell Appear: BigShake, shaman 1, the orb.
                    set_state(pd, 1);
                    self.orb = true;
                    event.big_shake = true;
                    Phase::Appear(0)
                } else {
                    Phase::Summon(t + 1)
                });
            }
            Some(Phase::Appear(t)) => {
                self.phase = if t + 1 >= APPEAR_TICKS {
                    None
                } else {
                    Some(Phase::Appear(t + 1))
                };
            }
            Some(Phase::Absorb(t)) => {
                if t == 180 {
                    event.big_shake = true;
                }
                self.phase = Some(if t + 1 >= ABSORB_TICKS {
                    Phase::Black(0)
                } else {
                    Phase::Absorb(t + 1)
                });
            }
            Some(Phase::Black(t)) => {
                if t == 0 {
                    // Black and Set Respawns: the caller moves the Knight, fills
                    // SOUL, sets the respawn and saves; the spell is here.
                    pd.set_fireball(1);
                    event.wake = true;
                }
                self.phase = Some(if t + 1 >= BLACK_TICKS {
                    Phase::Message
                } else {
                    Phase::Black(t + 1)
                });
            }
            Some(Phase::Message) => {
                if pressed & (CROSS | CIRCLE | SQUARE) != 0 {
                    self.phase = Some(Phase::Fade(0));
                }
            }
            Some(Phase::Fade(t)) => {
                self.phase = Some(if t + 1 >= FADE_TICKS {
                    Phase::Prostrate
                } else {
                    Phase::Fade(t + 1)
                });
            }
            Some(Phase::Prostrate) => {
                if pressed & (LEFT | RIGHT | UP | CROSS | SQUARE) != 0 {
                    // Regain Control: shaman 2.
                    set_state(pd, 2);
                    self.phase = None;
                    event.risen = true;
                }
            }
        }
        event
    }
    /// Screen darkness: 255 on black and behind the message, easing back.
    pub fn shade(&self) -> u8 {
        match self.phase {
            Some(Phase::Black(_)) | Some(Phase::Message) => 255,
            Some(Phase::Fade(t)) => (255 - u32::from(t) * 255 / u32::from(FADE_TICKS)) as u8,
            _ => 0,
        }
    }
    /// The frame the Shaman shows at a 60 Hz age: `Summon` while summoning,
    /// `Sit Idle` when trapped, `Idle` otherwise.
    pub fn frame(&self, trapped: bool, age: u32) -> usize {
        let (clip, age) = match self.phase {
            Some(Phase::Summon(t)) => (1, u32::from(t)),
            _ => (if trapped { 2 } else { 0 }, age),
        };
        let c = &SHAMAN_CLIPS[clip];
        let f = (u64::from(age) * u64::from(c.fps) / 60) as usize;
        c.start
            + if c.wrap == 0 {
                f % c.count
            } else {
                f.min(c.count - 1)
            }
    }
}

#[cfg(not(test))]
mod live {
    use super::*;
    use crate::game_map::{MAP_CLUT_XY, MAP_PAGE_XY};
    use psx_gpu::ot::OrderingTable;
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        prim::{QuadGouraudBlended, QuadTextured},
    };
    use psx_vram::{Clut, TexDepth, Tpage};
    struct Store;
    impl Player for Store {
        fn get(&self, bit: u32) -> bool {
            persist::player(bit)
        }
        fn put(&mut self, bit: u32, on: bool) {
            if on {
                persist::set_player(bit)
            } else {
                persist::clear_player(bit)
            }
        }
        fn fireball(&self) -> u8 {
            persist::store().levels[persist::FIREBALL_LEVEL]
        }
        fn set_fireball(&mut self, level: u8) {
            persist::set_level(persist::FIREBALL_LEVEL, level)
        }
    }
    static mut SHAMAN: Shaman = Shaman::new();
    #[no_mangle]
    pub static mut HK_SHAMAN_STATE: u32 = 0;
    /// 0 idle, 1 talk, 2 summon, 3 appear, 4 absorb, 5 black, 6 message, 7 fade, 8 prostrate.
    #[no_mangle]
    pub static mut HK_SHAMAN_PHASE: u32 = 0;
    #[no_mangle]
    pub static mut HK_SHAMAN_TALKS: u32 = 0;
    #[no_mangle]
    pub static mut HK_SPELL_EARNED: u32 = 0;
    #[no_mangle]
    pub static mut HK_SHAMAN_DRAWN: u32 = 0;
    fn me() -> &'static mut Shaman {
        unsafe { &mut *(&raw mut SHAMAN) }
    }
    pub fn open() -> bool {
        me().open()
    }
    pub fn consumes_actions() -> bool {
        me().consumes_actions()
    }
    pub fn cancel() {
        me().cancel()
    }
    pub fn shade() -> u8 {
        me().shade()
    }
    pub fn tick(scene: usize, body: [i32; 4], eligible: bool, bits: u16) -> Event {
        let event = me().step(&mut Store, scene, body, eligible, bits);
        unsafe {
            HK_SHAMAN_STATE = u32::from(state(&Store));
            HK_SHAMAN_TALKS += u32::from(event.opened);
            HK_SPELL_EARNED += u32::from(event.wake);
            HK_SHAMAN_PHASE = match me().phase {
                None => 0,
                Some(Phase::Talk(..)) => 1,
                Some(Phase::Summon(_)) => 2,
                Some(Phase::Appear(_)) => 3,
                Some(Phase::Absorb(_)) => 4,
                Some(Phase::Black(_)) => 5,
                Some(Phase::Message) => 6,
                Some(Phase::Fade(_)) => 7,
                Some(Phase::Prostrate) => 8,
            };
        }
        if event.opened {
            crate::audio::ui_confirm();
        }
        if event.shake {
            crate::camera::request(crate::camera::Shake::Kill);
        }
        if event.big_shake {
            crate::camera::request(crate::camera::Shake::Big);
        }
        event
    }
    /// What the Mound shows this frame: the Shaman's frame and where, and the orb.
    fn visible(scene: usize, age: u32) -> [Option<(usize, [i32; 2])>; 2] {
        if scene != SHAMAN_SCENE {
            return [None, None];
        }
        let s = me();
        let shaman = Shaman::standing(&Store).map(|trapped| {
            (
                s.frame(trapped, age),
                if trapped { TRAPPED_AT } else { MEETING_AT },
            )
        });
        [shaman, s.orb().then_some((ORB_FRAME, ORB_AT))]
    }
    /// Draw what `visible` names, unmirrored at its cooked world point.
    #[inline(never)]
    pub fn draw(scene: usize, age: u32, camera: (i32, i32)) -> u32 {
        let mut drawn = 0;
        // Only once the page is up: the art is the map blob's.
        if !crate::game_map::ready() {
            return 0;
        }
        let tpage = Tpage::new(MAP_PAGE_XY.0, MAP_PAGE_XY.1, TexDepth::Bit4).uv_tpage_word(0);
        for (index, at) in visible(scene, age).into_iter().flatten() {
            let f = &SHAMAN_FRAMES[index];
            let (u, v) = (f.u as u8, f.v as u8);
            let b = f.bounds;
            let world = [[b[0], b[3]], [b[2], b[3]], [b[0], b[1]], [b[2], b[1]]];
            let vertices = world.map(|[wx, wy]| {
                let (px, py) = (at[0] + wx, at[1] + wy);
                (
                    (160 + (((i64::from(px) - i64::from(camera.0))
                        * i64::from(crate::KNIGHT_SCALE))
                        >> 28)) as i32,
                    (120 - (((i64::from(py) - i64::from(camera.1))
                        * i64::from(crate::KNIGHT_SCALE))
                        >> 28)) as i32,
                )
            });
            if vertices.iter().all(|p| p.0 < 0)
                || vertices.iter().all(|p| p.0 >= 320)
                || vertices.iter().all(|p| p.1 < 0)
                || vertices.iter().all(|p| p.1 >= 240)
            {
                continue;
            }
            let right = (f.u + f.w - 1) as u8;
            let bottom = (f.v + f.h - 1) as u8;
            let clut = Clut::new(MAP_CLUT_XY[f.clut].0, MAP_CLUT_XY[f.clut].1).uv_clut_word();
            let template = QuadTextured::with_material(
                [(0, 0); 4],
                [(u, v), (right, v), (u, bottom), (right, bottom)],
                TextureMaterial::blended(clut, tpage, (128, 128, 128), BlendMode::Average),
            );
            crate::render::resident_quad(&template, vertices.map(|(x, y)| (x as i16, y as i16)));
            drawn += 1;
        }
        unsafe {
            HK_SHAMAN_DRAWN = HK_SHAMAN_DRAWN.saturating_add(drawn);
        }
        drawn
    }
    /// The black and the fade, drawn under the panel text so the message
    /// shows on black; and the prompt label, panels and message through the
    /// dialogue glyphs. Returns false when there is nothing to compose.
    static mut VEIL: QuadGouraudBlended = QuadGouraudBlended::new(
        [(0, 0), (320, 0), (0, 240), (320, 240)],
        [(0, 0, 0); 4],
        BlendMode::Subtract,
    );
    static mut VEILED: bool = false;
    /// Once per rendered frame, whatever the dialogue panel draws.
    pub fn prepare_veil() {
        let shade = me().shade();
        unsafe {
            VEILED = shade != 0;
            VEIL = QuadGouraudBlended::new(
                [(0, 0), (320, 0), (0, 240), (320, 240)],
                [(shade, shade, shade); 4],
                BlendMode::Subtract,
            );
        }
    }
    pub fn prepare(camera: (i32, i32)) -> bool {
        use crate::dialogue::{panel_frame, panel_pages, panel_prompt, panel_text, panel_width};
        let s = me();
        match s.phase {
            Some(Phase::Talk(talk, page)) => {
                panel_pages(talk.pages(), page);
                true
            }
            Some(Phase::Message) => {
                panel_frame(30, 64, 260, 112);
                panel_text(160 - panel_width(SPELL_NAME) / 2, 76, SPELL_NAME);
                let o = "O";
                let w = panel_width(TAP) + 6 + panel_width(o) + 6 + panel_width(SPELL_LINE);
                let x = 160 - w / 2;
                panel_text(x, 104, TAP);
                panel_text(x + panel_width(TAP) + 6, 104, o);
                panel_text(
                    x + panel_width(TAP) + 6 + panel_width(o) + 6,
                    104,
                    SPELL_LINE,
                );
                for (i, line) in SPELL_NOTE.iter().enumerate() {
                    panel_text(160 - panel_width(line) / 2, 130 + i as i16 * 14, line);
                }
                true
            }
            None => match s.prompt {
                Some(trapped) => {
                    if trapped {
                        panel_prompt(camera, TRAPPED_MARKER, TRAPPED_LABEL)
                    } else {
                        panel_prompt(camera, MEETING_MARKER, MEETING_LABEL)
                    }
                    true
                }
                None => false,
            },
            _ => false,
        }
    }
    pub fn append(ot: &mut OrderingTable<1>) {
        unsafe {
            if VEILED {
                ot.add(0, &mut *(&raw mut VEIL), QuadGouraudBlended::WORDS);
            }
        }
    }
}
#[cfg(not(test))]
pub use live::*;

#[cfg(test)]
mod tests {
    use super::*;
    struct Pd(u32, u8);
    impl Player for Pd {
        fn get(&self, b: u32) -> bool {
            self.0 & (1 << b) != 0
        }
        fn put(&mut self, b: u32, on: bool) {
            if on {
                self.0 |= 1 << b
            } else {
                self.0 &= !(1 << b)
            }
        }
        fn fireball(&self) -> u8 {
            self.1
        }
        fn set_fireball(&mut self, l: u8) {
            self.1 = l;
        }
    }
    fn at(r: [i32; 4]) -> [i32; 4] {
        let x = (r[0] + r[2]) / 2;
        let y = (r[1] + r[3]) / 2;
        [x - 19000, y - 5000, x + 19000, y + 80000]
    }
    fn run(s: &mut Shaman, pd: &mut Pd, body: [i32; 4], bits: u16) -> Event {
        let e = s.step(pd, SHAMAN_SCENE, body, true, bits);
        let f = s.step(pd, SHAMAN_SCENE, body, true, 0);
        Event {
            opened: e.opened || f.opened,
            shake: e.shake || f.shake,
            big_shake: e.big_shake || f.big_shake,
            wake: e.wake || f.wake,
            risen: e.risen || f.risen,
        }
    }
    #[test]
    fn the_spell_is_met_summoned_taken_and_woken_from() {
        let (mut s, mut pd) = (Shaman::new(), Pd(0, 0));
        let near = at(MEETING_TRIGGER);
        s.step(&mut pd, SHAMAN_SCENE, near, true, 0);
        assert_eq!(s.prompt, Some(false));
        assert!(!s.orb());
        assert!(run(&mut s, &mut pd, near, UP).opened);
        assert_eq!(s.phase, Some(Phase::Talk(Talk::Meet, 0)));
        while let Some(Phase::Talk(..)) = s.phase {
            run(&mut s, &mut pd, near, CROSS);
        }
        let mut big = false;
        while s.open() {
            big |= s.step(&mut pd, SHAMAN_SCENE, near, true, 0).big_shake;
        }
        assert!(big);
        assert!(s.orb());
        assert_eq!(state(&pd), 1);
        // Talking again this visit: Summoned 1.
        run(&mut s, &mut pd, near, UP);
        assert_eq!(s.phase, Some(Phase::Talk(Talk::Summoned1, 0)));
        while s.open() {
            run(&mut s, &mut pd, near, CROSS);
        }
        // Into the orb.
        let e = s.step(&mut pd, SHAMAN_SCENE, at(ORB_TRIGGER), true, 0);
        assert!(e.shake);
        assert!(!s.orb());
        let mut woke = false;
        for _ in 0..ABSORB_TICKS + BLACK_TICKS + 2 {
            woke |= s.step(&mut pd, SHAMAN_SCENE, near, true, 0).wake;
        }
        assert!(woke);
        assert_eq!(pd.fireball(), 1);
        assert_eq!(s.phase, Some(Phase::Message));
        assert_eq!(s.shade(), 255);
        run(&mut s, &mut pd, near, CROSS);
        for _ in 0..FADE_TICKS {
            s.step(&mut pd, SHAMAN_SCENE, near, true, 0);
        }
        assert_eq!(s.phase, Some(Phase::Prostrate));
        assert!(run(&mut s, &mut pd, near, RIGHT).risen);
        assert_eq!(state(&pd), 2);
        assert!(!s.open());
        // Now the Trapped Shaman: once Trapped 1, then Trapped 2.
        assert_eq!(Shaman::standing(&pd), Some(true));
        let trapped = at(TRAPPED_TRIGGER);
        run(&mut s, &mut pd, trapped, UP);
        assert_eq!(s.phase, Some(Phase::Talk(Talk::Trapped1, 0)));
        assert_eq!(state(&pd), 3);
        while s.open() {
            run(&mut s, &mut pd, trapped, CROSS);
        }
        run(&mut s, &mut pd, trapped, UP);
        assert_eq!(s.phase, Some(Phase::Talk(Talk::Trapped2, 0)));
    }
    #[test]
    fn leaving_after_the_summon_brings_the_orb_back_and_summoned_2() {
        let (mut s, mut pd) = (Shaman::new(), Pd(0, 0));
        set_state(&mut pd, 1);
        s.step(&mut pd, SHAMAN_SCENE, [0; 4], true, 0);
        assert_eq!(state(&pd), 2);
        assert!(s.orb());
        run(&mut s, &mut pd, at(MEETING_TRIGGER), UP);
        assert_eq!(s.phase, Some(Phase::Talk(Talk::Summoned2, 0)));
    }
}
