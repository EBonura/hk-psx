//! The Focus effects at the Knight, from the Spell Control FSM: `Lines Anim` plays its
//! Focus Effect (the appear frames once, then the loop section) from the `Focus` state
//! until a cancel or the finish, which plays Focus Effect End; each heal (`Focus Heal`)
//! activates `Heal Anim`, whose Burst Effect runs once. The frames are the ability art's
//! (host/ability_art.py), drawn additively. The soul orb's `focus_ready` cue (Soul Orb
//! Control, `Can Heal 2`) plays when the SOUL reaches the cost while health is not full.
use crate::ability_art::{
    ABILITY_CLIPS, BURST_EFFECT, FOCUS_EFFECT, FOCUS_EFFECT_END, FOCUS_EFFECT_LOOP_START,
    SOUL_BURST,
};

static mut LINES: Option<u32> = None;
static mut END: Option<u32> = None;
static mut BURST: Option<u32> = None;
static mut SOUL: Option<u32> = None;
/// Ticks since the heal's `White Flash R` was spawned.
static mut FLASH: Option<u32> = None;
static mut WAS_ACTIVE: bool = false;
static mut WAS_SOUL: u16 = 0;
#[no_mangle]
pub static mut HK_FOCUS_BURSTS: u32 = 0;
#[no_mangle]
pub static mut HK_FOCUS_READY_CUES: u32 = 0;
/// Bursts of Focus dust raised so far (`dust_due`).
#[no_mangle]
pub static mut HK_FOCUS_DUST_BURSTS: u32 = 0;
/// The owner the cook gives the Knight's `Dust L` and `Dust R` emitters in every scene
/// (host/hk-cook break_effects.rs `HERO_DUST_OWNER`).
pub const DUST_OWNER: usize = 0xFFFF;
/// Each cooked burst is six particles over six ticks (60 a second, the rate the Spell Control
/// FSM sets while Focus runs), so a burst every six ticks is the continuous emission.
const DUST_PERIOD: u32 = 6;

fn ticks(clip: usize) -> u32 {
    ABILITY_CLIPS[clip].count as u32 * 60 / ABILITY_CLIPS[clip].fps
}

/// Call once per consumed tick. True when the soul orb's ready cue is due.
pub fn tick(active: bool, completed: bool, soul: u16, cost: u16, health_full: bool) -> bool {
    unsafe {
        if active {
            LINES = Some(if WAS_ACTIVE {
                LINES.map_or(0, |a| a + 1)
            } else {
                0
            });
            END = None;
        } else {
            if WAS_ACTIVE {
                END = Some(0);
            } else if let Some(a) = END {
                END = if a + 1 >= ticks(FOCUS_EFFECT_END) {
                    None
                } else {
                    Some(a + 1)
                };
            }
            LINES = None;
        }
        WAS_ACTIVE = active;
        if completed {
            BURST = Some(0);
            HK_FOCUS_BURSTS = HK_FOCUS_BURSTS.wrapping_add(1);
        } else if let Some(a) = BURST {
            BURST = if a + 1 >= ticks(BURST_EFFECT) {
                None
            } else {
                Some(a + 1)
            };
        }
        FLASH = if completed {
            Some(0)
        } else {
            FLASH.and_then(|a| (a + 1 < FLASH_TICKS).then_some(a + 1))
        };
        let ready = WAS_SOUL < cost && soul >= cost && !health_full;
        WAS_SOUL = soul;
        SOUL = if ready {
            HK_FOCUS_READY_CUES = HK_FOCUS_READY_CUES.wrapping_add(1);
            Some(0)
        } else {
            SOUL.and_then(|a| (a + 1 < ticks(SOUL_BURST)).then_some(a + 1))
        };
        ready
    }
}
/// True on the ticks Focus dust is due: from the drain until a cancel or the finish, every
/// `DUST_PERIOD` ticks. Counts the burst.
pub fn dust_due() -> bool {
    unsafe {
        let due = LINES.is_some_and(|age| age % DUST_PERIOD == 0);
        if due {
            HK_FOCUS_DUST_BURSTS = HK_FOCUS_DUST_BURSTS.wrapping_add(1);
        }
        due
    }
}
/// Stop everything at once (a load or a reset).
pub fn reset() {
    unsafe {
        LINES = None;
        END = None;
        BURST = None;
        SOUL = None;
        FLASH = None;
        WAS_ACTIVE = false;
    }
}

fn frame(clip: usize, age: u32, looping: bool) -> usize {
    let c = ABILITY_CLIPS[clip];
    let f = (u64::from(age) * u64::from(c.fps) / 60) as usize;
    let f = if looping && f >= c.count {
        FOCUS_EFFECT_LOOP_START
            + (f - FOCUS_EFFECT_LOOP_START) % (c.count - FOCUS_EFFECT_LOOP_START)
    } else {
        f.min(c.count - 1)
    };
    c.start + f
}
/// The ability-art frames to draw now: the lines (or their end), the heal's burst, then the
/// soul orb's star.
pub fn frames() -> [Option<usize>; 3] {
    unsafe {
        let lines = match (LINES, END) {
            (Some(a), _) => Some(frame(FOCUS_EFFECT, a, true)),
            (None, Some(a)) => Some(frame(FOCUS_EFFECT_END, a, false)),
            _ => None,
        };
        [
            lines,
            BURST.map(|a| frame(BURST_EFFECT, a, false)),
            SOUL.map(|a| frame(SOUL_BURST, a, false)),
        ]
    }
}

/// `White Flash R`, spawned at the Knight by `Focus Heal` (resources.assets:5267): a sprite
/// (`white_light`, a pale disc) scaled to cover the screen, white at alpha 0.52 and faded to
/// nothing in a second by `SimpleSpriteFade`. Measured in a real run of the original, it lifts a dark
/// view by 0.29 of what lies between it and white, halving in half a second. The GPU has no
/// alpha blend, so it is an additive wash of that size: grey 58, falling to nothing in 60 ticks.
const FLASH_TICKS: u32 = 60;
const FLASH_GREY: u32 = 58;
/// The wash covers the top 180 of the screen's 240 rows. The tail of the frame after the CPU's last
/// kick carries it, and the full 320x240 (0.78 clocks a pixel, 61k clocks) tipped two frames just after
/// the heal over two vblanks; 210 rows still tipped one, 180 none (every-tick hkref replay of the focus
/// route). The rows left out are the ground and the black under it.
const FLASH_ROWS: u16 = 180;
/// The flash's grey level this tick, 0 when none is running.
pub fn flash_level() -> u8 {
    unsafe { FLASH.map_or(0, |a| (FLASH_GREY * (FLASH_TICKS - a) / FLASH_TICKS) as u8) }
}
/// The wash as one packet: the draw mode (additive) and a flat semi-transparent rectangle over the
/// whole screen. A rectangle fills cheaper than the two Gouraud triangles of a quad.
#[cfg(not(test))]
#[repr(C, align(4))]
struct Wash {
    tag: u32,
    draw_mode: u32,
    color_cmd: u32,
    xy: u32,
    wh: u32,
}
/// Over the world, under the HUD: insertion prepends, so call this right after the HUD's.
#[cfg(not(test))]
#[inline(never)]
pub fn append(ot: &mut psx_gpu::ot::OrderingTable<1>) {
    use psx_gpu::material::{BlendMode, TextureMaterial};
    use psx_hw::gpu::{pack_color, pack_vertex, pack_xy};
    static mut WASH: Wash = Wash {
        tag: 0,
        draw_mode: 0,
        color_cmd: 0,
        xy: 0,
        wh: 0,
    };
    let g = flash_level();
    if g == 0 {
        return;
    }
    unsafe {
        WASH = Wash {
            tag: 0,
            draw_mode: TextureMaterial::blended(0, 0, (0, 0, 0), BlendMode::Add).draw_mode_word(),
            color_cmd: 0x6200_0000 | pack_color(g, g, g),
            xy: pack_vertex(0, 0),
            wh: pack_xy(320, FLASH_ROWS),
        };
        ot.add(0, &mut *(&raw mut WASH), 4);
    }
}
