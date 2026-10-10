//! Authored no-charm Focus audio lifecycle, independent of SPU registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Off,
    Charge,
    Fade(u32),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Events {
    pub start: bool,
    pub stop: bool,
    pub heal: bool,
    pub gain: u8,
    pub volume_changed: bool,
}
pub struct State {
    playing: bool,
    gain: u8,
}
impl State {
    pub const fn new() -> Self {
        Self {
            playing: false,
            gain: 0,
        }
    }
    pub fn step(&mut self, phase: Phase, start: bool, heal: bool, fade_ticks: u32) -> Events {
        let old = self.gain;
        // Source FadeAudio starts at1; OnExit forces0 before Regain Control.
        let gain = match phase {
            Phase::Off => 0,
            Phase::Charge => 128,
            Phase::Fade(age) => ((fade_ticks.saturating_sub(age) * 128) / fade_ticks.max(1)) as u8,
        };
        let stop = self.playing && matches!(phase, Phase::Off);
        let start = start && !matches!(phase, Phase::Off);
        if stop {
            self.playing = false;
        }
        if start {
            self.playing = true;
        }
        self.gain = if self.playing { gain } else { 0 };
        Events {
            start,
            stop,
            heal,
            gain: self.gain,
            volume_changed: old != self.gain,
        }
    }
    pub fn gain(&self) -> u8 {
        self.gain
    }
}
