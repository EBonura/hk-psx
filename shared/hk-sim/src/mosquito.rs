//! Source-derived Mosquito (Squit) `Mozzie` FSM: IdleBuzz roaming, alert,
//! Startle, DistanceFly hover 8 units off and 1 above the Knight, and within
//! 10 units a pause, a wind-up and a straight 18 unit/s lunge that rebounds
//! off terrain. Evidence: hk-enemies scratch/research/mosquito.md (FSM dump
//! of level130/134/135/136/146 and Assembly-CSharp IL).
//!
//! The caller owns the gravity-free body against terrain, the alert circle
//! and its raycast, hits, Recoil displacement, corpse, Geo and every draw.
//! `tick` is one nominal 60 Hz frame; the source 50 Hz FixedUpdate steps run
//! from an accumulator so the per-step constants stay as authored, and an
//! action's OnEnter call (DistanceFly and IdleBuzz both DoBuzz on enter) is
//! made where the state is entered. Random.Range is a deterministic
//! per-actor sequence.
use crate::buzz::{distance_fly_height, IdleBuzz};
use crate::ONE;

/// `Alert Range New`: circle 0.4111 at scale 21.116, 8.6808 units (Q16).
pub const ALERT_RADIUS: i32 = 568902;
/// IdleBuzz(waitMin .75, waitMax 1, speedMax 3, accelerationMax 19, range 1).
pub const IDLE_SPEED_MAX: i32 = 3 * ONE;
pub const IDLE_ACCELERATION_MAX: i32 = 19 * ONE;
/// DistanceFly(distance 8, speedMax 5.5, acceleration .1, height 1).
pub const HOVER_DISTANCE: i32 = 8 * ONE;
pub const HOVER_HEIGHT: i32 = ONE;
pub const HOVER_SPEED_MAX: i32 = 5 * ONE + ONE / 2;
pub const HOVER_ACCELERATION: i32 = 6554;
/// `Chase - In Sight`'s FloatCompare: ATTACK at or under 10 units.
pub const ATTACK_RANGE: i32 = 10 * ONE;
/// `Attention Span` 8 s.
pub const ATTENTION_TICKS: u16 = 480;
/// Startle: four frames at 12 fps.
pub const STARTLE_TICKS: u16 = 20;
/// `Attack Pause` WaitRandom 0.25..1 s.
pub const PAUSE_TICKS: [u16; 2] = [15, 60];
/// `Attack Antic` Wait 0.25 s, then `Attack Aim` until the 0.6 s clip ends.
pub const AIM_TICKS: u16 = 15;
pub const ANTIC_TICKS: u16 = 36;
/// SetVelocity2d (0, 1) through the wind-up.
pub const ANTIC_RISE: i32 = ONE;
/// GetAngleToTarget2D offsetY -0.5.
pub const AIM_OFFSET_Y: i32 = -ONE / 2;
pub const LUNGE_SPEED: i32 = 18 * ONE;
/// `Lunge Wait` 1.25 s.
pub const LUNGE_TICKS: u16 = 75;
/// `Pull Out`: velocity *= -0.3, Decelerate 0.2 a step, `Death Air` 0.25 s.
pub const REBOUND: i32 = -19661;
pub const PULL_OUT_DECELERATION: i32 = 13107;
pub const PULL_OUT_TICKS: u16 = 15;
/// `Recover`: DecelerateV2 0.9 on enter and each step, Wait 0.5 s.
pub const RECOVER_DAMPING: i32 = 58982;
pub const RECOVER_TICKS: u16 = 30;
/// `Stop`: Decelerate 0.3 a step, Wait 0.75 s.
pub const STOP_DECELERATION: i32 = 19661;
pub const STOP_TICKS: u16 = 45;
pub const FACE_PAUSE_TICKS: u16 = 30;
/// Recoil.recoilSpeedBase: 20, set to 0 by `Check Dir` and back to 20 only by
/// `Pull Out`, so a lunge that times out leaves the body without knockback.
pub const RECOIL_SPEED: i32 = 20 * ONE;
pub const HEALTH: i16 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Startle,
    Chase,
    AttackPause,
    Antic,
    Lunge,
    PullOut,
    Recover,
    Stop,
    Dead,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Clip {
    Idle,
    TurnToIdle,
    Startle,
    AttackAntic,
    Attack,
    DeathAir,
}
impl Clip {
    pub const COUNT: usize = 6;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// `fluke_fairy_call`, pitch 1.15..1.25.
    Call,
    /// `mosquito_charge_prepare`.
    Prepare,
    /// `mosquito_charge_charge`.
    Charge,
    /// `mosquito_wall_hit`.
    WallHit,
}
/// What one tick asks of the caller beyond the state it can read back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Start this clip from its first frame.
    pub play: Option<Clip>,
    pub sounds: [Option<Sound>; 2],
}
impl Step {
    fn sound(&mut self, sound: Sound) {
        let slot = self
            .sounds
            .iter_mut()
            .find(|s| s.is_none())
            .expect("two sounds a tick");
        *slot = Some(sound);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Senses {
    pub position: [i32; 2],
    pub hero: [i32; 2],
    /// LineOfSightDetector.canSeeHero: inside `Alert Range New` (8.68) and an
    /// unobstructed terrain ray. `In Alert Range` is implied by it.
    pub can_see_hero: bool,
    /// The body met terrain this tick (OnCollisionEnter2D): only read while
    /// lunging.
    pub hit_terrain: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mosquito {
    phase: Phase,
    velocity: [i32; 2],
    buzz: IdleBuzz,
    timer: u16,
    face_pause: u16,
    facing: i8,
    fixed_accumulator: u8,
    clip: Clip,
    /// The lunge direction as a unit vector (Q16), frozen by `Attack Aim`.
    aim: [i32; 2],
    /// Sprite rotation as a unit vector (Q16, (ONE, 0) upright): the lunge's
    /// FaceAngle, reset by SetRotation 0.
    rotation: [i32; 2],
    recoil_off: bool,
    tile_detector: bool,
    rng: u32,
}
impl Mosquito {
    pub fn new(position: [i32; 2], seed: u32) -> Self {
        Self {
            phase: Phase::Idle,
            velocity: [0; 2],
            buzz: IdleBuzz::new(position),
            timer: 0,
            face_pause: 0,
            facing: -1,
            fixed_accumulator: 0,
            clip: Clip::Idle,
            aim: [ONE, 0],
            rotation: [ONE, 0],
            recoil_off: false,
            tile_detector: true,
            rng: seed,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn velocity(&self) -> [i32; 2] {
        self.velocity
    }
    /// +1 right (source scale.x -1), -1 left: the art faces left.
    pub fn facing(&self) -> i32 {
        self.facing as i32
    }
    pub fn clip(&self) -> Clip {
        self.clip
    }
    pub fn rotation(&self) -> [i32; 2] {
        self.rotation
    }
    /// `TileDetector`, the second body box, until the first wind-up.
    pub fn tile_detector(&self) -> bool {
        self.tile_detector
    }
    pub fn recoil_speed(&self) -> i32 {
        if self.recoil_off {
            0
        } else {
            RECOIL_SPEED
        }
    }
    pub fn dead(&self) -> bool {
        self.phase == Phase::Dead
    }
    pub fn die(&mut self) {
        self.phase = Phase::Dead;
        self.velocity = [0; 2];
    }
    fn random(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng >> 8
    }
    fn range(&mut self, low: i32, high: i32) -> i32 {
        let span = (high - low) as i64;
        low + ((self.random() as i64 * span) >> 24) as i32
    }
    fn play(&mut self, clip: Clip, step: &mut Step) {
        self.clip = clip;
        step.play = Some(clip);
    }
    fn idle_buzz(&mut self, position: [i32; 2]) {
        let mut v = self.velocity;
        let mut buzz = self.buzz;
        buzz.step_with(
            position,
            &mut v,
            IDLE_SPEED_MAX,
            IDLE_ACCELERATION_MAX,
            &mut |lo, hi| self.range(lo, hi),
        );
        self.buzz = buzz;
        self.velocity = v;
    }
    fn hover(&mut self, senses: Senses) {
        distance_fly_height(
            senses.position,
            senses.hero,
            HOVER_DISTANCE,
            HOVER_HEIGHT,
            HOVER_SPEED_MAX,
            HOVER_ACCELERATION,
            &mut self.velocity,
        );
    }
    /// FaceObject toward the hero: strictly left of it faces right.
    fn face_hero(&mut self, senses: Senses, clip: bool, step: &mut Step) {
        let want = if senses.position[0] < senses.hero[0] {
            1
        } else {
            -1
        };
        if want != self.facing {
            self.facing = want;
            if clip {
                self.play(Clip::TurnToIdle, step);
            }
        }
    }
    /// FaceDirection with a pause: face the velocity's x sign.
    fn face_velocity(&mut self, step: &mut Step) {
        if self.face_pause > 0 {
            self.face_pause -= 1;
            return;
        }
        let want = if self.velocity[0] > 0 { 1 } else { -1 };
        if want != self.facing {
            self.facing = want;
            self.face_pause = FACE_PAUSE_TICKS;
            self.play(Clip::TurnToIdle, step);
        }
    }
    fn decelerate(&mut self, by: i32) {
        for v in self.velocity.iter_mut() {
            *v = v.signum() * (v.abs() - by).max(0);
        }
    }
    fn damp(&mut self) {
        for v in self.velocity.iter_mut() {
            *v = ((*v as i64 * RECOVER_DAMPING as i64) >> 16) as i32;
        }
    }
    /// `Chase - In Sight` entered: Idle (cutting a turn), FaceObject with
    /// TurnToIdle, DistanceFly's enter call, and the one 10-unit check.
    fn in_sight(&mut self, senses: Senses, step: &mut Step) {
        self.phase = Phase::Chase;
        self.timer = ATTENTION_TICKS;
        self.rotation = [ONE, 0];
        if self.clip != Clip::Idle {
            self.play(Clip::Idle, step);
        }
        self.face_hero(senses, true, step);
        self.hover(senses);
        let dx = (senses.hero[0] as i64 - senses.position[0] as i64) >> 8;
        let dy = (senses.hero[1] as i64 - senses.position[1] as i64) >> 8;
        let r = (ATTACK_RANGE >> 8) as i64;
        if dx * dx + dy * dy <= r * r {
            self.phase = Phase::AttackPause;
            self.timer =
                (self.range(PAUSE_TICKS[0] as i32 * ONE, PAUSE_TICKS[1] as i32 * ONE) >> 16) as u16;
            self.hover(senses);
        }
    }
    fn startle(&mut self, senses: Senses, step: &mut Step) {
        self.phase = Phase::Startle;
        self.timer = STARTLE_TICKS;
        self.play(Clip::Startle, step);
        step.sound(Sound::Call);
        step.sound(Sound::Prepare);
        self.facing = if senses.position[0] < senses.hero[0] {
            1
        } else {
            -1
        };
    }
    fn lunge(&mut self, step: &mut Step) {
        // `Check Dir`: [0, 90) and (270, 360) lunge right, [90, 270] left,
        // which for the aim vector is x > 0.
        self.phase = Phase::Lunge;
        self.timer = LUNGE_TICKS;
        self.recoil_off = true;
        step.sound(Sound::Charge);
        self.facing = if self.aim[0] > 0 { 1 } else { -1 };
        self.velocity = [
            ((self.aim[0] as i64 * LUNGE_SPEED as i64) >> 16) as i32,
            ((self.aim[1] as i64 * LUNGE_SPEED as i64) >> 16) as i32,
        ];
        // FaceAngle: the velocity's angle, plus 180 for the left-facing art.
        self.rotation = if self.facing > 0 {
            self.aim
        } else {
            [-self.aim[0], -self.aim[1]]
        };
        self.play(Clip::Attack, step);
    }
    /// One nominal 60 Hz frame.
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
        match self.phase {
            Phase::Idle => {
                if senses.can_see_hero {
                    self.startle(senses, &mut step);
                    return step;
                }
                if fixed {
                    self.idle_buzz(senses.position);
                }
                self.face_velocity(&mut step);
            }
            Phase::Startle => {
                self.timer -= 1;
                if self.timer == 0 {
                    // `Chase Start` (pitch 1.2) finishes into In Sight.
                    self.in_sight(senses, &mut step);
                }
            }
            Phase::Chase => {
                if senses.can_see_hero {
                    // In Sight's NextFrameEvent enters Out of Sight, whose
                    // BoolAllTrue sends ALERT on enter: both DistanceFly
                    // enter calls and the range check run every frame.
                    self.hover(senses);
                    self.in_sight(senses, &mut step);
                } else {
                    self.timer = self.timer.saturating_sub(1);
                    if self.timer == 0 {
                        self.phase = Phase::Stop;
                        self.timer = STOP_TICKS;
                        if self.clip != Clip::Idle {
                            self.play(Clip::Idle, &mut step);
                        }
                        self.decelerate(STOP_DECELERATION);
                        return step;
                    }
                    self.face_hero(senses, true, &mut step);
                }
                if fixed && self.phase == Phase::Chase {
                    self.hover(senses);
                }
            }
            Phase::AttackPause => {
                if fixed {
                    self.hover(senses);
                }
                self.face_hero(senses, false, &mut step);
                self.timer -= 1;
                if self.timer == 0 {
                    // `Still In Range?`: CANCEL back to In Sight.
                    if !senses.can_see_hero {
                        self.in_sight(senses, &mut step);
                    } else {
                        self.phase = Phase::Antic;
                        self.timer = 0;
                        self.tile_detector = false;
                        self.velocity = [0, ANTIC_RISE];
                        step.sound(Sound::Prepare);
                        self.face_hero(senses, false, &mut step);
                        self.play(Clip::AttackAntic, &mut step);
                    }
                }
            }
            Phase::Antic => {
                self.timer += 1;
                if self.timer == AIM_TICKS {
                    // GetAngleToTarget2D toward the hero, half a unit low.
                    let dx = senses.hero[0] as i64 - senses.position[0] as i64;
                    let dy =
                        senses.hero[1] as i64 + AIM_OFFSET_Y as i64 - senses.position[1] as i64;
                    let length = isqrt(dx * dx + dy * dy).max(1);
                    self.aim = [
                        (dx * ONE as i64 / length) as i32,
                        (dy * ONE as i64 / length) as i32,
                    ];
                    if dx == 0 && dy == 0 {
                        self.aim = [ONE, 0];
                    }
                }
                if self.timer >= ANTIC_TICKS {
                    self.lunge(&mut step);
                }
            }
            Phase::Lunge => {
                if senses.hit_terrain {
                    // `Pull Out`: rebound at 30 %, knockback back on.
                    self.phase = Phase::PullOut;
                    self.timer = PULL_OUT_TICKS;
                    self.recoil_off = false;
                    self.rotation = [ONE, 0];
                    step.sound(Sound::WallHit);
                    self.velocity = [
                        ((self.velocity[0] as i64 * REBOUND as i64) >> 16) as i32,
                        ((self.velocity[1] as i64 * REBOUND as i64) >> 16) as i32,
                    ];
                    self.decelerate(PULL_OUT_DECELERATION);
                    self.play(Clip::DeathAir, &mut step);
                    return step;
                }
                self.timer -= 1;
                if self.timer == 0 {
                    self.recover(&mut step);
                }
            }
            Phase::PullOut => {
                if fixed {
                    self.decelerate(PULL_OUT_DECELERATION);
                }
                self.timer -= 1;
                if self.timer == 0 {
                    self.recover(&mut step);
                }
            }
            Phase::Recover => {
                if fixed {
                    self.damp();
                }
                self.timer -= 1;
                if self.timer == 0 {
                    self.in_sight(senses, &mut step);
                }
            }
            Phase::Stop => {
                if fixed {
                    self.decelerate(STOP_DECELERATION);
                }
                self.timer -= 1;
                if self.timer == 0 {
                    self.phase = Phase::Idle;
                    self.buzz.enter(senses.position);
                    self.idle_buzz(senses.position);
                    self.play(Clip::Idle, &mut step);
                }
            }
            Phase::Dead => {}
        }
        step
    }
    fn recover(&mut self, step: &mut Step) {
        self.phase = Phase::Recover;
        self.timer = RECOVER_TICKS;
        self.rotation = [ONE, 0];
        self.damp();
        self.play(Clip::Idle, step);
    }
}
/// Integer square root (bitwise), no floating point on the guest.
fn isqrt(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let (mut x, mut bit, mut v) = (0i64, 1i64 << 62, v);
    while bit > v {
        bit >>= 2;
    }
    while bit != 0 {
        if v >= x + bit {
            v -= x + bit;
            x = (x >> 1) + bit;
        } else {
            x >>= 1;
        }
        bit >>= 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn senses(position: [i32; 2], hero: [i32; 2], see: bool) -> Senses {
        Senses {
            position,
            hero,
            can_see_hero: see,
            hit_terrain: false,
        }
    }
    /// Startle, then hover: a mosquito that sees the hero within 10 units
    /// winds up and lunges at it at 18 units a second.
    #[test]
    fn it_startles_winds_up_rising_and_lunges_at_the_hero() {
        let mut m = Mosquito::new([0, 0], 7);
        let hero = [6 * ONE, -2 * ONE];
        let first = m.tick(senses([0, 0], hero, true));
        assert_eq!(m.phase(), Phase::Startle);
        assert_eq!(first.play, Some(Clip::Startle));
        assert_eq!(first.sounds, [Some(Sound::Call), Some(Sound::Prepare)]);
        assert_eq!(m.facing(), 1);
        let mut t = 0;
        while m.phase() != Phase::Antic {
            m.tick(senses([0, 0], hero, true));
            t += 1;
            assert!(
                t < STARTLE_TICKS as u32 + PAUSE_TICKS[1] as u32 + 2,
                "attack pause too long"
            );
        }
        assert!(!m.tile_detector(), "Attack Antic deactivates TileDetector");
        assert_eq!(m.velocity(), [0, ANTIC_RISE]);
        for _ in 0..ANTIC_TICKS - 1 {
            m.tick(senses([0, 0], hero, true));
            assert_eq!(m.phase(), Phase::Antic);
        }
        let step = m.tick(senses([0, 0], hero, true));
        assert_eq!(m.phase(), Phase::Lunge);
        assert_eq!(step.play, Some(Clip::Attack));
        assert_eq!(step.sounds[0], Some(Sound::Charge));
        assert_eq!(m.recoil_speed(), 0, "Check Dir zeroes the knockback");
        let v = m.velocity();
        let speed = ((v[0] as i64 * v[0] as i64 + v[1] as i64 * v[1] as i64) as f64).sqrt();
        assert!((speed - LUNGE_SPEED as f64).abs() < 64.0, "{speed}");
        // Aimed half a unit below the hero: (6, -2.5) from the body.
        assert!(
            v[0] > 0 && v[1] < 0 && (v[1] as i64 * 6 * 2 + v[0] as i64 * 5).abs() < 2 * ONE as i64
        );
    }
    #[test]
    fn a_wall_rebounds_it_at_thirty_percent_and_restores_knockback() {
        let mut m = Mosquito::new([0, 0], 3);
        let hero = [-5 * ONE, 0];
        while m.phase() != Phase::Lunge {
            m.tick(senses([0, 0], hero, true));
        }
        let before = m.velocity();
        let step = m.tick(Senses {
            hit_terrain: true,
            ..senses([0, 0], hero, true)
        });
        assert_eq!(m.phase(), Phase::PullOut);
        assert_eq!(step.play, Some(Clip::DeathAir));
        assert_eq!(step.sounds[0], Some(Sound::WallHit));
        assert_eq!(m.recoil_speed(), RECOIL_SPEED);
        let after = m.velocity();
        assert!(
            after[0] > 0
                && (after[0] - (-(before[0] as i64 * 3 / 10) as i32 - PULL_OUT_DECELERATION)).abs()
                    < 64
        );
        for _ in 0..PULL_OUT_TICKS + RECOVER_TICKS {
            m.tick(senses([0, 0], hero, true));
        }
        assert!(
            matches!(m.phase(), Phase::Chase | Phase::AttackPause),
            "back to In Sight: {:?}",
            m.phase()
        );
    }
    #[test]
    fn a_lunge_that_hits_nothing_times_out_without_knockback() {
        let mut m = Mosquito::new([0, 0], 11);
        let hero = [3 * ONE, 0];
        while m.phase() != Phase::Lunge {
            m.tick(senses([0, 0], hero, true));
        }
        for _ in 0..LUNGE_TICKS {
            m.tick(senses([0, 0], hero, false));
        }
        assert_eq!(m.phase(), Phase::Recover);
        assert_eq!(
            m.recoil_speed(),
            0,
            "only Pull Out restores recoilSpeedBase"
        );
    }
    #[test]
    fn out_of_sight_for_its_attention_span_it_stops_and_idles() {
        let mut m = Mosquito::new([0, 0], 5);
        // Seen far away: startle, then hover without attacking.
        let hero = [30 * ONE, 0];
        m.tick(senses([0, 0], hero, true));
        for _ in 0..STARTLE_TICKS {
            m.tick(senses([0, 0], hero, true));
        }
        assert_eq!(m.phase(), Phase::Chase);
        for _ in 0..ATTENTION_TICKS {
            m.tick(senses([0, 0], hero, false));
        }
        assert_eq!(m.phase(), Phase::Stop);
        for _ in 0..STOP_TICKS {
            m.tick(senses([0, 0], hero, false));
        }
        assert_eq!(m.phase(), Phase::Idle);
    }
    #[test]
    fn integer_square_root_is_exact() {
        for v in [
            0i64,
            1,
            2,
            3,
            4,
            15,
            16,
            17,
            1 << 40,
            (1 << 40) + 12345,
            4294967296 * 9,
        ] {
            let r = isqrt(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "{v}");
        }
    }
}
