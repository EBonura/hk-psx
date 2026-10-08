//! Water drips (`WaterDrip`): a drop hangs, swells, falls and splashes.
//!
//! The source coroutine, per drip: `Idle` for Random.Range(idleTimeMin,
//! idleTimeMax) with its collider off; `Drip` played to its end; `Fall` with
//! gravity on at `fallVelocity` until the collider touches terrain; then
//! stopped, moved down by `impactTranslation`, `Impact` played to its end, put
//! back where it started, and round again. Where it lands is fixed by the room,
//! so the props cooker (host/hk-cook/src/props.rs) measures each drop's fall
//! at cook time and this only keeps the clock. The art is one sheet in the
//! scene actor bank, like the arena gates', drawn after the back scenery on
//! the gameplay plane the drips use.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/drips.rs"));
use hk_format::Room;

/// Drip sprites drawn, and drips left out for want of an animation slot (must
/// stay zero: a drop that vanishes for a frame is pop-in).
#[no_mangle] pub static mut HK_DRIP_DRAWS: u32 = 0;
#[no_mangle] pub static mut HK_DRIP_ART_DROPPED: u32 = 0;

const IDLE: u8 = 0;
const DRIP: u8 = 1;
const FALL: u8 = 2;
const IMPACT: u8 = 3;
/// The most drips one scene places (host/hk-cook/src/props.rs refuses more).
const MAX: usize = 16;

#[derive(Clone, Copy)]
struct State { phase: u8, age: u16, wait: u16 }

struct Scene { scene: usize, first: usize, count: usize, rng: u32, drips: [State; MAX] }

static mut NOW: Scene = Scene { scene: usize::MAX, first: 0, count: 0, rng: 0, drips: [State { phase: IDLE, age: 0, wait: 0 }; MAX] };

/// Random.Range(idleTimeMin, idleTimeMax) in ticks, from one LCG per scene so a
/// replayed route drips the same.
fn idle(rng: &mut u32) -> u16 {
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    IDLE_MIN + ((*rng >> 16) * (IDLE_MAX - IDLE_MIN) as u32 >> 16) as u16
}

/// Ticks a play-once clip takes at its rate.
const fn clip_ticks(clip: usize) -> u16 {
    ((CLIP_FRAMES[clip].len() as u32 * 60).div_ceil(CLIP_FPS[clip] as u32)) as u16
}

/// One simulation tick of the current scene's drips; a new scene starts every
/// drop idle, as a scene load restarts each coroutine.
#[inline(never)]
pub fn tick(scene: usize) {
    let now = unsafe { &mut *(&raw mut NOW) };
    if now.scene != scene {
        let first = DRIPS.partition_point(|d| (d.0 as usize) < scene);
        let count = DRIPS[first..].iter().take_while(|d| d.0 as usize == scene).count().min(MAX);
        *now = Scene { scene, first, count, rng: 0x44_52_49_50 ^ scene as u32, drips: [State { phase: IDLE, age: 0, wait: 0 }; MAX] };
        for i in 0..count { let w = idle(&mut now.rng); now.drips[i].wait = w; }
    }
    for i in 0..now.count {
        let fall = DRIPS[now.first + i].3;
        let d = &mut now.drips[i];
        d.age = d.age.saturating_add(1);
        let end = match d.phase { IDLE => d.wait, DRIP => clip_ticks(1), FALL => fall, IMPACT => clip_ticks(3), _ => 0 };
        if d.age >= end && end != u16::MAX {
            d.phase = (d.phase + 1) % 4;
            d.age = 0;
            if d.phase == IDLE { d.wait = idle(&mut now.rng); }
        }
    }
}

/// Where each visible drop is and which sprite it shows this frame.
pub struct Draws { count: usize, texture: usize, at: [[i32; 2]; MAX], sprite: [u8; MAX], x_scale: [i16; MAX] }

/// Pick the drops on screen and bind the sheet's slot.
#[inline(never)]
pub fn prepare(room: &Room, region: usize, scene: usize, camera: (i32, i32), needed: &mut [u16], len: &mut usize) -> Draws {
    let mut draws = Draws { count: 0, texture: 0, at: [[0; 2]; MAX], sprite: [0; MAX], x_scale: [4096; MAX] };
    let now = unsafe { &*(&raw const NOW) };
    if now.scene != scene || now.count == 0 { return draws; }
    let Some(sheet) = sheet_frame(scene, region) else { return draws };
    for i in 0..now.count {
        let (_, x, y, fall, x_scale) = DRIPS[now.first + i];
        let (x, y) = ((x as i32) << 10, (y as i32) << 10);
        let d = now.drips[i];
        let (clip, dy) = match d.phase {
            IDLE => (0, 0),
            DRIP => (1, 0),
            // y(t) = vt - g t^2 / 2 from the source's fallVelocity and gravity.
            FALL => (2, fall_drop(d.age as i32)),
            IMPACT => (3, fall_drop(fall as i32) + IMPACT_DROP),
            _ => continue,
        };
        let y = y - dy;
        if (x - camera.0).abs() > 12 * 65536 || (y - camera.1).abs() > 10 * 65536 { continue; }
        let frames = CLIP_FRAMES[clip];
        let step = ((d.age as u32 * CLIP_FPS[clip] as u32 / 60) as usize).min(frames.len() - 1);
        draws.at[draws.count] = [x, y];
        draws.sprite[draws.count] = frames[step];
        draws.x_scale[draws.count] = x_scale;
        draws.count += 1;
    }
    if draws.count == 0 { return draws; }
    let key = room.frame_grid(sheet).0;
    draws.texture = key;
    let key = key as u16;
    if !needed[..*len].contains(&key) {
        if *len >= needed.len() {
            unsafe { HK_DRIP_ART_DROPPED = HK_DRIP_ART_DROPPED.wrapping_add(draws.count as u32) }
            draws.count = 0;
            return draws;
        }
        needed[*len] = key;
        *len += 1;
    }
    draws
}

/// The view's drip sheet frame: its scene's usual one unless the view is listed.
fn sheet_frame(scene: usize, region: usize) -> Option<usize> {
    if let Ok(i) = VIEW_ART.binary_search_by_key(&(region as u16), |&(slot, _)| slot) { return Some(VIEW_ART[i].1 as usize); }
    SCENE_ART.binary_search_by_key(&(scene as u8), |&(s, _)| s).ok().map(|i| SCENE_ART[i].1 as usize)
}

/// Distance fallen `ticks` after the drop lets go, Q16 units.
fn fall_drop(ticks: i32) -> i32 {
    // FALL_SPEED and GRAVITY are Q16 units/s and units/s^2.
    let t = ticks.min(600);
    (FALL_SPEED as i64 * t as i64 / 60 + GRAVITY as i64 * (t * t) as i64 / 7200) as i32
}

impl Draws {
    #[inline(never)]
    pub fn draw(&self, camera: (i32, i32)) -> u32 {
        for i in 0..self.count {
            let p = self.at[i];
            let s = self.sprite[i] as usize;
            let b = SPRITE_BOX[s];
            let r = SPRITE_RECT[s];
            // The parent's x scale (Crossroads_46, mirrored in 46b) on the box.
            let sx = |v: i32| ((v as i64 * self.x_scale[i] as i64) >> 12) as i32;
            let coords = [(sx(b[0]), b[3]), (sx(b[2]), b[3]), (sx(b[0]), b[1]), (sx(b[2]), b[1])];
            let mut v = [(0i16, 0i16); 4];
            for (k, (x, y)) in coords.iter().enumerate() {
                v[k] = ((160 + ((((p[0] + x - camera.0) >> 8) * crate::KNIGHT_SCALE) >> 20)) as i16,
                        (120 - ((((p[1] + y - camera.1) >> 8) * crate::KNIGHT_SCALE) >> 20)) as i16);
            }
            crate::render::texture_sub(self.texture, v, (128, 128, 128), r);
        }
        unsafe { HK_DRIP_DRAWS = HK_DRIP_DRAWS.wrapping_add(self.count as u32) }
        self.count as u32
    }
}
