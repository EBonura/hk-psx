//! Original health-mask and SOUL-orb poses, uploaded once outside gameplay.
use hk_format::u16_at;
use psx_gpu::{
    material::TextureMaterial,
    ot::OrderingTable,
    prim::{RectFlat, Sprite},
};
use psx_vram::{upload_bytes, Clut, TextureDepth, TexturePage, VramRect};
#[path = "hud_state.rs"]
mod hud_state;
pub use hud_state::{geo_y, mask_position};
static DATA: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/hud.hk"));
const TEXTURE_X: [u16; 5] = [352, 352, 356, 366, 372];
const TEXTURE_Y: [u16; 5] = [0, 16, 0, 0, 0];
const CLUT_Y: [u16; 5] = [480, 481, 482, 483, 484];
fn signed(p: usize) -> i16 {
    u16_at(DATA, p) as i16
}
fn fixed(p: usize) -> i32 {
    i32::from_le_bytes(DATA[p..p + 4].try_into().unwrap())
}
pub fn upload() {
    assert!(&DATA[..8] == b"HKHUD002");
    for i in 0..5 {
        upload_bytes(
            VramRect::new(352, CLUT_Y[i], 16, 1),
            &DATA[80 + i * 32..112 + i * 32],
        );
        let p = 8 + i * 8;
        let w = u16_at(DATA, p);
        let h = u16_at(DATA, p + 2);
        let start = 240 + u16_at(DATA, p + 4) as usize;
        let len = ((w as usize + 3) / 4) * 2 * h as usize;
        upload_bytes(
            VramRect::new(TEXTURE_X[i], TEXTURE_Y[i], (w + 3) / 4, h),
            &DATA[start..start + len],
        );
    }
}
// All HUD commands remain in the scenery DMA chain. No immediate GP0 writes
// or texture transfers occur while drawing a frame.
#[repr(C, align(4))]
struct Mode {
    tag: u32,
    word: u32,
}
static mut MODE: Mode = Mode { tag: 0, word: 0 };
static mut MASKS: [Sprite; 9] = [const { Sprite::new(0, 0, 0, 0, (0, 0), 0, 128, 128, 128) }; 9];
static mut ORB: [Sprite; 3] = [const { Sprite::new(0, 0, 0, 0, (0, 0), 0, 128, 128, 128) }; 3];
static mut PAUSE: [RectFlat; 2] = [const { RectFlat::new(0, 0, 0, 0, 220, 220, 220) }; 2];
fn sprite(icon: usize, x: i16, y: i16, cut: u16, gain: u8) -> Sprite {
    let p = 8 + icon * 8;
    let material = TextureMaterial::opaque(
        Clut::new(352, CLUT_Y[icon]).uv_word(),
        TexturePage::new(320, 0, TextureDepth::Bit4).uv_word(0),
        (gain, gain, gain),
    );
    Sprite::with_material(
        x,
        y + cut as i16,
        u16_at(DATA, p),
        u16_at(DATA, p + 2) - cut,
        (
            ((TEXTURE_X[icon] - 320) * 4) as u8,
            (TEXTURE_Y[icon] + cut) as u8,
        ),
        material,
    )
}
/// `Health Break` and `Health Refill` (the health masks' tk2d clips, resources.assets 21155) play six
/// frames at 20 fps each: 18 ticks. Their frames are not cooked (the HUD's art is linked and VRAM has
/// no room for twelve more poses), so the masks' own two poses act them out: a break flashes the full
/// mask white and throws its four quarters apart and down while they darken, over the empty pose; a
/// refill flashes the full mask in from white.
const MASK_ANIM_TICKS: u8 = 18;
static mut BREAK_AGE: [u8; 9] = [0; 9];
static mut REFILL_AGE: [u8; 9] = [0; 9];
static mut SEEN_HEALTH: u16 = u16::MAX;
static mut SHARDS: [Sprite; 8] = [const { Sprite::new(0, 0, 0, 0, (0, 0), 0, 128, 128, 128) }; 8];
/// The SOUL orb's liquid: `MP Lose` moves it down with an `iTweenMoveTo` of 0.5 s (30 ticks; the
/// action's default ease, linear); `MP Gain` sets it at once.
const SOUL_DRAIN_TICKS: u8 = 30;
static mut SOUL_SHOWN: u16 = 0;
static mut SOUL_FROM: u16 = 0;
static mut SOUL_TO: u16 = 0;
static mut SOUL_AGE: u8 = 0;
/// GeoCounter (Assembly-CSharp): a change waits 1.5 s (`addRollerStartTimer`) with "+ N" (or "- N")
/// under the coin, then rolls into the total by `changePerTick` = max(1, N * 0.025 * 1.75) every
/// 0.025 s, the shown amount counting down with it. Time here is in thirds of a millisecond, so a
/// 60 Hz tick is 50 and a digit step 75.
const GEO_WAIT_TICKS: u8 = 90;
static mut GEO_SHOWN: u32 = u32::MAX;
static mut GEO_SEEN: u32 = 0;
static mut GEO_ADD: u32 = 0;
static mut GEO_TAKE: u32 = 0;
static mut GEO_WAIT: u8 = 0;
static mut GEO_STEP: u32 = 1;
static mut GEO_TIMER: i32 = 0;
/// One simulation tick of the HUD's own animations.
#[inline(never)]
pub fn tick(health: u16, soul: u16, geo: u32) {
    unsafe {
        if SEEN_HEALTH == u16::MAX {
            (SEEN_HEALTH, SOUL_SHOWN, SOUL_TO) = (health, soul, soul);
        }
        for i in 0..9u16 {
            let k = i as usize;
            if i >= health && i < SEEN_HEALTH {
                (BREAK_AGE[k], REFILL_AGE[k]) = (1, 0);
            } else if i < health && i >= SEEN_HEALTH {
                (BREAK_AGE[k], REFILL_AGE[k]) = (0, 1);
            } else {
                for age in [&mut BREAK_AGE[k], &mut REFILL_AGE[k]] {
                    if *age != 0 {
                        *age = if *age >= MASK_ANIM_TICKS { 0 } else { *age + 1 };
                    }
                }
            }
        }
        SEEN_HEALTH = health;
        if soul < SOUL_TO {
            (SOUL_FROM, SOUL_TO, SOUL_AGE) = (SOUL_SHOWN, soul, 1);
        } else if soul > SOUL_TO {
            (SOUL_SHOWN, SOUL_TO, SOUL_AGE) = (soul, soul, 0);
        }
        if SOUL_AGE != 0 {
            let a = u32::from(SOUL_AGE);
            SOUL_SHOWN = (u32::from(SOUL_FROM)
                - (u32::from(SOUL_FROM) - u32::from(SOUL_TO)) * a / u32::from(SOUL_DRAIN_TICKS))
                as u16;
            SOUL_AGE = if SOUL_AGE >= SOUL_DRAIN_TICKS { 0 } else { SOUL_AGE + 1 };
        }
        geo_tick(geo);
    }
}
unsafe fn geo_tick(geo: u32) {
    if GEO_SHOWN == u32::MAX {
        geo_refresh(geo);
    }
    if geo > GEO_SEEN {
        // AddGeo cancels a running take: the total jumps to what was left.
        if GEO_TAKE > 0 {
            (GEO_SHOWN, GEO_TAKE) = (GEO_SEEN, 0);
        }
        GEO_ADD += geo - GEO_SEEN;
        GEO_WAIT = GEO_WAIT_TICKS;
        GEO_STEP = (GEO_ADD * 7 / 160).max(1);
    } else if geo < GEO_SEEN {
        if GEO_ADD > 0 {
            (GEO_SHOWN, GEO_ADD) = (GEO_SEEN, 0);
        }
        GEO_TAKE += GEO_SEEN - geo;
        GEO_WAIT = GEO_WAIT_TICKS;
        GEO_STEP = (GEO_TAKE * 7 / 160).max(1);
    }
    GEO_SEEN = geo;
    if GEO_WAIT > 0 {
        GEO_WAIT -= 1;
        return;
    }
    if GEO_ADD == 0 && GEO_TAKE == 0 {
        GEO_SHOWN = geo;
        return;
    }
    GEO_TIMER -= 50;
    while GEO_TIMER < 0 && (GEO_ADD > 0 || GEO_TAKE > 0) {
        GEO_TIMER += 75;
        if GEO_ADD > 0 {
            let n = GEO_STEP.min(GEO_ADD);
            (GEO_ADD, GEO_SHOWN) = (GEO_ADD - n, GEO_SHOWN + n);
        } else {
            let n = GEO_STEP.min(GEO_TAKE).min(GEO_SHOWN);
            (GEO_TAKE, GEO_SHOWN) = (GEO_TAKE - n.max(1).min(GEO_TAKE), GEO_SHOWN - n);
        }
    }
    if GEO_ADD == 0 && GEO_TAKE == 0 {
        (GEO_SHOWN, GEO_TIMER) = (geo, 0);
    }
}
/// A new scene, a load or a respawn (`NewSceneRefresh`): the counter shows the purse, no roller runs.
pub fn geo_refresh(geo: u32) {
    unsafe {
        (GEO_SHOWN, GEO_SEEN, GEO_ADD, GEO_TAKE, GEO_WAIT, GEO_TIMER) = (geo, geo, 0, 0, 0, 0);
    }
}
/// The Geo total to draw, and the amount waiting beside it: positive for "+ N", negative for "- N".
pub fn geo_counter(geo: u32) -> (u32, i32) {
    unsafe {
        if GEO_SHOWN == u32::MAX {
            return (geo, 0);
        }
        (GEO_SHOWN, GEO_ADD as i32 - GEO_TAKE as i32)
    }
}
/// Whether this frame's screen covers the HUD (set by `dialogue::prepare`, before the frame is submitted).
static mut HIDDEN: bool = false;
pub fn hide(hidden: bool) {
    unsafe { HIDDEN = hidden };
}
/// Prepend before scenery, leaving the HUD last in GPU order. Caller owns
/// every static packet until both DMA and rasterization finish.
pub fn append(
    ot: &mut OrderingTable<1>,
    health: u16,
    max_health: u16,
    soul: u16,
    max_soul: u16,
    paused: bool,
    blue_health: u16,
) {
    unsafe {
        // The charm board covers the screen, as the source's inventory does: no masks, orb or
        // Geo count over its rows.
        if HIDDEN {
            return;
        }
    }
    crate::lifeblood::append_hud(ot, max_health, blue_health);
    unsafe {
        if paused {
            for (i, x) in [292, 300].into_iter().enumerate().rev() {
                PAUSE[i] = RectFlat::new(x, 16, 4, 12, 220, 220, 220);
                crate::display::ot_add(ot, 0, &mut PAUSE[i], RectFlat::WORDS);
            }
        }
        let mut shards = 0;
        for i in (0..max_health.min(9)).rev() {
            let (x, y) = mask_position(i);
            let k = i as usize;
            let (refill, broke) = (REFILL_AGE[k], BREAK_AGE[k]);
            // A refill comes in from white; anything else is the pose its health says.
            let gain = if refill != 0 && i < health {
                (255 - u32::from(refill) * 127 / u32::from(MASK_ANIM_TICKS)) as u8
            } else {
                128
            };
            MASKS[k] = sprite(usize::from(i >= health), x, y, 0, gain);
            if broke != 0 && i >= health && shards + 4 <= 8 {
                // The full pose's quarters fly apart and fall, white at first, then darkening.
                let a = i16::from(broke);
                let gain = if broke < 4 { 255 } else { (128 - (u16::from(broke) - 4) * 128 / 14) as u8 };
                for q in 0..4u16 {
                    let (qx, qy) = (q & 1, q >> 1);
                    let (w, h) = (6u16, 7u16);
                    let dx = if qx == 0 { -a / 2 } else { a / 2 };
                    let dy = (a * a) / 24 - if qy == 0 { a / 3 } else { 0 };
                    SHARDS[shards] = Sprite::with_material(
                        x + (qx * w) as i16 + dx,
                        y + (qy * h) as i16 + dy,
                        w,
                        h,
                        (
                            ((TEXTURE_X[0] - 320) * 4 + qx * w) as u8,
                            (TEXTURE_Y[0] + qy * h) as u8,
                        ),
                        TextureMaterial::opaque(
                            Clut::new(352, CLUT_Y[0]).uv_word(),
                            TexturePage::new(320, 0, TextureDepth::Bit4).uv_word(0),
                            (gain, gain, gain),
                        ),
                    );
                    crate::display::ot_add(ot, 0, &mut SHARDS[shards], Sprite::WORDS);
                    shards += 1;
                }
            }
            crate::display::ot_add(ot, 0, &mut MASKS[k], Sprite::WORDS);
        }
        let soul = if SOUL_AGE != 0 { SOUL_SHOWN } else { soul };
        let fill_h = u16_at(DATA, 34);
        let state = hud_state::SoulSpec {
            cut_zero: fixed(56),
            cut_per_mp: fixed(60),
            height: fill_h,
            focus_cost: u16_at(DATA, 64),
            eyes_at: u16_at(DATA, 66),
            hide_at: u16_at(DATA, 68),
            dim_gain: u16_at(DATA, 70) as u8,
            source_max: u16_at(DATA, 72),
        }
        .state(soul, max_soul);
        // Source Eyes Control changes at 50 MP independently of the 33 MP tint.
        if state.eyes {
            ORB[2] = sprite(4, 12 + signed(52), 10 + signed(54), 0, 128);
            crate::display::ot_add(ot, 0, &mut ORB[2], Sprite::WORDS);
        }
        if state.fill {
            ORB[1] = sprite(3, 12 + signed(48), 10 + signed(50), state.cut, state.gain);
            crate::display::ot_add(ot, 0, &mut ORB[1], Sprite::WORDS);
        }
        ORB[0] = sprite(2, 12, 10, 0, 128);
        crate::display::ot_add(ot, 0, &mut ORB[0], Sprite::WORDS);
        MODE.word = TextureMaterial::opaque(
            0,
            TexturePage::new(320, 0, TextureDepth::Bit4).uv_word(0),
            (128, 128, 128),
        )
        .draw_mode_word();
        crate::display::ot_add(ot, 0, &mut *(&raw mut MODE), 1);
    }
}
