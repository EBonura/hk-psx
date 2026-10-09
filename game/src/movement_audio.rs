//! Bounded audio-only subset of HeroController.FallCheck and
//! HeroAudioController.PlaySound/StopSound. No motion or animation is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    None,
    Soft,
    Hard,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Events {
    pub landing: Landing,
    pub start_run: bool,
    pub stop_run: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    falling_ticks: u16,
    run_left: u16,
    soft_left: u16,
    running: bool,
}
impl State {
    pub const fn new() -> Self {
        Self {
            falling_ticks: 0,
            run_left: 0,
            soft_left: 0,
            running: false,
        }
    }
    pub fn stop_run(&mut self) {
        self.run_left = 0;
        self.running = false;
    }
    /// One call per consumed 60Hz simulation tick. `falling` is the post-step
    /// negative vertical velocity; grounded contact selects the already accrued
    /// fall duration. Hard landings here cover ordinary dry terrain only; callers
    /// must exclude NoHardLanding surfaces. Wall-slide/acid FSMs are not ported.
    pub fn tick(
        &mut self,
        was_grounded: bool,
        grounded: bool,
        falling: bool,
        running: bool,
        hard_allowed: bool,
        hard_min_ticks: u16,
        soft_ticks: u16,
        run_ticks: u16,
    ) -> Events {
        assert!(hard_min_ticks > 0 && soft_ticks > 0 && run_ticks > 0);
        self.soft_left = self.soft_left.saturating_sub(1);
        self.run_left = self.run_left.saturating_sub(1);
        let landing = if !was_grounded && grounded {
            let kind = if hard_allowed && self.falling_ticks >= hard_min_ticks {
                Landing::Hard
            } else {
                Landing::Soft
            };
            self.falling_ticks = 0;
            if kind == Landing::Soft {
                self.soft_left = soft_ticks;
            }
            kind
        } else {
            self.falling_ticks = if !grounded && falling {
                self.falling_ticks.saturating_add(1)
            } else {
                0
            };
            Landing::None
        };
        let wants_run = running && grounded;
        let stop_run = self.running && !wants_run;
        if !wants_run {
            self.running = false;
            self.run_left = 0;
        }
        // Source footsteps are a complete nonlooping rhythm sequence. While
        // running, play again only once it ends; do not synthesize step cadence.
        // softLanding.isPlaying blocks a new start, not an existing sequence.
        let start_run = wants_run && self.run_left == 0 && self.soft_left == 0;
        if start_run {
            self.run_left = run_ticks;
            self.running = true;
        }
        Events {
            landing,
            start_run,
            stop_run,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tick(s: &mut State, was: bool, ground: bool, fall: bool, run: bool) -> Events {
        s.tick(was, ground, fall, run, true, 67, 12, 122)
    }
    #[test]
    fn continuous_running_replays_whole_sequence_not_every_tick() {
        let mut s = State::new();
        let mut starts = Vec::new();
        for i in 0..400 {
            let e = tick(&mut s, true, true, false, true);
            if e.start_run {
                starts.push(i);
            }
            assert!(!e.stop_run);
        }
        assert_eq!(starts, [0, 122, 244, 366]);
        assert!(tick(&mut s, true, true, false, false).stop_run);
        assert!(!tick(&mut s, true, true, false, false).stop_run);
        assert!(tick(&mut s, true, true, false, true).start_run);
    }
    #[test]
    fn jump_stops_run_soft_landing_delays_restart_until_clip_ends() {
        let mut s = State::new();
        assert!(tick(&mut s, true, true, false, true).start_run);
        assert!(tick(&mut s, true, false, false, true).stop_run);
        for _ in 0..20 {
            assert_eq!(
                tick(&mut s, false, false, true, true).landing,
                Landing::None
            );
        }
        let e = tick(&mut s, false, true, false, true);
        assert_eq!(e.landing, Landing::Soft);
        assert!(!e.start_run);
        for _ in 0..11 {
            assert!(!tick(&mut s, true, true, false, true).start_run);
        }
        assert!(tick(&mut s, true, true, false, true).start_run);
    }
    #[test]
    fn hard_threshold_counts_only_uninterrupted_descent_and_respects_surface() {
        for (duration, expected) in [
            (66, Landing::Soft),
            (67, Landing::Hard),
            (1000, Landing::Hard),
        ] {
            let mut s = State::new();
            for _ in 0..duration {
                tick(&mut s, false, false, true, false);
            }
            assert_eq!(tick(&mut s, false, true, false, false).landing, expected);
            assert_eq!(
                tick(&mut s, true, true, false, false).landing,
                Landing::None
            );
        }
        let mut s = State::new();
        for _ in 0..100 {
            tick(&mut s, false, false, true, false);
        }
        assert_eq!(
            s.tick(false, true, false, false, false, 67, 12, 122)
                .landing,
            Landing::Soft
        );
        let mut s = State::new();
        for _ in 0..100 {
            tick(&mut s, false, false, true, false);
        }
        tick(&mut s, false, false, false, false); // Upward recoil/pogo breaks descent.
        for _ in 0..20 {
            tick(&mut s, false, false, true, false);
        }
        assert_eq!(
            tick(&mut s, false, true, false, false).landing,
            Landing::Soft
        );
    }
    #[test]
    fn reset_discards_prior_scene_fall_and_sound_state() {
        let mut s = State::new();
        for _ in 0..100 {
            tick(&mut s, false, false, true, false);
        }
        s = State::new();
        assert_eq!(
            tick(&mut s, false, true, false, false).landing,
            Landing::Soft
        );
    }
    #[test]
    fn pause_stop_preserves_descent_and_restarts_sequence_on_resume() {
        let mut s = State::new();
        assert!(tick(&mut s, true, true, false, true).start_run);
        s.stop_run();
        assert!(tick(&mut s, true, true, false, true).start_run);
        for _ in 0..67 {
            tick(&mut s, false, false, true, false);
        }
        s.stop_run(); // The caller does not advance ticks while paused.
        assert_eq!(
            tick(&mut s, false, true, false, false).landing,
            Landing::Hard
        );
    }
}
