//! Source-derived Moss Charger (Mossy Control FSM, `Moss Charger` and `Moss
//! Charger 1`): a grass tuft that, when the Knight comes within its range, shows
//! up fourteen units to one side of him and charges across at fifteen units a
//! second until a wall or a gap stops it, then digs back into its tuft. Evidence:
//! docs/MOSS_CHARGER.md (FSM dump of level139 and level146, IL of RayCast2d,
//! Decelerate, tk2dBaseSprite.UpdateCollider).
//!
//! It is invincible (`HealthManager.invincible`) except after the twelfth blocked
//! hit, which bursts it out of its armour: it is flung, lands, gets up and runs
//! from the Knight, vulnerable, until it digs in. It has no collider of its own:
//! tk2d builds one from the frame showing, `SetCollider` switches it off while it
//! hides, and the renderer is off while it is hidden.
//!
//! `tick` is one nominal 60 Hz frame; `Decelerate` and `AccelerateVelocity` run
//! from a 50 Hz accumulator. The caller owns the body (the charge is a
//! gravity-free kinematic slide on the ground line; the burst and the run use
//! gravity), the rays it asks for through `needs`, hits, corpse and Geo.
use crate::ONE;

pub const CHARGE_SPEED: i32 = 15 * ONE;
/// `Emerge Speed` = `Current Charge Speed` * 0.25.
pub const EMERGE_SPEED: i32 = CHARGE_SPEED / 4;
/// `Emerge Right`/`Left`: `RandomFloat(14, 14)` from the Knight's x.
pub const APPEAR_DISTANCE: i32 = 14 * ONE;
/// `X Min`/`X Max` are the tuft's x less/plus half the range box's width, less/plus 2.
pub const RANGE_MARGIN: i32 = 2 * ONE;
/// `Emerge Pause`'s `WaitRandom` 0.5 to 1 s.
pub const EMERGE_PAUSE_TICKS: [u16; 2] = [30, 60];
/// `Appear`: six frames at 12 fps.
pub const APPEAR_TICKS: u16 = 30;
/// `Disappear` (twelve frames at 12 fps): trigger on frame 5, complete at the end.
pub const DISAPPEAR_TRIGGER_TICKS: u16 = 25;
pub const DISAPPEAR_TICKS: u16 = 60;
/// `Decelerate` 0.7 per fixed step in `Submerge` and its grass state.
pub const SUBMERGE_DECELERATION: i32 = 45_875;
/// `Submerge CD` Wait 0.35 s.
pub const SUBMERGE_CD_TICKS: u16 = 21;
/// `Line Loop` counts blocked hits; past 11 it bursts.
pub const BURST_HITS: u8 = 12;
/// `Burst`'s flings by the attack's cardinal (0 right, 1 up, 2 left, 3 down):
/// angle in degrees and speed.
pub const BURST_FLING: [(i32, i32); 4] = [(70, 18), (90, 20), (110, 18), (270, 10)];
/// `Burst` and `In Air` set gravity scale 1.5 (x 60).
pub const AIR_GRAVITY: i32 = 90 * ONE;
/// `Get Up`: four frames at 18 fps.
pub const GET_UP_TICKS: u16 = 14;
/// `Run L`/`Run R`: 0.5 per fixed step up to 10, for at most 1 s.
pub const RUN_ACCELERATION: i32 = 32_768;
pub const RUN_MAX: i32 = 10 * ONE;
pub const RUN_TICKS: u16 = 60;
/// `Escape` (fourteen frames at 12 fps): trigger on frame 5, complete at the end.
pub const ESCAPE_TRIGGER_TICKS: u16 = 25;
pub const ESCAPE_TICKS: u16 = 70;
/// Rays, in world offsets from the body (RayCast2d adds `fromPosition` to the
/// transform position and turns only the direction): `Charge`'s forward ray
/// (offset y -0.5, 5.5 long), its ground ray (3 long, offset -6.5 / +6.5 x and
/// -0.5 y, down), the run's forward ray (2) and ground ray (offset 3 ahead, 1.3 down).
pub const CHARGE_FORWARD: ([i32; 2], i32) = ([0, -ONE / 2], 11 * ONE / 2);
pub const CHARGE_GROUND: ([i32; 2], i32) = ([13 * ONE / 2, -ONE / 2], 3 * ONE);
pub const RUN_FORWARD: i32 = 2 * ONE;
pub const RUN_GROUND: ([i32; 2], i32) = ([3 * ONE, 0], 85_197);

/// Collider boxes tk2d builds from the sprite definitions, facing right (the
/// art's authored side); Q16 relative to the body.
pub const BIG: [i32; 4] = [-102_400, -125_952, 123_904, 12_288];
pub const BIG_LOW: [i32; 4] = [-102_400, -125_952, 123_904, -51_200];
pub const BIG_MID: [i32; 4] = [-102_400, -125_952, 123_904, 48_128];
pub const BIG_HIGH: [i32; 4] = [-102_400, -125_952, 123_904, 62_464];
pub const STUN: [i32; 4] = [-28_672, -43_008, 39_936, 22_528];
pub const RUN: [i32; 4] = [-28_672, -52_224, 39_936, 22_528];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `Hidden`: no collider, no renderer, watching the range box.
    Hidden,
    EmergePause,
    /// `Emerge`: placed, slow, `Appear` playing.
    Appear,
    Charge,
    /// `Submerge`: slowing, `Disappear` playing to its trigger frame.
    Submerge,
    /// `Submerge Grass effect`: slowing, until `Disappear` completes.
    SubmergeGrass,
    SubmergeCd,
    /// `Burst` -> `In Air`: flung with gravity until the floor.
    Air,
    GetUp,
    Run,
    /// `Dig Start`: `Escape` to its trigger frame.
    DigStart,
    /// `Dig`: `Escape` to its end.
    Dig,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    Appear,
    Charge,
    Disappear,
    Stun,
    GetUp,
    TurnRun,
    Escape,
}
impl Clip {
    pub const COUNT: usize = 7;
}
/// What a tick did that the caller must carry out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Restart the animation clock on this clip, from `from_frame`.
    pub play: Option<(Clip, u8)>,
    /// Move the body to this world position.
    pub teleport: Option<[i32; 2]>,
}
/// The rays and contacts the next tick reads; the caller computes only these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    pub charge_rays: bool,
    pub run_rays: bool,
    pub floor_contact: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    pub hero_x: i32,
    /// The Knight overlaps the `Attack Range` trigger box (fixed at the tuft).
    pub in_range: bool,
    /// `Charge`: the forward ray hit terrain; `Run`: the 2-long one.
    pub forward_hit: bool,
    /// `Charge`: the ground ray (`RayDown1`) hit; `Run`: the one ahead.
    pub ground_hit: bool,
    /// Bottom contact this frame (`CheckCollisionSide` DOWN).
    pub floor: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MossCharger {
    phase: Phase,
    timer: u16,
    start: [i32; 2],
    /// Half the range box's width, less the margin: x - start must stay within.
    reach: i32,
    /// +1 moving and facing right (the art's authored side).
    dir: i8,
    velocity: [i32; 2],
    position: [i32; 2],
    looper: u8,
    fixed_accumulator: u8,
    /// Frames the current clip has played, for the collider and the caller.
    clip_ticks: u16,
    clip: Clip,
    rng: u32,
    /// The burst's fling, for the caller's gravity body to take once.
    launch: Option<[i32; 2]>,
}
impl MossCharger {
    /// `range_width` is the `Attack Range` box's world width.
    pub fn new(start: [i32; 2], range_width: i32, seed: u32) -> Self {
        Self {
            phase: Phase::Hidden,
            timer: 0,
            start,
            reach: range_width / 2 - RANGE_MARGIN,
            dir: 1,
            velocity: [0; 2],
            position: start,
            looper: 0,
            fixed_accumulator: 0,
            clip_ticks: 0,
            clip: Clip::Appear,
            rng: seed,
            launch: None,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    /// The burst's fling velocity, once: the vertical part starts the caller's gravity body.
    pub fn take_launch(&mut self) -> Option<[i32; 2]> {
        self.launch.take()
    }
    /// The tuft the body returns to and the range box is fixed at.
    pub fn start(&self) -> [i32; 2] {
        self.start
    }
    pub fn dead(&self) -> bool {
        self.phase == Phase::Dead
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.velocity = [0; 2];
    }
    pub fn clip(&self) -> Clip {
        self.clip
    }
    pub fn clip_ticks(&self) -> u32 {
        self.clip_ticks as u32
    }
    /// The draw convention of the guest: +1 mirrors the cooked art, which faces right.
    pub fn facing(&self) -> i32 {
        -(self.dir as i32)
    }
    pub fn dir(&self) -> i32 {
        self.dir as i32
    }
    pub fn velocity(&self) -> [i32; 2] {
        self.velocity
    }
    pub fn position(&self) -> [i32; 2] {
        self.position
    }
    /// Gravity of the body, x 60 in Q16: zero except in the burst's fall and the run.
    pub fn gravity(&self) -> i32 {
        match self.phase {
            Phase::Air | Phase::GetUp | Phase::Run | Phase::DigStart | Phase::Dig => AIR_GRAVITY,
            _ => 0,
        }
    }
    /// `SetMeshRenderer` off while hidden.
    pub fn visible(&self) -> bool {
        !matches!(
            self.phase,
            Phase::Hidden | Phase::EmergePause | Phase::SubmergeCd | Phase::Dead
        )
    }
    /// `HealthManager.invincible`: set from the start, cleared by `In Air`, set
    /// again by `Submerge CD`.
    pub fn invincible(&self) -> bool {
        !matches!(
            self.phase,
            Phase::Air | Phase::GetUp | Phase::Run | Phase::DigStart
        )
    }
    /// A blocked hit is only a blocked hit while the collider exists.
    pub fn bodied(&self) -> bool {
        self.collider().is_some()
    }
    /// The collider tk2d has built for the frame showing, relative to the body.
    pub fn collider(&self) -> Option<[i32; 4]> {
        let frame = (self.clip_ticks / frame_ticks(self.clip)) as usize;
        let right = match self.phase {
            Phase::Appear | Phase::Charge => BIG,
            Phase::Submerge | Phase::SubmergeGrass => match frame {
                0 | 2 => BIG,
                1 => BIG_LOW,
                3 => BIG_MID,
                4 => BIG_HIGH,
                _ => return None,
            },
            Phase::Air | Phase::GetUp => STUN,
            Phase::Run => {
                if frame < 2 && self.clip == Clip::TurnRun {
                    STUN
                } else {
                    RUN
                }
            }
            Phase::DigStart | Phase::Dig if frame <= 5 => STUN,
            _ => return None,
        };
        Some(if self.dir >= 0 {
            right
        } else {
            [-right[2], right[1], -right[0], right[3]]
        })
    }
    pub fn needs(&self) -> Needs {
        Needs {
            charge_rays: self.phase == Phase::Charge,
            run_rays: self.phase == Phase::Run,
            floor_contact: self.phase == Phase::Air,
        }
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + ((self.random() as i64 * (high - low + 1) as i64) >> 24) as i32
    }
    fn play(&mut self, clip: Clip, from: u8, step: &mut Step) {
        self.clip = clip;
        self.clip_ticks = from as u16 * frame_ticks(clip);
        step.play = Some((clip, from));
    }
    /// `Decelerate`: each axis toward zero by `amount`, never past it.
    fn decelerate(&mut self, amount: i32) {
        for v in &mut self.velocity {
            *v = if *v > 0 {
                (*v - amount).max(0)
            } else {
                (*v + amount).min(0)
            };
        }
    }
    /// A blocked hit: `Line Loop` counts it and `State 2` re-enters the state it
    /// interrupted; the twelfth is `LOOP COMPLETE` and bursts. `cardinal` is the
    /// attack's direction (0 right, 1 up, 2 left, 3 down).
    pub fn blocked_hit(&mut self, cardinal: u8) -> Step {
        let mut step = Step::default();
        if !self.invincible() || !self.bodied() {
            return step;
        }
        self.looper = self.looper.saturating_add(1);
        if self.looper >= BURST_HITS {
            let (degrees, speed) = BURST_FLING[(cardinal & 3) as usize];
            let a = (degrees as i64 * 4096 / 360) as u16;
            self.velocity = [
                ((speed as i64 * psx_math::cos_q12(a) as i64 * ONE as i64) >> 12) as i32,
                ((speed as i64 * psx_math::sin_q12(a) as i64 * ONE as i64) >> 12) as i32,
            ];
            self.launch = Some(self.velocity);
            self.phase = Phase::Air;
            self.timer = 0;
            self.play(Clip::Stun, 0, &mut step);
            return step;
        }
        match self.phase {
            // `Emerge` again: the same place, speed and `Appear`.
            Phase::Appear => {
                self.timer = APPEAR_TICKS;
                self.velocity = [self.dir as i32 * EMERGE_SPEED, 0];
                self.play(Clip::Appear, 0, &mut step);
            }
            // `Charge` again: full speed, the clip restarted.
            Phase::Charge => {
                self.velocity = [self.dir as i32 * CHARGE_SPEED, 0];
                self.play(Clip::Charge, 0, &mut step);
            }
            // `Submerge` again from its start.
            Phase::Submerge | Phase::SubmergeGrass => {
                self.phase = Phase::Submerge;
                self.timer = DISAPPEAR_TRIGGER_TICKS;
                self.play(Clip::Disappear, 0, &mut step);
            }
            _ => {}
        }
        step
    }
    fn hide(&mut self, step: &mut Step) {
        self.phase = Phase::SubmergeCd;
        self.timer = SUBMERGE_CD_TICKS;
        self.velocity = [0; 2];
        self.position = self.start;
        step.teleport = Some(self.start);
    }
    fn begin_submerge(&mut self, step: &mut Step) {
        self.phase = Phase::Submerge;
        self.timer = DISAPPEAR_TRIGGER_TICKS;
        self.play(Clip::Disappear, 0, step);
    }
    /// `Direction` -> `Run L`/`Run R`: away from the Knight; `from` is the
    /// `TurnRun` frame to start on (2 from `Direction`, 0 on a change).
    fn run(&mut self, hero_x: i32, from: u8, step: &mut Step) {
        self.phase = Phase::Run;
        self.timer = RUN_TICKS;
        self.dir = if hero_x < self.position[0] { 1 } else { -1 };
        self.play(Clip::TurnRun, from, step);
    }
    /// One nominal 60 Hz frame. `x` is the body's x now; the caller moves it by
    /// `velocity` afterwards and writes any `teleport` first.
    pub fn tick(&mut self, senses: Senses) -> Step {
        let mut step = Step::default();
        if self.phase == Phase::Dead {
            return step;
        }
        self.fixed_accumulator += 50;
        let fixed = self.fixed_accumulator >= 60;
        if fixed {
            self.fixed_accumulator -= 60;
        }
        self.clip_ticks = self.clip_ticks.saturating_add(1);
        match self.phase {
            Phase::Hidden => {
                if senses.in_range {
                    self.phase = Phase::EmergePause;
                    self.timer = self
                        .range(EMERGE_PAUSE_TICKS[0] as i32, EMERGE_PAUSE_TICKS[1] as i32)
                        as u16;
                }
            }
            Phase::EmergePause => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    // `Hero Beyond?`: past the reach on either side, back to Hidden.
                    let off = senses.hero_x - self.start[0];
                    if off.abs() > self.reach {
                        self.phase = Phase::Hidden;
                    } else {
                        // `Left or Right?`, then the other side if this one is out of reach.
                        let mut right = self.random() & 1 == 0;
                        let appear_right = senses.hero_x + APPEAR_DISTANCE;
                        let appear_left = senses.hero_x - APPEAR_DISTANCE;
                        if right && appear_right > self.start[0] + self.reach {
                            right = false;
                        } else if !right && appear_left < self.start[0] - self.reach {
                            right = true;
                        }
                        let x = if right { appear_right } else { appear_left };
                        // Emerging on the right means charging left.
                        self.dir = if right { -1 } else { 1 };
                        self.position = [x, self.start[1]];
                        step.teleport = Some(self.position);
                        self.velocity = [self.dir as i32 * EMERGE_SPEED, 0];
                        self.phase = Phase::Appear;
                        self.timer = APPEAR_TICKS;
                        self.play(Clip::Appear, 0, &mut step);
                    }
                }
            }
            Phase::Appear => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.phase = Phase::Charge;
                    self.velocity = [self.dir as i32 * CHARGE_SPEED, 0];
                    self.play(Clip::Charge, 0, &mut step);
                }
            }
            Phase::Charge => {
                // Either ray ends the charge: a wall within 5.5, or no ground at the probe.
                if senses.forward_hit || !senses.ground_hit {
                    self.begin_submerge(&mut step);
                }
            }
            Phase::Submerge => {
                if fixed {
                    self.decelerate(SUBMERGE_DECELERATION);
                }
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.phase = Phase::SubmergeGrass;
                    self.timer = DISAPPEAR_TICKS - DISAPPEAR_TRIGGER_TICKS;
                }
            }
            Phase::SubmergeGrass => {
                if fixed {
                    self.decelerate(SUBMERGE_DECELERATION);
                }
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.hide(&mut step);
                }
            }
            Phase::SubmergeCd => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.phase = Phase::Hidden;
                }
            }
            Phase::Air => {
                if senses.floor && self.clip_ticks > 1 {
                    // `Land`: the x velocity is zeroed; `Get Up` follows.
                    self.velocity[0] = 0;
                    self.phase = Phase::GetUp;
                    self.timer = GET_UP_TICKS;
                    self.play(Clip::GetUp, 0, &mut step);
                }
            }
            Phase::GetUp => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.run(senses.hero_x, 2, &mut step);
                }
            }
            Phase::Run => {
                if fixed {
                    let target = self.dir as i32 * RUN_MAX;
                    let v = &mut self.velocity[0];
                    *v = if self.dir > 0 {
                        (*v + RUN_ACCELERATION).min(target)
                    } else {
                        (*v - RUN_ACCELERATION).max(target)
                    };
                }
                // `CheckTargetDirection`: the Knight on the side it runs toward turns it.
                let hero_right = senses.hero_x > self.position[0];
                if (self.dir > 0) == hero_right && senses.hero_x != self.position[0] {
                    self.run(senses.hero_x, 0, &mut step);
                    return step;
                }
                self.timer = self.timer.saturating_sub(1);
                if senses.forward_hit || !senses.ground_hit || self.timer == 0 {
                    // `On Ground?` -> `Dig Start`.
                    self.phase = Phase::DigStart;
                    self.timer = ESCAPE_TRIGGER_TICKS;
                    self.velocity[0] = (self.velocity[0] as i64 * 6 / 10) as i32;
                    self.play(Clip::Escape, 0, &mut step);
                }
            }
            Phase::DigStart => {
                if fixed {
                    self.decelerate(26_214);
                }
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.phase = Phase::Dig;
                    self.timer = ESCAPE_TICKS - ESCAPE_TRIGGER_TICKS;
                }
            }
            Phase::Dig => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.hide(&mut step);
                }
            }
            Phase::Dead => {}
        }
        step
    }
    /// The caller tells the sim where the body ended up after moving it.
    pub fn moved_to(&mut self, position: [i32; 2]) {
        self.position = position;
    }
}
/// Ticks one frame of a clip lasts at its authored rate.
const fn frame_ticks(clip: Clip) -> u16 {
    match clip {
        Clip::Charge => 4,
        Clip::GetUp => 3,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: [i32; 2] = [100 * ONE, 10 * ONE];
    const WIDTH: i32 = 33 * ONE;
    const CLEAR: Senses = Senses {
        hero_x: 0,
        in_range: false,
        forward_hit: false,
        ground_hit: true,
        floor: false,
    };

    fn near(hero_x: i32) -> Senses {
        Senses {
            hero_x,
            in_range: true,
            ..CLEAR
        }
    }
    fn run_until(c: &mut MossCharger, phase: Phase, senses: Senses, limit: u32) -> u32 {
        for n in 0..limit {
            if c.phase() == phase {
                return n;
            }
            let s = c.tick(senses);
            if let Some(p) = s.teleport {
                c.moved_to(p);
            }
        }
        panic!("never reached {phase:?}, at {:?}", c.phase());
    }

    #[test]
    fn hides_until_the_knight_is_in_range_then_appears_fourteen_units_to_a_side() {
        let mut c = MossCharger::new(START, WIDTH, 5);
        for _ in 0..200 {
            c.tick(Senses {
                hero_x: START[0],
                ..CLEAR
            });
        }
        assert_eq!(
            (c.phase(), c.collider(), c.visible()),
            (Phase::Hidden, None, false)
        );
        let hero = START[0] + 3 * ONE;
        let mut appeared = None;
        for _ in 0..200 {
            let s = c.tick(near(hero));
            if let Some(p) = s.teleport {
                appeared = Some(p);
                break;
            }
        }
        let p = appeared.expect("it appears after the emerge pause");
        assert_eq!(p[1], START[1]);
        assert!((p[0] - hero).abs() == APPEAR_DISTANCE, "{}", p[0]);
        assert_eq!(c.phase(), Phase::Appear);
        // It charges toward the Knight.
        assert_eq!(c.velocity()[0].signum(), if p[0] > hero { -1 } else { 1 });
        assert_eq!(c.velocity()[0].abs(), EMERGE_SPEED);
        assert!(c.invincible() && c.collider().is_some());
    }

    #[test]
    fn the_far_side_is_taken_when_the_near_one_is_out_of_reach() {
        for seed in 0..32 {
            let mut c = MossCharger::new(START, WIDTH, seed);
            // Hero at the far left of the reach: appearing left would be out of reach.
            let hero = START[0] - WIDTH / 2 + 3 * ONE;
            let mut dir = 0;
            for _ in 0..200 {
                if c.tick(near(hero)).teleport.is_some() {
                    dir = c.dir();
                    break;
                }
            }
            assert_eq!(
                dir, -1,
                "it must come from the right and charge left (seed {seed})"
            );
        }
    }

    #[test]
    fn a_charge_runs_at_fifteen_until_a_ray_ends_it_then_hides_back_at_the_tuft() {
        let mut c = MossCharger::new(START, WIDTH, 2);
        run_until(&mut c, Phase::Charge, near(START[0]), 400);
        assert_eq!(c.velocity()[0].abs(), CHARGE_SPEED);
        assert!(c.needs().charge_rays);
        c.tick(Senses {
            forward_hit: true,
            ..CLEAR
        });
        assert_eq!(c.phase(), Phase::Submerge);
        let slow = c.velocity()[0].abs();
        for _ in 0..40 {
            c.tick(CLEAR);
        }
        assert!(
            c.velocity()[0].abs() < slow,
            "0.7 per fixed step is gone in half a second"
        );
        run_until(&mut c, Phase::SubmergeCd, CLEAR, 100);
        assert_eq!(c.position(), START);
        assert!(!c.visible() && c.collider().is_none() && c.invincible());
        run_until(&mut c, Phase::Hidden, CLEAR, 40);
    }

    #[test]
    fn no_ground_under_the_probe_ends_the_charge_too() {
        let mut c = MossCharger::new(START, WIDTH, 3);
        run_until(&mut c, Phase::Charge, near(START[0]), 400);
        c.tick(Senses {
            ground_hit: false,
            ..CLEAR
        });
        assert_eq!(c.phase(), Phase::Submerge);
    }

    #[test]
    fn the_twelfth_blocked_hit_bursts_it_and_it_flees_the_knight_vulnerable() {
        let mut c = MossCharger::new(START, WIDTH, 4);
        run_until(&mut c, Phase::Charge, near(START[0]), 400);
        for n in 1..BURST_HITS {
            c.blocked_hit(0);
            assert_eq!(c.phase(), Phase::Charge, "hit {n}");
            assert!(c.invincible());
        }
        let step = c.blocked_hit(0);
        assert_eq!(c.phase(), Phase::Air);
        assert_eq!(step.play.map(|p| p.0), Some(Clip::Stun));
        assert!(!c.invincible(), "In Air clears the invincibility");
        assert!(
            c.velocity()[1] > 0 && c.velocity()[0] > 0,
            "a right hit flings it up and to the right"
        );
        let hero = c.position()[0] - 5 * ONE;
        // Not on the floor yet: stays airborne; then lands.
        for _ in 0..10 {
            c.tick(Senses {
                hero_x: hero,
                ..CLEAR
            });
        }
        assert_eq!(c.phase(), Phase::Air);
        c.tick(Senses {
            hero_x: hero,
            floor: true,
            ..CLEAR
        });
        assert_eq!((c.phase(), c.velocity()[0]), (Phase::GetUp, 0));
        run_until(
            &mut c,
            Phase::Run,
            Senses {
                hero_x: hero,
                ..CLEAR
            },
            40,
        );
        assert_eq!(c.dir(), 1, "the Knight is to its left: it runs right");
        for _ in 0..40 {
            c.tick(Senses {
                hero_x: hero,
                ..CLEAR
            });
        }
        assert_eq!(c.velocity()[0], RUN_MAX);
        // The Knight crossing over turns it round.
        let turned = c.tick(Senses {
            hero_x: c.position()[0] + ONE,
            ..CLEAR
        });
        assert_eq!(turned.play.map(|p| p.0), Some(Clip::TurnRun));
        assert_eq!(c.dir(), -1);
    }

    #[test]
    fn running_ends_in_a_dig_back_to_the_tuft() {
        let mut c = MossCharger::new(START, WIDTH, 6);
        run_until(&mut c, Phase::Charge, near(START[0]), 400);
        for _ in 0..BURST_HITS {
            c.blocked_hit(2);
        }
        let s = Senses {
            hero_x: c.position()[0] - 4 * ONE,
            floor: true,
            ..CLEAR
        };
        run_until(&mut c, Phase::Run, s, 200);
        c.tick(Senses {
            ground_hit: false,
            ..s
        });
        assert_eq!(c.phase(), Phase::DigStart);
        run_until(&mut c, Phase::SubmergeCd, s, 200);
        assert_eq!(c.position(), START);
    }

    #[test]
    fn death_is_final() {
        let mut c = MossCharger::new(START, WIDTH, 1);
        c.die();
        assert!(c.tick(near(START[0])).play.is_none() && c.dead());
    }
}
