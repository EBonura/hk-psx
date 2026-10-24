//! RestBench subset of the source `Bench Control` FSM (host/benches.py): UP
//! inside the trigger starts the rest, the Knight slides to the seat and sits
//! (Sit, then Sit Idle), Rest Burst heals and sets the respawn marker and
//! saves, any movement/jump/attack input gets off (Get Off, then Idle) and
//! control returns half a second later. Map, charm prompt, sleep and tilting
//! benches are not modelled; tween and waits are 60 Hz tick counts.
use crate::{
    ability_art,
    world::{Bench, Region},
};

/// iTween 0.2 s move plus the 0.3 s wait before Rest Burst.
const REST_BURST_TICK: u16 = 30;
/// Rest Burst, Pause 0.1 s, Save Game: input is listened to from here.
const RESTING_TICK: u16 = 36;
const GET_OFF_CLIP_TICKS: u16 = 25; // five frames at 12 fps
const GET_OFF_TICKS: u16 = GET_OFF_CLIP_TICKS + 15 + 6 + 30; // move 0.25 s, Idle 0.1 s, regain 0.5 s
const SEAT_SLIDE_TICKS: u16 = 12; // iTween 0.2 s
/// `Init Resting` after a death: `Sit Fall Asleep` for 1.2 s, then `Startle` plays `Wake To Sit`.
const ASLEEP_TICKS: u16 = 72;
/// The camera fades back in over this many ticks as the respawned Knight appears (`RESPAWN` sent to
/// the camera's fade FSM; the fade-in length is the camera's ordinary 0.33 s).
const FADE_IN_TICKS: u16 = 20;
/// `Wake Up Ground`'s 25 identical lying frames at 18 fps.
const LYING_TICKS: u32 = 25 * 60 / 18;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    /// A death respawn that waits for its scene to load and tick, to find the bench it woke on.
    Respawned,
    Asleep(u16),
    Waking(u16),
    /// A death respawn at the new-game spawn waiting for its first tick (`Wake Up Ground`).
    RespawnedGround,
    Lying(u16),
    Rising(u16),
    Sitting(u16),
    GettingOff(u16),
}
pub struct State {
    phase: Phase,
    /// The respawn found its bench: the Knight stays seated after waking, as after a rest. A card
    /// whose marker is not at a bench (the seeded test fixtures) wakes and then stands.
    at_bench: bool,
    seat: [i32; 2],
    clip_base: u16,
    start_x: i32,
}
#[no_mangle]
pub static mut HK_BENCH_RESTS: u32 = 0;
/// Death respawns that woke asleep on a bench, and ones that woke on the ground, for the route pins.
#[no_mangle]
pub static mut HK_BENCH_WAKES: u32 = 0;
#[no_mangle]
pub static mut HK_GROUND_WAKES: u32 = 0;
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
            at_bench: false,
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
            Phase::Idle
            | Phase::Respawned
            | Phase::Asleep(_)
            | Phase::Waking(_)
            | Phase::RespawnedGround
            | Phase::Lying(_)
            | Phase::Rising(_) => None,
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
    /// The Knight comes back from a death at his bench: asleep, then waking, with control locked.
    pub fn respawn(&mut self, at_bench: bool) {
        self.phase = if at_bench {
            Phase::Respawned
        } else {
            Phase::RespawnedGround
        };
    }
    /// The ability clip (`ability_art`) that owns the body while the Knight sleeps or wakes.
    pub fn ability_animation(&self) -> Option<(usize, u32)> {
        match self.phase {
            Phase::Asleep(t) => Some((ability_art::SIT_FALL_ASLEEP, u32::from(t))),
            Phase::Waking(t) => Some((ability_art::WAKE_TO_SIT, u32::from(t))),
            Phase::Lying(t) => Some((ability_art::WAKE_GROUND_LIE, u32::from(t))),
            Phase::Rising(t) => Some((ability_art::WAKE_GROUND_RISE, u32::from(t))),
            _ => None,
        }
    }
    /// Screen darkness while the respawned Knight fades in, 0 (none) to 255 (black).
    pub fn fade_shade(&self) -> u8 {
        match self.phase {
            Phase::Respawned | Phase::RespawnedGround => 255,
            Phase::Asleep(t) | Phase::Lying(t) if t < FADE_IN_TICKS => {
                (255 - u32::from(t) * 255 / u32::from(FADE_IN_TICKS)) as u8
            }
            _ => 0,
        }
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
            Phase::Respawned => {
                // The marker put the Knight on the seat: the bench is the one he stands in.
                let found = crate::world::benches(region).find(|b: &Bench| {
                    b.bounds[0] <= hero_body[2]
                        && b.bounds[2] >= hero_body[0]
                        && b.bounds[1] <= hero_body[3]
                        && b.bounds[3] >= hero_body[1]
                });
                self.at_bench = found.is_some();
                if let Some(bench) = found {
                    self.seat = bench.seat;
                    self.clip_base = bench.clip_base;
                }
                self.phase = Phase::Asleep(0);
                unsafe {
                    HK_BENCH_WAKES += 1;
                }
            }
            Phase::RespawnedGround => {
                self.phase = Phase::Lying(0);
                unsafe {
                    HK_GROUND_WAKES += 1;
                }
            }
            Phase::Lying(t) => {
                // The lying pose is one cooked frame held for the clip's first 25 frames.
                self.phase = if u32::from(t) + 1 >= LYING_TICKS {
                    Phase::Rising(0)
                } else {
                    Phase::Lying(t + 1)
                };
            }
            Phase::Rising(t) => {
                self.phase = if u32::from(t) + 1 >= ability_art::clip_ticks(ability_art::WAKE_GROUND_RISE) {
                    Phase::Idle
                } else {
                    Phase::Rising(t + 1)
                };
            }
            Phase::Asleep(t) => {
                player.vy = 0;
                player.jumping = false;
                self.phase = if t + 1 >= ASLEEP_TICKS {
                    Phase::Waking(0)
                } else {
                    Phase::Asleep(t + 1)
                };
            }
            Phase::Waking(t) => {
                player.vy = 0;
                player.jumping = false;
                // Seated and listening for input, as after the Rest Burst.
                self.phase = if t + 1 >= ability_art::clip_ticks(ability_art::WAKE_TO_SIT) as u16 {
                    if self.at_bench {
                        Phase::Sitting(RESTING_TICK + 1)
                    } else {
                        Phase::Idle
                    }
                } else {
                    Phase::Waking(t + 1)
                };
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
