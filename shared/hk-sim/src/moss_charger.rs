//! Source-derived Moss Charger (`Mossy Control`, Greenpath's Mosscreep charger): a
//! grass tuft that, when the Knight enters its `Attack Range` box, comes up fourteen
//! units to one side of him and charges across at fifteen units a second until a
//! wall ahead or a gap below ends the charge, then sinks back into its tuft.
//! Evidence: FSM dump of level139 and level149 (`Mossy Control`, 33 states), the
//! tk2d library of the placement, IL of `Decelerate`, `AccelerateVelocity`,
//! `HealthManager.Invincible` and PlayMaker's `Fsm.GotoPreviousState` (docs/MOSS_CHARGER.md).
//!
//! `HealthManager.invincible` is set from the start, so a nail hit while it charges
//! is `BLOCKED HIT`, the global transition to `Line Loop`. `Line Loop` counts the hit
//! and its `State 2` runs `GotoPreviousState`, whose previous state is `Line Loop`
//! itself (PlayMaker sets it on exit), so one blocked hit loops there twelve times in
//! the same frame and leaves on `LOOP COMPLETE` (`Looper` > 11) for `Burst`: the first
//! tink flings it out of the grass. From `In Air` it is vulnerable (hp 15): it lands,
//! gets up and runs from the Knight for at most a second, then digs back in.
//!
//! It has no collider of its own: tk2d builds one from the frame showing
//! (`tk2dBaseSprite.UpdateCollider`), `SetCollider` switches it off while it hides,
//! and tk2d switches it back on at the next sprite change, which matters only for the
//! `Escape` clip that follows `Dig Start`'s `SetCollider(false)`.
//!
//! `tick` is one nominal 60 Hz frame; `Decelerate` and `AccelerateVelocity` run on the
//! 50 Hz fixed step (PlayMakerFixedUpdate) from an accumulator and once on entry. The
//! caller owns the body, the rays it asks for through `needs`, hits, corpse and Geo.
use crate::ONE;

pub const HEALTH: i16 = 15;
pub const CHARGE_SPEED: i32 = 15 * ONE;
/// `Emerge Speed` = `Current Charge Speed` * 0.25.
pub const EMERGE_SPEED: i32 = CHARGE_SPEED / 4;
/// `Emerge Right`/`Left`: `RandomFloat(14, 14)` from the Knight's x.
pub const APPEAR_DISTANCE: i32 = 14 * ONE;
/// `X Min`/`X Max`: the tuft's x less/plus half the range box's width, then 2 inside.
pub const RANGE_MARGIN: i32 = 2 * ONE;
/// `Emerge Pause`'s `WaitRandom` 0.5 to 1 s.
pub const EMERGE_PAUSE_TICKS: [u16; 2] = [30, 60];
/// `Appear`: six frames at 12 fps, to completion.
pub const APPEAR_TICKS: u16 = 30;
/// `Disappear` (twelve frames at 12 fps): its trigger frame 5, then the end.
pub const DISAPPEAR_TRIGGER_TICKS: u16 = 25;
pub const DISAPPEAR_TICKS: u16 = 60;
/// `Decelerate` 0.7 per fixed step in `Submerge` and `Submerge Grass effect`.
pub const SUBMERGE_DECELERATION: i32 = 45_875;
/// `Dig Start`'s `Decelerate` 0.4.
pub const DIG_DECELERATION: i32 = 26_214;
/// `Submerge CD` Wait 0.35 s.
pub const SUBMERGE_CD_TICKS: u16 = 21;
/// `Line Loop` leaves on `Looper` > 11: one blocked hit loops it there to 12 in its frame.
pub const LOOP_LIMIT: u8 = 11;
/// `Burst`'s flings by the attack's cardinal (0 right, 1 up, 2 left, 3 down):
/// `SetVelocityAsAngle` 70 at 18, 90 at 20, 110 at 18, 270 at 10, as Q16 velocities.
pub const BURST_FLING: [[i32; 2]; 4] = [[403_463, 1_108_507], [0, 1_310_720], [-403_463, 1_108_507], [0, -655_360]];
/// `Burst` and `In Air` set gravity scale 1.5; `Dig Start` sets it to 0.
pub const AIR_GRAVITY_SCALE_Q16: i32 = 3 * ONE / 2;
/// `Get Up`: four frames at 18 fps, to completion (13.3 ticks: the 14th).
pub const GET_UP_TICKS: u16 = 14;
/// `Run L`/`Run R`: `AccelerateVelocity` 0.5 per fixed step, clamped to 10; `Wait` 1 s.
pub const RUN_ACCELERATION: i32 = ONE / 2;
pub const RUN_MAX: i32 = 10 * ONE;
pub const RUN_TICKS: u16 = 60;
/// `Escape` (fourteen frames at 12 fps): its trigger frame 5, then the end.
pub const ESCAPE_TRIGGER_TICKS: u16 = 25;
pub const ESCAPE_TICKS: u16 = 70;
/// `Charge`'s two `RayCast2d` (space Self, repeatInterval 2, terrain only): the forward
/// ray from (0, -0.5) along `RayForward Direction`, 5.5 long, and the ground ray from
/// `RayDown X` (6.5 ahead, -0.5) straight down, 3 long. `fromPosition` is added to the
/// position as written; only the direction turns with the transform (identity here).
pub const CHARGE_FORWARD_ORIGIN: [i32; 2] = [0, -ONE / 2];
pub const CHARGE_FORWARD_LENGTH: i32 = 11 * ONE / 2;
pub const CHARGE_GROUND_ORIGIN: [i32; 2] = [13 * ONE / 2, -ONE / 2];
pub const CHARGE_GROUND_LENGTH: i32 = 3 * ONE;
pub const CHARGE_RAY_INTERVAL: u8 = 2;
/// `Run L`/`Run R`'s rays (repeatInterval 1, terrain and soft terrain): forward from the
/// position, 2 long, and down from 3 ahead, 1.3 long.
pub const RUN_FORWARD_LENGTH: i32 = 2 * ONE;
pub const RUN_GROUND_ORIGIN: [i32; 2] = [3 * ONE, 0];
pub const RUN_GROUND_LENGTH: i32 = 85_197;
/// `Dig Start`/`Dig`'s `RayCast2dV2` from `Dig Check` (on the body) straight down, 1 long.
pub const DIG_LENGTH: i32 = ONE;

/// Collider boxes tk2d builds from the sprite definitions, facing right (scale +1, the
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
    /// `Emerge Right`/`Emerge Left` found the side out of reach: `Pause` waits a frame
    /// and tries the other side (`right`: the side it tries next).
    Retry { right: bool },
    /// `Emerge`: placed, slow, `Appear` playing.
    Appear,
    Charge,
    /// `Submerge`: slowing, `Disappear` playing to its trigger frame.
    Submerge,
    /// `Submerge Grass effect`: slowing, until `Disappear` completes.
    SubmergeGrass,
    SubmergeCd,
    /// `Fly *`: flung this frame; `In Air` from the next.
    Fly,
    /// `In Air`: gravity 1.5 until a floor contact.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// `Emerge`'s one-shot (and the camera's `AverageShake`).
    Emerge,
    /// `Charge` and `Direction`: the looping AudioSource starts.
    LoopOn,
    /// `Submerge` and `On Ground?`: `AudioStop`, and `Submerge`'s one-shot.
    Submerge,
    LoopOff,
    /// `Burst`'s one-shot and `EnemyKillShake`.
    Burst,
    /// `Dig`'s `AudioPlayRandom`.
    Dig,
}
/// What a tick did that the caller must carry out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Restart the animation clock on this clip.
    pub play: Option<Clip>,
    /// Move the body to this world position (`SetPosition`).
    pub teleport: Option<[i32; 2]>,
    pub sound: Option<Sound>,
}
/// The rays and contacts the next tick reads; the caller computes only these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Needs {
    pub range: bool,
    pub charge_rays: bool,
    pub run_rays: bool,
    pub dig_ray: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Senses {
    pub hero_x: i32,
    /// The Knight overlaps the `Attack Range` trigger box (left at the tuft).
    pub in_range: bool,
    /// `Charge`: the forward ray hit terrain; `Run`: the 2-long one.
    pub forward_hit: bool,
    /// `Charge`: the ground ray (`RayDown1`); `Run`: `Ground Ahead`.
    pub ground_hit: bool,
    /// `Dig Check`'s ray found ground.
    pub dig_ground: bool,
    /// A bottom contact this frame (`CheckCollisionSide` DOWN).
    pub floor: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MossCharger {
    phase: Phase,
    timer: u16,
    start: [i32; 2],
    /// Half the range box's width less the margin: `X Min`/`X Max` about the tuft.
    reach: i32,
    /// +1 moving and facing right (scale +1, the art's authored side).
    dir: i8,
    velocity: [i32; 2],
    position: [i32; 2],
    fixed_accumulator: u8,
    ray_clock: u8,
    /// `RayDown1`, stored by the last ground cast.
    ray_down: bool,
    /// Frames the current clip has played.
    clip_ticks: u16,
    clip: Clip,
    rng: u32,
    /// `frame_box`, `invincible` and `visible` as of the last tick, so a resident caller
    /// reads fields rather than carrying the phase tests (`settle`).
    hurt: [i32; 4],
    bodied: bool,
    shielded: bool,
    shown: bool,
}
impl MossCharger {
    /// `reach` is half the `Attack Range` box's world width less 2 (`X Min`/`X Max`).
    pub fn new(start: [i32; 2], reach: i32, seed: u32) -> Self {
        Self {
            phase: Phase::Hidden,
            timer: 0,
            start,
            reach,
            dir: 1,
            velocity: [0; 2],
            position: start,
            fixed_accumulator: 0,
            ray_clock: 0,
            ray_down: true,
            clip_ticks: 0,
            clip: Clip::Appear,
            rng: seed,
            hurt: [0; 4],
            bodied: false,
            shielded: true,
            shown: false,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn start(&self) -> [i32; 2] {
        self.start
    }
    pub fn dead(&self) -> bool {
        self.phase == Phase::Dead
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.velocity = [0; 2];
        self.bodied = false;
        self.shown = false;
    }
    pub fn clip(&self) -> Clip {
        self.clip
    }
    pub fn clip_ticks(&self) -> u32 {
        self.clip_ticks as u32
    }
    /// +1 facing right (scale +1).
    pub fn dir(&self) -> i32 {
        self.dir as i32
    }
    pub fn velocity(&self) -> [i32; 2] {
        self.velocity
    }
    /// Gravity scale of the body, Q16: 1.5 from `Burst` until `Dig Start`, else 0.
    pub fn gravity_scale(&self) -> i32 {
        match self.phase {
            Phase::Fly | Phase::Air | Phase::GetUp | Phase::Run => AIR_GRAVITY_SCALE_Q16,
            _ => 0,
        }
    }
    /// `SetMeshRenderer` off while hidden.
    #[inline(always)]
    pub fn visible(&self) -> bool {
        self.shown
    }
    fn showing(&self) -> bool {
        !matches!(
            self.phase,
            Phase::Hidden | Phase::EmergePause | Phase::Retry { .. } | Phase::SubmergeCd | Phase::Dead
        )
    }
    /// `HealthManager.invincible`: set from the start, cleared by `In Air`, set again by
    /// `Submerge CD`.
    #[inline(always)]
    pub fn invincible(&self) -> bool {
        self.shielded
    }
    fn shielding(&self) -> bool {
        !matches!(
            self.phase,
            Phase::Air | Phase::GetUp | Phase::Run | Phase::DigStart | Phase::Dig
        )
    }
    /// The collider tk2d has built for the frame showing, relative to the body, or none.
    #[inline(always)]
    pub fn collider(&self) -> Option<[i32; 4]> {
        self.bodied.then_some(self.hurt)
    }
    /// The collider, or `none` when the frame has no box.
    #[inline(always)]
    pub fn collider_or(&self, none: [i32; 4]) -> [i32; 4] {
        if self.bodied {
            self.hurt
        } else {
            none
        }
    }
    fn settle(&mut self) {
        self.shielded = self.shielding();
        self.shown = self.showing();
        match self.frame_box() {
            Some(b) => {
                self.hurt = b;
                self.bodied = true;
            }
            None => self.bodied = false,
        }
    }
    fn frame_box(&self) -> Option<[i32; 4]> {
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
            Phase::Fly | Phase::Air | Phase::GetUp => STUN,
            Phase::Run if frame < 2 => STUN,
            Phase::Run => RUN,
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
            range: self.phase == Phase::Hidden,
            charge_rays: self.phase == Phase::Charge && self.ray_clock.is_multiple_of(CHARGE_RAY_INTERVAL),
            run_rays: self.phase == Phase::Run,
            dig_ray: matches!(self.phase, Phase::DigStart | Phase::Dig),
        }
    }
    fn random(&mut self) -> u32 {
        self.rng = psx_fx::NrLcg::step(self.rng);
        self.rng >> 8
    }
    fn play(&mut self, clip: Clip, step: &mut Step) {
        self.clip = clip;
        self.clip_ticks = 0;
        step.play = Some(clip);
    }
    /// `Decelerate`: each axis toward zero by `amount`, never past it.
    fn decelerate(&mut self, amount: i32) {
        for v in &mut self.velocity {
            *v = if *v > 0 { (*v - amount).max(0) } else { (*v + amount).min(0) };
        }
    }
    /// `BLOCKED HIT`: only while invincible and bodied (a hidden tuft has no collider).
    /// `cardinal` is the attack's direction (`GetAttackDirection`: 0 right, 1 up, 2
    /// left, 3 down). `Line Loop` -> `State 2` -> `GotoPreviousState` runs `Line Loop`
    /// again until `Looper` passes 11, all inside this frame, so every blocked hit ends
    /// in `Burst`.
    pub fn blocked_hit(&mut self, cardinal: u8) -> Step {
        let mut step = Step::default();
        if !self.invincible() || self.collider().is_none() || self.phase == Phase::Dead {
            return step;
        }
        // `Burst`: gravity 1.5, `Stun`, then `Fly *` by the cardinal.
        self.velocity = BURST_FLING[(cardinal & 3) as usize];
        self.phase = Phase::Fly;
        step.sound = Some(Sound::Burst);
        self.play(Clip::Stun, &mut step);
        self.settle();
        step
    }
    fn hide(&mut self, step: &mut Step) {
        self.phase = Phase::SubmergeCd;
        self.timer = SUBMERGE_CD_TICKS;
        self.velocity = [0; 2];
        self.position = self.start;
        step.teleport = Some(self.start);
    }
    /// `Emerge Right` (`right`, charging left) or `Emerge Left`: the side is out of
    /// reach and the other one is tried next frame, or `Emerge`.
    fn emerge(&mut self, right: bool, hero_x: i32, step: &mut Step) {
        let x = if right { hero_x + APPEAR_DISTANCE } else { hero_x - APPEAR_DISTANCE };
        let out = if right { x > self.start[0] + self.reach } else { x < self.start[0] - self.reach };
        if out {
            self.phase = Phase::Retry { right: !right };
            return;
        }
        // Emerging on the right means charging left.
        self.dir = if right { -1 } else { 1 };
        self.position = [x, self.start[1]];
        self.appear(step);
    }
    /// `Emerge`, also its re-entry after a blocked hit (`SetPosition` again).
    fn appear(&mut self, step: &mut Step) {
        step.teleport = Some(self.position);
        self.velocity = [self.dir as i32 * EMERGE_SPEED, 0];
        self.phase = Phase::Appear;
        self.timer = APPEAR_TICKS;
        step.sound = Some(Sound::Emerge);
        self.play(Clip::Appear, step);
    }
    fn charge(&mut self, step: &mut Step) {
        self.phase = Phase::Charge;
        self.velocity = [self.dir as i32 * CHARGE_SPEED, 0];
        self.ray_clock = 0;
        step.sound = Some(Sound::LoopOn);
        self.play(Clip::Charge, step);
    }
    fn submerge(&mut self, step: &mut Step) {
        self.phase = Phase::Submerge;
        self.timer = DISAPPEAR_TRIGGER_TICKS;
        self.decelerate(SUBMERGE_DECELERATION);
        step.sound = Some(Sound::Submerge);
        self.play(Clip::Disappear, step);
    }
    /// `Direction` -> `Run L`/`Run R`: away from the Knight, `TurnRun` from frame 0
    /// (`Run *`'s `Tk2dPlayFrame` 0 overrides `Direction`'s frame 2 in the same frame).
    fn run(&mut self, hero_x: i32, step: &mut Step) {
        self.phase = Phase::Run;
        self.timer = RUN_TICKS;
        // CheckTargetDirection: the Knight left (or level) runs right.
        self.dir = if hero_x <= self.position[0] { 1 } else { -1 };
        self.play(Clip::TurnRun, step);
    }
    fn dig_start(&mut self, step: &mut Step) {
        step.sound = Some(Sound::LoopOff);
        self.phase = Phase::DigStart;
        self.timer = ESCAPE_TRIGGER_TICKS;
        self.decelerate(DIG_DECELERATION);
        self.play(Clip::Escape, step);
    }
    /// `FALL` from `Dig Start`/`Dig`: `In Air` again.
    fn fall(&mut self, step: &mut Step) {
        self.phase = Phase::Air;
        self.play(Clip::Stun, step);
    }
    /// One nominal 60 Hz frame. The caller writes any `teleport` first, then moves the
    /// body by `velocity` and reports where it ended up through `moved_to`.
    pub fn tick(&mut self, senses: Senses) -> Step {
        let step = self.advance(senses);
        self.settle();
        step
    }
    fn advance(&mut self, senses: Senses) -> Step {
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
                    let span = (EMERGE_PAUSE_TICKS[1] - EMERGE_PAUSE_TICKS[0]) as u32 + 1;
                    self.timer = EMERGE_PAUSE_TICKS[0]
                        + psx_math::WideU::mul(self.random(), span).shr_u32(24) as u16;
                }
            }
            Phase::EmergePause => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    // `Hero Beyond?`: past `X Max` or below `X Min` is CANCEL.
                    let off = senses.hero_x - self.start[0];
                    if off > self.reach || off < -self.reach {
                        self.phase = Phase::Hidden;
                    } else {
                        // `Left or Right?`: SendRandomEvent, even weights.
                        let right = self.random() & 0x8000 == 0;
                        self.emerge(right, senses.hero_x, &mut step);
                    }
                }
            }
            Phase::Retry { right } => self.emerge(right, senses.hero_x, &mut step),
            Phase::Appear => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    self.charge(&mut step);
                }
            }
            Phase::Charge => {
                if self.ray_clock.is_multiple_of(CHARGE_RAY_INTERVAL) {
                    self.ray_down = senses.ground_hit;
                    if senses.forward_hit {
                        self.submerge(&mut step);
                        return step;
                    }
                }
                self.ray_clock = self.ray_clock.wrapping_add(1);
                if !self.ray_down {
                    self.submerge(&mut step);
                }
            }
            Phase::Submerge | Phase::SubmergeGrass => {
                if fixed {
                    self.decelerate(SUBMERGE_DECELERATION);
                }
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    if self.phase == Phase::Submerge {
                        self.phase = Phase::SubmergeGrass;
                        self.timer = DISAPPEAR_TICKS - DISAPPEAR_TRIGGER_TICKS;
                    } else {
                        self.hide(&mut step);
                    }
                }
            }
            Phase::SubmergeCd => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    // `Play Range` -> `Hidden`.
                    self.phase = Phase::Hidden;
                }
            }
            Phase::Fly => {
                // `In Air` restarts `Stun` and switches the collider on.
                self.phase = Phase::Air;
                self.play(Clip::Stun, &mut step);
            }
            Phase::Air => {
                if senses.floor {
                    // `Land` zeroes the x velocity; `Get Up` follows.
                    self.velocity[0] = 0;
                    self.phase = Phase::GetUp;
                    self.timer = GET_UP_TICKS;
                    self.play(Clip::GetUp, &mut step);
                }
            }
            Phase::GetUp => {
                self.timer = self.timer.saturating_sub(1);
                if self.timer == 0 {
                    step.sound = Some(Sound::LoopOn);
                    self.run(senses.hero_x, &mut step);
                }
            }
            Phase::Run => {
                // `CheckTargetDirection` every frame: the Knight on the side it runs toward
                // turns it (the other `Run` state re-enters: clip, wait and all).
                let toward = if self.dir > 0 {
                    senses.hero_x > self.position[0]
                } else {
                    senses.hero_x < self.position[0]
                };
                if toward {
                    self.run(senses.hero_x, &mut step);
                    return step;
                }
                if fixed {
                    let v = self.velocity[0] + self.dir as i32 * RUN_ACCELERATION;
                    self.velocity[0] = v.clamp(-RUN_MAX, RUN_MAX);
                }
                self.timer = self.timer.saturating_sub(1);
                if senses.forward_hit || !senses.ground_hit || self.timer == 0 {
                    // `On Ground?` -> `Dig Start`.
                    self.dig_start(&mut step);
                }
            }
            Phase::DigStart | Phase::Dig => {
                if !senses.dig_ground {
                    self.fall(&mut step);
                    return step;
                }
                if self.phase == Phase::DigStart {
                    if fixed {
                        self.decelerate(DIG_DECELERATION);
                    }
                    self.timer = self.timer.saturating_sub(1);
                    if self.timer == 0 {
                        // `Dig`: x stops, the grass and the dig sound.
                        self.phase = Phase::Dig;
                        self.velocity[0] = 0;
                        self.timer = ESCAPE_TICKS - ESCAPE_TRIGGER_TICKS;
                        step.sound = Some(Sound::Dig);
                    }
                } else {
                    self.timer = self.timer.saturating_sub(1);
                    if self.timer == 0 {
                        self.hide(&mut step);
                    }
                }
            }
            Phase::Dead => {}
        }
        step
    }
    /// Where the body ended up after the caller moved it.
    pub fn moved_to(&mut self, position: [i32; 2], velocity: [i32; 2]) {
        self.position = position;
        self.velocity = velocity;
    }
}
/// Ticks one frame of a clip lasts at its authored rate (Charge 15 fps, Get Up 18, the
/// rest 12; Get Up's 3.3 rounds down, which only shifts its collider frame).
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
    /// The 28-wide box at scale 1.175: half 16.45, less 2.
    const REACH: i32 = 947_323;
    const CLEAR: Senses = Senses {
        hero_x: 0,
        in_range: false,
        forward_hit: false,
        ground_hit: true,
        dig_ground: true,
        floor: false,
    };
    fn near(hero_x: i32) -> Senses {
        Senses { hero_x, in_range: true, ..CLEAR }
    }
    fn run_until(c: &mut MossCharger, phase: Phase, senses: Senses, limit: u32) -> u32 {
        for n in 0..limit {
            if c.phase() == phase {
                return n;
            }
            let s = c.tick(senses);
            if let Some(p) = s.teleport {
                c.moved_to(p, c.velocity());
            }
        }
        panic!("never reached {phase:?}, at {:?}", c.phase());
    }

    #[test]
    fn hides_until_the_knight_is_in_range_then_appears_fourteen_units_to_a_side() {
        for seed in 0..16 {
            let mut c = MossCharger::new(START, REACH, seed);
            for _ in 0..100 {
                c.tick(CLEAR);
            }
            assert_eq!(c.phase(), Phase::Hidden);
            assert!(!c.visible() && c.collider().is_none());
            let hero = START[0];
            c.tick(near(hero));
            let t = run_until(&mut c, Phase::Appear, near(hero), 80);
            assert!((29..=61).contains(&t), "Emerge Pause 0.5..1 s: {t}");
            assert!(c.visible() && c.collider().is_some() && c.invincible());
            let side = c.position[0] - hero;
            assert_eq!(side.abs(), APPEAR_DISTANCE);
            // Right of the Knight charges left, at a quarter speed while it appears.
            assert_eq!(c.velocity()[0], -side.signum() * EMERGE_SPEED);
            assert_eq!(run_until(&mut c, Phase::Charge, near(hero), 40), APPEAR_TICKS as u32);
            assert_eq!(c.velocity()[0], -side.signum() * CHARGE_SPEED);
        }
    }
    #[test]
    fn a_knight_past_the_reach_cancels_and_a_cramped_side_swaps() {
        let mut c = MossCharger::new(START, REACH, 3);
        c.tick(near(START[0] + REACH + 1));
        run_until(&mut c, Phase::Hidden, near(START[0] + REACH + 1), 80);
        // Near the right end only the left side fits: it always comes up left and charges right.
        for seed in 0..8 {
            let mut c = MossCharger::new(START, REACH, seed);
            let hero = START[0] + REACH - ONE;
            c.tick(near(hero));
            run_until(&mut c, Phase::Appear, near(hero), 80);
            assert_eq!((c.position[0], c.dir()), (hero - APPEAR_DISTANCE, 1));
        }
    }
    #[test]
    fn a_wall_or_a_gap_ends_the_charge_and_it_sinks_back_into_its_tuft() {
        let mut c = MossCharger::new(START, REACH, 1);
        c.tick(near(START[0]));
        run_until(&mut c, Phase::Charge, near(START[0]), 200);
        c.tick(Senses { forward_hit: true, ..near(START[0]) });
        assert_eq!(c.phase(), Phase::Submerge);
        assert_eq!(c.clip(), Clip::Disappear);
        assert_eq!(run_until(&mut c, Phase::SubmergeGrass, CLEAR, 40), DISAPPEAR_TRIGGER_TICKS as u32);
        assert_eq!(
            run_until(&mut c, Phase::SubmergeCd, CLEAR, 60),
            (DISAPPEAR_TICKS - DISAPPEAR_TRIGGER_TICKS) as u32
        );
        assert_eq!((c.position, c.velocity()), (START, [0, 0]));
        assert_eq!(run_until(&mut c, Phase::Hidden, CLEAR, 40), SUBMERGE_CD_TICKS as u32);
        // A gap: the ground ray (cast every second frame) misses.
        let mut c = MossCharger::new(START, REACH, 2);
        c.tick(near(START[0]));
        run_until(&mut c, Phase::Charge, near(START[0]), 200);
        c.tick(near(START[0]));
        c.tick(Senses { ground_hit: false, ..near(START[0]) });
        assert_eq!(c.phase(), Phase::Charge, "the ray is not cast on odd frames");
        c.tick(Senses { ground_hit: false, ..near(START[0]) });
        assert_eq!(c.phase(), Phase::Submerge);
    }
    #[test]
    fn one_blocked_hit_bursts_it_out_then_it_lands_runs_and_digs_in() {
        let mut c = MossCharger::new(START, REACH, 4);
        c.tick(near(START[0]));
        run_until(&mut c, Phase::Charge, near(START[0]), 200);
        // A hidden tuft cannot be hit.
        let mut hidden = MossCharger::new(START, REACH, 4);
        assert_eq!(hidden.blocked_hit(0), Step::default());
        let step = c.blocked_hit(0);
        assert_eq!((c.phase(), step.play, c.velocity()), (Phase::Fly, Some(Clip::Stun), BURST_FLING[0]));
        assert_eq!(c.gravity_scale(), AIR_GRAVITY_SCALE_Q16);
        c.tick(CLEAR);
        assert_eq!(c.phase(), Phase::Air);
        assert!(!c.invincible());
        c.tick(Senses { floor: true, ..CLEAR });
        assert_eq!((c.phase(), c.velocity()[0]), (Phase::GetUp, 0));
        let hero = c.position[0] + 5 * ONE;
        assert_eq!(run_until(&mut c, Phase::Run, Senses { hero_x: hero, ..CLEAR }, 30), GET_UP_TICKS as u32);
        assert_eq!((c.dir(), c.clip()), (-1, Clip::TurnRun), "it runs from the Knight");
        let ran = run_until(&mut c, Phase::DigStart, Senses { hero_x: hero, ..CLEAR }, 100);
        assert_eq!(ran, RUN_TICKS as u32);
        assert!(c.velocity()[0] < 0 && c.velocity()[0] >= -RUN_MAX);
        assert_eq!(run_until(&mut c, Phase::Dig, CLEAR, 40), ESCAPE_TRIGGER_TICKS as u32);
        assert_eq!(c.velocity()[0], 0);
        assert_eq!(run_until(&mut c, Phase::SubmergeCd, CLEAR, 60), (ESCAPE_TICKS - ESCAPE_TRIGGER_TICKS) as u32);
        assert!(c.invincible());
        // The next charge bursts on its first blocked hit too.
        run_until(&mut c, Phase::Hidden, CLEAR, 40);
        c.tick(near(START[0]));
        run_until(&mut c, Phase::Charge, near(START[0]), 200);
        c.blocked_hit(1);
        assert_eq!((c.phase(), c.velocity()), (Phase::Fly, BURST_FLING[1]));
    }
    #[test]
    fn running_toward_the_knight_turns_it_and_a_wall_or_ledge_ends_the_run() {
        let mut c = MossCharger::new(START, REACH, 5);
        c.tick(near(START[0]));
        run_until(&mut c, Phase::Charge, near(START[0]), 200);
        c.blocked_hit(2);
        c.tick(CLEAR);
        c.tick(Senses { floor: true, ..CLEAR });
        let left = Senses { hero_x: c.position[0] - ONE, ..CLEAR };
        run_until(&mut c, Phase::Run, left, 30);
        assert_eq!(c.dir(), 1);
        for _ in 0..10 {
            c.tick(left);
        }
        // The Knight crosses to its right: it turns and the second flees left.
        c.tick(Senses { hero_x: c.position[0] + ONE, ..CLEAR });
        assert_eq!((c.dir(), c.clip_ticks()), (-1, 0));
        c.tick(Senses { ground_hit: false, hero_x: c.position[0] + ONE, ..CLEAR });
        assert_eq!(c.phase(), Phase::DigStart);
        // The dig finds no ground: `FALL` to `In Air`.
        c.tick(Senses { dig_ground: false, ..CLEAR });
        assert_eq!(c.phase(), Phase::Air);
    }
    #[test]
    fn colliders_follow_the_frame_and_mirror_with_the_facing() {
        let mut c = MossCharger::new(START, REACH, 6);
        c.tick(near(START[0]));
        run_until(&mut c, Phase::Appear, near(START[0]), 200);
        let b = c.collider().unwrap();
        if c.dir() > 0 {
            assert_eq!(b, BIG);
        } else {
            assert_eq!(b, [-BIG[2], BIG[1], -BIG[0], BIG[3]]);
        }
        run_until(&mut c, Phase::Charge, near(START[0]), 40);
        c.tick(Senses { forward_hit: true, ..near(START[0]) });
        let mut seen = 0;
        for _ in 0..60 {
            if c.collider().is_none() {
                seen += 1;
            }
            c.tick(CLEAR);
        }
        assert!(seen >= 30, "the last Disappear frames carry no box: {seen}");
    }
}
