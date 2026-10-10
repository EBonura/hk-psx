//! The Focus effects at the Knight, from the Spell Control FSM: `Lines Anim` plays its
//! Focus Effect (the appear frames once, then the loop section) from the `Focus` state
//! until a cancel or the finish, which plays Focus Effect End; each heal (`Focus Heal`)
//! activates `Heal Anim`, whose Burst Effect runs once. The frames are the ability art's
//! (host/ability_art.py), drawn additively. The soul orb's `focus_ready` cue (Soul Orb
//! Control, `Can Heal 2`) plays when the SOUL reaches the cost while health is not full.
use crate::ability_art::{
    ABILITY_CLIPS, BURST_EFFECT, FOCUS_EFFECT, FOCUS_EFFECT_END, FOCUS_EFFECT_LOOP_START,
};

static mut LINES: Option<u32> = None;
static mut END: Option<u32> = None;
static mut BURST: Option<u32> = None;
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
        let ready = WAS_SOUL < cost && soul >= cost && !health_full;
        WAS_SOUL = soul;
        if ready {
            HK_FOCUS_READY_CUES = HK_FOCUS_READY_CUES.wrapping_add(1);
        }
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
/// The ability-art frames to draw now: the lines (or their end), then the burst.
pub fn frames() -> [Option<usize>; 2] {
    unsafe {
        let lines = match (LINES, END) {
            (Some(a), _) => Some(frame(FOCUS_EFFECT, a, true)),
            (None, Some(a)) => Some(frame(FOCUS_EFFECT_END, a, false)),
            _ => None,
        };
        [lines, BURST.map(|a| frame(BURST_EFFECT, a, false))]
    }
}
