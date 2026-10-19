//! The Knight's look clips while he stands.
//!
//! `HeroAnimationController.UpdateAnimation` plays `LookUp` while `lookingUpAnim` is set and
//! `LookDown` while `lookingDownAnim` is, in the idle state. Both flags come from
//! `HeroController.Update`: up (or down) held on a standing Knight for `LOOK_ANIM_DELAY` (0.25 s),
//! well before the camera follows at `LOOK_DELAY` (0.85 s). `PlayIdle` answers a released look
//! with `LookUpEnd` / `LookDownEnd`, played once, before `Idle` returns. The flags themselves are
//! the camera's look timer (camera.rs `look_anim`); this holds which clip is playing and for how long.
use crate::ability_art::{clip_ticks, LOOK_DOWN, LOOK_DOWN_END, LOOK_UP, LOOK_UP_END};

/// Ticks spent in LookUp and LookDown, and end clips started, for the route pins.
#[no_mangle]
pub static mut HK_LOOK_UP_TICKS: u32 = 0;
#[no_mangle]
pub static mut HK_LOOK_DOWN_TICKS: u32 = 0;
#[no_mangle]
pub static mut HK_LOOK_ENDS: u32 = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Off,
    Up,
    UpEnd,
    Down,
    DownEnd,
}

#[derive(Clone, Copy)]
pub struct LookPose {
    phase: Phase,
    age: u16,
}

impl LookPose {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Off,
            age: 0,
        }
    }
    /// One simulation tick. `anim` is `Camera::look_anim` (+1 up, -1 down, 0 neither) and
    /// `standing` says the Knight is idle on the ground with nothing else owning his body: any
    /// other state replaces the look clip outright, as it does in the animation controller.
    pub fn tick(&mut self, anim: i8, standing: bool) {
        if !standing {
            *self = Self::new();
            return;
        }
        let held = match anim {
            1 => Phase::Up,
            -1 => Phase::Down,
            _ => Phase::Off,
        };
        if held != Phase::Off {
            unsafe {
                if held == Phase::Up {
                    HK_LOOK_UP_TICKS = HK_LOOK_UP_TICKS.wrapping_add(1);
                } else {
                    HK_LOOK_DOWN_TICKS = HK_LOOK_DOWN_TICKS.wrapping_add(1);
                }
            }
            if self.phase == held {
                self.age = self.age.saturating_add(1);
            } else {
                *self = Self {
                    phase: held,
                    age: 0,
                };
            }
            return;
        }
        match self.phase {
            // Released: PlayIdle starts the end clip of the look that was playing.
            Phase::Up | Phase::Down => {
                unsafe {
                    HK_LOOK_ENDS = HK_LOOK_ENDS.wrapping_add(1);
                }
                *self = Self {
                    phase: if self.phase == Phase::Up {
                        Phase::UpEnd
                    } else {
                        Phase::DownEnd
                    },
                    age: 0,
                }
            }
            Phase::UpEnd | Phase::DownEnd => {
                self.age = self.age.saturating_add(1);
                let (clip, _) = self.pose().expect("an end phase has a clip");
                if u32::from(self.age) >= clip_ticks(clip) {
                    *self = Self::new();
                }
            }
            Phase::Off => {}
        }
    }
    /// The ability clip that owns the Knight's body and its age in ticks, if a look does.
    pub fn pose(&self) -> Option<(usize, u32)> {
        let clip = match self.phase {
            Phase::Off => return None,
            Phase::Up => LOOK_UP,
            Phase::UpEnd => LOOK_UP_END,
            Phase::Down => LOOK_DOWN,
            Phase::DownEnd => LOOK_DOWN_END,
        };
        Some((clip, u32::from(self.age)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_look_plays_then_its_end_clip_once_then_idle() {
        let mut look = LookPose::new();
        assert!(look.pose().is_none());
        look.tick(1, true);
        assert_eq!(look.pose(), Some((LOOK_UP, 0)));
        look.tick(1, true);
        assert_eq!(look.pose(), Some((LOOK_UP, 1)));
        look.tick(0, true);
        assert_eq!(look.pose(), Some((LOOK_UP_END, 0)));
        for _ in 0..clip_ticks(LOOK_UP_END) - 1 {
            look.tick(0, true);
            assert!(matches!(look.pose(), Some((LOOK_UP_END, _))));
        }
        look.tick(0, true);
        assert!(look.pose().is_none());
    }

    #[test]
    fn down_has_its_own_clips_and_a_switch_restarts_the_clip() {
        let mut look = LookPose::new();
        look.tick(-1, true);
        assert_eq!(look.pose(), Some((LOOK_DOWN, 0)));
        look.tick(1, true);
        assert_eq!(look.pose(), Some((LOOK_UP, 0)));
        look.tick(0, true);
        look.tick(-1, true);
        assert_eq!(look.pose(), Some((LOOK_DOWN, 0)));
        look.tick(0, true);
        assert_eq!(look.pose(), Some((LOOK_DOWN_END, 0)));
    }

    #[test]
    fn anything_but_standing_ends_the_look_without_an_end_clip() {
        let mut look = LookPose::new();
        look.tick(1, true);
        look.tick(1, false);
        assert!(look.pose().is_none());
        look.tick(-1, true);
        look.tick(0, true);
        look.tick(0, false);
        assert!(look.pose().is_none());
    }

    #[test]
    fn the_looped_clip_repeats_from_its_second_frame() {
        // LookUp: six frames at 12 fps, looping from frame 1 (tk2d LoopSection).
        let c = crate::ability_art::ABILITY_CLIPS[LOOK_UP];
        let first = crate::ability_art::frame_index(LOOK_UP, 0);
        assert_eq!(first, c.start);
        let ticks_per_frame = 60 / u32::from(c.fps);
        let mut seen_zero_again = false;
        for age in 0..(c.count as u32 * 3 * ticks_per_frame) {
            let f = crate::ability_art::frame_index(LOOK_UP, age);
            assert!(f >= c.start && f < c.start + c.count);
            if age >= c.count as u32 * ticks_per_frame && f == c.start {
                seen_zero_again = true;
            }
        }
        assert!(!seen_zero_again, "the loop must not return to frame 0");
    }
}
