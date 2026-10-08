//! The arena gates as the original draws them: each gate's `BG Control` clip,
//! and the `Close Effect` flash its `Close 2` starts, from the few sheet
//! textures host/battle_gates.py appended to its scene's actor bank. Every
//! sprite is a rectangle of one sheet (`SPRITE_RECT`), so all the gates on
//! screen bind at most `SHEETS` animation slots between them.
//!
//! `battle_gates.rs` owns the state (which clip, since when) and the collider;
//! this only turns that into quads. An open gate is not invisible: `BG Opened`
//! is the gate's tip under its lintel, and the closed pose is the whole gate.
use crate::battle_gates::{self, ART, EFFECT_OFFSET, GATES, POSITION, SHEETS, SPRITE_BOX, SPRITE_RECT};
use hk_format::Room;

/// Gate and effect quads drawn, and sprites left out because the frame's
/// animation working set had no slot for their sheet. The second must stay
/// zero on every route: a gate that drops out of a frame is pop-in.
#[no_mangle] pub static mut HK_GATE_DRAWS: u32 = 0;
#[no_mangle] pub static mut HK_GATE_ART_DROPPED: u32 = 0;

/// Half a screen at KNIGHT_SCALE (10.8 by 8.1 units) plus half a gate.
const REACH_X: i32 = 12 * 65536;
const REACH_Y: i32 = 11 * 65536;
/// A gate and its flash.
const MAX_DRAWS: usize = GATES * 2;

/// The view's first gate sheet frame, when its pack carries the gate art.
fn base(region: usize) -> Option<usize> {
    ART.binary_search_by_key(&(region as u16), |&(slot, _)| slot).ok().map(|i| ART[i].1 as usize)
}

/// The sprites one frame draws, each at its origin, and each sheet's texture.
pub struct Draws {
    count: usize,
    texture: [u16; SHEETS],
    origin: [[i32; 2]; MAX_DRAWS],
    sprite: [u8; MAX_DRAWS],
}

impl Draws {
    pub const NONE: Self = Self { count: 0, texture: [0; SHEETS], origin: [[0; 2]; MAX_DRAWS], sprite: [0; MAX_DRAWS] };
    fn push(&mut self, origin: [i32; 2], sprite: usize) {
        if self.count < MAX_DRAWS {
            self.origin[self.count] = origin;
            self.sprite[self.count] = sprite as u8;
            self.count += 1;
        }
    }
}

/// Pick the gates on screen and append each sheet they use once.
#[inline(never)]
pub fn prepare(room: &Room, region: usize, scene: usize, camera: (i32, i32),
               needed: &mut [u16], len: &mut usize) -> Draws {
    let mut draws = Draws::NONE;
    let mask = battle_gates::scene_gates(scene);
    if mask == 0 { return draws; }
    let Some(first) = base(region) else { return draws };
    let mut bits = mask;
    while bits != 0 {
        let gate = bits.trailing_zeros() as usize;
        bits &= bits - 1;
        let p = POSITION[gate];
        if (p[0] - camera.0).abs() > REACH_X || (p[1] - camera.1).abs() > REACH_Y { continue; }
        let (sprite, effect) = battle_gates::sprites(gate);
        draws.push(p, sprite);
        if let Some(effect) = effect { draws.push([p[0] + EFFECT_OFFSET[0], p[1] + EFFECT_OFFSET[1]], effect); }
    }
    // Each sheet a drawn sprite sits on, once; a sheet with no room drops its
    // sprites (counted) rather than draw from a slot nobody bound.
    let mut used = 0u8;
    for i in 0..draws.count { used |= 1 << SPRITE_RECT[draws.sprite[i] as usize][0]; }
    let mut kept = 0u8;
    for sheet in 0..SHEETS {
        if used & (1 << sheet) == 0 { continue; }
        let key = room.frame_grid(first + sheet).0 as u16;
        draws.texture[sheet] = key;
        if needed[..*len].contains(&key) { kept |= 1 << sheet; continue; }
        if *len < needed.len() {
            needed[*len] = key;
            *len += 1;
            kept |= 1 << sheet;
        }
    }
    if kept != used {
        let mut n = 0;
        for i in 0..draws.count {
            if kept & (1 << SPRITE_RECT[draws.sprite[i] as usize][0]) != 0 {
                draws.origin[n] = draws.origin[i];
                draws.sprite[n] = draws.sprite[i];
                n += 1;
            }
        }
        unsafe { HK_GATE_ART_DROPPED = HK_GATE_ART_DROPPED.wrapping_add((draws.count - n) as u32) }
        draws.count = n;
    }
    draws
}

impl Draws {
    /// The sprites on the gameplay plane, upright and untinted.
    #[inline(never)]
    pub fn draw(&self, camera: (i32, i32)) -> u32 {
        for i in 0..self.count {
            let p = self.origin[i];
            let sprite = self.sprite[i] as usize;
            let b = SPRITE_BOX[sprite];
            let r = SPRITE_RECT[sprite];
            let coords = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
            let mut v = [(0i16, 0i16); 4];
            for (k, (x, y)) in coords.iter().enumerate() {
                v[k] = ((160 + ((((p[0] + x - camera.0) >> 8) * crate::KNIGHT_SCALE) >> 20)) as i16,
                        (120 - ((((p[1] + y - camera.1) >> 8) * crate::KNIGHT_SCALE) >> 20)) as i16);
            }
            crate::render::texture_sub(self.texture[r[0] as usize] as usize, v, (128, 128, 128), [r[1], r[2], r[3], r[4]]);
        }
        unsafe { HK_GATE_DRAWS = HK_GATE_DRAWS.wrapping_add(self.count as u32) }
        self.count as u32
    }
}
