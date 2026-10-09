//! RestBench subset of the source `Bench Control` FSM (host/benches.py): UP
//! inside the trigger starts the rest, the Knight slides to the seat and sits
//! (Sit, then Sit Idle), Rest Burst heals and sets the respawn marker and
//! saves, any movement/jump/attack input gets off (Get Off, then Idle) and
//! control returns half a second later. Map, charm prompt, sleep and tilting
//! benches are not modelled; tween and waits are 60 Hz tick counts.
use crate::world::{Bench, Region};

/// iTween 0.2 s move plus the 0.3 s wait before Rest Burst.
const REST_BURST_TICK: u16 = 30;
/// Rest Burst, Pause 0.1 s, Save Game: input is listened to from here.
const RESTING_TICK: u16 = 36;
const GET_OFF_CLIP_TICKS: u16 = 25; // five frames at 12 fps
const GET_OFF_TICKS: u16 = GET_OFF_CLIP_TICKS + 15 + 6 + 30; // move 0.25 s, Idle 0.1 s, regain 0.5 s
const SEAT_SLIDE_TICKS: u16 = 12; // iTween 0.2 s

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Sitting(u16),
    GettingOff(u16),
}
pub struct State {
    phase: Phase,
    seat: [i32; 2],
    clip_base: u16,
    start_x: i32,
}
#[no_mangle]
pub static mut HK_BENCH_RESTS: u32 = 0;
#[derive(Clone, Copy, Default)]
pub struct Events {
    /// Rest Burst this tick: heal to full and set the respawn marker, then save.
    pub rest: bool,
    /// `Start Rest` this tick, which plays `bench_rest`.
    pub sat: bool,
}
impl State {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            seat: [0; 2],
            clip_base: 0,
            start_x: 0,
        }
    }
    pub fn locks_control(&self) -> bool {
        self.phase != Phase::Idle
    }
    pub fn seated(&self) -> bool {
        matches!(self.phase, Phase::Sitting(_))
    }
    pub fn seat(&self) -> [i32; 2] {
        self.seat
    }
    /// The Knight clip (Sit, Sit Idle, Get Off, Idle) and its age while the bench owns the animation.
    pub fn animation(&self) -> Option<(usize, u32)> {
        match self.phase {
            Phase::Idle => None,
            Phase::Sitting(t) => Some(if t < 18 {
                (self.clip_base as usize, t as u32)
            } else {
                (self.clip_base as usize + 1, (t - 18) as u32)
            }),
            Phase::GettingOff(t) => Some(if t < GET_OFF_CLIP_TICKS {
                (self.clip_base as usize + 2, t as u32)
            } else {
                (0, (t - GET_OFF_CLIP_TICKS) as u32)
            }),
        }
    }
    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
    }
    /// One tick. `up_pressed` is the UP edge, `leave` any jump/attack/direction
    /// press; the hero body is the Knight's collision box.
    pub fn tick(
        &mut self,
        region: &Region,
        hero_body: [i32; 4],
        player: &mut hk_sim::Player,
        grounded: bool,
        can_control: bool,
        up_pressed: bool,
        leave: bool,
    ) -> Events {
        let mut events = Events::default();
        match self.phase {
            Phase::Idle => {
                if !(up_pressed && grounded && can_control) {
                    return events;
                }
                let bench = crate::world::benches(region).find(|b: &Bench| {
                    b.bounds[0] <= hero_body[2]
                        && b.bounds[2] >= hero_body[0]
                        && b.bounds[1] <= hero_body[3]
                        && b.bounds[3] >= hero_body[1]
                });
                if let Some(bench) = bench {
                    events.sat = true;
                    self.phase = Phase::Sitting(0);
                    self.seat = bench.seat;
                    self.clip_base = bench.clip_base;
                    self.start_x = player.x;
                }
            }
            Phase::Sitting(t) => {
                // Start Rest slides the Knight onto the seat; it stays put after.
                if t < SEAT_SLIDE_TICKS {
                    // seat - start fits i32 while positions stay inside +/-2^30.
                    player.x = self.start_x
                        + psx_math::int32::mul_div_i32(
                            self.seat[0] - self.start_x,
                            t as i32 + 1,
                            SEAT_SLIDE_TICKS as i32,
                        );
                }
                player.vy = 0;
                player.jumping = false;
                if t == REST_BURST_TICK {
                    events.rest = true;
                    unsafe {
                        HK_BENCH_RESTS = HK_BENCH_RESTS.saturating_add(1);
                    }
                }
                if t >= RESTING_TICK && leave {
                    self.phase = Phase::GettingOff(0);
                } else {
                    self.phase = Phase::Sitting(t.saturating_add(1));
                }
            }
            Phase::GettingOff(t) => {
                player.vy = 0;
                self.phase = if t + 1 >= GET_OFF_TICKS {
                    Phase::Idle
                } else {
                    Phase::GettingOff(t + 1)
                };
            }
        }
        events
    }
}
