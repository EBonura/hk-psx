//! The hard landing, from `HeroController` (`FallCheck`, `ShouldHardLand`, `DoHardLanding`,
//! `Update`'s `hard_landing` branch, `FixedUpdate`'s `ResetMotion`).
//!
//! Fall for more than `BIG_FALL_TIME` (1.1 s) without the descent ever being interrupted and
//! the Knight lands in a crouch (`HARD_LANDING_TIME`, 0.8 s) instead of landing softly: he stops
//! dead, does not steer, jump, dash, strike, focus or interact, and the camera shakes. This module
//! owns the counting and the lock; the effects (the `HardLand` clip, dust, camera shake and sound)
//! belong to the caller and key off the event [`Player::hard_landing_tick`] returns.
//!
//! The lock is input gating: a locked Knight is stepped with no direction and no jump, so the
//! sweep keeps him on the floor where he landed. Callers gate the rest of their inputs on
//! [`Player::hard_landing`].
use crate::Player;

/// The two `HeroController` times, resampled to ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardLandingParams {
    /// Ticks of uninterrupted descent that arm the hard landing: the number of 60 Hz updates it takes
    /// `fallTimer` to exceed `BIG_FALL_TIME` (`floor(1.1 * 60) + 1`). Zero never hard-lands.
    pub fall_ticks: u16,
    /// Ticks the Knight is locked after the landing tick: `HARD_LANDING_TIME` (`floor(0.8 * 60)`).
    /// The update that finds the timer past it already hands control back.
    pub land_ticks: u16,
}

impl Player {
    /// One call per consumed 60 Hz tick, after the tick's `step`. `was_grounded` is `grounded`
    /// before that step; `allowed` is false while something other than falling owns the Knight (a
    /// gate entry: his state is `no_input` there, and `LeaveScene` reset the timers). Returns true on
    /// the tick the Knight hard-lands, once.
    ///
    /// `FallCheck` counts a tick of descent while the vertical velocity is negative and the Knight is
    /// off the floor; a wall slide holds the count at zero, and so does anything that ends the
    /// descent (a jump, a double jump, a bounce, a dash, which sets the velocity to zero).
    /// `NoHardLanding` surfaces and acid would clear `allowed` too; none is cooked yet (see
    /// docs/HARD_LANDING.md).
    pub fn hard_landing_tick(
        &mut self,
        rule: HardLandingParams,
        was_grounded: bool,
        allowed: bool,
    ) -> bool {
        self.hard_land = self.hard_land.saturating_sub(1);
        if !allowed {
            self.fall_ticks = 0;
            return false;
        }
        if self.grounded && !was_grounded {
            let hard = rule.fall_ticks != 0 && self.fall_ticks >= rule.fall_ticks;
            self.fall_ticks = 0;
            if hard {
                self.start_hard_landing(rule.land_ticks);
            }
            return hard;
        }
        self.fall_ticks = if !self.grounded && self.vy < 0 && !self.wall_sliding {
            self.fall_ticks.saturating_add(1)
        } else {
            0
        };
        false
    }

    /// The Knight is in `hard_landing`: no steering, no jump, dash, strike, focus or interact.
    pub fn hard_landing(&self) -> bool {
        self.hard_land > 0
    }

    /// Locks the Knight for `ticks` as `DoHardLanding` does (a scripted landing, such as King's Pass's
    /// opening crash, picks its own length). A held or queued jump from the fall must not fire when
    /// control returns (`ResetInput`, `CancelAttack`).
    pub fn start_hard_landing(&mut self, ticks: u16) {
        self.hard_land = ticks;
        self.jumping = false;
        self.jump_queuing = false;
        self.dash_queuing = false;
    }

    /// Ticks since the lock began (zero on the landing tick), for the crouch clip; zero when not locked.
    /// `total` is the length the lock was started with.
    pub fn hard_landing_age(&self, total: u16) -> u32 {
        if self.hard_land == 0 {
            0
        } else {
            u32::from(total - self.hard_land)
        }
    }

    /// A hit, a death or a respawn takes the Knight out of `hard_landing` (his state becomes
    /// `no_input`, and `HeroController.Update` stops counting the timer).
    pub fn end_hard_landing(&mut self) {
        self.hard_land = 0;
        self.fall_ticks = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Params, ONE};

    const P: Params = Params {
        speed: 8 * ONE,
        jump: 16 * ONE,
        gravity: 47 * ONE,
        fall: 20 * ONE,
        hold_ticks: 12,
        min_ticks: 5,
        jump_queue_ticks: 2,
        half_width: ONE / 2,
        bottom: -ONE,
        top: ONE / 4,
        wallslide_speed: -8 * ONE,
        ..Params::ZERO
    };
    const FLOOR: [i32; 4] = [-100 * ONE, 0, 100 * ONE, 0];
    /// The floor and a wall at x = 3.
    const ROOM: [[i32; 4]; 2] = [FLOOR, [3 * ONE, 0, 3 * ONE, 400 * ONE]];

    /// Drops a Knight from `height` above the floor; returns the descent ticks counted at landing.
    fn drop(rule: HardLandingParams, height: i32) -> (Player, u32, bool) {
        let mut p = Player::spawn(0, height + ONE);
        let mut counted = 0;
        loop {
            let was = p.grounded;
            let before = p.fall_ticks;
            p.step(P, 0, false, 1, |_| FLOOR);
            let hard = p.hard_landing_tick(rule, was, true);
            if p.grounded {
                return (p, u32::from(before), hard);
            }
            counted += 1;
            assert!(counted < 2000, "never landed");
        }
    }

    #[test]
    fn a_fall_just_under_the_threshold_lands_soft_and_just_over_lands_hard() {
        // Probe how many descent ticks a 40 unit drop takes, then set the rule on either side of it.
        let probe = HardLandingParams {
            fall_ticks: 0,
            land_ticks: 48,
        };
        let (_, ticks, hard) = drop(probe, 40 * ONE);
        assert!(!hard, "a rule of zero never hard-lands");
        assert!(ticks > 20, "the probe fell for {ticks} ticks");
        let n = ticks as u16;
        let (p, _, hard) = drop(
            HardLandingParams {
                fall_ticks: n,
                land_ticks: 48,
            },
            40 * ONE,
        );
        assert!(hard, "exactly the threshold hard-lands");
        assert!(p.hard_landing());
        let (p, _, hard) = drop(
            HardLandingParams {
                fall_ticks: n + 1,
                land_ticks: 48,
            },
            40 * ONE,
        );
        assert!(!hard, "one tick short lands softly");
        assert!(!p.hard_landing());
    }

    #[test]
    fn the_lock_lasts_land_ticks_after_the_landing_tick_and_gates_exactly_that_long() {
        let rule = HardLandingParams {
            fall_ticks: 10,
            land_ticks: 48,
        };
        let (mut p, _, hard) = drop(rule, 40 * ONE);
        assert!(hard);
        assert_eq!(p.hard_landing_age(rule.land_ticks), 0);
        for tick in 1..=48 {
            assert!(p.hard_landing(), "locked on tick {tick}");
            // Pressing right and jump has no effect on a caller that gates on the lock, as the
            // game does: the Knight is stepped with neither.
            let x = p.x;
            p.step(P, 0, false, 1, |_| FLOOR);
            p.hard_landing_tick(rule, true, true);
            assert_eq!(p.x, x);
            assert!(p.grounded);
            assert_eq!(p.hard_landing_age(rule.land_ticks), (tick % 48) as u32);
        }
        assert!(!p.hard_landing(), "control returns on the next tick");
    }

    #[test]
    fn the_landing_cancels_a_held_jump_and_a_queued_press() {
        let rule = HardLandingParams {
            fall_ticks: 10,
            land_ticks: 48,
        };
        let mut p = Player::spawn(0, 40 * ONE);
        let mut hard = false;
        for _ in 0..400 {
            let was = p.grounded;
            // The player mashes jump all the way down.
            p.step(P, 0, true, 1, |_| FLOOR);
            hard |= p.hard_landing_tick(rule, was, true);
            if hard {
                break;
            }
        }
        assert!(hard);
        assert!(!p.jump_queuing && !p.jumping);
    }

    #[test]
    fn upward_velocity_a_wall_slide_or_a_gate_entry_restart_the_count() {
        let rule = HardLandingParams {
            fall_ticks: 30,
            land_ticks: 48,
        };
        let mut p = Player::spawn(0, 200 * ONE);
        for _ in 0..25 {
            let was = p.grounded;
            p.step(P, 0, false, 2, |i| ROOM[i]);
            p.hard_landing_tick(rule, was, true);
        }
        assert_eq!(p.fall_ticks, 25);
        // A bounce (a pogo, a mushroom): the velocity turns upward.
        p.vy = 5 * ONE;
        p.step(P, 0, false, 2, |i| ROOM[i]);
        p.hard_landing_tick(rule, false, true);
        assert_eq!(p.fall_ticks, 0, "an upward tick ends the descent");
        p.vy = 0;
        for _ in 0..25 {
            let was = p.grounded;
            p.step(P, 0, false, 2, |i| ROOM[i]);
            p.hard_landing_tick(rule, was, true);
        }
        assert_eq!(p.fall_ticks, 25);
        // Sliding down a wall holds the count at zero (the Claw is cooked in, but the flag is
        // what FallCheck reads).
        p.wall_sliding = true;
        p.hard_landing_tick(rule, false, true);
        assert_eq!(p.fall_ticks, 0);
        p.wall_sliding = false;
        p.step(P, 0, false, 2, |i| ROOM[i]);
        p.hard_landing_tick(rule, false, true);
        assert_eq!(p.fall_ticks, 1);
        // A gate entry's drop never counts and never lands hard.
        p.hard_landing_tick(rule, false, false);
        assert_eq!(p.fall_ticks, 0);
    }

    #[test]
    fn a_dash_in_the_fall_resets_it() {
        let rule = HardLandingParams {
            fall_ticks: 30,
            land_ticks: 48,
        };
        let dash = Params {
            dash_speed: 20 * ONE,
            dash_ticks: 5,
            dash_cooldown_ticks: 10,
            dash_queue_ticks: 4,
            ..P
        };
        let mut p = Player::spawn(0, 200 * ONE);
        p.has_dash = true;
        for _ in 0..20 {
            let was = p.grounded;
            p.step(dash, 0, false, 1, |_| FLOOR);
            p.hard_landing_tick(rule, was, true);
        }
        assert_eq!(p.fall_ticks, 20);
        p.dash_input(dash, true);
        for _ in 0..5 {
            let was = p.grounded;
            p.step(dash, 0, false, 1, |_| FLOOR);
            p.hard_landing_tick(rule, was, true);
        }
        assert_eq!(p.fall_ticks, 0, "the dash holds vy at zero");
    }

    #[test]
    fn a_scripted_landing_picks_its_own_length_and_ages_against_it() {
        let rule = HardLandingParams {
            fall_ticks: 10,
            land_ticks: 48,
        };
        let (mut p, _, hard) = drop(rule, 40 * ONE);
        assert!(hard);
        // The opening crash locks the Knight for far longer than an ordinary hard landing.
        p.start_hard_landing(265);
        for tick in 1..=265u32 {
            assert!(p.hard_landing(), "locked on tick {tick}");
            assert_eq!(p.hard_landing_age(265), tick - 1);
            p.step(P, 0, false, 1, |_| FLOOR);
            p.hard_landing_tick(rule, true, true);
        }
        assert!(!p.hard_landing());
    }

    #[test]
    fn ending_the_hard_landing_frees_the_knight_at_once() {
        let rule = HardLandingParams {
            fall_ticks: 10,
            land_ticks: 48,
        };
        let (mut p, _, hard) = drop(rule, 40 * ONE);
        assert!(hard && p.hard_landing());
        p.end_hard_landing();
        assert!(!p.hard_landing());
    }
}
