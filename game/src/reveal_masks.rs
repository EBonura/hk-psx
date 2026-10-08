//! Source reveal-mask FSM subset, independent of room residency.
//! Scene-ready substitutes for Pause's hero-position callback/1- or 2-second
//! timeout and inverse masks' 0.01-second Idle initialization tween. Inverse
//! owners start at zero opacity; ordinary owners retain authored full opacity.
//! Activated is bookkeeping only; it never latches renderer visibility.
//!
//! One-way owners are the authored secret masks: their definition declares no
//! COVER event and no Hero Leave, so the first hero entry uncovers the secret
//! and nothing puts it back. They latch here for the same reason.
//!
//! A driven owner has no trigger: a hidden wall's or cracked floor's break
//! fires it (`fire_driver`), and its scene loading with that secret already
//! broken seats it uncovered, or replays its fade for the one shape whose
//! source sends UNCOVER again on every load (`restore_driven`).
use hk_sim::{Params, Player};
pub const MAX_CONTROLLERS: usize = 16;
const FULL: u32 = 65536;

/// The shape the controller catalogue had while it linked. Nothing reads one
/// any more; it stays only so a `data/regions.rs` cooked before the catalogue
/// moved into the metadata bank still compiles, and goes with the next recook.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct RevealMaskSpec {
    pub source_id: u32,
    pub trigger: &'static [[i32; 2]],
    pub fade_ticks: u16,
    pub initial_opacity: u8,
    pub one_way: bool,
}
#[derive(Clone, Copy)]
pub struct RevealMaskBinding {
    pub controller: u8,
    pub draw: u16,
}
/// One controller as the admitted bank records it. The trigger polygon is not
/// here: the bank keeps it, `bounds` is the AABB it cooked for that polygon, and
/// the exact shape is only asked for when the hero's box has reached that AABB.
#[derive(Clone, Copy)]
pub struct RevealMask {
    pub source_id: u32,
    pub bounds: [i32; 4],
    pub fade_ticks: u16,
    pub initial_opacity: u8,
    pub one_way: bool,
    /// The bank flags: 4 chimes, 8 driven, 16 replays its fade on load.
    pub flags: u16,
    /// The driving secret's state within the scene, or -1.
    pub driver: i32,
    /// The save slot of a saved one-way owner, or -1.
    pub slot: i32,
}
pub const CHIMES: u16 = 4;
pub const DRIVEN: u16 = 8;
pub const REPLAY: u16 = 16;
#[derive(Clone, Copy)]
struct Slot {
    source_id: u32,
    bounds: [i32; 4],
    alpha: u32,
    from: u32,
    target: u32,
    elapsed: u16,
    fade_ticks: u16,
    /// For a one-way owner this is "has fired", and it never returns to false.
    inside: bool,
    one_way: bool,
    initial_opacity: u8,
    flags: u16,
    driver: i16,
    slot: i8,
}
const EMPTY: Slot = Slot {
    source_id: 0,
    bounds: [0; 4],
    alpha: 0,
    from: 0,
    target: 0,
    elapsed: 0,
    fade_ticks: 0,
    inside: false,
    one_way: false,
    initial_opacity: 0,
    flags: 0,
    driver: -1,
    slot: -1,
};

pub struct State {
    scene: Option<usize>,
    running: bool,
    count: usize,
    slots: [Slot; MAX_CONTROLLERS],
}
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Applied {
    pub hidden: u32,
    pub partial: u32,
    pub visible: u32,
}
impl State {
    pub const fn new() -> Self {
        Self {
            scene: None,
            running: false,
            count: 0,
            slots: [EMPTY; MAX_CONTROLLERS],
        }
    }
    /// Every region supplies the same ordered scene-wide controller catalogue.
    /// Rebinding a draw after a grid change must not restart its source tween.
    /// True when this call seated the scene afresh, which is when a save's
    /// uncovered secrets have to be put back (`restore_revealed`).
    pub fn scene_ready(&mut self, scene: usize, masks: impl Iterator<Item = RevealMask>) -> bool {
        if self.scene == Some(scene) {
            let mut seen = 0;
            for (slot, mask) in self.slots[..self.count].iter().zip(masks) {
                assert_eq!(slot.source_id, mask.source_id);
                assert_eq!(slot.initial_opacity, mask.initial_opacity);
                assert_eq!(slot.one_way, mask.one_way);
                seen += 1;
            }
            assert_eq!(self.count, seen);
            return false;
        }
        *self = Self::new();
        self.scene = Some(scene);
        self.running = true;
        for mask in masks {
            let index = self.count;
            assert!(index < MAX_CONTROLLERS);
            assert!(mask.initial_opacity == 0 || mask.initial_opacity == 128);
            // Every authored one-way reveal starts covered and clears for good.
            assert!(!mask.one_way || mask.initial_opacity == 128);
            assert!(self.slots[..index]
                .iter()
                .all(|prior| prior.source_id != mask.source_id));
            self.slots[index] = Slot {
                source_id: mask.source_id,
                bounds: mask.bounds,
                alpha: mask.initial_opacity as u32 * 512,
                from: mask.initial_opacity as u32 * 512,
                target: mask.initial_opacity as u32 * 512,
                fade_ticks: mask.fade_ticks,
                initial_opacity: mask.initial_opacity,
                one_way: mask.one_way,
                flags: mask.flags,
                driver: mask.driver.clamp(-1, 127) as i16,
                slot: mask.slot.clamp(-1, 15) as i8,
                ..EMPTY
            };
            self.count = index + 1;
        }
        true
    }
    /// The save slots of the saved one-way controllers that have fired: the
    /// source's `Activated` on each secret mask's `PersistentBoolItem`.
    pub fn revealed(&self) -> u16 {
        self.slots[..self.count].iter()
            .filter(|slot| slot.one_way && slot.inside && slot.slot >= 0)
            .fold(0, |mask, slot| mask | 1 << slot.slot)
    }
    /// Whether any controller in `fired` (a bit per controller) is authored to
    /// play the reveal chime.
    pub fn chimes(&self, fired: u16) -> bool {
        self.slots[..self.count].iter().enumerate()
            .any(|(index, slot)| fired & (1 << index) != 0 && slot.flags & CHIMES != 0)
    }
    /// Seat the saved one-way controllers whose slots are in `mask` as already
    /// uncovered, right after `scene_ready` seated their scene. A reversible
    /// owner is never restored: the source does not save one.
    pub fn restore_revealed(&mut self, mask: u16) {
        for slot in self.slots[..self.count].iter_mut() {
            if slot.one_way && slot.slot >= 0 && mask & (1 << slot.slot) != 0 {
                uncover(slot);
            }
        }
    }
    /// Seat the driven controllers whose secret is already broken: uncovered,
    /// or fading again for an owner the source replays on every load.
    #[inline(never)]
    pub fn restore_driven(&mut self, broken: &dyn Fn(usize) -> bool) {
        for slot in self.slots[..self.count].iter_mut() {
            if slot.flags & DRIVEN != 0 && slot.driver >= 0 && broken(slot.driver as usize) {
                if slot.flags & REPLAY != 0 { start(slot); } else { uncover(slot); }
            }
        }
    }
    /// A secret broke: fire every controller it drives. Returns them as a bit
    /// per controller.
    #[inline(never)]
    pub fn fire_driver(&mut self, driver: usize) -> u16 {
        let mut fired = 0;
        for (index, slot) in self.slots[..self.count].iter_mut().enumerate() {
            if slot.flags & DRIVEN != 0 && slot.driver as usize == driver && !slot.inside {
                start(slot);
                fired |= 1 << index;
            }
        }
        fired
    }
    /// Source global HERO LEAVE enters an actionless state, stopping its tween.
    pub fn leave_scene(&mut self) {
        self.running = false;
    }
    pub fn reset_scene(&mut self, scene: usize) {
        if self.scene == Some(scene) {
            *self = Self::new();
        }
    }
    /// Call once per unpaused simulation tick. The exact trigger polygon sees
    /// the entire hero collider, not just the hero origin or trigger AABB, so
    /// `reaches` answers that polygon test for one controller against the hero
    /// box. It is only ever called after the slot's own AABB has admitted the
    /// box, and a tick that touches no trigger never reads the bank's points.
    /// The one-way controllers that fired this tick, a bit each: the moment
    /// their `Activated` becomes worth saving and their chime plays.
    pub fn tick(&mut self, player: &Player, params: Params,
                mut reaches: impl FnMut(usize, [i32; 4]) -> bool) -> u16 {
        let mut fired = 0;
        if !self.running {
            return fired;
        }
        let body = [
            player.x.saturating_sub(params.half_width),
            player.y.saturating_add(params.bottom),
            player.x.saturating_add(params.half_width),
            player.y.saturating_add(params.top),
        ];
        for (index, slot) in self.slots[..self.count].iter_mut().enumerate() {
            // A one-way owner's source FSM leaves its Fade state only for the
            // sound branch, so once its trigger has fired no later answer can
            // change anything. This bool sits beside the alpha the loop already
            // touches, so a revealed secret costs one load per tick instead of
            // an AABB and a polygon walk, and a fully revealed one costs that
            // plus the equality below. That is what keeps the added controllers
            // off a frame that is stall-bound.
            // A driven owner has no trigger; only its secret's break moves it.
            if !(slot.one_way && slot.inside) && slot.flags & DRIVEN == 0 {
                let b = slot.bounds;
                let inside = body[0] <= b[2]
                    && body[2] >= b[0]
                    && body[1] <= b[3]
                    && body[3] >= b[1]
                    && reaches(index, body);
                // Stay is ignored while fading toward the inside target. Exit changes target;
                // re-entry reverses from the current alpha without a discontinuity.
                if inside != slot.inside {
                    slot.inside = inside;
                    if slot.one_way {
                        fired |= 1 << index;
                    }
                    slot.from = slot.alpha;
                    let outside = slot.initial_opacity as u32 * 512;
                    slot.target = if inside { FULL - outside } else { outside };
                    slot.elapsed = 0;
                }
            }
            if slot.alpha == slot.target {
                continue;
            }
            if slot.fade_ticks == 0 {
                slot.alpha = slot.target;
                continue;
            }
            slot.elapsed = slot.elapsed.saturating_add(1).min(slot.fade_ticks);
            // Q16 linear interpolation. u32 product is at most65536*65535,
            // including a reversed fade; no float, heap, or wide division.
            let distance =
                slot.target.abs_diff(slot.from) * slot.elapsed as u32 / slot.fade_ticks as u32;
            slot.alpha = if slot.target >= slot.from {
                slot.from + distance
            } else {
                slot.from - distance
            };
        }
        fired
    }
    pub fn opacity(&self, controller: usize) -> u8 {
        assert!(controller < self.count);
        ((self.slots[controller].alpha * 128 + FULL / 2) / FULL) as u8
    }
    /// Apply after world::State::apply resets renderer state for this frame.
    /// Counts describe bound draws, allowing route checks without another scan.
    /// `authored` gives a controller's Idle opacity from the admitted bank. It
    /// is read only while no scene is bound, so a settled frame never asks.
    pub fn apply(&self, authored: impl Fn(usize) -> u8, bindings: impl Iterator<Item = RevealMaskBinding>) -> Applied {
        let mut result = Applied::default();
        for binding in bindings {
            // During a requested full reset the old region may render once
            // before activation. Restore this controller's authored Idle state.
            let alpha = if self.scene.is_none() {
                authored(binding.controller as usize)
            } else {
                self.opacity(binding.controller as usize)
            };
            // A member the subtractive CLUT cannot fade (coloured, soft or
            // partly transparent art that fades with the black) takes the
            // controller's opacity as its gain: iTween multiplies the material
            // colour, so it darkens to nothing as the black lifts.
            if crate::render::black_mask(binding.draw as usize) {
                crate::render::set_opacity(binding.draw as usize, alpha);
            } else {
                crate::render::set_gain(binding.draw as usize, alpha);
            }
            match alpha {
                0 => result.hidden += 1,
                128 => result.visible += 1,
                _ => result.partial += 1,
            }
        }
        result
    }
}

/// Uncovered and settled, as a save or an already broken secret seats it.
fn uncover(slot: &mut Slot) {
    slot.inside = true;
    slot.alpha = 0;
    slot.from = 0;
    slot.target = 0;
    slot.elapsed = slot.fade_ticks;
}
/// The fade toward uncovered starts from wherever the owner is.
fn start(slot: &mut Slot) {
    slot.inside = true;
    slot.from = slot.alpha;
    slot.target = 0;
    slot.elapsed = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use hk_sim::{Params, Player};

    /// A square trigger around the origin, in the Q16 world units the cook emits.
    const ONE: i32 = hk_sim::ONE;
    const BOX: &[[i32; 2]] = &[[-ONE, -ONE], [ONE, -ONE], [ONE, ONE], [-ONE, ONE]];

    fn params() -> Params {
        Params { half_width: ONE / 4, bottom: -ONE / 2, top: ONE / 2, ..Params::ZERO }
    }

    fn at(x: i32) -> Player {
        Player::spawn(x, 0)
    }

    fn mask(one_way: bool) -> RevealMask {
        RevealMask {
            source_id: 1,
            bounds: [-ONE, -ONE, ONE, ONE],
            fade_ticks: 4,
            initial_opacity: 128,
            one_way,
            flags: 0,
            driver: -1,
            slot: if one_way { 0 } else { -1 },
        }
    }

    /// Stands in for the bank's polygon lookup, against this module's square.
    fn reaches(_: usize, body: [i32; 4]) -> bool {
        hk_sim::polygon_hits_box(BOX, body)
    }

    /// The authored secret mask: covered, then clear for good on first entry.
    /// The reversible neighbour is here to show the difference is only the exit.
    #[test]
    fn one_way_owner_never_recovers_and_reversible_one_does() {
        for one_way in [true, false] {
            let mut state = State::new();
            state.scene_ready(0, [mask(one_way)].into_iter());
            assert_eq!(state.opacity(0), 128);
            for _ in 0..4 {
                state.tick(&at(0), params(), reaches);
            }
            assert_eq!(state.opacity(0), 0, "entry uncovers either way");
            for _ in 0..4 {
                state.tick(&at(40 * ONE), params(), reaches);
            }
            assert_eq!(state.opacity(0), if one_way { 0 } else { 128 });
        }
    }

    /// Re-entering a latched owner must not restart the tween, and leaving and
    /// coming back must not either: the source Fade state is left only for the
    /// sound branch, so nothing in the definition can fade it back in.
    #[test]
    fn latched_owner_ignores_later_crossings() {
        let mut state = State::new();
        state.scene_ready(0, [mask(true)].into_iter());
        for _ in 0..4 {
            state.tick(&at(0), params(), reaches);
        }
        for pass in 0..3 {
            for _ in 0..6 {
                state.tick(&at(if pass % 2 == 0 { 40 * ONE } else { 0 }), params(), reaches);
            }
            assert_eq!(state.opacity(0), 0);
        }
    }

    /// The fade is the authored one, not an instant swap: a half-elapsed tween
    /// reads half opacity, which is what the subtractive CLUT draws.
    #[test]
    fn one_way_fade_is_gradual_and_completes_once() {
        let mut state = State::new();
        state.scene_ready(0, [mask(true)].into_iter());
        state.tick(&at(0), params(), reaches);
        assert_eq!(state.opacity(0), 96);
        state.tick(&at(0), params(), reaches);
        assert_eq!(state.opacity(0), 64);
        // Leaving mid-fade must not reverse it, unlike a reversible owner.
        state.tick(&at(40 * ONE), params(), reaches);
        state.tick(&at(40 * ONE), params(), reaches);
        assert_eq!(state.opacity(0), 0);
    }

    /// Scene-ready is the only thing that covers a secret again, which is what
    /// the missing save system means: a reload re-hides what the source keeps.
    #[test]
    fn reset_scene_restores_the_authored_cover() {
        let mut state = State::new();
        state.scene_ready(0, [mask(true)].into_iter());
        for _ in 0..4 {
            state.tick(&at(0), params(), reaches);
        }
        assert_eq!(state.opacity(0), 0);
        state.reset_scene(0);
        state.scene_ready(0, [mask(true)].into_iter());
        assert_eq!(state.opacity(0), 128);
    }

    /// A mask a secret uncovers ignores the hero and fires on its driver's
    /// break; a later load seats it uncovered, or fades it again when its
    /// source replays the fade on every load.
    #[test]
    fn driven_owner_fires_on_its_secret_and_restores_after_a_load() {
        let driven = |flags| RevealMask { flags: DRIVEN | flags, driver: 127, slot: -1, ..mask(true) };
        let mut state = State::new();
        state.scene_ready(0, [driven(0)].into_iter());
        for _ in 0..4 {
            assert_eq!(state.tick(&at(0), params(), reaches), 0, "the hero does not fire it");
        }
        assert_eq!(state.opacity(0), 128);
        assert_eq!(state.fire_driver(126), 0);
        assert_eq!(state.fire_driver(127), 1);
        for _ in 0..4 {
            state.tick(&at(0), params(), reaches);
        }
        assert_eq!(state.opacity(0), 0);
        assert_eq!(state.revealed(), 0, "a driven owner is saved by its secret, not by a slot");
        for (flags, settled) in [(0, 0), (REPLAY, 128)] {
            let mut state = State::new();
            state.scene_ready(0, [driven(flags)].into_iter());
            state.restore_driven(&|local| local == 127);
            assert_eq!(state.opacity(0), settled);
        }
    }

    /// Only a placement authored with `Play Sound` chimes, and a saved owner
    /// is recorded under its slot rather than its controller index.
    #[test]
    fn chime_and_save_slot_are_per_controller() {
        let silent = RevealMask { source_id: 2, slot: 3, ..mask(true) };
        let chiming = RevealMask { flags: CHIMES, slot: 1, ..mask(true) };
        let mut state = State::new();
        state.scene_ready(0, [silent, chiming].into_iter());
        let fired = state.tick(&at(0), params(), reaches);
        assert_eq!(fired, 3);
        assert!(state.chimes(fired));
        assert!(!state.chimes(1));
        assert_eq!(state.revealed(), 1 << 3 | 1 << 1);
        let mut reloaded = State::new();
        reloaded.scene_ready(0, [silent, chiming].into_iter());
        reloaded.restore_revealed(1 << 1);
        assert_eq!((reloaded.opacity(0), reloaded.opacity(1)), (128, 0));
    }

    /// A one-way owner must be authored covered; the cook refuses anything else
    /// and this is the guest-side half of that pin.
    #[test]
    #[should_panic]
    fn one_way_owner_cannot_start_uncovered() {
        State::new().scene_ready(0, [RevealMask { initial_opacity: 0, ..mask(true) }].into_iter());
    }
}
