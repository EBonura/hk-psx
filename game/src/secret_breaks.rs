//! Hidden walls and cracked floors: the hit counter, lockout, recoil and sag
//! of the secrets host/secret_breaks.py cooks. A secret is an ordinary bank
//! breakable (flag 8) followed by a `KIND_SECRET` object, so the broken bit,
//! the terrain it takes away and its save item are world.rs's; this module is
//! only what happens before the break.
//!
//! Hits taken are FSM variables in the source (`Hits`), not saved: leaving the
//! scene or dying starts every unbroken secret over, as a reload does there.
use hk_format::world_meta::{self as meta, KIND_SECRET};
use hk_sim::ONE;

pub const FAMILY_WALL: u16 = 1;
pub const FAMILY_WALL_TK2D: u16 = 2;
pub const FAMILY_FLOOR: u16 = 3;
pub const FAMILY_FLOOR_OPEN: u16 = 4;
/// Most secrets a scene holds; the cook numbers them 127 downwards.
pub const SLOTS: usize = 8;
const BREAKABLES_PER_SCENE: usize = crate::world::BREAKABLES_PER_SCENE;
/// `Hit X` then `Return X`, two `Wait 0.1` (breakable_wall_v2, Break Wall 2).
const WALL_LOCKOUT: u8 = 12;
/// break_floor `Hit 1` / `Hit 2` end in `Wait 0.25`.
const FLOOR_LOCKOUT: u8 = 15;
/// The wall's kinematic body moves at 1 unit/s for 0.1 s, then back.
const RECOIL_TICKS: u8 = 6;
const RECOIL_Q16: i32 = ONE / 10;
/// `IntSwitch Facing`: 0 RIGHT, 1 UP, 2 LEFT, 3 DOWN, and the way each moves
/// the wall first (`Hit Up` only waits).
const WALL_RECOIL: [[i32; 2]; 4] = [[-1, 0], [0, 0], [1, 0], [0, 1]];

#[no_mangle]
pub static mut HK_SECRET_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_SECRET_BREAKS: u32 = 0;
#[no_mangle]
pub static mut HK_SECRET_REFUSED: u32 = 0;

/// One cooked secret: the `KIND_SECRET` object after its breakable.
#[derive(Clone, Copy)]
pub struct Spec<'a> {
    object: meta::Object<'a>,
}
impl<'a> Spec<'a> {
    /// The secret after the breakable at `index` in `region`, if that is one.
    pub fn after(region: meta::Region<'a>, index: usize) -> Option<Self> {
        let object = region.object(index + 1)?;
        (object.kind() == KIND_SECRET).then_some(Self { object })
    }
    pub fn family(&self) -> u16 {
        self.object.flags() & 7
    }
    pub fn wall(&self) -> bool {
        matches!(self.family(), FAMILY_WALL | FAMILY_WALL_TK2D)
    }
    fn facing(&self) -> usize {
        (self.object.flags() >> 3 & 3) as usize
    }
    pub fn hits(&self) -> u8 {
        (self.object.flags() >> 5 & 15) as u8
    }
    pub fn spell(&self) -> bool {
        self.object.flags() & 512 != 0
    }
    /// The `Hero Range` box the Knight's body must overlap for a hit to count.
    pub fn hero_range(&self) -> Option<[i32; 4]> {
        (self.object.flags() & 1024 != 0).then(|| self.object.bounds())
    }
    pub fn origin(&self) -> [i32; 2] {
        [self.object.extra(1), self.object.extra(2)]
    }
    /// (moving part, draw) pairs present in this region.
    pub fn moving(&self) -> impl Iterator<Item = (usize, usize)> + 'a {
        let mut words = self.object.indices(0);
        core::iter::from_fn(move || Some((words.next()? as usize, words.next()? as usize)))
    }
    fn parts(&self) -> usize {
        self.object.polygon_count() / 2
    }
    /// Plank `part`'s world quad after hit `stage` (1 or 2), in the cook's
    /// corner order.
    pub fn quad(&self, stage: u8, part: usize) -> Option<[[i32; 2]; 4]> {
        let parts = self.parts();
        if stage == 0 || part >= parts {
            return None;
        }
        let polygon = self
            .object
            .polygons()
            .nth((stage as usize - 1).min(1) * parts + part)?
            .ok()?;
        let mut quad = [[0; 2]; 4];
        let mut points = polygon.points().flatten();
        for corner in &mut quad {
            *corner = points.next()?;
        }
        Some(quad)
    }
    fn lockout(&self) -> u8 {
        if self.wall() {
            WALL_LOCKOUT
        } else {
            FLOOR_LOCKOUT
        }
    }
}

#[derive(Clone, Copy)]
struct Slot {
    taken: u8,
    lockout: u8,
    recoil: u8,
    /// The swing that last landed: `damages_enemy` sends one TAKE DAMAGE per
    /// target per attack.
    swing: u32,
}
const EMPTY: Slot = Slot {
    taken: 0,
    lockout: 0,
    recoil: 0,
    swing: u32::MAX,
};

pub enum Outcome {
    Refused,
    Hit(u8),
    Broken,
}

pub struct Hits {
    slots: [Slot; SLOTS],
}
fn slot(id: usize) -> Option<usize> {
    let local = id % BREAKABLES_PER_SCENE;
    (local >= BREAKABLES_PER_SCENE - SLOTS).then(|| BREAKABLES_PER_SCENE - 1 - local)
}
impl Hits {
    pub const fn new() -> Self {
        Self {
            slots: [EMPTY; SLOTS],
        }
    }
    pub fn reset(&mut self) {
        self.slots = [EMPTY; SLOTS];
    }
    /// One 60 Hz tick: lockouts and recoils run down.
    pub fn tick(&mut self) {
        for s in &mut self.slots {
            s.lockout = s.lockout.saturating_sub(1);
            s.recoil = s.recoil.saturating_sub(1);
        }
    }
    /// A nail swing that already overlaps the secret's hit polygon. `body` is
    /// the Knight's collider box, for a floor's `Hero Range`.
    pub fn nail(&mut self, id: usize, spec: &Spec, swing: u32, body: [i32; 4]) -> Outcome {
        let Some(index) = slot(id) else {
            return Outcome::Refused;
        };
        let s = &mut self.slots[index];
        if s.lockout != 0 || s.swing == swing {
            return Outcome::Refused;
        }
        if let Some(r) = spec.hero_range() {
            if !(body[0] <= r[2] && body[2] >= r[0] && body[1] <= r[3] && body[3] >= r[1]) {
                unsafe { HK_SECRET_REFUSED = HK_SECRET_REFUSED.wrapping_add(1) };
                return Outcome::Refused;
            }
        }
        s.swing = swing;
        s.taken = s.taken.saturating_add(1);
        unsafe { HK_SECRET_HITS = HK_SECRET_HITS.wrapping_add(1) };
        if s.taken >= spec.hits() {
            unsafe { HK_SECRET_BREAKS = HK_SECRET_BREAKS.wrapping_add(1) };
            return Outcome::Broken;
        }
        s.lockout = spec.lockout();
        // The killing hit's `IntCompare` fires before the recoil actions, so
        // only a hit that leaves the wall standing moves it.
        if spec.wall() {
            s.recoil = 2 * RECOIL_TICKS;
        }
        Outcome::Hit(s.taken)
    }
    /// Hits taken so far, which is the sag stage of a floor.
    pub fn taken(&self, id: usize) -> u8 {
        slot(id).map_or(0, |i| self.slots[i].taken)
    }
    /// The wall's recoil displacement this tick, in Q16 world units.
    pub fn recoil(&self, id: usize, spec: &Spec) -> Option<[i32; 2]> {
        let left = self.slots[slot(id)?].recoil;
        if left == 0 || !spec.wall() {
            return None;
        }
        let elapsed = 2 * RECOIL_TICKS - left;
        let out = if elapsed <= RECOIL_TICKS {
            elapsed
        } else {
            2 * RECOIL_TICKS - elapsed
        } as i32;
        let dir = WALL_RECOIL[spec.facing()];
        let d = RECOIL_Q16 * out / RECOIL_TICKS as i32;
        (dir != [0, 0]).then(|| [dir[0] * d, dir[1] * d])
    }
    /// True while this secret's moving art is drawn off its cooked place.
    pub fn displaced(&self, id: usize, spec: &Spec) -> bool {
        if spec.wall() {
            self.recoil(id, spec).is_some()
        } else {
            self.taken(id) != 0
        }
    }
}

static mut RANDOM: u32 = 0x5ec2_e75b;
fn random() -> u32 {
    unsafe {
        RANDOM = RANDOM.wrapping_mul(1664525).wrapping_add(1013904223);
        RANDOM >> 16
    }
}
/// A wall's `AudioPlayRandom`: breakable_wall_hit_1 or _2 at 1:1, pitch
/// U(0.85, 1.15). _1 is the resident door clip, which `great_door_hit` plays
/// with that same range; _2 is the scene bank's, and a scene without room
/// for it plays _1 every time.
#[cfg(not(test))]
pub fn wall_hit_sound() {
    let event = crate::scene_sfx::BREAKABLE_WALL_HIT_2;
    if random() & 1 != 0 && crate::scene_sfx::resident(event) {
        // 0.85..1.15 in Q12.
        crate::scene_sfx::play_pitched(event, 3482 + random() * (4710 - 3482 + 1) / 65536);
    } else {
        crate::audio::great_door_hit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_the_top_of_the_scene_range() {
        assert_eq!(slot(3 * 128 + 127), Some(0));
        assert_eq!(slot(3 * 128 + 120), Some(7));
        assert_eq!(slot(3 * 128 + 119), None);
        assert_eq!(slot(5), None);
    }
}
