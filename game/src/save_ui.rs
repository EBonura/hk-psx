//! The bench Save prompt and what follows it: the notice while the card is
//! written, the result, and the questions a card can ask (format it?).
//!
//! A card write holds the CPU for longer than a video period (every frame is a
//! transaction plus the card's commit time), so the frame the player is looking
//! at is the last one drawn until it ends. The notice therefore has to be on
//! screen before the write starts: `Saving` counts the frames presented with it
//! up and only then lets the main loop begin. A failure is never silent: it
//! stays up, with the card's fault, until the player closes it.
//!
//! Pure state, no drawing and no card: `dialogue.rs` draws it and `main.rs`
//! owns the write, so the transitions are covered by tests/save_ui_runtime.rs.
use crate::save::Fault;

/// Presented frames the "Saving" notice must have been up before the write
/// begins. The display flip is deferred a frame, so one would not be enough.
pub const NOTICE_FRAMES: u8 = 3;
/// Ticks "Game saved" stays up (the source's 50 Hz simulation).
pub const SAVED_TICKS: u8 = 75;

/// `Ui::code`, for the replay watches and a console photograph.
#[no_mangle]
pub static mut HK_SAVE_UI: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ui {
    Idle,
    /// The seated question. Row 0 is Yes, row 1 is No.
    Ask(u8),
    /// The card is not formatted: format it and save? Row 0 is Yes.
    Format(u8),
    /// A write is owed or running. `frames` is the presented frames so far
    /// with the notice up; `format` says the card is being formatted too.
    Saving { frames: u8, format: bool },
    /// The write finished; the ticks left on the confirmation.
    Saved(u8),
    /// The write did not. Stays until closed.
    Failed(Fault),
}

/// This tick's fresh button presses, as the sampler reports them.
#[derive(Clone, Copy, Default)]
pub struct Edges {
    pub up: bool,
    pub down: bool,
    pub cross: bool,
    pub circle: bool,
}

impl Ui {
    /// The Knight is held and the pad belongs to the notice.
    pub fn modal(self) -> bool {
        !matches!(self, Ui::Idle | Ui::Saved(_))
    }
    /// A question or failure that reads the pad (a write in progress does not).
    pub fn reads_pad(self) -> bool {
        matches!(self, Ui::Ask(_) | Ui::Format(_) | Ui::Failed(_))
    }
    /// Open the seated question.
    pub fn ask(&mut self) {
        *self = Ui::Ask(0);
        self.publish();
    }
    fn publish(&self) {
        unsafe { HK_SAVE_UI = self.code() }
    }
    /// One tick of input. Returns `Some(format)` when the player has agreed to
    /// a write, which the caller then queues.
    pub fn step(&mut self, e: Edges) -> Option<bool> {
        match *self {
            Ui::Ask(row) | Ui::Format(row) => {
                let format = matches!(*self, Ui::Format(_));
                if e.up || e.down {
                    *self = if format {
                        Ui::Format(1 - row)
                    } else {
                        Ui::Ask(1 - row)
                    };
                } else if e.cross {
                    if row == 0 {
                        *self = Ui::Saving { frames: 0, format };
                        self.publish();
                        return Some(format);
                    }
                    *self = Ui::Idle;
                } else if e.circle {
                    *self = Ui::Idle;
                }
            }
            Ui::Failed(_) if e.cross || e.circle => *self = Ui::Idle,
            Ui::Saved(left) => {
                *self = if left <= 1 { Ui::Idle } else { Ui::Saved(left - 1) };
            }
            _ => {}
        }
        self.publish();
        None
    }
    /// Once per presented frame while a write is owed: true when the notice has
    /// been on screen long enough for the write to start.
    pub fn armed(&mut self) -> bool {
        match self {
            Ui::Saving { frames, .. } => {
                *frames = frames.saturating_add(1);
                *frames > NOTICE_FRAMES
            }
            _ => false,
        }
    }
    /// The write's result. A card that needs formatting asks first; anything
    /// else that failed is shown.
    pub fn finish(&mut self, result: Result<(), Fault>) {
        *self = match result {
            Ok(()) => Ui::Saved(SAVED_TICKS),
            Err(Fault::Unformatted) => Ui::Format(0),
            Err(fault) => Ui::Failed(fault),
        };
        self.publish();
    }
    /// Telemetry code for the replay watches: 0 idle, 1 ask, 2 format, 3 saving,
    /// 4 saved, 5 failed.
    pub fn code(self) -> u32 {
        match self {
            Ui::Idle => 0,
            Ui::Ask(_) => 1,
            Ui::Format(_) => 2,
            Ui::Saving { .. } => 3,
            Ui::Saved(_) => 4,
            Ui::Failed(_) => 5,
        }
    }
}
