//! Chests and pickups (host/pickups.py).
//!
//! A chest opens to a nail hit with the Knight inside its `Hero Region`,
//! flings its Geo and frees the item it holds. A shiny is taken with UP inside
//! its `Inspect Region`; a heart or vessel piece on touch. The `Key Giver`'s
//! City Crest waits for `falseKnightDefeated`. Opened chests and taken pickups
//! are the source's `PersistentBoolItem`s, kept in the world save as
//! `persist::Kind::Pickup`; the caller records them.
//!
//! The art is in the scene's own actor bank, appended to every view of the
//! scene, so this module links tables only. A view's first pickup frame is
//! `REGIONS`' base; the frame indices below are relative to it.
//!
//! Not reproduced: the Knight's `Hero Down` kneel and the item message's wait
//! for a button, the chest's `Open` clip and the chest's solid body, the item
//! flung out of a chest (it rests against the chest), and pickup sounds.
#[cfg(not(test))]
use hk_format::Room;

#[derive(Clone, Copy)]
pub struct Chest {
    pub scene: u16,
    pub local: u8,
    pub position: [i32; 2],
    pub closed: u16,
    pub opened: u16,
    pub body: [i32; 4],
    pub reach: [i32; 4],
    /// Small, medium and large coins `Spawn Items` flings.
    pub geo: [u16; 3],
    pub speed: [i32; 2],
    pub angle: [i32; 2],
    /// Projection at its own depth, x4096 (host FOCAL/(z-CAM_Z)).
    pub scale: i32,
    /// The lid's `Open` clip, played once before the opened frame; count 0
    /// in a view without room for it.
    pub open_first: u16,
    pub open_count: u16,
    pub open_fps: u32,
}
/// What a pickup gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grant { Charm(u8), Trinket(u8), CityKey, MaskShard, VesselFragment, RancidEgg }
#[derive(Clone, Copy)]
pub struct Pickup {
    pub scene: u16,
    /// Local id in the scene's save space: the chests first, then these.
    pub local: u8,
    pub position: [i32; 2],
    pub reach: [i32; 4],
    /// Heart and vessel pieces are taken on contact; a shiny on UP.
    pub touch: bool,
    pub grant: Grant,
    /// The local id of the chest it waits in, or NO_CHEST.
    pub chest: u8,
    pub after_false_knight: bool,
    /// A Heart Piece `Battle Control` hides until its arena is won
    /// (Crossroads_09's, after Brooding Mawlek).
    pub after_arena: bool,
    pub first: u16,
    pub count: u16,
    /// Frames per second, x256.
    pub fps: u32,
    pub scale: i32,
    /// `Fling On Start`: where the opened chest throws it from.
    pub fling: Option<[i32; 2]>,
    /// The message line; empty for heart and vessel pieces.
    pub name: &'static str,
}
pub const NO_CHEST: u8 = u8::MAX;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/pickups.rs"));

pub const MAX_CHESTS: usize = 2;
pub const MAX_PICKUPS: usize = 4;
#[derive(Clone, Copy)]
struct ChestState { index: u16, open: bool, age: u16 }
/// The throw out of a chest, in 60 Hz ticks, and how high it arcs.
const FLING_TICKS: u32 = 30;
const FLING_ARC: i32 = 3 * 65536;
/// `Shiny Control`: `Hero Down` waits 0.75 s, `Flash` 1.0 s, then `Hero Up`
/// plays Collect Normal 3 to its end.
const KNEEL_DOWN: u16 = 45;
const KNEEL_FLASH: u16 = 60;
#[derive(Clone, Copy)]
struct Kneel { slot: u8, ticks: u16 }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Held { Waiting, Ready, Taken }
#[derive(Clone, Copy)]
struct PickupState { index: u16, phase: Held, ticks: u32 }

#[no_mangle]
pub static mut HK_CHESTS_OPENED: u32 = 0;
#[no_mangle]
pub static mut HK_PICKUPS_TAKEN: u32 = 0;

pub struct World {
    scene: usize,
    chests: [Option<ChestState>; MAX_CHESTS],
    pickups: [Option<PickupState>; MAX_PICKUPS],
    up_held: bool,
    kneel: Option<Kneel>,
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
fn polygon_hits_box(polygon: &[[i32; 2]], b: [i32; 4]) -> bool {
    let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
    for p in polygon {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    overlap([lo[0], lo[1], hi[0], hi[1]], b)
}
impl World {
    pub const fn new() -> Self {
        Self { scene: usize::MAX, chests: [None; MAX_CHESTS], pickups: [None; MAX_PICKUPS], up_held: false, kneel: None }
    }
    /// A scene entered: chests shut and pickups waiting, except what the save
    /// says was opened or taken (`taken(local)`).
    #[inline(never)]
    pub fn enter_scene(&mut self, scene: usize, taken: &dyn Fn(usize) -> bool) {
        if self.scene == scene { return; }
        *self = Self { scene, ..Self::new() };
        // Tables are sorted by scene; the cook refuses more than fit.
        let (mut c, mut p) = (0, 0);
        for i in 0..CHESTS.len() {
            if usize::from(CHESTS[i].scene) == scene && c < MAX_CHESTS {
                let open = taken(usize::from(CHESTS[i].local));
                self.chests[c] = Some(ChestState { index: i as u16, open, age: if open { u16::MAX } else { 0 } });
                c += 1;
            }
        }
        for i in 0..PICKUPS.len() {
            if usize::from(PICKUPS[i].scene) == scene && p < MAX_PICKUPS {
                let phase = if taken(usize::from(PICKUPS[i].local)) { Held::Taken } else { Held::Waiting };
                self.pickups[p] = Some(PickupState { index: i as u16, phase, ticks: 0 });
                p += 1;
            }
        }
    }
    /// The scene starts over on the next `enter_scene`, even the same one.
    pub fn leave(&mut self) { self.scene = usize::MAX; }
    /// `Chest Control`'s `Range?`: the nail's polygon on a shut chest with the
    /// Knight in `Hero Region`. Returns the chest it opened.
    #[inline(never)]
    pub fn strike(&mut self, polygon: &[[i32; 2]], body: [i32; 4]) -> Option<&'static Chest> {
        if polygon.len() < 3 { return None; }
        for c in self.chests.iter_mut().flatten() {
            let spec = &CHESTS[c.index as usize];
            if !c.open && polygon_hits_box(polygon, spec.body) && overlap(spec.reach, body) {
                c.open = true;
                c.age = 0;
                unsafe { HK_CHESTS_OPENED = HK_CHESTS_OPENED.wrapping_add(1); }
                return Some(spec);
            }
        }
        None
    }
    /// One tick of pickups. `inspect` is UP held while the Knight is free to
    /// inspect; a shiny answers the press (`START INSPECT`), a piece the
    /// Knight's touch (`GET`). `taken` records each one taken.
    #[inline(never)]
    pub fn collect(&mut self, body: [i32; 4], inspect: bool, false_knight_defeated: bool, arena_won: bool,
                   mut taken: impl FnMut(&'static Pickup)) {
        let pressed = inspect && !self.up_held && self.kneel.is_none();
        self.up_held = inspect;
        for c in self.chests.iter_mut().flatten() { c.age = c.age.saturating_add(1); }
        let chests = self.chests;
        let mut kneel = None;
        for (slot, p) in self.pickups.iter_mut().enumerate().filter_map(|(i, p)| p.as_mut().map(|p| (i, p))) {
            let spec = &PICKUPS[p.index as usize];
            p.ticks = p.ticks.wrapping_add(1);
            if p.phase == Held::Waiting {
                // `Spawn Items` follows the lid's `Open` clip.
                let chest_open = spec.chest == NO_CHEST || chests.iter().flatten().any(|c| {
                    let chest = &CHESTS[c.index as usize];
                    chest.local == spec.chest && c.open
                        && u32::from(c.age) * chest.open_fps >= u32::from(chest.open_count) * 60 * 256
                });
                if chest_open && (!spec.after_false_knight || false_knight_defeated) && (!spec.after_arena || arena_won) {
                    p.phase = Held::Ready;
                    p.ticks = 0;
                }
            }
            // A flung item is taken once it has landed.
            let landed = spec.fling.is_none() || p.ticks >= FLING_TICKS;
            if p.phase == Held::Ready && landed && overlap(spec.reach, body) {
                if spec.touch {
                    p.phase = Held::Taken;
                    taken(spec);
                    unsafe { HK_PICKUPS_TAKEN = HK_PICKUPS_TAKEN.wrapping_add(1); }
                } else if pressed && kneel.is_none() {
                    // `START INSPECT`: the Knight kneels; the item is his
                    // at the end of `Hero Down` (kneel_tick).
                    kneel = Some(Kneel { slot: slot as u8, ticks: 0 });
                }
            }
        }
        if kneel.is_some() { self.kneel = kneel; }
    }
    /// One tick of the kneel. `hurt` is a hit this tick: during `Hero Down`
    /// it gives the item back (`HERO DAMAGED` returns to `Idle`), later it
    /// ends the kneel (`Finish`). Returns the shiny taken this tick.
    #[inline(never)]
    pub fn kneel_tick(&mut self, hurt: bool) -> Option<&'static Pickup> {
        let mut k = self.kneel?;
        k.ticks += 1;
        let p = self.pickups[usize::from(k.slot)].as_mut()?;
        let spec = &PICKUPS[p.index as usize];
        if hurt {
            self.kneel = None;
            return None;
        }
        let up = (KNEEL_CLIPS[2].0.len() as u32 * 60 * 256).div_ceil(KNEEL_CLIPS[2].1.max(1)) as u16;
        self.kneel = if k.ticks >= KNEEL_DOWN + KNEEL_FLASH + up { None } else { Some(k) };
        if k.ticks == KNEEL_DOWN {
            p.phase = Held::Taken;
            unsafe { HK_PICKUPS_TAKEN = HK_PICKUPS_TAKEN.wrapping_add(1); }
            return Some(spec);
        }
        None
    }
    /// Whether the kneel owns the Knight (no input, no movement).
    pub fn kneeling(&self) -> bool { self.kneel.is_some() }
    /// The kneel's frame, relative to the pickup frames: (block first, index).
    fn kneel_pose(&self) -> Option<u16> {
        let k = self.kneel?;
        let &(_, first, held) = KNEELS.iter().find(|r| usize::from(r.0) == self.scene)?;
        if held { return Some(first); }
        let (clip, age) = if k.ticks < KNEEL_DOWN { (0, k.ticks) }
            else if k.ticks < KNEEL_DOWN + KNEEL_FLASH { (1, k.ticks - KNEEL_DOWN) }
            else { (2, k.ticks - KNEEL_DOWN - KNEEL_FLASH) };
        let (frames, fps) = KNEEL_CLIPS[clip];
        let at = ((u32::from(age) * fps / (60 * 256)) as usize).min(frames.len() - 1);
        Some(first + u16::from(frames[at]))
    }
    /// Every frame to draw now, as (frame relative to the view's base, origin, scale).
    pub fn visible(&self, mut f: impl FnMut(u16, [i32; 2], i32)) {
        for c in self.chests.iter().flatten() {
            let spec = CHESTS[c.index as usize];
            let frame = if !c.open { spec.closed }
                else {
                    let at = u32::from(c.age) * spec.open_fps / (60 * 256);
                    if at < u32::from(spec.open_count) { spec.open_first + at as u16 } else { spec.opened }
                };
            f(frame, spec.position, spec.scale);
        }
        for p in self.pickups.iter().flatten().filter(|p| p.phase == Held::Ready) {
            let spec = PICKUPS[p.index as usize];
            let frame = (u64::from(p.ticks) * u64::from(spec.fps) / (60 * 256)) as u32 % u32::from(spec.count);
            let mut at = spec.position;
            if let Some(from) = spec.fling.filter(|_| p.ticks < FLING_TICKS) {
                // Out of the chest on a parabola: linear across, an arc up.
                let t = p.ticks as i32;
                let n = FLING_TICKS as i32;
                at = [from[0] + (spec.position[0] - from[0]) / n * t,
                      from[1] + (spec.position[1] - from[1]) / n * t + FLING_ARC / (n * n) * 4 * t * (n - t)];
            }
            f(spec.first + frame as u16, at, spec.scale);
        }
    }
    #[cfg(test)]
    fn phase(&self, n: usize) -> Held { self.pickups[n].unwrap().phase }
}
/// First pickup frame of a view's room, for the views of scenes that have any.
fn base(region: usize) -> Option<u16> {
    let at = BASES.partition_point(|r| usize::from(r.1) < region);
    BASES.get(at).filter(|r| usize::from(r.0) <= region).map(|r| r.2)
}
/// The Knight's body frame while he kneels to a shiny, as an absolute room
/// frame of the view `region`.
#[inline(never)]
pub fn kneel_frame(world: &World, region: usize) -> Option<usize> {
    Some(usize::from(base(region)? + world.kneel_pose()?))
}
/// The cooked draws of the drawn view that this module draws itself.
pub fn view_draws(view: usize) -> &'static [u16] {
    HIDE.iter().find(|h| usize::from(h.0) == view).map_or(&[], |h| h.1)
}
pub const MAX_DRAWS: usize = 4;
#[cfg(not(test))]
/// This frame's pickup frames (absolute room frame, origin), chosen where the
/// working set has room for them; a frame that does not fit is left out.
pub struct Draws { list: [(usize, [i32; 2], i32); MAX_DRAWS], len: usize }
#[cfg(not(test))]
impl World {
    #[inline(never)]
    pub fn append_needed(&self, room: &Room, region: usize, needed: &mut [u16], len: &mut usize) -> Draws {
        let mut out = Draws { list: [(0, [0; 2], 0); MAX_DRAWS], len: 0 };
        let Some(base) = base(region) else { return out };
        self.visible(|frame, origin, scale| {
            let frame = usize::from(base + frame);
            let (_, cols, rows) = room.frame_grid(frame);
            if out.len == MAX_DRAWS || *len + cols * rows > needed.len() { return; }
            crate::render::append_frame_keys(room, frame, needed, len);
            out.list[out.len] = (frame, origin, scale);
            out.len += 1;
        });
        out
    }
}
#[cfg(not(test))]
impl Draws {
    #[inline(never)]
    pub fn draw(&self, room: &Room, camera: (i32, i32)) -> u32 {
        let mut drawn = 0;
        for &(frame, origin, scale) in &self.list[..self.len] {
            let (_, cols, rows) = room.frame_grid(frame);
            for tile in 0..cols * rows {
                let (texture, b) = room.frame_tile(frame, tile);
                let coords = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
                let mut vertices = [(0i16, 0i16); 4];
                for (k, (x, y)) in coords.iter().enumerate() {
                    vertices[k] = ((160 + ((((origin[0] + x - camera.0) >> 8) * scale) >> 20)).clamp(-1024, 1023) as i16,
                                   (120 - ((((origin[1] + y - camera.1) >> 8) * scale) >> 20)).clamp(-1024, 1023) as i16);
                }
                crate::render::texture(texture, vertices, (128, 128, 128));
                drawn += 1;
            }
        }
        drawn
    }
}
#[cfg(not(test))]
pub fn apply(view: usize) {
    for &draw in view_draws(view) { crate::render::set_visible(draw as usize, false); }
}
#[cfg(test)]
mod tests {
    use super::*;
    const FAR: [i32; 4] = [-500 * 65536, -500 * 65536, -499 * 65536, -499 * 65536];
    #[test]
    fn a_chest_opens_to_a_hit_in_range_and_frees_what_it_holds() {
        let Some(spec) = PICKUPS.iter().find(|p| p.chest != NO_CHEST) else { return };
        let chest = *CHESTS.iter().find(|c| c.scene == spec.scene && c.local == spec.chest).unwrap();
        let poly = [[chest.body[0], chest.body[1]], [chest.body[2], chest.body[1]], [chest.body[2], chest.body[3]]];
        let mut w = World::new();
        w.enter_scene(spec.scene as usize, &|_| false);
        let mut got = None;
        w.collect(spec.reach, false, true, true, |p| got = Some(p.grant));
        w.collect(spec.reach, true, true, true, |p| got = Some(p.grant));
        assert!(!w.kneeling() && got.is_none(), "a shut chest's item cannot be taken");
        assert!(w.strike(&poly, chest.reach).is_some());
        assert!(w.strike(&poly, chest.reach).is_none(), "a chest opens once");
        // The lid's clip, then the item's flight, then UP kneels for it.
        let mut frames = std::vec::Vec::new();
        for _ in 0..120 { w.collect(spec.reach, false, true, true, |_| {}); w.visible(|f, _, _| frames.push(f)); }
        if chest.open_count > 0 { assert!(frames.contains(&chest.open_first), "the lid opens"); }
        w.collect(spec.reach, true, true, true, |p| got = Some(p.grant));
        assert!(w.kneeling() && got.is_none(), "UP starts the kneel, it does not take the item yet");
        let mut ticks = 0;
        while w.kneeling() { ticks += 1; if let Some(p) = w.kneel_tick(false) { got = Some(p.grant); } }
        assert_eq!(got, Some(spec.grant));
        assert!(ticks > KNEEL_DOWN + KNEEL_FLASH, "the kneel runs through Hero Up");
        let mut w = World::new();
        w.enter_scene(spec.scene as usize, &|l| l == chest.local as usize || l == spec.local as usize);
        let mut frames = std::vec::Vec::new();
        w.visible(|f, _, _| frames.push(f));
        assert_eq!(frames, [chest.opened], "a saved chest is open and its item gone");
    }
    #[test]
    fn the_key_givers_crest_waits_for_the_false_knight_and_a_press() {
        let Some(spec) = PICKUPS.iter().find(|p| p.after_false_knight) else { return };
        let mut w = World::new();
        w.enter_scene(spec.scene as usize, &|_| false);
        for _ in 0..3 { w.collect(spec.reach, true, false, true, |_| {}); w.collect(spec.reach, false, false, true, |_| {}); }
        assert!(!w.kneeling(), "nothing before falseKnightDefeated");
        w.collect(spec.reach, true, false, true, |_| {});
        w.collect(spec.reach, true, true, true, |_| {});
        assert!(!w.kneeling(), "UP held from before it appeared is not a press");
        w.collect(spec.reach, false, true, true, |_| {});
        w.collect(spec.reach, true, true, true, |_| {});
        assert!(w.kneeling());
        // A hit during Hero Down gives the crest back.
        assert!(w.kneel_tick(true).is_none());
        assert_eq!(w.phase(0), Held::Ready);
        w.collect(spec.reach, false, true, true, |_| {});
        w.collect(spec.reach, true, true, true, |_| {});
        let mut got = std::vec::Vec::new();
        while w.kneeling() { if let Some(p) = w.kneel_tick(false) { got.push(p.grant); } }
        assert_eq!(got, [Grant::CityKey]);
        assert_eq!(w.phase(0), Held::Taken);
    }
    #[test]
    fn the_arena_piece_waits_for_the_arena() {
        let Some(spec) = PICKUPS.iter().find(|p| p.after_arena) else { return };
        let mut w = World::new();
        w.enter_scene(spec.scene as usize, &|_| false);
        let mut got = None;
        w.collect(spec.reach, false, false, false, |p| got = Some(p.grant));
        assert_eq!(got, None, "hidden until Battle Control's End Wait");
        w.collect(spec.reach, false, false, true, |p| got = Some(p.grant));
        assert_eq!(got, Some(Grant::MaskShard));
    }
    #[test]
    fn a_piece_is_taken_by_touch() {
        let Some(spec) = PICKUPS.iter().find(|p| p.touch && !p.after_arena) else { return };
        let mut w = World::new();
        w.enter_scene(spec.scene as usize, &|_| false);
        let mut got = None;
        w.collect(FAR, false, false, true, |p| got = Some(p.grant));
        assert_eq!(got, None);
        w.collect(spec.reach, false, false, true, |p| got = Some(p.grant));
        assert_eq!(got, Some(spec.grant));
    }
    #[test]
    fn every_frame_and_region_is_in_range() {
        for w in BASES.windows(2) { assert!(w[0].0 <= w[0].1 && w[0].1 < w[1].0); }
        for c in CHESTS { assert!(c.closed != c.opened); }
        for p in PICKUPS { assert!(p.count > 0 && p.fps > 0); }
    }
}
