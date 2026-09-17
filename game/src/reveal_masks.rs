//! Source reversible-mask FSM subset, independent of room residency.
//! Scene-ready substitutes for Pause's hero-position callback/1- or 2-second
//! timeout and inverse masks' 0.01-second Idle initialization tween. Inverse
//! owners start at zero opacity; ordinary owners retain authored full opacity.
//! Activated is bookkeeping only; it never latches renderer visibility.
use hk_sim::{Params, Player};
pub const MAX_CONTROLLERS: usize = 16;
const FULL: u32 = 65536;

#[derive(Clone, Copy)]
pub struct RevealMaskSpec {
    pub source_id: u32,
    pub trigger: &'static [[i32; 2]],
    pub fade_ticks: u16,
    pub initial_opacity: u8,
}
#[derive(Clone, Copy)]
pub struct RevealMaskBinding {
    pub controller: u8,
    pub draw: u16,
}
#[derive(Clone, Copy)]
struct Slot {
    source_id: u32,
    bounds: [i32; 4],
    alpha: u32,
    from: u32,
    target: u32,
    elapsed: u16,
    inside: bool,
    initial_opacity: u8,
}
const EMPTY: Slot = Slot {
    source_id: 0,
    bounds: [0; 4],
    alpha: 0,
    from: 0,
    target: 0,
    elapsed: 0,
    inside: false,
    initial_opacity: 0,
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
    pub fn scene_ready(&mut self, scene: usize, specs: &[RevealMaskSpec]) {
        assert!(specs.len() <= MAX_CONTROLLERS);
        if self.scene == Some(scene) {
            assert_eq!(self.count, specs.len());
            for (slot, spec) in self.slots[..self.count].iter().zip(specs) {
                assert_eq!(slot.source_id, spec.source_id);
                assert_eq!(slot.initial_opacity, spec.initial_opacity);
            }
            return;
        }
        *self = Self::new();
        self.scene = Some(scene);
        self.running = true;
        self.count = specs.len();
        for (index, spec) in specs.iter().enumerate() {
            assert!((3..=16).contains(&spec.trigger.len()));
            assert!(spec.initial_opacity == 0 || spec.initial_opacity == 128);
            assert!(specs[..index]
                .iter()
                .all(|prior| prior.source_id != spec.source_id));
            let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for &[x, y] in spec.trigger {
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x);
                bounds[3] = bounds[3].max(y);
            }
            self.slots[index] = Slot {
                source_id: spec.source_id,
                bounds,
                alpha: spec.initial_opacity as u32 * 512,
                from: spec.initial_opacity as u32 * 512,
                target: spec.initial_opacity as u32 * 512,
                initial_opacity: spec.initial_opacity,
                ..EMPTY
            };
        }
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
    /// the entire hero collider, not just the hero origin or trigger AABB.
    pub fn tick(&mut self, specs: &[RevealMaskSpec], player: &Player, params: Params) {
        if !self.running {
            return;
        }
        assert_eq!(specs.len(), self.count);
        let body = [
            player.x.saturating_sub(params.half_width),
            player.y.saturating_add(params.bottom),
            player.x.saturating_add(params.half_width),
            player.y.saturating_add(params.top),
        ];
        for (slot, spec) in self.slots[..self.count].iter_mut().zip(specs) {
            let b = slot.bounds;
            let inside = body[0] <= b[2]
                && body[2] >= b[0]
                && body[1] <= b[3]
                && body[3] >= b[1]
                && hk_sim::polygon_hits_box(spec.trigger, body);
            // Stay is ignored while fading toward the inside target. Exit changes target;
            // re-entry reverses from the current alpha without a discontinuity.
            if inside != slot.inside {
                slot.inside = inside;
                slot.from = slot.alpha;
                let outside = slot.initial_opacity as u32 * 512;
                slot.target = if inside { FULL - outside } else { outside };
                slot.elapsed = 0;
            }
            if slot.alpha == slot.target {
                continue;
            }
            if spec.fade_ticks == 0 {
                slot.alpha = slot.target;
                continue;
            }
            slot.elapsed = slot.elapsed.saturating_add(1).min(spec.fade_ticks);
            // Q16 linear interpolation. u32 product is at most65536*65535,
            // including a reversed fade; no float, heap, or wide division.
            let distance =
                slot.target.abs_diff(slot.from) * slot.elapsed as u32 / spec.fade_ticks as u32;
            slot.alpha = if slot.target >= slot.from {
                slot.from + distance
            } else {
                slot.from - distance
            };
        }
    }
    pub fn opacity(&self, controller: usize) -> u8 {
        assert!(controller < self.count);
        ((self.slots[controller].alpha * 128 + FULL / 2) / FULL) as u8
    }
    /// Apply after world::State::apply resets renderer state for this frame.
    /// Counts describe bound draws, allowing route checks without another scan.
    pub fn apply(&self, specs: &[RevealMaskSpec], bindings: impl Iterator<Item = RevealMaskBinding>) -> Applied {
        let mut result = Applied::default();
        for binding in bindings {
            // During a requested full reset the old region may render once
            // before activation. Restore this controller's authored Idle state.
            let alpha = if self.scene.is_none() {
                specs[binding.controller as usize].initial_opacity
            } else {
                self.opacity(binding.controller as usize)
            };
            crate::render::set_opacity(binding.draw as usize, alpha);
            match alpha {
                0 => result.hidden += 1,
                128 => result.visible += 1,
                _ => result.partial += 1,
            }
        }
        result
    }
}
