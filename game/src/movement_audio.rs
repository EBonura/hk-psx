//! The landing and footstep half of HeroAudioController.PlaySound/StopSound. No motion or
//! animation is changed; whether a landing is hard is the simulation's verdict
//! (`hk_sim::Player::hard_landing_tick`: FallCheck, ShouldHardLand).
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
    run_left: u16,
    soft_left: u16,
    running: bool,
}
impl State {
    pub const fn new() -> Self {
        Self {
            run_left: 0,
            soft_left: 0,
            running: false,
        }
    }
    pub fn stop_run(&mut self) {
        self.run_left = 0;
        self.running = false;
    }
    /// One call per consumed 60Hz simulation tick. A landing is the tick `grounded` turns on;
    /// `hard` is the simulation's verdict for that tick (the Knight fell long enough and the
    /// floor allows it), which selects the hard landing sound over the soft one.
    pub fn tick(
        &mut self,
        was_grounded: bool,
        grounded: bool,
        running: bool,
        hard: bool,
        soft_ticks: u16,
        run_ticks: u16,
    ) -> Events {
        assert!(soft_ticks > 0 && run_ticks > 0);
        self.soft_left = self.soft_left.saturating_sub(1);
        self.run_left = self.run_left.saturating_sub(1);
        let landing = if !was_grounded && grounded {
            let kind = if hard { Landing::Hard } else { Landing::Soft };
            if kind == Landing::Soft {
                self.soft_left = soft_ticks;
            }
            kind
        } else {
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
    fn tick(s: &mut State, was: bool, ground: bool, run: bool) -> Events {
        s.tick(was, ground, run, false, 12, 122)
    }
    #[test]
    fn continuous_running_replays_whole_sequence_not_every_tick() {
        let mut s = State::new();
        let mut starts = Vec::new();
        for i in 0..400 {
            let e = tick(&mut s, true, true, true);
            if e.start_run {
                starts.push(i);
            }
            assert!(!e.stop_run);
        }
        assert_eq!(starts, [0, 122, 244, 366]);
        assert!(tick(&mut s, true, true, false).stop_run);
        assert!(!tick(&mut s, true, true, false).stop_run);
        assert!(tick(&mut s, true, true, true).start_run);
    }
    #[test]
    fn jump_stops_run_soft_landing_delays_restart_until_clip_ends() {
        let mut s = State::new();
        assert!(tick(&mut s, true, true, true).start_run);
        assert!(tick(&mut s, true, false, true).stop_run);
        for _ in 0..20 {
            assert_eq!(tick(&mut s, false, false, true).landing, Landing::None);
        }
        let e = tick(&mut s, false, true, true);
        assert_eq!(e.landing, Landing::Soft);
        assert!(!e.start_run);
        for _ in 0..11 {
            assert!(!tick(&mut s, true, true, true).start_run);
        }
        assert!(tick(&mut s, true, true, true).start_run);
    }
    #[test]
    fn the_simulations_verdict_picks_the_landing_and_only_on_the_landing_tick() {
        let mut s = State::new();
        assert_eq!(s.tick(false, true, false, true, 12, 122).landing, Landing::Hard);
        // Standing on the floor afterwards is not a landing, hard flag or not.
        assert_eq!(s.tick(true, true, false, true, 12, 122).landing, Landing::None);
        assert_eq!(s.tick(false, false, false, true, 12, 122).landing, Landing::None);
        // A hard landing does not start the soft-landing clip that holds footsteps back.
        let mut s = State::new();
        s.tick(false, true, false, true, 12, 122);
        assert!(s.tick(true, true, true, false, 12, 122).start_run);
    }
}
