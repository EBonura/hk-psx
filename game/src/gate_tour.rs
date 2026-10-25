//! Scripted gate tour for `gate-tour` builds: once gameplay starts, stand for
//! `DWELL` ticks after each entry walk, then take the next listed gate through
//! the ordinary gate path, so every load is a real scene-gate transition.
//! Pad input is ignored while the tour runs. Ordinary builds compile nothing.
#![allow(dead_code)]
#[cfg(feature = "gate-tour")]
mod imp {
    include!("gate_tour_list.rs");
    const DWELL: u32 = match option_env!("HK_TOUR_DWELL") {
        Some(s) => parse(s),
        None => 150,
    };
    const fn parse(s: &str) -> u32 {
        let b = s.as_bytes();
        let mut i = 0;
        let mut v = 0;
        while i < b.len() {
            v = v * 10 + (b[i] - b'0') as u32;
            i += 1;
        }
        v
    }
    #[no_mangle]
    pub static mut HK_TOUR_STEP: u32 = 0;
    /// Stop the tour after this many steps (a diagnosis run); 0 runs the whole list.
    const STOP: u32 = match option_env!("HK_TOUR_STOP") {
        Some(s) => parse(s),
        None => 0,
    };
    #[no_mangle]
    pub static mut HK_TOUR_MISSES: u32 = 0;
    #[no_mangle]
    pub static mut HK_TOUR_DONE: u32 = 0;
    static mut IDLE: u32 = 0;
    /// Ticks the Knight has stood in the scene since its entry finished.
    pub fn tick(entering: bool) {
        unsafe {
            if entering {
                IDLE = 0;
            } else {
                IDLE = IDLE.saturating_add(1);
            }
        }
    }
    pub fn pad(_bits: u16) -> u16 {
        0
    }
    /// Walk mode (`tour-walk`): units per second the Knight runs, as Q16 per 50 Hz tick.
    const WALK_Q16_PER_TICK: i32 = (8_3 * 65536 / 10) / 50;
    /// Ticks a walk may take before the tour gives the step up as missed.
    const WALK_LIMIT: u32 = 6000;
    static mut ARRIVED: bool = false;
    /// Walk the Knight straight from where the entry left him to the next listed
    /// gate at running speed, ignoring terrain, so the ordinary gate predictor
    /// sees a Knight that approaches (the tour otherwise stands still). The
    /// step is due when he is inside the gate's trigger box.
    #[cfg(feature = "tour-walk")]
    pub fn walk(player: &mut hk_sim::Player, scene: usize) {
        unsafe {
            if HK_TOUR_DONE != 0 || IDLE < 2 {
                return;
            }
            let step = HK_TOUR_STEP as usize;
            let Some(&(_, _, region)) = TOUR.get(step) else {
                return;
            };
            if !from_matches(step, scene) {
                return;
            }
            let to = crate::disc::scene_index(region as usize);
            let Some(g) = crate::world::gates(scene)
                .find(|g| g.target_scene == to && g.target_region == region as usize)
            else {
                return;
            };
            let b = g.bounds;
            let tx = player.x.clamp(b[0], b[2]);
            let ty = player.y.clamp(b[1], b[3]);
            if tx == player.x && ty == player.y {
                ARRIVED = true;
                return;
            }
            // Head for the box's centre; stop at its edge.
            let cx = (b[0] >> 1) + (b[2] >> 1);
            let cy = (b[1] >> 1) + (b[3] >> 1);
            let (dx, dy) = (cx - player.x, cy - player.y);
            let far = dx.abs().max(dy.abs()).max(1);
            let step_q = WALK_Q16_PER_TICK.min(far);
            // 32-bit only: scale the shorter axis by step/far in units of 1/16.
            let (fd, sd) = (far >> 12, step_q);
            if fd == 0 {
                player.x = cx;
                player.y = cy;
            } else {
                player.x += (dx >> 12) * sd / fd;
                player.y += (dy >> 12) * sd / fd;
            }
            player.vy = 0;
            if dx != 0 {
                player.facing = dx.signum();
            }
        }
    }
    /// The list names scenes by the manifest order of the day it was written; the
    /// manifest is sorted by resident size, which moves. The target regions are
    /// stable, so a step starts in the scene the previous step's region is in
    /// (the first step starts wherever the new game does).
    fn from_matches(step: usize, scene: usize) -> bool {
        step == 0 || crate::disc::scene_index(TOUR[step - 1].2 as usize) == scene
    }
    /// The tour's next (target scene, target region), if it is due now.
    pub fn due(scene: usize) -> Option<(usize, usize)> {
        unsafe {
            #[cfg(feature = "tour-walk")]
            {
                if HK_TOUR_DONE != 0 || !(ARRIVED || IDLE >= WALK_LIMIT) {
                    return None;
                }
            }
            #[cfg(not(feature = "tour-walk"))]
            if HK_TOUR_DONE != 0 || IDLE < DWELL {
                return None;
            }
            let step = HK_TOUR_STEP as usize;
            if step >= TOUR.len() || (STOP != 0 && HK_TOUR_STEP >= STOP) {
                HK_TOUR_DONE = 1;
                return None;
            }
            let region = TOUR[step].2 as usize;
            if !from_matches(step, scene) {
                HK_TOUR_MISSES += 1;
                HK_TOUR_DONE = 2;
                return None;
            }
            Some((crate::disc::scene_index(region), region))
        }
    }
    pub fn taken() {
        unsafe {
            HK_TOUR_STEP += 1;
            IDLE = 0;
            ARRIVED = false;
        }
    }
    pub fn active() -> bool {
        unsafe { HK_TOUR_DONE == 0 }
    }
    /// The next listed destination after the current step, for an oracle prefetch.
    pub fn upcoming() -> Option<(usize, usize)> {
        let step = unsafe { HK_TOUR_STEP } as usize;
        TOUR.get(step)
            .map(|&(_, _, region)| (crate::disc::scene_index(region as usize), region as usize))
    }
}
#[cfg(feature = "gate-tour")]
pub use imp::{active, due, pad, taken, tick, upcoming};
#[cfg(feature = "tour-walk")]
pub use imp::walk;
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn tick(_: bool) {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn pad(bits: u16) -> u16 {
    bits
}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn due(_: usize) -> Option<(usize, usize)> {
    None
}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn taken() {}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn active() -> bool {
    false
}
#[cfg(not(feature = "gate-tour"))]
#[inline(always)]
pub fn upcoming() -> Option<(usize, usize)> {
    None
}
